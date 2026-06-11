//! Firewall management over firewalld (`org.fedoraproject.FirewallD1`).
//!
//! Reads (status/list/show/services) open an unprivileged `dbus-json3` channel;
//! mutations (add/remove service/port, set-default-zone, reload, confirm,
//! panic) open a privileged one and escalate. fez holds no state: the
//! runtime-vs-permanent split that guards against lockout is firewalld's own,
//! read live each call and committed only via `runtimeToPermanent`.

use crate::cli::{Cli, FirewallAction};
use crate::envelope::{ApiError, Envelope};
use crate::error::{is_service_unknown, FezError, Result};
use crate::protocol::client::BridgeClient;
use crate::transport;
use serde_json::{json, Value};

const FW_NAME: &str = "org.fedoraproject.FirewallD1";
const FW_PATH: &str = "/org/fedoraproject/FirewallD1";
const FW_IFACE: &str = "org.fedoraproject.FirewallD1";
const FW_ZONE_IFACE: &str = "org.fedoraproject.FirewallD1.zone";
const FW_CONFIG_PATH: &str = "/org/fedoraproject/FirewallD1/config";
const FW_CONFIG_IFACE: &str = "org.fedoraproject.FirewallD1.config";
const FW_CONFIG_ZONE_IFACE: &str = "org.fedoraproject.FirewallD1.config.zone";

/// A rendered capability result: the JSON payload plus its human form.
struct View {
    kind: &'static str,
    host: String,
    data: Value,
    human: String,
    hints: Option<Value>,
}

/// Route a parsed `firewall` action to its handler and return the exit code.
pub fn dispatch(cli: &Cli, action: &FirewallAction) -> i32 {
    let view = run(cli, action);
    render(cli, view)
}

/// The [`FezError::DependencyMissing`] returned when firewalld is absent.
fn dependency_missing() -> FezError {
    FezError::DependencyMissing {
        component: "firewalld".into(),
        dbus_name: FW_NAME.into(),
        remediation: "Install firewalld on the target (dnf install firewalld) and enable+start it (systemctl enable --now firewalld.service), then retry.".into(),
    }
}

/// Connect to the bridge and dispatch the requested action.
fn run(cli: &Cli, action: &FirewallAction) -> Result<View> {
    let transport = transport::from_host(cli.host.as_deref());
    let mut client = BridgeClient::connect(transport.as_ref())?;
    let host = client.host().to_string();
    match action {
        FirewallAction::Status => {
            let ch = open_channel(&mut client, false)?;
            status(&mut client, &ch, host)
        }
        FirewallAction::List => {
            let ch = open_channel(&mut client, false)?;
            list(&mut client, &ch, host)
        }
        FirewallAction::Show { zone } => {
            let ch = open_channel(&mut client, false)?;
            show(&mut client, &ch, host, zone)
        }
        FirewallAction::Services => {
            let ch = open_channel(&mut client, false)?;
            services(&mut client, &ch, host)
        }
        // Every mutation routes through the privileged path.
        _ => mutate(cli, &mut client, host, action),
    }
}

/// Open a firewalld `dbus-json3` channel (privileged for mutations).
///
/// firewalld activation failure (the service is absent) surfaces on the first
/// method call, not at open time; the caller probes it via [`fw_call`], which
/// maps ServiceUnknown to [`dependency_missing`]. A privileged open escalates
/// first and can itself fail with `AccessDenied` (exit 11).
///
/// # Errors
///
/// Propagates any channel-open or escalation error from the bridge client.
fn open_channel(client: &mut BridgeClient, privileged: bool) -> Result<String> {
    if privileged {
        client.dbus_open_privileged(FW_NAME)
    } else {
        client.dbus_open(FW_NAME)
    }
}

/// Call a firewalld method on the main object, mapping ServiceUnknown to the
/// dependency-missing error.
fn fw_call(
    client: &mut BridgeClient,
    channel: &str,
    iface: &str,
    method: &str,
    args: Value,
) -> Result<Value> {
    fw_call_path(client, channel, FW_PATH, iface, method, args)
}

/// Call a firewalld method on an explicit object path, mapping ServiceUnknown
/// to the dependency-missing error.
fn fw_call_path(
    client: &mut BridgeClient,
    channel: &str,
    path: &str,
    iface: &str,
    method: &str,
    args: Value,
) -> Result<Value> {
    match client.dbus_call(channel, path, iface, method, args) {
        Ok(v) => Ok(v),
        Err(FezError::Dbus { name, .. }) if is_service_unknown(&name) => Err(dependency_missing()),
        Err(e) => Err(e),
    }
}

