use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;

mod common;
use common::{fez_fake, AuditLog, PKG_COLUMNS};

#[test]
fn packages_list_json_has_packages() {
    fez_fake()
        .args(["packages", "list", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"PackageList\""))
        // Columnar shape: column names stated once, items as positional rows.
        .stdout(contains(PKG_COLUMNS))
        .stdout(contains("\"rows\":"))
        .stdout(contains("\"count\":"))
        .stdout(contains("\"scope\":\"installed\""))
        .stdout(contains("bash"))
        .stdout(contains("htop"));
}

#[test]
fn packages_list_human_default() {
    fez_fake()
        .args(["packages", "list"])
        .assert()
        .success()
        .stdout(contains("bash"))
        .stdout(contains("NAME"));
}

// Issue #59: --repo must restrict rows to the exact repo id. The fake returns
// bash/htop/nginx from `fedora` and vim-enhanced from `updates`; filtering to
// `fedora` must keep the three and drop vim-enhanced.
#[test]
fn packages_list_repo_filter_is_exact() {
    fez_fake()
        .args(["packages", "list", "--repo", "fedora", "--json"])
        .assert()
        .success()
        .stdout(contains("bash"))
        .stdout(contains("htop"))
        .stdout(contains("nginx"))
        .stdout(contains("vim-enhanced").not())
        // The requested filter is echoed back for transparency.
        .stdout(contains("\"repos\":[\"fedora\"]"));
}

// A repo with no matching rows yields an empty table, not the full set.
#[test]
fn packages_list_repo_filter_updates_only() {
    fez_fake()
        .args(["packages", "list", "--repo", "updates", "--json"])
        .assert()
        .success()
        .stdout(contains("vim-enhanced"))
        .stdout(contains("\"bash\"").not())
        .stdout(contains("\"nginx\"").not());
}

// Multiple --repo flags union (OR): a row is kept if its repo id is in the set.
#[test]
fn packages_list_multiple_repo_flags_union() {
    fez_fake()
        .args([
            "packages", "list", "--repo", "fedora", "--repo", "updates", "--json",
        ])
        .assert()
        .success()
        .stdout(contains("bash"))
        .stdout(contains("vim-enhanced"))
        .stdout(contains("\"repos\":[\"fedora\",\"updates\"]"));
}

// An unknown repo id matches nothing rather than falling back to all rows.
#[test]
fn packages_list_unknown_repo_is_empty() {
    fez_fake()
        .args(["packages", "list", "--repo", "no-such-repo", "--json"])
        .assert()
        .success()
        .stdout(contains("\"bash\"").not())
        .stdout(contains("vim-enhanced").not())
        .stdout(contains("\"count\":0"));
}

// No --repo means no filter: all rows from every repo are returned.
#[test]
fn packages_list_no_repo_returns_all() {
    fez_fake()
        .args(["packages", "list", "--json"])
        .assert()
        .success()
        .stdout(contains("bash"))
        .stdout(contains("vim-enhanced"));
}

#[test]
fn packages_info_json() {
    fez_fake()
        .args(["packages", "info", "bash", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"PackageInfo\""))
        .stdout(contains("\"name\":\"bash\""));
}

#[test]
fn packages_search_finds_nginx() {
    fez_fake()
        .args(["packages", "search", "ngin", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"PackageSearch\""))
        .stdout(contains(PKG_COLUMNS))
        .stdout(contains("\"pattern\":\"ngin\""))
        .stdout(contains("nginx"));
}

#[test]
fn packages_check_update_lists_updates() {
    fez_fake()
        .args(["packages", "check-update", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"PackageUpdates\""))
        .stdout(contains(PKG_COLUMNS))
        .stdout(contains("\"rows\":"))
        .stdout(contains("\"count\":"));
}

#[test]
fn packages_repolist_shows_enabled_state() {
    fez_fake()
        .args(["packages", "repolist", "--all", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"RepoList\""))
        .stdout(contains("\"columns\":[\"id\",\"name\",\"enabled\"]"))
        .stdout(contains("\"rows\":"))
        .stdout(contains("\"count\":"))
        .stdout(contains("fedora"));
}

