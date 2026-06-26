//! Read-only DNS resolver queries: status and hostname resolution.

use super::model::{decode_dns_address, DnsAddress, GlobalDnsConfig, LinkDnsConfig, QueryResult};
use super::{INTROSPECT_IFACE, PROPS_IFACE, RESOLVE_LINK_IFACE, RESOLVE_MGR_IFACE, RESOLVE_PATH};
use crate::capabilities::{CapabilityContext, View};
use crate::error::{FezError, Result};
use serde_json::{json, Value};

/// `GetAll` the properties of a resolve1 object.
fn get_all(ctx: &mut CapabilityContext<'_>, path: &str, iface: &str) -> Result<Value> {
    let out = ctx
        .client
        .dbus_call(ctx.channel, path, PROPS_IFACE, "GetAll", json!([iface]))?;
    out.get(0)
        .cloned()
        .ok_or_else(|| FezError::Problem(format!("GetAll({iface}) returned no value")))
}

/// Decode a D-Bus label-encoded node name back to an ifindex.
///
/// `_32` → 2, `_314` → 14, `_3130` → 130.
fn decode_node_name(name: &str) -> Option<u32> {
    let hex_rest = name.strip_prefix('_')?;
    if hex_rest.len() < 2 {
        return None;
    }
    let first_char = u8::from_str_radix(&hex_rest[..2], 16).ok()? as char;
    let mut decoded = String::new();
    decoded.push(first_char);
    decoded.push_str(&hex_rest[2..]);
    decoded.parse().ok()
}

/// D-Bus label-encode an ifindex to a path segment.
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

/// Enumerate link ifindexes by introspecting the link container.
fn enumerate_links(ctx: &mut CapabilityContext<'_>) -> Result<Vec<u32>> {
    let out = ctx.client.dbus_call(
        ctx.channel,
        "/org/freedesktop/resolve1/link",
        INTROSPECT_IFACE,
        "Introspect",
        json!([]),
    )?;
    let xml = out
        .get(0)
        .and_then(Value::as_str)
        .ok_or_else(|| FezError::Problem("Introspect returned no XML".into()))?;

    // ponytail: regex-free XML parse — just split on `name="` and grab until `"`
    let mut indices = Vec::new();
    for segment in xml.split("name=\"") {
        if let Some(end) = segment.find('"') {
            let name = &segment[..end];
            if let Some(idx) = decode_node_name(name) {
                indices.push(idx);
            }
        }
    }
    indices.sort_unstable();
    Ok(indices)
}

/// Gather DNS resolver status: global config plus per-link detail.
///
/// # Errors
///
/// Returns errors from D-Bus calls or property parsing. Stale links
/// (removed between introspect and `GetAll`) are silently skipped.
pub(super) fn status(ctx: &mut CapabilityContext<'_>, all: bool) -> Result<View> {
    let global_props = get_all(ctx, RESOLVE_PATH, RESOLVE_MGR_IFACE)?;
    let global = GlobalDnsConfig::from_value(global_props)?;

    let link_indices = enumerate_links(ctx)?;
    let mut links = Vec::new();
    for idx in link_indices {
        let encoded = encode_ifindex(idx);
        let path = format!("/org/freedesktop/resolve1/link/{encoded}");
        // Links can disappear between introspect and GetAll (e.g. container
        // veth torn down); skip stale entries instead of aborting.
        let link_props = match get_all(ctx, &path, RESOLVE_LINK_IFACE) {
            Ok(props) => props,
            Err(FezError::Dbus { ref name, .. })
                if name.contains("UnknownObject") || name.contains("UnknownInterface") =>
            {
                continue;
            }
            Err(e) => return Err(e),
        };
        let link = LinkDnsConfig::from_value(idx, link_props)?;
        if all || link.has_dns() {
            links.push(link);
        }
    }

    let data = json!({
        "global": &global,
        "links": &links,
    });

    let human = render_status_human(&global, &links);
    Ok(View::new("DnsStatus", ctx.host, data, human))
}

