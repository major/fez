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

/// The read/mutate split of a parsed [`ServicesAction`].
///
/// [`classify`] is the single place that matches the full clap enum; everything
/// downstream consumes one arm of this and is therefore total.
enum Plan<'a> {
    Read(ReadAction<'a>),
    Mutate { mutation: Mutation, unit: &'a str },
}

/// Map the flat clap enum onto the read/mutate [`Plan`] split.
///
/// This is the only exhaustive match over [`ServicesAction`]; the rest of the
/// module works off [`Plan`], so a new variant breaks the build here instead of
/// hitting an `unreachable!` at runtime.
fn classify(action: &ServicesAction) -> Plan<'_> {
    match action {
        ServicesAction::List { state } => Plan::Read(ReadAction::List {
            state: state.as_deref(),
        }),
        ServicesAction::Status { unit } => Plan::Read(ReadAction::Status { unit }),
        ServicesAction::Logs {
            unit,
            since,
            priority,
            lines,
            follow,
        } => Plan::Read(ReadAction::Logs {
            unit,
            since: since.as_deref(),
            priority: priority.as_deref(),
            lines: *lines,
            follow: *follow,
        }),
        ServicesAction::Start { unit } => Plan::Mutate {
            mutation: Mutation::Start,
            unit,
        },
        ServicesAction::Stop { unit } => Plan::Mutate {
            mutation: Mutation::Stop,
            unit,
        },
        ServicesAction::Restart { unit } => Plan::Mutate {
            mutation: Mutation::Restart,
            unit,
        },
        ServicesAction::Reload { unit } => Plan::Mutate {
            mutation: Mutation::Reload,
            unit,
        },
        ServicesAction::Enable { unit, now } => Plan::Mutate {
            mutation: Mutation::Enable { now: *now },
            unit,
        },
        ServicesAction::Disable { unit, now } => Plan::Mutate {
            mutation: Mutation::Disable { now: *now },
            unit,
        },
    }
}

/// Run the requested `services` subcommand and return the process exit code.
pub fn dispatch(cli: &Cli, action: &ServicesAction) -> i32 {
    let view = match classify(action) {
        Plan::Read(read) => reads::run(cli, read),
        Plan::Mutate { mutation, unit } => mutations::run(cli, mutation, unit),
    };
    render(cli, view)
}

#[cfg(test)]
mod tests {
    use super::{classify, mangle_unit, Mutation, Plan};
    use crate::cli::ServicesAction;

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

    #[test]
    fn classify_routes_reads() {
        assert!(matches!(
            classify(&ServicesAction::List { state: None }),
            Plan::Read(_)
        ));
        assert!(matches!(
            classify(&ServicesAction::Status {
                unit: "sshd".into()
            }),
            Plan::Read(_)
        ));
        assert!(matches!(
            classify(&ServicesAction::Logs {
                unit: "sshd".into(),
                since: None,
                priority: None,
                lines: Some(50),
                follow: false,
            }),
            Plan::Read(_)
        ));
    }

    #[test]
    fn classify_routes_mutations() {
        let cases = [
            (ServicesAction::Start { unit: "u".into() }, Mutation::Start),
            (ServicesAction::Stop { unit: "u".into() }, Mutation::Stop),
            (
                ServicesAction::Restart { unit: "u".into() },
                Mutation::Restart,
            ),
            (
                ServicesAction::Reload { unit: "u".into() },
                Mutation::Reload,
            ),
            (
                ServicesAction::Enable {
                    unit: "u".into(),
                    now: true,
                },
                Mutation::Enable { now: true },
            ),
            (
                ServicesAction::Disable {
                    unit: "u".into(),
                    now: false,
                },
                Mutation::Disable { now: false },
            ),
        ];
        for (action, want) in cases {
            match classify(&action) {
                Plan::Mutate { mutation, unit } => {
                    assert!(matches!(
                        (mutation, want),
                        (Mutation::Start, Mutation::Start)
                            | (Mutation::Stop, Mutation::Stop)
                            | (Mutation::Restart, Mutation::Restart)
                            | (Mutation::Reload, Mutation::Reload)
                            | (Mutation::Enable { .. }, Mutation::Enable { .. })
                            | (Mutation::Disable { .. }, Mutation::Disable { .. })
                    ));
                    assert_eq!(unit, "u");
                }
                Plan::Read(_) => panic!("mutation classified as read"),
            }
        }
    }
}
