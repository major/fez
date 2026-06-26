//! Typed model structs for DNS resolver data.

use crate::error::{FezError, Result};
use serde::Serialize;
use serde_json::Value;
use std::net::{Ipv4Addr, Ipv6Addr};

/// A single resolved DNS address.
#[derive(Debug, Clone, Serialize)]
pub(super) struct DnsAddress {
    /// Address family: `"ipv4"` or `"ipv6"`.
    pub family: String,
    /// Formatted address string.
    pub address: String,
    /// Interface index the address was learned on (0 = global).
    pub ifindex: i64,
}

/// Global DNS resolver configuration from manager properties.
#[derive(Debug, Serialize)]
pub(super) struct GlobalDnsConfig {
    pub dns_servers: Vec<DnsAddress>,
    pub current_server: Option<String>,
    pub dnssec: String,
    pub dnssec_supported: bool,
    pub dns_over_tls: String,
    pub llmnr: String,
    pub multicast_dns: String,
    pub resolv_conf_mode: String,
    pub cache_size: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub transactions_current: u64,
    pub transactions_total: u64,
}

/// Per-link DNS configuration.
#[derive(Debug, Serialize)]
pub(super) struct LinkDnsConfig {
    pub ifindex: u32,
    pub dns_servers: Vec<DnsAddress>,
    pub current_server: Option<String>,
    pub dnssec: String,
    pub dnssec_supported: bool,
    pub dns_over_tls: String,
    pub llmnr: String,
    pub multicast_dns: String,
    pub default_route: bool,
}

/// DNS query result.
#[derive(Debug, Serialize)]
pub(super) struct QueryResult {
    pub hostname: String,
    pub canonical: String,
    pub addresses: Vec<DnsAddress>,
}

// -- D-Bus variant helpers --

/// Unwrap a variant-wrapped D-Bus property value (`{"t": ..., "v": val}` → `val`).
fn unwrap_variant(val: &Value) -> &Value {
    val.get("v").unwrap_or(val)
}

fn unwrap_str(val: &Value) -> String {
    unwrap_variant(val).as_str().unwrap_or("").to_string()
}

fn unwrap_bool(val: &Value) -> bool {
    unwrap_variant(val).as_bool().unwrap_or(false)
}

// -- Address decoding --

/// Decode a DNS address from a D-Bus family code and byte array.
///
/// Family 2 = `AF_INET` (4 bytes → IPv4), family 10 = `AF_INET6` (16 bytes → IPv6).
/// Returns `None` for unknown families or mismatched byte lengths.
///
/// The byte array may arrive as either a JSON array of integers (fake bridge)
/// or a base64-encoded string (real cockpit-bridge encodes `ay` as base64).
pub(super) fn decode_dns_address(family: i64, bytes: &Value) -> Option<String> {
    let octets = decode_byte_array(bytes)?;
    match family {
        2 if octets.len() == 4 => {
            Some(Ipv4Addr::from([octets[0], octets[1], octets[2], octets[3]]).to_string())
        }
        10 if octets.len() == 16 => {
            let mut arr = [0u8; 16];
            arr.copy_from_slice(&octets);
            Some(Ipv6Addr::from(arr).to_string())
        }
        _ => None,
    }
}

/// Decode a byte array from either a JSON integer array or a base64 string.
///
/// cockpit-bridge encodes D-Bus `ay` values as base64 strings; the fake
/// bridge uses JSON integer arrays. This handles both transparently.
fn decode_byte_array(val: &Value) -> Option<Vec<u8>> {
    match val {
        Value::Array(arr) => {
            let bytes: Vec<u8> = arr
                .iter()
                .filter_map(Value::as_u64)
                .map(|b| b as u8)
                .collect();
            if bytes.len() == arr.len() {
                Some(bytes)
            } else {
                None
            }
        }
        Value::String(s) => base64_decode(s),
        _ => None,
    }
}

/// Minimal base64 decoder (RFC 4648, no padding required).
///
/// ponytail: 15-line stdlib-only decoder avoids adding a base64 crate dep
/// for one call site. Upgrade to the `base64` crate if a second consumer appears.
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut buf = Vec::with_capacity(input.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    for &ch in input.as_bytes() {
        if ch == b'=' {
            break;
        }
        let val = TABLE.iter().position(|&c| c == ch)? as u32;
        acc = (acc << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            buf.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(buf)
}

fn family_label(family: i64) -> &'static str {
    if family == 2 {
        "ipv4"
    } else {
        "ipv6"
    }
}

/// Parse a manager DNS array: `a(iiay)` → `(ifindex, family, bytes)`.
pub(super) fn parse_manager_dns_array(val: &Value) -> Vec<DnsAddress> {
    let arr = unwrap_variant(val);
    let Some(entries) = arr.as_array() else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let entry = entry.as_array()?;
            let ifindex = entry.first()?.as_i64()?;
            let family = entry.get(1)?.as_i64()?;
            let bytes = entry.get(2)?;
            let address = decode_dns_address(family, bytes)?;
            Some(DnsAddress {
                family: family_label(family).into(),
                address,
                ifindex,
            })
        })
        .collect()
}

