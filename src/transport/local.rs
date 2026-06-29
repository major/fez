use super::Transport;
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::Command;

/// Runs the bridge as a local child process.
pub struct LocalTransport {
    /// Bridge program to spawn (overridable via `FEZ_BRIDGE`).
    pub program: OsString,
}

fn bridge_program(override_program: Option<OsString>, allow_test_override: bool) -> OsString {
    if allow_test_override {
        if let Some(program) = override_program {
            if Path::new(&program).file_name() == Some(OsStr::new("fez-fake-bridge")) {
                return program;
            }
        }
    }
    OsString::from("cockpit-bridge")
}

impl Default for LocalTransport {
    fn default() -> Self {
        LocalTransport {
            program: bridge_program(std::env::var_os("FEZ_BRIDGE"), cfg!(debug_assertions)),
        }
    }
}

impl Transport for LocalTransport {
    fn command(&self) -> Command {
        Command::new(&self.program)
    }
    fn host_label(&self) -> String {
        "localhost".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_bridge_command() {
        let t = LocalTransport {
            program: "cockpit-bridge".into(),
        };
        assert_eq!(t.command().get_program(), "cockpit-bridge");
        assert_eq!(t.host_label(), "localhost");
    }

    #[test]
    fn bridge_override_is_ignored_in_release_mode() {
        assert_eq!(
            bridge_program(Some("/tmp/evil".into()), false),
            OsString::from("cockpit-bridge")
        );
    }

    #[test]
    fn debug_override_only_accepts_fake_bridge() {
        assert_eq!(
            bridge_program(Some("/tmp/fez-fake-bridge".into()), true),
            OsString::from("/tmp/fez-fake-bridge")
        );
        assert_eq!(
            bridge_program(Some("/tmp/evil".into()), true),
            OsString::from("cockpit-bridge")
        );
    }
}