/// First out-arg of a reply as a string array.
fn arg_str_vec(out: &Value) -> Vec<String> {
    out.get(0)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// First out-arg of a reply as a single string.
fn arg_str(out: &Value) -> String {
    out.get(0).and_then(Value::as_str).unwrap_or("").to_string()
}

/// First out-arg of a reply as a bool.
fn arg_bool(out: &Value) -> bool {
    out.get(0).and_then(Value::as_bool).unwrap_or(false)
}

/// Render a `getPorts` `aas` reply (each entry `[port, proto]`) as
/// `"port/proto"` labels.
fn ports_from_reply(out: &Value) -> Vec<String> {
    out.get(0)
        .and_then(Value::as_array)
        .map(|a| a.iter().map(port_label).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default()
}

/// Join a firewalld `[port, protocol]` entry into a `"port/proto"` label.
/// A malformed entry renders empty.
fn port_label(entry: &Value) -> String {
    let port = entry.get(0).and_then(Value::as_str).unwrap_or("");
    let proto = entry.get(1).and_then(Value::as_str).unwrap_or("");
    if port.is_empty() || proto.is_empty() {
        String::new()
    } else {
        format!("{port}/{proto}")
    }
}

/// Parse a `port/proto` spec into `(port, protocol)`.
///
/// # Errors
///
/// Returns [`FezError::NotFound`] when the spec is not `<u16>/<proto>` with a
/// non-empty protocol (used as a bad-argument signal; renders exit 4).
fn parse_port_spec(spec: &str) -> Result<(u16, String)> {
    let (port, proto) = spec
        .split_once('/')
        .ok_or_else(|| FezError::NotFound(format!("port spec {spec:?} (expected port/proto)")))?;
    let port: u16 = port
        .parse()
        .map_err(|_| FezError::NotFound(format!("port {port:?} (expected 1-65535)")))?;
    if proto.is_empty() {
        return Err(FezError::NotFound(format!(
            "port spec {spec:?} (empty protocol)"
        )));
    }
    Ok((port, proto.to_string()))
}

/// Compute runtime-vs-permanent drift as a list of `"+/-kind value"` tokens.
///
/// `+` means present at runtime but not permanent (would be lost on reload);
/// `-` means present permanent but not runtime (removed at runtime, not yet
/// committed). Stateless: both sides are read live each call.
fn compute_drift(
    runtime_services: &[String],
    permanent_services: &[String],
    runtime_ports: &[String],
    permanent_ports: &[String],
) -> Vec<String> {
    let mut drift = Vec::new();
    for s in runtime_services {
        if !permanent_services.contains(s) {
            drift.push(format!("+service {s}"));
        }
    }
    for s in permanent_services {
        if !runtime_services.contains(s) {
            drift.push(format!("-service {s}"));
        }
    }
    for p in runtime_ports {
        if !permanent_ports.contains(p) {
            drift.push(format!("+port {p}"));
        }
    }
    for p in permanent_ports {
        if !runtime_ports.contains(p) {
            drift.push(format!("-port {p}"));
        }
    }
    drift
}

/// Read the permanent (`config`) services and ports for a zone, for drift.
fn permanent_zone(
    client: &mut BridgeClient,
    channel: &str,
    zone: &str,
) -> Result<(Vec<String>, Vec<String>)> {
    let obj = fw_call_path(
        client,
        channel,
        FW_CONFIG_PATH,
        FW_CONFIG_IFACE,
        "getZoneByName",
        json!([zone]),
    )?;
    let zone_path = arg_str(&obj);
    let services = arg_str_vec(&fw_call_path(
        client,
        channel,
        &zone_path,
        FW_CONFIG_ZONE_IFACE,
        "getServices",
        json!([]),
    )?);
    let ports = ports_from_reply(&fw_call_path(
        client,
        channel,
        &zone_path,
        FW_CONFIG_ZONE_IFACE,
        "getPorts",
        json!([]),
    )?);
    Ok((services, ports))
}

/// Read the runtime services and ports for a zone.
fn runtime_zone(
    client: &mut BridgeClient,
    channel: &str,
    zone: &str,
) -> Result<(Vec<String>, Vec<String>)> {
    let services = arg_str_vec(&fw_call(
        client,
        channel,
        FW_ZONE_IFACE,
        "getServices",
        json!([zone]),
    )?);
    let ports = ports_from_reply(&fw_call(
        client,
        channel,
        FW_ZONE_IFACE,
        "getPorts",
        json!([zone]),
    )?);
    Ok((services, ports))
}

/// `firewall status`: state, default zone, panic flag, and pending drift.
fn status(client: &mut BridgeClient, channel: &str, host: String) -> Result<View> {
    let default_zone = arg_str(&fw_call(
        client,
        channel,
        FW_IFACE,
        "getDefaultZone",
        json!([]),
    )?);
    let panic = arg_bool(&fw_call(
        client,
        channel,
        FW_IFACE,
        "queryPanicMode",
        json!([]),
    )?);
    let (rt_services, rt_ports) = runtime_zone(client, channel, &default_zone)?;
    let (perm_services, perm_ports) = permanent_zone(client, channel, &default_zone)?;
    let drift = compute_drift(&rt_services, &perm_services, &rt_ports, &perm_ports);

    let data = json!({
        "running": true,
        "default_zone": default_zone,
        "panic_mode": panic,
        "pending_changes": drift,
    });
    let mut human = format!(
        "running:       yes\ndefault zone:  {default_zone}\npanic mode:    {}\n",
        if panic { "on" } else { "off" }
    );
    if drift.is_empty() {
        human.push_str("pending:       none\n");
    } else {
        human.push_str(&format!("pending:       {}\n", drift.join(", ")));
    }
    let hints = if drift.is_empty() {
        None
    } else {
        Some(json!({
            "warning": "uncommitted runtime changes; run `fez firewall confirm` to persist or `fez firewall reload` to discard",
            "pending": drift,
        }))
    };
    Ok(View {
        kind: "FirewallStatus",
        host,
        data,
        human,
        hints,
    })
}

/// `firewall list`: every zone with a per-zone summary.
fn list(client: &mut BridgeClient, channel: &str, host: String) -> Result<View> {
    let zones = arg_str_vec(&fw_call(client, channel, FW_IFACE, "getZones", json!([]))?);
    let default_zone = arg_str(&fw_call(
        client,
        channel,
        FW_IFACE,
        "getDefaultZone",
        json!([]),
    )?);

    let columns = ["zone", "default", "services", "ports", "interfaces"];
    let mut rows = Vec::new();
    let mut human = format!(
        "{:<12} {:<8} {:<24} {:<16} {}\n",
        "ZONE", "DEFAULT", "SERVICES", "PORTS", "INTERFACES"
    );
    for zone in &zones {
        let (services, ports) = runtime_zone(client, channel, zone)?;
        let interfaces = arg_str_vec(&fw_call(
            client,
            channel,
            FW_ZONE_IFACE,
            "getInterfaces",
            json!([zone]),
        )?);
        let is_default = *zone == default_zone;
        human.push_str(&format!(
            "{:<12} {:<8} {:<24} {:<16} {}\n",
            zone,
            if is_default { "yes" } else { "" },
            services.join(","),
            ports.join(","),
            interfaces.join(","),
        ));
        rows.push(json!([
            zone,
            is_default,
            services.join(","),
            ports.join(","),
            interfaces.join(","),
        ]));
    }
    Ok(View {
        kind: "FirewallZoneList",
        host,
        data: crate::envelope::table_data(&columns, rows),
        human,
        hints: None,
    })
}

/// `firewall show <zone>`: one zone's full detail.
fn show(client: &mut BridgeClient, channel: &str, host: String, zone: &str) -> Result<View> {
    let zones = arg_str_vec(&fw_call(client, channel, FW_IFACE, "getZones", json!([]))?);
    if !zones.iter().any(|z| z == zone) {
        return Err(FezError::NotFound(format!("firewall zone {zone}")));
    }
    let (services, ports) = runtime_zone(client, channel, zone)?;
    let interfaces = arg_str_vec(&fw_call(
        client,
        channel,
        FW_ZONE_IFACE,
        "getInterfaces",
        json!([zone]),
    )?);
    let sources = arg_str_vec(&fw_call(
        client,
        channel,
        FW_ZONE_IFACE,
        "getSources",
        json!([zone]),
    )?);
    let data = json!({
        "zone": zone,
        "services": services,
        "ports": ports,
        "interfaces": interfaces,
        "sources": sources,
    });
    let human = format!(
        "Zone:       {zone}\nServices:   {}\nPorts:      {}\nInterfaces: {}\nSources:    {}\n",
        services.join(", "),
        ports.join(", "),
        interfaces.join(", "),
        sources.join(", "),
    );
    Ok(View {
        kind: "FirewallZone",
        host,
        data,
        human,
        hints: None,
    })
}

/// `firewall services`: the service catalog firewalld knows about.
fn services(client: &mut BridgeClient, channel: &str, host: String) -> Result<View> {
    let mut catalog = arg_str_vec(&fw_call(
        client,
        channel,
        FW_IFACE,
        "listServices",
        json!([]),
    )?);
    catalog.sort();
    let mut human = String::new();
    for s in &catalog {
        human.push_str(s);
        human.push('\n');
    }
    Ok(View {
        kind: "FirewallServiceCatalog",
        host,
        data: json!({ "services": catalog }),
        human,
        hints: None,
    })
}

/// The set of firewall services treated as session-critical (always `ssh`).
fn session_services() -> Vec<String> {
    vec!["ssh".to_string()]
}

/// Parse the server-side port (4th field) out of an `SSH_CONNECTION` value.
fn session_port_from(ssh_connection: &str) -> Option<u16> {
    ssh_connection.split_whitespace().nth(3)?.parse().ok()
}

/// The session-critical port set, derived live from `$SSH_CONNECTION`.
/// Empty when fez is invoked locally (no SSH session).
fn session_ports() -> Vec<u16> {
    std::env::var("SSH_CONNECTION")
        .ok()
        .and_then(|c| session_port_from(&c))
        .into_iter()
        .collect()
}

/// Resolve the effective zone for a mutation: the `--zone` flag, or the live
/// default zone when omitted.
fn effective_zone(
    client: &mut BridgeClient,
    channel: &str,
    requested: &Option<String>,
) -> Result<String> {
    match requested {
        Some(z) => Ok(z.clone()),
        None => Ok(arg_str(&fw_call(
            client,
            channel,
            FW_IFACE,
            "getDefaultZone",
            json!([]),
        )?)),
    }
}

/// Run a privileged firewalld mutation: open the privileged channel, apply the
/// protected guards, audit attempt/result around the runtime-only call, and
/// attach the confirm hint.
fn mutate(
    cli: &Cli,
    client: &mut BridgeClient,
    host: String,
    action: &FirewallAction,
) -> Result<View> {
    let channel = open_channel(client, true)?;

    match action {
        FirewallAction::AddService {
            service,
            zone,
            timeout,
        } => {
            let zone = effective_zone(client, &channel, zone)?;
            let t = i64::from(timeout.unwrap_or(0));
            run_audited(
                client,
                &channel,
                &host,
                "add-service",
                &format!("{zone}:{service}"),
                FW_ZONE_IFACE,
                "addService",
                json!([zone, service, t]),
            )?;
            Ok(change_view(
                host,
                "add-service",
                &zone,
                &format!("service {service}"),
                *timeout,
            ))
        }
        FirewallAction::RemoveService { service, zone } => {
            let zone = effective_zone(client, &channel, zone)?;
            crate::safety::check_firewall_service_removal(service, &session_services(), cli.force)?;
            run_audited(
                client,
                &channel,
                &host,
                "remove-service",
                &format!("{zone}:{service}"),
                FW_ZONE_IFACE,
                "removeService",
                json!([zone, service]),
            )?;
            Ok(change_view(
                host,
                "remove-service",
                &zone,
                &format!("service {service}"),
                None,
            ))
        }
        FirewallAction::AddPort {
            port,
            zone,
            timeout,
        } => {
            let (p, proto) = parse_port_spec(port)?;
            let zone = effective_zone(client, &channel, zone)?;
            let t = i64::from(timeout.unwrap_or(0));
            run_audited(
                client,
                &channel,
                &host,
                "add-port",
                &format!("{zone}:{p}/{proto}"),
                FW_ZONE_IFACE,
                "addPort",
                json!([zone, p.to_string(), proto, t]),
            )?;
            Ok(change_view(
                host,
                "add-port",
                &zone,
                &format!("port {p}/{proto}"),
                *timeout,
            ))
        }
        FirewallAction::RemovePort { port, zone } => {
            let (p, proto) = parse_port_spec(port)?;
            let zone = effective_zone(client, &channel, zone)?;
            crate::safety::check_firewall_port_removal(p, &session_ports(), cli.force)?;
            run_audited(
                client,
                &channel,
                &host,
                "remove-port",
                &format!("{zone}:{p}/{proto}"),
                FW_ZONE_IFACE,
                "removePort",
                json!([zone, p.to_string(), proto]),
            )?;
            Ok(change_view(
                host,
                "remove-port",
                &zone,
                &format!("port {p}/{proto}"),
                None,
            ))
        }
        FirewallAction::SetDefaultZone { zone } => {
            crate::safety::check_firewall_default_zone(cli.force)?;
            run_audited(
                client,
                &channel,
                &host,
                "set-default-zone",
                zone,
                FW_IFACE,
                "setDefaultZone",
                json!([zone]),
            )?;
            Ok(change_view(
                host,
                "set-default-zone",
                zone,
                "default zone",
                None,
            ))
        }
        FirewallAction::Reload => {
            let default_zone = arg_str(&fw_call(
                client,
                &channel,
                FW_IFACE,
                "getDefaultZone",
                json!([]),
            )?);
            let (rt_s, rt_p) = runtime_zone(client, &channel, &default_zone)?;
            let (pm_s, pm_p) = permanent_zone(client, &channel, &default_zone)?;
            let has_drift = !compute_drift(&rt_s, &pm_s, &rt_p, &pm_p).is_empty();
            crate::safety::check_firewall_reload(has_drift, cli.force)?;
            run_audited(
                client,
                &channel,
                &host,
                "reload",
                "firewall",
                FW_IFACE,
                "reload",
                json!([]),
            )?;
            Ok(reload_view(host))
        }
        FirewallAction::Confirm => {
            run_audited(
                client,
                &channel,
                &host,
                "confirm",
                "firewall",
                FW_IFACE,
                "runtimeToPermanent",
                json!([]),
            )?;
            Ok(confirm_view(host))
        }
        FirewallAction::Panic { state } => {
            let on = state == "on";
            if on {
                crate::safety::check_firewall_panic_on(cli.force)?;
            }
            let method = if on {
                "enablePanicMode"
            } else {
                "disablePanicMode"
            };
            run_audited(
                client,
                &channel,
                &host,
                &format!("panic-{state}"),
                "firewall",
                FW_IFACE,
                method,
                json!([]),
            )?;
            Ok(panic_view(host, on))
        }
        // Reads are dispatched in `run`; they never reach `mutate`. Return a
        // defensive error rather than panicking, so a future refactor that
        // reroutes a read here fails gracefully instead of aborting.
        FirewallAction::Status
        | FirewallAction::List
        | FirewallAction::Show { .. }
        | FirewallAction::Services => Err(FezError::Problem("read action routed to mutate".into())),
    }
}

/// Audit the attempt, run the runtime-only firewalld call, audit the result.
#[allow(clippy::too_many_arguments)]
fn run_audited(
    client: &mut BridgeClient,
    channel: &str,
    host: &str,
    operation: &str,
    target: &str,
    iface: &str,
    method: &str,
    args: Value,
) -> Result<()> {
    let sink = crate::audit::sink_from_env();
    let ctx = crate::audit::AuditContext::new(
        &crate::audit::actor(),
        host,
        operation,
        target,
        &crate::audit::correlation_id(),
    );
    sink.write(&ctx.record(crate::audit::Outcome::Attempt));
    let exec = fw_call(client, channel, iface, method, args);
    match &exec {
        Ok(_) => sink.write(&ctx.record(crate::audit::Outcome::Ok)),
        Err(e) => sink.write(&ctx.record(crate::audit::Outcome::Error(e.to_string()))),
    }
    exec.map(|_| ())
}

/// The standard "runtime-only; confirm to persist" hint.
fn confirm_hint() -> Value {
    json!({
        "persisted": false,
        "note": "runtime-only change; run `fez firewall confirm` to persist it",
    })
}

/// Build the `FirewallChange` view for an add/remove/set mutation.
fn change_view(host: String, op: &str, zone: &str, what: &str, timeout: Option<u32>) -> View {
    let mut data = json!({
        "operation": op,
        "zone": zone,
        "change": what,
        "persisted": false,
    });
    if let Some(t) = timeout {
        data["timeout"] = json!(t);
    }
    let human = format!("{op} {what} in zone {zone} (runtime only)\n");
    View {
        kind: "FirewallChange",
        host,
        data,
        human,
        hints: Some(confirm_hint()),
    }
}

/// Build the `FirewallChange` view for `reload`.
fn reload_view(host: String) -> View {
    View {
        kind: "FirewallChange",
        host,
        data: json!({"operation": "reload", "persisted": true}),
        human: "reloaded permanent config into runtime\n".into(),
        hints: None,
    }
}

/// Build the `FirewallConfirm` view for `confirm`.
fn confirm_view(host: String) -> View {
    View {
        kind: "FirewallConfirm",
        host,
        data: json!({"operation": "confirm", "persisted": true}),
        human: "runtime config committed to permanent\n".into(),
        hints: None,
    }
}

/// Build the `FirewallChange` view for `panic on|off`.
fn panic_view(host: String, on: bool) -> View {
    View {
        kind: "FirewallChange",
        host,
        data: json!({"operation": "panic", "panic_mode": on, "persisted": false}),
        human: format!("panic mode {}\n", if on { "enabled" } else { "disabled" }),
        hints: None,
    }
}

/// Render a [`View`] (or error) to stdout/stderr and return the exit code.
fn render(cli: &Cli, result: Result<View>) -> i32 {
    let host = cli.resolved_host();
    match result {
        Ok(view) => {
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
                        detail: error_detail(&e),
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

/// Structured `detail` for the error envelope, for errors that carry one.
fn error_detail(e: &FezError) -> Option<Value> {
    match e {
        FezError::DependencyMissing {
            component,
            dbus_name,
            remediation,
        } => Some(json!({
            "component": component,
            "dbusName": dbus_name,
            "remediation": remediation,
        })),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_port_spec_splits_port_and_proto() {
        assert_eq!(
            parse_port_spec("8080/tcp").unwrap(),
            (8080, "tcp".to_string())
        );
        assert_eq!(parse_port_spec("53/udp").unwrap(), (53, "udp".to_string()));
    }

    #[test]
    fn parse_port_spec_rejects_garbage() {
        assert!(parse_port_spec("nope").is_err());
        assert!(parse_port_spec("8080").is_err());
        assert!(parse_port_spec("99999/tcp").is_err()); // out of u16 range
        assert!(parse_port_spec("80/").is_err());
    }

    #[test]
    fn port_label_joins_port_and_proto() {
        assert_eq!(port_label(&json!(["9090", "tcp"])), "9090/tcp");
        // A malformed entry renders empty rather than panicking.
        assert_eq!(port_label(&json!([])), "");
    }

    #[test]
    fn drift_reports_runtime_only_ports() {
        // runtime has 9090/tcp + ssh; permanent has only ssh -> one added port.
        let runtime_ports = vec!["9090/tcp".to_string()];
        let permanent_ports: Vec<String> = vec![];
        let runtime_services = vec!["ssh".to_string()];
        let permanent_services = vec!["ssh".to_string()];
        let drift = compute_drift(
            &runtime_services,
            &permanent_services,
            &runtime_ports,
            &permanent_ports,
        );
        assert_eq!(drift, vec!["+port 9090/tcp".to_string()]);
    }

    #[test]
    fn drift_empty_when_runtime_matches_permanent() {
        let s = vec!["ssh".to_string()];
        let p: Vec<String> = vec![];
        assert!(compute_drift(&s, &s, &p, &p).is_empty());
    }

    #[test]
    fn drift_reports_removed_service() {
        // permanent has http but runtime does not -> service removed at runtime.
        let runtime_services: Vec<String> = vec![];
        let permanent_services = vec!["http".to_string()];
        let p: Vec<String> = vec![];
        let drift = compute_drift(&runtime_services, &permanent_services, &p, &p);
        assert_eq!(drift, vec!["-service http".to_string()]);
    }

    #[test]
    fn session_port_parses_ssh_connection() {
        // SSH_CONNECTION = "client_ip client_port server_ip server_port".
        assert_eq!(session_port_from("10.0.0.1 5520 10.0.0.2 22"), Some(22));
        assert_eq!(session_port_from("10.0.0.1 5520 10.0.0.2 2222"), Some(2222));
    }

    #[test]
    fn session_port_none_when_absent_or_malformed() {
        assert_eq!(session_port_from(""), None);
        assert_eq!(session_port_from("garbage"), None);
        assert_eq!(session_port_from("a b c notaport"), None);
    }

    #[test]
    fn session_services_always_includes_ssh() {
        assert_eq!(session_services(), vec!["ssh".to_string()]);
    }
}
