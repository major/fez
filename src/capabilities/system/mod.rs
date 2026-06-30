//! System overview capability.
//!
//! Gathers host identity, OS, kernel, hardware, firmware, and time/NTP
//! information from two universally available systemd D-Bus services:
//! `org.freedesktop.hostname1` and `org.freedesktop.timedate1`. Both are
//! part of systemd itself, so they require zero extra packages on Fedora
//! and RHEL. Read-only: no mutations, no privilege escalation.

use crate::capabilities::{render, View};
use crate::cli::{Cli, SystemAction};
use crate::error::Result;

mod metrics;
mod reads;

// ponytail: name == iface for both services, so 4 constants not 6
pub(super) const HOSTNAME_NAME: &str = "org.freedesktop.hostname1";
pub(super) const HOSTNAME_PATH: &str = "/org/freedesktop/hostname1";

pub(super) const TIMEDATE_NAME: &str = "org.freedesktop.timedate1";
pub(super) const TIMEDATE_PATH: &str = "/org/freedesktop/timedate1";

pub(super) const LOCALE_NAME: &str = "org.freedesktop.locale1";
pub(super) const LOCALE_PATH: &str = "/org/freedesktop/locale1";

pub(super) const PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";

/// Route a parsed `system` action to its handler and render the result.
///
/// Returns the process exit code.
pub fn dispatch(cli: &Cli, action: &SystemAction) -> i32 {
    let result = run(cli, action);
    render(cli, result)
}

/// Connect to the bridge and dispatch the requested read action.
fn run(cli: &Cli, action: &SystemAction) -> Result<View> {
    let mut client = crate::capabilities::connect(cli)?;
    let host = client.host().to_string();
    match action {
        SystemAction::Show => reads::show(&mut client, &host),
        SystemAction::Metrics => metrics::show(&mut client, &host),
    }
}
