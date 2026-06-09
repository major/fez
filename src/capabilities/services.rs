use crate::audit::AuditRecord;
use crate::cli::{Cli, ServicesAction, TopCommand};
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

fn action_to_mutation(action: &ServicesAction) -> (Mutation, &str) {
    match action {
        ServicesAction::Start { unit } => (Mutation::Start, unit),
        ServicesAction::Stop { unit } => (Mutation::Stop, unit),
        ServicesAction::Restart { unit } => (Mutation::Restart, unit),
        ServicesAction::Reload { unit } => (Mutation::Reload, unit),
        ServicesAction::Enable { unit, now } => (Mutation::Enable { now: *now }, unit),
        ServicesAction::Disable { unit, now } => (Mutation::Disable { now: *now }, unit),
        _ => unreachable!("action_to_mutation called for a read"),
    }
}

/// Run the requested `services` subcommand and return the process exit code.
pub fn dispatch(cli: &Cli) -> i32 {
    let action = match &cli.command {
        TopCommand::Services { action } => action,
        _ => unreachable!("dispatch called for non-services command"),
    };
    render(cli, run_action(cli, action))
}

fn run_action(cli: &Cli, action: &ServicesAction) -> Result<View> {
    match action {
        ServicesAction::List { .. }
        | ServicesAction::Status { .. }
        | ServicesAction::Logs { .. } => run_read(cli, action),
        _ => run_mutation(cli, action),
    }
}

fn run_read(cli: &Cli, action: &ServicesAction) -> Result<View> {
    let transport = transport::from_host(cli.host.as_deref());
    let mut client = BridgeClient::connect(transport.as_ref())?;
    let host = client.host().to_string();
    match action {
        ServicesAction::List { state } => list(&mut client, host, state.as_deref()),
        ServicesAction::Status { unit } => status(&mut client, host, unit),
        ServicesAction::Logs {
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
            since.as_deref(),
            priority.as_deref(),
            *lines,
            *follow,
        ),
        _ => unreachable!("run_read called for a mutation"),
    }
}

fn run_mutation(cli: &Cli, action: &ServicesAction) -> Result<View> {
    let (m, unit) = action_to_mutation(action);
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
    let actor = crate::audit::actor();
    let cid = crate::audit::correlation_id();
    sink.write(&AuditRecord::new(
        &actor,
        &host,
        m.verb(),
        unit,
        "attempt",
        None,
        &cid,
    ));
    let result = execute(cli, &m, &host, unit);
    match &result {
        Ok(_) => sink.write(&AuditRecord::new(
            &actor,
            &host,
            m.verb(),
            unit,
            "ok",
            None,
            &cid,
        )),
        Err(e) => sink.write(&AuditRecord::new(
            &actor,
            &host,
            m.verb(),
            unit,
            "error",
            Some(e.to_string()),
            &cid,
        )),
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
    fn manager_method(&self) -> &'static str {
        match self {
            Mutation::Start => "StartUnit",
            Mutation::Stop => "StopUnit",
            Mutation::Restart => "RestartUnit",
            Mutation::Reload => "ReloadUnit",
            _ => unreachable!("manager_method called for enable/disable"),
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
    match m {
        Mutation::Start | Mutation::Stop | Mutation::Restart | Mutation::Reload => {
            let out = client.dbus_call(
                &channel,
                MGR_PATH,
                MGR_IFACE,
                m.manager_method(),
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
        Mutation::Enable { now } => {
            let out = client.dbus_call(
                &channel,
                MGR_PATH,
                MGR_IFACE,
                "EnableUnitFiles",
                json!([[unit], false, false]),
            )?;
            // out_args = [carries_install_info (bool), changes (array)].
            let changes = out.get(1).cloned().unwrap_or_else(|| json!([]));
            // EnableUnitFiles writes the symlink but leaves each unit's cached
            // UnitFileState stale (still "disabled") until the manager reloads.
            // `systemctl enable` reloads implicitly; do the same so a follow-up
            // status reports "enabled" instead of the stale value.
            reload_daemon(&mut client, &channel)?;
            if *now {
                client.dbus_call(
                    &channel,
                    MGR_PATH,
                    MGR_IFACE,
                    "StartUnit",
                    json!([unit, "replace"]),
                )?;
            }
            Ok(mutation_view(
                m,
                host,
                unit,
                json!({"operation": "enable", "unit": unit, "host": host, "now": *now, "changes": changes}),
            ))
        }
        Mutation::Disable { now } => {
            let out = client.dbus_call(
                &channel,
                MGR_PATH,
                MGR_IFACE,
                "DisableUnitFiles",
                json!([[unit], false]),
            )?;
            // out_args = [changes (array)].
            let changes = out.get(0).cloned().unwrap_or_else(|| json!([]));
            // Same stale-cache caveat as enable: reload so the cached
            // UnitFileState refreshes to "disabled".
            reload_daemon(&mut client, &channel)?;
            if *now {
                client.dbus_call(
                    &channel,
                    MGR_PATH,
                    MGR_IFACE,
                    "StopUnit",
                    json!([unit, "replace"]),
                )?;
            }
            Ok(mutation_view(
                m,
                host,
                unit,
                json!({"operation": "disable", "unit": unit, "host": host, "now": *now, "changes": changes}),
            ))
        }
    }
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
                        println!("{}", serde_json::to_string(&log_entry(&v)).unwrap());
                    } else {
                        println!(
                            "{}  {}: {}",
                            s(&v, "__REALTIME_TIMESTAMP"),
                            s(&v, "SYSLOG_IDENTIFIER"),
                            s(&v, "MESSAGE")
                        );
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
            human.push_str(&format!(
                "{}  {}: {}\n",
                s(&v, "__REALTIME_TIMESTAMP"),
                s(&v, "SYSLOG_IDENTIFIER"),
                s(&v, "MESSAGE")
            ));
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
}
