use predicates::str::contains;

mod common;
use common::fez_fake_quiet;

#[test]
fn journal_default_returns_entries() {
    fez_fake_quiet()
        .args(["journal"])
        .assert()
        .success()
        .stdout(contains("sshd"))
        .stdout(contains("info:"));
}

#[test]
fn journal_json_envelope() {
    fez_fake_quiet()
        .args(["journal", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"JournalEntries\""))
        .stdout(contains("\"truncated\""))
        .stdout(contains("\"entries\""));
}

#[test]
fn journal_lines_limits_output() {
    let out = fez_fake_quiet()
        .args(["journal", "--lines", "2", "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let entries = v["data"]["entries"].as_array().unwrap();
    assert!(entries.len() <= 2);
}

#[test]
fn journal_unit_filters() {
    let out = fez_fake_quiet()
        .args(["journal", "--unit", "chronyd.service", "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let entries = v["data"]["entries"].as_array().unwrap();
    assert!(!entries.is_empty(), "expected chronyd entries");
    for entry in entries {
        assert!(
            entry["identifier"].as_str().unwrap().contains("chronyd"),
            "expected chronyd entries only, got {:?}",
            entry
        );
    }
}

#[test]
fn journal_multiple_units() {
    let out = fez_fake_quiet()
        .args([
            "journal",
            "--unit",
            "sshd.service",
            "--unit",
            "chronyd.service",
            "--json",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let entries = v["data"]["entries"].as_array().unwrap();
    // Should have entries from both units.
    let has_sshd = entries.iter().any(|e| e["identifier"] == "sshd");
    let has_chronyd = entries.iter().any(|e| e["identifier"] == "chronyd");
    assert!(has_sshd, "expected sshd entries");
    assert!(has_chronyd, "expected chronyd entries");
}

#[test]
fn journal_priority_filters() {
    let out = fez_fake_quiet()
        .args(["journal", "--priority", "err", "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let entries = v["data"]["entries"].as_array().unwrap();
    assert!(!entries.is_empty(), "expected error entries");
    for entry in entries {
        let pri = entry["priority"].as_str().unwrap();
        assert!(
            ["emerg", "alert", "crit", "err"].contains(&pri),
            "expected err or higher, got {pri}"
        );
    }
}

#[test]
fn journal_grep_filters() {
    let out = fez_fake_quiet()
        .args(["journal", "--grep", "publickey", "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let entries = v["data"]["entries"].as_array().unwrap();
    assert!(!entries.is_empty());
    for entry in entries {
        assert!(
            entry["message"].as_str().unwrap().contains("publickey"),
            "expected grep match"
        );
    }
}

#[test]
fn journal_boot_current() {
    let out = fez_fake_quiet()
        .args(["journal", "--boot", "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let entries = v["data"]["entries"].as_array().unwrap();
    // Entry 6 is from boot-prev, should not appear.
    // All current-boot entries have timestamps >= 1700000000000000.
    assert!(!entries.is_empty());
    for entry in entries {
        let ts: u64 = entry["timestamp"].as_str().unwrap().parse().unwrap_or(0);
        assert!(
            ts >= 1_700_000_000_000_000,
            "expected current boot entries only"
        );
    }
}

#[test]
fn journal_list_boots() {
    let out = fez_fake_quiet()
        .args(["journal", "--list-boots", "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("\"kind\":\"JournalBoots\""));
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let boots = v["data"]["boots"].as_array().unwrap();
    assert_eq!(boots.len(), 2);
}

#[test]
fn journal_list_fields() {
    let out = fez_fake_quiet()
        .args(["journal", "--list-fields", "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("\"kind\":\"JournalFields\""));
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let fields = v["data"]["fields"].as_array().unwrap();
    assert!(fields.len() >= 5);
    // Should include well-known fields.
    let names: Vec<&str> = fields.iter().filter_map(|f| f.as_str()).collect();
    assert!(names.contains(&"MESSAGE"));
    assert!(names.contains(&"PRIORITY"));
}

#[test]
fn journal_output_fields_adds_extras() {
    let out = fez_fake_quiet()
        .args(["journal", "--output-fields", "_COMM,_EXE", "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let entries = v["data"]["entries"].as_array().unwrap();
    assert!(!entries.is_empty());
    // Extra fields should appear on entries.
    let first = &entries[0];
    assert!(first.get("_COMM").is_some(), "expected _COMM in output");
    // Default fields should still be present.
    assert!(first.get("timestamp").is_some());
    assert!(first.get("message").is_some());
}

#[test]
fn journal_truncation_hint() {
    // Default limit is 25, but fake only has 6 entries.
    // Use --lines 2 to trigger truncation.
    let out = fez_fake_quiet()
        .args(["journal", "--lines", "2", "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["data"]["truncated"], true);
}

#[test]
fn journal_no_truncation_when_all_fit() {
    // 25 lines, only 6 entries — no truncation.
    let out = fez_fake_quiet()
        .args(["journal", "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["data"]["truncated"], false);
}

#[test]
fn journal_bad_priority_rejected() {
    fez_fake_quiet()
        .args(["journal", "--priority", "potato"])
        .assert()
        .failure();
}

#[test]
fn journal_list_boots_conflicts_with_unit() {
    fez_fake_quiet()
        .args(["journal", "--list-boots", "--unit", "sshd"])
        .assert()
        .failure()
        .stderr(contains("cannot be used with"));
}
