use super::model::{ActiveConnection, IpConfig, Ipv6Config, NetworkDevice, NetworkDeviceDetail};
use super::{
    ACTIVE_IFACE, DEVICE_IFACE, DHCP4_IFACE, IP4_IFACE, IP6_IFACE, NM_MGR_IFACE, NM_MGR_PATH,
    PROPS_IFACE,
};
use crate::capabilities::{CapabilityContext, View};
use crate::error::{FezError, Result};
use crate::protocol::client::BridgeClient;
use serde_json::{json, Value};

/// `GetAll` the properties of a NM object, returning the unwrapped `a{sv}` map.
///
/// A `GetAll` always returns one out-arg (the property dict); a missing one is
/// a malformed reply, not an empty object, so it surfaces as an error rather
/// than being silently treated as a device with no properties.
fn get_all(client: &mut BridgeClient, channel: &str, path: &str, iface: &str) -> Result<Value> {
    let out = client.dbus_call(channel, path, PROPS_IFACE, "GetAll", json!([iface]))?;
    out.get(0)
        .cloned()
        .ok_or_else(|| FezError::Problem(format!("GetAll({iface}) returned no value")))
}

fn load_ip4_config(
    client: &mut BridgeClient,
    channel: &str,
    path: Option<&str>,
) -> Result<IpConfig> {
    match path {
        Some(path) => IpConfig::from_value(get_all(client, channel, path, IP4_IFACE)?),
        None => Ok(IpConfig::default()),
    }
}

fn load_ip6_config(
    client: &mut BridgeClient,
    channel: &str,
    path: Option<&str>,
) -> Result<Ipv6Config> {
    match path {
        Some(path) => Ipv6Config::from_value(get_all(client, channel, path, IP6_IFACE)?),
        None => Ok(Ipv6Config::default()),
    }
}

fn load_active_connection(
    client: &mut BridgeClient,
    channel: &str,
    path: Option<&str>,
) -> Result<Option<ActiveConnection>> {
    match path {
        Some(path) => {
            let val = get_all(client, channel, path, ACTIVE_IFACE)?;
            Ok(Some(ActiveConnection::from_value(val)?))
        }
        None => Ok(None),
    }
}

fn load_dhcp4_options(
    client: &mut BridgeClient,
    channel: &str,
    path: Option<&str>,
) -> Result<Option<Value>> {
    match path {
        Some(path) => {
            let val = get_all(client, channel, path, DHCP4_IFACE)?;
            // ponytail: manual unwrap here - dynamic keys can't be a derive struct
            Ok(val
                .get("Options")
                .map(|v| v.get("v").unwrap_or(v))
                .and_then(flatten_options))
        }
        None => Ok(None),
    }
}

/// Call `GetDevices` on the manager and return the device object paths.
///
/// `GetDevices` always returns one out-arg (an array of object paths); a
/// missing or non-array first out-arg is a malformed reply, not "no devices",
/// so it errors rather than silently yielding an empty inventory.
fn device_paths(client: &mut BridgeClient, channel: &str) -> Result<Vec<String>> {
    let out = client.dbus_call(channel, NM_MGR_PATH, NM_MGR_IFACE, "GetDevices", json!([]))?;
    let arr = out
        .get(0)
        .and_then(Value::as_array)
        .ok_or_else(|| FezError::Problem("GetDevices returned a non-array response".into()))?;
    Ok(arr
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect())
}

/// List network devices, hiding unmanaged virtual interfaces unless `all`.
pub(super) fn list(ctx: &mut CapabilityContext<'_>, all: bool) -> Result<View> {
    let paths = device_paths(ctx.client, ctx.channel)?;

    let mut devices = Vec::new();
    for path in &paths {
        let device =
            NetworkDevice::from_value(get_all(ctx.client, ctx.channel, path, DEVICE_IFACE)?)?;
        if !device.should_list(all) {
            continue;
        }
        let ip4 = load_ip4_config(ctx.client, ctx.channel, device.ip4_config.as_deref())?;
        let ip6 = load_ip6_config(ctx.client, ctx.channel, device.ip6_config.as_deref())?;
        devices.push((device, ip4, ip6));
    }

    let mut human = format!(
        "{:<14} {:<10} {:<13} {:<20} {}\n",
        "DEVICE", "TYPE", "STATE", "IPV4", "MAC"
    );
    for d in &devices {
        human.push_str(&format!(
            "{:<14} {:<10} {:<13} {:<20} {}\n",
            d.0.interface,
            d.0.type_name(),
            d.0.state_name(),
            d.1.primary_address(),
            d.0.mac,
        ));
    }

    let columns = ["interface", "type", "state", "ip4", "ip6", "mac"];
    let rows: Vec<Value> = devices
        .iter()
        .map(|(device, ip4, ip6)| {
            json!([
                device.interface,
                device.type_name(),
                device.state_name(),
                ip4.primary_address(),
                ip6.primary_address(),
                device.mac,
            ])
        })
        .collect();
    Ok(View::new(
        "NetworkDeviceList",
        ctx.host,
        crate::envelope::table_data(&columns, rows),
        human,
    ))
}

