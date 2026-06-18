//! PackageKit fallback backend (`org.freedesktop.PackageKit`).
//!
//! Used when dnf5daemon (`org.rpm.dnf.v0`) is absent (notably RHEL 10). Drives
//! PackageKit's per-transaction D-Bus API: a transaction object path is created,
//! a method is called on it, and the results arrive as a stream of signals
//! collected by [`crate::protocol::client::BridgeClient::dbus_call_collect`].
//!
//! PackageKit's plan carries no install/download sizes (only `info`,
//! `package_id`, `summary` per package), so size fields are emitted as `null`
//! and every payload carries `"backend":"packagekit"` plus a hint so callers
//! can see the schema is degraded relative to the dnf5daemon path.
use crate::error::{FezError, Result};
use crate::protocol::client::BridgeClient;
use serde_json::{json, Value};

/// Well-known bus name of the PackageKit daemon.
const PK_NAME: &str = "org.freedesktop.PackageKit";
/// Object path of the PackageKit daemon's root controller.
const PK_PATH: &str = "/org/freedesktop/PackageKit";
/// Root controller interface (carries `CreateTransaction`).
const PK_IFACE: &str = "org.freedesktop.PackageKit";
/// Per-transaction interface (carries the operation methods + result signals).
const TX_IFACE: &str = "org.freedesktop.PackageKit.Transaction";

/// PackageKit transaction flag: execute for real (no special handling).
const TF_NONE: u64 = 0;
/// PackageKit transaction flag: simulate only (resolve the plan, change nothing).
const TF_SIMULATE: u64 = 2;

/// PackageKit filter bit: only installed packages (`PK_FILTER_ENUM_INSTALLED`).
const FILTER_INSTALLED: u64 = 1 << 2;
/// PackageKit filter bit: newest version only (`PK_FILTER_ENUM_NEWEST`).
const FILTER_NEWEST: u64 = 1 << 16;
/// PackageKit filter: no filtering (`PK_FILTER_ENUM_NONE`).
const FILTER_NONE: u64 = 0;

/// PackageKit `info` enum: package will be installed.
const INFO_INSTALLING: u64 = 12;
/// PackageKit `info` enum: package will be removed.
const INFO_REMOVING: u64 = 13;
/// PackageKit `info` enum: package will be updated.
const INFO_UPDATING: u64 = 11;
/// PackageKit `info` enum: package will be obsoleted (treated as a removal).
const INFO_OBSOLETING: u64 = 15;
/// PackageKit `info` enum: package will be downgraded.
const INFO_DOWNGRADING: u64 = 20;

/// PackageKit error enum: `PK_ERROR_ENUM_NOT_AUTHORIZED`.
const PK_ERROR_NOT_AUTHORIZED: u64 = 6;

/// A render-ready PackageKit result, assembled into a `package::View` by the
/// caller (which supplies the host).
pub struct PkView {
    /// Envelope `kind` (e.g. `PackageList`, `PackagePlan`, `PackageMutation`).
    pub kind: &'static str,
    /// The `fez/v1` data payload, carrying `"backend":"packagekit"`.
    pub data: Value,
    /// Human-readable rendering.
    pub human: String,
    /// Optional envelope hints (carries the degraded-schema note).
    pub hints: Option<Value>,
}

/// One package parsed from a PackageKit `Package(info, package_id, summary)`
/// signal. `package_id` is `name;version;arch;data` (data = repo or `installed`).
struct PkPackage {
    info: u64,
    name: String,
    version: String,
    arch: String,
    data: String,
    summary: String,
}

impl PkPackage {
    /// Parse a `Package` signal's `[info, package_id, summary]` args.
    ///
    /// Returns `None` if the args are malformed (missing `info` or `package_id`).
    fn from_signal(args: &[Value]) -> Option<Self> {
        let info = args.first()?.as_u64()?;
        let pid = args.get(1)?.as_str()?;
        let summary = args
            .get(2)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let mut parts = pid.splitn(4, ';');
        let name = parts.next()?.to_string();
        let version = parts.next().unwrap_or("").to_string();
        let arch = parts.next().unwrap_or("").to_string();
        let data = parts.next().unwrap_or("").to_string();
        Some(PkPackage {
            info,
            name,
            version,
            arch,
            data,
            summary,
        })
    }

