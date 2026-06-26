//! DNS cache flush — the only mutation in the dns capability.

use super::{RESOLVE_MGR_IFACE, RESOLVE_PATH};
use crate::audit;
use crate::capabilities::View;
use crate::error::Result;
use crate::protocol::client::BridgeClient;
use serde_json::json;

/// Flush the DNS resolver cache via `FlushCaches()`.
///
/// Audited: destroys resolver cache state.
pub(super) fn flush(client: &mut BridgeClient, channel: &str, host: &str) -> Result<View> {
    audit::run_audited(host, "dns-flush", "", || {
        client.dbus_call(
            channel,
            RESOLVE_PATH,
            RESOLVE_MGR_IFACE,
            "FlushCaches",
            json!([]),
        )?;
        Ok(())
    })?;

    Ok(View::new(
        "DnsFlush",
        host,
        json!({"flushed": true}),
        format!("DNS cache flushed on {host}\n"),
    ))
}
