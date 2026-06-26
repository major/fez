//! DNS resolver capability (systemd-resolved).
//!
//! Reads the resolver configuration, cache statistics, and per-link DNS
//! detail from `org.freedesktop.resolve1` over cockpit-bridge. Also supports
//! cache flush and hostname resolution. Read-only except for flush.

use crate::capabilities::{render, View};
use crate::cli::{Cli, DnsAction};
use crate::error::{FezError, Result};

pub(super) const RESOLVE_NAME: &str = "org.freedesktop.resolve1";
pub(super) const RESOLVE_PATH: &str = "/org/freedesktop/resolve1";
pub(super) const RESOLVE_MGR_IFACE: &str = "org.freedesktop.resolve1.Manager";
pub(super) const RESOLVE_LINK_IFACE: &str = "org.freedesktop.resolve1.Link";
pub(super) const PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";
pub(super) const INTROSPECT_IFACE: &str = "org.freedesktop.DBus.Introspectable";

/// Route a parsed `dns` action to its handler and render the result.
///
/// Returns the process exit code.
pub fn dispatch(cli: &Cli, action: &DnsAction) -> i32 {
    let result = run(cli, action);
    render(cli, result)
}

/// Connect to the bridge and dispatch the requested action.
fn run(cli: &Cli, action: &DnsAction) -> Result<View> {
    let mut client = crate::capabilities::connect(cli)?;
    let host = client.host().to_string();
    let channel = crate::capabilities::map_service_unknown(
        client.dbus_open(RESOLVE_NAME),
        || FezError::DependencyMissing {
            component: "systemd-resolved".into(),
            dbus_name: RESOLVE_NAME.into(),
            remediation: "systemctl enable --now systemd-resolved".into(),
        },
    )?;
    match action {
        DnsAction::Status { .. } | DnsAction::Flush | DnsAction::Query { .. } => {
            drop(channel);
            todo!("dns capability — wired in next tasks")
        }
    }
}