    /// The NEVRA-ish label `name-version.arch` for plan rendering.
    fn label(&self) -> String {
        format!("{}-{}.{}", self.name, self.version, self.arch)
    }

    /// The full `name;version;arch;data` package_id (mutations need this form).
    fn package_id(&self) -> String {
        format!("{};{};{};{}", self.name, self.version, self.arch, self.data)
    }

    /// The repo/data field; PackageKit puts the repo id (or `installed`) here.
    fn repo(&self) -> &str {
        &self.data
    }

    /// One positional row aligned to [`PK_COLUMNS`]; `install_size` is `null`.
    fn row(&self) -> Value {
        json!([
            self.name,
            self.version,
            self.arch,
            self.repo(),
            Value::Null,
            self.summary,
        ])
    }

    /// Record-shaped package info payload without the backend marker.
    fn object(&self) -> Value {
        json!({
            "name": self.name,
            "evr": self.version,
            "arch": self.arch,
            "repo_id": self.repo(),
            "install_size": Value::Null,
            "summary": self.summary,
        })
    }
}

/// One repository parsed from a PackageKit `RepoDetail(id, name, enabled)` signal.
struct PkRepo {
    id: String,
    name: String,
    enabled: bool,
}

impl PkRepo {
    /// Parse a `RepoDetail` signal's `[id, name, enabled]` args.
    fn from_signal(args: &[Value]) -> Option<Self> {
        Some(Self {
            id: args.first()?.as_str()?.to_string(),
            name: args.get(1)?.as_str()?.to_string(),
            enabled: args.get(2)?.as_bool()?,
        })
    }

    /// One positional row aligned to [`PK_REPO_COLUMNS`].
    fn row(&self) -> Value {
        json!([self.id, self.name, self.enabled])
    }
}

/// Pull the `Package` rows out of a collected signal stream, in arrival order.
fn packages_from(signals: &[(String, Vec<Value>)]) -> Vec<PkPackage> {
    signals
        .iter()
        .filter(|(member, _)| member == "Package")
        .filter_map(|(_, args)| PkPackage::from_signal(args))
        .collect()
}

/// Map a PackageKit `ErrorCode(code, details)` in the stream, if any, to a
/// [`FezError`].
///
/// # Errors
///
/// Returns [`FezError::AccessDenied`] (exit 11) when the stream carries an
/// `ErrorCode` with `PK_ERROR_ENUM_NOT_AUTHORIZED`, or [`FezError::Dbus`] for
/// any other PackageKit error.
fn check_stream(signals: &[(String, Vec<Value>)]) -> Result<()> {
    if let Some((_, args)) = signals.iter().find(|(m, _)| m == "ErrorCode") {
        let code = args.first().and_then(Value::as_u64).unwrap_or(0);
        let details = args
            .get(1)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if code == PK_ERROR_NOT_AUTHORIZED {
            return Err(FezError::AccessDenied {
                remediation: format!(
                    "PackageKit denied the operation ({details}). Ensure privilege escalation is available (passwordless sudo or a polkit rule) and retry."
                ),
            });
        }
        return Err(FezError::Dbus {
            name: "org.freedesktop.PackageKit.Error".into(),
            message: details,
        });
    }
    Ok(())
}

/// Column order for PackageKit list/search payloads, identical to the
/// dnf5daemon backend's `PKG_COLUMNS` so the two schemas line up, except
/// `install_size` is always JSON `null` here (PackageKit reports no size).
const PK_COLUMNS: &[&str] = &["name", "evr", "arch", "repo_id", "install_size", "summary"];

