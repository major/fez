//! Zone parsing, effective-zone resolution, drift computation, and session
//! safety helpers.

use super::{
    arg_bool, arg_str, arg_str_vec, fw_call, fw_call_path, FW_CONFIG_IFACE, FW_CONFIG_PATH,
    FW_CONFIG_ZONE_IFACE, FW_IFACE, FW_ZONE_IFACE,
};
use crate::error::{FezError, Result};
use crate::protocol::client::BridgeClient;
use serde_json::{json, Value};

#[derive(Debug, PartialEq, Eq)]
pub(super) struct PortSpec {
    pub(super) port: u16,
    pub(super) proto: String,
}

impl PortSpec {
    pub(super) fn label(&self) -> String {
        format!("{}/{}", self.port, self.proto)
    }
}

/// Parse a `port/proto` spec.
///
/// # Errors
///
/// Returns [`FezError::NotFound`] when the spec is not `<u16>/<proto>` with a
/// non-empty protocol (used as a bad-argument signal; renders exit 4).
pub(super) fn parse_port_spec(spec: &str) -> Result<PortSpec> {
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

/// Render a `getPorts` `aas` reply (each entry `[port, proto]`) as
/// `"port/proto"` labels.
pub(super) fn ports_from_reply(out: &Value) -> Vec<String> {
    out.get(0)
        .and_then(Value::as_array)
        .map(|a| a.iter().map(port_label).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default()
}

/// Join a firewalld `[port, protocol]` entry into a `"port/proto"` label.
/// A malformed entry renders empty.
pub(super) fn port_label(entry: &Value) -> String {
    let port = entry.get(0).and_then(Value::as_str).unwrap_or("");
    let proto = entry.get(1).and_then(Value::as_str).unwrap_or("");
    if port.is_empty() || proto.is_empty() {
        String::new()
    } else {
        format!("{port}/{proto}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RuntimeZone {
    pub(super) services: Vec<String>,
    pub(super) ports: Vec<String>,
    pub(super) masquerade: bool,
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

/// Read the permanent (`config`) services, ports, and masquerade for a zone,
/// for drift.
pub(super) fn permanent_zone(
    client: &mut BridgeClient,
    channel: &str,
    zone: &str,
) -> Result<RuntimeZone> {
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
pub(super) fn runtime_zone(
    client: &mut BridgeClient,
    channel: &str,
    zone: &str,
) -> Result<RuntimeZone> {
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

/// Compute runtime-vs-permanent drift as a list of `"+/-kind value"` tokens.
///
/// `+` means present at runtime but not permanent (would be lost on reload);
/// `-` means present permanent but not runtime (removed at runtime, not yet
/// committed). Covers services, ports, and masquerade. Stateless: both sides
/// are read live each call.
pub(super) fn compute_drift(
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
pub(super) fn is_config_info_denied(e: &FezError) -> bool {
    match e {
        FezError::Dbus { name, message } => {
            name.contains("NotAuthorized")
                || name.contains("AccessDenied")
                || message.contains("config.info")
        }
        _ => false,
    }
}

/// Resolve the effective zone for a mutation: the `--zone` flag, or the live
/// default zone when omitted.
pub(super) fn effective_zone(
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

/// The set of firewall services treated as session-critical (always `ssh`).
pub(super) fn session_services() -> Vec<String> {
    vec!["ssh".to_string()]
}

/// Parse the server-side port (4th field) out of an `SSH_CONNECTION` value.
pub(super) fn session_port_from(ssh_connection: &str) -> Option<u16> {
    ssh_connection.split_whitespace().nth(3)?.parse().ok()
}

fn session_ports_from(ssh_connection: Option<&str>) -> Vec<u16> {
    let mut ports = vec![22];
    if let Some(port) = ssh_connection.and_then(session_port_from) {
        if port != 22 {
            ports.push(port);
        }
    }
    ports
}

/// The session-critical port set: the default SSH port plus any server-side
/// port parsed from `$SSH_CONNECTION`.
pub(super) fn session_ports() -> Vec<u16> {
    session_ports_from(std::env::var("SSH_CONNECTION").ok().as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
    fn drift_reports_runtime_only_ports() {
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
    fn drift_reports_permanent_only_port() {
        let s = vec!["ssh".to_string()];
        let runtime_ports: Vec<String> = vec![];
        let permanent_ports = vec!["443/tcp".to_string()];
        let drift = compute_drift(&s, &s, &runtime_ports, &permanent_ports, false, false);
        assert_eq!(drift, vec!["-port 443/tcp".to_string()]);
    }

    #[test]
    fn drift_empty_when_runtime_matches_permanent() {
        let s = vec!["ssh".to_string()];
        let p: Vec<String> = vec![];
        assert!(compute_drift(&s, &s, &p, &p, false, false).is_empty());
    }

    #[test]
    fn drift_reports_removed_service() {
        let runtime_services: Vec<String> = vec![];
        let permanent_services = vec!["http".to_string()];
        let p: Vec<String> = vec![];
        let drift = compute_drift(&runtime_services, &permanent_services, &p, &p, false, false);
        assert_eq!(drift, vec!["-service http".to_string()]);
    }

    #[test]
    fn drift_reports_masquerade_added_at_runtime() {
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
    fn is_config_info_denied_matches_not_authorized() {
        let e = FezError::Dbus {
            name: "org.fedoraproject.FirewallD1.NotAuthorizedException".into(),
            message: "polkit denied".into(),
        };
        assert!(is_config_info_denied(&e));
    }

    #[test]
    fn is_config_info_denied_matches_access_denied() {
        let e = FezError::Dbus {
            name: "org.freedesktop.DBus.Error.AccessDenied".into(),
            message: "not allowed".into(),
        };
        assert!(is_config_info_denied(&e));
    }

    #[test]
    fn is_config_info_denied_matches_config_info_message() {
        let e = FezError::Dbus {
            name: "org.freedesktop.PolicyKit.NotAuthorized".into(),
            message: "org.fedoraproject.FirewallD1.config.info denied".into(),
        };
        assert!(is_config_info_denied(&e));
    }

    #[test]
    fn is_config_info_denied_rejects_unrelated_errors() {
        let e = FezError::Dbus {
            name: "org.freedesktop.DBus.Error.UnknownMethod".into(),
            message: "no such method".into(),
        };
        assert!(!is_config_info_denied(&e));
        assert!(!is_config_info_denied(&FezError::Problem(
            "access-denied".into()
        )));
    }

    #[test]
    fn session_port_parses_ssh_connection() {
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

    #[test]
    fn session_ports_always_include_ssh_default_port() {
        assert_eq!(session_ports_from(None), vec![22]);
        assert_eq!(
            session_ports_from(Some("10.0.0.1 5520 10.0.0.2 22")),
            vec![22]
        );
        assert_eq!(
            session_ports_from(Some("10.0.0.1 5520 10.0.0.2 2222")),
            vec![22, 2222]
        );
    }
}
