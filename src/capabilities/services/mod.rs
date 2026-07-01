use crate::capabilities::render;
use crate::capabilities::View;
use crate::cli::{Cli, ServicesAction};
use crate::error::{FezError, Result};
use std::borrow::Cow;

mod logs;
mod mutations;
mod reads;

const MGR_PATH: &str = "/org/freedesktop/systemd1";
const MGR_IFACE: &str = "org.freedesktop.systemd1.Manager";
const PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";
const UNIT_IFACE: &str = "org.freedesktop.systemd1.Unit";

/// systemd's recognized unit-type extensions. A name ending in one of these is
/// already fully qualified; anything else defaults to `.service`. Mirrors the
/// suffix-defaulting half of systemd's `unit_name_mangle()`.
const UNIT_SUFFIXES: [&str; 11] = [
    ".service",
    ".socket",
    ".target",
    ".timer",
    ".mount",
    ".automount",
    ".swap",
    ".path",
    ".slice",
    ".scope",
    ".device",
];

/// Validate and normalize a unit name before it crosses into systemd or journalctl.
///
/// Rejects names that are empty, too long, contain path separators or control
/// characters, or start with `-`.  After validation the name is defaulted to
/// `.service` when it lacks a recognised systemd unit-type suffix.
///
/// # Errors
///
/// Returns [`FezError::Usage`] (exit 2) on any validation failure so the agent
/// sees a stable error code rather than a systemd-level failure.
pub(crate) fn validate_unit(name: &str) -> Result<Cow<'_, str>> {
    const MAX_LEN: usize = 256;

    if name.is_empty() {
        return Err(FezError::Usage("unit name must not be empty".into()));
    }
    // Reject names with path separators (including `..` directory traversal).
    // `Path::file_name()` doesn't catch `../../etc/shadow` as a traversal
    // because it only returns the final component ("shadow").
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(FezError::Usage(format!(
            "unit name contains path separators: {name}"
        )));
    }
    // After the structural checks, strip any accidental leading path components
    // (e.g. a path prefix without separators shouldn't happen, but be safe).
    let basename = std::path::Path::new(name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(name);
    // Reject names that carry control characters, newlines, null bytes, or
    // start with `-` (prevents option-injection into journalctl argv).
    for ch in basename.chars() {
        if ch.is_control() || ch == '\u{fffc}' || ch == '\u{fffd}' {
            return Err(FezError::Usage(format!(
                "unit name contains control character U+{ch:04X}",
                ch = ch as u32
            )));
        }
    }
    if basename.starts_with('-') {
        return Err(FezError::Usage(format!(
            "unit name must not start with '-': {basename}"
        )));
    }
    // Mangle first, then check length — `mangle_unit` may append `.service`.
    let mangled = mangle_unit(basename);
    if mangled.len() > MAX_LEN {
        return Err(FezError::Usage(format!(
            "unit name too long after mangling ({} > {MAX_LEN})",
            mangled.len()
        )));
    }
    Ok(mangled)
}

/// journalctl `--priority` levels. Must be one of the named levels or 0-7.
const LOG_PRIORITIES: &[&str] = &[
    "emerg", "alert", "crit", "err", "warning", "notice", "info", "debug",
];

/// Validate a journalctl `--since` argument.
///
/// Rejects values that start with `-` (prevents option injection into
/// journalctl's argv). Other than that, journalctl's own parser validates
/// the timestamp; fez only enforces the structural guard.
pub(crate) fn validate_log_since(raw: &str) -> Result<()> {
    if raw.is_empty() || raw.starts_with('-') {
        return Err(FezError::Usage(format!("invalid --since value: {raw}")));
    }
    Ok(())
}

/// Validate a journalctl `--priority` argument.
///
/// Accepts the eight named syslog levels (case-insensitive) and numeric 0-7.
pub(crate) fn validate_log_priority(raw: &str) -> Result<()> {
    if raw.is_empty() || raw.starts_with('-') {
        return Err(FezError::Usage(format!("invalid --priority value: {raw}")));
    }
    if LOG_PRIORITIES
        .iter()
        .any(|level| level.eq_ignore_ascii_case(raw))
    {
        return Ok(());
    }
    if let Ok(n) = raw.parse::<u8>() {
        if n <= 7 {
            return Ok(());
        }
    }
    Err(FezError::Usage(format!(
        "invalid --priority value: {raw} (expected emerg, alert, crit, err, warning, notice, info, debug, or 0-7)"
    )))
}

