use crate::cli::{Cli, ServicesAction};
use crate::envelope::{ApiError, Envelope};
use crate::error::{FezError, Result};
use crate::protocol::client::BridgeClient;
use crate::transport;
use serde_json::{json, Value};
use std::io::IsTerminal;

const MGR_PATH: &str = "/org/freedesktop/systemd1";
const MGR_IFACE: &str = "org.freedesktop.systemd1.Manager";
const PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";
const UNIT_IFACE: &str = "org.freedesktop.systemd1.Unit";

struct View {
    kind: &'static str,
    host: String,
    data: Value,
    human: String,
    pre_rendered: bool, // true when already streamed to stdout (logs --follow)
    hints: Option<Value>,
}

#[derive(Clone, Copy)]
enum Mutation {
    Start,
    Stop,
    Restart,
    Reload,
    Enable { now: bool },
    Disable { now: bool },
}

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
}

/// A read subcommand and its arguments, borrowed from the parsed action.
///
/// Splitting reads out of [`ServicesAction`] makes [`run_read`] total: every
/// variant here maps to a handler, so adding one is a compile error rather than
/// a runtime panic.
enum ReadAction<'a> {
    List {
        state: Option<&'a str>,
    },
    Status {
        unit: &'a str,
    },
    Logs {
        unit: &'a str,
        since: Option<&'a str>,
        priority: Option<&'a str>,
        lines: Option<u32>,
        follow: bool,
    },
}

/// The read/mutate split of a parsed [`ServicesAction`].
///
/// [`classify`] is the single place that matches the full clap enum; everything
/// downstream consumes one arm of this and is therefore total.
enum Plan<'a> {
    Read(ReadAction<'a>),
    Mutate { mutation: Mutation, unit: &'a str },
}

/// Map the flat clap enum onto the read/mutate [`Plan`] split.
///
/// This is the only exhaustive match over [`ServicesAction`]; the rest of the
/// module works off [`Plan`], so a new variant breaks the build here instead of
/// hitting an `unreachable!` at runtime.
fn classify(action: &ServicesAction) -> Plan<'_> {
    match action {
        ServicesAction::List { state } => Plan::Read(ReadAction::List {
            state: state.as_deref(),
        }),
        ServicesAction::Status { unit } => Plan::Read(ReadAction::Status { unit }),
        ServicesAction::Logs {
            unit,
            since,
            priority,
            lines,
            follow,
        } => Plan::Read(ReadAction::Logs {
            unit,
            since: since.as_deref(),
            priority: priority.as_deref(),
            lines: *lines,
            follow: *follow,
        }),
        ServicesAction::Start { unit } => Plan::Mutate {
            mutation: Mutation::Start,
            unit,
        },
        ServicesAction::Stop { unit } => Plan::Mutate {
            mutation: Mutation::Stop,
            unit,
        },
        ServicesAction::Restart { unit } => Plan::Mutate {
            mutation: Mutation::Restart,
            unit,
        },
        ServicesAction::Reload { unit } => Plan::Mutate {
            mutation: Mutation::Reload,
            unit,
        },
        ServicesAction::Enable { unit, now } => Plan::Mutate {
            mutation: Mutation::Enable { now: *now },
            unit,
        },
        ServicesAction::Disable { unit, now } => Plan::Mutate {
            mutation: Mutation::Disable { now: *now },
            unit,
        },
    }
}

/// Run the requested `services` subcommand and return the process exit code.
pub fn dispatch(cli: &Cli, action: &ServicesAction) -> i32 {
    let view = match classify(action) {
        Plan::Read(read) => run_read(cli, read),
        Plan::Mutate { mutation, unit } => run_mutation(cli, mutation, unit),
    };
    render(cli, view)
}

fn run_read(cli: &Cli, action: ReadAction<'_>) -> Result<View> {
    let transport = transport::from_host(cli.host.as_deref());
    let mut client = BridgeClient::connect(transport.as_ref())?;
    let host = client.host().to_string();
    match action {
        ReadAction::List { state } => list(&mut client, host, state),
        ReadAction::Status { unit } => status(&mut client, host, unit),
        ReadAction::Logs {
            unit,
            since,
            priority,
            lines,
            follow,
        } => logs(
            &mut client,
            host,
            cli.json,
            unit,
            since,
            priority,
            lines,
            follow,
        ),
    }
}

