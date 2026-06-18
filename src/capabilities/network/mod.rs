//! NetworkManager inspection capability.
//!
//! Reads the readable NetworkManager surface over the cockpit-bridge
//! `dbus-json3` channel (`org.freedesktop.NetworkManager`, system bus,
//! unprivileged). Two actions: `network list` (device inventory) and
//! `network show <device>` (full per-device detail). Read-only: no mutation,
//! no privilege escalation.

use crate::capabilities::{render, CapabilityContext, View};
use crate::cli::{Cli, NetworkAction};
use crate::error::Result;

mod model;
mod reads;

pub(super) const NM_NAME: &str = "org.freedesktop.NetworkManager";
pub(super) const NM_MGR_PATH: &str = "/org/freedesktop/NetworkManager";
pub(super) const NM_MGR_IFACE: &str = "org.freedesktop.NetworkManager";
pub(super) const PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";
pub(super) const DEVICE_IFACE: &str = "org.freedesktop.NetworkManager.Device";
pub(super) const IP4_IFACE: &str = "org.freedesktop.NetworkManager.IP4Config";
pub(super) const IP6_IFACE: &str = "org.freedesktop.NetworkManager.IP6Config";
pub(super) const ACTIVE_IFACE: &str = "org.freedesktop.NetworkManager.Connection.Active";
pub(super) const DHCP4_IFACE: &str = "org.freedesktop.NetworkManager.DHCP4Config";

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
        NetworkAction::List { all } => reads::list(&mut ctx, *all),
        NetworkAction::Show { device } => reads::show(&mut ctx, device),
    }
}
