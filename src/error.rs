//! Crate-wide error type and its mapping to stable codes and exit statuses.
use serde_json::{json, Value};
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
    /// A required target dependency (e.g. dnf5daemon) is absent or not activatable.
    #[error("missing dependency {component} on target: {remediation}")]
    DependencyMissing {
        /// Human-facing name of the missing component (e.g. `dnf5daemon`).
        component: String,
        /// The D-Bus name that failed to activate (e.g. `org.rpm.dnf.v0`).
        dbus_name: String,
        /// Actionable guidance to install/enable the dependency and retry.
        remediation: String,
    },
    /// A resolved transaction was refused by removal guardrails without `--force`.
    #[error("refused: dangerous transaction ({reason}); use --force to override")]
    DangerousTransaction {
        /// Why the transaction was refused (protected package or cascade size).
        reason: String,
        /// Package names the resolved plan would remove.
        removed: Vec<String>,
    },
    /// A CLI usage error (clap parse failure: missing/invalid argument or
    /// unknown flag) surfaced as a `fez/v1` envelope because `--json` was
    /// requested. The plain-text path keeps clap's own rendering.
    #[error("usage error: {0}")]
    Usage(String),
    /// The user declined a confirmation prompt.
    #[error("aborted by user")]
    Aborted,
    /// Privilege escalation to root failed (the bridge could not become root,
    /// e.g. sudo requires a password fez does not supply).
    #[error("access denied: {remediation}")]
    AccessDenied {
        /// Actionable guidance to enable privilege escalation and retry.
        remediation: String,
    },
    /// The managed subsystem is present but does not expose a D-Bus method fez
    /// needs (the call returned `UnknownMethod`), e.g. an older firewalld
    /// without `getMasquerade`. Distinct from a missing dependency: the service
    /// is reachable but its API surface is too old.
    #[error("unsupported API: {0} is not available on the target")]
    UnsupportedApi(String),
}

/// One documented exit code in the agent-facing contract.
pub struct ExitCodeDoc {
    /// Numeric process exit code.
    pub code: i32,
    /// Stable label tying the code to its error category.
    pub label: &'static str,
    /// One-line human meaning.
    pub meaning: &'static str,
}

