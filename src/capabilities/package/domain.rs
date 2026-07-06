//! Backend-neutral package payload helpers.

use serde_json::{json, Value};

/// Backend marker for dnf5daemon payloads.
pub(super) const DNF5_BACKEND: &str = "dnf5daemon";

/// Backend marker for PackageKit payloads.
pub(super) const PACKAGEKIT_BACKEND: &str = "packagekit";

/// Column order for package list/search/update payloads.
pub(super) const PACKAGE_COLUMNS: &[&str] =
    &["name", "evr", "arch", "repo_id", "install_size", "summary"];

/// Column order for repository list payloads.
pub(super) const REPO_COLUMNS: &[&str] = &["id", "name", "enabled"];

/// Backend-neutral package adapter for shared JSON row/object builders.
pub(super) trait PackageRow {
    /// Package name.
    fn name(&self) -> &str;
    /// Package epoch-version-release or backend equivalent.
    fn evr(&self) -> &str;
    /// Package architecture.
    fn arch(&self) -> &str;
    /// Repository id or backend data field.
    fn repo_id(&self) -> &str;
    /// Installed/download size payload; PackageKit uses `null`.
    fn install_size(&self) -> Value;
    /// Package summary.
    fn summary(&self) -> &str;

    /// One positional package row aligned to [`PACKAGE_COLUMNS`].
    fn package_row(&self) -> Value {
        package_row(
            self.name(),
            self.evr(),
            self.arch(),
            self.repo_id(),
            self.install_size(),
            self.summary(),
        )
    }

    /// Record-shaped package info payload without the backend marker.
    fn package_object(&self) -> Value {
        package_object(
            self.name(),
            self.evr(),
            self.arch(),
            self.repo_id(),
            self.install_size(),
            self.summary(),
        )
    }
}

impl<T: PackageRow + ?Sized> PackageRow for &T {
    fn name(&self) -> &str {
        (*self).name()
    }
    fn evr(&self) -> &str {
        (*self).evr()
    }
    fn arch(&self) -> &str {
        (*self).arch()
    }
    fn repo_id(&self) -> &str {
        (*self).repo_id()
    }
    fn install_size(&self) -> Value {
        (*self).install_size()
    }
    fn summary(&self) -> &str {
        (*self).summary()
    }
}

/// Backend-neutral repository adapter for shared JSON row builders.
pub(super) trait RepoRow {
    /// Repository id.
    fn id(&self) -> &str;
    /// Repository display name.
    fn name(&self) -> &str;
    /// Whether this repository is enabled.
    fn enabled(&self) -> bool;

    /// One positional repository row aligned to [`REPO_COLUMNS`].
    fn repo_row(&self) -> Value {
        repo_row(self.id(), self.name(), self.enabled())
    }
}

impl<T: RepoRow + ?Sized> RepoRow for &T {
    fn id(&self) -> &str {
        (*self).id()
    }
    fn name(&self) -> &str {
        (*self).name()
    }
    fn enabled(&self) -> bool {
        (*self).enabled()
    }
}

/// One positional package row aligned to [`PACKAGE_COLUMNS`].
pub(super) fn package_row(
    name: &str,
    evr: &str,
    arch: &str,
    repo_id: &str,
    install_size: Value,
    summary: &str,
) -> Value {
    json!([name, evr, arch, repo_id, install_size, summary])
}

/// Record-shaped package info payload without the backend marker.
pub(super) fn package_object(
    name: &str,
    evr: &str,
    arch: &str,
    repo_id: &str,
    install_size: Value,
    summary: &str,
) -> Value {
    json!({
        "name": name,
        "evr": evr,
        "arch": arch,
        "repo_id": repo_id,
        "install_size": install_size,
        "summary": summary,
    })
}

/// One positional repository row aligned to [`REPO_COLUMNS`].
pub(super) fn repo_row(id: &str, name: &str, enabled: bool) -> Value {
    json!([id, name, enabled])
}

