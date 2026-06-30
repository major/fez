//! Firmware reads via fwupd (org.freedesktop.fwupd).
//!
//! Lists devices, host security attributes (HSI score), and available
//! firmware upgrades. Read-only. fwupd is polkit-gated; try unprivileged
//! first, escalate on access-denied.

use crate::capabilities::{map_absent_service, View};
use crate::error::{FezError, Result};
use crate::protocol::client::{variant_value, BridgeClient};
use serde_json::{json, Value};

const FWUPD_NAME: &str = "org.freedesktop.fwupd";
const FWUPD_PATH: &str = "/org/freedesktop/fwupd";
const FWUPD_IFACE: &str = "org.freedesktop.fwupd";
const PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";

/// fwupd `FWUPD_DEVICE_FLAG_UPDATABLE` — bit 1 of the device Flags bitmask.
const FWUPD_FLAG_UPDATABLE: u64 = 0x2;

fn dependency_missing() -> FezError {
    FezError::DependencyMissing {
        component: "fwupd".into(),
        dbus_name: FWUPD_NAME.into(),
        remediation: "Install fwupd: sudo dnf install fwupd".into(),
    }
}

fn open_fwupd(client: &mut BridgeClient) -> Result<String> {
    map_absent_service(client.dbus_open(FWUPD_NAME), dependency_missing)
}

fn fwupd_call(
    client: &mut BridgeClient,
    channel: &str,
    method: &str,
    args: Value,
) -> Result<Value> {
    map_absent_service(
        client.dbus_call(channel, FWUPD_PATH, FWUPD_IFACE, method, args),
        dependency_missing,
    )
}

fn vv_str(dict: &Value, key: &str) -> String {
    variant_value(dict.get(key).unwrap_or(&Value::Null))
        .as_str()
        .unwrap_or("")
        .to_string()
}

fn vv_u64(dict: &Value, key: &str) -> u64 {
    variant_value(dict.get(key).unwrap_or(&Value::Null))
        .as_u64()
        .unwrap_or(0)
}

/// List firmware devices.
pub(super) fn list(client: &mut BridgeClient, host: &str) -> Result<View> {
    let channel = open_fwupd(client)?;
    let out = fwupd_call(client, &channel, "GetDevices", json!([]))?;
    let raw = out
        .get(0)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let devices: Vec<Value> = raw
        .iter()
        .map(|d| {
            let flags = vv_u64(d, "Flags");
            json!({
                "name": vv_str(d, "Name"),
                "device_id": vv_str(d, "DeviceId"),
                "vendor": vv_str(d, "Vendor"),
                "version": vv_str(d, "Version"),
                "updatable": flags & FWUPD_FLAG_UPDATABLE != 0,
            })
        })
        .collect();

    let data = json!({"devices": devices});
    let human = render_devices(&devices);
    Ok(View::new("FirmwareDeviceList", host, data, human))
}

fn render_devices(devices: &[Value]) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "{:<30} {:<15} {:<15} {}\n",
        "NAME", "VENDOR", "VERSION", "UPDATABLE"
    ));
    for d in devices {
        s.push_str(&format!(
            "{:<30} {:<15} {:<15} {}\n",
            d["name"].as_str().unwrap_or(""),
            d["vendor"].as_str().unwrap_or(""),
            d["version"].as_str().unwrap_or(""),
            if d["updatable"].as_bool() == Some(true) {
                "yes"
            } else {
                "no"
            },
        ));
    }
    s
}

/// Show host firmware security attributes (HSI score).
pub(super) fn security(client: &mut BridgeClient, host: &str) -> Result<View> {
    let channel = open_fwupd(client)?;

    // Get HSI score from property
    let prop_out = map_absent_service(
        client.dbus_call(
            &channel,
            FWUPD_PATH,
            PROPS_IFACE,
            "Get",
            json!([FWUPD_IFACE, "HostSecurityId"]),
        ),
        dependency_missing,
    )?;
    let hsi_score = variant_value(prop_out.get(0).unwrap_or(&Value::Null))
        .as_str()
        .unwrap_or("")
        .to_string();

    // Get security attributes
    let out = fwupd_call(client, &channel, "GetHostSecurityAttrs", json!([]))?;
    let raw = out
        .get(0)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let attrs: Vec<Value> = raw
        .iter()
        .map(|a| {
            json!({
                "id": vv_str(a, "AppstreamId"),
                "name": vv_str(a, "Name"),
                "result": vv_str(a, "HsiResult"),
                "level": vv_u64(a, "HsiLevel"),
            })
        })
        .collect();

    let data = json!({"hsi_score": hsi_score, "attributes": attrs});
    let human = render_security(&hsi_score, &attrs);
    Ok(View::new("FirmwareSecurityReport", host, data, human))
}

fn render_security(hsi_score: &str, attrs: &[Value]) -> String {
    let mut s = String::new();
    s.push_str(&format!("Host Security: {hsi_score}\n\n"));
    s.push_str(&format!(
        "{:<40} {:<15} {}\n",
        "ATTRIBUTE", "RESULT", "LEVEL"
    ));
    for a in attrs {
        s.push_str(&format!(
            "{:<40} {:<15} HSI-{}\n",
            a["name"].as_str().unwrap_or(""),
            a["result"].as_str().unwrap_or(""),
            a["level"].as_u64().unwrap_or(0),
        ));
    }
    s
}

/// List available firmware upgrades for updatable devices.
pub(super) fn upgrades(client: &mut BridgeClient, host: &str) -> Result<View> {
    let channel = open_fwupd(client)?;

    // First, list devices to find updatable ones
    let dev_out = fwupd_call(client, &channel, "GetDevices", json!([]))?;
    let devices = dev_out
        .get(0)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut all_upgrades = Vec::new();
    for d in &devices {
        let flags = vv_u64(d, "Flags");
        if flags & FWUPD_FLAG_UPDATABLE == 0 {
            continue; // not updatable
        }
        let device_id = vv_str(d, "DeviceId");
        let device_name = vv_str(d, "Name");
        let current_version = vv_str(d, "Version");

        let out = fwupd_call(client, &channel, "GetUpgrades", json!([&device_id]))?;
        let raw = out
            .get(0)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        for u in &raw {
            all_upgrades.push(json!({
                "device": device_name,
                "device_id": device_id,
                "current_version": current_version,
                "version": vv_str(u, "Version"),
                "description": vv_str(u, "Description"),
                "vendor": vv_str(u, "Vendor"),
            }));
        }
    }

    let data = json!({"upgrades": all_upgrades});
    let human = render_upgrades(&all_upgrades);
    Ok(View::new("FirmwareUpgradeList", host, data, human))
}

fn render_upgrades(upgrades: &[Value]) -> String {
    if upgrades.is_empty() {
        return "No firmware upgrades available.\n".into();
    }
    let mut s = String::new();
    for u in upgrades {
        s.push_str(&format!(
            "{}: {} → {} ({})\n",
            u["device"].as_str().unwrap_or(""),
            u["current_version"].as_str().unwrap_or(""),
            u["version"].as_str().unwrap_or(""),
            u["description"].as_str().unwrap_or(""),
        ));
    }
    s
}
