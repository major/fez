//! RPM package management over dnf5daemon (`org.rpm.dnf.v0`).
//!
//! Mirrors the structure of [`crate::capabilities::services`]: a single
//! [`classify`] splits the flat clap enum into reads and mutations, reads run
//! unprivileged and render a [`View`], mutations resolve the transaction first,
//! apply removal guardrails, audit, then execute. Every dnf5daemon session is
//! closed best-effort on every return path so the daemon never leaks sessions.
use crate::cli::{Cli, PackagesAction};
use crate::envelope::{ApiError, Envelope};
use crate::error::{is_service_unknown, FezError, Result};
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

/// One render-ready response: a payload, a human rendering, and an envelope kind.
struct View {
    kind: &'static str,
    host: String,
    data: Value,
    human: String,
    hints: Option<Value>,
}

/// A staging mutation that goes through resolve-first/guardrail/execute.
#[derive(Clone, Copy)]
enum Mutation {
    Install,
    Remove,
    Upgrade,
    DistroSync,
    Downgrade,
}

impl Mutation {
    fn verb(self) -> &'static str {
        match self {
            Mutation::Install => "install",
            Mutation::Remove => "remove",
            Mutation::Upgrade => "upgrade",
            Mutation::DistroSync => "distro-sync",
            Mutation::Downgrade => "downgrade",
        }
    }
    /// The dnf5daemon `Rpm` D-Bus method name to stage this mutation.
    ///
    /// Distinct from [`Mutation::verb`] (the user-facing display verb): e.g.
    /// `distro-sync` (verb) maps to the `distro_sync` D-Bus method.
    fn method(self) -> &'static str {
        match self {
            Mutation::Install => "install",
            Mutation::Remove => "remove",
            Mutation::Upgrade => "upgrade",
            Mutation::DistroSync => "distro_sync",
            Mutation::Downgrade => "downgrade",
        }
    }
}

