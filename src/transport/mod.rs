//! Transports that launch the bridge: locally or over SSH.
/// Local-process transport.
pub mod local;
/// SSH transport.
pub mod ssh;

use std::process::Command;

/// Builds the command that launches a cockpit-bridge speaking the protocol on stdio.
pub trait Transport {
    /// The fully configured command that spawns the bridge.
    fn command(&self) -> Command;
    /// Label for diagnostics and the envelope `host` field.
    fn host_label(&self) -> String;
}

/// Select a transport from the global `--host` flag.
pub fn from_host(host: Option<&str>) -> Box<dyn Transport> {
    match host {
        None | Some("localhost") | Some("local") => Box::new(local::LocalTransport::default()),
        Some(h) => Box::new(ssh::SshTransport::new(h)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_and_localhost_select_local() {
        assert_eq!(from_host(None).host_label(), "localhost");
        assert_eq!(from_host(Some("localhost")).host_label(), "localhost");
        assert_eq!(from_host(Some("local")).host_label(), "localhost");
    }

    #[test]
    fn explicit_host_selects_ssh() {
        let t = from_host(Some("fedora@host.example"));
        assert_eq!(t.host_label(), "fedora@host.example");
        assert_eq!(t.command().get_program(), "ssh");
    }
}