/// Normalize a unit name the way systemctl does client-side: if it already ends
/// in a recognized systemd unit-type extension, pass it through; otherwise
/// append `.service`.
fn mangle_unit(name: &str) -> Cow<'_, str> {
    if UNIT_SUFFIXES.iter().any(|suffix| name.ends_with(suffix)) {
        Cow::Borrowed(name)
    } else {
        Cow::Owned(format!("{name}.service"))
    }
}

#[derive(Clone, Copy)]
enum Mutation {
    Start,
    Stop,
    Restart,
    Reload,
    Enable { now: bool },
    Disable { now: bool },
}

/// A read subcommand and its arguments, borrowed from the parsed action.
///
/// Splitting reads out of [`ServicesAction`] makes [`reads::run`] total: every
/// variant here maps to a handler, so adding one is a compile error rather than
/// a runtime panic.
enum ReadAction<'a> {
    List {
        state: Option<&'a str>,
    },
    Status {
        unit: &'a str,
    },
    Logs {
        unit: &'a str,
        since: Option<&'a str>,
        priority: Option<&'a str>,
        lines: Option<u32>,
        follow: bool,
    },
}

/// Run the requested `services` subcommand and return the process exit code.
pub fn dispatch(cli: &Cli, action: &ServicesAction) -> i32 {
    let view = match action {
        ServicesAction::List { state } => reads::run(
            cli,
            ReadAction::List {
                state: state.as_deref(),
            },
        ),
        ServicesAction::Status { unit } => match validate_unit(unit) {
            Ok(u) => reads::run(cli, ReadAction::Status { unit: &u }),
            Err(e) => Err(e),
        },
        ServicesAction::Logs {
            unit,
            since,
            priority,
            lines,
            follow,
        } => {
            let unit = match validate_unit(unit) {
                Ok(u) => u,
                Err(e) => return render(cli, Err(e)),
            };
            if let Some(raw) = since.as_deref() {
                if let Err(e) = validate_log_since(raw) {
                    return render(cli, Err(e));
                }
            }
            if let Some(raw) = priority.as_deref() {
                if let Err(e) = validate_log_priority(raw) {
                    return render(cli, Err(e));
                }
            }
            reads::run(
                cli,
                ReadAction::Logs {
                    unit: &unit,
                    since: since.as_deref(),
                    priority: priority.as_deref(),
                    lines: *lines,
                    follow: *follow,
                },
            )
        }
        ServicesAction::Start { unit }
        | ServicesAction::Stop { unit }
        | ServicesAction::Restart { unit }
        | ServicesAction::Reload { unit }
        | ServicesAction::Enable { unit, .. }
        | ServicesAction::Disable { unit, .. } => validate_and_mutate(cli, action, unit),
    };
    render(cli, view)
}

