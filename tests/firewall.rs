use predicates::prelude::PredicateBooleanExt;
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

// A protected op (removing the session-critical ssh service) is refused without
// --force: exit 8 and the envelope carries the stable protected-unit code. This
// pins the without-force half of the guard contract on its own.
#[test]
fn protected_op_refused_without_force() {
    fez_fake()
        .args(["firewall", "remove-service", "ssh", "--json"])
        .assert()
        .code(8)
        .stdout(contains("\"code\":\"protected-unit\""));
}

// The same protected op succeeds with --force: exit 0 and the mutation envelope.
// This pins the with-force half independently of the refusal case.
#[test]
fn protected_op_allowed_with_force() {
    fez_fake()
        .args(["firewall", "remove-service", "ssh", "--force", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"FirewallChange\""));
}

// remove-port on a non-session-critical port succeeds runtime-only with the
// confirm hint (no SSH_CONNECTION in the test env, so 9090/tcp is not gated).
#[test]
fn remove_port_succeeds() {
    fez_fake()
        .args(["firewall", "remove-port", "9090/tcp", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"FirewallChange\""))
        .stdout(contains("\"persisted\":false"))
        .stdout(contains("fez firewall confirm"));
}

// After a removal, a follow-up runtime read no longer lists the port. The fake
// bridge models the post-removal runtime state via FEZ_FAKE_PORT_REMOVED, so the
// drift port 9090/tcp is gone from the public zone.
#[test]
fn remove_port_gone_from_runtime_after_removal() {
    fez_fake()
        .env("FEZ_FAKE_PORT_REMOVED", "1")
        .args(["firewall", "show", "public", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"FirewallZone\""))
        .stdout(contains("ssh"))
        .stdout(contains("9090/tcp").not());
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

// firewalld absent on `list`: the first getZones call (on the zone interface)
// hits ServiceUnknown, exercising the error-propagation arm of that read and
// mapping to exit 9.
#[test]
fn list_firewalld_absent_exits_9() {
    fez_fake()
        .env("FEZ_FAKE_NO_FIREWALLD", "1")
        .args(["firewall", "list", "--json"])
        .assert()
        .code(9)
        .stdout(contains("\"code\":\"dependency-missing\""));
}

// firewalld absent on `show`: same getZones error path as `list`.
#[test]
fn show_firewalld_absent_exits_9() {
    fez_fake()
        .env("FEZ_FAKE_NO_FIREWALLD", "1")
        .args(["firewall", "show", "public", "--json"])
        .assert()
        .code(9)
        .stdout(contains("\"code\":\"dependency-missing\""));
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

// Regression for #33: `getZones` lives on the zone interface, not the root
// `org.fedoraproject.FirewallD1` interface. The realistic fake answers it only
// on the zone interface, so `list` succeeding proves fez calls the right one.
// (A path-only fake masked this; real firewalld returns UnknownMethod.)
#[test]
fn list_calls_getzones_on_zone_interface() {
    fez_fake()
        .args(["firewall", "list", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"FirewallZoneList\""))
        .stdout(contains("internal"));
}

// Regression for #33: `show` looks the zone up via the zone-interface
// `getZones`, so it resolves a real zone instead of erroring on the wrong
// interface.
#[test]
fn show_resolves_zone_via_zone_interface() {
    fez_fake()
        .args(["firewall", "show", "internal", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"FirewallZone\""))
        .stdout(contains("\"zone\":\"internal\""));
}

// Regression for #34: the permanent-config read for drift is polkit-gated
// (PK_ACTION_CONFIG, auth_admin_keep on server and desktop), so `status`
// escalates for it. On a host advertising no escalation mechanism the
// privileged read is denied and status fails with exit 11, rather than
// silently reporting empty drift. This is the e2e symptom from #32/#34.
#[test]
fn status_without_escalation_exits_11() {
    fez_fake()
        .env("FEZ_FAKE_BRIDGES", "")
        .args(["firewall", "status", "--json"])
        .assert()
        .code(11);
}

// Regression for #34: with a working escalation mechanism, `status` reads the
// permanent config over the privileged channel and reports the seeded drift.
// (Covered by status_reports_default_zone_and_drift; this asserts the drift
// specifically came from a privileged permanent read by also checking the
// confirm hint.)
#[test]
fn status_with_escalation_reports_drift() {
    fez_fake()
        .args(["firewall", "status", "--json"])
        .assert()
        .success()
        .stdout(contains("+port 9090/tcp"))
        .stdout(contains("fez firewall confirm"));
}

// Regression for #51: real firewalld can still reject the permanent config
// `config.info` read after cockpit superuser routing. `status` should keep the
// read-only runtime status usable and make the missing drift explicit.
#[test]
fn status_with_config_info_denied_reports_runtime_status() {
    fez_fake()
        .env("FEZ_FAKE_CONFIG_INFO_DENIED", "1")
        .args(["firewall", "status", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"FirewallStatus\""))
        .stdout(contains("\"pending_changes_available\":false"))
        .stdout(contains("permanent firewall config was not readable"));
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

// show <zone> reports the seeded runtime masquerade state (public is on).
#[test]
fn show_public_reports_masquerade_on() {
    fez_fake()
        .args(["firewall", "show", "public", "--json"])
        .assert()
        .success()
        .stdout(contains("\"masquerade\":true"));
}

// status lists the seeded masquerade drift (runtime public on, permanent off).
#[test]
fn status_reports_masquerade_drift() {
    fez_fake()
        .args(["firewall", "status", "--json"])
        .assert()
        .success()
        .stdout(contains("+masquerade"));
}

// masquerade off without --force is protected (exit 8). Default escalation is
// available (FEZ_FAKE_BRIDGES unset = sudo:ok), so the guard, not escalation,
// is what trips.
#[test]
fn masquerade_off_without_force_exits_8() {
    fez_fake()
        .args(["firewall", "masquerade", "off"])
        .assert()
        .code(8);
}

// masquerade off with --force succeeds.
#[test]
fn masquerade_off_with_force_succeeds() {
    fez_fake()
        .args(["firewall", "masquerade", "off", "--force", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"FirewallChange\""))
        .stdout(contains("\"masquerade\":false"));
}

// masquerade on is unguarded and succeeds without --force.
#[test]
fn masquerade_on_succeeds() {
    fez_fake()
        .args(["firewall", "masquerade", "on"])
        .assert()
        .success();
}

// masquerade on --json emits a compact FirewallChange envelope mentioning masquerade.
#[test]
fn masquerade_on_json_reports_change() {
    fez_fake()
        .args(["firewall", "masquerade", "on", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"FirewallChange\""))
        .stdout(contains("\"operation\":\"masquerade\""))
        .stdout(contains("\"masquerade\":true"));
}

// masquerade on with a timeout succeeds and echoes the timeout.
#[test]
fn masquerade_on_with_timeout_echoes_timeout() {
    fez_fake()
        .args(["firewall", "masquerade", "on", "--timeout", "60", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"FirewallChange\""))
        .stdout(contains("\"timeout\":60"));
}

// masquerade on a host advertising no escalation mechanism -> exit 11.
#[test]
fn masquerade_on_without_escalation_exits_11() {
    fez_fake()
        .env("FEZ_FAKE_BRIDGES", "")
        .args(["firewall", "masquerade", "on"])
        .assert()
        .code(11);
}

// ---- issue #60: actionable firewall dependency and API errors ----

// On a real host firewalld's D-Bus name may be unreachable (absent or its unit
// failed): cockpit closes the channel with `not-found`, which previously
// surfaced as the vague `channel problem: not-found`. It must instead map to a
// stable dependency-missing error (exit 9) with remediation, not exit 4.
#[test]
fn unreachable_firewalld_maps_to_dependency_missing() {
    fez_fake()
        .env("FEZ_FAKE_FIREWALLD_UNREACHABLE", "1")
        .args(["firewall", "list", "--json"])
        .assert()
        .code(9)
        .stdout(contains("\"code\":\"dependency-missing\""))
        .stdout(contains("firewalld"))
        // The vague channel-problem wording must not leak through.
        .stdout(contains("channel problem").not());
}

// The dependency-missing envelope carries a remediation hint for safe
// read-only follow-up (checking the service), not just a bare message.
#[test]
fn unreachable_firewalld_includes_remediation_detail() {
    fez_fake()
        .env("FEZ_FAKE_FIREWALLD_UNREACHABLE", "1")
        .args(["firewall", "status", "--json"])
        .assert()
        .code(9)
        .stdout(contains("\"code\":\"dependency-missing\""))
        .stdout(contains("firewalld.service"))
        // A safe read-only follow-up hint points at the service-status check.
        .stdout(contains("\"hints\""))
        .stdout(contains("fez services status firewalld.service"));
}

// An older firewalld without getMasquerade returns UnknownMethod; that used to
// leak as a raw dbus-error. It must map to the unsupported-api code (exit 12)
// with the method name, so an LLM treats the feature as unsupported rather than
// retrying or recommending an install.
#[test]
fn missing_masquerade_method_maps_to_unsupported_api() {
    fez_fake()
        .env("FEZ_FAKE_NO_MASQUERADE", "1")
        .args(["firewall", "status", "--json"])
        .assert()
        .code(12)
        .stdout(contains("\"code\":\"unsupported-api\""))
        .stdout(contains("getMasquerade"))
        // The hint tells the caller to treat the feature as unsupported.
        .stdout(contains("unsupported"));
}

// Plain-text (no --json) still renders a single actionable error line, not the
// raw dbus/channel internals.
#[test]
fn unreachable_firewalld_plain_text_is_actionable() {
    fez_fake()
        .env("FEZ_FAKE_FIREWALLD_UNREACHABLE", "1")
        .args(["firewall", "list"])
        .assert()
        .code(9)
        .stderr(contains("firewalld"))
        .stderr(contains("channel problem").not());
}
