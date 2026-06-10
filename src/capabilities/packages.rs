//! RPM package management over dnf5daemon (`org.rpm.dnf.v0`).
use crate::cli::{Cli, PackagesAction};

/// Run the requested `packages` subcommand and return the process exit code.
pub fn dispatch(_cli: &Cli, _action: &PackagesAction) -> i32 {
    0
}
