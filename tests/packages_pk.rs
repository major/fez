//! Integration tests for the PackageKit fallback backend (dnf5daemon absent).
//!
//! Each test forces dnf5daemon absent via `fez_fake_pk` so the `packages`
//! capability falls back to PackageKit, then asserts the PackageKit-backed
//! results: the `backend` marker, null sizes, the degraded-schema hint, and the
//! guardrail / escalation exit codes.
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use serde_json::{json, Value};

mod common;
use common::fez_fake_pk;

#[test]
fn list_falls_back_to_packagekit() {
    fez_fake_pk()
        .args(["packages", "list", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"PackageList\""))
        .stdout(contains("\"backend\":\"packagekit\""))
        .stdout(contains(
            "\"columns\":[\"name\",\"evr\",\"arch\",\"repo_id\",\"install_size\",\"summary\"]",
        ))
        .stdout(contains("htop"));
}

#[test]
fn list_sizes_are_null_on_packagekit() {
    // PackageKit carries no install_size; the column is null, not 0. The payload
    // is columnar (positional rows), so install_size lives at the column after
    // repo_id. Anchor the null to that position (`"<repo_id>",null,`) so the
    // test fails if some other field is the null one. `installed` is the repo_id
    // the fake reports for its installed packages.
    fez_fake_pk()
        .args(["packages", "list", "--json"])
        .assert()
        .success()
        .stdout(contains("\"installed\",null,"));
}

#[test]
fn list_carries_degraded_schema_hint() {
    fez_fake_pk()
        .args(["packages", "list", "--json"])
        .assert()
        .success()
        .stdout(contains("\"hints\""))
        .stdout(contains("sizes are unavailable"));
}

#[test]
fn search_via_packagekit() {
    fez_fake_pk()
        .args(["packages", "search", "nginx", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"PackageSearch\""))
        .stdout(contains("\"backend\":\"packagekit\""))
        .stdout(contains(
            "\"columns\":[\"name\",\"evr\",\"arch\",\"repo_id\",\"install_size\",\"summary\"]",
        ))
        .stdout(contains("nginx"));
}

#[test]
fn info_via_packagekit() {
    fez_fake_pk()
        .args(["packages", "info", "nginx", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"PackageInfo\""))
        .stdout(contains("\"backend\":\"packagekit\""))
        .stdout(contains("nginx"));
}

#[test]
fn check_update_via_packagekit() {
    fez_fake_pk()
        .args(["packages", "check-update", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"PackageUpdates\""))
        .stdout(contains("\"backend\":\"packagekit\""))
        .stdout(contains(
            "\"columns\":[\"name\",\"evr\",\"arch\",\"repo_id\",\"install_size\",\"summary\"]",
        ));
}

#[test]
fn repolist_filters_to_enabled_by_default() {
    // The fake advertises fedora+updates (enabled) and crb (disabled). The
    // default filter keeps only the enabled repos.
    fez_fake_pk()
        .args(["packages", "repolist", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"RepoList\""))
        .stdout(contains("\"columns\":[\"id\",\"name\",\"enabled\"]"))
        .stdout(contains("fedora"))
        .stdout(contains("updates"))
        .stdout(contains("crb").not());
}

#[test]
fn install_dry_run_via_packagekit() {
    let output = fez_fake_pk()
        .args(["packages", "install", "nginx", "--dry-run", "--json"])
        .output()
        .expect("run fez");
    assert!(output.status.success());
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("valid JSON envelope");
    let data = &envelope["data"];

    assert_eq!(envelope["kind"], "PackagePlan");
    assert_eq!(data["backend"], "packagekit");
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["operation"], "install");
    assert_eq!(data["install_size_total"], Value::Null);
    assert_eq!(
        data["counts"],
        json!({
            "install": 2,
            "remove": 0,
            "upgrade": 0,
            "downgrade": 0,
        })
    );
}

#[test]
fn install_executes_via_packagekit() {
    // Default fake host has passwordless sudo, so escalation succeeds.
    fez_fake_pk()
        .args(["packages", "install", "nginx", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"PackageMutation\""))
        .stdout(contains("\"backend\":\"packagekit\""));
}

#[test]
fn remove_protected_is_blocked_exit_10() {
    // FEZ_FAKE_PK_PLAN=protected adds `systemd` to the removal plan; the
    // removal guardrail refuses the dangerous transaction (exit 10).
    fez_fake_pk()
        .env("FEZ_FAKE_PK_PLAN", "protected")
        .args(["packages", "remove", "htop", "--json"])
        .assert()
        .code(10)
        .stdout(contains("\"code\":\"dangerous-transaction\""));
}

#[test]
fn not_authorized_maps_to_exit_11() {
    fez_fake_pk()
        .env("FEZ_FAKE_PK_ERROR", "notauth")
        .args(["packages", "install", "nginx", "--json"])
        .assert()
        .code(11)
        .stdout(contains("\"code\":\"access-denied\""));
}

#[test]
fn both_backends_absent_is_exit_9() {
    fez_fake_pk()
        .env("FEZ_FAKE_NO_PACKAGEKIT", "1")
        .args(["packages", "list", "--json"])
        .assert()
        .code(9)
        .stdout(contains("\"code\":\"dependency-missing\""))
        .stdout(contains("PackageKit"));
}

#[test]
fn no_escalation_mutation_is_exit_11() {
    // A host advertising no escalation mechanism cannot open the privileged
    // channel PackageKit mutations need.
    fez_fake_pk()
        .env("FEZ_FAKE_BRIDGES", "")
        .args(["packages", "install", "nginx", "--json"])
        .assert()
        .code(11)
        .stdout(contains("\"code\":\"access-denied\""));
}
