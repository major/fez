//! NetworkManager DnsManager fallback for hosts without systemd-resolved.
//!
//! RHEL 10 does not ship systemd-resolved; NM manages DNS directly. The
//! `DnsManager` interface at `/org/freedesktop/NetworkManager/DnsManager`
//! exposes the effective DNS configuration: mode, rc-manager, and per-interface
//! server entries. This is a subset of what resolve1 provides (no cache stats,
//! no DNSSEC/DoT, no flush/query) but covers the core "what DNS am I using?"
//! question.

use crate::capabilities::{CapabilityContext, View};
use crate::error::{FezError, Result};
use serde_json::{json, Value};

const NM_DNS_PATH: &str = "/org/freedesktop/NetworkManager/DnsManager";
const NM_DNS_IFACE: &str = "org.freedesktop.NetworkManager.DnsManager";
const PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";

/// Gather DNS status from NetworkManager's DnsManager (resolve1 fallback).
pub(super) fn status(ctx: &mut CapabilityContext<'_>) -> Result<View> {
    let out = ctx.client.dbus_call(
        ctx.channel,
        NM_DNS_PATH,
        PROPS_IFACE,
        "GetAll",
        json!([NM_DNS_IFACE]),
    )?;
    let props = out
        .get(0)
        .ok_or_else(|| FezError::Problem("GetAll(DnsManager) returned no value".into()))?;

    let mode = unwrap_str(props.get("Mode"));
    let rc_manager = unwrap_str(props.get("RcManager"));

    // Configuration is aa{sv} — array of per-interface dicts
    let config = unwrap_variant(props.get("Configuration").unwrap_or(&Value::Null));
    let entries: Vec<Value> = config
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .filter_map(|entry| {
            // Each entry has: nameservers (as), interface (s), priority (i), vpn (b)
            let nameservers = unwrap_variant(entry.get("nameservers")?)
                .as_array()?
                .iter()
                .filter_map(|v| unwrap_variant(v).as_str().map(String::from))
                .collect::<Vec<_>>();
            let interface = unwrap_str(entry.get("interface"));
            let priority = unwrap_variant(entry.get("priority").unwrap_or(&Value::Null))
                .as_i64()
                .unwrap_or(0);
            let vpn = unwrap_variant(entry.get("vpn").unwrap_or(&Value::Null))
                .as_bool()
                .unwrap_or(false);
            Some(json!({
                "nameservers": nameservers,
                "interface": interface,
                "priority": priority,
                "vpn": vpn,
            }))
        })
        .collect();

    let all_servers: Vec<String> = entries
        .iter()
        .filter_map(|e| e.get("nameservers")?.as_array())
        .flat_map(|arr| arr.iter().filter_map(Value::as_str).map(String::from))
        .collect();

    let data = json!({
        "backend": "networkmanager",
        "mode": mode,
        "rc_manager": rc_manager,
        "dns_servers": all_servers,
        "interfaces": entries,
    });

    let mut human = String::new();
    human.push_str(&format!("DNS Mode: {mode}\n"));
    human.push_str(&format!("resolv.conf: {rc_manager}\n"));
    if !all_servers.is_empty() {
        human.push_str(&format!("DNS Servers: {}\n", all_servers.join(" ")));
    }
    for entry in &entries {
        let iface = entry["interface"].as_str().unwrap_or("?");
        let servers: Vec<&str> = entry["nameservers"]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        if !servers.is_empty() {
            human.push_str(&format!("\n  {iface}: {}\n", servers.join(" ")));
        }
    }

    let mut view = View::new("DnsStatus", ctx.host, data, human);
    view = view.with_hints(json!({
        "note": "systemd-resolved is not available; showing NetworkManager DNS config (no cache stats, DNSSEC, flush, or query)"
    }));
    Ok(view)
}

fn unwrap_variant(val: &Value) -> &Value {
    val.get("v").unwrap_or(val)
}

fn unwrap_str(val: Option<&Value>) -> String {
    val.map(|v| unwrap_variant(v).as_str().unwrap_or(""))
        .unwrap_or("")
        .to_string()
}
