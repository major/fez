use assert_cmd::Command as AssertCommand;
use fez::protocol::client::BridgeClient;
use fez::transport::local::LocalTransport;
use fez::transport::Transport;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use serde_json::json;
use std::process::Command;

mod common;
use common::{fez_fake, AuditLog};

fn fez_without_bridge() -> AssertCommand {
    let mut c = AssertCommand::cargo_bin("fez").unwrap();
    c.env("FEZ_BRIDGE", "/nonexistent/cockpit-bridge")
        .env("FEZ_AUDIT", "off");
    c
}

fn fake_transport() -> LocalTransport {
    LocalTransport {
        program: env!("CARGO_BIN_EXE_fez-fake-bridge").into(),
    }
}

struct NoisyFakeTransport;

impl Transport for NoisyFakeTransport {
    fn command(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_fez-fake-bridge"));
        // More than a typical pipe buffer. If BridgeClient does not drain
        // stderr, the fake blocks here before it can send the init frame.
        cmd.env("FEZ_FAKE_STDERR_BYTES", "1048576");
        cmd
    }

    fn host_label(&self) -> String {
        "localhost".to_string()
    }
}

#[test]
fn dbus_call_returns_listunits_out_args() {
    let t = fake_transport();
    let mut c = BridgeClient::connect(&t).unwrap();
    let channel = c.dbus_open("org.freedesktop.systemd1").unwrap();
    let out = c
        .dbus_call(
            &channel,
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
            "ListUnits",
            json!([]),
        )
        .unwrap();
    // out = out_args = reply[0]; units = out[0]
    let units = out.get(0).and_then(|u| u.as_array()).unwrap();
    assert_eq!(units.len(), 2);
    assert_eq!(units[0][0], json!("sshd.service"));
}

#[test]
fn bridge_connect_drains_child_stderr() {
    let t = NoisyFakeTransport;
    let mut c = BridgeClient::connect(&t).unwrap();
    let channel = c.dbus_open("org.freedesktop.systemd1").unwrap();
    let out = c
        .dbus_call(
            &channel,
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
            "ListUnits",
            json!([]),
        )
        .unwrap();
    assert!(out.get(0).and_then(|u| u.as_array()).is_some());
}

#[test]
fn stream_collects_journal_lines() {
    let t = fake_transport();
    let mut c = BridgeClient::connect(&t).unwrap();
    let blob = c
        .stream_collect(&["journalctl", "--output=json", "--unit", "sshd.service"])
        .unwrap();
    let lines: Vec<&[u8]> = blob
        .split(|&b| b == b'\n')
        .filter(|l| serde_json::from_slice::<serde_json::Value>(l).is_ok())
        .collect();
    assert_eq!(lines.len(), 2);
}

#[test]
fn stream_each_invokes_callback_per_chunk() {
    let t = fake_transport();
    let mut c = BridgeClient::connect(&t).unwrap();
    let mut total = 0usize;
    c.stream_each(
        &["journalctl", "--output=json", "--unit", "sshd.service"],
        |chunk| total += chunk.len(),
    )
    .unwrap();
    assert!(total > 0, "expected the fake bridge to emit stream bytes");
}

#[test]
fn dbus_call_surfaces_dbus_error() {
    let t = fake_transport();
    let mut c = BridgeClient::connect(&t).unwrap();
    let channel = c.dbus_open("org.freedesktop.systemd1").unwrap();
    let err = c
        .dbus_call(
            &channel,
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
            "NoSuchMethod",
            json!([]),
        )
        .unwrap_err();
    match err {
        fez::error::FezError::Dbus { name, .. } => {
            assert_eq!(name, "org.freedesktop.DBus.Error.UnknownMethod");
        }
        other => panic!("expected a D-Bus error, got {other:?}"),
    }
}

#[test]
fn privileged_dbus_open_routes_through_sudo_peer() {
    let t = fake_transport();
    let mut c = BridgeClient::connect(&t).unwrap();
    let channel = c.dbus_open_privileged("org.freedesktop.systemd1").unwrap();
    let out = c
        .dbus_call(
            &channel,
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
            "ListUnits",
            json!([]),
        )
        .unwrap();
    assert!(out.get(0).and_then(|u| u.as_array()).is_some());
}

#[test]
fn services_list_json_is_columnar() {
    fez_fake()
        .args(["services", "list", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"ServiceList\""))
        .stdout(contains("\"columns\":"))
        .stdout(contains("\"rows\":"))
        .stdout(contains("\"count\":"))
        .stdout(contains("\"name\""))
        .stdout(contains("\"active_state\""))
        .stdout(contains("sshd.service"))
        .stdout(contains("\"units\"").not());
}

#[test]
fn services_list_human_default() {
    fez_fake()
        .args(["services", "list"])
        .assert()
        .success()
        .stdout(contains("sshd.service"))
        .stdout(contains("active"));
}

