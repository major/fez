//! Firewall management over firewalld (`org.fedoraproject.FirewallD1`).
//!
//! Reads (status/list/show/services) open an unprivileged `dbus-json3` channel;
//! mutations (add/remove service/port, set-default-zone, reload, confirm,
//! panic) open a privileged one and escalate. fez holds no state: the
//! runtime-vs-permanent split that guards against lockout is firewalld's own,
//! read live each call and committed only via `runtimeToPermanent`.

use crate::capabilities::{render_with_hints, View};
use crate::cli::{Cli, FirewallAction};
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

/// Route a parsed `firewall` action to its handler and return the exit code.
pub fn dispatch(cli: &Cli, action: &FirewallAction) -> i32 {
    let view = run(cli, action);
    render_with_hints(cli, view, error_hints)
}

/// Safe read-only follow-up hints for an actionable firewall error (issue #60).
///
/// A `dependency-missing` failure points at the service-status check (fez
/// cannot tell absent from stopped, so the hint covers both); an
/// `unsupported-api` failure tells the caller the feature is unavailable on
/// this firewalld and not to retry. Other errors carry no firewall-specific
/// hint.
fn error_hints(e: &FezError) -> Option<Value> {
    match e {
        FezError::DependencyMissing { .. } => Some(json!({
            "checkService": "fez services status firewalld.service --json",
            "install": "dnf install firewalld",
        })),
        FezError::UnsupportedApi(method) => Some(json!({
            "unsupported": format!(
                "firewalld on this host does not expose {method}; treat the feature as unsupported"
            ),
        })),
        _ => None,
    }
}

/// The [`FezError::DependencyMissing`] returned when firewalld is absent.
fn dependency_missing() -> FezError {
    FezError::DependencyMissing {
        component: "firewalld".into(),
        dbus_name: FW_NAME.into(),
        remediation: "Install firewalld on the target (dnf install firewalld) and enable+start it (systemctl enable --now firewalld.service), then retry.".into(),
    }
}

/// A read-only firewall subcommand and its borrowed arguments.
#[derive(Debug, PartialEq, Eq)]
enum ReadAction<'a> {
    Status,
    List,
    Show { zone: &'a str },
    Services,
}

/// A firewall mutation subcommand and its borrowed arguments.
#[derive(Debug, PartialEq, Eq)]
enum Mutation<'a> {
    AddService {
        service: &'a str,
        zone: Option<&'a str>,
        timeout: Option<u32>,
    },
    RemoveService {
        service: &'a str,
        zone: Option<&'a str>,
    },
    AddPort {
        port: &'a str,
        zone: Option<&'a str>,
        timeout: Option<u32>,
    },
    RemovePort {
        port: &'a str,
        zone: Option<&'a str>,
    },
    SetDefaultZone {
        zone: &'a str,
    },
    Reload,
    Confirm,
    Panic {
        state: &'a str,
    },
    Masquerade {
        state: &'a str,
        zone: Option<&'a str>,
        timeout: Option<u32>,
    },
}

/// The read/mutate split of a parsed [`FirewallAction`].
#[derive(Debug, PartialEq, Eq)]
enum Plan<'a> {
    Read(ReadAction<'a>),
    Mutate(Mutation<'a>),
}

/// Map the flat clap enum onto a typed read/mutate plan.
fn classify(action: &FirewallAction) -> Plan<'_> {
    match action {
        FirewallAction::Status => Plan::Read(ReadAction::Status),
        FirewallAction::List => Plan::Read(ReadAction::List),
        FirewallAction::Show { zone } => Plan::Read(ReadAction::Show { zone }),
        FirewallAction::Services => Plan::Read(ReadAction::Services),
        FirewallAction::AddService {
            service,
            zone,
            timeout,
        } => Plan::Mutate(Mutation::AddService {
            service,
            zone: zone.as_deref(),
            timeout: *timeout,
        }),
        FirewallAction::RemoveService { service, zone } => Plan::Mutate(Mutation::RemoveService {
            service,
            zone: zone.as_deref(),
        }),
        FirewallAction::AddPort {
            port,
            zone,
            timeout,
        } => Plan::Mutate(Mutation::AddPort {
            port,
            zone: zone.as_deref(),
            timeout: *timeout,
        }),
        FirewallAction::RemovePort { port, zone } => Plan::Mutate(Mutation::RemovePort {
            port,
            zone: zone.as_deref(),
        }),
        FirewallAction::SetDefaultZone { zone } => Plan::Mutate(Mutation::SetDefaultZone { zone }),
        FirewallAction::Reload => Plan::Mutate(Mutation::Reload),
        FirewallAction::Confirm => Plan::Mutate(Mutation::Confirm),
        FirewallAction::Panic { state } => Plan::Mutate(Mutation::Panic { state }),
        FirewallAction::Masquerade {
            state,
            zone,
            timeout,
        } => Plan::Mutate(Mutation::Masquerade {
            state,
            zone: zone.as_deref(),
            timeout: *timeout,
        }),
    }
}