fn run_mutation(cli: &Cli, m: Mutation, unit: &str) -> Result<View> {
    let host = cli.host.clone().unwrap_or_else(|| "localhost".into());

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
    let sink = crate::audit::sink_from_env();
    let audit = crate::audit::AuditContext::new(
        &crate::audit::actor(),
        &host,
        m.verb(),
        unit,
        &crate::audit::correlation_id(),
    );
    sink.write(&audit.record(crate::audit::Outcome::Attempt));
    let result = execute(cli, &m, &host, unit);
    match &result {
        Ok(_) => sink.write(&audit.record(crate::audit::Outcome::Ok)),
        Err(e) => sink.write(&audit.record(crate::audit::Outcome::Error(e.to_string()))),
    }
    result
}

fn dry_run_view(m: &Mutation, host: &str, unit: &str) -> View {
    let command = format!("fez services {} {}{}", m.verb(), unit, m.now_suffix());
    let human = format!(
        "DRY-RUN: would {} {} on {} (requires elevation)\n",
        m.verb(),
        unit,
        host
    );
    View {
        kind: "DryRun",
        host: host.to_string(),
        data: json!({
            "operation": m.verb(),
            "unit": unit,
            "host": host,
            "privileged": true,
            "command": command,
        }),
        human,
        pre_rendered: false,
        hints: None,
    }
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

impl Mutation {
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

fn mutation_view(m: &Mutation, host: &str, unit: &str, data: Value) -> View {
    let human = format!("{} {} on {}\n", m.past(), unit, host);
    let hints = m.reverse_cmd(unit).map(|c| json!({ "reverse": c }));
    View {
        kind: m.kind(),
        host: host.to_string(),
        data,
        human,
        pre_rendered: false,
        hints,
    }
}

fn execute(cli: &Cli, m: &Mutation, host: &str, unit: &str) -> Result<View> {
    let transport = transport::from_host(cli.host.as_deref());
    let mut client = BridgeClient::connect(transport.as_ref())?;
    let channel = client.dbus_open_privileged("org.freedesktop.systemd1")?;
    // Helper for the simple `*Unit` ops, which differ only by manager method.
    // Keeping it a fn (not a closure) avoids capturing `client`, so the
    // enablement arms below can still borrow it.
    fn simple_unit(
        client: &mut BridgeClient,
        channel: &str,
        m: &Mutation,
        host: &str,
        unit: &str,
        method: &str,
    ) -> Result<View> {
        let out = client.dbus_call(
            channel,
            MGR_PATH,
            MGR_IFACE,
            method,
            json!([unit, "replace"]),
        )?;
        let job = out.get(0).and_then(Value::as_str).unwrap_or("").to_string();
        Ok(mutation_view(
            m,
            host,
            unit,
            json!({"operation": m.verb(), "unit": unit, "host": host, "job": job}),
        ))
    }
    match m {
        Mutation::Start => simple_unit(&mut client, &channel, m, host, unit, "StartUnit"),
        Mutation::Stop => simple_unit(&mut client, &channel, m, host, unit, "StopUnit"),
        Mutation::Restart => simple_unit(&mut client, &channel, m, host, unit, "RestartUnit"),
        Mutation::Reload => simple_unit(&mut client, &channel, m, host, unit, "ReloadUnit"),
        Mutation::Enable { now } => {
            execute_enablement(&mut client, &channel, Enablement::Enable, host, unit, *now)
        }
        Mutation::Disable { now } => {
            execute_enablement(&mut client, &channel, Enablement::Disable, host, unit, *now)
        }
    }
}

/// The two unit-file operations, split out of [`Mutation`] so the enablement
/// path is total: every variant here maps to a real D-Bus call, so the simple
/// `*Unit` ops cannot reach [`execute_enablement`] and no `unreachable!` is
/// needed to satisfy exhaustiveness.
#[derive(Clone, Copy)]
enum Enablement {
    Enable,
    Disable,
}

impl Enablement {
    /// The owning [`Mutation`] for `mutation_view`, with `now` threaded back in.
    fn mutation(self, now: bool) -> Mutation {
        match self {
            Enablement::Enable => Mutation::Enable { now },
            Enablement::Disable => Mutation::Disable { now },
        }
    }
}

fn execute_enablement(
    client: &mut BridgeClient,
    channel: &str,
    op: Enablement,
    host: &str,
    unit: &str,
    now: bool,
) -> Result<View> {
    // Shared enable/disable path: issue the unit-file D-Bus call, extract the
    // method-specific changes output, refresh systemd's cached state, and run
    // the matching StartUnit/StopUnit follow-up when --now is requested.
    //
    // `args`: EnableUnitFiles takes [[units], runtime, force]; DisableUnitFiles
    // takes [[units], runtime]. `changes_idx`: EnableUnitFiles out_args are
    // [carries_install_info (bool), changes (array)] so changes is at 1;
    // DisableUnitFiles out_args are [changes (array)] so changes is at 0.
    let (unit_file_method, followup_method, args, changes_idx) = match op {
        Enablement::Enable => (
            "EnableUnitFiles",
            "StartUnit",
            json!([[unit], false, false]),
            1,
        ),
        Enablement::Disable => ("DisableUnitFiles", "StopUnit", json!([[unit], false]), 0),
    };
    let out = client.dbus_call(channel, MGR_PATH, MGR_IFACE, unit_file_method, args)?;
    let changes = out.get(changes_idx).cloned().unwrap_or_else(|| json!([]));

    // Unit file changes leave systemd's cached UnitFileState stale until reload.
    reload_daemon(client, channel)?;
    if now {
        client.dbus_call(
            channel,
            MGR_PATH,
            MGR_IFACE,
            followup_method,
            json!([unit, "replace"]),
        )?;
    }
    let m = op.mutation(now);
    Ok(mutation_view(
        &m,
        host,
        unit,
        json!({"operation": m.verb(), "unit": unit, "host": host, "now": now, "changes": changes}),
    ))
}

/// Ask systemd to reload its manager configuration so cached unit-file states
/// reflect symlink changes made by EnableUnitFiles/DisableUnitFiles.
fn reload_daemon(client: &mut BridgeClient, channel: &str) -> Result<()> {
    client.dbus_call(channel, MGR_PATH, MGR_IFACE, "Reload", json!([]))?;
    Ok(())
}

fn s(v: &Value, key: &str) -> String {
    let field = v.get(key);
    // cockpit-bridge wraps a{sv} dict values as D-Bus variants
    // ({"t":"s","v":"active"}); journalctl/ListUnits fields are flat strings.
    // Accept both: prefer the variant's "v" payload, else the value itself.
    field
        .and_then(|f| f.get("v").unwrap_or(f).as_str())
        .unwrap_or("")
        .to_string()
}
fn at(v: &Value, idx: usize) -> String {
    v.get(idx).and_then(Value::as_str).unwrap_or("").to_string()
}

fn list(client: &mut BridgeClient, host: String, state: Option<&str>) -> Result<View> {
    let channel = client.dbus_open("org.freedesktop.systemd1")?;
    let out = client.dbus_call(&channel, MGR_PATH, MGR_IFACE, "ListUnits", json!([]))?;
    let units_raw = out
        .get(0)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut units = Vec::new();
    for u in &units_raw {
        let active = at(u, 3);
        if let Some(want) = state {
            if active != want {
                continue;
            }
        }
        units.push(json!({
            "name": at(u, 0), "description": at(u, 1), "load_state": at(u, 2),
            "active_state": active, "sub_state": at(u, 4),
        }));
    }

    let mut human = format!(
        "{:<28} {:<10} {:<10} {}\n",
        "UNIT", "ACTIVE", "SUB", "DESCRIPTION"
    );
    for u in &units {
        human.push_str(&format!(
            "{:<28} {:<10} {:<10} {}\n",
            s(u, "name"),
            s(u, "active_state"),
            s(u, "sub_state"),
            s(u, "description")
        ));
    }
    Ok(View {
        kind: "ServiceList",
        host,
        data: json!({"units": units}),
        human,
        pre_rendered: false,
        hints: None,
    })
}

fn status(client: &mut BridgeClient, host: String, unit: &str) -> Result<View> {
    let channel = client.dbus_open("org.freedesktop.systemd1")?;
    let got = client.dbus_call(&channel, MGR_PATH, MGR_IFACE, "GetUnit", json!([unit]));
    let path = match got {
        Ok(out) => out.get(0).and_then(Value::as_str).unwrap_or("").to_string(),
        Err(FezError::Dbus { name, .. }) if name.contains("NoSuchUnit") => {
            return Err(FezError::NotFound(unit.to_string()))
        }
        Err(e) => return Err(e),
    };
    let out = client.dbus_call(&channel, &path, PROPS_IFACE, "GetAll", json!([UNIT_IFACE]))?;
    let props = out.get(0).cloned().unwrap_or(Value::Null);

    let data = json!({
        "id": s(&props, "Id"),
        "description": s(&props, "Description"),
        "load_state": s(&props, "LoadState"),
        "active_state": s(&props, "ActiveState"),
        "sub_state": s(&props, "SubState"),
        "unit_file_state": s(&props, "UnitFileState"),
    });
    let human = format!(
        "{} - {}\n  state: {} ({})\n  enabled: {}\n",
        s(&props, "Id"),
        s(&props, "Description"),
        s(&props, "ActiveState"),
        s(&props, "SubState"),
        s(&props, "UnitFileState")
    );
    Ok(View {
        kind: "ServiceStatus",
        host,
        data,
        human,
        pre_rendered: false,
        hints: None,
    })
}

#[allow(clippy::too_many_arguments)]
fn logs(
    client: &mut BridgeClient,
    host: String,
    as_json: bool,
    unit: &str,
    since: Option<&str>,
    priority: Option<&str>,
    lines: Option<u32>,
    follow: bool,
) -> Result<View> {
    let lines_s = lines.map(|n| n.to_string());
    let mut argv: Vec<&str> = vec!["journalctl", "--output=json", "--no-pager", "--unit", unit];
    if let Some(x) = since {
        argv.extend(["--since", x]);
    }
    if let Some(x) = priority {
        argv.extend(["--priority", x]);
    }
    if let Some(x) = lines_s.as_deref() {
        argv.extend(["--lines", x]);
    }
    if follow {
        argv.push("--follow");
    }

    if follow {
        // stream live; print each parsed line as it arrives.
        client.stream_each(&argv, |chunk| {
            for line in chunk.split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
                if let Ok(v) = serde_json::from_slice::<Value>(line) {
                    if as_json {
                        println!(
                            "{}",
                            serde_json::to_string(&log_entry(&v)).unwrap_or_default()
                        );
                    } else {
                        println!("{}", log_human_line(&v));
                    }
                }
            }
        })?;
        return Ok(View {
            kind: "LogEntries",
            host,
            data: Value::Null,
            human: String::new(),
            pre_rendered: true,
            hints: None,
        });
    }

    let blob = client.stream_collect(&argv)?;
    let mut entries = Vec::new();
    let mut human = String::new();
    for line in blob.split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
        if let Ok(v) = serde_json::from_slice::<Value>(line) {
            human.push_str(&log_human_line(&v));
            human.push('\n');
            entries.push(log_entry(&v));
        }
    }
    Ok(View {
        kind: "LogEntries",
        host,
        data: json!({"unit": unit, "entries": entries}),
        human,
        pre_rendered: false,
        hints: None,
    })
}