#[test]
fn packages_install_dry_run_emits_plan() {
    fez_fake()
        .env("FEZ_FAKE_PLAN", "install")
        .env("FEZ_AUDIT", "off")
        .args(["packages", "install", "htop", "--dry-run", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"PackagePlan\""))
        .stdout(contains("\"dry_run\":true"))
        .stdout(contains("\"operation\":\"install\""));
}

#[test]
fn packages_remove_small_plan_succeeds() {
    fez_fake()
        .env("FEZ_FAKE_PLAN", "small")
        .env("FEZ_AUDIT", "off")
        .args(["packages", "remove", "htop", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"PackageMutation\""))
        .stdout(contains("\"operation\":\"remove\""));
}

#[test]
fn packages_upgrade_emits_mutation() {
    fez_fake()
        .env("FEZ_AUDIT", "off")
        .env("FEZ_FAKE_PLAN", "install")
        .args(["packages", "upgrade", "nginx", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"PackageMutation\""))
        .stdout(contains("\"operation\":\"upgrade\""));
}

#[test]
fn packages_upgrade_all_dry_run_emits_plan() {
    fez_fake()
        .env("FEZ_AUDIT", "off")
        .env("FEZ_FAKE_PLAN", "install")
        .args(["packages", "upgrade", "--dry-run", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"PackagePlan\""))
        .stdout(contains("\"dry_run\":true"));
}

#[test]
fn packages_remove_protected_human_error_to_stderr() {
    fez_fake()
        .env("FEZ_AUDIT", "off")
        .env("FEZ_FAKE_PLAN", "protected")
        .args(["packages", "remove", "glibc"])
        .assert()
        .code(10)
        .stderr(contains("error:"))
        .stderr(contains("dangerous transaction"));
}

#[test]
fn packages_remove_protected_refused_without_force() {
    fez_fake()
        .env("FEZ_FAKE_PLAN", "protected")
        .env("FEZ_AUDIT", "off")
        .args(["packages", "remove", "glibc", "--json"])
        .assert()
        .code(10)
        .stdout(contains("\"code\":\"dangerous-transaction\""))
        .stdout(contains("glibc"));
}

#[test]
fn packages_remove_protected_allowed_with_force() {
    fez_fake()
        .env("FEZ_FAKE_PLAN", "protected")
        .env("FEZ_AUDIT", "off")
        .args(["packages", "remove", "glibc", "--force", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"PackageMutation\""));
}

#[test]
fn packages_remove_cascade_refused_without_force() {
    fez_fake()
        .env("FEZ_FAKE_PLAN", "cascade")
        .env("FEZ_AUDIT", "off")
        .args(["packages", "remove", "leaf", "--json"])
        .assert()
        .code(10)
        .stdout(contains("\"code\":\"dangerous-transaction\""));
}

#[test]
fn packages_dependency_missing_returns_exit_9() {
    // Exit 9 now requires BOTH backends absent: with only dnf5daemon gone fez
    // falls back to PackageKit, so this test also forces PackageKit absent.
    fez_fake()
        .env("FEZ_FAKE_NO_DNF5", "1")
        .env("FEZ_FAKE_NO_PACKAGEKIT", "1")
        .args(["packages", "list", "--json"])
        .assert()
        .code(9)
        .stdout(contains("\"code\":\"dependency-missing\""))
        .stdout(contains("dnf5daemon"))
        .stdout(contains("\"remediation\""));
}

#[test]
fn packages_mutation_writes_attempt_and_result_audit_records() {
    let audit = AuditLog::new("pkg-audit");
    fez_fake()
        .env("FEZ_AUDIT", audit.env_value())
        .env("FEZ_FAKE_PLAN", "small")
        .args(["packages", "remove", "htop", "--json"])
        .assert()
        .success();
    let records = audit.records();
    assert_eq!(records.len(), 2, "expected attempt + result records");
    let (attempt, result) = (&records[0], &records[1]);
    assert_eq!(attempt["result"], "attempt");
    assert_eq!(result["result"], "ok");
    assert_eq!(result["operation"], "remove");
}
