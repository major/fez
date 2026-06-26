//! DNS resolver capability.
//!
//! Primary backend: `org.freedesktop.resolve1` (systemd-resolved) — full
//! resolver config, cache stats, DNSSEC/DoT status, flush, and query.
//!
//! Fallback backend: `org.freedesktop.NetworkManager.DnsManager` — basic
//! DNS server list and mode. Used automatically when systemd-resolved is
//! absent (e.g. RHEL 10). Flush and query are unavailable on the fallback.

use crate::capabilities::{render, CapabilityContext, View};
use crate::cli::{Cli, DnsAction};
use crate::error::{FezError, Result};

mod flush;
mod model;
mod nm_fallback;
mod reads;

pub(super) const RESOLVE_NAME: &str = "org.freedesktop.resolve1";
pub(super) const RESOLVE_PATH: &str = "/org/freedesktop/resolve1";
pub(super) const RESOLVE_MGR_IFACE: &str = "org.freedesktop.resolve1.Manager";
pub(super) const RESOLVE_LINK_IFACE: &str = "org.freedesktop.resolve1.Link";
pub(super) const PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";
pub(super) const INTROSPECT_IFACE: &str = "org.freedesktop.DBus.Introspectable";

const NM_NAME: &str = "org.freedesktop.NetworkManager";

/// Route a parsed `dns` action to its handler and render the result.
///
/// Returns the process exit code.
pub fn dispatch(cli: &Cli, action: &DnsAction) -> i32 {
    let result = run(cli, action);
    render(cli, result)
}

/// Whether an error indicates the D-Bus service is absent.
fn is_service_absent(e: &FezError) -> bool {
    match e {
        FezError::Dbus { name, .. } => crate::error::is_service_unknown(name),
        FezError::Problem(p) => p == "not-found" || p == "not-supported",
        _ => false,
    }
}

/// Connect to the bridge and dispatch the requested action.
///
/// Tries resolve1 first. On `dns status`, falls back to NM DnsManager when
/// resolve1 is absent. On `dns flush` and `dns query`, resolve1 is required —
/// returns a dependency-missing error when absent.
fn run(cli: &Cli, action: &DnsAction) -> Result<View> {
    let mut client = crate::capabilities::connect(cli)?;
    let host = client.host().to_string();
    let channel = client.dbus_open(RESOLVE_NAME)?;

    // Try resolve1
    let result = match action {
        DnsAction::Status { all } => {
            let mut ctx = CapabilityContext {
                client: &mut client,
                channel: &channel,
                host: &host,
            };
            reads::status(&mut ctx, *all)
        }
        DnsAction::Flush => flush::flush(&mut client, &channel, &host),
        DnsAction::Query { hostname } => {
            let mut ctx = CapabilityContext {
                client: &mut client,
                channel: &channel,
                host: &host,
            };
            reads::query(&mut ctx, hostname)
        }
    };

    match result {
        Ok(view) => Ok(view),
        Err(ref e) if is_service_absent(e) => match action {
            // Status can fall back to NM
            DnsAction::Status { .. } => {
                let nm_channel = client.dbus_open(NM_NAME)?;
                let mut ctx = CapabilityContext {
                    client: &mut client,
                    channel: &nm_channel,
                    host: &host,
                };
                nm_fallback::status(&mut ctx)
            }
            // Flush and query need resolve1
            DnsAction::Flush => Err(FezError::DependencyMissing {
                component: "systemd-resolved".into(),
                dbus_name: RESOLVE_NAME.into(),
                remediation: "dns flush requires systemd-resolved; \
                              install and enable it: dnf install systemd-resolved && \
                              systemctl enable --now systemd-resolved"
                    .into(),
            }),
            DnsAction::Query { .. } => Err(FezError::DependencyMissing {
                component: "systemd-resolved".into(),
                dbus_name: RESOLVE_NAME.into(),
                remediation: "dns query requires systemd-resolved; \
                              install and enable it: dnf install systemd-resolved && \
                              systemctl enable --now systemd-resolved"
                    .into(),
            }),
        },
        Err(e) => Err(e),
    }
}