/// Column order for the PackageKit `RepoList` payload, matching the dnf
/// backend's `REPO_COLUMNS`.
const PK_REPO_COLUMNS: &[&str] = &["id", "name", "enabled"];

/// Open an (optionally privileged) channel to the PackageKit bus name.
///
/// Reads use an unprivileged channel; mutations pass `privileged = true` so the
/// bridge escalates to root (which bypasses PackageKit's `auth_admin` polkit
/// actions).
///
/// # Errors
///
/// Propagates transport / channel-open errors.
fn open_pk(client: &mut BridgeClient, privileged: bool) -> Result<String> {
    if privileged {
        client.dbus_open_privileged(PK_NAME)
    } else {
        client.dbus_open(PK_NAME)
    }
}

/// Create a fresh PackageKit transaction object on `channel`.
///
/// Each PackageKit transaction is single-use (it is gone once `Finished`
/// fires), so callers create one per operation. A ServiceUnknown D-Bus error
/// here means PackageKit itself is absent; the caller maps that to
/// dependency-missing.
///
/// # Errors
///
/// Propagates the `CreateTransaction` call error (including ServiceUnknown when
/// PackageKit is not installed).
fn new_tx(client: &mut BridgeClient, channel: &str) -> Result<String> {
    let out = client.dbus_call(channel, PK_PATH, PK_IFACE, "CreateTransaction", json!([]))?;
    Ok(out
        .as_array()
        .and_then(|a| a.first())
        .or(Some(&out))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string())
}

/// The envelope hint every PackageKit payload carries, flagging the degraded
/// schema (no sizes) and the active backend.
fn pk_hints() -> Option<Value> {
    Some(json!({
        "backend": "packagekit",
        "note": "Running via the PackageKit fallback backend; install/download sizes are unavailable on this backend.",
    }))
}

/// Human-readable NAME/VERSION/ARCH/REPO table for list output.
fn human_table(pkgs: &[&PkPackage]) -> String {
    let mut s = format!(
        "{:<24} {:<20} {:<10} {}\n",
        "NAME", "VERSION", "ARCH", "REPO"
    );
    for p in pkgs {
        s.push_str(&format!(
            "{:<24} {:<20} {:<10} {}\n",
            p.name,
            p.version,
            p.arch,
            p.repo()
        ));
    }
    s
}

/// `packages list`: `GetPackages` with the INSTALLED or NEWEST filter.
///
/// # Errors
///
/// Propagates transport, transaction, or PackageKit stream errors.
pub fn list(
    client: &mut BridgeClient,
    available: bool,
    repos: &[String],
    name: Option<&str>,
    limit: Option<usize>,
    offset: usize,
) -> Result<PkView> {
    let channel = open_pk(client, false)?;
    let tx = new_tx(client, &channel)?;
    let filter = if available {
        FILTER_NEWEST
    } else {
        FILTER_INSTALLED
    };
    let signals =
        client.dbus_call_collect(&channel, &tx, TX_IFACE, "GetPackages", json!([filter]))?;
    check_stream(&signals)?;
    let pkgs = packages_from(&signals);
    let filtered: Vec<&PkPackage> = pkgs
        .iter()
        .filter(|p| repos.is_empty() || repos.iter().any(|r| r == p.repo()))
        .filter(|p| name.is_none_or(|pattern| p.name.contains(pattern)))
        .collect();
    let total = filtered.len();
    let start = offset.min(total);
    let end = match limit {
        Some(limit) => (start + limit).min(total),
        None => total,
    };
    let page = &filtered[start..end];
    let rows: Vec<Value> = page.iter().map(|p| p.row()).collect();
    let mut data = crate::envelope::table_data(PK_COLUMNS, rows);
    data["scope"] = json!(if available { "available" } else { "installed" });
    data["repos"] = json!(repos);
    data["name"] = json!(name);
    data["total"] = json!(total);
    data["returned"] = json!(end - start);
    data["limit"] = json!(limit);
    data["offset"] = json!(offset);
    data["next_offset"] = json!((end < total).then_some(end));
    data["backend"] = json!("packagekit");
    let human = human_table(page);
    let hints = if limit.is_none() && total > 1000 {
        Some(json!([
            pk_hints().expect("PackageKit hint is always present"),
            format!(
                "This response has {total} rows. Prefer packages search <pattern>, use --name, or use --limit."
            )
        ]))
    } else {
        pk_hints()
    };
    Ok(PkView {
        kind: "PackageList",
        data,
        human,
        hints,
    })
}

