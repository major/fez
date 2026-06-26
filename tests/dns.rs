use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;

mod common;
use common::{fez_fake, fez_fake_quiet, AuditLog};

// --- dns status ---

#[test]
fn status_json_returns_global_and_filtered_links() {
    fez_fake()
        .args(["dns", "status", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"DnsStatus\""))
        .stdout(contains("192.168.1.1"))
        .stdout(contains("fd00::1"))
        .stdout(contains("\"dnssec\":\"no\""))
        .stdout(contains("\"cache_size\":100"))
        .stdout(contains("\"cache_hits\":500"))
        // Link 2 (has DNS) should appear
        .stdout(contains("\"ifindex\":2"))
        // Link 3 (no DNS) should be hidden by default
        .stdout(contains("\"ifindex\":3").not());
}

#[test]
fn status_all_includes_links_without_dns() {
    fez_fake()
        .args(["dns", "status", "--all", "--json"])
        .assert()
        .success()
        .stdout(contains("\"ifindex\":2"))
        .stdout(contains("\"ifindex\":3"))
        .stdout(contains("\"ifindex\":10"));
}

#[test]
fn status_human_renders_sections() {
    fez_fake()
        .args(["dns", "status"])
        .assert()
        .success()
        .stdout(contains("Global"))
        .stdout(contains("192.168.1.1"))
        .stdout(contains("DNSSEC"))
        .stdout(contains("Cache"));
}

// --- dns flush ---

#[test]
fn flush_json_returns_dns_flush_envelope() {
    let audit = AuditLog::new("dns-flush");
    let mut cmd = fez_fake();
    cmd.env("FEZ_AUDIT", audit.env_value());
    cmd.args(["dns", "flush", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"DnsFlush\""))
        .stdout(contains("\"flushed\":true"));

    // Verify audit records
    let records = audit.records();
    assert!(records.len() >= 2, "expected attempt + result records");
    assert_eq!(records[0]["operation"], "dns-flush");
    assert_eq!(records[1]["result"], "ok");
}

#[test]
fn flush_human_confirms() {
    fez_fake_quiet()
        .args(["dns", "flush"])
        .assert()
        .success()
        .stdout(contains("DNS cache flushed"));
}

// --- dns query ---

#[test]
fn query_json_resolves_example_com() {
    fez_fake()
        .args(["dns", "query", "example.com", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"DnsQuery\""))
        .stdout(contains("\"hostname\":\"example.com\""))
        .stdout(contains("\"canonical\":\"example.com\""))
        .stdout(contains("93.184.215.14"))
        .stdout(contains("\"family\":\"ipv4\""));
}

#[test]
fn query_human_shows_addresses() {
    fez_fake()
        .args(["dns", "query", "example.com"])
        .assert()
        .success()
        .stdout(contains("93.184.215.14"));
}

#[test]
fn query_nxdomain_exits_4() {
    fez_fake()
        .args(["dns", "query", "nonexistent.invalid"])
        .assert()
        .code(4)
        .stderr(contains("NXDOMAIN"));
}

#[test]
fn query_nxdomain_json_envelope() {
    fez_fake()
        .args(["dns", "query", "nonexistent.invalid", "--json"])
        .assert()
        .code(4)
        .stdout(contains("\"code\":\"not-found\""))
        .stdout(contains("NXDOMAIN"));
}

// --- NM fallback (resolved absent) ---

#[test]
fn status_falls_back_to_nm_when_resolved_absent() {
    let mut cmd = fez_fake();
    cmd.env("FEZ_FAKE_NO_RESOLVED", "1");
    cmd.args(["dns", "status", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"DnsStatus\""))
        .stdout(contains("\"backend\":\"networkmanager\""))
        .stdout(contains("192.168.1.1"))
        .stdout(contains("\"mode\":\"default\""));
}

#[test]
fn status_nm_fallback_human_shows_mode() {
    let mut cmd = fez_fake();
    cmd.env("FEZ_FAKE_NO_RESOLVED", "1");
    cmd.args(["dns", "status"])
        .assert()
        .success()
        .stdout(contains("DNS Mode"))
        .stdout(contains("192.168.1.1"));
}

#[test]
fn flush_exits_9_when_resolved_absent() {
    let mut cmd = fez_fake_quiet();
    cmd.env("FEZ_FAKE_NO_RESOLVED", "1");
    cmd.args(["dns", "flush", "--json"])
        .assert()
        .code(9)
        .stdout(contains("\"code\":\"dependency-missing\""))
        .stdout(contains("dns flush requires systemd-resolved"));
}

#[test]
fn query_exits_9_when_resolved_absent() {
    let mut cmd = fez_fake_quiet();
    cmd.env("FEZ_FAKE_NO_RESOLVED", "1");
    cmd.args(["dns", "query", "example.com", "--json"])
        .assert()
        .code(9)
        .stdout(contains("\"code\":\"dependency-missing\""))
        .stdout(contains("dns query requires systemd-resolved"));
}

// --- describe ---

#[test]
fn describe_dns_status_returns_descriptor() {
    fez_fake()
        .args(["describe", "dns.status"])
        .assert()
        .success()
        .stdout(contains("dns.status"))
        .stdout(contains("DnsStatus"));
}

#[test]
fn capabilities_includes_dns() {
    fez_fake()
        .args(["capabilities"])
        .assert()
        .success()
        .stdout(contains("dns.status"))
        .stdout(contains("dns.flush"))
        .stdout(contains("dns.query"));
}
