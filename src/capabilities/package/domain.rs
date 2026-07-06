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

#[cfg(test)]
mod tests {
    use super::*;

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
}
