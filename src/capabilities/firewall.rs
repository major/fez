//! Firewall management over firewalld (`org.fedoraproject.FirewallD1`).
//!
//! Reads (status/list/show/services) open an unprivileged `dbus-json3` channel;
//! mutations (add/remove service/port, set-default-zone, reload, confirm,
//! panic) open a privileged one and escalate. fez holds no state: the
//! runtime-vs-permanent split that guards against lockout is firewalld's own,
//! read live each call and committed only via `runtimeToPermanent`.

use crate::cli::{Cli, FirewallAction};

/// Route a parsed `firewall` action to its handler and return the exit code.
pub fn dispatch(cli: &Cli, action: &FirewallAction) -> i32 {
    let _ = (cli, action);
    // Filled in by later tasks.
    0
}
