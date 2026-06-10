use assert_cmd::Command as AssertCommand;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;

fn fez_fake() -> AssertCommand {
    let mut c = AssertCommand::cargo_bin("fez").unwrap();
    c.env("FEZ_BRIDGE", env!("CARGO_BIN_EXE_fez-fake-bridge"));
    c
}

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
