use crate::capabilities::{render, View};
use crate::cli::{Cli, ServicesAction};
use crate::error::{FezError, Result};
use crate::protocol::client::BridgeClient;
use crate::transport;
use serde_json::{json, Value};
use std::borrow::Cow;
use std::io::IsTerminal;

const MGR_PATH: &str = "/org/freedesktop/systemd1";
const MGR_IFACE: &str = "org.freedesktop.systemd1.Manager";
const PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";
const UNIT_IFACE: &str = "org.freedesktop.systemd1.Unit";

/// systemd's recognized unit-type extensions. A name ending in one of these is
/// already fully qualified; anything else defaults to `.service`. Mirrors the
/// suffix-defaulting half of systemd's `unit_name_mangle()`.
const UNIT_SUFFIXES: [&str; 11] = [
    ".service",
    ".socket",
    ".target",
    ".timer",
    ".mount",
    ".automount",
    ".swap",
    ".path",
    ".slice",
    ".scope",
    ".device",
];

/// Normalize a unit name the way systemctl does client-side: if it already ends
/// in a recognized systemd unit-type extension, pass it through; otherwise append
/// `.service`. Path/slash escaping is intentionally not handled.
fn mangle_unit(name: &str) -> Cow<'_, str> {
    if UNIT_SUFFIXES.iter().any(|suffix| name.ends_with(suffix)) {
        Cow::Borrowed(name)
    } else {
        Cow::Owned(format!("{name}.service"))
    }
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct ServiceUnit {
    name: String,
    description: String,
    load_state: String,
    active_state: String,
    sub_state: String,
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
        ReadAction::Status { unit } => status(&mut client, host, &mangle_unit(unit)),
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
            &mangle_unit(unit),
            since,
            priority,
            lines,
            follow,
        ),
    }
}

fn run_mutation(cli: &Cli, m: Mutation, unit: &str) -> Result<View> {
    let unit = mangle_unit(unit);
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
    View::new(m.kind(), host.to_string(), data, human).with_hints_opt(hints)
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

fn protocol_decode_error(message: impl Into<String>) -> FezError {
    FezError::Decode(<serde_json::Error as serde::de::Error>::custom(
        message.into(),
    ))
}

fn required_row_string(row: &Value, row_index: usize, idx: usize, field: &str) -> Result<String> {
    row.get(idx)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| {
            protocol_decode_error(format!(
                "ListUnits row {row_index} missing string field {idx} ({field})"
            ))
        })
}

fn parse_service_unit(row: &Value, row_index: usize) -> Result<ServiceUnit> {
    Ok(ServiceUnit {
        name: required_row_string(row, row_index, 0, "name")?,
        description: required_row_string(row, row_index, 1, "description")?,
        load_state: required_row_string(row, row_index, 2, "load_state")?,
        active_state: required_row_string(row, row_index, 3, "active_state")?,
        sub_state: required_row_string(row, row_index, 4, "sub_state")?,
    })
}

fn parse_list_units(out: &Value) -> Result<Vec<ServiceUnit>> {
    let units = out
        .get(0)
        .and_then(Value::as_array)
        .ok_or_else(|| protocol_decode_error("ListUnits reply missing unit array"))?;

    units
        .iter()
        .enumerate()
        .map(|(idx, row)| parse_service_unit(row, idx))
        .collect()
}

fn get_unit_path(out: &Value, unit: &str) -> Result<String> {
    out.get(0)
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            protocol_decode_error(format!("GetUnit reply for {unit} missing object path"))
        })
}

fn list(client: &mut BridgeClient, host: String, state: Option<&str>) -> Result<View> {
    let channel = client.dbus_open("org.freedesktop.systemd1")?;
    let out = client.dbus_call(&channel, MGR_PATH, MGR_IFACE, "ListUnits", json!([]))?;
    let units = parse_list_units(&out)?;

    let mut filtered = Vec::new();
    for unit in units {
        if let Some(want) = state {
            if unit.active_state != want {
                continue;
            }
        }
        filtered.push(unit);
    }

    let mut human = format!(
        "{:<28} {:<10} {:<10} {}\n",
        "UNIT", "ACTIVE", "SUB", "DESCRIPTION"
    );
    for unit in &filtered {
        human.push_str(&format!(
            "{:<28} {:<10} {:<10} {}\n",
            unit.name, unit.active_state, unit.sub_state, unit.description
        ));
    }
    // Columnar projection via the shared envelope helper: field names are
    // stated once in `columns`, and each unit becomes a positional row aligned
    // to that order. This keeps the `--json` payload compact for LLM consumers.
    // `units` above stays the source of truth for the human renderer.
    let columns = [
        "name",
        "description",
        "load_state",
        "active_state",
        "sub_state",
    ];
    let rows: Vec<Value> = filtered
        .iter()
        .map(|unit| {
            json!([
                unit.name,
                unit.description,
                unit.load_state,
                unit.active_state,
                unit.sub_state,
            ])
        })
        .collect();
    Ok(View::new(
        "ServiceList",
        host,
        crate::envelope::table_data(&columns, rows),
        human,
    ))
}

