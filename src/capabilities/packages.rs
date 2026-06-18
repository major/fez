//! RPM package management over dnf5daemon (`org.rpm.dnf.v0`).
//!
//! Mirrors the structure of [`crate::capabilities::services`]: a single
//! [`classify`] splits the flat clap enum into reads and mutations, reads run
//! unprivileged and render a [`View`], mutations resolve the transaction first,
//! apply removal guardrails, audit, then execute. Every dnf5daemon session is
//! closed best-effort on every return path so the daemon never leaks sessions.
use crate::capabilities::{render, CapabilityContext, View};
use crate::cli::{Cli, PackagesAction};
use crate::error::{FezError, Result};
use crate::protocol::client::BridgeClient;
use serde_json::{json, Value};

const DNF_NAME: &str = "org.rpm.dnf.v0";
const SM_PATH: &str = "/org/rpm/dnf/v0";
const SM_IFACE: &str = "org.rpm.dnf.v0.SessionManager";
const RPM_IFACE: &str = "org.rpm.dnf.v0.rpm.Rpm";
const REPO_IFACE: &str = "org.rpm.dnf.v0.rpm.Repo";
const GOAL_IFACE: &str = "org.rpm.dnf.v0.Goal";

/// Package attributes requested from dnf5daemon's `Rpm.list`.
const PKG_ATTRS: &[&str] = &["name", "evr", "arch", "repo_id", "install_size", "summary"];

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
/// Splitting reads out of [`PackagesAction`] keeps [`run_read`] total: every
/// variant here maps to a handler, so adding one is a compile error rather than
/// a runtime panic.
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

/// The [`FezError::DependencyMissing`] returned when dnf5daemon is absent.
fn dependency_missing() -> FezError {
    FezError::DependencyMissing {
        component: "dnf5daemon".into(),
        dbus_name: DNF_NAME.into(),
        remediation: "Install the dnf5daemon server on the target (dnf install dnf5daemon-server) and ensure its D-Bus service org.rpm.dnf.v0 is activatable, then retry.".into(),
    }
}

/// Wrap a value as a cockpit D-Bus variant (`{"t": signature, "v": value}`).
///
/// dnf5daemon's option arguments are typed `a{sv}` (a string-keyed dict of
/// variants). cockpit-bridge marshals each dict value as a variant, which on
/// the wire is an explicit `{"t","v"}` object: a bare JSON scalar makes the
/// bridge's marshaller raise `'bool' object is not subscriptable` (or the
/// equivalent for other types) when it tries to read `value["t"]`.
fn variant(signature: &str, value: Value) -> Value {
    json!({ "t": signature, "v": value })
}

/// Build an `a{sv}` options dict, variant-wrapping every value.
///
/// Each entry is `(key, dbus_signature, value)`; the value is wrapped via
/// [`variant`] so the bridge can marshal the dict without introspection.
fn options(entries: &[(&str, &str, Value)]) -> Value {
    let map: serde_json::Map<String, Value> = entries
        .iter()
        .map(|(k, sig, v)| ((*k).to_string(), variant(sig, v.clone())))
        .collect();
    Value::Object(map)
}

/// Open the dnf channel (privileged for mutations) and start a daemon session.
///
/// Returns `(channel, session_path)`; a ServiceUnknown D-Bus error (the daemon
/// is not installed/activatable) is mapped to [`dependency_missing`].
fn open_session(client: &mut BridgeClient, privileged: bool) -> Result<(String, String)> {
    let channel = if privileged {
        client.dbus_open_privileged(DNF_NAME)?
    } else {
        client.dbus_open(DNF_NAME)?
    };
    let out = client.dbus_call(
        &channel,
        SM_PATH,
        SM_IFACE,
        "open_session",
        json!([options(&[
            ("load_system_repo", "b", json!(true)),
            ("load_available_repos", "b", json!(true)),
        ])]),
    );
    let out = crate::capabilities::map_service_unknown(out, dependency_missing)?;
    let session = out.get(0).and_then(Value::as_str).unwrap_or("").to_string();
    Ok((channel, session))
}

