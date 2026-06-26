//! DNS resolver capability (systemd-resolved).
//!
//! Reads the resolver configuration, cache statistics, and per-link DNS
//! detail from `org.freedesktop.resolve1` over cockpit-bridge. Also supports
//! cache flush and hostname resolution. Read-only except for flush.

use crate::capabilities::{render, CapabilityContext, View};
use crate::cli::{Cli, DnsAction};
use crate::error::{FezError, Result};

mod flush;
mod model;
mod reads;

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

/// The `DependencyMissing` error returned when systemd-resolved is absent.
fn dependency_missing() -> FezError {
    FezError::DependencyMissing {
        component: "systemd-resolved".into(),
        dbus_name: RESOLVE_NAME.into(),
        remediation: "systemctl enable --now systemd-resolved".into(),
    }
}

/// Map a resolve1 error to `dependency_missing` when the service is absent.
///
/// systemd-resolved absence surfaces as:
/// - `Dbus { ServiceUnknown | NameHasNoOwner }`: name not activatable.
/// - `Problem("not-found")`: cockpit closed the channel because the name
///   could not be reached.
/// - `Problem("not-supported")`: the bus refused the name.
fn map_resolve_error(e: FezError) -> FezError {
    match e {
        FezError::Dbus { ref name, .. } if crate::error::is_service_unknown(name) => {
            dependency_missing()
        }
        FezError::Problem(ref p) if p == "not-found" || p == "not-supported" => {
            dependency_missing()
        }
        other => other,
    }
}

/// Connect to the bridge and dispatch the requested action.
fn run(cli: &Cli, action: &DnsAction) -> Result<View> {
    let mut client = crate::capabilities::connect(cli)?;
    let host = client.host().to_string();
    let channel = client.dbus_open(RESOLVE_NAME)?;
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
    result.map_err(map_resolve_error)
}