/// Validate the unit, then dispatch the mutation.
fn validate_and_mutate(cli: &Cli, action: &ServicesAction, unit: &str) -> Result<View> {
    let unit: String = validate_unit(unit)?.into_owned();
    match action {
        ServicesAction::Start { .. } => mutations::run(cli, Mutation::Start, &unit),
        ServicesAction::Stop { .. } => mutations::run(cli, Mutation::Stop, &unit),
        ServicesAction::Restart { .. } => mutations::run(cli, Mutation::Restart, &unit),
        ServicesAction::Reload { .. } => mutations::run(cli, Mutation::Reload, &unit),
        ServicesAction::Enable { now, .. } => {
            mutations::run(cli, Mutation::Enable { now: *now }, &unit)
        }
        ServicesAction::Disable { now, .. } => {
            mutations::run(cli, Mutation::Disable { now: *now }, &unit)
        }
        _ => Err(FezError::Usage(
            "bug: validate_and_mutate called for non-mutation action".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{mangle_unit, validate_log_priority, validate_log_since, validate_unit};
    use crate::error::FezError;

    #[test]
    fn validate_rejects_empty_and_overlong() {
        assert!(matches!(validate_unit(""), Err(FezError::Usage(_))));
        let long = "a".repeat(257);
        assert!(matches!(validate_unit(&long), Err(FezError::Usage(_))));
    }

    #[test]
    fn validate_rejects_path_separators() {
        assert!(matches!(
            validate_unit("../../etc/shadow"),
            Err(FezError::Usage(_))
        ));
        assert!(matches!(
            validate_unit("/usr/lib/systemd/system/evil.service"),
            Err(FezError::Usage(_))
        ));
    }

    #[test]
    fn validate_rejects_control_chars() {
        assert!(matches!(
            validate_unit("foo\x00bar"),
            Err(FezError::Usage(_))
        ));
        assert!(matches!(validate_unit("foo\nbar"), Err(FezError::Usage(_))));
        assert!(matches!(
            validate_unit("foo\x1bbar"),
            Err(FezError::Usage(_))
        ));
    }

    #[test]
    fn validate_rejects_leading_dash() {
        assert!(matches!(validate_unit("--help"), Err(FezError::Usage(_))));
    }

    #[test]
    fn validate_accepts_normal_units() {
        assert_eq!(
            validate_unit("sshd.service").unwrap().as_ref(),
            "sshd.service"
        );
        assert_eq!(
            validate_unit("chronyd").unwrap().as_ref(),
            "chronyd.service"
        );
    }

    #[test]
    fn validate_allows_known_chars_in_unit_names() {
        // systemd allows alphanumerics, `:`, `@`, `.`, `_`, `\\`, `-`
        assert_eq!(
            validate_unit("getty@tty1.service").unwrap().as_ref(),
            "getty@tty1.service"
        );
        assert_eq!(
            validate_unit("dev-sda1.device").unwrap().as_ref(),
            "dev-sda1.device"
        );
    }

    #[test]
    fn validate_log_priority_accepts_known_levels() {
        assert!(validate_log_priority("emerg").is_ok());
        assert!(validate_log_priority("alert").is_ok());
        assert!(validate_log_priority("crit").is_ok());
        assert!(validate_log_priority("err").is_ok());
        assert!(validate_log_priority("warning").is_ok());
        assert!(validate_log_priority("notice").is_ok());
        assert!(validate_log_priority("info").is_ok());
        assert!(validate_log_priority("debug").is_ok());
        assert!(validate_log_priority("0").is_ok());
        assert!(validate_log_priority("7").is_ok());
    }

    #[test]
    fn validate_log_priority_rejects_bad_values() {
        assert!(matches!(
            validate_log_priority("critical"),
            Err(FezError::Usage(_))
        ));
        assert!(matches!(
            validate_log_priority("8"),
            Err(FezError::Usage(_))
        ));
        assert!(matches!(validate_log_priority(""), Err(FezError::Usage(_))));
        assert!(matches!(
            validate_log_priority("--help"),
            Err(FezError::Usage(_))
        ));
    }

    #[test]
    fn validate_log_since_rejects_leading_dash() {
        assert!(matches!(
            validate_log_since("--help"),
            Err(FezError::Usage(_))
        ));
    }

    #[test]
    fn validate_log_since_accepts_typical_timestamps() {
        assert!(validate_log_since("2024-01-01").is_ok());
        assert!(validate_log_since("1 hour ago").is_ok());
        assert!(validate_log_since("yesterday").is_ok());
    }

    #[test]
    fn mangle_appends_service_to_bare_name() {
        assert_eq!(mangle_unit("NetworkManager"), "NetworkManager.service");
    }

    #[test]
    fn mangle_leaves_known_suffixes_untouched() {
        for name in [
            "sshd.service",
            "dbus.socket",
            "multi-user.target",
            "logrotate.timer",
            "var-lib.mount",
            "proc-sys.automount",
            "dev-sda1.swap",
            "run-foo.path",
            "user.slice",
            "session-1.scope",
            "dev-sda.device",
        ] {
            assert_eq!(mangle_unit(name), name);
        }
    }

    #[test]
    fn mangle_treats_unknown_dotted_tail_as_bare() {
        // Matches systemd: only a *recognized* unit-type extension is left alone.
        assert_eq!(mangle_unit("foo.bar"), "foo.bar.service");
    }

    #[test]
    fn mangle_does_not_double_suffix_service() {
        assert_eq!(mangle_unit("sshd.service"), "sshd.service");
    }
}
