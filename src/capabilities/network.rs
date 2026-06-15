//! NetworkManager inspection capability.
//!
//! Reads the readable NetworkManager surface over the cockpit-bridge
//! `dbus-json3` channel (`org.freedesktop.NetworkManager`, system bus,
//! unprivileged). Two actions: `network list` (device inventory) and
//! `network show <device>` (full per-device detail). Read-only: no mutation,
//! no privilege escalation.

use crate::capabilities::{render, View};
use crate::cli::{Cli, NetworkAction};
use crate::error::{FezError, Result};
use crate::protocol::client::BridgeClient;
use crate::transport;
use serde_json::{json, Value};

const NM_NAME: &str = "org.freedesktop.NetworkManager";
const NM_MGR_PATH: &str = "/org/freedesktop/NetworkManager";
const NM_MGR_IFACE: &str = "org.freedesktop.NetworkManager";
const PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";
const DEVICE_IFACE: &str = "org.freedesktop.NetworkManager.Device";
const IP4_IFACE: &str = "org.freedesktop.NetworkManager.IP4Config";
const IP6_IFACE: &str = "org.freedesktop.NetworkManager.IP6Config";
const ACTIVE_IFACE: &str = "org.freedesktop.NetworkManager.Connection.Active";
const DHCP4_IFACE: &str = "org.freedesktop.NetworkManager.DHCP4Config";

/// Route a parsed `network` action to its handler and render the result.
///
/// Returns the process exit code.
pub fn dispatch(cli: &Cli, action: &NetworkAction) -> i32 {
    let result = run(cli, action);
    render(cli, result)
}

/// Connect to the bridge and dispatch the requested read action.
fn run(cli: &Cli, action: &NetworkAction) -> Result<View> {
    let transport = transport::from_host(cli.host.as_deref());
    let mut client = BridgeClient::connect(transport.as_ref())?;
    let host = client.host().to_string();
    let channel = client.dbus_open(NM_NAME)?;
    match action {
        NetworkAction::List { all } => list(&mut client, &channel, host, *all),
        NetworkAction::Show { device } => show(&mut client, &channel, host, device),
    }
}

/// Render a [`View`] (or error) to stdout/stderr and return the exit code.
/// Decode an `NMDeviceType` (`u`) to its short string.
///
/// Mirrors the upstream `NMDeviceType` enum. Unrecognized values render as
/// `type-<n>` so a new NM enum value degrades gracefully.
fn device_type_str(n: u64) -> String {
    match n {
        0 => "unknown",
        1 => "ethernet",
        2 => "wifi",
        5 => "bluetooth",
        6 => "olpc-mesh",
        7 => "wimax",
        8 => "modem",
        9 => "infiniband",
        10 => "bond",
        11 => "vlan",
        12 => "adsl",
        13 => "bridge",
        14 => "generic",
        15 => "team",
        16 => "tun",
        17 => "ip-tunnel",
        18 => "macvlan",
        19 => "vxlan",
        20 => "veth",
        32 => "loopback",
        _ => return format!("type-{n}"),
    }
    .to_string()
}

/// Decode an `NMDeviceState` (`u`) to its short string.
///
/// Mirrors the upstream `NMDeviceState` enum. Unrecognized values render as
/// `state-<n>`.
fn device_state_str(n: u64) -> String {
    match n {
        0 => "unknown",
        10 => "unmanaged",
        20 => "unavailable",
        30 => "disconnected",
        40 => "prepare",
        50 => "config",
        60 => "need-auth",
        70 => "ip-config",
        80 => "ip-check",
        90 => "secondaries",
        100 => "activated",
        110 => "deactivating",
        120 => "failed",
        _ => return format!("state-{n}"),
    }
    .to_string()
}

/// Device types the default `network list` filter always keeps even when the
/// device is unmanaged (real NICs and loopback, never container clutter).
const PHYSICAL_TYPES: [u64; 8] = [
    1,  // ethernet
    2,  // wifi
    9,  // infiniband
    10, // bond
    11, // vlan
    13, // bridge
    15, // team
    32, // loopback
];

