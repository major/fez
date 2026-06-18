//! Read-only firewall queries: status, list, show, services.

use super::zone::{
    compute_drift, is_config_info_denied, permanent_zone, runtime_zone,
};
use super::{arg_str, arg_str_vec, fw_call, open_channel, FW_IFACE, FW_ZONE_IFACE};
use crate::capabilities::View;
use crate::error::{FezError, Result};
use crate::protocol::client::BridgeClient;
use serde_json::{json, Value};

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

/// `firewall status`: state, default zone, panic flag, and pending drift.
///
/// Runtime reads (default zone, panic flag, the runtime zone's services/ports)
/// go over the unprivileged `channel`. The permanent-config read needed for
/// drift is polkit-gated (firewalld's `PK_ACTION_CONFIG`, `auth_admin_keep` on
/// both server and desktop installs), so it is issued on a separate privileged
/// channel that escalates first. A host with no usable escalation mechanism
/// therefore fails `status` with `access-denied` (exit 11) rather than
/// silently reporting empty drift.
pub(super) fn status(
    client: &mut BridgeClient,
    channel: &str,
    host: String,
) -> Result<View> {
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
pub(super) fn list(
    client: &mut BridgeClient,
    channel: &str,
    host: String,
) -> Result<View> {
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
pub(super) fn show(
    client: &mut BridgeClient,
    channel: &str,
    host: String,
    zone: &str,
) -> Result<View> {
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
pub(super) fn services(
    client: &mut BridgeClient,
    channel: &str,
    host: String,
) -> Result<View> {
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

// reads.rs uses arg_bool from the parent, pull it in for the status fn.
use super::arg_bool;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
}
