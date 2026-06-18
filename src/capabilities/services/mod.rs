use crate::capabilities::render;
use crate::cli::{Cli, ServicesAction};
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

/// Normalize a unit name the way systemctl does client-side: if it already ends
/// in a recognized systemd unit-type extension, pass it through; otherwise append
/// `.service`. Path/slash escaping is intentionally not handled.
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
        ServicesAction::Status { unit } => reads::run(cli, ReadAction::Status { unit }),
        ServicesAction::Logs {
            unit,
            since,
            priority,
            lines,
            follow,
        } => reads::run(
            cli,
            ReadAction::Logs {
                unit,
                since: since.as_deref(),
                priority: priority.as_deref(),
                lines: *lines,
                follow: *follow,
            },
        ),
        ServicesAction::Start { unit } => mutations::run(cli, Mutation::Start, unit),
        ServicesAction::Stop { unit } => mutations::run(cli, Mutation::Stop, unit),
        ServicesAction::Restart { unit } => mutations::run(cli, Mutation::Restart, unit),
        ServicesAction::Reload { unit } => mutations::run(cli, Mutation::Reload, unit),
        ServicesAction::Enable { unit, now } => {
            mutations::run(cli, Mutation::Enable { now: *now }, unit)
        }
        ServicesAction::Disable { unit, now } => {
            mutations::run(cli, Mutation::Disable { now: *now }, unit)
        }
    };
    render(cli, view)
}

#[cfg(test)]
mod tests {
    use super::mangle_unit;

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