/// Parse a link DNS array: `a(iay)` → `(family, bytes)` (no ifindex).
pub(super) fn parse_link_dns_array(val: &Value) -> Vec<DnsAddress> {
    let arr = unwrap_variant(val);
    let Some(entries) = arr.as_array() else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let entry = entry.as_array()?;
            let family = entry.first()?.as_i64()?;
            let bytes = entry.get(1)?;
            let address = decode_dns_address(family, bytes)?;
            Some(DnsAddress {
                family: family_label(family).into(),
                address,
                ifindex: 0,
            })
        })
        .collect()
}

/// Parse the current DNS server from a manager `(iiay)` or link `(iay)` tuple.
fn parse_current_server(val: &Value, manager: bool) -> Option<String> {
    let arr = unwrap_variant(val).as_array()?;
    if manager {
        let family = arr.get(1)?.as_i64()?;
        decode_dns_address(family, arr.get(2)?)
    } else {
        let family = arr.first()?.as_i64()?;
        decode_dns_address(family, arr.get(1)?)
    }
}

impl GlobalDnsConfig {
    /// Parse global config from the manager's `GetAll` property dict.
    pub(super) fn from_value(val: Value) -> Result<Self> {
        let dns_servers = parse_manager_dns_array(
            val.get("DNS")
                .ok_or_else(|| FezError::Problem("missing DNS property".into()))?,
        );
        let current_server = val
            .get("CurrentDNSServer")
            .and_then(|v| parse_current_server(v, true));

        let cache = unwrap_variant(
            val.get("CacheStatistics")
                .ok_or_else(|| FezError::Problem("missing CacheStatistics".into()))?,
        );
        let cache_arr = cache.as_array();
        let cache_size = cache_arr.and_then(|a| a.first()?.as_u64()).unwrap_or(0);
        let cache_hits = cache_arr.and_then(|a| a.get(1)?.as_u64()).unwrap_or(0);
        let cache_misses = cache_arr.and_then(|a| a.get(2)?.as_u64()).unwrap_or(0);

        let tx = unwrap_variant(
            val.get("TransactionStatistics")
                .ok_or_else(|| FezError::Problem("missing TransactionStatistics".into()))?,
        );
        let tx_arr = tx.as_array();
        let transactions_current = tx_arr.and_then(|a| a.first()?.as_u64()).unwrap_or(0);
        let transactions_total = tx_arr.and_then(|a| a.get(1)?.as_u64()).unwrap_or(0);

        Ok(Self {
            dns_servers,
            current_server,
            dnssec: unwrap_str(val.get("DNSSEC").unwrap_or(&Value::Null)),
            dnssec_supported: unwrap_bool(val.get("DNSSECSupported").unwrap_or(&Value::Null)),
            dns_over_tls: unwrap_str(val.get("DNSOverTLS").unwrap_or(&Value::Null)),
            llmnr: unwrap_str(val.get("LLMNR").unwrap_or(&Value::Null)),
            multicast_dns: unwrap_str(val.get("MulticastDNS").unwrap_or(&Value::Null)),
            resolv_conf_mode: unwrap_str(val.get("ResolvConfMode").unwrap_or(&Value::Null)),
            cache_size,
            cache_hits,
            cache_misses,
            transactions_current,
            transactions_total,
        })
    }
}

impl LinkDnsConfig {
    /// Parse link config from a link's `GetAll` property dict.
    pub(super) fn from_value(ifindex: u32, val: Value) -> Result<Self> {
        let dns_servers = parse_link_dns_array(val.get("DNS").unwrap_or(&Value::Null));
        let current_server = val
            .get("CurrentDNSServer")
            .and_then(|v| parse_current_server(v, false));

        Ok(Self {
            ifindex,
            dns_servers,
            current_server,
            dnssec: unwrap_str(val.get("DNSSEC").unwrap_or(&Value::Null)),
            dnssec_supported: unwrap_bool(val.get("DNSSECSupported").unwrap_or(&Value::Null)),
            dns_over_tls: unwrap_str(val.get("DNSOverTLS").unwrap_or(&Value::Null)),
            llmnr: unwrap_str(val.get("LLMNR").unwrap_or(&Value::Null)),
            multicast_dns: unwrap_str(val.get("MulticastDNS").unwrap_or(&Value::Null)),
            default_route: unwrap_bool(val.get("DefaultRoute").unwrap_or(&Value::Null)),
        })
    }

