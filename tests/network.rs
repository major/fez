use assert_cmd::cargo::CommandCargoExt;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use serde_json::Value;
use std::process::Command;

mod common;
use common::fez_fake;

// `network list --json` shows the managed/physical devices and excludes the
// unmanaged container veth by default. enp1s0/enp2s0 (ethernet) and lo
// (loopback, kept by type) appear; veth0 does not.
#[test]
fn list_json_hides_unmanaged_veth() {
    fez_fake()
        .args(["network", "list", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"NetworkDeviceList\""))
        .stdout(contains("enp1s0"))
        .stdout(contains("enp2s0"))
        .stdout(contains("lo"))
        .stdout(contains("veth0").not());
}

// `network list --all --json` includes the unmanaged veth device.
#[test]
fn list_all_includes_veth() {
    fez_fake()
        .args(["network", "list", "--all", "--json"])
        .assert()
        .success()
        .stdout(contains("veth0"));
}

// Human `network list` renders a table with decoded type/state strings, not
// raw NM enum numbers.
#[test]
fn list_human_decodes_enums() {
    fez_fake()
        .args(["network", "list"])
        .assert()
        .success()
        .stdout(contains("ethernet"))
        .stdout(contains("activated"))
        .stdout(contains("loopback"));
}

// `network show <up-ethernet> --json` resolves the full detail: address +
// prefix, gateway, DNS, MAC, and the active connection id/type.
#[test]
fn show_json_resolves_full_detail() {
    fez_fake()
        .args(["network", "show", "enp1s0", "--json"])
        .assert()
        .success()
        .stdout(contains("\"kind\":\"NetworkDeviceDetail\""))
        .stdout(contains("192.168.10.20"))
        .stdout(contains("192.168.10.1"))
        .stdout(contains("52:54:00:12:34:56"))
        .stdout(contains("802-3-ethernet"))
        .stdout(contains("fd00::20"))
        // DHCP options are flattened: clean scalar, not the {"t","v"} wire shape.
        .stdout(contains("\"routers\":\"192.168.10.1\""));
}

// `network show <down-ethernet>` renders a device with no IP config (object
// path "/") without panicking, and reports a non-activated state.
#[test]
fn show_device_without_ip_config_does_not_panic() {
    fez_fake()
        .args(["network", "show", "enp2s0"])
        .assert()
        .success()
        .stdout(contains("enp2s0"))
        .stdout(contains("unavailable"));
}

// An unknown device name is a not-found error (exit 4).
#[test]
fn show_unknown_device_exits_4() {
    fez_fake()
        .args(["network", "show", "doesnotexist"])
        .assert()
        .code(4);
}

#[test]
fn list_json_shape_stays_stable() {
    let output = Command::cargo_bin("fez")
        .unwrap()
        .env("FEZ_BRIDGE", env!("CARGO_BIN_EXE_fez-fake-bridge"))
        .args(["network", "list", "--json"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        envelope,
        serde_json::json!({
            "apiVersion": "fez/v1",
            "kind": "NetworkDeviceList",
            "host": "localhost",
            "status": "ok",
            "data": {
                "columns": ["interface", "type", "state", "ip4", "ip6", "mac"],
                "count": 3,
                "rows": [
                    [
                        "enp1s0",
                        "ethernet",
                        "activated",
                        "192.168.10.20",
                        "fd00::20",
                        "52:54:00:12:34:56"
                    ],
                    ["enp2s0", "ethernet", "unavailable", "", "", "52:54:00:12:34:56"],
                    ["lo", "loopback", "unmanaged", "", "", "52:54:00:12:34:56"]
                ]
            }
        })
    );
}

#[test]
fn show_json_shape_stays_stable() {
    let output = Command::cargo_bin("fez")
        .unwrap()
        .env("FEZ_BRIDGE", env!("CARGO_BIN_EXE_fez-fake-bridge"))
        .args(["network", "show", "enp1s0", "--json"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        envelope,
        serde_json::json!({
            "apiVersion": "fez/v1",
            "kind": "NetworkDeviceDetail",
            "host": "localhost",
            "status": "ok",
            "data": {
                "connection": {
                    "default": true,
                    "id": "enp1s0",
                    "type": "802-3-ethernet"
                },
                "dhcp4": {
                    "ip_address": "192.168.10.20",
                    "routers": "192.168.10.1"
                },
                "interface": "enp1s0",
                "ipv4": {
                    "addresses": ["192.168.10.20/24"],
                    "dns": ["192.168.10.1", "1.1.1.1"],
                    "domains": ["lan"],
                    "gateway": "192.168.10.1"
                },
                "ipv6": {
                    "addresses": ["fd00::20/64"]
                },
                "mac": "52:54:00:12:34:56",
                "mtu": 1500,
                "state": "activated",
                "type": "ethernet"
            }
        })
    );
}
