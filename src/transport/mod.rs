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
    from_host_with_options(host, false)
}

/// Select a transport and apply SSH-specific global options when needed.
pub fn from_host_with_options(host: Option<&str>, ssh_identities_only: bool) -> Box<dyn Transport> {
    match host {
        None | Some("localhost") | Some("local") => Box::new(local::LocalTransport::default()),
        Some(h) => Box::new(ssh::SshTransport::new_with_identities_only(
            h,
            ssh_identities_only,
        )),
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

    #[test]
    fn explicit_host_can_enable_identities_only() {
        let t = from_host_with_options(Some("fedora@host.example"), true);
        assert_eq!(t.host_label(), "fedora@host.example");
        let args: Vec<String> = t
            .command()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(args.iter().any(|arg| arg == "IdentitiesOnly=yes"));
    }
}
