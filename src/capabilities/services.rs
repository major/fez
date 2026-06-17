use crate::capabilities::{render, View};
use crate::cli::{Cli, ServicesAction};
use crate::error::{FezError, Result};
use crate::protocol::client::BridgeClient;
use crate::protocol::variant::Variant;
use crate::transport;
use serde::Deserialize;
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

struct EnablementCall {
    method: &'static str,
    followup_method: &'static str,
    args: Value,
    changes_index: usize,
}

impl Enablement {
    /// The owning [`Mutation`] for `mutation_view`, with `now` threaded back in.
    fn mutation(self, now: bool) -> Mutation {
        match self {
            Enablement::Enable => Mutation::Enable { now },
            Enablement::Disable => Mutation::Disable { now },
        }
    }

    fn unit_file_call(self, unit: &str) -> EnablementCall {
        match self {
            Enablement::Enable => EnablementCall {
                method: "EnableUnitFiles",
                followup_method: "StartUnit",
                args: json!([[unit], false, false]),
                changes_index: 1,
            },
            Enablement::Disable => EnablementCall {
                method: "DisableUnitFiles",
                followup_method: "StopUnit",
                args: json!([[unit], false]),
                changes_index: 0,
            },
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
    let call = op.unit_file_call(unit);
    let out = client.dbus_call(channel, MGR_PATH, MGR_IFACE, call.method, call.args)?;
    let changes = out
        .get(call.changes_index)
        .cloned()
        .unwrap_or_else(|| json!([]));

    // Unit file changes leave systemd's cached UnitFileState stale until reload.
    reload_daemon(client, channel)?;
    if now {
        client.dbus_call(
            channel,
            MGR_PATH,
            MGR_IFACE,
            call.followup_method,
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

/// systemd `Unit` interface properties read via `Properties.GetAll`.
///
/// cockpit delivers this `a{sv}` dict with each value wrapped as a D-Bus
/// variant (`{"t":"s","v":"active"}`); [`Variant`] unwraps it transparently.
/// Every field is `#[serde(default)]` so an absent property decodes to the
/// empty string, matching the previous `s()` accessor's tolerance.
#[derive(Debug, Default, Deserialize)]
struct UnitProps {
    #[serde(rename = "Id", default)]
    id: Variant<String>,
    #[serde(rename = "Description", default)]
    description: Variant<String>,
    #[serde(rename = "LoadState", default)]
    load_state: Variant<String>,
    #[serde(rename = "ActiveState", default)]
    active_state: Variant<String>,
    #[serde(rename = "SubState", default)]
    sub_state: Variant<String>,
    #[serde(rename = "UnitFileState", default)]
    unit_file_state: Variant<String>,
}

/// A single journald entry from `journalctl --output=json`.
///
/// journald fields arrive as flat scalars (no variant envelope); [`Variant`]
/// passes them through unchanged. Defaults keep partial entries decodable.
#[derive(Debug, Default, Deserialize)]
struct JournalLine {
    #[serde(rename = "__REALTIME_TIMESTAMP", default)]
    timestamp: Variant<String>,
    #[serde(rename = "PRIORITY", default)]
    priority: Variant<String>,
    #[serde(rename = "SYSLOG_IDENTIFIER", default)]
    identifier: Variant<String>,
    #[serde(rename = "MESSAGE", default)]
    message: Variant<String>,
    #[serde(rename = "_PID", default)]
    pid: Variant<String>,
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
    let props_val = out.get(0).cloned().unwrap_or_else(|| json!({}));
    let props: UnitProps = serde_json::from_value(props_val).map_err(FezError::Decode)?;

    let data = json!({
        "id": props.id.0,
        "description": props.description.0,
        "load_state": props.load_state.0,
        "active_state": props.active_state.0,
        "sub_state": props.sub_state.0,
        "unit_file_state": props.unit_file_state.0,
    });
    let human = format!(
        "{} - {}\n  state: {} ({})\n  enabled: {}\n",
        props.id.0,
        props.description.0,
        props.active_state.0,
        props.sub_state.0,
        props.unit_file_state.0
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
                if let Ok(entry) = serde_json::from_slice::<JournalLine>(line) {
                    if as_json {
                        println!(
                            "{}",
                            serde_json::to_string(&log_entry(&entry)).unwrap_or_default()
                        );
                    } else {
                        println!("{}", log_human_line(&entry));
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
        if let Ok(entry) = serde_json::from_slice::<JournalLine>(line) {
            human.push_str(&log_human_line(&entry));
            human.push('\n');
            entries.push(log_entry(&entry));
        }
    }
    Ok(View::new(
        "LogEntries",
        host,
        json!({"unit": unit, "entries": entries}),
        human,
    ))
}

fn log_entry(entry: &JournalLine) -> Value {
    json!({
        "timestamp": entry.timestamp.0,
        "priority": entry.priority.0,
        "identifier": entry.identifier.0,
        "message": entry.message.0,
        "pid": entry.pid.0,
    })
}

fn log_human_line(entry: &JournalLine) -> String {
    format!(
        "{}  {}: {}",
        entry.timestamp.0, entry.identifier.0, entry.message.0
    )
}

#[cfg(test)]
mod tests {
    use super::{get_unit_path, mangle_unit, parse_list_units, JournalLine, UnitProps};
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
    // D-Bus variant: {"t":"s","v":"active"}. UnitProps unwraps via Variant<T>.
    #[test]
    fn unit_props_unwraps_variant_dict() {
        let props: UnitProps = serde_json::from_value(json!({
            "Id": {"t": "s", "v": "sshd.service"},
            "ActiveState": {"t": "s", "v": "active"},
            "UnitFileState": {"t": "s", "v": "enabled"},
        }))
        .unwrap();
        assert_eq!(props.id.0, "sshd.service");
        assert_eq!(props.active_state.0, "active");
        assert_eq!(props.unit_file_state.0, "enabled");
        // Absent properties default to the empty string, as `s()` did.
        assert_eq!(props.description.0, "");
    }

    // journald JSON fields are flat strings, not variants; Variant<T> passes
    // them through unchanged and absent fields default to empty.
    #[test]
    fn journal_line_reads_flat_fields() {
        let entry: JournalLine = serde_json::from_value(json!({
            "MESSAGE": "hello",
            "SYSLOG_IDENTIFIER": "sshd",
        }))
        .unwrap();
        assert_eq!(entry.message.0, "hello");
        assert_eq!(entry.identifier.0, "sshd");
        assert_eq!(entry.priority.0, "");
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

    #[test]
    fn enablement_describes_dbus_call_shape() {
        use super::Enablement;

        let enable = Enablement::Enable.unit_file_call("sshd.service");
        assert_eq!(enable.method, "EnableUnitFiles");
        assert_eq!(enable.followup_method, "StartUnit");
        assert_eq!(enable.args, json!([["sshd.service"], false, false]));
        assert_eq!(enable.changes_index, 1);

        let disable = Enablement::Disable.unit_file_call("sshd.service");
        assert_eq!(disable.method, "DisableUnitFiles");
        assert_eq!(disable.followup_method, "StopUnit");
        assert_eq!(disable.args, json!([["sshd.service"], false]));
        assert_eq!(disable.changes_index, 0);
    }
}