/// `packages info <spec>`: `Resolve` the spec and render the first match.
///
/// # Errors
///
/// Returns [`FezError::NotFound`] when the spec resolves to nothing, or
/// propagates transport / stream errors.
pub fn info(client: &mut BridgeClient, spec: &str) -> Result<PkView> {
    let channel = open_pk(client, false)?;
    let tx = new_tx(client, &channel)?;
    let signals = client.dbus_call_collect(
        &channel,
        &tx,
        TX_IFACE,
        "Resolve",
        json!([FILTER_NEWEST, [spec]]),
    )?;
    check_stream(&signals)?;
    let pkgs = packages_from(&signals);
    let p = pkgs
        .first()
        .ok_or_else(|| FezError::NotFound(spec.to_string()))?;
    let mut data = p.object();
    data["backend"] = json!("packagekit");
    let human = format!(
        "Name        : {}\nVersion     : {}\nArch        : {}\nRepo        : {}\nInstall size: (unavailable)\nSummary     : {}\n",
        p.name,
        p.version,
        p.arch,
        p.repo(),
        p.summary,
    );
    Ok(PkView {
        kind: "PackageInfo",
        data,
        human,
        hints: pk_hints(),
    })
}

/// `packages search <pattern>`: `SearchNames` over available packages.
///
/// # Errors
///
/// Propagates transport, transaction, or PackageKit stream errors.
pub fn search(client: &mut BridgeClient, pattern: &str) -> Result<PkView> {
    let channel = open_pk(client, false)?;
    let tx = new_tx(client, &channel)?;
    let signals = client.dbus_call_collect(
        &channel,
        &tx,
        TX_IFACE,
        "SearchNames",
        json!([FILTER_NEWEST, [pattern]]),
    )?;
    check_stream(&signals)?;
    let pkgs = packages_from(&signals);
    let refs: Vec<&PkPackage> = pkgs.iter().collect();
    let rows: Vec<Value> = refs.iter().map(|p| p.row()).collect();
    let mut data = crate::envelope::table_data(PK_COLUMNS, rows);
    data["pattern"] = json!(pattern);
    data["backend"] = json!("packagekit");
    let mut human = String::new();
    for p in &refs {
        human.push_str(&format!("{} - {}\n", p.name, p.summary));
    }
    Ok(PkView {
        kind: "PackageSearch",
        data,
        human,
        hints: pk_hints(),
    })
}

/// `packages check-update`: `GetUpdates` reports the available upgrades.
///
/// # Errors
///
/// Propagates transport, transaction, or PackageKit stream errors.
pub fn check_update(client: &mut BridgeClient) -> Result<PkView> {
    let channel = open_pk(client, false)?;
    let tx = new_tx(client, &channel)?;
    let signals =
        client.dbus_call_collect(&channel, &tx, TX_IFACE, "GetUpdates", json!([FILTER_NONE]))?;
    check_stream(&signals)?;
    let pkgs = packages_from(&signals);
    let refs: Vec<&PkPackage> = pkgs.iter().collect();
    let rows: Vec<Value> = refs.iter().map(|p| p.row()).collect();
    let mut data = crate::envelope::table_data(PK_COLUMNS, rows);
    data["backend"] = json!("packagekit");
    let mut human = format!("{:<24} {:<20} {}\n", "NAME", "VERSION", "REPO");
    for p in &refs {
        human.push_str(&format!("{:<24} {:<20} {}\n", p.name, p.version, p.repo()));
    }
    Ok(PkView {
        kind: "PackageUpdates",
        data,
        human,
        hints: pk_hints(),
    })
}

