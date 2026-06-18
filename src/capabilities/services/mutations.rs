use super::{Mutation, MGR_IFACE, MGR_PATH};
use crate::capabilities::{CapabilityContext, View};
use crate::cli::Cli;
use crate::error::{FezError, Result};
use serde_json::{json, Value};
use std::io::IsTerminal;

impl Mutation {
    fn verb(&self) -> &'static str {
        match self {
            Mutation::Start => "start",
            Mutation::Stop => "stop",
            Mutation::Restart => "restart",
            Mutation::Reload => "reload",
            Mutation::Enable { .. } => "enable",
            Mutation::Disable { .. } => "disable",
        }
    }

    fn is_destructive(&self) -> bool {
        matches!(
            self,
            Mutation::Stop | Mutation::Restart | Mutation::Disable { .. }
        )
    }

    fn now_suffix(&self) -> &'static str {
        match self {
            Mutation::Enable { now: true } | Mutation::Disable { now: true } => " --now",
            _ => "",
        }
    }

    fn past(&self) -> &'static str {
        match self {
            Mutation::Start => "started",
            Mutation::Stop => "stopped",
            Mutation::Restart => "restarted",
            Mutation::Reload => "reloaded",
            Mutation::Enable { .. } => "enabled",
            Mutation::Disable { .. } => "disabled",
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Mutation::Enable { .. } | Mutation::Disable { .. } => "ServiceEnablement",
            _ => "ServiceMutation",
        }
    }

    /// The inverse invocation, for the reversibility hint (Section 8, layer 5).
    fn reverse_cmd(&self, unit: &str) -> Option<String> {
        match self {
            Mutation::Start => Some(format!("fez services stop {unit}")),
            Mutation::Stop => Some(format!("fez services start {unit}")),
            Mutation::Enable { .. } => {
                Some(format!("fez services disable {unit}{}", self.now_suffix()))
            }
            Mutation::Disable { .. } => {
                Some(format!("fez services enable {unit}{}", self.now_suffix()))
            }
            Mutation::Restart | Mutation::Reload => None,
        }
    }
}

pub(super) fn run(cli: &Cli, m: Mutation, unit: &str) -> Result<View> {
    let unit = super::mangle_unit(unit);
    let unit = unit.as_ref();
    let host = cli.resolved_host();

    // Layer 3: protected-unit policy — before anything privileged.
    crate::safety::check_protected(unit, cli.force)?;

    // Layer 2: dry-run short-circuits before connecting (no side effects).
    if cli.dry_run {
        return Ok(dry_run_view(&m, &host, unit));
    }

    // Layer 6: TTY-gated confirmation (humans only; agents are non-TTY).
    let is_tty = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    if crate::safety::should_prompt(m.is_destructive(), is_tty, cli.force) {
        confirm(&m, &host, unit)?;
    }

    // Layer 4: structured audit — attempt, execute, then result.
    crate::audit::run_audited(&host, m.verb(), unit, || execute(cli, &m, &host, unit))
}

fn dry_run_view(m: &Mutation, host: &str, unit: &str) -> View {
    let command = format!("fez services {} {}{}", m.verb(), unit, m.now_suffix());
    let human = format!(
        "DRY-RUN: would {} {} on {} (requires elevation)\n",
        m.verb(),
        unit,
        host
    );
    View::new(
        "DryRun",
        host.to_string(),
        json!({
            "operation": m.verb(),
            "unit": unit,
            "host": host,
            "privileged": true,
            "command": command,
        }),
        human,
    )
}

fn confirm(m: &Mutation, host: &str, unit: &str) -> Result<()> {
    use std::io::Write;
    eprint!(
        "About to {} {} on {}. Proceed? [y/N] ",
        m.verb(),
        unit,
        host
    );
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(FezError::Io)?;
    match line.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => Ok(()),
        _ => Err(FezError::Aborted),
    }
}

fn mutation_view(m: &Mutation, host: &str, unit: &str, data: Value) -> View {
    let human = format!("{} {} on {}\n", m.past(), unit, host);
    let hints = m.reverse_cmd(unit).map(|c| json!({ "reverse": c }));
    View::new(m.kind(), host.to_string(), data, human).with_hints_opt(hints)
}

fn execute(cli: &Cli, m: &Mutation, host: &str, unit: &str) -> Result<View> {
    let mut client = crate::capabilities::connect(cli)?;
    let channel = client.dbus_open_privileged("org.freedesktop.systemd1")?;
    let mut ctx = CapabilityContext {
        client: &mut client,
        channel: &channel,
        host,
    };
    // Helper for the simple `*Unit` ops, which differ only by manager method.
    fn simple_unit(
        ctx: &mut CapabilityContext<'_>,
        m: &Mutation,
        unit: &str,
        method: &str,
    ) -> Result<View> {
        let out = ctx.client.dbus_call(
            ctx.channel,
            MGR_PATH,
            MGR_IFACE,
            method,
            json!([unit, "replace"]),
        )?;
        let job = out.get(0).and_then(Value::as_str).unwrap_or("").to_string();
        Ok(mutation_view(
            m,
            ctx.host,
            unit,
            json!({"operation": m.verb(), "unit": unit, "host": ctx.host, "job": job}),
        ))
    }
    match m {
        Mutation::Start => simple_unit(&mut ctx, m, unit, "StartUnit"),
        Mutation::Stop => simple_unit(&mut ctx, m, unit, "StopUnit"),
        Mutation::Restart => simple_unit(&mut ctx, m, unit, "RestartUnit"),
        Mutation::Reload => simple_unit(&mut ctx, m, unit, "ReloadUnit"),
        Mutation::Enable { now } => execute_enablement(
            &mut ctx,
            m,
            unit,
            *now,
            (
                "EnableUnitFiles",
                "StartUnit",
                json!([[unit], false, false]),
                1,
            ),
        ),
        Mutation::Disable { now } => execute_enablement(
            &mut ctx,
            m,
            unit,
            *now,
            ("DisableUnitFiles", "StopUnit", json!([[unit], false]), 0),
        ),
    }
}

fn execute_enablement(
    ctx: &mut CapabilityContext<'_>,
    m: &Mutation,
    unit: &str,
    now: bool,
    call: (&str, &str, Value, usize),
) -> Result<View> {
    let (method, followup_method, args, changes_index) = call;
    let out = ctx
        .client
        .dbus_call(ctx.channel, MGR_PATH, MGR_IFACE, method, args)?;
    let changes = out.get(changes_index).cloned().unwrap_or_else(|| json!([]));

    // Unit file changes leave systemd's cached UnitFileState stale until reload.
    ctx.client
        .dbus_call(ctx.channel, MGR_PATH, MGR_IFACE, "Reload", json!([]))?;
    if now {
        ctx.client.dbus_call(
            ctx.channel,
            MGR_PATH,
            MGR_IFACE,
            followup_method,
            json!([unit, "replace"]),
        )?;
    }
    Ok(mutation_view(
        m,
        ctx.host,
        unit,
        json!({"operation": m.verb(), "unit": unit, "host": ctx.host, "now": now, "changes": changes}),
    ))
}
