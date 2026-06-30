use predicates::str::contains;

mod common;
use common::{fez_fake, fez_fake_quiet, AuditLog};

#[test]
fn reboot_without_force_exits_8() {
    fez_fake_quiet()
        .args(["system", "reboot"])
        .assert()
        .code(8)
        .stderr(contains("protected"));
}

#[test]
fn reboot_without_force_json_exits_8() {
    fez_fake_quiet()
        .args(["system", "reboot", "--json"])
        .assert()
        .code(8)
        .stdout(contains("\"code\":\"protected-unit\""));
}

#[test]
fn reboot_with_force_succeeds() {
    let audit = AuditLog::new("power-reboot");
    fez_fake()
        .env("FEZ_AUDIT", audit.env_value())
        .args(["system", "reboot", "--force"])
        .assert()
        .success()
        .stdout(contains("reboot initiated"));

    let records = audit.records();
    assert!(records
        .iter()
        .any(|r| r["operation"] == "system-reboot" && r["result"] == "attempt"));
    assert!(records
        .iter()
        .any(|r| r["operation"] == "system-reboot" && r["result"] == "ok"));
}

#[test]
fn reboot_json_with_force_returns_envelope() {
    fez_fake_quiet()
        .args(["system", "reboot", "--force", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"PowerAction\""))
        .stdout(contains("\"action\":\"reboot\""));
}

#[test]
fn poweroff_with_force_succeeds() {
    fez_fake_quiet()
        .args(["system", "poweroff", "--force"])
        .assert()
        .success()
        .stdout(contains("poweroff initiated"));
}

#[test]
fn suspend_exits_9_not_available() {
    // CanSuspend returns "na" in the fake bridge
    fez_fake_quiet()
        .args(["system", "suspend", "--force"])
        .assert()
        .code(9)
        .stderr(contains("missing dependency logind suspend"));
}

#[test]
fn suspend_json_exits_9_with_envelope() {
    fez_fake_quiet()
        .args(["system", "suspend", "--force", "--json"])
        .assert()
        .code(9)
        .stdout(contains("\"code\":\"dependency-missing\""));
}

#[test]
fn reboot_escalation_off_exits_11() {
    fez_fake_quiet()
        .env("FEZ_ESCALATION", "off")
        .args(["system", "reboot", "--force"])
        .assert()
        .code(11);
}