#[test]
fn services_list_state_filter() {
    fez_fake()
        .args(["services", "list", "--state", "active"])
        .assert()
        .success()
        .stdout(contains("sshd.service"))
        .stdout(contains("chronyd.service").not()); // chronyd is inactive in the fake
}

#[test]
fn services_status_json() {
    fez_fake()
        .args(["services", "status", "sshd.service", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"ServiceStatus\""))
        .stdout(contains("\"active_state\":\"active\""))
        .stdout(contains("\"unit_file_state\":\"enabled\""));
}

#[test]
fn services_status_human() {
    fez_fake()
        .args(["services", "status", "sshd.service"])
        .assert()
        .success()
        .stdout(contains("sshd.service"))
        .stdout(contains("active (running)"));
}

// A bare unit name (no `.service` suffix) must resolve the same way the
// fully-qualified form does, matching systemctl's client-side name mangling.
#[test]
fn services_status_accepts_bare_unit_name() {
    fez_fake()
        .args(["services", "status", "sshd"])
        .assert()
        .success()
        .stdout(contains("sshd.service"))
        .stdout(contains("active (running)"));
}

#[test]
fn services_status_unknown_unit_exits_not_found() {
    fez_fake()
        .args(["services", "status", "missing.service", "--json"])
        .assert()
        .code(4)
        .stdout(contains("\"code\":\"not-found\""));
}

#[test]
fn services_status_generic_dbus_error_passes_through() {
    fez_fake()
        .args(["services", "status", "broken.service", "--json"])
        .assert()
        .code(7)
        .stdout(contains("\"code\":\"dbus-error\""));
}

#[test]
fn services_logs_json_entries() {
    fez_fake()
        .args(["services", "logs", "sshd.service", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"LogEntries\""))
        .stdout(contains("Accepted publickey"));
}

#[test]
fn services_logs_human() {
    fez_fake()
        .args(["services", "logs", "sshd.service"])
        .assert()
        .success()
        .stdout(contains("sshd"))
        .stdout(contains("listening"));
}

#[test]
fn services_logs_accepts_filters() {
    fez_fake()
        .args([
            "services",
            "logs",
            "sshd.service",
            "--since",
            "today",
            "--priority",
            "info",
            "--lines",
            "1",
            "--json",
        ])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"LogEntries\""));
}

#[test]
fn services_logs_follow_json_streams_entries() {
    fez_fake()
        .args(["services", "logs", "sshd.service", "--follow", "--json"])
        .assert()
        .success()
        .stdout(contains("\"message\":\"Server listening on port 22.\""))
        .stdout(contains("\"message\":\"Accepted publickey for fedora\""));
}

#[test]
fn services_logs_follow_human_streams_entries() {
    fez_fake()
        .args(["services", "logs", "sshd.service", "--follow"])
        .assert()
        .success()
        .stdout(contains("sshd: Server listening on port 22."))
        .stdout(contains("sshd: Accepted publickey for fedora"));
}

// `BridgeClient`, `LocalTransport`, and `json` are already imported at the top
// of tests/services.rs (Plan 1). Reuse the existing `fake_transport()` helper.
#[test]
fn privileged_dbus_call_succeeds_against_fake() {
    let t = fake_transport();
    let mut c = BridgeClient::connect(&t).unwrap();
    let channel = c.dbus_open_privileged("org.freedesktop.systemd1").unwrap();
    let out = c
        .dbus_call(
            &channel,
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
            "StartUnit",
            json!(["chronyd.service", "replace"]),
        )
        .unwrap();
    assert_eq!(
        out.get(0).and_then(|v| v.as_str()),
        Some("/org/freedesktop/systemd1/job/42")
    );
}

// Dry-run must never spawn a bridge: point FEZ_BRIDGE at a nonexistent path and
// assert success — proof that no connection was attempted (no Spawn error).
#[test]
fn services_start_dry_run_does_not_connect() {
    fez_without_bridge()
        .args([
            "services",
            "start",
            "chronyd.service",
            "--dry-run",
            "--json",
        ])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"DryRun\""))
        .stdout(contains("\"operation\":\"start\""))
        .stdout(contains("\"privileged\":true"));
}

// Protected-unit refusal happens before connecting: exit 8, no Spawn error.
#[test]
fn protected_unit_refused_before_connecting() {
    fez_without_bridge()
        .args(["services", "stop", "sshd.service", "--json"])
        .assert()
        .code(8)
        .stdout(contains("\"code\":\"protected-unit\""));
}

// --force overrides the policy; --dry-run still previews without executing.
#[test]
fn force_overrides_policy_in_dry_run() {
    fez_without_bridge()
        .args([
            "services",
            "stop",
            "sshd.service",
            "--force",
            "--dry-run",
            "--json",
        ])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"DryRun\""));
}