/// Close a dnf5daemon session, ignoring any error (best-effort cleanup).
fn close_session(client: &mut BridgeClient, channel: &str, session: &str) {
    let _ = client.dbus_call(
        channel,
        SM_PATH,
        SM_IFACE,
        "close_session",
        json!([session]),
    );
}

/// Pull a variant-wrapped (`{"t":..,"v":..}`) or flat string field as a `String`.
fn sv(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(|f| f.get("v").unwrap_or(f).as_str())
        .unwrap_or("")
        .to_string()
}

/// Pull a variant-wrapped (`{"t":"t","v":..}`) numeric field as a `u64`,
/// tolerating either a JSON number or a numeric string payload.
fn sv_u64(v: &Value, key: &str) -> u64 {
    let field = v.get(key).map(|f| f.get("v").unwrap_or(f));
    match field {
        Some(f) if f.is_u64() => f.as_u64().unwrap_or(0),
        Some(f) if f.is_i64() => u64::try_from(f.as_i64().unwrap_or(0)).unwrap_or(0),
        Some(f) => f.as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
        None => 0,
    }
}

/// Pull a variant-wrapped (`{"t":"b","v":..}`) or flat boolean field.
fn sv_bool(v: &Value, key: &str) -> bool {
    v.get(key)
        .and_then(|f| f.get("v").unwrap_or(f).as_bool())
        .unwrap_or(false)
}

/// A package record parsed from dnf5daemon's variant-wrapped package object.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PackageRecord {
    name: String,
    evr: String,
    arch: String,
    repo_id: String,
    install_size: u64,
    summary: String,
}

impl PackageRecord {
    fn from_value(v: &Value) -> Self {
        Self {
            name: sv(v, "name"),
            evr: sv(v, "evr"),
            arch: sv(v, "arch"),
            repo_id: sv(v, "repo_id"),
            install_size: sv_u64(v, "install_size"),
            summary: sv(v, "summary"),
        }
    }

    fn object(&self) -> Value {
        json!({
            "name": self.name,
            "evr": self.evr,
            "arch": self.arch,
            "repo_id": self.repo_id,
            "install_size": self.install_size,
            "summary": self.summary,
        })
    }

    fn row(&self) -> Value {
        json!([
            self.name,
            self.evr,
            self.arch,
            self.repo_id,
            self.install_size,
            self.summary,
        ])
    }

    #[cfg(test)]
    fn nevra(&self) -> String {
        format!("{}-{}.{}", self.name, self.evr, self.arch)
    }
}

/// Column order for the columnar `PackageList`/`PackageSearch` payloads.
const PKG_COLUMNS: &[&str] = &["name", "evr", "arch", "repo_id", "install_size", "summary"];

/// Connect, open an unprivileged session, dispatch the read, and always close.
///
/// When dnf5daemon is absent (the `open_session` ServiceUnknown path, surfaced
/// as [`FezError::DependencyMissing`]), the read transparently falls back to the
/// PackageKit backend (RHEL 10). Only when PackageKit is *also* absent does the
/// call return a dependency-missing error naming both daemons.
fn run_read(cli: &Cli, action: ReadAction<'_>) -> Result<View> {
    let mut client = crate::capabilities::connect(cli)?;
    let host = client.host().to_string();
    let (channel, session) = match open_session(&mut client, false) {
        Ok(pair) => pair,
        Err(FezError::DependencyMissing { .. }) => {
            return read_via_packagekit(&mut client, host, action);
        }
        Err(e) => return Err(e),
    };
    let mut ctx = CapabilityContext {
        client: &mut client,
        channel: &channel,
        host: &host,
    };
    let result = match action {
        ReadAction::List(filters) => list(&mut ctx, &session, filters),
        ReadAction::Info { spec } => info(&mut ctx, &session, spec),
        ReadAction::Search { pattern } => search(&mut ctx, &session, pattern),
        ReadAction::CheckUpdate => check_update(&mut ctx, &session),
        ReadAction::Repolist { filter } => repolist(&mut ctx, &session, filter),
    };
    close_session(ctx.client, ctx.channel, &session);
    result
}