/// Connect to the bridge and dispatch the requested action.
fn run(cli: &Cli, action: &FirewallAction) -> Result<View> {
    let transport = transport::from_host(cli.host.as_deref());
    let mut client = BridgeClient::connect(transport.as_ref())?;
    let host = client.host().to_string();
    match classify(action) {
        Plan::Read(ReadAction::Status) => {
            let ch = open_channel(&mut client, false)?;
            status(&mut client, &ch, host)
        }
        Plan::Read(ReadAction::List) => {
            let ch = open_channel(&mut client, false)?;
            list(&mut client, &ch, host)
        }
        Plan::Read(ReadAction::Show { zone }) => {
            let ch = open_channel(&mut client, false)?;
            show(&mut client, &ch, host, zone)
        }
        Plan::Read(ReadAction::Services) => {
            let ch = open_channel(&mut client, false)?;
            services(&mut client, &ch, host)
        }
        Plan::Mutate(mutation) => mutate(cli, &mut client, host, mutation),
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

/// Call a firewalld method on an explicit object path, mapping low-level
/// transport/D-Bus failures to actionable firewall errors via [`map_fw_error`].
fn fw_call_path(
    client: &mut BridgeClient,
    channel: &str,
    path: &str,
    iface: &str,
    method: &str,
    args: Value,
) -> Result<Value> {
    client
        .dbus_call(channel, path, iface, method, args)
        .map_err(|e| map_fw_error(e, method))
}

/// Map a raw bridge/D-Bus failure to an actionable firewall error (issue #60).
///
/// firewalld is D-Bus-activated, so an absent or failed service is not
/// observably distinct from "installed but stopped": both surface as the name
/// being unreachable. We therefore collapse all of those to
/// [`dependency_missing`] (whose remediation covers install **and**
/// enable+start) rather than inventing a `service-inactive` code fez cannot
/// reliably detect:
/// - `Dbus { ServiceUnknown | NameHasNoOwner }`: name not activatable.
/// - `Problem("not-found")`: cockpit closed the channel because the name could
///   not be reached (the symptom reported in #60).
/// - `Problem("not-supported")`: the bus refused the name.
///
/// A `Dbus { UnknownMethod }` means firewalld is reachable but too old to
/// expose the method; that maps to [`FezError::UnsupportedApi`] carrying the
/// method name, so a caller treats the feature as unsupported instead of
/// recommending an install. All other errors pass through unchanged, so the
/// raw cause is preserved when it is already actionable (e.g. `AccessDenied`).
fn map_fw_error(e: FezError, method: &str) -> FezError {
    match e {
        FezError::Dbus { ref name, .. } if is_service_unknown(name) => dependency_missing(),
        FezError::Dbus { ref name, .. } if name.contains("UnknownMethod") => {
            FezError::UnsupportedApi(method.to_string())
        }
        FezError::Problem(ref p) if p == "not-found" || p == "not-supported" => {
            dependency_missing()
        }
        other => other,
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

#[derive(Debug, PartialEq, Eq)]
struct PortSpec {
    port: u16,
    proto: String,
}

impl PortSpec {
    fn label(&self) -> String {
        format!("{}/{}", self.port, self.proto)
    }
}

/// Parse a `port/proto` spec.
///
/// # Errors
///
/// Returns [`FezError::NotFound`] when the spec is not `<u16>/<proto>` with a
/// non-empty protocol (used as a bad-argument signal; renders exit 4).
fn parse_port_spec(spec: &str) -> Result<PortSpec> {
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
    Ok(PortSpec {
        port,
        proto: proto.to_string(),
    })
}

/// Compute runtime-vs-permanent drift as a list of `"+/-kind value"` tokens.
///
/// `+` means present at runtime but not permanent (would be lost on reload);
/// `-` means present permanent but not runtime (removed at runtime, not yet
/// committed). Covers services, ports, and masquerade. Stateless: both sides
/// are read live each call.
fn compute_drift(
    runtime_services: &[String],
    permanent_services: &[String],
    runtime_ports: &[String],
    permanent_ports: &[String],
    runtime_masquerade: bool,
    permanent_masquerade: bool,
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
    if runtime_masquerade && !permanent_masquerade {
        drift.push("+masquerade".to_string());
    }
    if permanent_masquerade && !runtime_masquerade {
        drift.push("-masquerade".to_string());
    }
    drift
}

/// Whether a permanent-config read failed because firewalld rejected the
/// `config.info` polkit action. This is distinct from failing to open a
/// privileged cockpit channel: the read reached firewalld, but firewalld denied
/// the config API.
fn is_config_info_denied(e: &FezError) -> bool {
    match e {
        FezError::Dbus { name, message } => {
            name.contains("NotAuthorized")
                || name.contains("AccessDenied")
                || message.contains("config.info")
        }
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FirewallStatusData {
    running: bool,
    default_zone: String,
    panic_mode: bool,
    masquerade: bool,
    pending_changes: Vec<String>,
    pending_changes_available: bool,
}

impl FirewallStatusData {
    fn from_runtime(
        default_zone: String,
        panic_mode: bool,
        masquerade: bool,
        drift: Option<Vec<String>>,
    ) -> Self {
        let (pending_changes, pending_changes_available) = match drift {
            Some(pending_changes) => (pending_changes, true),
            None => (Vec::new(), false),
        };
        Self {
            running: true,
            default_zone,
            panic_mode,
            masquerade,
            pending_changes,
            pending_changes_available,
        }
    }

    fn data(&self) -> Value {
        json!({
            "running": self.running,
            "default_zone": self.default_zone,
            "panic_mode": self.panic_mode,
            "masquerade": self.masquerade,
            "pending_changes": self.pending_changes,
            "pending_changes_available": self.pending_changes_available,
        })
    }

    fn human_prefix(&self) -> String {
        format!(
            "running:       {}\ndefault zone:  {}\npanic mode:    {}\nmasquerade:    {}\n",
            if self.running { "yes" } else { "no" },
            self.default_zone,
            if self.panic_mode { "on" } else { "off" },
            if self.masquerade { "on" } else { "off" }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeZone {
    services: Vec<String>,
    ports: Vec<String>,
    masquerade: bool,
}

impl RuntimeZone {
    fn new(services: Vec<String>, ports: Vec<String>, masquerade: bool) -> Self {
        Self {
            services,
            ports,
            masquerade,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FirewallZoneSummary {
    zone: String,
    is_default: bool,
    services: Vec<String>,
    ports: Vec<String>,
    interfaces: Vec<String>,
}

impl FirewallZoneSummary {
    fn row(&self) -> Value {
        json!([
            self.zone,
            self.is_default,
            self.services.join(","),
            self.ports.join(","),
            self.interfaces.join(","),
        ])
    }

    fn human_row(&self) -> String {
        format!(
            "{:<12} {:<8} {:<24} {:<16} {}\n",
            self.zone,
            if self.is_default { "yes" } else { "" },
            self.services.join(","),
            self.ports.join(","),
            self.interfaces.join(","),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FirewallZoneDetail {
    zone: String,
    services: Vec<String>,
    ports: Vec<String>,
    interfaces: Vec<String>,
    sources: Vec<String>,
    masquerade: bool,
}

impl FirewallZoneDetail {
    fn data(&self) -> Value {
        json!({
            "zone": self.zone,
            "services": self.services,
            "ports": self.ports,
            "interfaces": self.interfaces,
            "sources": self.sources,
            "masquerade": self.masquerade,
        })
    }

    fn human(&self) -> String {
        format!(
            "Zone:       {}\nServices:   {}\nPorts:      {}\nInterfaces: {}\nSources:    {}\nMasquerade: {}\n",
            self.zone,
            self.services.join(", "),
            self.ports.join(", "),
            self.interfaces.join(", "),
            self.sources.join(", "),
            if self.masquerade { "on" } else { "off" },
        )
    }
}

/// Read the permanent (`config`) services, ports, and masquerade for a zone,
/// for drift.
fn permanent_zone(client: &mut BridgeClient, channel: &str, zone: &str) -> Result<RuntimeZone> {
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
    let masquerade = arg_bool(&fw_call_path(
        client,
        channel,
        &zone_path,
        FW_CONFIG_ZONE_IFACE,
        "getMasquerade",
        json!([]),
    )?);
    Ok(RuntimeZone::new(services, ports, masquerade))
}

/// Read the runtime services, ports, and masquerade for a zone.
fn runtime_zone(client: &mut BridgeClient, channel: &str, zone: &str) -> Result<RuntimeZone> {
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
    let masquerade = arg_bool(&fw_call(
        client,
        channel,
        FW_ZONE_IFACE,
        "getMasquerade",
        json!([zone]),
    )?);
    Ok(RuntimeZone::new(services, ports, masquerade))
}

/// `firewall status`: state, default zone, panic flag, and pending drift.
///
/// Runtime reads (default zone, panic flag, the runtime zone's services/ports)
/// go over the unprivileged `channel`. The permanent-config read needed for
/// drift is polkit-gated (firewalld's `PK_ACTION_CONFIG`, `auth_admin_keep` on
/// both server and desktop installs), so it is issued on a separate privileged
/// channel that escalates first. A host with no usable escalation mechanism
/// therefore fails `status` with `access-denied` (exit 11) rather than
/// silently reporting empty drift.
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
    let runtime = runtime_zone(client, channel, &default_zone)?;
    // Permanent config is polkit-gated; read it on a privileged channel.
    let priv_channel = open_channel(client, true)?;
    let drift = match permanent_zone(client, &priv_channel, &default_zone) {
        Ok(permanent) => Some(compute_drift(
            &runtime.services,
            &permanent.services,
            &runtime.ports,
            &permanent.ports,
            runtime.masquerade,
            permanent.masquerade,
        )),
        Err(e) if is_config_info_denied(&e) => None,
        Err(e) => return Err(e),
    };
    let status = FirewallStatusData::from_runtime(default_zone, panic, runtime.masquerade, drift);
    let data = status.data();
    let mut human = status.human_prefix();
    let hints = if !status.pending_changes_available {
        human.push_str("pending:       unavailable (permanent config not readable)\n");
        Some(json!({
            "warning": "permanent firewall config was not readable; runtime status is shown but pending_changes may be incomplete",
            "follow_up": "Check firewalld config.info authorization for the target user or run `fez firewall status` from a context allowed by polkit."
        }))
    } else if status.pending_changes.is_empty() {
        human.push_str("pending:       none\n");
        None
    } else {
        human.push_str(&format!(
            "pending:       {}\n",
            status.pending_changes.join(", ")
        ));
        Some(json!({
            "warning": "uncommitted runtime changes; run `fez firewall confirm` to persist or `fez firewall reload` to discard",
            "pending": status.pending_changes,
        }))
    };
    Ok(View::new("FirewallStatus", host, data, human).with_hints_opt(hints))
}

/// `firewall list`: every zone with a per-zone summary.
fn list(client: &mut BridgeClient, channel: &str, host: String) -> Result<View> {
    let zones = arg_str_vec(&fw_call(
        client,
        channel,
        FW_ZONE_IFACE,
        "getZones",
        json!([]),
    )?);
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
        let runtime = runtime_zone(client, channel, zone)?;
        let interfaces = arg_str_vec(&fw_call(
            client,
            channel,
            FW_ZONE_IFACE,
            "getInterfaces",
            json!([zone]),
        )?);
        let summary = FirewallZoneSummary {
            zone: zone.clone(),
            is_default: *zone == default_zone,
            services: runtime.services,
            ports: runtime.ports,
            interfaces,
        };
        human.push_str(&summary.human_row());
        rows.push(summary.row());
    }
    Ok(View::new(
        "FirewallZoneList",
        host,
        crate::envelope::table_data(&columns, rows),
        human,
    ))
}

/// `firewall show <zone>`: one zone's full detail.
fn show(client: &mut BridgeClient, channel: &str, host: String, zone: &str) -> Result<View> {
    let zones = arg_str_vec(&fw_call(
        client,
        channel,
        FW_ZONE_IFACE,
        "getZones",
        json!([]),
    )?);
    if !zones.iter().any(|z| z == zone) {
        return Err(FezError::NotFound(format!("firewall zone {zone}")));
    }
    let runtime = runtime_zone(client, channel, zone)?;
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
    let detail = FirewallZoneDetail {
        zone: zone.to_string(),
        services: runtime.services,
        ports: runtime.ports,
        interfaces,
        sources,
        masquerade: runtime.masquerade,
    };
    Ok(View::new(
        "FirewallZone",
        host,
        detail.data(),
        detail.human(),
    ))
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
    Ok(View::new(
        "FirewallServiceCatalog",
        host,
        json!({ "services": catalog }),
        human,
    ))
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
    requested: Option<&str>,
) -> Result<String> {
    match requested {
        Some(z) => Ok(z.to_string()),
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
    action: Mutation<'_>,
) -> Result<View> {
    let channel = open_channel(client, true)?;

    match action {
        Mutation::AddService {
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
                timeout,
            ))
        }
        Mutation::RemoveService { service, zone } => {
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
        Mutation::AddPort {
            port,
            zone,
            timeout,
        } => {
            let spec = parse_port_spec(port)?;
            let zone = effective_zone(client, &channel, zone)?;
            let t = i64::from(timeout.unwrap_or(0));
            let label = spec.label();
            run_audited(
                client,
                &channel,
                &host,
                "add-port",
                &format!("{zone}:{label}"),
                FW_ZONE_IFACE,
                "addPort",
                json!([zone, spec.port.to_string(), spec.proto, t]),
            )?;
            Ok(change_view(
                host,
                "add-port",
                &zone,
                &format!("port {label}"),
                timeout,
            ))
        }
        Mutation::RemovePort { port, zone } => {
            let spec = parse_port_spec(port)?;
            let zone = effective_zone(client, &channel, zone)?;
            crate::safety::check_firewall_port_removal(spec.port, &session_ports(), cli.force)?;
            let label = spec.label();
            run_audited(
                client,
                &channel,
                &host,
                "remove-port",
                &format!("{zone}:{label}"),
                FW_ZONE_IFACE,
                "removePort",
                json!([zone, spec.port.to_string(), spec.proto]),
            )?;
            Ok(change_view(
                host,
                "remove-port",
                &zone,
                &format!("port {label}"),
                None,
            ))
        }
        Mutation::SetDefaultZone { zone } => {
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
        Mutation::Reload => {
            let default_zone = arg_str(&fw_call(
                client,
                &channel,
                FW_IFACE,
                "getDefaultZone",
                json!([]),
            )?);
            let runtime = runtime_zone(client, &channel, &default_zone)?;
            let has_drift = match permanent_zone(client, &channel, &default_zone) {
                Ok(permanent) => !compute_drift(
                    &runtime.services,
                    &permanent.services,
                    &runtime.ports,
                    &permanent.ports,
                    runtime.masquerade,
                    permanent.masquerade,
                )
                .is_empty(),
                Err(e) if is_config_info_denied(&e) => true,
                Err(e) => return Err(e),
            };
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
        Mutation::Confirm => {
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
        Mutation::Panic { state } => {
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
        Mutation::Masquerade {
            state,
            zone,
            timeout,
        } => {
            let on = state == "on";
            let zone = effective_zone(client, &channel, zone)?;
            if !on {
                crate::safety::check_firewall_masquerade_off(cli.force)?;
            }
            let (method, args) = if on {
                let t = i64::from(timeout.unwrap_or(0));
                ("addMasquerade", json!([zone, t]))
            } else {
                ("removeMasquerade", json!([zone]))
            };
            run_audited(
                client,
                &channel,
                &host,
                &format!("masquerade-{state}"),
                &zone,
                FW_ZONE_IFACE,
                method,
                args,
            )?;
            Ok(masquerade_view(
                host,
                &zone,
                on,
                if on { timeout } else { None },
            ))
        }
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeChangeData {
    operation: String,
    zone: String,
    change: String,
    timeout: Option<u32>,
}

impl RuntimeChangeData {
    fn new(operation: &str, zone: &str, change: &str, timeout: Option<u32>) -> Self {
        Self {
            operation: operation.to_string(),
            zone: zone.to_string(),
            change: change.to_string(),
            timeout,
        }
    }

    fn data(&self) -> Value {
        let mut data = json!({
            "operation": self.operation,
            "zone": self.zone,
            "change": self.change,
            "persisted": false,
        });
        if let Some(timeout) = self.timeout {
            data["timeout"] = json!(timeout);
        }
        data
    }

    fn human(&self) -> String {
        format!(
            "{} {} in zone {} (runtime only)\n",
            self.operation, self.change, self.zone
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PersistedFirewallOperation {
    operation: &'static str,
}

impl PersistedFirewallOperation {
    fn data(self) -> Value {
        json!({"operation": self.operation, "persisted": true})
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PanicChangeData {
    on: bool,
}

impl PanicChangeData {
    fn data(self) -> Value {
        json!({"operation": "panic", "panic_mode": self.on, "persisted": false})
    }

    fn human(self) -> String {
        format!(
            "panic mode {}\n",
            if self.on { "enabled" } else { "disabled" }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MasqueradeChangeData {
    zone: String,
    on: bool,
    timeout: Option<u32>,
}

impl MasqueradeChangeData {
    fn new(zone: &str, on: bool, timeout: Option<u32>) -> Self {
        Self {
            zone: zone.to_string(),
            on,
            timeout,
        }
    }

    fn change(&self) -> &'static str {
        if self.on {
            "+masquerade"
        } else {
            "-masquerade"
        }
    }

    fn data(&self) -> Value {
        let mut data = json!({
            "operation": "masquerade",
            "zone": self.zone,
            "change": self.change(),
            "masquerade": self.on,
            "persisted": false,
        });
        if let Some(timeout) = self.timeout {
            data["timeout"] = json!(timeout);
        }
        data
    }

    fn human(&self) -> String {
        format!(
            "masquerade {} in zone {} (runtime only)\n",
            if self.on { "enabled" } else { "disabled" },
            self.zone
        )
    }
}

/// Build the `FirewallChange` view for an add/remove/set mutation.
fn change_view(host: String, op: &str, zone: &str, what: &str, timeout: Option<u32>) -> View {
    let change = RuntimeChangeData::new(op, zone, what, timeout);
    View::new("FirewallChange", host, change.data(), change.human()).with_hints(confirm_hint())
}

/// Build the `FirewallChange` view for `reload`.
fn reload_view(host: String) -> View {
    let operation = PersistedFirewallOperation {
        operation: "reload",
    };
    View::new(
        "FirewallChange",
        host,
        operation.data(),
        "reloaded permanent config into runtime\n".into(),
    )
}

/// Build the `FirewallConfirm` view for `confirm`.
fn confirm_view(host: String) -> View {
    let operation = PersistedFirewallOperation {
        operation: "confirm",
    };
    View::new(
        "FirewallConfirm",
        host,
        operation.data(),
        "runtime config committed to permanent\n".into(),
    )
}

/// Build the `FirewallChange` view for `panic on|off`.
fn panic_view(host: String, on: bool) -> View {
    let change = PanicChangeData { on };
    View::new("FirewallChange", host, change.data(), change.human())
}

/// Build the `FirewallChange` view for `masquerade on|off`.
fn masquerade_view(host: String, zone: &str, on: bool, timeout: Option<u32>) -> View {
    let change = MasqueradeChangeData::new(zone, on, timeout);
    View::new("FirewallChange", host, change.data(), change.human()).with_hints(confirm_hint())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dbus(name: &str) -> FezError {
        FezError::Dbus {
            name: name.into(),
            message: "boom".into(),
        }
    }

    #[test]
    fn map_fw_error_service_unknown_is_dependency_missing() {
        let mapped = map_fw_error(
            dbus("org.freedesktop.DBus.Error.ServiceUnknown"),
            "getZones",
        );
        assert_eq!(mapped.code(), "dependency-missing");
        assert_eq!(mapped.exit_code(), 9);
        // NameHasNoOwner (the other activation-failure name) maps the same way.
        assert_eq!(
            map_fw_error(
                dbus("org.freedesktop.DBus.Error.NameHasNoOwner"),
                "getZones"
            )
            .code(),
            "dependency-missing"
        );
    }

    #[test]
    fn map_fw_error_unknown_method_is_unsupported_api() {
        let mapped = map_fw_error(
            dbus("org.freedesktop.DBus.Error.UnknownMethod"),
            "getMasquerade",
        );
        assert_eq!(mapped.code(), "unsupported-api");
        assert_eq!(mapped.exit_code(), 12);
        // The method name is carried for the caller.
        assert!(matches!(
            mapped,
            FezError::UnsupportedApi(ref m) if m == "getMasquerade"
        ));
    }

    #[test]
    fn map_fw_error_channel_problem_is_dependency_missing() {
        for problem in ["not-found", "not-supported"] {
            let mapped = map_fw_error(FezError::Problem(problem.into()), "getZones");
            assert_eq!(
                mapped.code(),
                "dependency-missing",
                "Problem({problem}) should map to dependency-missing"
            );
        }
    }

    #[test]
    fn map_fw_error_passes_through_unrelated_errors() {
        // A channel problem that is not an activation symptom is left as-is, so
        // its already-actionable raw cause survives (here: an unrelated
        // "authentication-failed" -> code auth-failed, not dependency-missing).
        assert_eq!(
            map_fw_error(
                FezError::Problem("authentication-failed".into()),
                "getZones"
            )
            .code(),
            "auth-failed"
        );
        // AccessDenied is untouched.
        let denied = FezError::AccessDenied {
            remediation: "enable sudo".into(),
        };
        assert_eq!(map_fw_error(denied, "getZones").code(), "access-denied");
    }

    #[test]
    fn classify_routes_reads_and_mutations_to_typed_plans() {
        assert!(matches!(
            classify(&FirewallAction::Status),
            Plan::Read(ReadAction::Status)
        ));
        assert!(matches!(
            classify(&FirewallAction::List),
            Plan::Read(ReadAction::List)
        ));
        assert!(matches!(
            classify(&FirewallAction::Show {
                zone: "public".into()
            }),
            Plan::Read(ReadAction::Show { zone: "public" })
        ));
        assert!(matches!(
            classify(&FirewallAction::Services),
            Plan::Read(ReadAction::Services)
        ));
        assert!(matches!(
            classify(&FirewallAction::AddService {
                service: "ssh".into(),
                zone: Some("public".into()),
                timeout: Some(60),
            }),
            Plan::Mutate(Mutation::AddService {
                service: "ssh",
                zone: Some("public"),
                timeout: Some(60),
            })
        ));
        assert!(matches!(
            classify(&FirewallAction::RemoveService {
                service: "ssh".into(),
                zone: Some("public".into()),
            }),
            Plan::Mutate(Mutation::RemoveService {
                service: "ssh",
                zone: Some("public"),
            })
        ));
        assert!(matches!(
            classify(&FirewallAction::AddPort {
                port: "8080/tcp".into(),
                zone: Some("public".into()),
                timeout: Some(60),
            }),
            Plan::Mutate(Mutation::AddPort {
                port: "8080/tcp",
                zone: Some("public"),
                timeout: Some(60),
            })
        ));
        assert!(matches!(
            classify(&FirewallAction::RemovePort {
                port: "8080/tcp".into(),
                zone: Some("public".into()),
            }),
            Plan::Mutate(Mutation::RemovePort {
                port: "8080/tcp",
                zone: Some("public"),
            })
        ));
        assert!(matches!(
            classify(&FirewallAction::SetDefaultZone {
                zone: "internal".into(),
            }),
            Plan::Mutate(Mutation::SetDefaultZone { zone: "internal" })
        ));
        assert!(matches!(
            classify(&FirewallAction::Reload),
            Plan::Mutate(Mutation::Reload)
        ));
        assert!(matches!(
            classify(&FirewallAction::Confirm),
            Plan::Mutate(Mutation::Confirm)
        ));
        assert!(matches!(
            classify(&FirewallAction::Panic { state: "on".into() }),
            Plan::Mutate(Mutation::Panic { state: "on" })
        ));
        assert!(matches!(
            classify(&FirewallAction::Masquerade {
                state: "on".into(),
                zone: Some("public".into()),
                timeout: Some(60),
            }),
            Plan::Mutate(Mutation::Masquerade {
                state: "on",
                zone: Some("public"),
                timeout: Some(60),
            })
        ));
    }

    #[test]
    fn parse_port_spec_splits_port_and_proto() {
        assert_eq!(
            parse_port_spec("8080/tcp").unwrap(),
            PortSpec {
                port: 8080,
                proto: "tcp".to_string(),
            }
        );
        assert_eq!(
            parse_port_spec("53/udp").unwrap(),
            PortSpec {
                port: 53,
                proto: "udp".to_string(),
            }
        );
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
    fn firewall_status_data_preserves_json_contract() {
        let status = FirewallStatusData::from_runtime(
            "public".to_string(),
            false,
            true,
            Some(vec!["+service http".to_string()]),
        );

        assert_eq!(
            status.data(),
            json!({
                "running": true,
                "default_zone": "public",
                "panic_mode": false,
                "masquerade": true,
                "pending_changes": ["+service http"],
                "pending_changes_available": true,
            })
        );
        assert_eq!(
            status.human_prefix(),
            "running:       yes\ndefault zone:  public\npanic mode:    off\nmasquerade:    on\n"
        );
    }

    #[test]
    fn firewall_status_data_marks_pending_unavailable_when_drift_is_none() {
        let status = FirewallStatusData::from_runtime("public".to_string(), false, false, None);

        assert_eq!(
            status.data(),
            json!({
                "running": true,
                "default_zone": "public",
                "panic_mode": false,
                "masquerade": false,
                "pending_changes": [],
                "pending_changes_available": false,
            })
        );
        assert_eq!(
            status.human_prefix(),
            "running:       yes\ndefault zone:  public\npanic mode:    off\nmasquerade:    off\n"
        );
    }

    #[test]
    fn firewall_status_data_keeps_empty_drift_available() {
        let status =
            FirewallStatusData::from_runtime("public".to_string(), true, false, Some(vec![]));

        assert_eq!(
            status.data(),
            json!({
                "running": true,
                "default_zone": "public",
                "panic_mode": true,
                "masquerade": false,
                "pending_changes": [],
                "pending_changes_available": true,
            })
        );
        assert_eq!(
            status.human_prefix(),
            "running:       yes\ndefault zone:  public\npanic mode:    on\nmasquerade:    off\n"
        );
    }

    #[test]
    fn firewall_zone_summary_preserves_row_contract() {
        let summary = FirewallZoneSummary {
            zone: "public".to_string(),
            is_default: true,
            services: vec!["ssh".to_string(), "http".to_string()],
            ports: vec!["9090/tcp".to_string()],
            interfaces: vec!["eth0".to_string()],
        };

        assert_eq!(
            summary.row(),
            json!(["public", true, "ssh,http", "9090/tcp", "eth0"])
        );
        assert_eq!(
            summary.human_row(),
            "public       yes      ssh,http                 9090/tcp         eth0\n"
        );
    }

    #[test]
    fn firewall_zone_detail_preserves_json_contract() {
        let detail = FirewallZoneDetail {
            zone: "public".to_string(),
            services: vec!["ssh".to_string()],
            ports: vec!["9090/tcp".to_string()],
            interfaces: vec!["eth0".to_string()],
            sources: vec!["192.0.2.0/24".to_string()],
            masquerade: false,
        };

        assert_eq!(
            detail.data(),
            json!({
                "zone": "public",
                "services": ["ssh"],
                "ports": ["9090/tcp"],
                "interfaces": ["eth0"],
                "sources": ["192.0.2.0/24"],
                "masquerade": false,
            })
        );
        assert_eq!(
            detail.human(),
            "Zone:       public\nServices:   ssh\nPorts:      9090/tcp\nInterfaces: eth0\nSources:    192.0.2.0/24\nMasquerade: off\n"
        );
    }

    #[test]
    fn runtime_change_data_preserves_json_contract() {
        let change = RuntimeChangeData::new("add-service", "public", "service http", Some(60));

        assert_eq!(
            change.data(),
            json!({
                "operation": "add-service",
                "zone": "public",
                "change": "service http",
                "persisted": false,
                "timeout": 60,
            })
        );
        assert_eq!(
            change.human(),
            "add-service service http in zone public (runtime only)\n"
        );
    }

    #[test]
    fn persisted_firewall_operation_preserves_json_contract() {
        let reload = PersistedFirewallOperation {
            operation: "reload",
        };
        let confirm = PersistedFirewallOperation {
            operation: "confirm",
        };

        assert_eq!(
            reload.data(),
            json!({"operation": "reload", "persisted": true})
        );
        assert_eq!(
            confirm.data(),
            json!({"operation": "confirm", "persisted": true})
        );
    }

    #[test]
    fn panic_change_data_preserves_json_contract() {
        let change = PanicChangeData { on: true };

        assert_eq!(
            change.data(),
            json!({"operation": "panic", "panic_mode": true, "persisted": false})
        );
        assert_eq!(change.human(), "panic mode enabled\n");
    }

    #[test]
    fn masquerade_change_data_preserves_json_contract() {
        let change = MasqueradeChangeData::new("public", false, None);

        assert_eq!(
            change.data(),
            json!({
                "operation": "masquerade",
                "zone": "public",
                "change": "-masquerade",
                "masquerade": false,
                "persisted": false,
            })
        );
        assert_eq!(
            change.human(),
            "masquerade disabled in zone public (runtime only)\n"
        );
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
            false,
            false,
        );
        assert_eq!(drift, vec!["+port 9090/tcp".to_string()]);
    }

    #[test]
    fn drift_empty_when_runtime_matches_permanent() {
        let s = vec!["ssh".to_string()];
        let p: Vec<String> = vec![];
        assert!(compute_drift(&s, &s, &p, &p, false, false).is_empty());
    }

    #[test]
    fn drift_reports_removed_service() {
        // permanent has http but runtime does not -> service removed at runtime.
        let runtime_services: Vec<String> = vec![];
        let permanent_services = vec!["http".to_string()];
        let p: Vec<String> = vec![];
        let drift = compute_drift(&runtime_services, &permanent_services, &p, &p, false, false);
        assert_eq!(drift, vec!["-service http".to_string()]);
    }

    #[test]
    fn drift_reports_masquerade_added_at_runtime() {
        // runtime masquerade on, permanent off -> +masquerade.
        let s = vec!["ssh".to_string()];
        let p: Vec<String> = vec![];
        let drift = compute_drift(&s, &s, &p, &p, true, false);
        assert_eq!(drift, vec!["+masquerade".to_string()]);
    }

    #[test]
    fn drift_reports_masquerade_removed_at_runtime() {
        let s = vec!["ssh".to_string()];
        let p: Vec<String> = vec![];
        let drift = compute_drift(&s, &s, &p, &p, false, true);
        assert_eq!(drift, vec!["-masquerade".to_string()]);
    }

    #[test]
    fn drift_empty_when_masquerade_matches() {
        let s = vec!["ssh".to_string()];
        let p: Vec<String> = vec![];
        assert!(compute_drift(&s, &s, &p, &p, true, true).is_empty());
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
