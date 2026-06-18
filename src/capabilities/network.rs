//! NetworkManager inspection capability.
//!
//! Reads the readable NetworkManager surface over the cockpit-bridge
//! `dbus-json3` channel (`org.freedesktop.NetworkManager`, system bus,
//! unprivileged). Two actions: `network list` (device inventory) and
//! `network show <device>` (full per-device detail). Read-only: no mutation,
//! no privilege escalation.

use crate::capabilities::{render, CapabilityContext, View};
use crate::cli::{Cli, NetworkAction};
use crate::error::{FezError, Result};
use crate::protocol::client::BridgeClient;
use crate::protocol::variant::Variant;
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
    let mut client = crate::capabilities::connect(cli)?;
    let host = client.host().to_string();
    let channel = client.dbus_open(NM_NAME)?;
    let mut ctx = CapabilityContext {
        client: &mut client,
        channel: &channel,
        host: &host,
    };
    match action {
        NetworkAction::List { all } => list(&mut ctx, *all),
        NetworkAction::Show { device } => show(&mut ctx, device),
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

#[derive(Debug, Clone)]
struct NetworkDevice {
    interface: String,
    device_type: u64,
    state: u64,
    managed: bool,
    mac: String,
    mtu: u64,
    ip4_config: Option<String>,
    ip6_config: Option<String>,
    active_connection: Option<String>,
    dhcp4_config: Option<String>,
}

#[derive(Debug, Clone)]
struct NetworkDeviceSummary {
    interface: String,
    device_type: String,
    state: String,
    ip4: String,
    ip6: String,
    mac: String,
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

#[derive(Debug, Clone, serde::Serialize)]
struct ActiveConnection {
    id: String,
    #[serde(rename = "type")]
    connection_type: String,
    default: bool,
}

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

/// D-Bus device properties from `org.freedesktop.NetworkManager.Device`.
#[derive(Debug, Default, serde::Deserialize)]
struct DeviceProps {
    #[serde(rename = "Interface", default)]
    interface: Variant<String>,
    #[serde(rename = "DeviceType", default)]
    device_type: Variant<u64>,
    #[serde(rename = "State", default)]
    state: Variant<u64>,
    #[serde(rename = "Managed", default)]
    managed: Variant<bool>,
    #[serde(rename = "HwAddress", default)]
    hw_address: Variant<String>,
    #[serde(rename = "Mtu", default)]
    mtu: Variant<u64>,
    #[serde(rename = "Ip4Config", default)]
    ip4_config: Variant<String>,
    #[serde(rename = "Ip6Config", default)]
    ip6_config: Variant<String>,
    #[serde(rename = "ActiveConnection", default)]
    active_connection: Variant<String>,
    #[serde(rename = "Dhcp4Config", default)]
    dhcp4_config: Variant<String>,
}

/// Treat the NM null object path `"/"` and empty string as absent.
fn nm_path(s: String) -> Option<String> {
    if s.is_empty() || s == "/" {
        None
    } else {
        Some(s)
    }
}

/// An entry from NM's `AddressData` (`aa{sv}`).
#[derive(Debug, Default, serde::Deserialize)]
struct AddressDataEntry {
    #[serde(default)]
    address: Variant<String>,
    #[serde(default)]
    prefix: Variant<u64>,
}

/// An entry from NM's `NameserverData` (`aa{sv}`).
#[derive(Debug, Default, serde::Deserialize)]
struct NameserverDataEntry {
    #[serde(default)]
    address: Variant<String>,
}

/// IPv4/IPv6 config properties from `org.freedesktop.NetworkManager.IP4Config`
/// (or `IP6Config`; same property names, different interface).
#[derive(Debug, Default, serde::Deserialize)]
struct IpProps {
    #[serde(rename = "AddressData", default)]
    address_data: Variant<Vec<AddressDataEntry>>,
    #[serde(rename = "Gateway", default)]
    gateway: Variant<String>,
    #[serde(rename = "NameserverData", default)]
    nameserver_data: Variant<Vec<NameserverDataEntry>>,
    #[serde(rename = "Domains", default)]
    domains: Variant<Vec<String>>,
}

/// Active connection properties from
/// `org.freedesktop.NetworkManager.Connection.Active`.
#[derive(Debug, Default, serde::Deserialize)]
struct ActiveConnectionProps {
    #[serde(rename = "Id", default)]
    id: Variant<String>,
    #[serde(rename = "Type", default)]
    connection_type: Variant<String>,
    #[serde(rename = "Default", default)]
    default: Variant<bool>,
}

/// Format address entries as `"address/prefix"` strings, dropping entries
/// without an address.
fn format_addresses(entries: &[AddressDataEntry]) -> Vec<String> {
    entries
        .iter()
        .filter(|e| !e.address.0.is_empty())
        .map(|e| {
            if e.prefix.0 > 0 {
                format!("{}/{}", e.address.0, e.prefix.0)
            } else {
                e.address.0.clone()
            }
        })
        .collect()
}

impl NetworkDevice {
    fn from_value(val: Value) -> Result<Self> {
        let props: DeviceProps = serde_json::from_value(val).map_err(FezError::Decode)?;
        Ok(Self {
            interface: props.interface.0,
            device_type: props.device_type.0,
            state: props.state.0,
            managed: props.managed.0,
            mac: props.hw_address.0,
            mtu: props.mtu.0,
            ip4_config: nm_path(props.ip4_config.0),
            ip6_config: nm_path(props.ip6_config.0),
            active_connection: nm_path(props.active_connection.0),
            dhcp4_config: nm_path(props.dhcp4_config.0),
        })
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

impl NetworkDeviceSummary {
    fn from_device(device: &NetworkDevice, ip4: &IpConfig, ip6: &Ipv6Config) -> Self {
        Self {
            interface: device.interface.clone(),
            device_type: device.type_name(),
            state: device.state_name(),
            ip4: ip4.primary_address(),
            ip6: ip6.primary_address(),
            mac: device.mac.clone(),
        }
    }

    fn table_row(&self) -> Vec<Value> {
        vec![
            json!(self.interface),
            json!(self.device_type),
            json!(self.state),
            json!(self.ip4),
            json!(self.ip6),
            json!(self.mac),
        ]
    }
}

impl IpConfig {
    fn from_value(val: Value) -> Result<Self> {
        let props: IpProps = serde_json::from_value(val).map_err(FezError::Decode)?;
        Ok(Self {
            addresses: format_addresses(&props.address_data.0),
            gateway: props.gateway.0,
            dns: props
                .nameserver_data
                .0
                .iter()
                .filter(|e| !e.address.0.is_empty())
                .map(|e| e.address.0.clone())
                .collect(),
            domains: props.domains.0,
        })
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
    fn from_value(val: Value) -> Result<Self> {
        let props: IpProps = serde_json::from_value(val).map_err(FezError::Decode)?;
        Ok(Self {
            addresses: format_addresses(&props.address_data.0),
        })
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

impl ActiveConnection {
    fn from_value(val: Value) -> Result<Self> {
        let props: ActiveConnectionProps = serde_json::from_value(val).map_err(FezError::Decode)?;
        Ok(Self {
            id: props.id.0,
            connection_type: props.connection_type.0,
            default: props.default.0,
        })
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
        Some(path) => IpConfig::from_value(get_all(client, channel, path, IP4_IFACE)?),
        None => Ok(IpConfig::empty()),
    }
}

fn load_ip6_config(
    client: &mut BridgeClient,
    channel: &str,
    path: Option<&str>,
) -> Result<Ipv6Config> {
    match path {
        Some(path) => Ipv6Config::from_value(get_all(client, channel, path, IP6_IFACE)?),
        None => Ok(Ipv6Config::empty()),
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
            // ponytail: manual unwrap here — dynamic keys can't be a derive struct
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
fn list(ctx: &mut CapabilityContext<'_>, all: bool) -> Result<View> {
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
        devices.push(NetworkDeviceSummary::from_device(&device, &ip4, &ip6));
    }

    let mut human = format!(
        "{:<14} {:<10} {:<13} {:<20} {}\n",
        "DEVICE", "TYPE", "STATE", "IPV4", "MAC"
    );
    for d in &devices {
        human.push_str(&format!(
            "{:<14} {:<10} {:<13} {:<20} {}\n",
            d.interface, d.device_type, d.state, d.ip4, d.mac,
        ));
    }

    let columns = ["interface", "type", "state", "ip4", "ip6", "mac"];
    let rows: Vec<Value> = devices
        .iter()
        .map(|d| Value::Array(d.table_row()))
        .collect();
    Ok(View::new(
        "NetworkDeviceList",
        ctx.host,
        crate::envelope::table_data(&columns, rows),
        human,
    ))
}

/// Show one device's full network detail, chasing NM's object indirection.
fn show(ctx: &mut CapabilityContext<'_>, device: &str) -> Result<View> {
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
/// ponytail: inline variant unwrap — dynamic DHCP keys can't use a derive struct
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
    fn nm_path_treats_null_path_and_empty_as_absent() {
        assert_eq!(nm_path("/".into()), None);
        assert_eq!(nm_path("".into()), None);
        assert_eq!(nm_path("/x/1".into()).as_deref(), Some("/x/1"));
    }

    #[test]
    fn format_addresses_projects_address_and_prefix() {
        let entries = vec![
            AddressDataEntry {
                address: Variant("10.0.0.5".into()),
                prefix: Variant(24),
            },
            AddressDataEntry {
                address: Variant("10.0.0.6".into()),
                prefix: Variant(0), // absent prefix defaults to 0 → bare address
            },
            AddressDataEntry {
                address: Variant(String::new()),
                prefix: Variant(24), // empty address → filtered out
            },
        ];
        assert_eq!(format_addresses(&entries), vec!["10.0.0.5/24", "10.0.0.6"]);
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
    fn network_device_from_value_unwraps_known_fields() {
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

        let device = NetworkDevice::from_value(props).unwrap();

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
    fn list_summary_projects_display_and_table_rows() {
        let device = NetworkDevice {
            interface: "eth0".into(),
            device_type: 1,
            state: 100,
            managed: true,
            mac: "52:54:00:00:00:01".into(),
            mtu: 1500,
            ip4_config: None,
            ip6_config: None,
            active_connection: None,
            dhcp4_config: None,
        };
        let ip4 = IpConfig {
            addresses: vec!["192.0.2.10/24".into()],
            gateway: "192.0.2.1".into(),
            dns: Vec::new(),
            domains: Vec::new(),
        };
        let ip6 = Ipv6Config {
            addresses: vec!["2001:db8::10/64".into()],
        };

        let summary = NetworkDeviceSummary::from_device(&device, &ip4, &ip6);

        assert_eq!(summary.interface, "eth0");
        assert_eq!(summary.device_type, "ethernet");
        assert_eq!(summary.state, "activated");
        assert_eq!(summary.ip4, "192.0.2.10");
        assert_eq!(summary.ip6, "2001:db8::10");
        assert_eq!(summary.mac, "52:54:00:00:00:01");
        assert_eq!(
            summary.table_row(),
            vec![
                json!("eth0"),
                json!("ethernet"),
                json!("activated"),
                json!("192.0.2.10"),
                json!("2001:db8::10"),
                json!("52:54:00:00:00:01"),
            ]
        );
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

        let config = IpConfig::from_value(props).unwrap();

        assert_eq!(config.addresses, vec!["192.0.2.10/24", "192.0.2.11/24"]);
        assert_eq!(config.gateway, "192.0.2.1");
        assert_eq!(config.dns, vec!["1.1.1.1"]);
        assert_eq!(config.domains, vec!["example.test"]);
        assert_eq!(config.primary_address(), "192.0.2.10");
        assert_eq!(IpConfig::empty().primary_address(), "");
    }

    #[test]
    fn active_connection_from_value_unwraps_known_fields() {
        let props = json!({
            "Id": {"t":"s","v":"Wired connection 1"},
            "Type": {"t":"s","v":"802-3-ethernet"},
            "Default": {"t":"b","v":true},
        });

        let connection = ActiveConnection::from_value(props).unwrap();

        assert_eq!(connection.id, "Wired connection 1");
        assert_eq!(connection.connection_type, "802-3-ethernet");
        assert!(connection.default);
    }
}