/// Convert a PackageKit backend result into a dnf-backend [`View`].
fn from_pk(pk: crate::capabilities::packages_pk::PkView, host: String) -> View {
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
    use crate::capabilities::packages_pk as pk;
    let result = match action {
        ReadAction::List(filters) => pk::list(
            client,
            filters.available,
            filters.repos,
            filters.name,
            filters.limit,
            filters.offset,
        ),
        ReadAction::Info { spec } => pk::info(client, spec),
        ReadAction::Search { pattern } => pk::search(client, pattern),
        ReadAction::CheckUpdate => pk::check_update(client),
        ReadAction::Repolist { filter } => {
            pk::repolist(client, move |enabled| filter.accepts(enabled))
        }
    };
    let view = crate::capabilities::map_service_unknown(result, both_missing)?;
    Ok(from_pk(view, host))
}

/// Call `Rpm.list` on the session with the given scope/patterns and return the
/// parsed package array.
fn rpm_list(
    client: &mut BridgeClient,
    channel: &str,
    session: &str,
    scope: &str,
    patterns: &[String],
) -> Result<Vec<PackageRecord>> {
    let out = client.dbus_call(
        channel,
        session,
        RPM_IFACE,
        "list",
        json!([options(&[
            ("scope", "s", json!(scope)),
            ("patterns", "as", json!(patterns)),
            ("package_attrs", "as", json!(PKG_ATTRS)),
        ])]),
    )?;
    Ok(out
        .get(0)
        .and_then(Value::as_array)
        .map(|items| items.iter().map(PackageRecord::from_value).collect())
        .unwrap_or_default())
}

fn list(ctx: &mut CapabilityContext<'_>, session: &str, filters: ListFilters<'_>) -> Result<View> {
    let scope = if filters.available {
        "available"
    } else {
        "installed"
    };
    let packages = rpm_list(ctx.client, ctx.channel, session, scope, &[])?;
    // dnf5daemon's Rpm.list has no server-side repo filter (only install/upgrade
    // accept `repo_ids`, for resolution), so we filter client-side on the exact
    // `repo_id`. Multiple --repo flags union: a row is kept if its repo id is in
    // the requested set. An empty set means no filter (issue #59).
    let filtered: Vec<&PackageRecord> = packages
        .iter()
        .filter(|p| filters.repos.is_empty() || filters.repos.iter().any(|r| r == &p.repo_id))
        .filter(|p| filters.name.is_none_or(|pattern| p.name.contains(pattern)))
        .collect();
    let total = filtered.len();
    let start = filters.offset.min(total);
    let end = match filters.limit {
        Some(limit) => (start + limit).min(total),
        None => total,
    };
    let page = &filtered[start..end];
    let mut human = format!(
        "{:<24} {:<20} {:<10} {}\n",
        "NAME", "VERSION", "ARCH", "REPO"
    );
    for p in page {
        human.push_str(&format!(
            "{:<24} {:<20} {:<10} {}\n",
            p.name, p.evr, p.arch, p.repo_id,
        ));
    }
    let rows: Vec<Value> = page.iter().map(|p| p.row()).collect();
    let mut data = crate::envelope::table_data(PKG_COLUMNS, rows);
    data["scope"] = json!(scope);
    // Echo the requested repo filter so callers can confirm what was applied.
    data["repos"] = json!(filters.repos);
    data["name"] = json!(filters.name);
    data["total"] = json!(total);
    data["returned"] = json!(end - start);
    data["limit"] = json!(filters.limit);
    data["offset"] = json!(filters.offset);
    data["next_offset"] = json!((end < total).then_some(end));
    data["backend"] = json!("dnf5daemon");
    let hints = if filters.limit.is_none() && total > 1000 {
        Some(json!([format!(
            "This response has {total} rows. Prefer packages search <pattern>, use --name, or use --limit."
        )]))
    } else {
        None
    };
    Ok(View::new("PackageList", ctx.host, data, human).with_hints_opt(hints))
}

