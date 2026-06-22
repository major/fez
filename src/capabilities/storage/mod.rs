//! UDisks2 storage inspection capability.
//!
//! Reads the block-device, drive, partition, filesystem, and NVMe health
//! surface over the cockpit-bridge `dbus-json3` channel
//! (`org.freedesktop.UDisks2`, system bus, unprivileged). Three actions:
//! `storage list` (device inventory), `storage show <device>` (per-device
//! detail), and `storage health` (SMART/NVMe drive health). Read-only: no
//! mutations, no privilege escalation.

use crate::capabilities::{render, CapabilityContext, View};
use crate::cli::{Cli, StorageAction};
use crate::error::Result;

mod model;
mod reads;

pub(super) const UDISKS_NAME: &str = "org.freedesktop.UDisks2";
pub(super) const UDISKS_MGR_PATH: &str = "/org/freedesktop/UDisks2/Manager";
pub(super) const UDISKS_MGR_IFACE: &str = "org.freedesktop.UDisks2.Manager";
pub(super) const BLOCK_IFACE: &str = "org.freedesktop.UDisks2.Block";
pub(super) const PARTITION_IFACE: &str = "org.freedesktop.UDisks2.Partition";
pub(super) const PTABLE_IFACE: &str = "org.freedesktop.UDisks2.PartitionTable";
pub(super) const FS_IFACE: &str = "org.freedesktop.UDisks2.Filesystem";
pub(super) const DRIVE_IFACE: &str = "org.freedesktop.UDisks2.Drive";
pub(super) const ENCRYPTED_IFACE: &str = "org.freedesktop.UDisks2.Encrypted";
pub(super) const NVME_CTRL_IFACE: &str = "org.freedesktop.UDisks2.NVMe.Controller";
pub(super) const PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";

/// Route a parsed `storage` action to its handler and render the result.
///
/// Returns the process exit code.
pub fn dispatch(cli: &Cli, action: &StorageAction) -> i32 {
    let result = run(cli, action);
    render(cli, result)
}

/// Connect to the bridge and dispatch the requested read action.
fn run(cli: &Cli, action: &StorageAction) -> Result<View> {
    let mut client = crate::capabilities::connect(cli)?;
    let host = client.host().to_string();
    let channel = client.dbus_open(UDISKS_NAME)?;
    let mut ctx = CapabilityContext {
        client: &mut client,
        channel: &channel,
        host: &host,
    };
    match action {
        StorageAction::List => reads::list(&mut ctx),
        StorageAction::Show { device } => reads::show(&mut ctx, device),
        StorageAction::Health { drive } => reads::health(&mut ctx, drive.as_deref()),
    }
}
