use super::Transport;
use std::process::Command;

/// Runs the bridge on a remote host over SSH, reusing a multiplexed connection.
pub struct SshTransport {
    target: String,
    /// Optional explicit ssh client config (`ssh -F <path>`). Sourced from
    /// `FEZ_SSH_CONFIG`; lets callers (and the E2E harness) pin a hermetic
    /// config instead of relying on the ambient `~/.ssh/config`.
    config: Option<String>,
}

impl SshTransport {
    /// Build a transport for `target` (host, user@host, or ssh_config alias).
    pub fn new(target: &str) -> Self {
        SshTransport {
            target: target.to_string(),
            config: std::env::var("FEZ_SSH_CONFIG")
                .ok()
                .filter(|s| !s.is_empty()),
        }
    }
}

impl Transport for SshTransport {
    fn command(&self) -> Command {
        let mut cmd = Command::new("ssh");
        if let Some(cfg) = &self.config {
            cmd.arg("-F").arg(cfg);
        }
        cmd.arg("-o")
            .arg("BatchMode=yes")
            .arg("-o")
            .arg("ControlMaster=auto")
            .arg("-o")
            .arg("ControlPersist=60")
            .arg("-o")
            .arg("ControlPath=~/.ssh/fez-%r@%h:%p")
            .arg("--")
            .arg(&self.target)
            .arg("cockpit-bridge");
        cmd
    }
    fn host_label(&self) -> String {
        self.target.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_ssh_argv() {
        let t = SshTransport::new("fedora@host.example");
        let cmd = t.command();
        assert_eq!(cmd.get_program(), "ssh");
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(args.windows(2).any(|w| w == ["-o", "BatchMode=yes"]));
        assert!(args.contains(&"fedora@host.example".to_string()));
        // target and bridge invocation both after `--` (prevents option injection)
        let dd = args.iter().position(|a| a == "--").unwrap();
        assert_eq!(args[dd + 1], "fedora@host.example");
        assert_eq!(args[dd + 2], "cockpit-bridge");
    }

    #[test]
    fn host_label_is_target() {
        assert_eq!(SshTransport::new("h1").host_label(), "h1");
    }

    #[test]
    fn injects_config_flag_when_set() {
        let t = SshTransport {
            target: "target".into(),
            config: Some("/run/fez/ssh_config".into()),
        };
        let args: Vec<String> = t
            .command()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(
            args.windows(2).any(|w| w == ["-F", "/run/fez/ssh_config"]),
            "expected -F /run/fez/ssh_config, got {args:?}"
        );
    }

    #[test]
    fn omits_config_flag_when_unset() {
        let t = SshTransport {
            target: "target".into(),
            config: None,
        };
        let args: Vec<String> = t
            .command()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(!args.iter().any(|a| a == "-F"), "unexpected -F in {args:?}");
    }
}