/// `packages repolist`: `GetRepoList` plus the enabled/disabled/all filter.
///
/// `accepts` mirrors the dnf backend's `RepoFilter::accepts`: `enabled` keeps
/// only enabled repos, `disabled` only disabled, `all` keeps everything.
///
/// # Errors
///
/// Propagates transport, transaction, or PackageKit stream errors.
pub fn repolist(client: &mut BridgeClient, accepts: impl Fn(bool) -> bool) -> Result<PkView> {
    let channel = open_pk(client, false)?;
    let tx = new_tx(client, &channel)?;
    let signals =
        client.dbus_call_collect(&channel, &tx, TX_IFACE, "GetRepoList", json!([FILTER_NONE]))?;
    check_stream(&signals)?;
    let repos: Vec<PkRepo> = signals
        .iter()
        .filter(|(member, _)| member == "RepoDetail")
        .filter_map(|(_, args)| PkRepo::from_signal(args))
        .collect();
    let mut rows = Vec::new();
    let mut human = format!("{:<24} {:<10} {}\n", "REPO ID", "ENABLED", "NAME");
    for repo in &repos {
        if !accepts(repo.enabled) {
            continue;
        }
        human.push_str(&format!(
            "{:<24} {:<10} {}\n",
            repo.id, repo.enabled, repo.name
        ));
        rows.push(repo.row());
    }
    let mut data = crate::envelope::table_data(PK_REPO_COLUMNS, rows);
    data["backend"] = json!("packagekit");
    Ok(PkView {
        kind: "RepoList",
        data,
        human,
        hints: pk_hints(),
    })
}