/// Render human-readable DNS status.
fn render_status_human(global: &GlobalDnsConfig, links: &[LinkDnsConfig]) -> String {
    let mut out = String::new();

    out.push_str("Global\n");
    if let Some(ref current) = global.current_server {
        out.push_str(&format!("  Current DNS Server: {current}\n"));
    }
    if !global.dns_servers.is_empty() {
        let servers: Vec<&str> = global
            .dns_servers
            .iter()
            .map(|a| a.address.as_str())
            .collect();
        out.push_str(&format!("       DNS Servers: {}\n", servers.join(" ")));
    }
    out.push_str(&format!(
        "           DNSSEC: {} (supported: {})\n",
        global.dnssec, global.dnssec_supported
    ));
    out.push_str(&format!("      DNS-over-TLS: {}\n", global.dns_over_tls));
    out.push_str(&format!("             LLMNR: {}\n", global.llmnr));
    out.push_str(&format!("      MulticastDNS: {}\n", global.multicast_dns));
    out.push_str(&format!(
        "   resolv.conf mode: {}\n",
        global.resolv_conf_mode
    ));

    let total_lookups = global.cache_hits + global.cache_misses;
    let hit_rate = if total_lookups > 0 {
        format!(
            "{:.1}%",
            (global.cache_hits as f64 / total_lookups as f64) * 100.0
        )
    } else {
        "n/a".into()
    };
    out.push_str(&format!(
        "   Cache: {} entries, {} hits, {} misses ({})\n",
        global.cache_size, global.cache_hits, global.cache_misses, hit_rate
    ));
    out.push_str(&format!(
        "   Transactions: {} current, {} total\n",
        global.transactions_current, global.transactions_total
    ));

    for link in links {
        out.push_str(&format!("\nLink {}\n", link.ifindex));
        if let Some(ref current) = link.current_server {
            out.push_str(&format!("  Current DNS Server: {current}\n"));
        }
        if !link.dns_servers.is_empty() {
            let servers: Vec<&str> = link
                .dns_servers
                .iter()
                .map(|a| a.address.as_str())
                .collect();
            out.push_str(&format!("       DNS Servers: {}\n", servers.join(" ")));
        }
        out.push_str(&format!("     Default Route: {}\n", link.default_route));
        out.push_str(&format!("           DNSSEC: {}\n", link.dnssec));
        out.push_str(&format!("      DNS-over-TLS: {}\n", link.dns_over_tls));
        out.push_str(&format!("             LLMNR: {}\n", link.llmnr));
        out.push_str(&format!("      MulticastDNS: {}\n", link.multicast_dns));
    }

    out
}

/// Resolve a hostname via `ResolveHostname`.
///
/// # Errors
///
/// Returns [`FezError::NotFound`] for NXDOMAIN, or [`FezError::Problem`]
/// for malformed replies or other D-Bus errors.
pub(super) fn query(ctx: &mut CapabilityContext<'_>, hostname: &str) -> Result<View> {
    let result = ctx.client.dbus_call(
        ctx.channel,
        RESOLVE_PATH,
        RESOLVE_MGR_IFACE,
        "ResolveHostname",
        json!([0, hostname, 0, 0]),
    );

    // Map NXDOMAIN D-Bus errors to NotFound
    let out = match result {
        Err(FezError::Dbus { ref name, .. }) if name.contains("NXDOMAIN") => {
            return Err(FezError::NotFound(format!("{hostname}: NXDOMAIN")));
        }
        other => other?,
    };

    // Parse: [a(iiay), canonical_name, flags]
    let addr_array = out
        .get(0)
        .and_then(Value::as_array)
        .ok_or_else(|| FezError::Problem("ResolveHostname returned no addresses".into()))?;
    let canonical = out
        .get(1)
        .and_then(Value::as_str)
        .ok_or_else(|| FezError::Problem("ResolveHostname returned no canonical name".into()))?
        .to_string();

    let mut addresses = Vec::new();
    for entry in addr_array {
        let arr = entry
            .as_array()
            .ok_or_else(|| FezError::Problem("ResolveHostname entry not a tuple".into()))?;
        let ifindex = arr.first().and_then(Value::as_i64).unwrap_or(0);
        let family = arr
            .get(1)
            .and_then(Value::as_i64)
            .ok_or_else(|| FezError::Problem("ResolveHostname entry missing family".into()))?;
        let bytes = arr
            .get(2)
            .ok_or_else(|| FezError::Problem("ResolveHostname entry missing address".into()))?;
        if let Some(address) = decode_dns_address(family, bytes) {
            addresses.push(DnsAddress {
                family: if family == 2 { "ipv4" } else { "ipv6" }.into(),
                address,
                ifindex,
            });
        }
    }

    let query_result = QueryResult {
        hostname: hostname.to_string(),
        canonical,
        addresses,
    };

    let data = serde_json::to_value(&query_result)
        .map_err(|e| FezError::Problem(format!("failed to serialize DnsQuery: {e}")))?;

    let mut human = String::new();
    for addr in &query_result.addresses {
        human.push_str(&format!("{} ({})\n", addr.address, addr.family));
    }
    if query_result.addresses.is_empty() {
        human.push_str(&format!("{hostname}: no addresses found\n"));
    }

    Ok(View::new("DnsQuery", ctx.host, data, human))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_node_name_works() {
        assert_eq!(decode_node_name("_32"), Some(2));
        assert_eq!(decode_node_name("_314"), Some(14));
        assert_eq!(decode_node_name("_3130"), Some(130));
        assert_eq!(decode_node_name("_3233"), Some(233));
        assert_eq!(decode_node_name("abc"), None);
        assert_eq!(decode_node_name("_"), None);
        assert_eq!(decode_node_name("_zz"), None);
    }

    #[test]
    fn encode_decode_roundtrip() {
        for idx in [2, 14, 130, 233, 1, 99, 1000] {
            let encoded = encode_ifindex(idx);
            assert_eq!(
                decode_node_name(&encoded),
                Some(idx),
                "roundtrip failed for {idx}"
            );
        }
    }
}