/// Build package table data with the shared column contract.
pub(super) fn package_table(rows: Vec<Value>) -> Value {
    crate::envelope::table_data(PACKAGE_COLUMNS, rows)
}

/// Build repository table data with the shared column contract.
pub(super) fn repo_table(rows: Vec<Value>) -> Value {
    crate::envelope::table_data(REPO_COLUMNS, rows)
}

/// Build a stamped package table payload from package adapters.
pub(super) fn package_table_data<T>(items: impl IntoIterator<Item = T>, backend: &str) -> Value
where
    T: PackageRow,
{
    let rows = items.into_iter().map(|item| item.package_row()).collect();
    let mut data = package_table(rows);
    stamp_backend(&mut data, backend);
    data
}

/// Build a stamped package list payload with shared pagination/filter fields.
pub(super) fn package_list_data<T>(
    page: impl IntoIterator<Item = T>,
    scope: &str,
    repos: &[String],
    name: Option<&str>,
    total: usize,
    limit: Option<usize>,
    offset: usize,
    backend: &str,
) -> Value
where
    T: PackageRow,
{
    let rows = page
        .into_iter()
        .map(|item| item.package_row())
        .collect::<Vec<_>>();
    let returned = rows.len();
    let start = offset.min(total);
    let next_offset = (start + returned < total).then_some(start + returned);
    let mut data = package_table(rows);
    data["scope"] = json!(scope);
    data["repos"] = json!(repos);
    data["name"] = json!(name);
    data["total"] = json!(total);
    data["returned"] = json!(returned);
    data["limit"] = json!(limit);
    data["offset"] = json!(offset);
    data["next_offset"] = json!(next_offset);
    stamp_backend(&mut data, backend);
    data
}

/// Build a stamped package search payload.
pub(super) fn package_search_data<T>(
    items: impl IntoIterator<Item = T>,
    pattern: &str,
    backend: &str,
) -> Value
where
    T: PackageRow,
{
    let mut data = package_table_data(items, backend);
    data["pattern"] = json!(pattern);
    data
}

/// Build a stamped repository table payload from repository adapters.
pub(super) fn repo_table_data<T>(items: impl IntoIterator<Item = T>, backend: &str) -> Value
where
    T: RepoRow,
{
    let rows = items.into_iter().map(|item| item.repo_row()).collect();
    let mut data = repo_table(rows);
    stamp_backend(&mut data, backend);
    data
}

/// Attach the common package backend marker to a payload.
pub(super) fn stamp_backend(data: &mut Value, backend: &str) {
    data["backend"] = json!(backend);
}

/// Hint emitted when callers request a very large unpaginated package table.
pub(super) fn large_result_hint(total: usize) -> Option<String> {
    (total > 1000).then(|| {
        format!(
            "This response has {total} rows. Prefer packages search <pattern>, use --name, or use --limit."
        )
    })
}

/// Convert collected text hints into the envelope representation.
pub(super) fn hints_array(hints: Vec<String>) -> Option<Value> {
    (!hints.is_empty()).then(|| json!(hints))
}

/// Backend-neutral view of a resolved mutation plan's package buckets.
pub(super) trait MutationPlanBuckets {
    /// Packages to install.
    fn install(&self) -> &[String];
    /// Packages to remove.
    fn remove(&self) -> &[String];
    /// Packages to upgrade.
    fn upgrade(&self) -> &[String];
    /// Packages to downgrade.
    fn downgrade(&self) -> &[String];
}

/// Counts tuple consumed by the shared human plan renderer.
pub(super) fn plan_counts(plan: &impl MutationPlanBuckets) -> (usize, usize, usize, usize) {
    (
        plan.install().len(),
        plan.remove().len(),
        plan.upgrade().len(),
        plan.downgrade().len(),
    )
}

