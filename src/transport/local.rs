use super::Transport;
use std::ffi::OsString;
use std::process::Command;

/// Runs the bridge as a local child process.
pub struct LocalTransport {
    /// Bridge program to spawn (overridable via `FEZ_BRIDGE`).
    pub program: OsString,
}

impl Default for LocalTransport {
    fn default() -> Self {
        // `FEZ_BRIDGE` lets integration tests point at the fake bridge.
        let program =
            std::env::var_os("FEZ_BRIDGE").unwrap_or_else(|| OsString::from("cockpit-bridge"));
        LocalTransport { program }
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
}
