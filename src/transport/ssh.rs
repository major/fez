use super::Transport;
use std::path::{Component, Path};
use std::process::Command;

/// Runs the bridge on a remote host over SSH, reusing a multiplexed connection.
pub struct SshTransport {
    target: String,
    /// Optional explicit ssh client config (`ssh -F <path>`). Sourced from
    /// `FEZ_SSH_CONFIG`; lets callers (and the E2E harness) pin a hermetic
    /// config instead of relying on the ambient `~/.ssh/config`.
    config: Option<String>,
}

fn ssh_config_from_env(raw: Option<String>) -> Option<String> {
    raw.filter(|path| safe_ssh_config_path(path))
}

fn safe_ssh_config_path(path: &str) -> bool {
    let path = Path::new(path);
    path.is_absolute()
        && (path.starts_with("/etc/fez") || path.starts_with("/run/fez"))
        && !path
            .components()
            .any(|component| component == Component::ParentDir)
}

impl SshTransport {
    /// Build a transport for `target` (host, user@host, or ssh_config alias).
    pub fn new(target: &str) -> Self {
        SshTransport {
            target: target.to_string(),
            config: ssh_config_from_env(std::env::var("FEZ_SSH_CONFIG").ok()),
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
            // Belt-and-suspenders SSH hardening (Section 5 of the security model):
            // - StrictHostKeyChecking=yes: refuse to connect if the host key is
            //   absent from known_hosts or mismatches.  Without this a user whose
            //   ~/.ssh/config has `StrictHostKeyChecking no` would silently
            //   accept any host key, making the cockpit-bridge session MITM-able.
            // - PasswordAuthentication=no: never fall back to keyboard-interactive
            //   or password auth even if the user's config allows it.  fez is an
            //   agent-driven tool; credentials belong in ssh-agent or key files.
            // - These appear **after** the optional -F config so they override any
            //   user-level StrictHostKeyChecking or PasswordAuthentication setting.
            .arg("-o")
            .arg("StrictHostKeyChecking=yes")
            .arg("-o")
            .arg("PasswordAuthentication=no")
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
        assert!(args.windows(2).any(|w| w == ["-o", "StrictHostKeyChecking=yes"]));
        assert!(args.windows(2).any(|w| w == ["-o", "PasswordAuthentication=no"]));
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

    #[test]
    fn env_config_rejects_untrusted_paths() {
        assert_eq!(ssh_config_from_env(Some("/tmp/evil".into())), None);
        assert_eq!(
            ssh_config_from_env(Some("/etc/fez/ssh_config".into())),
            Some("/etc/fez/ssh_config".into())
        );
    }

    /// fez hardening options must appear **after** the user-supplied -F config
    /// so they override any weaker settings in the user's file.
    #[test]
    fn hardening_follows_config_flag() {
        let t = SshTransport {
            target: "target".into(),
            config: Some("/etc/fez/ssh_config".into()),
        };
        let args: Vec<String> = t
            .command()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let config_pos = args.iter().position(|a| a == "-F").unwrap();
        let strict_pos = args
            .iter()
            .position(|a| a == "StrictHostKeyChecking=yes")
            .unwrap();
        assert!(
            strict_pos > config_pos,
            "StrictHostKeyChecking appeared at index {strict_pos}, must be after -F at index {config_pos}"
        );
    }
}