// Temporary staged models for the upcoming list/show refactor; remove these
// dead-code allowances when the typed boundary is wired into those commands.
#[derive(Debug, Clone)]
struct NetworkDevice {
    interface: String,
    device_type: u64,
    state: u64,
    managed: bool,
    mac: String,
    #[allow(dead_code)]
    mtu: u64,
    ip4_config: Option<String>,
    ip6_config: Option<String>,
    #[allow(dead_code)]
    active_connection: Option<String>,
    #[allow(dead_code)]
    dhcp4_config: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct IpConfig {
    addresses: Vec<String>,
    gateway: String,
    dns: Vec<String>,
    domains: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct Ipv6Config {
    addresses: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, serde::Serialize)]
struct ActiveConnection {
    id: String,
    #[serde(rename = "type")]
    connection_type: String,
    default: bool,
}

#[allow(dead_code)]
#[derive(Debug, Clone, serde::Serialize)]
struct NetworkDeviceDetail {
    interface: String,
    #[serde(rename = "type")]
    device_type: String,
    state: String,
    mac: String,
    mtu: u64,
    ipv4: IpConfig,
    ipv6: Ipv6Config,
    connection: Option<ActiveConnection>,
    dhcp4: Option<Value>,
}

/// Whether a device should appear in `network list` without `--all`.
///
/// Keeps managed devices and physical/interesting types; hides unmanaged
/// virtual interfaces (container `veth`, `tun`, etc.).
fn keep_device(device_type: u64, managed: bool) -> bool {
    managed || PHYSICAL_TYPES.contains(&device_type)
}

/// Unwrap a cockpit variant value (`{"t":<sig>,"v":<value>}`), falling back to
/// the value itself when it is already flat.
fn unwrap_variant(v: &Value) -> &Value {
    v.get("v").unwrap_or(v)
}

/// Read a string property from an unwrapped `a{sv}` map.
fn prop_str(props: &Value, key: &str) -> String {
    props
        .get(key)
        .map(unwrap_variant)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// Read a `u64` property from an unwrapped `a{sv}` map.
fn prop_u64(props: &Value, key: &str) -> u64 {
    props
        .get(key)
        .map(unwrap_variant)
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

/// Read a `bool` property from an unwrapped `a{sv}` map.
fn prop_bool(props: &Value, key: &str) -> bool {
    props
        .get(key)
        .map(unwrap_variant)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// Read an object-path property, treating the NM null path `"/"` as absent.
fn prop_path(props: &Value, key: &str) -> Option<String> {
    let p = prop_str(props, key);
    if p.is_empty() || p == "/" {
        None
    } else {
        Some(p)
    }
}

impl NetworkDevice {
    fn from_props(props: &Value) -> Self {
        Self {
            interface: prop_str(props, "Interface"),
            device_type: prop_u64(props, "DeviceType"),
            state: prop_u64(props, "State"),
            managed: prop_bool(props, "Managed"),
            mac: prop_str(props, "HwAddress"),
            mtu: prop_u64(props, "Mtu"),
            ip4_config: prop_path(props, "Ip4Config"),
            ip6_config: prop_path(props, "Ip6Config"),
            active_connection: prop_path(props, "ActiveConnection"),
            dhcp4_config: prop_path(props, "Dhcp4Config"),
        }
    }

    fn should_list(&self, all: bool) -> bool {
        all || keep_device(self.device_type, self.managed)
    }

    fn type_name(&self) -> String {
        device_type_str(self.device_type)
    }

    fn state_name(&self) -> String {
        device_state_str(self.state)
    }
}

impl IpConfig {
    fn from_props(props: &Value) -> Self {
        Self {
            addresses: addresses(props),
            gateway: prop_str(props, "Gateway"),
            dns: nameservers(props),
            domains: domains_list(props),
        }
    }

    fn empty() -> Self {
        Self {
            addresses: Vec::new(),
            gateway: String::new(),
            dns: Vec::new(),
            domains: Vec::new(),
        }
    }

    fn primary_address(&self) -> String {
        self.addresses
            .first()
            .map(|address| {
                address
                    .split_once('/')
                    .map_or(address.as_str(), |(addr, _)| addr)
            })
            .unwrap_or("")
            .to_string()
    }
}

impl Ipv6Config {
    fn from_props(props: &Value) -> Self {
        Self {
            addresses: addresses(props),
        }
    }

    fn empty() -> Self {
        Self {
            addresses: Vec::new(),
        }
    }

    fn primary_address(&self) -> String {
        self.addresses
            .first()
            .map(|address| {
                address
                    .split_once('/')
                    .map_or(address.as_str(), |(addr, _)| addr)
            })
            .unwrap_or("")
            .to_string()
    }
}

#[allow(dead_code)]
impl ActiveConnection {
    fn from_props(props: &Value) -> Self {
        Self {
            id: prop_str(props, "Id"),
            connection_type: prop_str(props, "Type"),
            default: prop_bool(props, "Default"),
        }
    }
}

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
        Some(path) => Ok(IpConfig::from_props(&get_all(
            client, channel, path, IP4_IFACE,
        )?)),
        None => Ok(IpConfig::empty()),
    }
}

fn load_ip6_config(
    client: &mut BridgeClient,
    channel: &str,
    path: Option<&str>,
) -> Result<Ipv6Config> {
    match path {
        Some(path) => Ok(Ipv6Config::from_props(&get_all(
            client, channel, path, IP6_IFACE,
        )?)),
        None => Ok(Ipv6Config::empty()),
    }
}

/// Project an NM `AddressData` (`aa{sv}`) entry to `"address/prefix"`.
fn address_entry(entry: &Value) -> Option<String> {
    let addr = entry.get("address").map(unwrap_variant)?.as_str()?;
    let prefix = entry
        .get("prefix")
        .map(unwrap_variant)
        .and_then(Value::as_u64);
    Some(match prefix {
        Some(p) => format!("{addr}/{p}"),
        None => addr.to_string(),
    })
}

/// Collect every `"address/prefix"` from an IP config's `AddressData`.
fn addresses(ip_props: &Value) -> Vec<String> {
    ip_props
        .get("AddressData")
        .map(unwrap_variant)
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(address_entry).collect())
        .unwrap_or_default()
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
fn list(client: &mut BridgeClient, channel: &str, host: String, all: bool) -> Result<View> {
    let paths = device_paths(client, channel)?;

    let mut devices = Vec::new();
    for path in &paths {
        let device = NetworkDevice::from_props(&get_all(client, channel, path, DEVICE_IFACE)?);
        if !device.should_list(all) {
            continue;
        }
        let ip4 = load_ip4_config(client, channel, device.ip4_config.as_deref())?;
        let ip6 = load_ip6_config(client, channel, device.ip6_config.as_deref())?;
        devices.push(json!({
            "interface": device.interface,
            "type": device.type_name(),
            "state": device.state_name(),
            "ip4": ip4.primary_address(),
            "ip6": ip6.primary_address(),
            "mac": device.mac,
        }));
    }

    let mut human = format!(
        "{:<14} {:<10} {:<13} {:<20} {}\n",
        "DEVICE", "TYPE", "STATE", "IPV4", "MAC"
    );
    for d in &devices {
        human.push_str(&format!(
            "{:<14} {:<10} {:<13} {:<20} {}\n",
            ds(d, "interface"),
            ds(d, "type"),
            ds(d, "state"),
            ds(d, "ip4"),
            ds(d, "mac"),
        ));
    }

    let columns = ["interface", "type", "state", "ip4", "ip6", "mac"];
    let rows: Vec<Value> = devices
        .iter()
        .map(|d| Value::Array(columns.iter().map(|c| d[*c].clone()).collect()))
        .collect();
    Ok(View::new(
        "NetworkDeviceList",
        host,
        crate::envelope::table_data(&columns, rows),
        human,
    ))
}

/// Show one device's full network detail, chasing NM's object indirection.
fn show(client: &mut BridgeClient, channel: &str, host: String, device: &str) -> Result<View> {
    let paths = device_paths(client, channel)?;

    // Find the device whose Interface matches the requested name.
    let mut found: Option<Value> = None;
    for path in &paths {
        let props = get_all(client, channel, path, DEVICE_IFACE)?;
        if prop_str(&props, "Interface") == device {
            found = Some(props);
            break;
        }
    }
    let props = found.ok_or_else(|| FezError::NotFound(format!("network device {device}")))?;

    let device_type = prop_u64(&props, "DeviceType");
    let state = device_state_str(prop_u64(&props, "State"));

    // IPv4 detail.
    let (mut ip4_addrs, mut gateway, mut dns, mut domains) =
        (Vec::new(), String::new(), Vec::new(), Vec::new());
    if let Some(p) = prop_path(&props, "Ip4Config") {
        let ip = get_all(client, channel, &p, IP4_IFACE)?;
        ip4_addrs = addresses(&ip);
        gateway = prop_str(&ip, "Gateway");
        dns = nameservers(&ip);
        domains = domains_list(&ip);
    }

    // IPv6 detail.
    let mut ip6_addrs = Vec::new();
    if let Some(p) = prop_path(&props, "Ip6Config") {
        let ip = get_all(client, channel, &p, IP6_IFACE)?;
        ip6_addrs = addresses(&ip);
    }

    // Active connection profile.
    let connection = match prop_path(&props, "ActiveConnection") {
        Some(p) => {
            let ac = get_all(client, channel, &p, ACTIVE_IFACE)?;
            Some(json!({
                "id": prop_str(&ac, "Id"),
                "type": prop_str(&ac, "Type"),
                "default": prop_bool(&ac, "Default"),
            }))
        }
        None => None,
    };

    // DHCPv4 lease options. NM hands these back as an `a{sv}` whose values are
    // variant-wrapped; flatten them so the envelope carries clean scalars
    // instead of leaking the `{"t","v"}` wire shape.
    let dhcp = match prop_path(&props, "Dhcp4Config") {
        Some(p) => {
            let d = get_all(client, channel, &p, DHCP4_IFACE)?;
            d.get("Options")
                .map(unwrap_variant)
                .and_then(flatten_options)
        }
        None => None,
    };

    let data = json!({
        "interface": prop_str(&props, "Interface"),
        "type": device_type_str(device_type),
        "state": state,
        "mac": prop_str(&props, "HwAddress"),
        "mtu": prop_u64(&props, "Mtu"),
        "ipv4": { "addresses": ip4_addrs, "gateway": gateway, "dns": dns, "domains": domains },
        "ipv6": { "addresses": ip6_addrs },
        "connection": connection,
        "dhcp4": dhcp,
    });

    let human = render_show_human(&data);
    Ok(View::new("NetworkDeviceDetail", host, data, human))
}

/// Flatten an `a{sv}` options map by unwrapping each variant value, so the
/// envelope carries plain scalars instead of the `{"t","v"}` wire shape.
fn flatten_options(opts: &Value) -> Option<Value> {
    let obj = opts.as_object()?;
    let flat: serde_json::Map<String, Value> = obj
        .iter()
        .map(|(k, v)| (k.clone(), unwrap_variant(v).clone()))
        .collect();
    Some(Value::Object(flat))
}

/// Collect DNS server addresses from an IP config's `NameserverData`.
fn nameservers(ip_props: &Value) -> Vec<String> {
    ip_props
        .get("NameserverData")
        .map(unwrap_variant)
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|e| {
                    e.get("address")
                        .map(unwrap_variant)
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Collect search domains from an IP config's `Domains` (`as`).
fn domains_list(ip_props: &Value) -> Vec<String> {
    ip_props
        .get("Domains")
        .map(unwrap_variant)
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Render the human form of a `network show` detail object.
fn render_show_human(d: &Value) -> String {
    let mut out = String::new();
    out.push_str(&format!("Device:    {}\n", json_str(d, "interface")));
    out.push_str(&format!("Type:      {}\n", json_str(d, "type")));
    out.push_str(&format!("State:     {}\n", json_str(d, "state")));
    out.push_str(&format!("MAC:       {}\n", json_str(d, "mac")));
    out.push_str(&format!(
        "MTU:       {}\n",
        d.get("mtu").and_then(Value::as_u64).unwrap_or(0)
    ));
    let ip4 = &d["ipv4"];
    out.push_str(&format!("IPv4:      {}\n", join_arr(ip4, "addresses")));
    out.push_str(&format!("Gateway:   {}\n", json_str(ip4, "gateway")));
    out.push_str(&format!("DNS:       {}\n", join_arr(ip4, "dns")));
    out.push_str(&format!("Domains:   {}\n", join_arr(ip4, "domains")));
    out.push_str(&format!(
        "IPv6:      {}\n",
        join_arr(&d["ipv6"], "addresses")
    ));
    if let Some(conn) = d.get("connection").filter(|c| !c.is_null()) {
        out.push_str(&format!(
            "Connection: {} ({})\n",
            json_str(conn, "id"),
            json_str(conn, "type")
        ));
    }
    out
}

/// Fetch a string field from a JSON object, empty when absent.
fn json_str(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

/// Join a JSON string array field with commas, empty when absent.
fn join_arr(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

/// Fetch a string field from a device row object built by [`list`].
fn ds(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_type_decodes_known_and_unknown() {
        assert_eq!(device_type_str(1), "ethernet");
        assert_eq!(device_type_str(2), "wifi");
        assert_eq!(device_type_str(32), "loopback");
        assert_eq!(device_type_str(20), "veth");
        assert_eq!(device_type_str(999), "type-999");
    }

    #[test]
    fn device_state_decodes_known_and_unknown() {
        assert_eq!(device_state_str(100), "activated");
        assert_eq!(device_state_str(20), "unavailable");
        assert_eq!(device_state_str(10), "unmanaged");
        assert_eq!(device_state_str(777), "state-777");
    }

    #[test]
    fn filter_keeps_managed_and_physical_drops_unmanaged_virtual() {
        // Managed ethernet kept.
        assert!(keep_device(1, true));
        // Unmanaged loopback kept by type.
        assert!(keep_device(32, false));
        // Unmanaged veth dropped.
        assert!(!keep_device(20, false));
        // Managed veth kept (managed overrides type).
        assert!(keep_device(20, true));
    }

    #[test]
    fn unwrap_variant_handles_wrapped_and_flat() {
        assert_eq!(unwrap_variant(&json!({"t":"s","v":"x"})), &json!("x"));
        assert_eq!(unwrap_variant(&json!("x")), &json!("x"));
    }

    #[test]
    fn address_entry_projects_address_and_prefix() {
        let e = json!({"address":{"t":"s","v":"10.0.0.5"},"prefix":{"t":"u","v":24}});
        assert_eq!(address_entry(&e).as_deref(), Some("10.0.0.5/24"));
        // Missing prefix falls back to the bare address.
        let e = json!({"address":{"t":"s","v":"10.0.0.5"}});
        assert_eq!(address_entry(&e).as_deref(), Some("10.0.0.5"));
        // No address yields None.
        assert_eq!(address_entry(&json!({})), None);
    }

    #[test]
    fn flatten_options_unwraps_variant_values() {
        let opts = json!({
            "routers": {"t":"s","v":"192.168.10.1"},
            "lease_time": {"t":"s","v":"3600"},
        });
        let flat = flatten_options(&opts).unwrap();
        assert_eq!(flat["routers"], json!("192.168.10.1"));
        assert_eq!(flat["lease_time"], json!("3600"));
        // A non-object input yields None.
        assert_eq!(flatten_options(&json!("x")), None);
    }

    #[test]
    fn prop_path_treats_null_path_as_absent() {
        let props = json!({"Ip4Config":{"t":"o","v":"/"},"Ok":{"t":"o","v":"/x/1"}});
        assert_eq!(prop_path(&props, "Ip4Config"), None);
        assert_eq!(prop_path(&props, "Ok").as_deref(), Some("/x/1"));
        assert_eq!(prop_path(&props, "Missing"), None);
    }

    #[test]
    fn network_device_from_props_unwraps_known_fields() {
        let props = json!({
            "Interface": {"t":"s","v":"eth0"},
            "DeviceType": {"t":"u","v":1},
            "State": {"t":"u","v":100},
            "Managed": {"t":"b","v":true},
            "HwAddress": {"t":"s","v":"52:54:00:00:00:01"},
            "Mtu": {"t":"u","v":1500},
            "Ip4Config": {"t":"o","v":"/org/freedesktop/NetworkManager/IP4Config/1"},
            "Ip6Config": {"t":"o","v":"/"},
            "ActiveConnection": {"t":"o","v":"/org/freedesktop/NetworkManager/ActiveConnection/1"},
            "Dhcp4Config": {"t":"o","v":"/org/freedesktop/NetworkManager/DHCP4Config/1"},
        });

        let device = NetworkDevice::from_props(&props);

        assert_eq!(device.interface, "eth0");
        assert_eq!(device.device_type, 1);
        assert_eq!(device.state, 100);
        assert!(device.managed);
        assert_eq!(device.mac, "52:54:00:00:00:01");
        assert_eq!(device.mtu, 1500);
        assert_eq!(
            device.ip4_config.as_deref(),
            Some("/org/freedesktop/NetworkManager/IP4Config/1")
        );
        assert_eq!(device.ip6_config, None);
        assert_eq!(
            device.active_connection.as_deref(),
            Some("/org/freedesktop/NetworkManager/ActiveConnection/1")
        );
        assert_eq!(
            device.dhcp4_config.as_deref(),
            Some("/org/freedesktop/NetworkManager/DHCP4Config/1")
        );
        assert!(device.should_list(false));
        assert!(device.should_list(true));
        assert_eq!(device.type_name(), "ethernet");
        assert_eq!(device.state_name(), "activated");
    }

    #[test]
    fn ip_config_primary_address_drops_prefix() {
        let props = json!({
            "AddressData": {"t":"aa{sv}","v":[
                {"address":{"t":"s","v":"192.0.2.10"},"prefix":{"t":"u","v":24}},
                {"address":{"t":"s","v":"192.0.2.11"},"prefix":{"t":"u","v":24}}
            ]},
            "Gateway": {"t":"s","v":"192.0.2.1"},
            "NameserverData": {"t":"aa{sv}","v":[{"address":{"t":"s","v":"1.1.1.1"}}]},
            "Domains": {"t":"as","v":["example.test"]}
        });

        let config = IpConfig::from_props(&props);

        assert_eq!(config.addresses, vec!["192.0.2.10/24", "192.0.2.11/24"]);
        assert_eq!(config.gateway, "192.0.2.1");
        assert_eq!(config.dns, vec!["1.1.1.1"]);
        assert_eq!(config.domains, vec!["example.test"]);
        assert_eq!(config.primary_address(), "192.0.2.10");
        assert_eq!(IpConfig::empty().primary_address(), "");
    }

    #[test]
    fn active_connection_from_props_unwraps_known_fields() {
        let props = json!({
            "Id": {"t":"s","v":"Wired connection 1"},
            "Type": {"t":"s","v":"802-3-ethernet"},
            "Default": {"t":"b","v":true},
        });

        let connection = ActiveConnection::from_props(&props);

        assert_eq!(connection.id, "Wired connection 1");
        assert_eq!(connection.connection_type, "802-3-ethernet");
        assert!(connection.default);
    }
}
