use assert_cmd::Command as AssertCommand;
use predicates::str::contains;

fn fez_fake() -> AssertCommand {
    let mut c = AssertCommand::cargo_bin("fez").unwrap();
    c.env("FEZ_BRIDGE", env!("CARGO_BIN_EXE_fez-fake-bridge"));
    c
}

#[test]
fn packages_list_json_has_packages() {
    fez_fake()
        .args(["packages", "list", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"PackageList\""))
        // Columnar shape: column names stated once, items as positional rows.
        .stdout(contains(
            "\"columns\":[\"name\",\"evr\",\"arch\",\"repo_id\",\"install_size\",\"summary\"]",
        ))
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
        .stdout(contains(
            "\"columns\":[\"name\",\"evr\",\"arch\",\"repo_id\",\"install_size\",\"summary\"]",
        ))
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
        .stdout(contains(
            "\"columns\":[\"name\",\"evr\",\"arch\",\"repo_id\",\"install_size\",\"summary\"]",
        ))
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
fn packages_advisories_lists_errata() {
    fez_fake()
        .args(["packages", "advisories", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"AdvisoryList\""))
        .stdout(contains(
            "\"columns\":[\"name\",\"title\",\"type\",\"severity\",\"buildtime\"]",
        ))
        .stdout(contains("\"rows\":"))
        .stdout(contains("\"count\":"))
        .stdout(contains(
            "[\"FEDORA-2025-aaaa\",\"kernel security update\",\"security\",\"important\",",
        ))
        .stdout(contains("\"bugfix\""));
}

#[test]
fn packages_history_lists_recent_changes() {
    fez_fake()
        .args(["packages", "history", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"PackageHistory\""))
        .stdout(contains("\"columns\":[\"change\",\"name\",\"evr\",\"arch\"]"))
        .stdout(contains("\"rows\":"))
        .stdout(contains("\"count\":"))
        .stdout(contains("installed"))
        .stdout(contains("htop"));
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
fn packages_distro_sync_dry_run_emits_plan() {
    fez_fake()
        .env("FEZ_FAKE_PLAN", "install")
        .env("FEZ_AUDIT", "off")
        .args(["packages", "distro-sync", "--dry-run", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"PackagePlan\""))
        .stdout(contains("\"dry_run\":true"))
        .stdout(contains("\"operation\":\"distro-sync\""));
}

#[test]
fn packages_distro_sync_emits_mutation() {
    fez_fake()
        .env("FEZ_AUDIT", "off")
        .env("FEZ_FAKE_PLAN", "install")
        .args(["packages", "distro-sync", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"PackageMutation\""))
        .stdout(contains("\"operation\":\"distro-sync\""));
}

#[test]
fn packages_downgrade_dry_run_emits_plan() {
    fez_fake()
        .env("FEZ_FAKE_PLAN", "install")
        .env("FEZ_AUDIT", "off")
        .args(["packages", "downgrade", "nginx", "--dry-run", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"PackagePlan\""))
        .stdout(contains("\"dry_run\":true"))
        .stdout(contains("\"operation\":\"downgrade\""));
}

#[test]
fn packages_downgrade_emits_mutation() {
    fez_fake()
        .env("FEZ_AUDIT", "off")
        .env("FEZ_FAKE_PLAN", "install")
        .args(["packages", "downgrade", "nginx", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"PackageMutation\""))
        .stdout(contains("\"operation\":\"downgrade\""));
}

#[test]
fn packages_reinstall_dry_run_emits_plan() {
    fez_fake()
        .env("FEZ_FAKE_PLAN", "install")
        .env("FEZ_AUDIT", "off")
        .args(["packages", "reinstall", "bash", "--dry-run", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"PackagePlan\""))
        .stdout(contains("\"dry_run\":true"))
        .stdout(contains("\"operation\":\"reinstall\""));
}

#[test]
fn packages_reinstall_emits_mutation() {
    fez_fake()
        .env("FEZ_AUDIT", "off")
        .env("FEZ_FAKE_PLAN", "install")
        .args(["packages", "reinstall", "bash", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"PackageMutation\""))
        .stdout(contains("\"operation\":\"reinstall\""));
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
    fez_fake()
        .env("FEZ_FAKE_NO_DNF5", "1")
        .args(["packages", "list", "--json"])
        .assert()
        .code(9)
        .stdout(contains("\"code\":\"dependency-missing\""))
        .stdout(contains("dnf5daemon"))
        .stdout(contains("\"remediation\""));
}

#[test]
fn packages_mutation_writes_attempt_and_result_audit_records() {
    let path = std::env::temp_dir().join(format!("fez-pkg-audit-{}.jsonl", std::process::id()));
    let _ = std::fs::remove_file(&path);
    fez_fake()
        .env("FEZ_AUDIT", format!("file:{}", path.display()))
        .env("FEZ_FAKE_PLAN", "small")
        .args(["packages", "remove", "htop", "--json"])
        .assert()
        .success();
    let body = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = body.lines().collect();
    assert_eq!(lines.len(), 2, "expected attempt + result records");
    let attempt: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    let result: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(attempt["result"], "attempt");
    assert_eq!(result["result"], "ok");
    assert_eq!(result["operation"], "remove");
    let _ = std::fs::remove_file(&path);
}
