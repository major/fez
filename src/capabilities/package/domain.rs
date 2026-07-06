//! Backend-neutral package read payload helpers.

use serde_json::{json, Value};

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
}
