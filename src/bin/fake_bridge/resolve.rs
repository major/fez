//! Canned systemd-resolved (resolve1) replies for the fake bridge.

use serde_json::{json, Value};

const RESOLVE_MGR_PATH: &str = "/org/freedesktop/resolve1";
const RESOLVE_MGR_IFACE: &str = "org.freedesktop.resolve1.Manager";
const PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";

/// D-Bus label-encode an ifindex for use as a node name / path segment.
///
/// D-Bus node names cannot start with a digit. systemd-resolved encodes the
/// leading character as underscore + two hex digits (ASCII value), leaving
/// remaining characters literal.
/// Examples: 2 → `_32`, 14 → `_314`, 130 → `_3130`.
fn encode_ifindex(idx: u32) -> String {
    let s = idx.to_string();
    let mut encoded = String::new();
    let mut chars = s.chars();
    if let Some(first) = chars.next() {
        encoded.push('_');
        encoded.push_str(&format!("{:02x}", first as u32));
        for c in chars {
            encoded.push(c);
        }
    }
    encoded
}

/// Canned global manager properties (`GetAll` on `resolve1.Manager`).
fn manager_props() -> Value {
    json!({
        "DNS": {"t": "a(iiay)", "v": [
            [2, 2, [192, 168, 1, 1]],
            [0, 10, [253, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]]
        ]},
        "CurrentDNSServer": {"t": "(iiay)", "v": [2, 2, [192, 168, 1, 1]]},
        "DNSOverTLS": {"t": "s", "v": "no"},
        "DNSSEC": {"t": "s", "v": "no"},
        "DNSSECSupported": {"t": "b", "v": false},
        "LLMNR": {"t": "s", "v": "resolve"},
        "MulticastDNS": {"t": "s", "v": "no"},
        "ResolvConfMode": {"t": "s", "v": "stub"},
        "DNSStubListener": {"t": "s", "v": "yes"},
        "CacheStatistics": {"t": "(ttt)", "v": [100, 500, 50]},
        "TransactionStatistics": {"t": "(tt)", "v": [0, 1200]},
        "Domains": {"t": "a(isb)", "v": []},
        "FallbackDNS": {"t": "a(iiay)", "v": []},
        "DNSSECNegativeTrustAnchors": {"t": "as", "v": ["local", "lan"]},
        "LLMNRHostname": {"t": "s", "v": "testbox"}
    })
}

/// Canned link 2 properties — has DNS servers, default route.
fn link2_props() -> Value {
    json!({
        "DNS": {"t": "a(iay)", "v": [
            [2, [192, 168, 1, 1]],
            [10, [253, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]]
        ]},
        "CurrentDNSServer": {"t": "(iay)", "v": [2, [192, 168, 1, 1]]},
        "DNSOverTLS": {"t": "s", "v": "no"},
        "DNSSEC": {"t": "s", "v": "no"},
        "DNSSECSupported": {"t": "b", "v": false},
        "DefaultRoute": {"t": "b", "v": true},
        "LLMNR": {"t": "s", "v": "resolve"},
        "MulticastDNS": {"t": "s", "v": "no"},
        "Domains": {"t": "a(sb)", "v": []},
        "DNSSECNegativeTrustAnchors": {"t": "as", "v": []},
        "ScopesMask": {"t": "t", "v": 7}
    })
}

/// Canned link 3 properties — no DNS servers.
fn empty_link_props() -> Value {
    json!({
        "DNS": {"t": "a(iay)", "v": []},
        "CurrentDNSServer": {"t": "(iay)", "v": [0, []]},
        "DNSOverTLS": {"t": "s", "v": "no"},
        "DNSSEC": {"t": "s", "v": "no"},
        "DNSSECSupported": {"t": "b", "v": false},
        "DefaultRoute": {"t": "b", "v": false},
        "LLMNR": {"t": "s", "v": "resolve"},
        "MulticastDNS": {"t": "s", "v": "no"},
        "Domains": {"t": "a(sb)", "v": []},
        "DNSSECNegativeTrustAnchors": {"t": "as", "v": []},
        "ScopesMask": {"t": "t", "v": 0}
    })
}

/// Introspect XML for `/org/freedesktop/resolve1/link` listing children.
fn link_introspect_xml() -> String {
    let l2 = encode_ifindex(2);
    let l3 = encode_ifindex(3);
    let l10 = encode_ifindex(10);
    format!(
        r#"<!DOCTYPE node PUBLIC "-//freedesktop//DTD D-BUS Object Introspection 1.0//EN"
"https://www.freedesktop.org/standards/dbus/1.0/introspect.dtd">
<node>
 <node name="{l2}"/>
 <node name="{l3}"/>
 <node name="{l10}"/>
</node>"#
    )
}

/// Dispatch a resolve1 D-Bus call and return the reply frame value.
pub fn resolve_reply(path: &str, iface: &str, method: &str, args: &[Value], id: &Value) -> Value {
    // Introspect on the link container
    if path == "/org/freedesktop/resolve1/link" && method == "Introspect" {
        return ok_reply(id, json!([link_introspect_xml()]));
    }

    // Manager methods
    if path == RESOLVE_MGR_PATH {
        if iface == PROPS_IFACE && method == "GetAll" {
            return ok_reply(id, json!([manager_props()]));
        }
        if iface == RESOLVE_MGR_IFACE && method == "FlushCaches" {
            return ok_reply(id, json!([]));
        }
        if iface == RESOLVE_MGR_IFACE && method == "GetLink" {
            let ifindex = args.first().and_then(Value::as_u64).unwrap_or(0) as u32;
            let encoded = encode_ifindex(ifindex);
            let obj_path = format!("/org/freedesktop/resolve1/link/{encoded}");
            return ok_reply(id, json!([obj_path]));
        }
        if iface == RESOLVE_MGR_IFACE && method == "ResolveHostname" {
            let hostname = args.get(1).and_then(Value::as_str).unwrap_or("");
            if hostname == "example.com" {
                // a(iiay) s t
                return ok_reply(id, json!([[[0, 2, [93, 184, 215, 14]]], "example.com", 0]));
            }
            return err_reply(
                id,
                "org.freedesktop.resolve1.DnsError.NXDOMAIN",
                format!("{hostname}: NXDOMAIN"),
            );
        }
    }

    // Per-link properties
    let link_prefix = "/org/freedesktop/resolve1/link/";
    if let Some(suffix) = path.strip_prefix(link_prefix) {
        if iface == PROPS_IFACE && method == "GetAll" {
            let l2 = encode_ifindex(2);
            let l3 = encode_ifindex(3);
            let l10 = encode_ifindex(10);
            let props = if suffix == l2 {
                link2_props()
            } else if suffix == l3 || suffix == l10 {
                empty_link_props()
            } else {
                return err_reply(
                    id,
                    "org.freedesktop.DBus.Error.UnknownObject",
                    format!("Unknown link: {path}"),
                );
            };
            return ok_reply(id, json!([props]));
        }
    }

    err_reply(
        id,
        "org.freedesktop.DBus.Error.UnknownMethod",
        format!("No such method: {iface}.{method}"),
    )
}

fn ok_reply(id: &Value, out_args: Value) -> Value {
    json!({"reply": [out_args], "id": id})
}

fn err_reply(id: &Value, name: &str, msg: String) -> Value {
    json!({"error": [name, [msg]], "id": id})
}