/// A mutation plan parsed from a SIMULATE transaction's `Package` signals,
/// bucketed by the `info` enum (mirrors the dnf backend's `ResolvedPlan`).
struct PkPlan {
    install: Vec<String>,
    remove: Vec<String>,
    upgrade: Vec<String>,
    downgrade: Vec<String>,
    remove_names: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PkPlanAction {
    Install,
    Remove,
    Upgrade,
    Downgrade,
}

impl PkPlanAction {
    fn from_info(info: u64) -> Option<Self> {
        match info {
            INFO_INSTALLING => Some(Self::Install),
            INFO_UPDATING => Some(Self::Upgrade),
            INFO_DOWNGRADING => Some(Self::Downgrade),
            INFO_REMOVING | INFO_OBSOLETING => Some(Self::Remove),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PkMutation {
    Install,
    Remove,
    Upgrade,
}

impl PkMutation {
    fn from_verb(verb: &str) -> Result<Self> {
        match verb {
            "install" => Ok(Self::Install),
            "remove" => Ok(Self::Remove),
            "upgrade" => Ok(Self::Upgrade),
            _ => Err(FezError::Usage(format!(
                "unsupported PackageKit mutation verb: {verb}"
            ))),
        }
    }

    fn verb(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Remove => "remove",
            Self::Upgrade => "upgrade",
        }
    }
}

/// Bucket a SIMULATE stream's packages by their `info` enum into a [`PkPlan`].
fn plan_from(signals: &[(String, Vec<Value>)]) -> PkPlan {
    let mut plan = PkPlan {
        install: vec![],
        remove: vec![],
        upgrade: vec![],
        downgrade: vec![],
        remove_names: vec![],
    };
    for p in packages_from(signals) {
        match PkPlanAction::from_info(p.info) {
            Some(PkPlanAction::Install) => plan.install.push(p.label()),
            Some(PkPlanAction::Upgrade) => plan.upgrade.push(p.label()),
            Some(PkPlanAction::Downgrade) => plan.downgrade.push(p.label()),
            Some(PkPlanAction::Remove) => {
                plan.remove_names.push(p.name.clone());
                plan.remove.push(p.label());
            }
            None => {}
        }
    }
    plan
}

/// The PackageKit method name and argument array for each mutation verb.
fn method_for(mutation: PkMutation, flags: u64, ids: &[String]) -> (&'static str, Value) {
    match mutation {
        PkMutation::Install => ("InstallPackages", json!([flags, ids])),
        PkMutation::Upgrade => ("UpdatePackages", json!([flags, ids])),
        // RemovePackages(flags, ids, allow_deps=true, autoremove=false)
        PkMutation::Remove => ("RemovePackages", json!([flags, ids, true, false])),
    }
}

/// Resolve specs to full package_ids (mutations need them, not bare names).
///
/// # Errors
///
/// Propagates transport / stream errors. An empty resolve yields an empty id
/// list, which the caller surfaces as not-found.
fn resolve_ids(client: &mut BridgeClient, channel: &str, specs: &[String]) -> Result<Vec<String>> {
    let tx = new_tx(client, channel)?;
    let signals = client.dbus_call_collect(
        channel,
        &tx,
        TX_IFACE,
        "Resolve",
        json!([FILTER_NEWEST, specs]),
    )?;
    check_stream(&signals)?;
    Ok(packages_from(&signals)
        .iter()
        .map(PkPackage::package_id)
        .collect())
}

/// `packages install|remove|upgrade` over PackageKit.
///
/// Opens a privileged channel (cockpit escalates the bridge to root, which
/// bypasses PackageKit's `auth_admin` polkit actions), SIMULATEs to build a
/// plan, applies removal guardrails, audits, and executes. Honors `dry_run` and
/// `force` exactly like the dnf5daemon backend.
///
/// # Errors
///
/// Returns [`FezError::NotFound`] when the specs resolve to nothing,
/// [`FezError::DangerousTransaction`] (exit 8) when a removal guardrail fires
/// without `force`, [`FezError::AccessDenied`] (exit 11) when PackageKit denies
/// the operation, or propagates transport / stream errors.
pub fn mutate(
    client: &mut BridgeClient,
    verb: &str,
    specs: &[String],
    host: &str,
    dry_run: bool,
    force: bool,
) -> Result<PkView> {
    let mutation = PkMutation::from_verb(verb)?;
    let channel = open_pk(client, true)?;
    let ids = resolve_ids(client, &channel, specs)?;
    if ids.is_empty() {
        return Err(FezError::NotFound(specs.join(", ")));
    }

    // SIMULATE to build the plan (separate single-use transaction).
    let sim_tx = new_tx(client, &channel)?;
    let (m, args) = method_for(mutation, TF_SIMULATE, &ids);
    let sim = client.dbus_call_collect(&channel, &sim_tx, TX_IFACE, m, args)?;
    check_stream(&sim)?;
    let plan = plan_from(&sim);

    if dry_run {
        return Ok(plan_view(mutation, host, specs, &plan, true));
    }
    crate::safety::check_removal_plan(&plan.remove_names, force)?;

    let sink = crate::audit::sink_from_env();
    let audit = crate::audit::AuditContext::new(
        &crate::audit::actor(),
        host,
        mutation.verb(),
        &specs.join(","),
        &crate::audit::correlation_id(),
    );
    sink.write(&audit.record(crate::audit::Outcome::Attempt));
    let (m, args) = method_for(mutation, TF_NONE, &ids);
    let exec_tx = new_tx(client, &channel)?;
    let exec = client
        .dbus_call_collect(&channel, &exec_tx, TX_IFACE, m, args)
        .and_then(|s| check_stream(&s));
    match &exec {
        Ok(()) => sink.write(&audit.record(crate::audit::Outcome::Ok)),
        Err(e) => sink.write(&audit.record(crate::audit::Outcome::Error(e.to_string()))),
    }
    exec?;
    Ok(plan_view(mutation, host, specs, &plan, false))
}

/// Build a [`PkView`] for a plan (dry-run preview or executed mutation),
/// schema-compatible with the dnf5daemon backend's plan payload but with a
/// `null` `install_size_total` and a `"backend":"packagekit"` marker.
fn plan_view(
    mutation: PkMutation,
    host: &str,
    specs: &[String],
    plan: &PkPlan,
    dry_run: bool,
) -> PkView {
    use super::{plan_human, plan_kind};
    let verb = mutation.verb();
    let counts = (
        plan.install.len(),
        plan.remove.len(),
        plan.upgrade.len(),
        plan.downgrade.len(),
    );
    let data = json!({
        "operation": verb,
        "specs": specs,
        "dry_run": dry_run,
        "backend": "packagekit",
        "install": plan.install,
        "remove": plan.remove,
        "upgrade": plan.upgrade,
        "downgrade": plan.downgrade,
        "install_size_total": Value::Null,
        "counts": {
            "install": counts.0,
            "remove": counts.1,
            "upgrade": counts.2,
            "downgrade": counts.3,
        },
    });
    PkView {
        kind: plan_kind(dry_run),
        data,
        human: plan_human(verb, specs, host, counts, dry_run),
        hints: pk_hints(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_package_signal() {
        let args = vec![
            json!(INFO_INSTALLING),
            json!("htop;3.4.1-3.fc44;x86_64;fedora"),
            json!("Interactive process viewer"),
        ];
        let p = PkPackage::from_signal(&args).unwrap();
        assert_eq!(p.info, INFO_INSTALLING);
        assert_eq!(p.name, "htop");
        assert_eq!(p.version, "3.4.1-3.fc44");
        assert_eq!(p.arch, "x86_64");
        assert_eq!(p.repo(), "fedora");
        assert_eq!(p.label(), "htop-3.4.1-3.fc44.x86_64");
        assert_eq!(p.package_id(), "htop;3.4.1-3.fc44;x86_64;fedora");
    }

    #[test]
    fn malformed_package_signal_is_none() {
        assert!(PkPackage::from_signal(&[]).is_none());
        assert!(PkPackage::from_signal(&[json!(INFO_INSTALLING)]).is_none());
    }

    #[test]
    fn pk_repo_parses_repo_detail_signal_args() {
        let args = vec![json!("updates"), json!("Fedora Updates"), json!(true)];

        let repo = PkRepo::from_signal(&args).expect("valid RepoDetail args");

        assert_eq!(repo.id, "updates");
        assert_eq!(repo.name, "Fedora Updates");
        assert!(repo.enabled);
        assert_eq!(repo.row(), json!(["updates", "Fedora Updates", true]));
    }

    #[test]
    fn pk_repo_rejects_malformed_repo_detail_signal_args() {
        assert!(PkRepo::from_signal(&[json!("updates"), json!("Fedora Updates")]).is_none());
        assert!(
            PkRepo::from_signal(&[json!("updates"), json!("Fedora Updates"), json!("true")])
                .is_none()
        );
    }

    #[test]
    fn error_code_six_is_access_denied() {
        let signals = vec![
            (
                "ErrorCode".to_string(),
                vec![json!(6), json!("not authorized")],
            ),
            ("Finished".to_string(), vec![json!(4), json!(10)]),
        ];
        let err = check_stream(&signals).unwrap_err();
        assert!(matches!(err, FezError::AccessDenied { .. }));
    }

    #[test]
    fn other_error_code_is_dbus() {
        let signals = vec![(
            "ErrorCode".to_string(),
            vec![json!(4), json!("package not found")],
        )];
        let err = check_stream(&signals).unwrap_err();
        assert!(matches!(err, FezError::Dbus { .. }));
    }

    #[test]
    fn clean_stream_is_ok() {
        let signals = vec![
            (
                "Package".to_string(),
                vec![json!(8), json!("htop;1;x86_64;fedora"), json!("")],
            ),
            ("Finished".to_string(), vec![json!(1), json!(20)]),
        ];
        assert!(check_stream(&signals).is_ok());
    }

    #[test]
    fn plan_buckets_by_info_enum() {
        let signals = vec![
            (
                "Package".to_string(),
                vec![
                    json!(INFO_INSTALLING),
                    json!("nginx;1;x86_64;fedora"),
                    json!(""),
                ],
            ),
            (
                "Package".to_string(),
                vec![
                    json!(INFO_REMOVING),
                    json!("htop;1;x86_64;installed"),
                    json!(""),
                ],
            ),
            (
                "Package".to_string(),
                vec![
                    json!(INFO_UPDATING),
                    json!("bash;2;x86_64;updates"),
                    json!(""),
                ],
            ),
            ("Finished".to_string(), vec![json!(1), json!(20)]),
        ];
        let plan = plan_from(&signals);
        assert_eq!(plan.install, vec!["nginx-1.x86_64"]);
        assert_eq!(plan.remove, vec!["htop-1.x86_64"]);
        assert_eq!(plan.remove_names, vec!["htop"]);
        assert_eq!(plan.upgrade, vec!["bash-2.x86_64"]);
        assert!(plan.downgrade.is_empty());
    }

    #[test]
    fn plan_action_maps_known_packagekit_info_values() {
        assert_eq!(
            PkPlanAction::from_info(INFO_INSTALLING),
            Some(PkPlanAction::Install)
        );
        assert_eq!(
            PkPlanAction::from_info(INFO_REMOVING),
            Some(PkPlanAction::Remove)
        );
        assert_eq!(
            PkPlanAction::from_info(INFO_OBSOLETING),
            Some(PkPlanAction::Remove)
        );
        assert_eq!(
            PkPlanAction::from_info(INFO_UPDATING),
            Some(PkPlanAction::Upgrade)
        );
        assert_eq!(
            PkPlanAction::from_info(INFO_DOWNGRADING),
            Some(PkPlanAction::Downgrade)
        );
        assert_eq!(PkPlanAction::from_info(999), None);
    }

    #[test]
    fn mutation_from_verb_rejects_unknown_verbs() {
        assert_eq!(
            PkMutation::from_verb("install").unwrap(),
            PkMutation::Install
        );
        assert_eq!(PkMutation::from_verb("remove").unwrap(), PkMutation::Remove);
        assert_eq!(
            PkMutation::from_verb("upgrade").unwrap(),
            PkMutation::Upgrade
        );
        assert!(matches!(
            PkMutation::from_verb("reinstall"),
            Err(FezError::Usage(_))
        ));
    }

    #[test]
    fn method_for_maps_verbs() {
        let ids = vec!["nginx;1;x86_64;fedora".to_string()];
        assert_eq!(
            method_for(PkMutation::Install, TF_NONE, &ids).0,
            "InstallPackages"
        );
        assert_eq!(
            method_for(PkMutation::Upgrade, TF_NONE, &ids).0,
            "UpdatePackages"
        );
        let (m, args) = method_for(PkMutation::Remove, TF_SIMULATE, &ids);
        assert_eq!(m, "RemovePackages");
        // RemovePackages carries (flags, ids, allow_deps, autoremove).
        assert_eq!(args.as_array().unwrap().len(), 4);
    }

    #[test]
    fn dry_run_plan_view_has_null_size_and_backend() {
        let plan = PkPlan {
            install: vec!["nginx-1.x86_64".into()],
            remove: vec![],
            upgrade: vec![],
            downgrade: vec![],
            remove_names: vec![],
        };
        let view = plan_view(
            PkMutation::Install,
            "host",
            &["nginx".to_string()],
            &plan,
            true,
        );
        assert_eq!(view.kind, "PackagePlan");
        assert_eq!(view.data["backend"], json!("packagekit"));
        assert_eq!(view.data["install_size_total"], Value::Null);
        assert_eq!(view.data["dry_run"], json!(true));
        assert_eq!(view.data["counts"]["install"], json!(1));
    }
}