fn info(ctx: &mut CapabilityContext<'_>, session: &str, spec: &str) -> Result<View> {
    let packages = rpm_list(ctx.client, ctx.channel, session, "all", &[spec.to_string()])?;
    let first = packages
        .first()
        .ok_or_else(|| FezError::NotFound(spec.to_string()))?;
    let mut pkg = first.object();
    pkg["backend"] = json!("dnf5daemon");
    let human = format!(
        "Name        : {}\nVersion     : {}\nArch        : {}\nRepo        : {}\nInstall size: {}\nSummary     : {}\n",
        first.name,
        first.evr,
        first.arch,
        first.repo_id,
        first.install_size,
        first.summary,
    );
    Ok(View::new("PackageInfo", ctx.host, pkg, human))
}

fn search(ctx: &mut CapabilityContext<'_>, session: &str, pattern: &str) -> Result<View> {
    let glob = format!("*{pattern}*");
    let packages = rpm_list(ctx.client, ctx.channel, session, "available", &[glob])?;
    let mut human = String::new();
    for p in &packages {
        human.push_str(&format!("{} - {}\n", p.name, p.summary));
    }
    let rows: Vec<Value> = packages.iter().map(PackageRecord::row).collect();
    let mut data = crate::envelope::table_data(PKG_COLUMNS, rows);
    data["pattern"] = json!(pattern);
    data["backend"] = json!("dnf5daemon");
    Ok(View::new("PackageSearch", ctx.host, data, human))
}

fn check_update(ctx: &mut CapabilityContext<'_>, session: &str) -> Result<View> {
    let packages = rpm_list(ctx.client, ctx.channel, session, "upgrades", &[])?;
    let mut human = format!("{:<24} {:<20} {}\n", "NAME", "VERSION", "REPO");
    for p in &packages {
        human.push_str(&format!("{:<24} {:<20} {}\n", p.name, p.evr, p.repo_id,));
    }
    let rows: Vec<Value> = packages.iter().map(PackageRecord::row).collect();
    let mut data = crate::envelope::table_data(PKG_COLUMNS, rows);
    data["backend"] = json!("dnf5daemon");
    Ok(View::new("PackageUpdates", ctx.host, data, human))
}

/// Column order for the columnar `RepoList` payload (`enabled` stays a bool).
const REPO_COLUMNS: &[&str] = &["id", "name", "enabled"];

#[derive(Debug, Clone, PartialEq, Eq)]
struct RepoRecord {
    id: String,
    name: String,
    enabled: bool,
}

impl RepoRecord {
    fn from_value(v: &Value) -> Self {
        Self {
            id: sv(v, "id"),
            name: sv(v, "name"),
            enabled: sv_bool(v, "enabled"),
        }
    }

    fn row(&self) -> Value {
        json!([self.id, self.name, self.enabled])
    }
}

fn repolist(ctx: &mut CapabilityContext<'_>, session: &str, filter: RepoFilter) -> Result<View> {
    let out = ctx.client.dbus_call(
        ctx.channel,
        session,
        REPO_IFACE,
        "list",
        json!([options(&[
            ("enable_disable", "s", json!(filter.enable_disable())),
            ("repo_attrs", "as", json!(["id", "name", "enabled"])),
        ])]),
    )?;
    let raw: Vec<RepoRecord> = out
        .get(0)
        .and_then(Value::as_array)
        .map(|items| items.iter().map(RepoRecord::from_value).collect())
        .unwrap_or_default();
    let mut rows = Vec::new();
    let mut human = format!("{:<24} {:<10} {}\n", "REPO ID", "ENABLED", "NAME");
    for r in &raw {
        if !filter.accepts(r.enabled) {
            continue;
        }
        human.push_str(&format!("{:<24} {:<10} {}\n", r.id, r.enabled, r.name));
        rows.push(r.row());
    }
    let mut data = crate::envelope::table_data(REPO_COLUMNS, rows);
    data["backend"] = json!("dnf5daemon");
    Ok(View::new("RepoList", ctx.host, data, human))
}

