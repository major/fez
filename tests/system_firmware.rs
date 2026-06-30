use predicates::str::contains;
use serde_json::Value;

mod common;
use common::{fez_fake, fez_fake_quiet};

#[test]
fn firmware_list_json_returns_devices() {
    fez_fake()
        .args(["system", "firmware", "list", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"FirmwareDeviceList\""))
        .stdout(contains("\"name\":\"UEFI Device Firmware\""))
        .stdout(contains("\"vendor\":\"Dell Inc.\""))
        .stdout(contains("\"version\":\"1.10.1\""))
        .stdout(contains("\"updatable\":true"))
        .stdout(contains("\"name\":\"System Firmware\""))
        .stdout(contains("\"updatable\":false"));
}

#[test]
fn firmware_list_human_renders_table() {
    let out = fez_fake()
        .args(["system", "firmware", "list"])
        .output()
        .expect("run fez");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("NAME"));
    assert!(stdout.contains("UEFI Device Firmware"));
    assert!(stdout.contains("Dell Inc."));
    assert!(stdout.contains("1.10.1"));
}

#[test]
fn firmware_security_json_returns_hsi() {
    fez_fake()
        .args(["system", "firmware", "security", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"FirmwareSecurityReport\""))
        .stdout(contains("\"hsi_score\":\"HSI:1 (v2.1.3)\""))
        .stdout(contains("\"name\":\"TPM v2.0\""))
        .stdout(contains("\"result\":\"enabled\""))
        .stdout(contains("\"name\":\"IOMMU\""))
        .stdout(contains("\"result\":\"not-enabled\""));
}

#[test]
fn firmware_security_human_shows_score() {
    let out = fez_fake()
        .args(["system", "firmware", "security"])
        .output()
        .expect("run fez");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("HSI:1"));
    assert!(stdout.contains("TPM v2.0"));
    assert!(stdout.contains("Secure Boot"));
    assert!(stdout.contains("IOMMU"));
}

#[test]
fn firmware_upgrades_json_returns_upgrades() {
    fez_fake()
        .args(["system", "firmware", "upgrades", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"FirmwareUpgradeList\""))
        .stdout(contains("\"device\":\"UEFI Device Firmware\""))
        .stdout(contains("\"version\":\"1.11.0\""))
        .stdout(contains("\"current_version\":\"1.10.1\""))
        .stdout(contains("\"description\":\"Security fix\""));
}

#[test]
fn firmware_upgrades_human_shows_upgrade() {
    let out = fez_fake()
        .args(["system", "firmware", "upgrades"])
        .output()
        .expect("run fez");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("UEFI Device Firmware"));
    assert!(stdout.contains("1.10.1"));
    assert!(stdout.contains("1.11.0"));
}

#[test]
fn firmware_absent_exits_9() {
    fez_fake_quiet()
        .env("FEZ_FAKE_NO_FWUPD", "1")
        .args(["system", "firmware", "list"])
        .assert()
        .code(9)
        .stderr(contains("fwupd"));
}

#[test]
fn firmware_absent_json_exits_9() {
    fez_fake_quiet()
        .env("FEZ_FAKE_NO_FWUPD", "1")
        .args(["system", "firmware", "list", "--json"])
        .assert()
        .code(9)
        .stdout(contains("\"code\":\"dependency-missing\""))
        .stdout(contains("fwupd"));
}

#[test]
fn firmware_security_json_has_expected_shape() {
    let out = fez_fake()
        .args(["system", "firmware", "security", "--json"])
        .output()
        .expect("run fez");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let env: Value = serde_json::from_str(&stdout).expect("parse JSON");
    assert_eq!(env["apiVersion"], "fez/v1");
    assert_eq!(env["kind"], "FirmwareSecurityReport");
    let attrs = env["data"]["attributes"].as_array().unwrap();
    assert_eq!(attrs.len(), 3);
}

#[test]
fn describe_firmware_list_returns_descriptor() {
    fez_fake()
        .args(["describe", "system.firmware.list", "--json"])
        .assert()
        .success()
        .stdout(contains("\"id\":\"system.firmware.list\""))
        .stdout(contains("\"output_kind\":\"FirmwareDeviceList\""));
}

#[test]
fn capabilities_lists_firmware_commands() {
    fez_fake()
        .args(["capabilities"])
        .assert()
        .success()
        .stdout(contains("system.firmware.list"))
        .stdout(contains("system.firmware.security"))
        .stdout(contains("system.firmware.upgrades"));
}
