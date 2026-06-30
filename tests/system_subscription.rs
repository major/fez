use predicates::str::contains;
use serde_json::Value;

mod common;
use common::{fez_fake, fez_fake_quiet};

#[test]
fn subscription_json_returns_status() {
    fez_fake()
        .args(["system", "subscription", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"SubscriptionStatus\""))
        .stdout(contains(
            "\"consumer_uuid\":\"12345678-abcd-1234-abcd-123456789abc\"",
        ))
        .stdout(contains("\"status\":\"Current\""))
        .stdout(contains("\"Red Hat Enterprise Linux for x86_64\""))
        .stdout(contains("\"role\":\"Red Hat Enterprise Linux Server\""));
}

#[test]
fn subscription_json_has_correct_shape() {
    let out = fez_fake()
        .args(["system", "subscription", "--json"])
        .output()
        .expect("run fez");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let env: Value = serde_json::from_str(&stdout).expect("parse JSON");
    assert_eq!(env["apiVersion"], "fez/v1");
    assert_eq!(env["kind"], "SubscriptionStatus");
    assert!(env["data"]["installed_products"].is_array());
    assert!(env["data"]["syspurpose"].is_object());
}

#[test]
fn subscription_human_renders_details() {
    let out = fez_fake()
        .args(["system", "subscription"])
        .output()
        .expect("run fez");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Subscription"));
    assert!(stdout.contains("Current"));
    assert!(stdout.contains("12345678"));
    assert!(stdout.contains("Red Hat Enterprise Linux"));
}

#[test]
fn subscription_absent_exits_9() {
    fez_fake_quiet()
        .env("FEZ_FAKE_NO_RHSM", "1")
        .args(["system", "subscription"])
        .assert()
        .code(9)
        .stderr(contains("subscription-manager"));
}

#[test]
fn subscription_absent_json_exits_9() {
    fez_fake_quiet()
        .env("FEZ_FAKE_NO_RHSM", "1")
        .args(["system", "subscription", "--json"])
        .assert()
        .code(9)
        .stdout(contains("\"code\":\"dependency-missing\""))
        .stdout(contains("subscription-manager"));
}

#[test]
fn describe_subscription_returns_descriptor() {
    fez_fake()
        .args(["describe", "system.subscription", "--json"])
        .assert()
        .success()
        .stdout(contains("\"id\":\"system.subscription\""))
        .stdout(contains("\"output_kind\":\"SubscriptionStatus\""));
}
