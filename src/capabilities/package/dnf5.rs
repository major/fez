//! dnf5daemon backend (`org.rpm.dnf.v0`).
use super::{domain, plan_human, plan_kind, ListFilters, Mutation, ReadAction, RepoFilter};
use crate::capabilities::{CapabilityContext, View};
use crate::cli::Cli;
use crate::error::{FezError, Result};
use crate::protocol::client::BridgeClient;
use crate::protocol::variant::{Variant, VariantU64};
use serde_json::{json, Value};

const DNF_NAME: &str = "org.rpm.dnf.v0";
const SM_PATH: &str = "/org/rpm/dnf/v0";
const SM_IFACE: &str = "org.rpm.dnf.v0.SessionManager";
const RPM_IFACE: &str = "org.rpm.dnf.v0.rpm.Rpm";
const REPO_IFACE: &str = "org.rpm.dnf.v0.rpm.Repo";
const GOAL_IFACE: &str = "org.rpm.dnf.v0.Goal";

/// Package attributes requested from dnf5daemon's `Rpm.list`.
const PKG_ATTRS: &[&str] = domain::PACKAGE_COLUMNS;

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

/// Validate a package spec before it reaches dnf5daemon.
///
/// Rejects empty, over-long, control-character, or `-`-prefixed specs.
/// dnf5daemon validates the spec syntactically; fez adds a structural
/// guard against injection of arbitrary strings from an agent or script.
fn validate_package_spec(spec: &str) -> Result<()> {
    const MAX_LEN: usize = 512;
    if spec.is_empty() {
        return Err(FezError::Usage("package spec must not be empty".into()));
    }
    if spec.len() > MAX_LEN {
        return Err(FezError::Usage(format!(
            "package spec too long ({} > {MAX_LEN})",
            spec.len()
        )));
    }
    if spec.starts_with('-') {
        return Err(FezError::Usage(format!(
            "package spec must not start with '-': {spec}"
        )));
    }
    for ch in spec.chars() {
        if ch.is_control() {
            return Err(FezError::Usage(format!(
                "package spec contains control character U+{:04X}",
                ch as u32
            )));
        }
    }
    Ok(())
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
    let session = session_path(&out)?;
    Ok((channel, session))
}

fn session_path(out: &Value) -> Result<String> {
    out.get(0)
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| FezError::Dbus {
            name: "org.rpm.dnf.v0.MalformedResponse".into(),
            message: "open_session response did not include a session object path".into(),
        })
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

fn variant_string_field(v: &Value, field: &str) -> Option<String> {
    serde_json::from_value::<Variant<String>>(v.get(field)?.clone())
        .ok()
        .map(Variant::into_inner)
}

fn required_variant_string_field(v: &Value, field: &str) -> Option<String> {
    variant_string_field(v, field).filter(|s| !s.is_empty())
}

fn optional_variant_string_field(v: &Value, field: &str) -> String {
    // Optional display text: dnf5daemon may omit it on sparse records, and the
    // established output contract represents absent text as an empty string.
    variant_string_field(v, field).unwrap_or_default()
}

fn optional_variant_u64_field(v: &Value, field: &str) -> u64 {
    // Optional size: not all dnf5daemon records include size; keep the existing
    // output contract by rendering missing or malformed sizes as zero.
    v.get(field)
        .and_then(|value| serde_json::from_value::<VariantU64>(value.clone()).ok())
        .map(|size| size.0)
        .unwrap_or_default()
}