#[test]
fn services_start_returns_mutation_with_reverse_hint() {
    fez_fake()
        .env("FEZ_AUDIT", "off")
        .args(["services", "start", "chronyd.service", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"ServiceMutation\""))
        .stdout(contains("\"job\":\"/org/freedesktop/systemd1/job/42\""))
        .stdout(contains(
            "\"reverse\":\"fez services stop chronyd.service\"",
        ));
}

#[test]
fn services_stop_human_output() {
    fez_fake()
        .env("FEZ_AUDIT", "off")
        .args(["services", "stop", "chronyd.service"])
        .assert()
        .success()
        .stdout(contains("stopped chronyd.service"));
}

#[test]
fn services_restart_has_no_reverse_hint() {
    fez_fake()
        .env("FEZ_AUDIT", "off")
        .args(["services", "restart", "chronyd.service", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"ServiceMutation\""))
        .stdout(contains("hints").not());
}

#[test]
fn services_reload_has_no_reverse_hint() {
    fez_fake()
        .env("FEZ_AUDIT", "off")
        .args(["services", "reload", "chronyd.service", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"ServiceMutation\""))
        .stdout(contains("\"operation\":\"reload\""))
        .stdout(contains("hints").not());
}

#[test]
fn mutation_writes_attempt_and_result_audit_records() {
    let audit = AuditLog::new("audit-it");
    fez_fake()
        .env("FEZ_AUDIT", audit.env_value())
        .args(["services", "start", "chronyd.service", "--json"])
        .assert()
        .success();
    let records = audit.records();
    assert_eq!(records.len(), 2, "expected attempt + result records");
    let (attempt, result) = (&records[0], &records[1]);
    assert_eq!(attempt["result"], "attempt");
    assert_eq!(result["result"], "ok");
    assert_eq!(attempt["correlation_id"], result["correlation_id"]);
    assert_eq!(result["operation"], "start");
    assert_eq!(result["unit"], "chronyd.service");
}

#[test]
fn services_enable_returns_enablement_with_reverse_hint() {
    fez_fake()
        .env("FEZ_AUDIT", "off")
        .args(["services", "enable", "chronyd.service", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"ServiceEnablement\""))
        .stdout(contains("\"changes\""))
        .stdout(contains(
            "\"reverse\":\"fez services disable chronyd.service\"",
        ));
}

#[test]
fn services_enable_now_hint_includes_now() {
    fez_fake()
        .env("FEZ_AUDIT", "off")
        .args(["services", "enable", "chronyd.service", "--now", "--json"])
        .assert()
        .success()
        .stdout(contains(
            "\"reverse\":\"fez services disable chronyd.service --now\"",
        ));
}

#[test]
fn services_disable_human_output() {
    fez_fake()
        .env("FEZ_AUDIT", "off")
        .args(["services", "disable", "chronyd.service"])
        .assert()
        .success()
        .stdout(contains("disabled chronyd.service"));
}

#[test]
fn services_disable_now_stops_unit_and_keeps_reverse_hint() {
    fez_fake()
        .env("FEZ_AUDIT", "off")
        .args(["services", "disable", "chronyd.service", "--now", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"ServiceEnablement\""))
        .stdout(contains("\"operation\":\"disable\""))
        .stdout(contains(
            "\"reverse\":\"fez services enable chronyd.service --now\"",
        ));
}

// --- Privilege escalation: mid-operation denial ---------------------------
//
// Distinct from the mechanism-selection cases below: escalation succeeds (a
// mechanism starts), but the host policy then rejects the specific privileged
// channel. fez must surface that channel close as access-denied (exit 11), not
// a generic channel problem.

#[test]
fn privileged_channel_denied_maps_to_access_denied() {
    // sudo starts fine, but the sudoers/polkit policy rejects the privileged
    // channel mid-operation (FEZ_FAKE_DENY_PRIVILEGED). fez surfaces exit 11.
    fez_fake()
        .env("FEZ_AUDIT", "off")
        .env("FEZ_FAKE_BRIDGES", "sudo:ok")
        .env("FEZ_FAKE_DENY_PRIVILEGED", "1")
        .args(["services", "restart", "chronyd.service", "--json"])
        .assert()
        .code(11)
        .stdout(contains("\"code\":\"access-denied\""));
}

#[test]
fn read_path_unaffected_by_downstream_denial() {
    // A read opens an unprivileged channel, so the mid-operation privileged
    // denial does not touch it: listing still succeeds.
    fez_fake()
        .env("FEZ_AUDIT", "off")
        .env("FEZ_FAKE_BRIDGES", "sudo:ok")
        .env("FEZ_FAKE_DENY_PRIVILEGED", "1")
        .args(["services", "list", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"ServiceList\""));
}