    /// Whether this link has any DNS servers configured.
    pub(super) fn has_dns(&self) -> bool {
        !self.dns_servers.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decode_ipv4() {
        let addr = decode_dns_address(2, &json!([192, 168, 1, 1]));
        assert_eq!(addr, Some("192.168.1.1".into()));
    }

    #[test]
    fn decode_ipv6() {
        let addr = decode_dns_address(
            10,
            &json!([253, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]),
        );
        assert_eq!(addr, Some("fd00::1".into()));
    }

    #[test]
    fn decode_unknown_family_returns_none() {
        assert_eq!(decode_dns_address(99, &json!([])), None);
    }

    #[test]
    fn decode_wrong_length_returns_none() {
        assert_eq!(decode_dns_address(2, &json!([1, 2])), None);
    }

    #[test]
    fn decode_ipv4_base64() {
        // cockpit-bridge sends ay as base64: 192.168.10.1 = wKgKAQ==
        let addr = decode_dns_address(2, &json!("wKgKAQ=="));
        assert_eq!(addr, Some("192.168.10.1".into()));
    }

    #[test]
    fn decode_ipv6_base64() {
        // fd00::1 = /QAAAAAAAAAAAAAAAAAAAT
        let addr = decode_dns_address(10, &json!("/QAAAAAAAAAAAAAAAAAAAQ=="));
        assert_eq!(addr, Some("fd00::1".into()));
    }

    #[test]
    fn parse_manager_dns_array_works() {
        let val = json!({"t": "a(iiay)", "v": [
            [2, 2, [192, 168, 1, 1]],
            [0, 10, [253, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]]
        ]});
        let addrs = parse_manager_dns_array(&val);
        assert_eq!(addrs.len(), 2);
        assert_eq!(addrs[0].address, "192.168.1.1");
        assert_eq!(addrs[0].family, "ipv4");
        assert_eq!(addrs[0].ifindex, 2);
        assert_eq!(addrs[1].address, "fd00::1");
        assert_eq!(addrs[1].family, "ipv6");
    }

    #[test]
    fn parse_link_dns_array_works() {
        let val = json!({"t": "a(iay)", "v": [[2, [192, 168, 1, 1]]]});
        let addrs = parse_link_dns_array(&val);
        assert_eq!(addrs.len(), 1);
        assert_eq!(addrs[0].address, "192.168.1.1");
    }

    #[test]
    fn global_config_parses_canned_props() {
        let val = json!({
            "DNS": {"t": "a(iiay)", "v": [[2, 2, [192, 168, 1, 1]]]},
            "CurrentDNSServer": {"t": "(iiay)", "v": [2, 2, [192, 168, 1, 1]]},
            "DNSOverTLS": {"t": "s", "v": "no"},
            "DNSSEC": {"t": "s", "v": "no"},
            "DNSSECSupported": {"t": "b", "v": false},
            "LLMNR": {"t": "s", "v": "resolve"},
            "MulticastDNS": {"t": "s", "v": "no"},
            "ResolvConfMode": {"t": "s", "v": "stub"},
            "CacheStatistics": {"t": "(ttt)", "v": [100, 500, 50]},
            "TransactionStatistics": {"t": "(tt)", "v": [0, 1200]}
        });
        let config = GlobalDnsConfig::from_value(val).unwrap();
        assert_eq!(config.dns_servers.len(), 1);
        assert_eq!(config.dnssec, "no");
        assert_eq!(config.cache_size, 100);
        assert_eq!(config.cache_hits, 500);
        assert_eq!(config.cache_misses, 50);
    }

    #[test]
    fn link_config_with_dns() {
        let val = json!({
            "DNS": {"t": "a(iay)", "v": [[2, [192, 168, 1, 1]]]},
            "CurrentDNSServer": {"t": "(iay)", "v": [2, [192, 168, 1, 1]]},
            "DNSOverTLS": {"t": "s", "v": "no"},
            "DNSSEC": {"t": "s", "v": "no"},
            "DNSSECSupported": {"t": "b", "v": false},
            "DefaultRoute": {"t": "b", "v": true},
            "LLMNR": {"t": "s", "v": "resolve"},
            "MulticastDNS": {"t": "s", "v": "no"}
        });
        let link = LinkDnsConfig::from_value(2, val).unwrap();
        assert!(link.has_dns());
        assert_eq!(link.ifindex, 2);
        assert!(link.default_route);
    }

    #[test]
    fn link_config_without_dns() {
        let val = json!({
            "DNS": {"t": "a(iay)", "v": []},
            "CurrentDNSServer": {"t": "(iay)", "v": [0, []]},
            "DNSOverTLS": {"t": "s", "v": "no"},
            "DNSSEC": {"t": "s", "v": "no"},
            "DNSSECSupported": {"t": "b", "v": false},
            "DefaultRoute": {"t": "b", "v": false},
            "LLMNR": {"t": "s", "v": "resolve"},
            "MulticastDNS": {"t": "s", "v": "no"}
        });
        let link = LinkDnsConfig::from_value(3, val).unwrap();
        assert!(!link.has_dns());
    }
}
