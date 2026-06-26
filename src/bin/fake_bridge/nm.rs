//! Canned NetworkManager (`org.freedesktop.NetworkManager`) replies.

use super::{err_reply, ok_reply, ROOT_PATH};
use serde_json::{json, Value};

/// NetworkManager root manager object path.
pub(super) const NM_MGR_PATH: &str = "/org/freedesktop/NetworkManager";

/// Canned NetworkManager reply for a call against a NM object path.
///
/// NM reuses the generic `Get`/`GetAll` property methods across many object
/// types, so the fake disambiguates by **object path** rather than method name.
/// Every `a{sv}`/variant value is variant-wrapped
/// (`{"t":<sig>,"v":<value>}`) exactly like real cockpit-bridge.
///
/// Canned topology (returned by `GetDevices`):
/// - `/Devices/1` `enp1s0`: ethernet (type 1), activated (100), managed; full
///   IPv4/IPv6/active connection/DHCP.
/// - `/Devices/2` `enp2s0`: ethernet (type 1), unavailable (20), managed; no IP
///   config (object path `"/"`) to exercise the null-config guard.
/// - `/Devices/3` `lo`: loopback (type 32), unmanaged (10).
/// - `/Devices/9` `veth0`: veth (type 20), unmanaged (10); hidden by the
///   default filter, shown only with `--all`.
const NM_DNS_PATH: &str = "/org/freedesktop/NetworkManager/DnsManager";

pub(super) fn nm_reply(path: &str, method: &str, id: &Value) -> Value {
    if path == NM_DNS_PATH && method == "GetAll" {
        return ok_reply(
            id,
            json!([{
                "Mode": {"t": "s", "v": "default"},
                "RcManager": {"t": "s", "v": "symlink"},
                "Configuration": {"t": "aa{sv}", "v": [
                    {
                        "nameservers": {"t": "as", "v": ["192.168.1.1"]},
                        "interface": {"t": "s", "v": "enp1s0"},
                        "priority": {"t": "i", "v": 100},
                        "vpn": {"t": "b", "v": false}
                    }
                ]}
            }]),
        );
    }
    if path == NM_MGR_PATH {
        return match method {
            "GetDevices" => ok_reply(
                id,
                json!([[
                    format!("{NM_MGR_PATH}/Devices/1"),
                    format!("{NM_MGR_PATH}/Devices/2"),
                    format!("{NM_MGR_PATH}/Devices/3"),
                    format!("{NM_MGR_PATH}/Devices/9"),
                ]]),
            ),
            other => nm_unknown(other, id),
        };
    }
    if let Some(n) = path.strip_prefix(&format!("{NM_MGR_PATH}/Devices/")) {
        return match method {
            "GetAll" => nm_device_props(n, id),
            other => nm_unknown(other, id),
        };
    }
    if path.starts_with(&format!("{NM_MGR_PATH}/IP4Config/")) {
        return match method {
            "GetAll" => ok_reply(
                id,
                json!([{
                    "AddressData": {"t":"aa{sv}","v":[
                        {"address":{"t":"s","v":"192.168.10.20"},"prefix":{"t":"u","v":24}}
                    ]},
                    "Gateway": {"t":"s","v":"192.168.10.1"},
                    "NameserverData": {"t":"aa{sv}","v":[
                        {"address":{"t":"s","v":"192.168.10.1"}},
                        {"address":{"t":"s","v":"1.1.1.1"}}
                    ]},
                    "Domains": {"t":"as","v":["lan"]},
                }]),
            ),
            other => nm_unknown(other, id),
        };
    }
    if path.starts_with(&format!("{NM_MGR_PATH}/IP6Config/")) {
        return match method {
            "GetAll" => ok_reply(
                id,
                json!([{
                    "AddressData": {"t":"aa{sv}","v":[
                        {"address":{"t":"s","v":"fd00::20"},"prefix":{"t":"u","v":64}}
                    ]},
                    "Gateway": {"t":"s","v":"fd00::1"},
                }]),
            ),
            other => nm_unknown(other, id),
        };
    }
    if path.starts_with(&format!("{NM_MGR_PATH}/ActiveConnection/")) {
        return match method {
            "GetAll" => ok_reply(
                id,
                json!([{
                    "Id": {"t":"s","v":"enp1s0"},
                    "Type": {"t":"s","v":"802-3-ethernet"},
                    "Default": {"t":"b","v":true},
                }]),
            ),
            other => nm_unknown(other, id),
        };
    }
    if path.starts_with(&format!("{NM_MGR_PATH}/DHCP4Config/")) {
        return match method {
            "GetAll" => ok_reply(
                id,
                json!([{
                    "Options": {"t":"a{sv}","v":{
                        "routers": {"t":"s","v":"192.168.10.1"},
                        "ip_address": {"t":"s","v":"192.168.10.20"},
                    }},
                }]),
            ),
            other => nm_unknown(other, id),
        };
    }
    nm_unknown(method, id)
}

/// Build the `a{sv}` device property map for `Devices/<n>` GetAll.
fn nm_device_props(n: &str, id: &Value) -> Value {
    let nm = NM_MGR_PATH;
    let (iface, dtype, state, managed, ip4, ip6, active, dhcp4) = match n {
        "1" => (
            "enp1s0",
            1u32,
            100u32,
            true,
            format!("{nm}/IP4Config/1"),
            format!("{nm}/IP6Config/1"),
            format!("{nm}/ActiveConnection/1"),
            format!("{nm}/DHCP4Config/1"),
        ),
        "2" => (
            "enp2s0",
            1u32,
            20u32,
            true,
            ROOT_PATH.to_string(),
            ROOT_PATH.to_string(),
            ROOT_PATH.to_string(),
            ROOT_PATH.to_string(),
        ),
        "3" => (
            "lo",
            32u32,
            10u32,
            false,
            ROOT_PATH.to_string(),
            ROOT_PATH.to_string(),
            ROOT_PATH.to_string(),
            ROOT_PATH.to_string(),
        ),
        _ => (
            "veth0",
            20u32,
            10u32,
            false,
            ROOT_PATH.to_string(),
            ROOT_PATH.to_string(),
            ROOT_PATH.to_string(),
            ROOT_PATH.to_string(),
        ),
    };
    ok_reply(
        id,
        json!([{
            "Interface": {"t":"s","v":iface},
            "DeviceType": {"t":"u","v":dtype},
            "State": {"t":"u","v":state},
            "Managed": {"t":"b","v":managed},
            "HwAddress": {"t":"s","v":"52:54:00:12:34:56"},
            "Mtu": {"t":"u","v":1500},
            "Ip4Config": {"t":"o","v":ip4},
            "Ip6Config": {"t":"o","v":ip6},
            "ActiveConnection": {"t":"o","v":active},
            "Dhcp4Config": {"t":"o","v":dhcp4},
        }]),
    )
}

/// Unknown-method D-Bus error for a NM call the fake does not model.
fn nm_unknown(method: &str, id: &Value) -> Value {
    err_reply(
        id,
        "org.freedesktop.DBus.Error.UnknownMethod",
        format!("no NM fake for {method}"),
    )
}
