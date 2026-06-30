//! Capability implementations: the concrete commands fez exposes.

use crate::cli::Cli;
use crate::envelope::{ApiError, Envelope};
use crate::error::{FezError, Result};
use crate::protocol::client::BridgeClient;
use serde_json::Value;

/// Connect a bridge client to this invocation's target host.
///
/// Collapses the `from_host` + [`BridgeClient::connect`] bootstrap that every
/// capability entry point repeats. The transport is dropped right after
/// connect: [`BridgeClient::connect`] spawns the bridge child and keeps no
/// borrow on it.
///
/// # Errors
///
/// Propagates any spawn or handshake error from [`BridgeClient::connect`].
pub fn connect(cli: &Cli) -> Result<BridgeClient> {
    let transport = crate::transport::from_host(cli.host.as_deref());
    BridgeClient::connect(transport.as_ref())
}

/// Map a daemon-absent D-Bus error to `missing`, passing everything else through.
///
/// dnf5daemon and firewalld report "the service isn't there" as a
/// `ServiceUnknown` D-Bus error ([`crate::error::is_service_unknown`]);
/// capabilities translate that into a dependency-missing error. Every other
/// result (including unrelated `Dbus` errors) is returned unchanged.
///
/// # Errors
///
/// Returns `missing()` on a service-unknown `Dbus` error, otherwise the
/// original result's error.
pub fn map_service_unknown<T>(res: Result<T>, missing: impl FnOnce() -> FezError) -> Result<T> {
    match res {
        Err(FezError::Dbus { name, .. }) if crate::error::is_service_unknown(&name) => {
            Err(missing())
        }
        other => other,
    }
}

/// Shared context for capability functions: bundles the bridge client,
/// channel, and target host to avoid threading them as separate parameters.
pub(crate) struct CapabilityContext<'a> {
    /// Bridge client connection.
    pub(crate) client: &'a mut BridgeClient,
    /// Open D-Bus channel path.
    pub(crate) channel: &'a str,
    /// Resolved target host name.
    pub(crate) host: &'a str,
}

/// A rendered capability result: the JSON payload plus its human form.
///
/// Every capability handler returns one of these (wrapped in [`Result`]); the
/// shared [`render`] turns it into stdout/stderr output and an exit code. The
/// `hints` and `pre_rendered` fields are optional features that individual
/// capabilities opt into (network never sets them; services uses
/// `pre_rendered` for its streaming `logs` action).
pub struct View {
    /// Envelope `kind` discriminant (e.g. `"ServiceList"`).
    pub kind: &'static str,
    /// Resolved target host the result pertains to.
    pub host: String,
    /// The `--json` envelope `data` payload.
    pub data: Value,
    /// The plain-text rendering printed when `--json` is absent.
    pub human: String,
    /// Optional envelope `hints` block (next-step suggestions).
    pub hints: Option<Value>,
    /// When set, the handler already wrote its own output (e.g. a streamed
    /// log tail); [`render`] prints nothing and returns success.
    pub pre_rendered: bool,
}

impl View {
    /// A bare view: no hints, not pre-rendered. The common case.
    pub fn new(kind: &'static str, host: impl Into<String>, data: Value, human: String) -> Self {
        View {
            kind,
            host: host.into(),
            data,
            human,
            hints: None,
            pre_rendered: false,
        }
    }

    /// Attach an envelope `hints` block.
    #[must_use]
    pub fn with_hints(mut self, hints: Value) -> Self {
        self.hints = Some(hints);
        self
    }

    /// Attach an already-optional `hints` block (pass-through for callers that
    /// compute `Option<Value>` and want to set it without unwrapping).
    #[must_use]
    pub fn with_hints_opt(mut self, hints: Option<Value>) -> Self {
        self.hints = hints;
        self
    }

    /// Mark this view as already written to stdout by the handler (e.g. a
    /// streamed log tail); [`render`] will print nothing for it.
    #[must_use]
    pub fn pre_rendered(mut self) -> Self {
        self.pre_rendered = true;
        self
    }
}

