//! RPM package management capability.
//!
//! Keeps CLI planning and backend fallback in one small module. dnf5daemon is
//! the primary backend; PackageKit is the degraded fallback when dnf5daemon is
//! absent.
mod dnf5;
mod domain;
mod packagekit;

use crate::capabilities::{render, View};
use crate::cli::{Cli, PackagesAction};
use crate::error::{FezError, Result};
use crate::protocol::client::BridgeClient;

/// A staging mutation that goes through resolve-first/guardrail/execute.
#[derive(Clone, Copy)]
enum Mutation {
    Install,
    Remove,
    Upgrade,
}

impl Mutation {
    fn verb(self) -> &'static str {
        match self {
            Mutation::Install => "install",
            Mutation::Remove => "remove",
            Mutation::Upgrade => "upgrade",
        }
    }
    /// The dnf5daemon `Rpm` D-Bus method name to stage this mutation.
    ///
    /// Intentionally distinct from [`Mutation::verb`] (the user-facing display
    /// verb); the two happen to coincide today but answer different questions.
    fn method(self) -> &'static str {
        match self {
            Mutation::Install => "install",
            Mutation::Remove => "remove",
            Mutation::Upgrade => "upgrade",
        }
    }
}

/// A read subcommand and its arguments, borrowed from the parsed action.
///
/// Splitting reads out of [`PackagesAction`] keeps dnf5 and PackageKit dispatch
/// total: every variant here maps to a handler.
#[derive(Clone, Copy)]
enum ReadAction<'a> {
    List(ListFilters<'a>),
    Info { spec: &'a str },
    Search { pattern: &'a str },
    CheckUpdate,
    Repolist { filter: RepoFilter },
}

/// Client-side filters and pagination for `packages list`.
#[derive(Clone, Copy)]
struct ListFilters<'a> {
    available: bool,
    repos: &'a [String],
    name: Option<&'a str>,
    limit: Option<usize>,
    offset: usize,
}

/// Which repositories `repolist` should report.
#[derive(Clone, Copy)]
enum RepoFilter {
    Enabled,
    Disabled,
    All,
}

impl RepoFilter {
    /// The dnf5daemon `enable_disable` option value.
    fn enable_disable(self) -> &'static str {
        match self {
            RepoFilter::Enabled => "enabled",
            RepoFilter::Disabled => "disabled",
            RepoFilter::All => "all",
        }
    }
    /// Whether a repo with `enabled` state should appear under this filter.
    fn accepts(self, enabled: bool) -> bool {
        match self {
            RepoFilter::Enabled => enabled,
            RepoFilter::Disabled => !enabled,
            RepoFilter::All => true,
        }
    }
}

/// The read/mutate split of a parsed [`PackagesAction`].
enum Plan<'a> {
    Read(ReadAction<'a>),
    Mutate {
        mutation: Mutation,
        specs: Vec<String>,
    },
}

/// Map the flat clap enum onto the read/mutate [`Plan`] split.
///
/// This is the only exhaustive match over [`PackagesAction`]; everything
/// downstream consumes one arm of this and is therefore total, so a new variant
/// breaks the build here instead of hitting an `unreachable!` at runtime.
fn classify(action: &PackagesAction) -> Plan<'_> {
    match action {
        PackagesAction::List {
            installed: _installed,
            available,
            repo,
            name,
            limit,
            offset,
        } => Plan::Read(ReadAction::List(ListFilters {
            available: *available,
            repos: repo,
            name: name.as_deref(),
            limit: *limit,
            offset: *offset,
        })),
        PackagesAction::Info { spec } => Plan::Read(ReadAction::Info { spec }),
        PackagesAction::Search { pattern } => Plan::Read(ReadAction::Search { pattern }),
        PackagesAction::CheckUpdate => Plan::Read(ReadAction::CheckUpdate),
        PackagesAction::Repolist {
            enabled: _enabled,
            disabled,
            all,
        } => {
            let filter = if *all {
                RepoFilter::All
            } else if *disabled {
                RepoFilter::Disabled
            } else {
                RepoFilter::Enabled
            };
            Plan::Read(ReadAction::Repolist { filter })
        }
        PackagesAction::Install { specs } => Plan::Mutate {
            mutation: Mutation::Install,
            specs: specs.clone(),
        },
        PackagesAction::Remove { specs } => Plan::Mutate {
            mutation: Mutation::Remove,
            specs: specs.clone(),
        },
        PackagesAction::Upgrade { specs } => Plan::Mutate {
            mutation: Mutation::Upgrade,
            specs: specs.clone(),
        },
    }
}

/// Run the requested `packages` subcommand and return the process exit code.
pub fn dispatch(cli: &Cli, action: &PackagesAction) -> i32 {
    let view = match classify(action) {
        Plan::Read(read) => run_read(cli, read),
        Plan::Mutate { mutation, specs } => run_mutation(cli, mutation, &specs),
    };
    render(cli, view)
}

fn run_read(cli: &Cli, action: ReadAction<'_>) -> Result<View> {
    let mut client = crate::capabilities::connect(cli)?;
    let host = client.host().to_string();
    let result = dnf5::run_read(&mut client, &host, action);
    if matches!(result, Err(FezError::DependencyMissing { .. })) {
        read_via_packagekit(&mut client, host, action)
    } else {
        result
    }
}

