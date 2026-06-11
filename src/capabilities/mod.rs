//! Capability implementations: the concrete commands fez exposes.

use crate::cli::Cli;
use crate::envelope::{ApiError, Envelope};
use crate::error::Result;
use serde_json::Value;

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
    pub fn new(kind: &'static str, host: String, data: Value, human: String) -> Self {
        View {
            kind,
            host,
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
/// (with structured [`crate::error::FezError::detail`]) under `--json` or a
/// `error: ...` line otherwise, and returns the error's exit code.
///
/// This attaches no error-specific `hints`; a capability that wants safe
/// read-only follow-up hints on its error envelopes calls [`render_with_hints`]
/// instead.
pub fn render(cli: &Cli, result: Result<View>) -> i32 {
    render_with_hints(cli, result, |_| None)
}

/// [`render`] with a per-capability error-hints hook.
///
/// `error_hints` maps a failing [`crate::error::FezError`] to an optional
/// envelope `hints` block (safe read-only follow-ups, e.g. a service-status
/// check). It runs only on the error path and only under `--json`; the success
/// path and plain-text error path are identical to [`render`]. Domain-specific
/// hint wording therefore stays in the capability, not in the shared error type.
pub fn render_with_hints<F>(cli: &Cli, result: Result<View>, error_hints: F) -> i32
where
    F: FnOnce(&crate::error::FezError) -> Option<Value>,
{
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
                if let Some(h) = error_hints(&e) {
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
    use super::{render, render_with_hints, View};
    use crate::cli::Cli;
    use crate::error::FezError;
    use clap::Parser;
    use serde_json::json;

    fn cli(args: &[&str]) -> Cli {
        Cli::try_parse_from(args).expect("args parse")
    }

    #[test]
    fn new_is_bare() {
        let v = View::new("Kind", "host".into(), json!({"a": 1}), "human\n".into());
        assert_eq!(v.kind, "Kind");
        assert_eq!(v.host, "host");
        assert_eq!(v.human, "human\n");
        assert!(v.hints.is_none());
        assert!(!v.pre_rendered);
    }

    #[test]
    fn with_hints_sets_hints() {
        let v = View::new("K", "h".into(), json!(null), String::new())
            .with_hints(json!({"reverse": "fez x"}));
        assert_eq!(v.hints.unwrap()["reverse"], "fez x");
    }

    #[test]
    fn with_hints_opt_passes_through() {
        let some = View::new("K", "h".into(), json!(null), String::new())
            .with_hints_opt(Some(json!({"k": 1})));
        assert!(some.hints.is_some());
        let none = View::new("K", "h".into(), json!(null), String::new()).with_hints_opt(None);
        assert!(none.hints.is_none());
    }

    #[test]
    fn pre_rendered_marks_the_view() {
        let v = View::new("K", "h".into(), json!(null), String::new()).pre_rendered();
        assert!(v.pre_rendered);
    }

    #[test]
    fn render_pre_rendered_view_is_silent_success() {
        let c = cli(&["fez", "services", "list"]);
        let v =
            View::new("LogEntries", "localhost".into(), json!(null), String::new()).pre_rendered();
        assert_eq!(render(&c, Ok(v)), 0);
    }

    #[test]
    fn render_ok_human_returns_zero() {
        let c = cli(&["fez", "services", "list"]);
        let v = View::new(
            "ServiceList",
            "localhost".into(),
            json!({"a": 1}),
            "out\n".into(),
        );
        assert_eq!(render(&c, Ok(v)), 0);
    }

    #[test]
    fn render_ok_json_with_hints_returns_zero() {
        let c = cli(&["fez", "--json", "services", "stop", "x"]);
        let v = View::new(
            "Stopped",
            "localhost".into(),
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
    fn render_with_hints_runs_hook_on_error_and_returns_exit_code() {
        let c = cli(&["fez", "--json", "firewall", "status"]);
        let err = FezError::UnsupportedApi("getMasquerade".into());
        let expected = err.exit_code();
        let called = std::cell::Cell::new(false);
        let exit = render_with_hints(&c, Err(err), |_| {
            called.set(true);
            Some(json!({"unsupported": "treat as unavailable"}))
        });
        assert_eq!(exit, expected);
        assert!(called.get(), "hook runs on the error path");
    }

    #[test]
    fn render_with_hints_skips_hook_on_success() {
        let c = cli(&["fez", "--json", "firewall", "status"]);
        let v = View::new(
            "FirewallStatus",
            "localhost".into(),
            json!({}),
            "ok\n".into(),
        );
        let called = std::cell::Cell::new(false);
        let exit = render_with_hints(&c, Ok(v), |_| {
            called.set(true);
            None
        });
        assert_eq!(exit, 0);
        assert!(!called.get(), "hook does not run on the success path");
    }
}

/// systemd service management capabilities.
pub mod services;

/// RPM package management capabilities (via dnf5daemon).
pub mod packages;

/// PackageKit fallback package backend (used when dnf5daemon is absent).
pub mod packages_pk;

/// NetworkManager inspection capabilities.
pub mod network;

/// firewalld management capabilities.
pub mod firewall;