/// The agent-facing exit-code contract. `fez guide` renders this; a test
/// asserts every fatal code `exit_code()` can produce appears here. The
/// `exit_code()` match below stays the compile-time-exhaustive source for
/// per-variant mapping.
pub const EXIT_CODES: &[ExitCodeDoc] = &[
    ExitCodeDoc {
        code: 1,
        label: "general",
        meaning: "Unclassified failure (I/O, decode, aborted).",
    },
    ExitCodeDoc {
        code: 2,
        label: "usage",
        meaning: "CLI usage error (missing/invalid argument or unknown flag).",
    },
    ExitCodeDoc {
        code: 4,
        label: "not-found",
        meaning: "Target resource (e.g. a unit) does not exist.",
    },
    ExitCodeDoc {
        code: 5,
        label: "timeout",
        meaning: "The bridge did not respond before the deadline.",
    },
    ExitCodeDoc {
        code: 6,
        label: "bridge",
        meaning: "Bridge could not be spawned or the connection closed.",
    },
    ExitCodeDoc {
        code: 7,
        label: "dbus",
        meaning: "A D-Bus call returned an error.",
    },
    ExitCodeDoc {
        code: 8,
        label: "protected-unit",
        meaning: "Protected unit refused without --force.",
    },
    ExitCodeDoc {
        code: 9,
        label: "dependency-missing",
        meaning: "Required target dependency (dnf5daemon) is absent or not activatable.",
    },
    ExitCodeDoc {
        code: 10,
        label: "dangerous-transaction",
        meaning: "Resolved transaction refused by guardrails (protected package or cascade) without --force.",
    },
    ExitCodeDoc {
        code: 11,
        label: "access-denied",
        meaning: "Privilege escalation to root failed (e.g. sudo requires a password fez does not supply).",
    },
    ExitCodeDoc {
        code: 12,
        label: "unsupported-api",
        meaning: "The managed subsystem is reachable but lacks a required D-Bus method (API too old).",
    },
];

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
            FezError::DependencyMissing { .. } => "dependency-missing",
            FezError::DangerousTransaction { .. } => "dangerous-transaction",
            FezError::Usage(_) => "usage",
            FezError::Aborted => "aborted",
            FezError::AccessDenied { .. } => "access-denied",
            FezError::UnsupportedApi(_) => "unsupported-api",
        }
    }
    /// Process exit code to use when this error is fatal.
    pub fn exit_code(&self) -> i32 {
        match self {
            // Match clap's own convention so plain-text and --json usage errors
            // share exit 2.
            FezError::Usage(_) => 2,
            FezError::NotFound(_) | FezError::Problem(_) => 4,
            FezError::Timeout => 5,
            FezError::Spawn { .. } | FezError::BridgeClosed => 6,
            FezError::Dbus { .. } => 7,
            FezError::Protected { .. } => 8,
            FezError::DependencyMissing { .. } => 9,
            FezError::DangerousTransaction { .. } => 10,
            FezError::AccessDenied { .. } => 11,
            FezError::UnsupportedApi(_) => 12,
            _ => 1,
        }
    }
    /// Structured `detail` for the `fez/v1` error envelope.
    ///
    /// Returns the machine-readable payload for the error variants that carry
    /// one (`DependencyMissing`, `DangerousTransaction`, `UnsupportedApi`);
    /// every other variant has no structured detail and yields `None`.
    /// Capabilities call this when rendering an error envelope so the mapping
    /// lives in one place.
    pub fn detail(&self) -> Option<Value> {
        match self {
            FezError::DependencyMissing {
                component,
                dbus_name,
                remediation,
            } => Some(json!({
                "component": component,
                "dbusName": dbus_name,
                "remediation": remediation,
            })),
            FezError::DangerousTransaction { reason, removed } => Some(json!({
                "reason": reason,
                "removed": removed,
            })),
            FezError::UnsupportedApi(method) => Some(json!({ "method": method })),
            _ => None,
        }
    }

    /// Safe read-only follow-up hints for actionable errors.
    ///
    /// Returns a hints block suitable for the `fez/v1` error envelope, or
    /// `None` for errors without a useful next-step suggestion. The hint is
    /// derived from the error's own fields so it is correct regardless of which
    /// capability produced the error.
    ///
    /// - `DependencyMissing`: exposes the `remediation` text already carried by
    ///   the error, so the agent can act without parsing the `message` string.
    /// - `UnsupportedApi`: tells the caller the feature is absent on this host
    ///   and should not be retried.
    pub fn hints(&self) -> Option<Value> {
        match self {
            FezError::DependencyMissing { remediation, .. } => {
                Some(json!({ "remediation": remediation }))
            }
            FezError::UnsupportedApi(method) => Some(json!({
                "unsupported": format!("this host does not expose {method}; treat the feature as unsupported"),
            })),
            _ => None,
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

/// Whether a D-Bus error name indicates the service is not activatable (the
/// signal that a required daemon like dnf5daemon is not installed/running).
pub fn is_service_unknown(name: &str) -> bool {
    name.contains("ServiceUnknown") || name.contains("NameHasNoOwner")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_table_documents_every_nonone_code() {
        use std::collections::HashSet;
        let documented: HashSet<i32> = EXIT_CODES.iter().map(|e| e.code).collect();
        let produced = [
            FezError::NotFound("x".into()).exit_code(),
            FezError::Timeout.exit_code(),
            FezError::BridgeClosed.exit_code(),
            FezError::Dbus {
                name: "n".into(),
                message: "m".into(),
            }
            .exit_code(),
            FezError::Protected { unit: "u".into() }.exit_code(),
            FezError::DependencyMissing {
                component: "c".into(),
                dbus_name: "n".into(),
                remediation: "r".into(),
            }
            .exit_code(),
            FezError::DangerousTransaction {
                reason: "r".into(),
                removed: vec![],
            }
            .exit_code(),
            FezError::AccessDenied {
                remediation: "r".into(),
            }
            .exit_code(),
            FezError::Usage("missing <UNIT>".into()).exit_code(),
            FezError::UnsupportedApi("getMasquerade".into()).exit_code(),
        ];
        for code in produced {
            if code != 1 {
                assert!(
                    documented.contains(&code),
                    "exit code {code} undocumented in EXIT_CODES"
                );
            }
        }
    }

    #[test]
    fn exit_code_table_is_nonempty_and_sorted() {
        assert!(!EXIT_CODES.is_empty());
        let codes: Vec<i32> = EXIT_CODES.iter().map(|e| e.code).collect();
        let mut sorted = codes.clone();
        sorted.sort_unstable();
        assert_eq!(codes, sorted, "EXIT_CODES should be ascending by code");
    }

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
    fn dependency_missing_maps_code_and_exit() {
        let e = FezError::DependencyMissing {
            component: "dnf5daemon".into(),
            dbus_name: "org.rpm.dnf.v0".into(),
            remediation: "install it".into(),
        };
        assert_eq!(e.code(), "dependency-missing");
        assert_eq!(e.exit_code(), 9);
    }

    #[test]
    fn dangerous_transaction_maps_code_and_exit() {
        let e = FezError::DangerousTransaction {
            reason: "removes protected package glibc".into(),
            removed: vec!["glibc".into()],
        };
        assert_eq!(e.code(), "dangerous-transaction");
        assert_eq!(e.exit_code(), 10);
    }

    #[test]
    fn is_service_unknown_detects_activation_failure() {
        assert!(is_service_unknown(
            "org.freedesktop.DBus.Error.ServiceUnknown"
        ));
        assert!(is_service_unknown(
            "org.freedesktop.DBus.Error.NameHasNoOwner"
        ));
        assert!(!is_service_unknown("org.freedesktop.systemd1.NoSuchUnit"));
    }

    #[test]
    fn aborted_maps_code_and_exit() {
        assert_eq!(FezError::Aborted.code(), "aborted");
        assert_eq!(FezError::Aborted.exit_code(), 1);
    }

    #[test]
    fn usage_maps_code_and_exit() {
        let e = FezError::Usage("missing required argument: <UNIT>".into());
        assert_eq!(e.code(), "usage");
        assert_eq!(e.exit_code(), 2);
        assert!(e.to_string().contains("missing required argument"));
    }

    #[test]
    fn unsupported_api_maps_code_and_exit() {
        let e = FezError::UnsupportedApi("getMasquerade".into());
        assert_eq!(e.code(), "unsupported-api");
        assert_eq!(e.exit_code(), 12);
        assert!(e.to_string().contains("getMasquerade"));
    }

    #[test]
    fn detail_carries_dependency_missing_fields() {
        let e = FezError::DependencyMissing {
            component: "dnf5daemon".into(),
            dbus_name: "org.rpm.dnf.v0".into(),
            remediation: "install it".into(),
        };
        let d = e.detail().expect("dependency-missing has detail");
        assert_eq!(d["component"], "dnf5daemon");
        assert_eq!(d["dbusName"], "org.rpm.dnf.v0");
        assert_eq!(d["remediation"], "install it");
    }

    #[test]
    fn detail_carries_dangerous_transaction_fields() {
        let e = FezError::DangerousTransaction {
            reason: "removes glibc".into(),
            removed: vec!["glibc".into()],
        };
        let d = e.detail().expect("dangerous-transaction has detail");
        assert_eq!(d["reason"], "removes glibc");
        assert_eq!(d["removed"], json!(["glibc"]));
    }

    #[test]
    fn detail_carries_unsupported_api_method() {
        let e = FezError::UnsupportedApi("getMasquerade".into());
        let d = e.detail().expect("unsupported-api has detail");
        assert_eq!(d["method"], "getMasquerade");
    }

    #[test]
    fn detail_is_none_for_variants_without_structured_payload() {
        assert!(FezError::Timeout.detail().is_none());
        assert!(FezError::NotFound("x".into()).detail().is_none());
        assert!(FezError::Protected { unit: "u".into() }.detail().is_none());
        assert!(FezError::AccessDenied {
            remediation: "r".into()
        }
        .detail()
        .is_none());
    }

    #[test]
    fn access_denied_maps_code_and_exit() {
        let e = FezError::AccessDenied {
            remediation: "configure NOPASSWD sudo".into(),
        };
        assert_eq!(e.code(), "access-denied");
        assert_eq!(e.exit_code(), 11);
        assert!(e.to_string().contains("configure NOPASSWD sudo"));
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
        assert_eq!(
            FezError::DependencyMissing {
                component: "dnf5daemon".into(),
                dbus_name: "org.rpm.dnf.v0".into(),
                remediation: "install it".into(),
            }
            .to_string(),
            "missing dependency dnf5daemon on target: install it"
        );
        assert_eq!(
            FezError::DangerousTransaction {
                reason: "removes glibc".into(),
                removed: vec!["glibc".into()],
            }
            .to_string(),
            "refused: dangerous transaction (removes glibc); use --force to override"
        );
    }
}