fn run_mutation(cli: &Cli, mutation: Mutation, specs: &[String]) -> Result<View> {
    let host = cli.resolved_host();
    let mut client = crate::capabilities::connect(cli)?;
    match dnf5::run_mutation(cli, &mut client, mutation, specs, &host) {
        Ok(view) => Ok(view),
        Err(FezError::DependencyMissing { .. }) => {
            mutate_via_packagekit(&mut client, mutation, specs, &host, cli.dry_run, cli.force)
        }
        Err(err) => Err(err),
    }
}

/// Convert a PackageKit backend result into a dnf-backend [`View`].
fn from_pk(pk: packagekit::PkView, host: String) -> View {
    View::new(pk.kind, host, pk.data, pk.human).with_hints_opt(pk.hints)
}

/// Dependency-missing error when BOTH dnf5daemon and PackageKit are absent.
fn both_missing() -> FezError {
    FezError::DependencyMissing {
        component: "dnf5daemon or PackageKit".into(),
        dbus_name: "org.rpm.dnf.v0 / org.freedesktop.PackageKit".into(),
        remediation: "Install a package backend: dnf5daemon-server (Fedora) providing org.rpm.dnf.v0, or PackageKit providing org.freedesktop.PackageKit, then retry.".into(),
    }
}

/// Run a read over the PackageKit backend, mapping PackageKit's own absence to a
/// dependency-missing error naming both daemons.
fn read_via_packagekit(
    client: &mut BridgeClient,
    host: String,
    action: ReadAction<'_>,
) -> Result<View> {
    let result = match action {
        ReadAction::List(filters) => packagekit::list(
            client,
            filters.available,
            filters.repos,
            filters.name,
            filters.limit,
            filters.offset,
        ),
        ReadAction::Info { spec } => packagekit::info(client, spec),
        ReadAction::Search { pattern } => packagekit::search(client, pattern),
        ReadAction::CheckUpdate => packagekit::check_update(client),
        ReadAction::Repolist { filter } => {
            packagekit::repolist(client, move |enabled| filter.accepts(enabled))
        }
    };
    let view = crate::capabilities::map_service_unknown(result, both_missing)?;
    Ok(from_pk(view, host))
}

/// Run a mutation over the PackageKit backend, mapping PackageKit's own absence
/// to a dependency-missing error naming both daemons.
fn mutate_via_packagekit(
    client: &mut BridgeClient,
    mutation: Mutation,
    specs: &[String],
    host: &str,
    dry_run: bool,
    force: bool,
) -> Result<View> {
    let result = packagekit::mutate(client, mutation.verb(), specs, host, dry_run, force);
    let view = crate::capabilities::map_service_unknown(result, both_missing)?;
    Ok(from_pk(view, host.to_string()))
}

/// Envelope `kind` for a package plan: a dry run previews, a real run mutates.
///
/// Shared by both backends so the discriminant cannot drift.
pub(crate) fn plan_kind(dry_run: bool) -> &'static str {
    if dry_run {
        "PackagePlan"
    } else {
        "PackageMutation"
    }
}

/// The one-line human summary shared by both backends' plan views.
///
/// `counts` is `(install, remove, upgrade, downgrade)`. A dry run reads as a
/// preview ("would install ..."); a real run reads as applied ("installed
/// ...").
pub(crate) fn plan_human(
    verb: &str,
    specs: &[String],
    host: &str,
    counts: (usize, usize, usize, usize),
    dry_run: bool,
) -> String {
    let (install, remove, upgrade, downgrade) = counts;
    let specs = specs.join(" ");
    if dry_run {
        format!(
            "DRY-RUN: {verb} {specs} on {host} would install {install}, remove {remove}, upgrade {upgrade}, downgrade {downgrade} package(s)\n"
        )
    } else {
        format!(
            "{verb} {specs} on {host}: installed {install}, removed {remove}, upgraded {upgrade}, downgraded {downgrade} package(s)\n"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_filter_maps_to_backend_and_predicate() {
        assert_eq!(RepoFilter::Enabled.enable_disable(), "enabled");
        assert_eq!(RepoFilter::Disabled.enable_disable(), "disabled");
        assert_eq!(RepoFilter::All.enable_disable(), "all");
        assert!(RepoFilter::Enabled.accepts(true));
        assert!(!RepoFilter::Enabled.accepts(false));
        assert!(!RepoFilter::Disabled.accepts(true));
        assert!(RepoFilter::Disabled.accepts(false));
        assert!(RepoFilter::All.accepts(true));
        assert!(RepoFilter::All.accepts(false));
    }

    #[test]
    fn classify_repolist_prefers_all_then_disabled_then_enabled() {
        let all = PackagesAction::Repolist {
            enabled: true,
            disabled: true,
            all: true,
        };
        let disabled = PackagesAction::Repolist {
            enabled: false,
            disabled: true,
            all: false,
        };
        let enabled = PackagesAction::Repolist {
            enabled: true,
            disabled: false,
            all: false,
        };

        assert!(matches!(
            classify(&all),
            Plan::Read(ReadAction::Repolist {
                filter: RepoFilter::All
            })
        ));
        assert!(matches!(
            classify(&disabled),
            Plan::Read(ReadAction::Repolist {
                filter: RepoFilter::Disabled
            })
        ));
        assert!(matches!(
            classify(&enabled),
            Plan::Read(ReadAction::Repolist {
                filter: RepoFilter::Enabled
            })
        ));
    }
}