/// Build the shared mutation-plan payload view data from a bucketed plan.
pub(super) fn mutation_plan_data_from_buckets(
    operation: &str,
    specs: &[String],
    dry_run: bool,
    backend: &str,
    plan: &impl MutationPlanBuckets,
    install_size_total: Value,
) -> Value {
    mutation_plan_data(
        operation,
        specs,
        dry_run,
        backend,
        plan.install(),
        plan.remove(),
        plan.upgrade(),
        plan.downgrade(),
        install_size_total,
    )
}

/// Build the shared mutation-plan payload body.
pub(super) fn plan_data(
    install: &[String],
    remove: &[String],
    upgrade: &[String],
    downgrade: &[String],
    install_size_total: Value,
) -> Value {
    json!({
        "install": install,
        "remove": remove,
        "upgrade": upgrade,
        "downgrade": downgrade,
        "install_size_total": install_size_total,
        "counts": {
            "install": install.len(),
            "remove": remove.len(),
            "upgrade": upgrade.len(),
            "downgrade": downgrade.len(),
        },
    })
}

/// Build the shared mutation-plan payload view data, including operation metadata.
pub(super) fn mutation_plan_data(
    operation: &str,
    specs: &[String],
    dry_run: bool,
    backend: &str,
    install: &[String],
    remove: &[String],
    upgrade: &[String],
    downgrade: &[String],
    install_size_total: Value,
) -> Value {
    let mut data = plan_data(install, remove, upgrade, downgrade, install_size_total);
    data["operation"] = json!(operation);
    data["specs"] = json!(specs);
    data["dry_run"] = json!(dry_run);
    stamp_backend(&mut data, backend);
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Pkg;

    impl PackageRow for Pkg {
        fn name(&self) -> &str {
            "bash"
        }
        fn evr(&self) -> &str {
            "5.2-1"
        }
        fn arch(&self) -> &str {
            "x86_64"
        }
        fn repo_id(&self) -> &str {
            "fedora"
        }
        fn install_size(&self) -> Value {
            json!(42)
        }
        fn summary(&self) -> &str {
            "Shell"
        }
    }

    struct Repo;

    impl RepoRow for Repo {
        fn id(&self) -> &str {
            "fedora"
        }
        fn name(&self) -> &str {
            "Fedora"
        }
        fn enabled(&self) -> bool {
            true
        }
    }

    struct TestPlan {
        install: Vec<String>,
        remove: Vec<String>,
        upgrade: Vec<String>,
        downgrade: Vec<String>,
    }

    impl MutationPlanBuckets for TestPlan {
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

    #[test]
    fn package_helpers_preserve_columns_row_and_object_shape() {
        assert_eq!(
            PACKAGE_COLUMNS,
            &["name", "evr", "arch", "repo_id", "install_size", "summary"]
        );
        assert_eq!(
            package_row("bash", "5.2-1", "x86_64", "fedora", json!(42), "Shell"),
            json!(["bash", "5.2-1", "x86_64", "fedora", 42, "Shell"])
        );
        assert_eq!(
            package_object("bash", "5.2-1", "x86_64", "fedora", json!(42), "Shell"),
            json!({
                "name": "bash",
                "evr": "5.2-1",
                "arch": "x86_64",
                "repo_id": "fedora",
                "install_size": 42,
                "summary": "Shell"
            })
        );
    }

    #[test]
    fn repo_helpers_preserve_columns_and_row_shape() {
        assert_eq!(REPO_COLUMNS, &["id", "name", "enabled"]);
        assert_eq!(
            repo_row("fedora", "Fedora", true),
            json!(["fedora", "Fedora", true])
        );
    }

    #[test]
    fn backend_and_hint_helpers_preserve_shared_contract() {
        let mut data = package_table(Vec::new());
        stamp_backend(&mut data, DNF5_BACKEND);
        assert_eq!(data["backend"], json!("dnf5daemon"));

        assert_eq!(large_result_hint(1000), None);
        assert_eq!(
            large_result_hint(1001),
            Some(
                "This response has 1001 rows. Prefer packages search <pattern>, use --name, or use --limit."
                    .to_string()
            )
        );
        assert_eq!(hints_array(Vec::new()), None);
        assert_eq!(hints_array(vec!["hint".into()]), Some(json!(["hint"])));
    }

    #[test]
    fn package_list_builder_preserves_shared_pagination_contract() {
        let data = package_list_data(
            [&Pkg],
            "available",
            &["fedora".into()],
            Some("ba"),
            3,
            Some(1),
            1,
            DNF5_BACKEND,
        );

        assert_eq!(
            data["rows"],
            json!([["bash", "5.2-1", "x86_64", "fedora", 42, "Shell"]])
        );
        assert_eq!(data["scope"], json!("available"));
        assert_eq!(data["repos"], json!(["fedora"]));
        assert_eq!(data["name"], json!("ba"));
        assert_eq!(data["total"], json!(3));
        assert_eq!(data["returned"], json!(1));
        assert_eq!(data["limit"], json!(1));
        assert_eq!(data["offset"], json!(1));
        assert_eq!(data["next_offset"], json!(2));
        assert_eq!(data["backend"], json!(DNF5_BACKEND));
    }

    #[test]
    fn package_search_and_repo_builders_stamp_backend() {
        let search = package_search_data([&Pkg], "bash", PACKAGEKIT_BACKEND);
        assert_eq!(search["pattern"], json!("bash"));
        assert_eq!(search["backend"], json!(PACKAGEKIT_BACKEND));

        let repos = repo_table_data([&Repo], DNF5_BACKEND);
        assert_eq!(repos["rows"], json!([["fedora", "Fedora", true]]));
        assert_eq!(repos["backend"], json!(DNF5_BACKEND));
    }

    #[test]
    fn plan_data_preserves_shared_mutation_shape() {
        assert_eq!(
            plan_data(
                &["bash-1.x86_64".into()],
                &["old".into()],
                &["kernel".into()],
                &["lib".into()],
                json!(42)
            ),
            json!({
                "install": ["bash-1.x86_64"],
                "remove": ["old"],
                "upgrade": ["kernel"],
                "downgrade": ["lib"],
                "install_size_total": 42,
                "counts": {
                    "install": 1,
                    "remove": 1,
                    "upgrade": 1,
                    "downgrade": 1,
                },
            })
        );
    }

    #[test]
    fn mutation_plan_data_adds_operation_metadata_and_backend() {
        let specs = vec!["bash".to_string()];
        let data = mutation_plan_data(
            "install",
            &specs,
            true,
            DNF5_BACKEND,
            &["bash-1.x86_64".into()],
            &[],
            &[],
            &[],
            json!(42),
        );

        assert_eq!(data["operation"], json!("install"));
        assert_eq!(data["specs"], json!(["bash"]));
        assert_eq!(data["dry_run"], json!(true));
        assert_eq!(data["backend"], json!(DNF5_BACKEND));
        assert_eq!(data["counts"]["install"], json!(1));
    }

    #[test]
    fn mutation_plan_bucket_helpers_preserve_counts_and_payload_shape() {
        let specs = vec!["bash".to_string()];
        let plan = TestPlan {
            install: vec!["bash-1.x86_64".into()],
            remove: vec!["old".into()],
            upgrade: vec!["kernel".into()],
            downgrade: vec![],
        };

        assert_eq!(plan_counts(&plan), (1, 1, 1, 0));
        let data = mutation_plan_data_from_buckets(
            "install",
            &specs,
            false,
            PACKAGEKIT_BACKEND,
            &plan,
            Value::Null,
        );

        assert_eq!(data["install"], json!(["bash-1.x86_64"]));
        assert_eq!(data["remove"], json!(["old"]));
        assert_eq!(data["upgrade"], json!(["kernel"]));
        assert_eq!(data["downgrade"], json!([]));
        assert_eq!(data["install_size_total"], Value::Null);
        assert_eq!(data["counts"]["downgrade"], json!(0));
        assert_eq!(data["backend"], json!(PACKAGEKIT_BACKEND));
    }
}
