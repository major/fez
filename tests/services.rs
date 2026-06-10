use assert_cmd::Command as AssertCommand;
use fez::protocol::client::BridgeClient;
use fez::transport::local::LocalTransport;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use serde_json::json;

fn fez_fake() -> AssertCommand {
    let mut c = AssertCommand::cargo_bin("fez").unwrap();
    c.env("FEZ_BRIDGE", env!("CARGO_BIN_EXE_fez-fake-bridge"));
    c
}

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
fn stream_collects_journal_lines() {
    let t = fake_transport();
    let mut c = BridgeClient::connect(&t).unwrap();
    let blob = c
        .stream_collect(&["journalctl", "--output=json", "--unit", "sshd.service"])
        .unwrap();
    let lines: Vec<&[u8]> = blob
        .split(|&b| b == b'\n')
        .filter(|l| !l.is_empty())
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
fn mutation_writes_attempt_and_result_audit_records() {
    let path = std::env::temp_dir().join(format!("fez-audit-it-{}.jsonl", std::process::id()));
    let _ = std::fs::remove_file(&path);
    fez_fake()
        .env("FEZ_AUDIT", format!("file:{}", path.display()))
        .args(["services", "start", "chronyd.service", "--json"])
        .assert()
        .success();
    let body = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = body.lines().collect();
    assert_eq!(lines.len(), 2, "expected attempt + result records");
    let attempt: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    let result: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(attempt["result"], "attempt");
    assert_eq!(result["result"], "ok");
    assert_eq!(attempt["correlation_id"], result["correlation_id"]);
    assert_eq!(result["operation"], "start");
    assert_eq!(result["unit"], "chronyd.service");
    let _ = std::fs::remove_file(&path);
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

// --- Privilege escalation (superuser) handshake ---------------------------
//
// The fake bridge's FEZ_FAKE_SUPERUSER knob selects how the superuser
// negotiation behaves: "ok" (default) completes with superuser-init-done,
// "challenge" sends an authorize password prompt, "denied" closes privileged
// channels with access-denied. fez holds no sudo password, so any path that
// needs one must fail fast with exit 11 (access-denied), never hang.

#[test]
fn mutation_succeeds_with_passwordless_superuser() {
    // "ok" is the default, but assert it explicitly: a mutation that opens a
    // privileged channel completes when the bridge escalates without a prompt.
    fez_fake()
        .env("FEZ_AUDIT", "off")
        .env("FEZ_FAKE_SUPERUSER", "ok")
        .args(["services", "restart", "chronyd.service", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"ServiceMutation\""));
}

#[test]
fn authorize_challenge_fails_fast_with_access_denied() {
    // sudo wants a password. fez refuses the authorize challenge instead of
    // hanging, and reports exit 11 with remediation. Any operation trips this
    // because the handshake happens at connect time before any channel opens.
    fez_fake()
        .env("FEZ_AUDIT", "off")
        .env("FEZ_FAKE_SUPERUSER", "challenge")
        .args(["services", "restart", "chronyd.service", "--json"])
        .assert()
        .code(11)
        .stdout(contains("\"code\":\"access-denied\""))
        .stdout(contains("NOPASSWD"));
}

#[test]
fn read_fails_fast_when_challenge_required() {
    // Prove the handshake fails at connect time even for an unprivileged read:
    // a "challenge" the client cannot answer denies the connection before any
    // channel opens, so even `services list` exits 11.
    fez_fake()
        .env("FEZ_AUDIT", "off")
        .env("FEZ_FAKE_SUPERUSER", "challenge")
        .args(["services", "list", "--json"])
        .assert()
        .code(11)
        .stdout(contains("\"code\":\"access-denied\""))
        .stdout(contains("NOPASSWD"));
}

#[test]
fn privileged_channel_denied_maps_to_access_denied() {
    // Escalation "succeeds" at init time but the sudoers allow list rejects the
    // command: the privileged channel closes with access-denied mid-operation.
    // fez surfaces that as exit 11, not a generic channel problem.
    fez_fake()
        .env("FEZ_AUDIT", "off")
        .env("FEZ_FAKE_SUPERUSER", "denied")
        .args(["services", "restart", "chronyd.service", "--json"])
        .assert()
        .code(11)
        .stdout(contains("\"code\":\"access-denied\""));
}

#[test]
fn read_path_unaffected_by_downstream_denial() {
    // A read opens an unprivileged channel, so the "denied" downstream policy
    // (privileged-only) does not touch it: listing still succeeds.
    fez_fake()
        .env("FEZ_AUDIT", "off")
        .env("FEZ_FAKE_SUPERUSER", "denied")
        .args(["services", "list", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"ServiceList\""));
}