/// A resolved dnf5daemon transaction, bucketed by action for rendering and
/// guardrails.
struct ResolvedPlan {
    install: Vec<String>,
    remove: Vec<String>,
    upgrade: Vec<String>,
    downgrade: Vec<String>,
    install_size_total: u64,
    remove_names: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransactionAction {
    Install,
    Remove,
    Upgrade,
    Downgrade,
    Replaced,
    ReplacedBy,
    Erase,
    Obsoleted,
}

impl TransactionAction {
    fn from_str(action: &str) -> Option<Self> {
        match action {
            "Install" => Some(Self::Install),
            "Remove" => Some(Self::Remove),
            "Upgrade" => Some(Self::Upgrade),
            "Downgrade" => Some(Self::Downgrade),
            "Replaced" => Some(Self::Replaced),
            "ReplacedBy" => Some(Self::ReplacedBy),
            "Erase" => Some(Self::Erase),
            "Obsoleted" => Some(Self::Obsoleted),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TransactionPackage {
    name: String,
    evr: String,
    arch: String,
    install_size: u64,
}

impl TransactionPackage {
    fn from_value(v: &Value) -> Option<Self> {
        let name = sv(v, "name");
        let evr = sv(v, "evr");
        let arch = sv(v, "arch");
        if name.is_empty() || evr.is_empty() || arch.is_empty() {
            return None;
        }
        Some(Self {
            name,
            evr,
            arch,
            install_size: sv_u64(v, "install_size"),
        })
    }

    fn label(&self) -> String {
        format!("{}-{}.{}", self.name, self.evr, self.arch)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TransactionItem {
    action: TransactionAction,
    package: TransactionPackage,
}

impl TransactionItem {
    fn from_value(item: &Value) -> Option<Self> {
        let action = item
            .get(1)
            .and_then(Value::as_str)
            .and_then(TransactionAction::from_str)?;
        let package = item.get(4).and_then(TransactionPackage::from_value)?;
        Some(Self { action, package })
    }
}

impl ResolvedPlan {
    /// The plan rendered as the `fez/v1` data payload.
    fn data(&self) -> Value {
        json!({
            "install": self.install,
            "remove": self.remove,
            "upgrade": self.upgrade,
            "downgrade": self.downgrade,
            "install_size_total": self.install_size_total,
            "counts": {
                "install": self.install.len(),
                "remove": self.remove.len(),
                "upgrade": self.upgrade.len(),
                "downgrade": self.downgrade.len(),
            },
        })
    }
}

/// Parse a `Goal.resolve` `transaction_items` array into a [`ResolvedPlan`].
///
/// Each item is a 5-element array `[object_type, action, reason, attrs, object]`;
/// `action` is at index 1, `object` at index 4. Removal-class actions push the
/// bare name into `remove_names` (guardrail input) and the label into `remove`.
fn parse_plan(items: &Value) -> ResolvedPlan {
    let mut plan = ResolvedPlan {
        install: Vec::new(),
        remove: Vec::new(),
        upgrade: Vec::new(),
        downgrade: Vec::new(),
        install_size_total: 0,
        remove_names: Vec::new(),
    };
    let Some(arr) = items.as_array() else {
        return plan;
    };
    for item in arr.iter().filter_map(TransactionItem::from_value) {
        let label = item.package.label();
        match item.action {
            TransactionAction::Install => {
                plan.install_size_total += item.package.install_size;
                plan.install.push(label);
            }
            TransactionAction::Remove
            | TransactionAction::Replaced
            | TransactionAction::Erase
            | TransactionAction::Obsoleted => {
                plan.remove_names.push(item.package.name);
                plan.remove.push(label);
            }
            TransactionAction::Upgrade => plan.upgrade.push(label),
            TransactionAction::Downgrade => plan.downgrade.push(label),
            TransactionAction::ReplacedBy => {}
        }
    }
    plan
}

/// Resolve-first mutation: stage, resolve, optionally short-circuit on dry-run,
/// apply removal guardrails, audit, execute, and always close the session.
fn run_mutation(cli: &Cli, m: Mutation, specs: &[String]) -> Result<View> {
    let host = cli.resolved_host();
    let mut client = crate::capabilities::connect(cli)?;
    let (channel, session) = match open_session(&mut client, true) {
        Ok(pair) => pair,
        Err(FezError::DependencyMissing { .. }) => {
            return mutate_via_packagekit(&mut client, m, specs, &host, cli.dry_run, cli.force);
        }
        Err(e) => return Err(e),
    };
    // Do the work in an inner closure so the session is closed on every path,
    // success or failure, before the result propagates.
    let mut ctx = CapabilityContext {
        client: &mut client,
        channel: &channel,
        host: &host,
    };
    let result = mutation_inner(cli, &mut ctx, &session, m, specs);
    close_session(ctx.client, ctx.channel, &session);
    result
}

/// Run a mutation over the PackageKit backend, mapping PackageKit's own absence
/// to a dependency-missing error naming both daemons.
fn mutate_via_packagekit(
    client: &mut BridgeClient,
    m: Mutation,
    specs: &[String],
    host: &str,
    dry_run: bool,
    force: bool,
) -> Result<View> {
    let result =
        crate::capabilities::packages_pk::mutate(client, m.verb(), specs, host, dry_run, force);
    let view = crate::capabilities::map_service_unknown(result, both_missing)?;
    Ok(from_pk(view, host.to_string()))
}

fn mutation_inner(
    cli: &Cli,
    ctx: &mut CapabilityContext<'_>,
    session: &str,
    m: Mutation,
    specs: &[String],
) -> Result<View> {
    // 1. Stage the goal.
    ctx.client.dbus_call(
        ctx.channel,
        session,
        RPM_IFACE,
        m.method(),
        json!([specs, {}]),
    )?;
    // 2. Resolve into a concrete plan.
    let out = ctx
        .client
        .dbus_call(ctx.channel, session, GOAL_IFACE, "resolve", json!([{}]))?;
    let items = out.get(0).cloned().unwrap_or(Value::Null);
    let plan = parse_plan(&items);

    // 3. Dry-run: report the plan without executing.
    if cli.dry_run {
        return Ok(plan_view(m, ctx.host, specs, &plan, true));
    }

    // 4. Removal guardrails (protected package / cascade) before any execution.
    crate::safety::check_removal_plan(&plan.remove_names, cli.force)?;

    // 5. Audit attempt, execute, audit result.
    crate::audit::run_audited(ctx.host, m.verb(), &specs.join(","), || {
        ctx.client.dbus_call(
            ctx.channel,
            session,
            GOAL_IFACE,
            "do_transaction",
            json!([{}]),
        )?;
        Ok(plan_view(m, ctx.host, specs, &plan, false))
    })
}

/// Build the [`View`] for a resolved plan (dry-run preview or executed mutation).
/// Envelope `kind` for a package plan: a dry run previews, a real run mutates.
///
/// Shared by both backends ([`plan_view`] and
/// [`crate::capabilities::packages_pk`]) so the discriminant cannot drift.
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

fn plan_view(
    m: Mutation,
    host: &str,
    specs: &[String],
    plan: &ResolvedPlan,
    dry_run: bool,
) -> View {
    let mut data = plan.data();
    if let Value::Object(map) = &mut data {
        map.insert("operation".into(), json!(m.verb()));
        map.insert("specs".into(), json!(specs));
        map.insert("dry_run".into(), json!(dry_run));
        map.insert("backend".into(), json!("dnf5daemon"));
    }
    let counts = (
        plan.install.len(),
        plan.remove.len(),
        plan.upgrade.len(),
        plan.downgrade.len(),
    );
    let human = plan_human(m.verb(), specs, host, counts, dry_run);
    View::new(plan_kind(dry_run), host.to_string(), data, human)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(action: &str, name: &str, size: u64) -> Value {
        json!([ "Package", action, "User", {}, {
            "name": {"t":"s","v": name},
            "evr": {"t":"s","v":"1-1"},
            "arch": {"t":"s","v":"x86_64"},
            "repo_id": {"t":"s","v":"fedora"},
            "install_size": {"t":"t","v": size.to_string()}
        }])
    }

    #[test]
    fn variant_wraps_value_with_signature() {
        assert_eq!(variant("b", json!(true)), json!({"t": "b", "v": true}));
        assert_eq!(variant("s", json!("x")), json!({"t": "s", "v": "x"}));
    }

    #[test]
    fn options_wraps_every_value_as_a_variant() {
        let opts = options(&[
            ("load_system_repo", "b", json!(true)),
            ("scope", "s", json!("installed")),
            ("patterns", "as", json!(["htop"])),
        ]);
        // Every value must be the explicit {"t","v"} variant form, never a bare
        // scalar, or cockpit's a{sv} marshaller raises a TypeError.
        assert_eq!(
            opts,
            json!({
                "load_system_repo": {"t": "b", "v": true},
                "scope": {"t": "s", "v": "installed"},
                "patterns": {"t": "as", "v": ["htop"]},
            })
        );
    }

    #[test]
    fn package_record_parses_variant_wrapped_fields() {
        let raw = json!({
            "name": {"t":"s","v":"bash"},
            "evr": {"t":"s","v":"5.2.26-3.fc41"},
            "arch": {"t":"s","v":"x86_64"},
            "repo_id": {"t":"s","v":"fedora"},
            "install_size": {"t":"t","v":12345},
            "summary": {"t":"s","v":"The GNU Bourne Again shell"}
        });

        let record = PackageRecord::from_value(&raw);

        assert_eq!(record.name, "bash");
        assert_eq!(record.evr, "5.2.26-3.fc41");
        assert_eq!(record.arch, "x86_64");
        assert_eq!(record.repo_id, "fedora");
        assert_eq!(record.install_size, 12345);
        assert_eq!(record.summary, "The GNU Bourne Again shell");
        assert_eq!(
            record.row(),
            json!([
                "bash",
                "5.2.26-3.fc41",
                "x86_64",
                "fedora",
                12345,
                "The GNU Bourne Again shell"
            ])
        );
        assert_eq!(
            record.object(),
            json!({
                "name": "bash",
                "evr": "5.2.26-3.fc41",
                "arch": "x86_64",
                "repo_id": "fedora",
                "install_size": 12345,
                "summary": "The GNU Bourne Again shell"
            })
        );
    }

    #[test]
    fn package_record_parses_flat_and_string_size_fields() {
        let raw = json!({
            "name": "vim",
            "evr": "9.1.0-1.fc41",
            "arch": "x86_64",
            "repo_id": "updates",
            "install_size": "456",
            "summary": "Editor"
        });

        let record = PackageRecord::from_value(&raw);

        assert_eq!(record.install_size, 456);
        assert_eq!(record.nevra(), "vim-9.1.0-1.fc41.x86_64");
    }

    #[test]
    fn repo_record_parses_variant_wrapped_fields() {
        let raw = json!({
            "id": {"t":"s","v":"fedora"},
            "name": {"t":"s","v":"Fedora Everything"},
            "enabled": {"t":"b","v":true}
        });

        let repo = RepoRecord::from_value(&raw);

        assert_eq!(repo.id, "fedora");
        assert_eq!(repo.name, "Fedora Everything");
        assert!(repo.enabled);
        assert_eq!(repo.row(), json!(["fedora", "Fedora Everything", true]));
    }

    #[test]
    fn package_record_filters_use_typed_fields() {
        let records = [
            PackageRecord {
                name: "bash".into(),
                evr: "5.2.26-3.fc41".into(),
                arch: "x86_64".into(),
                repo_id: "fedora".into(),
                install_size: 1,
                summary: "Shell".into(),
            },
            PackageRecord {
                name: "vim".into(),
                evr: "9.1.0-1.fc41".into(),
                arch: "x86_64".into(),
                repo_id: "updates".into(),
                install_size: 2,
                summary: "Editor".into(),
            },
        ];

        let repos = ["updates".to_string()];
        let filtered: Vec<&PackageRecord> = records
            .iter()
            .filter(|p| repos.is_empty() || repos.iter().any(|r| r == &p.repo_id))
            .filter(|p| Some("vi").is_none_or(|pattern| p.name.contains(pattern)))
            .collect();

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "vim");
    }

    #[test]
    fn transaction_item_parses_install_action_and_package() {
        let item = json!(["package", "Install", "user", {}, {
            "name": {"t":"s","v":"nginx"},
            "evr": {"t":"s","v":"1.26.2-1.fc41"},
            "arch": {"t":"s","v":"x86_64"},
            "install_size": {"t":"t","v":2048}
        }]);

        let parsed = TransactionItem::from_value(&item).expect("valid item");

        assert_eq!(parsed.action, TransactionAction::Install);
        assert_eq!(parsed.package.name, "nginx");
        assert_eq!(parsed.package.label(), "nginx-1.26.2-1.fc41.x86_64");
        assert_eq!(parsed.package.install_size, 2048);
    }

    #[test]
    fn transaction_item_rejects_unknown_or_malformed_items() {
        assert!(
            TransactionItem::from_value(&json!(["package", "Unknown", "user", {}, {}])).is_none()
        );
        assert!(TransactionItem::from_value(&json!({"not":"an array"})).is_none());
    }

    #[test]
    fn transaction_item_rejects_malformed_package_object() {
        assert!(
            TransactionItem::from_value(&json!(["package", "Install", "user", {}, null])).is_none()
        );
        assert!(
            TransactionItem::from_value(&json!(["package", "Install", "user", {}, {}])).is_none()
        );
        assert!(TransactionItem::from_value(&json!(["package", "Install", "user", {}])).is_none());
    }

    #[test]
    fn parse_plan_buckets_by_action() {
        let items = json!([
            item("Install", "htop", 100),
            item("Remove", "oldpkg", 50),
            item("Upgrade", "nginx", 200),
            item("Downgrade", "foo", 10)
        ]);
        let plan = parse_plan(&items);
        assert_eq!(plan.install, vec!["htop-1-1.x86_64"]);
        assert_eq!(plan.remove, vec!["oldpkg-1-1.x86_64"]);
        assert_eq!(plan.upgrade, vec!["nginx-1-1.x86_64"]);
        assert_eq!(plan.downgrade, vec!["foo-1-1.x86_64"]);
        assert_eq!(plan.remove_names, vec!["oldpkg".to_string()]);
    }

    #[test]
    fn parse_plan_counts_replaced_as_removed() {
        let items = json!([
            item("Install", "newpkg", 100),
            item("Replaced", "oldpkg", 50)
        ]);
        assert_eq!(parse_plan(&items).remove_names, vec!["oldpkg".to_string()]);
    }

    #[test]
    fn parse_plan_counts_legacy_removal_actions_as_removed() {
        let items = json!([
            item("Obsoleted", "oldpkg", 50),
            item("Erase", "gonepkg", 25)
        ]);

        let plan = parse_plan(&items);

        assert_eq!(plan.remove, vec!["oldpkg-1-1.x86_64", "gonepkg-1-1.x86_64"]);
        assert_eq!(
            plan.remove_names,
            vec!["oldpkg".to_string(), "gonepkg".to_string()]
        );
    }

    #[test]
    fn parse_plan_ignores_replaced_by_to_preserve_existing_contract() {
        let items = json!([item("ReplacedBy", "newpkg", 50)]);

        let plan = parse_plan(&items);

        assert!(plan.upgrade.is_empty());
        assert!(plan.remove.is_empty());
    }

    #[test]
    fn parse_plan_drops_malformed_known_action_items() {
        let items = json!([
            ["package", "Remove", "user", {}, {}],
            ["package", "Obsoleted", "user", {}, null]
        ]);

        let plan = parse_plan(&items);

        assert!(plan.remove.is_empty());
        assert!(plan.remove_names.is_empty());
    }

    #[test]
    fn parse_plan_totals_install_size() {
        let items = json!([item("Install", "a", 100), item("Install", "b", 200)]);
        assert_eq!(parse_plan(&items).install_size_total, 300);
    }
}
