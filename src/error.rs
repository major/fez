//! Crate-wide error type and its mapping to stable codes and exit statuses.
use thiserror::Error;

/// Convenience alias for results carrying a [`FezError`].
pub type Result<T> = std::result::Result<T, FezError>;

/// Every error fez can surface to the caller.
#[derive(Debug, Error)]
pub enum FezError {
    /// A child process (the bridge) could not be spawned.
    #[error("failed to spawn {program}: {source}")]
    Spawn {
        /// Program that failed to launch.
        program: String,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// A generic I/O failure.
    #[error("i/o error: {0}")]
    Io(#[source] std::io::Error),
    /// A protocol message could not be decoded.
    #[error("protocol decode error: {0}")]
    Decode(#[source] serde_json::Error),
    /// The bridge did not respond before the deadline.
    #[error("timed out waiting for the bridge")]
    Timeout,
    /// The bridge connection was closed unexpectedly.
    #[error("bridge connection closed")]
    BridgeClosed,
    /// The bridge reported a problem; the string is a problem kind.
    #[error("channel problem: {0}")]
    Problem(String),
    /// A D-Bus call returned an error.
    #[error("dbus error {name}: {message}")]
    Dbus {
        /// D-Bus error name.
        name: String,
        /// D-Bus error message.
        message: String,
    },
    /// The requested resource (e.g. a unit) does not exist.
    #[error("not found: {0}")]
    NotFound(String),
    /// A protected unit was targeted without `--force`.
    #[error("refused: {unit} is a protected unit (use --force to override)")]
    Protected {
        /// The protected unit that was refused.
        unit: String,
    },
    /// The user declined a confirmation prompt.
    #[error("aborted by user")]
    Aborted,
}

impl FezError {
    /// Stable machine-readable error code for this error.
    pub fn code(&self) -> &'static str {
        match self {
            FezError::Spawn { .. } => "bridge-unavailable",
            FezError::Io(_) => "io-error",
            FezError::Decode(_) => "protocol-error",
            FezError::Timeout => "timeout",
            FezError::BridgeClosed => "bridge-closed",
            FezError::Problem(p) => problem_code(p),
            FezError::Dbus { .. } => "dbus-error",
            FezError::NotFound(_) => "not-found",
            FezError::Protected { .. } => "protected-unit",
            FezError::Aborted => "aborted",
        }
    }
    /// Process exit code to use when this error is fatal.
    pub fn exit_code(&self) -> i32 {
        match self {
            FezError::NotFound(_) | FezError::Problem(_) => 4,
            FezError::Timeout => 5,
            FezError::Spawn { .. } | FezError::BridgeClosed => 6,
            FezError::Dbus { .. } => 7,
            FezError::Protected { .. } => 8,
            _ => 1,
        }
    }
}

fn problem_code(p: &str) -> &'static str {
    match p {
        "not-found" => "not-found",
        "access-denied" => "access-denied",
        "authentication-failed" => "auth-failed",
        "not-supported" => "not-supported",
        _ => "channel-problem",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_problem_to_code() {
        assert_eq!(FezError::Problem("not-found".into()).code(), "not-found");
        assert_eq!(FezError::Problem("weird".into()).code(), "channel-problem");
    }

    #[test]
    fn maps_exit_codes() {
        assert_eq!(FezError::NotFound("x".into()).exit_code(), 4);
        assert_eq!(FezError::Timeout.exit_code(), 5);
        assert_eq!(FezError::BridgeClosed.exit_code(), 6);
    }

    #[test]
    fn protected_maps_code_and_exit() {
        let e = FezError::Protected {
            unit: "sshd.service".into(),
        };
        assert_eq!(e.code(), "protected-unit");
        assert_eq!(e.exit_code(), 8);
    }

    #[test]
    fn aborted_maps_code_and_exit() {
        assert_eq!(FezError::Aborted.code(), "aborted");
        assert_eq!(FezError::Aborted.exit_code(), 1);
    }

    #[test]
    fn problem_code_covers_all_known_kinds() {
        assert_eq!(
            FezError::Problem("access-denied".into()).code(),
            "access-denied"
        );
        assert_eq!(
            FezError::Problem("authentication-failed".into()).code(),
            "auth-failed"
        );
        assert_eq!(
            FezError::Problem("not-supported".into()).code(),
            "not-supported"
        );
    }

    #[test]
    fn codes_for_spawn_io_decode_dbus() {
        let spawn = FezError::Spawn {
            program: "cockpit-bridge".into(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "missing"),
        };
        assert_eq!(spawn.code(), "bridge-unavailable");
        assert_eq!(spawn.exit_code(), 6);

        let io = FezError::Io(std::io::Error::other("boom"));
        assert_eq!(io.code(), "io-error");
        assert_eq!(io.exit_code(), 1);

        let decode = FezError::Decode(serde_json::from_str::<i32>("nope").unwrap_err());
        assert_eq!(decode.code(), "protocol-error");

        let dbus = FezError::Dbus {
            name: "org.example.Err".into(),
            message: "bad".into(),
        };
        assert_eq!(dbus.code(), "dbus-error");
        assert_eq!(dbus.exit_code(), 7);
    }

    #[test]
    fn display_renders_messages() {
        assert_eq!(
            FezError::Timeout.to_string(),
            "timed out waiting for the bridge"
        );
        assert_eq!(
            FezError::BridgeClosed.to_string(),
            "bridge connection closed"
        );
        assert_eq!(FezError::Aborted.to_string(), "aborted by user");
        assert_eq!(
            FezError::NotFound("sshd.service".into()).to_string(),
            "not found: sshd.service"
        );
        assert_eq!(
            FezError::Protected {
                unit: "sshd.service".into(),
            }
            .to_string(),
            "refused: sshd.service is a protected unit (use --force to override)"
        );
        assert_eq!(
            FezError::Problem("not-found".into()).to_string(),
            "channel problem: not-found"
        );
        assert_eq!(
            FezError::Dbus {
                name: "org.example.Err".into(),
                message: "bad".into(),
            }
            .to_string(),
            "dbus error org.example.Err: bad"
        );
        assert_eq!(
            FezError::Spawn {
                program: "p".into(),
                source: std::io::Error::other("x"),
            }
            .to_string(),
            "failed to spawn p: x"
        );
        assert!(FezError::Io(std::io::Error::other("disk"))
            .to_string()
            .starts_with("i/o error"));
        assert!(
            FezError::Decode(serde_json::from_str::<i32>("x").unwrap_err())
                .to_string()
                .starts_with("protocol decode error")
        );
    }
}