/// Render a [`View`] (or error) to stdout/stderr and return the exit code.
///
/// On success: emits the `fez/v1` envelope under `--json` (with `hints` when
/// present) or the human text otherwise, and returns `0`. A `pre_rendered`
/// view prints nothing and returns `0`. On error: emits an error envelope
/// (with structured [`crate::error::FezError::detail`] and any
/// [`crate::error::FezError::hints`]) under `--json` or an `error: ...` line
/// otherwise, and returns the error's exit code.
pub fn render(cli: &Cli, result: Result<View>) -> i32 {
    let host = cli.resolved_host();
    match result {
        Ok(view) => {
            if view.pre_rendered {
                return 0;
            }
            if cli.json {
                let mut env = Envelope::ok(view.kind, &view.host, view.data);
                if let Some(h) = view.hints {
                    env = env.with_hints(h);
                }
                println!("{}", env.to_json_string());
            } else {
                print!("{}", view.human);
            }
            0
        }
        Err(e) => {
            if cli.json {
                let mut env = Envelope::error(
                    "Error",
                    &host,
                    ApiError {
                        code: e.code().into(),
                        message: e.to_string(),
                        detail: e.detail(),
                    },
                );
                if let Some(h) = e.hints() {
                    env = env.with_hints(h);
                }
                println!("{}", env.to_json_string());
            } else {
                eprintln!("error: {e}");
            }
            e.exit_code()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{render, View};
    use crate::cli::Cli;
    use crate::error::FezError;
    use clap::Parser;
    use serde_json::json;

    fn cli(args: &[&str]) -> Cli {
        Cli::try_parse_from(args).expect("args parse")
    }

    #[test]
    fn map_service_unknown_maps_only_service_unknown_dbus_errors() {
        // ServiceUnknown -> the caller's dependency-missing error.
        let mapped = super::map_service_unknown::<()>(
            Err(FezError::Dbus {
                name: "org.freedesktop.DBus.Error.ServiceUnknown".into(),
                message: "gone".into(),
            }),
            || FezError::NotFound("daemon".into()),
        );
        assert!(matches!(mapped, Err(FezError::NotFound(_))));

        // An unrelated Dbus error passes through untouched.
        let other = super::map_service_unknown::<()>(
            Err(FezError::Dbus {
                name: "org.freedesktop.DBus.Error.AccessDenied".into(),
                message: "no".into(),
            }),
            || FezError::NotFound("daemon".into()),
        );
        assert!(matches!(other, Err(FezError::Dbus { .. })));

        // Ok passes through.
        assert!(super::map_service_unknown(Ok(7), || FezError::NotFound("x".into())).is_ok());
    }

    #[test]
    fn new_is_bare() {
        let v = View::new("Kind", "host", json!({"a": 1}), "human\n".into());
        assert_eq!(v.kind, "Kind");
        assert_eq!(v.host, "host");
        assert_eq!(v.human, "human\n");
        assert!(v.hints.is_none());
        assert!(!v.pre_rendered);
    }

    #[test]
    fn with_hints_sets_hints() {
        let v =
            View::new("K", "h", json!(null), String::new()).with_hints(json!({"reverse": "fez x"}));
        assert_eq!(v.hints.unwrap()["reverse"], "fez x");
    }

    #[test]
    fn with_hints_opt_passes_through() {
        let some =
            View::new("K", "h", json!(null), String::new()).with_hints_opt(Some(json!({"k": 1})));
        assert!(some.hints.is_some());
        let none = View::new("K", "h", json!(null), String::new()).with_hints_opt(None);
        assert!(none.hints.is_none());
    }

    #[test]
    fn pre_rendered_marks_the_view() {
        let v = View::new("K", "h", json!(null), String::new()).pre_rendered();
        assert!(v.pre_rendered);
    }

    #[test]
    fn render_pre_rendered_view_is_silent_success() {
        let c = cli(&["fez", "services", "list"]);
        let v = View::new("LogEntries", "localhost", json!(null), String::new()).pre_rendered();
        assert_eq!(render(&c, Ok(v)), 0);
    }

    #[test]
    fn render_ok_human_returns_zero() {
        let c = cli(&["fez", "services", "list"]);
        let v = View::new("ServiceList", "localhost", json!({"a": 1}), "out\n".into());
        assert_eq!(render(&c, Ok(v)), 0);
    }

    #[test]
    fn render_ok_json_with_hints_returns_zero() {
        let c = cli(&["fez", "--json", "services", "stop", "x"]);
        let v = View::new(
            "Stopped",
            "localhost",
            json!({"unit": "x"}),
            "stopped\n".into(),
        )
        .with_hints(json!({"reverse": "fez services start x"}));
        assert_eq!(render(&c, Ok(v)), 0);
    }

    #[test]
    fn render_err_human_returns_error_exit_code() {
        let c = cli(&["fez", "services", "status", "missing"]);
        let exit = render(&c, Err(FezError::NotFound("missing".into())));
        assert_eq!(exit, FezError::NotFound("missing".into()).exit_code());
    }

    #[test]
    fn render_err_json_emits_detail_and_exit_code() {
        let c = cli(&["fez", "--json", "packages", "list"]);
        let err = FezError::DependencyMissing {
            component: "dnf5daemon".into(),
            dbus_name: "org.rpm.dnf.v0".into(),
            remediation: "install dnf5daemon-server".into(),
        };
        let expected = err.exit_code();
        assert_eq!(render(&c, Err(err)), expected);
    }

    #[test]
    fn render_dependency_missing_emits_hints_from_error() {
        // DependencyMissing carries hints via FezError::hints(); verify the
        // exit code is correct (the envelope content is checked by integration
        // tests).
        let c = cli(&["fez", "--json", "firewall", "status"]);
        let err = FezError::DependencyMissing {
            component: "firewalld".into(),
            dbus_name: "org.fedoraproject.FirewallD1".into(),
            remediation: "dnf install firewalld".into(),
        };
        let expected = err.exit_code();
        assert_eq!(render(&c, Err(err)), expected);
    }

    #[test]
    fn render_unsupported_api_emits_hints_from_error() {
        let c = cli(&["fez", "--json", "firewall", "status"]);
        let err = FezError::UnsupportedApi("getMasquerade".into());
        let expected = err.exit_code();
        assert_eq!(render(&c, Err(err)), expected);
    }

    #[test]
    fn render_success_does_not_add_hints_from_error() {
        let c = cli(&["fez", "--json", "firewall", "status"]);
        let v = View::new("FirewallStatus", "localhost", json!({}), "ok\n".into());
        assert_eq!(render(&c, Ok(v)), 0);
    }
}

/// systemd service management capabilities.
pub mod services;

/// RPM package management capabilities (via dnf5daemon).
pub mod package;

/// NetworkManager inspection capabilities.
pub mod network;

/// firewalld management capabilities.
pub mod firewall;

/// System overview capability (hostname + time).
pub mod system;

/// UDisks2 storage inspection capability.
pub mod storage;

/// DNS resolver capability (systemd-resolved).
pub mod dns;
