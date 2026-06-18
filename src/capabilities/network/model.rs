use crate::error::{FezError, Result};
use crate::protocol::variant::Variant;
use serde_json::Value;

/// Decode an `NMDeviceType` (`u`) to its short string.
///
/// Mirrors the upstream `NMDeviceType` enum. Unrecognized values render as
/// `type-<n>` so a new NM enum value degrades gracefully.
pub(super) fn device_type_str(n: u64) -> String {
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
pub(super) fn device_state_str(n: u64) -> String {
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
pub(super) struct NetworkDevice {
    pub(super) interface: String,
    device_type: u64,
    state: u64,
    managed: bool,
    pub(super) mac: String,
    pub(super) mtu: u64,
    pub(super) ip4_config: Option<String>,
    pub(super) ip6_config: Option<String>,
    pub(super) active_connection: Option<String>,
    pub(super) dhcp4_config: Option<String>,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub(super) struct IpConfig {
    pub(super) addresses: Vec<String>,
    pub(super) gateway: String,
    pub(super) dns: Vec<String>,
    pub(super) domains: Vec<String>,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub(super) struct Ipv6Config {
    pub(super) addresses: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(super) struct ActiveConnection {
    pub(super) id: String,
    #[serde(rename = "type")]
    pub(super) connection_type: String,
    pub(super) default: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(super) struct NetworkDeviceDetail {
    pub(super) interface: String,
    #[serde(rename = "type")]
    pub(super) device_type: String,
    pub(super) state: String,
    pub(super) mac: String,
    pub(super) mtu: u64,
    pub(super) ipv4: IpConfig,
    pub(super) ipv6: Ipv6Config,
    pub(super) connection: Option<ActiveConnection>,
    pub(super) dhcp4: Option<Value>,
}

/// Whether a device should appear in `network list` without `--all`.
///
/// Keeps managed devices and physical/interesting types; hides unmanaged
/// virtual interfaces (container `veth`, `tun`, etc.).
pub(super) fn keep_device(device_type: u64, managed: bool) -> bool {
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
pub(super) fn nm_path(s: String) -> Option<String> {
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
    pub(super) fn from_value(val: Value) -> Result<Self> {
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

    pub(super) fn should_list(&self, all: bool) -> bool {
        all || keep_device(self.device_type, self.managed)
    }

    pub(super) fn type_name(&self) -> String {
        device_type_str(self.device_type)
    }

    pub(super) fn state_name(&self) -> String {
        device_state_str(self.state)
    }
}

impl IpConfig {
    pub(super) fn from_value(val: Value) -> Result<Self> {
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

    pub(super) fn primary_address(&self) -> String {
        primary_address_from(&self.addresses)
    }
}

impl Ipv6Config {
    pub(super) fn from_value(val: Value) -> Result<Self> {
        let props: IpProps = serde_json::from_value(val).map_err(FezError::Decode)?;
        Ok(Self {
            addresses: format_addresses(&props.address_data.0),
        })
    }

    pub(super) fn primary_address(&self) -> String {
        primary_address_from(&self.addresses)
    }
}

fn primary_address_from(addresses: &[String]) -> String {
    addresses
        .first()
        .map(|address| {
            address
                .split_once('/')
                .map_or(address.as_str(), |(addr, _)| addr)
        })
        .unwrap_or("")
        .to_string()
}

impl ActiveConnection {
    pub(super) fn from_value(val: Value) -> Result<Self> {
        let props: ActiveConnectionProps = serde_json::from_value(val).map_err(FezError::Decode)?;
        Ok(Self {
            id: props.id.0,
            connection_type: props.connection_type.0,
            default: props.default.0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn filter_keeps_managed_and_physical_drops_unmanaged_virtual() {
        assert!(keep_device(1, true));
        assert!(keep_device(32, false));
        assert!(!keep_device(20, false));
        assert!(keep_device(20, true));
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
        assert_eq!(IpConfig::default().primary_address(), "");
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