fn status(client: &mut BridgeClient, host: String, unit: &str) -> Result<View> {
    let channel = client.dbus_open("org.freedesktop.systemd1")?;
    let got = client.dbus_call(&channel, MGR_PATH, MGR_IFACE, "GetUnit", json!([unit]));
    let path = match got {
        Ok(out) => get_unit_path(&out, unit)?,
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
    Ok(View::new("ServiceStatus", host, data, human))
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
        return Ok(View::new("LogEntries", host, Value::Null, String::new()).pre_rendered());
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
    Ok(View::new(
        "LogEntries",
        host,
        json!({"unit": unit, "entries": entries}),
        human,
    ))
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

#[cfg(test)]
mod tests {
    use super::{
        get_unit_path, mangle_unit, parse_list_units, parse_service_unit, protocol_decode_error,
        required_row_string, s, ServiceUnit,
    };
    use crate::error::FezError;
    use serde_json::json;

    #[test]
    fn mangle_appends_service_to_bare_name() {
        assert_eq!(mangle_unit("NetworkManager"), "NetworkManager.service");
    }

    #[test]
    fn mangle_leaves_known_suffixes_untouched() {
        for name in [
            "sshd.service",
            "dbus.socket",
            "multi-user.target",
            "logrotate.timer",
            "var-lib.mount",
            "proc-sys.automount",
            "dev-sda1.swap",
            "run-foo.path",
            "user.slice",
            "session-1.scope",
            "dev-sda.device",
        ] {
            assert_eq!(mangle_unit(name), name);
        }
    }

    #[test]
    fn mangle_treats_unknown_dotted_tail_as_bare() {
        // Matches systemd: only a *recognized* unit-type extension is left alone.
        assert_eq!(mangle_unit("foo.bar"), "foo.bar.service");
    }

    #[test]
    fn mangle_does_not_double_suffix_service() {
        assert_eq!(mangle_unit("sshd.service"), "sshd.service");
    }

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

    #[test]
    fn parse_list_units_errors_when_unit_array_missing() {
        let err = parse_list_units(&json!([])).unwrap_err();
        assert!(matches!(err, FezError::Decode(_)));
    }

    #[test]
    fn parse_list_units_errors_when_required_row_field_missing() {
        let err = parse_list_units(&json!([[["sshd.service", "OpenSSH server"]]])).unwrap_err();
        assert!(matches!(err, FezError::Decode(_)));
    }

    #[test]
    fn parse_list_units_errors_when_required_row_field_is_wrong_type() {
        let err = parse_list_units(&json!([[[
            "sshd.service",
            "OpenSSH server",
            "loaded",
            true,
            "running"
        ]]]))
        .unwrap_err();
        assert!(matches!(err, FezError::Decode(_)));
    }

    #[test]
    fn get_unit_path_errors_when_success_reply_has_no_object_path() {
        let err = get_unit_path(&json!([]), "sshd.service").unwrap_err();
        assert!(matches!(err, FezError::Decode(_)));
    }

    // ── protocol_decode_error ────────────────────────────────────────────────

    #[test]
    fn protocol_decode_error_returns_decode_variant() {
        let err = protocol_decode_error("something went wrong");
        assert!(matches!(err, FezError::Decode(_)));
    }

    #[test]
    fn protocol_decode_error_message_is_preserved() {
        let err = protocol_decode_error("custom message");
        assert!(err.to_string().contains("custom message"));
    }

    // ── required_row_string ──────────────────────────────────────────────────

    #[test]
    fn required_row_string_returns_string_at_index() {
        let row = json!(["alpha", "beta", "gamma"]);
        assert_eq!(
            required_row_string(&row, 0, 1, "field").unwrap(),
            "beta"
        );
    }

    #[test]
    fn required_row_string_returns_first_element() {
        let row = json!(["first"]);
        assert_eq!(
            required_row_string(&row, 0, 0, "name").unwrap(),
            "first"
        );
    }

    #[test]
    fn required_row_string_errors_when_index_out_of_bounds() {
        let row = json!(["only-one"]);
        let err = required_row_string(&row, 0, 5, "missing").unwrap_err();
        assert!(matches!(err, FezError::Decode(_)));
    }

    #[test]
    fn required_row_string_errors_when_value_is_boolean() {
        let row = json!([true]);
        let err = required_row_string(&row, 0, 0, "field").unwrap_err();
        assert!(matches!(err, FezError::Decode(_)));
    }

    #[test]
    fn required_row_string_errors_when_value_is_number() {
        let row = json!([42]);
        let err = required_row_string(&row, 0, 0, "field").unwrap_err();
        assert!(matches!(err, FezError::Decode(_)));
    }

    #[test]
    fn required_row_string_errors_when_value_is_null() {
        let row = json!([null]);
        let err = required_row_string(&row, 0, 0, "field").unwrap_err();
        assert!(matches!(err, FezError::Decode(_)));
    }

    #[test]
    fn required_row_string_errors_when_value_is_array() {
        let row = json!([["nested"]]);
        let err = required_row_string(&row, 0, 0, "field").unwrap_err();
        assert!(matches!(err, FezError::Decode(_)));
    }

    // ── parse_service_unit ───────────────────────────────────────────────────

    #[test]
    fn parse_service_unit_succeeds_with_all_fields_present() {
        let row = json!(["sshd.service", "OpenSSH server", "loaded", "active", "running"]);
        let unit = parse_service_unit(&row, 0).unwrap();
        assert_eq!(unit.name, "sshd.service");
        assert_eq!(unit.description, "OpenSSH server");
        assert_eq!(unit.load_state, "loaded");
        assert_eq!(unit.active_state, "active");
        assert_eq!(unit.sub_state, "running");
    }

    #[test]
    fn parse_service_unit_errors_when_row_is_empty() {
        let row = json!([]);
        let err = parse_service_unit(&row, 0).unwrap_err();
        assert!(matches!(err, FezError::Decode(_)));
    }

    #[test]
    fn parse_service_unit_errors_when_missing_sub_state_field() {
        // Only 4 fields — sub_state (idx 4) is absent
        let row = json!(["sshd.service", "OpenSSH server", "loaded", "active"]);
        let err = parse_service_unit(&row, 0).unwrap_err();
        assert!(matches!(err, FezError::Decode(_)));
    }

    #[test]
    fn parse_service_unit_errors_when_name_field_is_wrong_type() {
        let row = json!([99, "OpenSSH server", "loaded", "active", "running"]);
        let err = parse_service_unit(&row, 0).unwrap_err();
        assert!(matches!(err, FezError::Decode(_)));
    }

    #[test]
    fn parse_service_unit_errors_when_active_state_field_is_wrong_type() {
        let row = json!(["sshd.service", "OpenSSH server", "loaded", false, "running"]);
        let err = parse_service_unit(&row, 0).unwrap_err();
        assert!(matches!(err, FezError::Decode(_)));
    }

    // ── parse_list_units ─────────────────────────────────────────────────────

    #[test]
    fn parse_list_units_succeeds_with_empty_unit_array() {
        let out = json!([[]]);
        let units = parse_list_units(&out).unwrap();
        assert!(units.is_empty());
    }

    #[test]
    fn parse_list_units_succeeds_with_single_unit() {
        let out = json!([[
            ["sshd.service", "OpenSSH server daemon", "loaded", "active", "running"]
        ]]);
        let units = parse_list_units(&out).unwrap();
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].name, "sshd.service");
        assert_eq!(units[0].description, "OpenSSH server daemon");
        assert_eq!(units[0].load_state, "loaded");
        assert_eq!(units[0].active_state, "active");
        assert_eq!(units[0].sub_state, "running");
    }

    #[test]
    fn parse_list_units_succeeds_with_multiple_units() {
        let out = json!([[
            ["sshd.service", "OpenSSH server daemon", "loaded", "active", "running"],
            ["nginx.service", "nginx web server", "loaded", "inactive", "dead"],
            ["cron.service", "Regular background work", "loaded", "active", "waiting"]
        ]]);
        let units = parse_list_units(&out).unwrap();
        assert_eq!(units.len(), 3);
        assert_eq!(units[0].name, "sshd.service");
        assert_eq!(units[1].name, "nginx.service");
        assert_eq!(units[1].active_state, "inactive");
        assert_eq!(units[2].name, "cron.service");
        assert_eq!(units[2].sub_state, "waiting");
    }

    #[test]
    fn parse_list_units_errors_when_top_level_is_not_array() {
        // Out is a JSON object, not an array — get(0) returns None
        let err = parse_list_units(&json!({"units": []})).unwrap_err();
        assert!(matches!(err, FezError::Decode(_)));
    }

    #[test]
    fn parse_list_units_errors_when_first_element_is_not_array() {
        // out[0] exists but is a string, not an array
        let err = parse_list_units(&json!(["not-an-array"])).unwrap_err();
        assert!(matches!(err, FezError::Decode(_)));
    }

    #[test]
    fn parse_list_units_errors_on_second_row_with_missing_field() {
        // First row is valid; second is incomplete — error propagates
        let out = json!([[
            ["sshd.service", "OpenSSH server daemon", "loaded", "active", "running"],
            ["broken.service", "Incomplete row"]
        ]]);
        let err = parse_list_units(&out).unwrap_err();
        assert!(matches!(err, FezError::Decode(_)));
    }

    // ── get_unit_path ────────────────────────────────────────────────────────

    #[test]
    fn get_unit_path_returns_path_on_success() {
        let path = "/org/freedesktop/systemd1/unit/sshd_2eservice";
        let out = json!([path]);
        assert_eq!(get_unit_path(&out, "sshd.service").unwrap(), path);
    }

    #[test]
    fn get_unit_path_errors_when_path_is_empty_string() {
        let err = get_unit_path(&json!([""]), "sshd.service").unwrap_err();
        assert!(matches!(err, FezError::Decode(_)));
    }

    #[test]
    fn get_unit_path_errors_when_first_element_is_number() {
        let err = get_unit_path(&json!([42]), "sshd.service").unwrap_err();
        assert!(matches!(err, FezError::Decode(_)));
    }

    #[test]
    fn get_unit_path_errors_when_first_element_is_null() {
        let err = get_unit_path(&json!([null]), "sshd.service").unwrap_err();
        assert!(matches!(err, FezError::Decode(_)));
    }

    #[test]
    fn get_unit_path_errors_when_first_element_is_object() {
        let err = get_unit_path(&json!([{"path": "/some/path"}]), "sshd.service").unwrap_err();
        assert!(matches!(err, FezError::Decode(_)));
    }

    // ── ServiceUnit struct properties ────────────────────────────────────────

    #[test]
    fn service_unit_equality_holds_for_identical_values() {
        let a = ServiceUnit {
            name: "sshd.service".into(),
            description: "OpenSSH server".into(),
            load_state: "loaded".into(),
            active_state: "active".into(),
            sub_state: "running".into(),
        };
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn service_unit_inequality_detected_on_differing_field() {
        let a = ServiceUnit {
            name: "sshd.service".into(),
            description: "OpenSSH server".into(),
            load_state: "loaded".into(),
            active_state: "active".into(),
            sub_state: "running".into(),
        };
        let b = ServiceUnit {
            name: "sshd.service".into(),
            description: "OpenSSH server".into(),
            load_state: "loaded".into(),
            active_state: "inactive".into(), // differs
            sub_state: "dead".into(),
        };
        assert_ne!(a, b);
    }

    #[test]
    fn service_unit_clone_produces_independent_copy() {
        let original = ServiceUnit {
            name: "cron.service".into(),
            description: "Scheduled tasks".into(),
            load_state: "loaded".into(),
            active_state: "active".into(),
            sub_state: "waiting".into(),
        };
        let cloned = original.clone();
        assert_eq!(original, cloned);
        // Confirm fields match individually
        assert_eq!(cloned.name, "cron.service");
        assert_eq!(cloned.sub_state, "waiting");
    }

    // ── regression: empty top-level JSON value ───────────────────────────────

    #[test]
    fn parse_list_units_errors_on_json_null() {
        let err = parse_list_units(&json!(null)).unwrap_err();
        assert!(matches!(err, FezError::Decode(_)));
    }

    #[test]
    fn get_unit_path_error_message_names_the_unit() {
        let err = get_unit_path(&json!([]), "my-custom.service").unwrap_err();
        // The error message should mention the unit name for diagnostics
        assert!(err.to_string().contains("my-custom.service"));
    }

    // `classify` is the single total mapping from the flat clap enum to the
    // read/mutate split. It replaces the `unreachable!()`-guarded helpers: if a
    // new `ServicesAction` variant is added, this match fails to compile rather

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
