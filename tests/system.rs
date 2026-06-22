use predicates::str::contains;
use serde_json::Value;

mod common;
use common::fez_fake;

// `system show --json` returns a SystemOverview envelope with all expected
// fields from hostname1 and timedate1 in a single JSON blob.
#[test]
fn show_json_returns_complete_overview() {
    fez_fake()
        .args(["system", "show", "--json"])
        .assert()
        .success()
        // Envelope
        .stdout(contains("\"kind\":\"SystemOverview\""))
        // Host identity
        .stdout(contains("\"hostname\":\"testbox.example.com\""))
        .stdout(contains(
            "\"machine_id\":\"b2692de9176e4abcb3342d4e31033c51\"",
        ))
        .stdout(contains("\"boot_id\":\"1363d915afea4a549f28c4e3258a3364\""))
        // OS
        .stdout(contains("\"os\":\"Fedora Linux 44 (Forty Four)\""))
        .stdout(contains("\"os_id\":\"fedora\""))
        .stdout(contains("\"os_version_id\":\"44\""))
        .stdout(contains("\"os_variant_id\":\"server\""))
        .stdout(contains("\"os_cpe\":\"cpe:/o:fedoraproject:fedora:44\""))
        .stdout(contains("\"os_support_end\":\"2027-05-19"))
        // Kernel
        .stdout(contains("\"kernel\":\"Linux\""))
        .stdout(contains("\"kernel_release\":\"7.0.10-201.fc44.x86_64\""))
        // Hardware + firmware
        .stdout(contains("\"hardware_vendor\":\"QEMU\""))
        .stdout(contains("\"chassis\":\"vm\""))
        .stdout(contains(
            "\"firmware_vendor\":\"EFI Development Kit II / OVMF\"",
        ))
        // Time/NTP
        .stdout(contains("\"timezone\":\"America/Chicago\""))
        .stdout(contains("\"ntp_enabled\":true"))
        .stdout(contains("\"ntp_synchronized\":true"))
        .stdout(contains("\"local_rtc\":false"))
        .stdout(contains("\"time_utc\":\"2023-11-15T00:00:00Z\""));
}

// The parsed os_release object has lowercase keys agents can read directly.
#[test]
fn show_json_parses_os_release_and_shape() {
    let out = fez_fake()
        .args(["system", "show", "--json"])
        .output()
        .expect("run fez");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let env: Value = serde_json::from_str(&stdout).expect("parse JSON");

    // os_release sub-object
    let os_release = &env["data"]["os_release"];
    assert_eq!(os_release["id"], "fedora");
    assert_eq!(os_release["version_id"], "44");
    assert_eq!(os_release["variant_id"], "server");

    // Envelope shape stays stable
    assert_eq!(env["apiVersion"], "fez/v1");
    assert_eq!(env["kind"], "SystemOverview");
    assert_eq!(env["status"], "ok");
    for field in [
        "hostname",
        "machine_id",
        "boot_id",
        "os",
        "kernel",
        "kernel_release",
        "timezone",
        "ntp_enabled",
        "ntp_synchronized",
        "time_utc",
    ] {
        assert!(
            env["data"].get(field).is_some(),
            "missing required field: {field}"
        );
    }
}

// Human output renders section headings, key fields, and omits null values.
#[test]
fn show_human_renders_sections_and_omits_nulls() {
    let out = fez_fake()
        .args(["system", "show"])
        .output()
        .expect("run fez");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Host"));
    assert!(stdout.contains("testbox.example.com"));
    assert!(stdout.contains("Operating System"));
    assert!(stdout.contains("Fedora Linux 44"));
    assert!(stdout.contains("Kernel"));
    assert!(stdout.contains("7.0.10-201.fc44.x86_64"));
    assert!(stdout.contains("Hardware"));
    assert!(stdout.contains("QEMU"));
    assert!(stdout.contains("Time"));
    assert!(stdout.contains("America/Chicago"));
    // Null PrettyHostname should be omitted
    assert!(!stdout.contains("Pretty hostname"));
}

// `fez describe system.show --json` returns the capability descriptor.
#[test]
fn describe_system_show_returns_descriptor() {
    fez_fake()
        .args(["describe", "system.show", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"CapabilityDescriptor\""))
        .stdout(contains("\"id\":\"system.show\""))
        .stdout(contains("\"output_kind\":\"SystemOverview\""))
        .stdout(contains("\"privileged\":false"));
}

// `fez capabilities` includes system.show.
#[test]
fn capabilities_lists_system_show() {
    fez_fake()
        .args(["capabilities"])
        .assert()
        .success()
        .stdout(contains("system.show"));
}
