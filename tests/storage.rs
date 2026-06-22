use assert_cmd::cargo::CommandCargoExt;
use predicates::str::contains;
use serde_json::Value;
use std::process::Command;

mod common;
use common::fez_fake;

// ── storage list ────────────────────────────────────────────────────────

#[test]
fn list_json_returns_all_block_devices() {
    fez_fake()
        .args(["storage", "list", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"StorageDeviceList\""))
        .stdout(contains("nvme0n1"))
        .stdout(contains("nvme0n1p1"))
        .stdout(contains("nvme0n1p2"))
        .stdout(contains("nvme0n1p3"))
        .stdout(contains("dm-0"));
}

#[test]
fn list_human_renders_table() {
    fez_fake()
        .args(["storage", "list"])
        .assert()
        .success()
        .stdout(contains("DEVICE"))
        .stdout(contains("vfat"))
        .stdout(contains("ext4"))
        .stdout(contains("/boot/efi"))
        .stdout(contains("/boot"));
}

// ── storage show ────────────────────────────────────────────────────────

#[test]
fn show_json_partition_detail() {
    fez_fake()
        .args(["storage", "show", "/dev/nvme0n1p1", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"StorageDeviceDetail\""))
        .stdout(contains("vfat"))
        .stdout(contains("D7E3-1F6A"))
        .stdout(contains("/boot/efi"))
        .stdout(contains("EFI System Partition"))
        .stdout(contains("Samsung SSD 990 PRO 4TB"));
}

#[test]
fn show_json_whole_disk_has_partition_table() {
    fez_fake()
        .args(["storage", "show", "/dev/nvme0n1", "--json"])
        .assert()
        .success()
        .stdout(contains("\"partition_table\""))
        .stdout(contains("\"gpt\""));
}

#[test]
fn show_json_encrypted_device() {
    fez_fake()
        .args(["storage", "show", "/dev/nvme0n1p3", "--json"])
        .assert()
        .success()
        .stdout(contains("crypto_LUKS"))
        .stdout(contains("\"encrypted\""))
        .stdout(contains("dm_2d0"));
}

#[test]
fn show_human_renders_detail() {
    fez_fake()
        .args(["storage", "show", "/dev/nvme0n1p1"])
        .assert()
        .success()
        .stdout(contains("Device:"))
        .stdout(contains("vfat"))
        .stdout(contains("/boot/efi"))
        .stdout(contains("SSD/NVMe"));
}

#[test]
fn show_short_name_matches() {
    // "nvme0n1p2" without /dev/ prefix should still match.
    fez_fake()
        .args(["storage", "show", "nvme0n1p2"])
        .assert()
        .success()
        .stdout(contains("ext4"));
}

#[test]
fn show_unknown_device_exits_4() {
    fez_fake()
        .args(["storage", "show", "doesnotexist"])
        .assert()
        .code(4);
}

// ── storage health ──────────────────────────────────────────────────────

#[test]
fn health_json_returns_drive_health() {
    fez_fake()
        .args(["storage", "health", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"StorageHealth\""))
        .stdout(contains("Samsung SSD 990 PRO 4TB"))
        .stdout(contains("S7KGNU0X524885F"))
        .stdout(contains("5522"))
        .stdout(contains("322"))
        .stdout(contains("success"));
}

#[test]
fn health_human_renders_drive_info() {
    fez_fake()
        .args(["storage", "health"])
        .assert()
        .success()
        .stdout(contains("Drive:"))
        .stdout(contains("Samsung SSD 990 PRO 4TB"))
        .stdout(contains("49°C"))
        .stdout(contains("5522 hours"))
        .stdout(contains("Warnings:    none"));
}

#[test]
fn health_filter_matches_serial() {
    fez_fake()
        .args(["storage", "health", "--drive", "S7KGNU0X524885F"])
        .assert()
        .success()
        .stdout(contains("Samsung SSD 990 PRO 4TB"));
}

#[test]
fn health_filter_no_match_exits_4() {
    fez_fake()
        .args(["storage", "health", "--drive", "nonexistent"])
        .assert()
        .code(4);
}

// ── Envelope shape stability ────────────────────────────────────────────

#[test]
fn list_json_shape_stays_stable() {
    let output = Command::cargo_bin("fez")
        .unwrap()
        .env("FEZ_BRIDGE", env!("CARGO_BIN_EXE_fez-fake-bridge"))
        .args(["storage", "list", "--json"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();

    assert_eq!(envelope["apiVersion"], "fez/v1");
    assert_eq!(envelope["kind"], "StorageDeviceList");
    assert_eq!(envelope["status"], "ok");

    let cols = &envelope["data"]["columns"];
    assert_eq!(
        cols,
        &serde_json::json!([
            "device",
            "size",
            "fs_type",
            "label",
            "uuid",
            "mountpoint",
            "read_only"
        ])
    );

    let rows = envelope["data"]["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 5);

    // Rows are sorted by device path; /dev/dm-0 comes first.
    assert_eq!(rows[0][0], "/dev/dm-0");
    assert_eq!(rows[1][0], "/dev/nvme0n1");
    assert_eq!(rows[2][0], "/dev/nvme0n1p1");
}

#[test]
fn show_json_shape_stays_stable() {
    let output = Command::cargo_bin("fez")
        .unwrap()
        .env("FEZ_BRIDGE", env!("CARGO_BIN_EXE_fez-fake-bridge"))
        .args(["storage", "show", "/dev/nvme0n1p1", "--json"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();

    assert_eq!(envelope["apiVersion"], "fez/v1");
    assert_eq!(envelope["kind"], "StorageDeviceDetail");
    assert_eq!(envelope["status"], "ok");

    let data = &envelope["data"];
    assert_eq!(data["device"], "/dev/nvme0n1p1");
    assert_eq!(data["fs_type"], "vfat");
    assert_eq!(data["uuid"], "D7E3-1F6A");
    assert_eq!(data["mountpoints"], serde_json::json!(["/boot/efi"]));
    assert_eq!(data["partition"]["name"], "EFI System Partition");
    assert_eq!(data["drive"]["model"], "Samsung SSD 990 PRO 4TB");
}

#[test]
fn health_json_shape_stays_stable() {
    let output = Command::cargo_bin("fez")
        .unwrap()
        .env("FEZ_BRIDGE", env!("CARGO_BIN_EXE_fez-fake-bridge"))
        .args(["storage", "health", "--json"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();

    assert_eq!(envelope["apiVersion"], "fez/v1");
    assert_eq!(envelope["kind"], "StorageHealth");
    assert_eq!(envelope["status"], "ok");

    let data = envelope["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);

    let drive = &data[0];
    assert_eq!(drive["model"], "Samsung SSD 990 PRO 4TB");
    assert_eq!(drive["serial"], "S7KGNU0X524885F");
    assert_eq!(drive["power_on_hours"], 5522);
    assert_eq!(drive["temperature_kelvin"], 322);
    assert_eq!(drive["selftest_status"], "success");
    assert_eq!(drive["state"], "live");
    assert_eq!(drive["critical_warnings"], serde_json::json!([]));
}
