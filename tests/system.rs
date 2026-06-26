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

// --- system metrics integration tests ---

// `system metrics --json` returns a SystemMetrics envelope with all sections.
#[test]
fn metrics_json_returns_performance_snapshot() {
    fez_fake()
        .args(["system", "metrics", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"SystemMetrics\""))
        // Load
        .stdout(contains("\"1_minute\""))
        .stdout(contains("\"5_minute\""))
        .stdout(contains("\"15_minute\""))
        // CPU
        .stdout(contains("\"user_ms_per_s\""))
        .stdout(contains("\"system_ms_per_s\""))
        .stdout(contains("\"idle_ms_per_s\""))
        .stdout(contains("\"used_percent\""))
        // Memory
        .stdout(contains("\"total_kb\""))
        .stdout(contains("\"available_kb\""))
        // Disk
        .stdout(contains("\"iops\""))
        // Network
        .stdout(contains("\"interface\""))
        .stdout(contains("\"bytes_per_s\""));
}

// Human output renders section headings and key values.
#[test]
fn metrics_human_shows_summary() {
    fez_fake()
        .args(["system", "metrics"])
        .assert()
        .success()
        .stdout(contains("Load average:"))
        .stdout(contains("CPU:"))
        .stdout(contains("Memory:"))
        .stdout(contains("Disk I/O:"))
        .stdout(contains("enp1s0"));
}

// Missing PCP packages close the channel with not-supported → exit 9.
#[test]
fn metrics_missing_pcp_exits_9() {
    fez_fake()
        .env("FEZ_FAKE_NO_PCP", "1")
        .args(["system", "metrics"])
        .assert()
        .code(9)
        .stderr(contains("missing dependency pcp"));
}

// Missing PCP with --json returns a structured error envelope.
#[test]
fn metrics_missing_pcp_json_returns_error_envelope() {
    fez_fake()
        .env("FEZ_FAKE_NO_PCP", "1")
        .args(["system", "metrics", "--json"])
        .assert()
        .code(9)
        .stdout(contains("\"status\":\"error\""))
        .stdout(contains("\"code\":\"dependency-missing\""));
}

// Parsed JSON has correct computed values from the canned fake-bridge data.
#[test]
fn metrics_json_has_expected_structure() {
    let out = fez_fake()
        .args(["system", "metrics", "--json"])
        .output()
        .expect("run fez");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let env: Value = serde_json::from_str(&stdout).expect("parse JSON");

    // Envelope shape
    assert_eq!(env["apiVersion"], "fez/v1");
    assert_eq!(env["kind"], "SystemMetrics");
    assert_eq!(env["status"], "ok");

    let cpu_pct = env["data"]["cpu"]["used_percent"].as_f64().unwrap();
    // (250+75)/(250+75+9675)*100 = 3.25 → round to 1 decimal = 3.3
    assert_eq!(cpu_pct, 3.3, "cpu used_percent: {cpu_pct}");

    // Memory used_percent: (16384000-12000000)/16384000 = 26.8%
    let mem_pct = env["data"]["memory"]["used_percent"].as_f64().unwrap();
    // (16384000-12000000)/16384000*100 = 26.757... → round to 1 decimal = 26.8
    assert_eq!(mem_pct, 26.8, "mem used_percent: {mem_pct}");

    // Network has 3 interfaces
    let net = env["data"]["network"].as_array().unwrap();
    assert_eq!(net.len(), 3);
}