fn log_entry(v: &Value) -> Value {
    json!({
        "timestamp": s(v, "__REALTIME_TIMESTAMP"),
        "priority": s(v, "PRIORITY"),
        "identifier": s(v, "SYSLOG_IDENTIFIER"),
        "message": s(v, "MESSAGE"),
        "pid": s(v, "_PID"),
    })
}

fn log_human_line(v: &Value) -> String {
    format!(
        "{}  {}: {}",
        s(v, "__REALTIME_TIMESTAMP"),
        s(v, "SYSLOG_IDENTIFIER"),
        s(v, "MESSAGE")
    )
}

fn render(cli: &Cli, result: Result<View>) -> i32 {
    let host = cli.host.clone().unwrap_or_else(|| "localhost".into());
    match result {
        Ok(view) => {
            if view.pre_rendered {
                return 0;
            }
            if cli.json {
                let mut env = Envelope::ok(view.kind, &view.host, view.data);
                if let Some(h) = view.hints {
                    env = env.with_hints(h);
                }
                println!("{}", env.to_json_string());
            } else {
                print!("{}", view.human);
            }
            0
        }
        Err(e) => {
            if cli.json {
                let env = Envelope::error(
                    "Error",
                    &host,
                    ApiError {
                        code: e.code().into(),
                        message: e.to_string(),
                        detail: None,
                    },
                );
                println!("{}", env.to_json_string());
            } else {
                eprintln!("error: {e}");
            }
            e.exit_code()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::s;
    use serde_json::json;

    // Real cockpit-bridge returns a{sv} dicts with each value wrapped as a
    // D-Bus variant: {"t":"s","v":"active"}. `s()` must unwrap that to "active".
    #[test]
    fn s_unwraps_string_variant() {
        let props = json!({
            "ActiveState": {"t": "s", "v": "active"},
            "UnitFileState": {"t": "s", "v": "enabled"},
        });
        assert_eq!(s(&props, "ActiveState"), "active");
        assert_eq!(s(&props, "UnitFileState"), "enabled");
    }

    // journalctl JSON and ListUnits positional fields are flat strings, not
    // variants. `s()` must keep returning them unchanged.
    #[test]
    fn s_passes_through_flat_string() {
        let flat = json!({"MESSAGE": "hello"});
        assert_eq!(s(&flat, "MESSAGE"), "hello");
    }

    // Missing keys yield an empty string.
    #[test]
    fn s_missing_key_is_empty() {
        let props = json!({"ActiveState": {"t": "s", "v": "active"}});
        assert_eq!(s(&props, "Nope"), "");
    }

    // `classify` is the single total mapping from the flat clap enum to the
    // read/mutate split. It replaces the `unreachable!()`-guarded helpers: if a
    // new `ServicesAction` variant is added, this match fails to compile rather
    // than panicking at runtime. These cases pin the routing for every variant.
    #[test]
    fn classify_routes_reads() {
        use super::{classify, Plan};
        use crate::cli::ServicesAction;

        assert!(matches!(
            classify(&ServicesAction::List { state: None }),
            Plan::Read(_)
        ));
        assert!(matches!(
            classify(&ServicesAction::Status {
                unit: "sshd".into()
            }),
            Plan::Read(_)
        ));
        assert!(matches!(
            classify(&ServicesAction::Logs {
                unit: "sshd".into(),
                since: None,
                priority: None,
                lines: Some(50),
                follow: false,
            }),
            Plan::Read(_)
        ));
    }

    #[test]
    fn classify_routes_mutations() {
        use super::{classify, Mutation, Plan};
        use crate::cli::ServicesAction;

        let cases = [
            (ServicesAction::Start { unit: "u".into() }, Mutation::Start),
            (ServicesAction::Stop { unit: "u".into() }, Mutation::Stop),
            (
                ServicesAction::Restart { unit: "u".into() },
                Mutation::Restart,
            ),
            (
                ServicesAction::Reload { unit: "u".into() },
                Mutation::Reload,
            ),
            (
                ServicesAction::Enable {
                    unit: "u".into(),
                    now: true,
                },
                Mutation::Enable { now: true },
            ),
            (
                ServicesAction::Disable {
                    unit: "u".into(),
                    now: false,
                },
                Mutation::Disable { now: false },
            ),
        ];
        for (action, want) in cases {
            match classify(&action) {
                Plan::Mutate { mutation, unit } => {
                    assert_eq!(mutation.verb(), want.verb());
                    assert!(matches!(
                        (mutation, want),
                        (Mutation::Start, Mutation::Start)
                            | (Mutation::Stop, Mutation::Stop)
                            | (Mutation::Restart, Mutation::Restart)
                            | (Mutation::Reload, Mutation::Reload)
                            | (Mutation::Enable { .. }, Mutation::Enable { .. })
                            | (Mutation::Disable { .. }, Mutation::Disable { .. })
                    ));
                    assert_eq!(unit, "u");
                }
                Plan::Read(_) => panic!("mutation classified as read"),
            }
        }
    }
}