/// A read subcommand and its arguments, borrowed from the parsed action.
///
/// Splitting reads out of [`PackagesAction`] keeps [`run_read`] total: every
/// variant here maps to a handler, so adding one is a compile error rather than
/// a runtime panic.
enum ReadAction<'a> {
    List { available: bool },
    Info { spec: &'a str },
    Search { pattern: &'a str },
    CheckUpdate,
    Repolist { filter: RepoFilter },
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
            repo: _repo,
        } => Plan::Read(ReadAction::List {
            available: *available,
        }),
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
        PackagesAction::DistroSync { specs } => Plan::Mutate {
            mutation: Mutation::DistroSync,
            specs: specs.clone(),
        },
        PackagesAction::Downgrade { specs } => Plan::Mutate {
            mutation: Mutation::Downgrade,
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
    let out = match out {
        Ok(v) => v,
        Err(FezError::Dbus { name, .. }) if is_service_unknown(&name) => {
            return Err(dependency_missing())
        }
        Err(e) => return Err(e),
    };
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

/// A single package row in list/info/search/check-update output.
fn package_json(p: &Value) -> Value {
    json!({
        "name": sv(p, "name"),
        "evr": sv(p, "evr"),
        "arch": sv(p, "arch"),
        "repo_id": sv(p, "repo_id"),
        "install_size": sv_u64(p, "install_size"),
        "summary": sv(p, "summary"),
    })
}

/// Column order for the columnar `PackageList`/`PackageSearch` payloads.
///
/// Stated once here so the header and each [`package_row`] stay aligned; the
/// record-shaped [`package_json`] (used by single-package `PackageInfo`) keeps
/// the same field names.
const PKG_COLUMNS: &[&str] = &["name", "evr", "arch", "repo_id", "install_size", "summary"];

/// Project one raw package into a positional row aligned to [`PKG_COLUMNS`].
///
/// `install_size` stays an integer; every other cell is a string.
fn package_row(p: &Value) -> Value {
    json!([
        sv(p, "name"),
        sv(p, "evr"),
        sv(p, "arch"),
        sv(p, "repo_id"),
        sv_u64(p, "install_size"),
        sv(p, "summary"),
    ])
}

/// Connect, open an unprivileged session, dispatch the read, and always close.
fn run_read(cli: &Cli, action: ReadAction<'_>) -> Result<View> {
    let transport = crate::transport::from_host(cli.host.as_deref());
    let mut client = BridgeClient::connect(transport.as_ref())?;
    let host = client.host().to_string();
    let (channel, session) = open_session(&mut client, false)?;
    let result = match action {
        ReadAction::List { available } => list(&mut client, &channel, &session, host, available),
        ReadAction::Info { spec } => info(&mut client, &channel, &session, host, spec),
        ReadAction::Search { pattern } => search(&mut client, &channel, &session, host, pattern),
        ReadAction::CheckUpdate => check_update(&mut client, &channel, &session, host),
        ReadAction::Repolist { filter } => repolist(&mut client, &channel, &session, host, filter),
    };
    close_session(&mut client, &channel, &session);
    result
}

/// Call `Rpm.list` on the session with the given scope/patterns and return the
/// raw package array.
fn rpm_list(
    client: &mut BridgeClient,
    channel: &str,
    session: &str,
    scope: &str,
    patterns: &[String],
) -> Result<Vec<Value>> {
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
        .cloned()
        .unwrap_or_default())
}

fn list(
    client: &mut BridgeClient,
    channel: &str,
    session: &str,
    host: String,
    available: bool,
) -> Result<View> {
    let scope = if available { "available" } else { "installed" };
    let raw = rpm_list(client, channel, session, scope, &[])?;
    let mut human = format!(
        "{:<24} {:<20} {:<10} {}\n",
        "NAME", "VERSION", "ARCH", "REPO"
    );
    for p in &raw {
        human.push_str(&format!(
            "{:<24} {:<20} {:<10} {}\n",
            sv(p, "name"),
            sv(p, "evr"),
            sv(p, "arch"),
            sv(p, "repo_id"),
        ));
    }
    let rows: Vec<Value> = raw.iter().map(package_row).collect();
    let mut data = crate::envelope::table_data(PKG_COLUMNS, rows);
    data["scope"] = json!(scope);
    Ok(View {
        kind: "PackageList",
        host,
        data,
        human,
        hints: None,
    })
}

fn info(
    client: &mut BridgeClient,
    channel: &str,
    session: &str,
    host: String,
    spec: &str,
) -> Result<View> {
    let raw = rpm_list(client, channel, session, "all", &[spec.to_string()])?;
    let first = raw
        .first()
        .ok_or_else(|| FezError::NotFound(spec.to_string()))?;
    let pkg = package_json(first);
    let human = format!(
        "Name        : {}\nVersion     : {}\nArch        : {}\nRepo        : {}\nInstall size: {}\nSummary     : {}\n",
        sv(first, "name"),
        sv(first, "evr"),
        sv(first, "arch"),
        sv(first, "repo_id"),
        sv_u64(first, "install_size"),
        sv(first, "summary"),
    );
    Ok(View {
        kind: "PackageInfo",
        host,
        data: pkg,
        human,
        hints: None,
    })
}

fn search(
    client: &mut BridgeClient,
    channel: &str,
    session: &str,
    host: String,
    pattern: &str,
) -> Result<View> {
    let glob = format!("*{pattern}*");
    let raw = rpm_list(client, channel, session, "available", &[glob])?;
    let mut human = String::new();
    for p in &raw {
        human.push_str(&format!("{} - {}\n", sv(p, "name"), sv(p, "summary")));
    }
    let rows: Vec<Value> = raw.iter().map(package_row).collect();
    let mut data = crate::envelope::table_data(PKG_COLUMNS, rows);
    data["pattern"] = json!(pattern);
    Ok(View {
        kind: "PackageSearch",
        host,
        data,
        human,
        hints: None,
    })
}

fn check_update(
    client: &mut BridgeClient,
    channel: &str,
    session: &str,
    host: String,
) -> Result<View> {
    let raw = rpm_list(client, channel, session, "upgrades", &[])?;
    let mut human = format!("{:<24} {:<20} {}\n", "NAME", "VERSION", "REPO");
    for p in &raw {
        human.push_str(&format!(
            "{:<24} {:<20} {}\n",
            sv(p, "name"),
            sv(p, "evr"),
            sv(p, "repo_id"),
        ));
    }
    let rows: Vec<Value> = raw.iter().map(package_row).collect();
    Ok(View {
        kind: "PackageUpdates",
        host,
        data: crate::envelope::table_data(PKG_COLUMNS, rows),
        human,
        hints: None,
    })
}

/// Column order for the columnar `RepoList` payload (`enabled` stays a bool).
const REPO_COLUMNS: &[&str] = &["id", "name", "enabled"];

fn repolist(
    client: &mut BridgeClient,
    channel: &str,
    session: &str,
    host: String,
    filter: RepoFilter,
) -> Result<View> {
    let out = client.dbus_call(
        channel,
        session,
        REPO_IFACE,
        "list",
        json!([options(&[
            ("enable_disable", "s", json!(filter.enable_disable())),
            ("repo_attrs", "as", json!(["id", "name", "enabled"])),
        ])]),
    )?;
    let raw = out
        .get(0)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut rows = Vec::new();
    let mut human = format!("{:<24} {:<10} {}\n", "REPO ID", "ENABLED", "NAME");
    for r in &raw {
        let enabled = sv_bool(r, "enabled");
        if !filter.accepts(enabled) {
            continue;
        }
        let id = sv(r, "id");
        let name = sv(r, "name");
        human.push_str(&format!("{id:<24} {enabled:<10} {name}\n"));
        rows.push(json!([id, name, enabled]));
    }
    Ok(View {
        kind: "RepoList",
        host,
        data: crate::envelope::table_data(REPO_COLUMNS, rows),
        human,
        hints: None,
    })
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

/// Format a package object's NEVRA label (`name-evr.arch`).
fn nevra(object: &Value) -> String {
    format!(
        "{}-{}.{}",
        sv(object, "name"),
        sv(object, "evr"),
        sv(object, "arch")
    )
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
    for item in arr {
        let action = item.get(1).and_then(Value::as_str).unwrap_or("");
        let object = item.get(4).cloned().unwrap_or(Value::Null);
        let label = nevra(&object);
        match action {
            "Install" => {
                plan.install_size_total += sv_u64(&object, "install_size");
                plan.install.push(label);
            }
            "Upgrade" => plan.upgrade.push(label),
            "Downgrade" => plan.downgrade.push(label),
            "Remove" | "Replaced" | "Obsoleted" | "Erase" => {
                plan.remove_names.push(sv(&object, "name"));
                plan.remove.push(label);
            }
            _ => {}
        }
    }
    plan
}

/// Resolve-first mutation: stage, resolve, optionally short-circuit on dry-run,
/// apply removal guardrails, audit, execute, and always close the session.
fn run_mutation(cli: &Cli, m: Mutation, specs: &[String]) -> Result<View> {
    let host = cli.resolved_host();
    let transport = crate::transport::from_host(cli.host.as_deref());
    let mut client = BridgeClient::connect(transport.as_ref())?;
    let (channel, session) = open_session(&mut client, true)?;
    // Do the work in an inner closure so the session is closed on every path,
    // success or failure, before the result propagates.
    let result = mutation_inner(cli, &mut client, &channel, &session, m, specs, &host);
    close_session(&mut client, &channel, &session);
    result
}

#[allow(clippy::too_many_arguments)]
fn mutation_inner(
    cli: &Cli,
    client: &mut BridgeClient,
    channel: &str,
    session: &str,
    m: Mutation,
    specs: &[String],
    host: &str,
) -> Result<View> {
    // 1. Stage the goal.
    client.dbus_call(channel, session, RPM_IFACE, m.method(), json!([specs, {}]))?;
    // 2. Resolve into a concrete plan.
    let out = client.dbus_call(channel, session, GOAL_IFACE, "resolve", json!([{}]))?;
    let items = out.get(0).cloned().unwrap_or(Value::Null);
    let plan = parse_plan(&items);

    // 3. Dry-run: report the plan without executing.
    if cli.dry_run {
        return Ok(plan_view(m, host, specs, &plan, true));
    }

    // 4. Removal guardrails (protected package / cascade) before any execution.
    crate::safety::check_removal_plan(&plan.remove_names, cli.force)?;

    // 5. Audit attempt, execute, audit result.
    let sink = crate::audit::sink_from_env();
    let audit = crate::audit::AuditContext::new(
        &crate::audit::actor(),
        host,
        m.verb(),
        &specs.join(","),
        &crate::audit::correlation_id(),
    );
    sink.write(&audit.record(crate::audit::Outcome::Attempt));
    let exec = client.dbus_call(channel, session, GOAL_IFACE, "do_transaction", json!([{}]));
    match &exec {
        Ok(_) => sink.write(&audit.record(crate::audit::Outcome::Ok)),
        Err(e) => sink.write(&audit.record(crate::audit::Outcome::Error(e.to_string()))),
    }
    exec?;
    Ok(plan_view(m, host, specs, &plan, false))
}

/// Build the [`View`] for a resolved plan (dry-run preview or executed mutation).
fn plan_view(
    m: Mutation,
    host: &str,
    specs: &[String],
    plan: &ResolvedPlan,
    dry_run: bool,
) -> View {
    let kind = if dry_run {
        "PackagePlan"
    } else {
        "PackageMutation"
    };
    let mut data = plan.data();
    if let Value::Object(map) = &mut data {
        map.insert("operation".into(), json!(m.verb()));
        map.insert("specs".into(), json!(specs));
        map.insert("dry_run".into(), json!(dry_run));
    }
    let human = if dry_run {
        format!(
            "DRY-RUN: {} {} on {} would install {}, remove {}, upgrade {}, downgrade {} package(s)\n",
            m.verb(),
            specs.join(" "),
            host,
            plan.install.len(),
            plan.remove.len(),
            plan.upgrade.len(),
            plan.downgrade.len(),
        )
    } else {
        format!(
            "{} {} on {}: installed {}, removed {}, upgraded {}, downgraded {} package(s)\n",
            m.verb(),
            specs.join(" "),
            host,
            plan.install.len(),
            plan.remove.len(),
            plan.upgrade.len(),
            plan.downgrade.len(),
        )
    };
    View {
        kind,
        host: host.to_string(),
        data,
        human,
        hints: None,
    }
}

/// Render a [`View`] (or error) as text or a `fez/v1` envelope; return exit code.
fn render(cli: &Cli, result: Result<View>) -> i32 {
    let host = cli.resolved_host();
    match result {
        Ok(view) => {
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
                let detail = error_detail(&e);
                let env = Envelope::error(
                    "Error",
                    &host,
                    ApiError {
                        code: e.code().into(),
                        message: e.to_string(),
                        detail,
                    },
                );
                println!("{}", env.to_json_string());
            } else {
                eprintln!("error: {e}");
            }
            e.exit_code()
        }
    }
}

/// Structured `detail` for the error envelope, for errors that carry one.
fn error_detail(e: &FezError) -> Option<Value> {
    match e {
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
        _ => None,
    }
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
    fn parse_plan_totals_install_size() {
        let items = json!([item("Install", "a", 100), item("Install", "b", 200)]);
        assert_eq!(parse_plan(&items).install_size_total, 300);
    }
}