/// Show one device's full network detail, chasing NM's object indirection.
pub(super) fn show(ctx: &mut CapabilityContext<'_>, device: &str) -> Result<View> {
    let paths = device_paths(ctx.client, ctx.channel)?;

    // Find the device whose Interface matches the requested name.
    let mut found: Option<NetworkDevice> = None;
    for path in &paths {
        let candidate =
            NetworkDevice::from_value(get_all(ctx.client, ctx.channel, path, DEVICE_IFACE)?)?;
        if candidate.interface == device {
            found = Some(candidate);
            break;
        }
    }
    let device = found.ok_or_else(|| FezError::NotFound(format!("network device {device}")))?;

    let ipv4 = load_ip4_config(ctx.client, ctx.channel, device.ip4_config.as_deref())?;
    let ipv6 = load_ip6_config(ctx.client, ctx.channel, device.ip6_config.as_deref())?;
    let connection =
        load_active_connection(ctx.client, ctx.channel, device.active_connection.as_deref())?;
    let dhcp4 = load_dhcp4_options(ctx.client, ctx.channel, device.dhcp4_config.as_deref())?;
    let device_type = device.type_name();
    let state = device.state_name();

    let detail = NetworkDeviceDetail {
        interface: device.interface,
        device_type,
        state,
        mac: device.mac,
        mtu: device.mtu,
        ipv4,
        ipv6,
        connection,
        dhcp4,
    };
    let human = render_show_human(&detail);
    let data = serde_json::to_value(detail).map_err(FezError::Decode)?;

    Ok(View::new("NetworkDeviceDetail", ctx.host, data, human))
}

/// Flatten an `a{sv}` options map by unwrapping each variant value, so the
/// envelope carries plain scalars instead of the `{"t","v"}` wire shape.
///
/// ponytail: inline variant unwrap - dynamic DHCP keys can't use a derive struct
fn flatten_options(opts: &Value) -> Option<Value> {
    let obj = opts.as_object()?;
    let flat: serde_json::Map<String, Value> = obj
        .iter()
        .map(|(k, v)| (k.clone(), v.get("v").unwrap_or(v).clone()))
        .collect();
    Some(Value::Object(flat))
}

/// Render the human form of a `network show` detail object.
fn render_show_human(d: &NetworkDeviceDetail) -> String {
    let mut out = String::new();
    out.push_str(&format!("Device:    {}\n", d.interface));
    out.push_str(&format!("Type:      {}\n", d.device_type));
    out.push_str(&format!("State:     {}\n", d.state));
    out.push_str(&format!("MAC:       {}\n", d.mac));
    out.push_str(&format!("MTU:       {}\n", d.mtu));
    out.push_str(&format!("IPv4:      {}\n", d.ipv4.addresses.join(", ")));
    out.push_str(&format!("Gateway:   {}\n", d.ipv4.gateway));
    out.push_str(&format!("DNS:       {}\n", d.ipv4.dns.join(", ")));
    out.push_str(&format!("Domains:   {}\n", d.ipv4.domains.join(", ")));
    out.push_str(&format!("IPv6:      {}\n", d.ipv6.addresses.join(", ")));
    if let Some(conn) = &d.connection {
        out.push_str(&format!(
            "Connection: {} ({})\n",
            conn.id, conn.connection_type
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatten_options_unwraps_variant_values() {
        let opts = json!({
            "routers": {"t":"s","v":"192.168.10.1"},
            "lease_time": {"t":"s","v":"3600"},
        });
        let flat = flatten_options(&opts).unwrap();
        assert_eq!(flat["routers"], json!("192.168.10.1"));
        assert_eq!(flat["lease_time"], json!("3600"));
        assert_eq!(flatten_options(&json!("x")), None);
    }

    #[test]
    fn show_human_renders_typed_detail() {
        let detail = NetworkDeviceDetail {
            interface: "eth0".into(),
            device_type: "ethernet".into(),
            state: "activated".into(),
            mac: "52:54:00:00:00:01".into(),
            mtu: 1500,
            ipv4: IpConfig {
                addresses: vec!["192.0.2.10/24".into()],
                gateway: "192.0.2.1".into(),
                dns: vec!["1.1.1.1".into()],
                domains: vec!["example.test".into()],
            },
            ipv6: Ipv6Config {
                addresses: vec!["2001:db8::10/64".into()],
            },
            connection: Some(ActiveConnection {
                id: "Wired".into(),
                connection_type: "802-3-ethernet".into(),
                default: true,
            }),
            dhcp4: None,
        };

        let human = render_show_human(&detail);

        assert!(human.contains("Device:    eth0"));
        assert!(human.contains("IPv4:      192.0.2.10/24"));
        assert!(human.contains("DNS:       1.1.1.1"));
        assert!(human.contains("Connection: Wired (802-3-ethernet)"));
    }
}