fn required_variant_bool_field(v: &Value, field: &str) -> Option<bool> {
    serde_json::from_value::<Variant<bool>>(v.get(field)?.clone())
        .ok()
        .map(Variant::into_inner)
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedRecords<T> {
    records: Vec<T>,
    dropped: usize,
}

fn collect_records<T>(items: &[Value], parse: impl Fn(&Value) -> Option<T>) -> ParsedRecords<T> {
    let mut records = Vec::new();
    let mut dropped = 0;
    for item in items {
        match parse(item) {
            Some(record) => records.push(record),
            None => dropped += 1,
        }
    }
    ParsedRecords { records, dropped }
}

fn malformed_records_hint(kind: &str, dropped: usize) -> Option<String> {
    (dropped > 0).then(|| format!("Dropped {dropped} malformed dnf5daemon {kind} record(s)."))
}

impl PackageRecord {
    fn from_value(v: &Value) -> Option<Self> {
        // Required NEVRA fields must be present with the expected JSON value
        // type. `repo_id` is metadata: dnf5daemon can omit it for local or
        // installed packages, so keep the historical empty-string default.
        Some(Self {
            name: required_variant_string_field(v, "name")?,
            evr: required_variant_string_field(v, "evr")?,
            arch: required_variant_string_field(v, "arch")?,
            repo_id: optional_variant_string_field(v, "repo_id"),
            install_size: optional_variant_u64_field(v, "install_size"),
            summary: optional_variant_string_field(v, "summary"),
        })
    }

    #[cfg(test)]
    fn object(&self) -> Value {
        domain::PackageRow::package_object(self)
    }

    #[cfg(test)]
    fn row(&self) -> Value {
        domain::PackageRow::package_row(self)
    }

    #[cfg(test)]
    fn nevra(&self) -> String {
        format!("{}-{}.{}", self.name, self.evr, self.arch)
    }
}

impl domain::PackageRow for PackageRecord {
    fn name(&self) -> &str {
        &self.name
    }
    fn evr(&self) -> &str {
        &self.evr
    }
    fn arch(&self) -> &str {
        &self.arch
    }
    fn repo_id(&self) -> &str {
        &self.repo_id
    }
    fn install_size(&self) -> Value {
        json!(self.install_size)
    }
    fn summary(&self) -> &str {
        &self.summary
    }
}

/// Connect, open an unprivileged session, dispatch the read, and always close.
pub(super) fn run_read(
    client: &mut BridgeClient,
    host: &str,
    action: ReadAction<'_>,
) -> Result<View> {
    let (channel, session) = open_session(client, false)?;
    let mut ctx = CapabilityContext {
        client,
        channel: &channel,
        host,
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

fn rpm_list_records(
    client: &mut BridgeClient,
    channel: &str,
    session: &str,
    scope: &str,
    patterns: &[String],
) -> Result<ParsedRecords<PackageRecord>> {
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
        .map(|items| collect_records(items, PackageRecord::from_value))
        .unwrap_or_else(|| ParsedRecords {
            records: Vec::new(),
            dropped: 0,
        }))
}

fn list(ctx: &mut CapabilityContext<'_>, session: &str, filters: ListFilters<'_>) -> Result<View> {
    let scope = if filters.available {
        "available"
    } else {
        "installed"
    };
    let parsed = rpm_list_records(ctx.client, ctx.channel, session, scope, &[])?;
    let packages = parsed.records;
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
    // Echo the requested repo filter so callers can confirm what was applied.
    let data = domain::package_list_data(
        page.iter().copied(),
        scope,
        filters.repos,
        filters.name,
        total,
        filters.limit,
        filters.offset,
        domain::DNF5_BACKEND,
    );
    let mut hints = Vec::new();
    if filters.limit.is_none() {
        if let Some(hint) = domain::large_result_hint(total) {
            hints.push(hint);
        }
    }
    if let Some(hint) = malformed_records_hint("package", parsed.dropped) {
        hints.push(hint);
    }
    let hints = domain::hints_array(hints);
    Ok(View::new("PackageList", ctx.host, data, human).with_hints_opt(hints))
}

fn info(ctx: &mut CapabilityContext<'_>, session: &str, spec: &str) -> Result<View> {
    let parsed = rpm_list_records(ctx.client, ctx.channel, session, "all", &[spec.to_string()])?;
    let packages = parsed.records;
    let first = packages
        .first()
        .ok_or_else(|| FezError::NotFound(spec.to_string()))?;
    let mut pkg = domain::PackageRow::package_object(first);
    domain::stamp_backend(&mut pkg, domain::DNF5_BACKEND);
    let human = format!(
        "Name        : {}\nVersion     : {}\nArch        : {}\nRepo        : {}\nInstall size: {}\nSummary     : {}\n",
        first.name,
        first.evr,
        first.arch,
        first.repo_id,
        first.install_size,
        first.summary,
    );
    Ok(
        View::new("PackageInfo", ctx.host, pkg, human).with_hints_opt(
            malformed_records_hint("package", parsed.dropped).map(|hint| json!([hint])),
        ),
    )
}

fn search(ctx: &mut CapabilityContext<'_>, session: &str, pattern: &str) -> Result<View> {
    let glob = format!("*{pattern}*");
    let parsed = rpm_list_records(ctx.client, ctx.channel, session, "available", &[glob])?;
    let packages = parsed.records;
    let mut human = String::new();
    for p in &packages {
        human.push_str(&format!("{} - {}\n", p.name, p.summary));
    }
    let data = domain::package_search_data(packages.iter(), pattern, domain::DNF5_BACKEND);
    Ok(
        View::new("PackageSearch", ctx.host, data, human).with_hints_opt(
            malformed_records_hint("package", parsed.dropped).map(|hint| json!([hint])),
        ),
    )
}

fn check_update(ctx: &mut CapabilityContext<'_>, session: &str) -> Result<View> {
    let parsed = rpm_list_records(ctx.client, ctx.channel, session, "upgrades", &[])?;
    let packages = parsed.records;
    let mut human = format!("{:<24} {:<20} {}\n", "NAME", "VERSION", "REPO");
    for p in &packages {
        human.push_str(&format!("{:<24} {:<20} {}\n", p.name, p.evr, p.repo_id,));
    }
    let data = domain::package_table_data(packages.iter(), domain::DNF5_BACKEND);
    Ok(
        View::new("PackageUpdates", ctx.host, data, human).with_hints_opt(
            malformed_records_hint("package", parsed.dropped).map(|hint| json!([hint])),
        ),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RepoRecord {
    id: String,
    name: String,
    enabled: bool,
}

impl RepoRecord {
    fn from_value(v: &Value) -> Option<Self> {
        // Required fields drive filtering and identity; optional display name
        // keeps the established empty-string default when absent or malformed.
        Some(Self {
            id: required_variant_string_field(v, "id")?,
            name: optional_variant_string_field(v, "name"),
            enabled: required_variant_bool_field(v, "enabled")?,
        })
    }

    #[cfg(test)]
    fn row(&self) -> Value {
        domain::RepoRow::repo_row(self)
    }
}

impl domain::RepoRow for RepoRecord {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn enabled(&self) -> bool {
        self.enabled
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
    let parsed = out
        .get(0)
        .and_then(Value::as_array)
        .map(|items| collect_records(items, RepoRecord::from_value))
        .unwrap_or_else(|| ParsedRecords {
            records: Vec::new(),
            dropped: 0,
        });
    let mut shown = Vec::new();
    let mut human = format!("{:<24} {:<10} {}\n", "REPO ID", "ENABLED", "NAME");
    for r in &parsed.records {
        if !filter.accepts(r.enabled) {
            continue;
        }
        human.push_str(&format!("{:<24} {:<10} {}\n", r.id, r.enabled, r.name));
        shown.push(r);
    }
    let data = domain::repo_table_data(shown, domain::DNF5_BACKEND);
    Ok(View::new("RepoList", ctx.host, data, human)
        .with_hints_opt(malformed_records_hint("repo", parsed.dropped).map(|hint| json!([hint]))))
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

impl domain::MutationPlanBuckets for ResolvedPlan {
    fn install(&self) -> &[String] {
        &self.install
    }
    fn remove(&self) -> &[String] {
        &self.remove
    }
    fn upgrade(&self) -> &[String] {
        &self.upgrade
    }
    fn downgrade(&self) -> &[String] {
        &self.downgrade
    }
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
        Some(Self {
            name: required_variant_string_field(v, "name")?,
            evr: required_variant_string_field(v, "evr")?,
            arch: required_variant_string_field(v, "arch")?,
            install_size: optional_variant_u64_field(v, "install_size"),
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
    fn data(&self, operation: &str, specs: &[String], dry_run: bool) -> Value {
        domain::mutation_plan_data_from_buckets(
            operation,
            specs,
            dry_run,
            domain::DNF5_BACKEND,
            self,
            json!(self.install_size_total),
        )
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
pub(super) fn run_mutation(
    cli: &Cli,
    client: &mut BridgeClient,
    m: Mutation,
    specs: &[String],
    host: &str,
) -> Result<View> {
    // Validate every spec before opening a privileged session.
    for spec in specs {
        validate_package_spec(spec)?;
    }
    let (channel, session) = open_session(client, true)?;
    // Do the work in an inner closure so the session is closed on every path,
    // success or failure, before the result propagates.
    let mut ctx = CapabilityContext {
        client,
        channel: &channel,
        host,
    };
    let result = mutation_inner(cli, &mut ctx, &session, m, specs);
    close_session(ctx.client, ctx.channel, &session);
    result
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

fn plan_view(
    m: Mutation,
    host: &str,
    specs: &[String],
    plan: &ResolvedPlan,
    dry_run: bool,
) -> View {
    let data = plan.data(m.verb(), specs, dry_run);
    let counts = domain::plan_counts(plan);
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
    fn session_path_rejects_missing_or_empty_response() {
        assert_eq!(
            session_path(&json!(["/org/rpm/dnf/v0/session/1"])).unwrap(),
            "/org/rpm/dnf/v0/session/1"
        );
        assert!(matches!(
            session_path(&json!([])),
            Err(FezError::Dbus { .. })
        ));
        assert!(matches!(
            session_path(&json!([""])),
            Err(FezError::Dbus { .. })
        ));
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

        let record = PackageRecord::from_value(&raw).expect("valid package record");

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

        let record = PackageRecord::from_value(&raw).expect("valid package record");

        assert_eq!(record.install_size, 456);
        assert_eq!(record.nevra(), "vim-9.1.0-1.fc41.x86_64");
    }

    #[test]
    fn package_record_rejects_missing_required_field() {
        let raw = json!({
            "name": "vim",
            "evr": "9.1.0-1.fc41",
            "repo_id": "updates",
            "install_size": "456",
            "summary": "Editor"
        });

        assert!(PackageRecord::from_value(&raw).is_none());
    }

    #[test]
    fn package_record_rejects_wrong_type_required_field() {
        let raw = json!({
            "name": "vim",
            "evr": {"t":"s","v":5},
            "arch": "x86_64",
            "repo_id": "updates",
            "install_size": "456",
            "summary": "Editor"
        });

        assert!(PackageRecord::from_value(&raw).is_none());
    }

    #[test]
    fn package_record_defaults_missing_repo_id() {
        let raw = json!({
            "name": "vim",
            "evr": "9.1.0-1.fc41",
            "arch": "x86_64",
            "install_size": "456",
            "summary": "Editor"
        });

        let record = PackageRecord::from_value(&raw).expect("valid package record");

        assert_eq!(record.repo_id, "");
    }

    #[test]
    fn package_record_defaults_optional_fields() {
        let raw = json!({
            "name": "vim",
            "evr": "9.1.0-1.fc41",
            "arch": "x86_64",
            "repo_id": "updates",
            "install_size": "not-a-size",
            "summary": {"t":"s","v":5}
        });

        let record = PackageRecord::from_value(&raw).expect("valid required fields");

        assert_eq!(record.install_size, 0);
        assert_eq!(record.summary, "");
    }

    #[test]
    fn package_record_collection_counts_malformed_and_keeps_valid_rows() {
        let items = vec![
            json!({
                "name": "bash",
                "evr": "5.2.26-3.fc41",
                "arch": "x86_64",
                "repo_id": "fedora",
                "install_size": 12345,
                "summary": "Shell"
            }),
            json!({
                "name": "broken",
                "evr": "1-1",
                "repo_id": "fedora"
            }),
        ];

        let parsed = collect_records(&items, PackageRecord::from_value);

        assert_eq!(parsed.dropped, 1);
        assert_eq!(parsed.records.len(), 1);
        assert_eq!(
            parsed.records[0].row(),
            json!(["bash", "5.2.26-3.fc41", "x86_64", "fedora", 12345, "Shell"])
        );
        assert_eq!(
            malformed_records_hint("package", parsed.dropped),
            Some("Dropped 1 malformed dnf5daemon package record(s).".into())
        );
    }

    #[test]
    fn package_record_collection_has_no_hint_for_all_valid_records() {
        let items = vec![json!({
            "name": "bash",
            "evr": "5.2.26-3.fc41",
            "arch": "x86_64",
            "repo_id": "fedora",
            "install_size": 12345,
            "summary": "Shell"
        })];

        let parsed = collect_records(&items, PackageRecord::from_value);

        assert_eq!(parsed.dropped, 0);
        assert_eq!(parsed.records.len(), 1);
        assert_eq!(malformed_records_hint("package", parsed.dropped), None);
    }

    #[test]
    fn repo_record_parses_variant_wrapped_fields() {
        let raw = json!({
            "id": {"t":"s","v":"fedora"},
            "name": {"t":"s","v":"Fedora Everything"},
            "enabled": {"t":"b","v":true}
        });

        let repo = RepoRecord::from_value(&raw).expect("valid repo record");

        assert_eq!(repo.id, "fedora");
        assert_eq!(repo.name, "Fedora Everything");
        assert!(repo.enabled);
        assert_eq!(repo.row(), json!(["fedora", "Fedora Everything", true]));
    }

    #[test]
    fn repo_record_rejects_missing_required_field() {
        let raw = json!({
            "name": {"t":"s","v":"Fedora Everything"},
            "enabled": {"t":"b","v":true}
        });

        assert!(RepoRecord::from_value(&raw).is_none());
    }

    #[test]
    fn repo_record_rejects_malformed_required_field() {
        let raw = json!({
            "id": {"t":"s","v":"fedora"},
            "name": {"t":"s","v":"Fedora Everything"},
            "enabled": {"t":"b","v":"yes"}
        });

        assert!(RepoRecord::from_value(&raw).is_none());
    }

    #[test]
    fn repo_record_defaults_optional_name() {
        let raw = json!({
            "id": {"t":"s","v":"fedora"},
            "name": {"t":"s","v":5},
            "enabled": {"t":"b","v":true}
        });

        let repo = RepoRecord::from_value(&raw).expect("valid required fields");

        assert_eq!(repo.name, "");
    }

    #[test]
    fn repo_record_collection_counts_malformed_and_keeps_valid_rows() {
        let items = vec![
            json!({
                "id": {"t":"s","v":"fedora"},
                "name": {"t":"s","v":"Fedora Everything"},
                "enabled": {"t":"b","v":true}
            }),
            json!({
                "id": {"t":"s","v":"broken"},
                "name": {"t":"s","v":"Broken Repo"},
                "enabled": {"t":"b","v":"yes"}
            }),
        ];

        let parsed = collect_records(&items, RepoRecord::from_value);

        assert_eq!(parsed.dropped, 1);
        assert_eq!(parsed.records.len(), 1);
        assert_eq!(
            parsed.records[0].row(),
            json!(["fedora", "Fedora Everything", true])
        );
        assert_eq!(
            malformed_records_hint("repo", parsed.dropped),
            Some("Dropped 1 malformed dnf5daemon repo record(s).".into())
        );
    }

    #[test]
    fn repo_record_collection_has_no_hint_for_all_valid_records() {
        let items = vec![json!({
            "id": {"t":"s","v":"fedora"},
            "name": {"t":"s","v":"Fedora Everything"},
            "enabled": {"t":"b","v":true}
        })];

        let parsed = collect_records(&items, RepoRecord::from_value);

        assert_eq!(parsed.dropped, 0);
        assert_eq!(parsed.records.len(), 1);
        assert_eq!(malformed_records_hint("repo", parsed.dropped), None);
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

    #[test]
    fn validate_package_spec_rejects_bad_specs() {
        use crate::error::FezError;
        assert!(matches!(validate_package_spec(""), Err(FezError::Usage(_))));
        assert!(matches!(
            validate_package_spec("--help"),
            Err(FezError::Usage(_))
        ));
        assert!(matches!(
            validate_package_spec("foo\x00bar"),
            Err(FezError::Usage(_))
        ));
        let long = "a".repeat(513);
        assert!(matches!(
            validate_package_spec(&long),
            Err(FezError::Usage(_))
        ));
    }

    #[test]
    fn validate_package_spec_accepts_valid_specs() {
        assert!(validate_package_spec("htop").is_ok());
        assert!(validate_package_spec("nginx-1.26.2-1.fc41.x86_64").is_ok());
        assert!(validate_package_spec("@development-tools").is_ok());
    }
}