// --- Transparent escalation (cockpit.Superuser mechanism fallback) --------
//
// FEZ_FAKE_BRIDGES models the host's viable escalation mechanisms as an
// ordered list of name:outcome pairs (ok|err). fez reads cockpit.Superuser
// Bridges and tries Start(name) for each until one succeeds; only if all fail
// does it return access-denied (exit 11). Reads never escalate.

#[test]
fn escalation_succeeds_with_passwordless_sudo() {
    // Case 1: sudo is present and starts cleanly -> mutation works.
    fez_fake()
        .env("FEZ_AUDIT", "off")
        .env("FEZ_FAKE_BRIDGES", "sudo:ok")
        .args(["services", "restart", "chronyd.service", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"ServiceMutation\""));
}

#[test]
fn escalation_succeeds_with_polkit_only() {
    // Case 2: no sudo, only polkit, and it starts -> mutation works.
    fez_fake()
        .env("FEZ_AUDIT", "off")
        .env("FEZ_FAKE_BRIDGES", "polkit:ok")
        .args(["services", "restart", "chronyd.service", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"ServiceMutation\""));
}

#[test]
fn escalation_falls_through_sudo_to_polkit() {
    // Case 3 (acceptance test for Layer 2): sudo errors on Start, fez falls
    // through to polkit, which succeeds. The old design (pin sudo) could not
    // pass this.
    fez_fake()
        .env("FEZ_AUDIT", "off")
        .env("FEZ_FAKE_BRIDGES", "sudo:err,polkit:ok")
        .args(["services", "restart", "chronyd.service", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"ServiceMutation\""));
}

#[test]
fn escalation_all_fail_maps_to_access_denied() {
    // Case 4a: every mechanism errors on Start -> exit 11 with remediation
    // that names both sudo and polkit.
    fez_fake()
        .env("FEZ_AUDIT", "off")
        .env("FEZ_FAKE_BRIDGES", "sudo:err,polkit:err")
        .args(["services", "restart", "chronyd.service", "--json"])
        .assert()
        .code(11)
        .stdout(contains("\"code\":\"access-denied\""))
        .stdout(contains("polkit"));
}

#[test]
fn escalation_empty_bridges_maps_to_access_denied() {
    // Case 4b: the host advertises no mechanism at all -> exit 11.
    fez_fake()
        .env("FEZ_AUDIT", "off")
        .env("FEZ_FAKE_BRIDGES", "")
        .args(["services", "restart", "chronyd.service", "--json"])
        .assert()
        .code(11)
        .stdout(contains("\"code\":\"access-denied\""))
        .stdout(contains("polkit"));
}

#[test]
fn read_succeeds_without_escalation() {
    // Case 5: a read never escalates, so it still works on a host with no
    // viable escalation mechanism.
    fez_fake()
        .env("FEZ_AUDIT", "off")
        .env("FEZ_FAKE_BRIDGES", "")
        .args(["services", "list", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"ServiceList\""));
}

#[test]
fn escalation_override_forces_named_mechanism() {
    // Case 6a: FEZ_ESCALATION=polkit selects polkit. sudo errors on Start, so
    // success is only possible by starting polkit, proving the override picked
    // it. The no-fall-through guarantee is covered separately by
    // escalation_override_named_failure_does_not_fall_through.
    fez_fake()
        .env("FEZ_AUDIT", "off")
        .env("FEZ_FAKE_BRIDGES", "sudo:err,polkit:ok")
        .env("FEZ_ESCALATION", "polkit")
        .args(["services", "restart", "chronyd.service", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"ServiceMutation\""));
}

#[test]
fn escalation_override_off_denies_mutation() {
    // Case 6b: FEZ_ESCALATION=off never escalates, so a mutation fails fast
    // with access-denied even though a mechanism is available.
    fez_fake()
        .env("FEZ_AUDIT", "off")
        .env("FEZ_FAKE_BRIDGES", "sudo:ok")
        .env("FEZ_ESCALATION", "off")
        .args(["services", "restart", "chronyd.service", "--json"])
        .assert()
        .code(11)
        .stdout(contains("\"code\":\"access-denied\""));
}

#[test]
fn escalation_override_named_failure_does_not_fall_through() {
    // Case 6c: forcing a mechanism that fails to start exits 11 without trying
    // any other (no fall-through under FEZ_ESCALATION), even though polkit
    // would have worked.
    fez_fake()
        .env("FEZ_AUDIT", "off")
        .env("FEZ_FAKE_BRIDGES", "sudo:err,polkit:ok")
        .env("FEZ_ESCALATION", "sudo")
        .args(["services", "restart", "chronyd.service", "--json"])
        .assert()
        .code(11)
        .stdout(contains("\"code\":\"access-denied\""));
}
