use predicates::str::contains;

mod common;
// Audit off so firewall tests do not depend on a journal socket being present.
use common::fez_fake_quiet as fez_fake;

// status reports the default zone, panic flag, and the seeded drift (runtime
// public carries 9090/tcp that permanent public lacks).
#[test]
fn status_reports_default_zone_and_drift() {
    fez_fake()
        .args(["firewall", "status", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"FirewallStatus\""))
        .stdout(contains("\"default_zone\":\"public\""))
        .stdout(contains("\"panic_mode\":false"))
        .stdout(contains("+port 9090/tcp"));
}

// list shows all three seeded zones and marks the default.
#[test]
fn list_shows_all_zones() {
    fez_fake()
        .args(["firewall", "list", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"FirewallZoneList\""))
        .stdout(contains("public"))
        .stdout(contains("internal"))
        .stdout(contains("drop"));
}

// show <zone> lists the zone's services and ports.
#[test]
fn show_public_lists_services_and_ports() {
    fez_fake()
        .args(["firewall", "show", "public", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"FirewallZone\""))
        .stdout(contains("ssh"))
        .stdout(contains("9090/tcp"));
}

// show <unknown zone> exits 4.
#[test]
fn show_unknown_zone_exits_4() {
    fez_fake()
        .args(["firewall", "show", "bogus"])
        .assert()
        .code(4);
}

// services lists the catalog.
#[test]
fn services_lists_catalog() {
    fez_fake()
        .args(["firewall", "services", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"FirewallServiceCatalog\""))
        .stdout(contains("http"))
        .stdout(contains("https"));
}

// add-service succeeds runtime-only with the confirm hint.
#[test]
fn add_service_runtime_only_with_hint() {
    fez_fake()
        .args(["firewall", "add-service", "http", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"FirewallChange\""))
        .stdout(contains("\"persisted\":false"))
        .stdout(contains("fez firewall confirm"));
}

// add-port with a timeout succeeds and echoes the timeout.
#[test]
fn add_port_with_timeout() {
    fez_fake()
        .args([
            "firewall",
            "add-port",
            "8080/tcp",
            "--timeout",
            "60",
            "--json",
        ])
        .assert()
        .success()
        .stdout(contains("\"timeout\":60"));
}

// add-port with a malformed spec exits 4 (parse error before any call).
#[test]
fn add_port_bad_spec_exits_4() {
    fez_fake()
        .args(["firewall", "add-port", "nope"])
        .assert()
        .code(4);
}

// Removing the session-critical ssh service without --force is protected (exit 8).
#[test]
fn remove_ssh_service_without_force_exits_8() {
    fez_fake()
        .args(["firewall", "remove-service", "ssh"])
        .assert()
        .code(8);
}

// With --force the ssh removal succeeds.
#[test]
fn remove_ssh_service_with_force_succeeds() {
    fez_fake()
        .args(["firewall", "remove-service", "ssh", "--force", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"FirewallChange\""));
}

// set-default-zone without --force is gated (exit 8); with --force it succeeds.
#[test]
fn set_default_zone_requires_force() {
    fez_fake()
        .args(["firewall", "set-default-zone", "internal"])
        .assert()
        .code(8);
    fez_fake()
        .args([
            "firewall",
            "set-default-zone",
            "internal",
            "--force",
            "--json",
        ])
        .assert()
        .success();
}

// panic on without --force is gated (exit 8); panic off succeeds.
#[test]
fn panic_on_gated_off_allowed() {
    fez_fake()
        .args(["firewall", "panic", "on"])
        .assert()
        .code(8);
    fez_fake()
        .args(["firewall", "panic", "off", "--json"])
        .assert()
        .success()
        .stdout(contains("\"panic_mode\":false"));
}

// confirm calls runtimeToPermanent and reports success.
#[test]
fn confirm_persists() {
    fez_fake()
        .args(["firewall", "confirm", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"FirewallConfirm\""))
        .stdout(contains("\"persisted\":true"));
}

// reload with drift present is refused without --force (exit 8); --force reloads.
#[test]
fn reload_with_drift_requires_force() {
    fez_fake().args(["firewall", "reload"]).assert().code(8);
    fez_fake()
        .args(["firewall", "reload", "--force", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"FirewallChange\""));
}

// firewalld absent -> exit 9 with remediation.
#[test]
fn firewalld_absent_exits_9() {
    fez_fake()
        .env("FEZ_FAKE_NO_FIREWALLD", "1")
        .args(["firewall", "status", "--json"])
        .assert()
        .code(9)
        .stdout(contains("\"code\":\"dependency-missing\""))
        .stdout(contains("firewalld"));
}

// A mutating call on a host advertising no escalation mechanism -> exit 11.
#[test]
fn mutation_without_escalation_exits_11() {
    fez_fake()
        .env("FEZ_FAKE_BRIDGES", "")
        .args(["firewall", "add-service", "http"])
        .assert()
        .code(11);
}

// panic off when the host starts in panic mode succeeds (FEZ_FAKE_PANIC).
#[test]
fn panic_off_when_panic_on() {
    fez_fake()
        .env("FEZ_FAKE_PANIC", "1")
        .args(["firewall", "panic", "off", "--json"])
        .assert()
        .success();
}
