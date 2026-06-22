//! Canned hostname1 + timedate1 replies for system overview tests.

use super::{err_reply, ok_reply};
use serde_json::{json, Value};

/// hostname1 object path.
pub(super) const HOSTNAME_PATH: &str = "/org/freedesktop/hostname1";

/// timedate1 object path.
pub(super) const TIMEDATE_PATH: &str = "/org/freedesktop/timedate1";

/// Canned reply for a call against hostname1 or timedate1.
pub(super) fn hosttime_reply(path: &str, method: &str, args: &[Value], id: &Value) -> Value {
    match (path, method) {
        (HOSTNAME_PATH, "Describe") => hostname_describe(id),
        (TIMEDATE_PATH, "GetAll") => timedate_getall(args, id),
        _ => err_reply(
            id,
            "org.freedesktop.DBus.Error.UnknownMethod",
            format!("no hosttime fake for {path} {method}"),
        ),
    }
}

fn hostname_describe(id: &Value) -> Value {
    // ponytail: only fields the capability reads; null PrettyHostname tests null handling
    let describe_json = serde_json::to_string(&json!({
        "Hostname": "testbox.example.com",
        "StaticHostname": "testbox.example.com",
        "PrettyHostname": null,
        "HostnameSource": "static",
        "Chassis": "vm",
        "KernelName": "Linux",
        "KernelRelease": "7.0.10-201.fc44.x86_64",
        "KernelVersion": "#1 SMP PREEMPT_DYNAMIC Wed May 27 13:57:41 UTC 2026",
        "OperatingSystemPrettyName": "Fedora Linux 44 (Forty Four)",
        "OperatingSystemCPEName": "cpe:/o:fedoraproject:fedora:44",
        "OperatingSystemSupportEnd": 1810684800000000u64,
        "OperatingSystemReleaseData": [
            "NAME=Fedora Linux",
            "VERSION=44 (Forty Four)",
            "ID=fedora",
            "VERSION_ID=44",
            "VARIANT_ID=server",
            "SUPPORT_END=2027-05-19"
        ],
        "HardwareVendor": "QEMU",
        "HardwareModel": "Standard PC (Q35 + ICH9, 2009)",
        "FirmwareVersion": "0.0.0",
        "FirmwareVendor": "EFI Development Kit II / OVMF",
        "MachineID": "b2692de9176e4abcb3342d4e31033c51",
        "BootID": "1363d915afea4a549f28c4e3258a3364",
    }))
    .expect("serialize hostname describe");

    ok_reply(id, json!([describe_json]))
}

fn timedate_getall(args: &[Value], id: &Value) -> Value {
    let iface = args.first().and_then(Value::as_str).unwrap_or("");
    if iface != "org.freedesktop.timedate1" {
        return err_reply(
            id,
            "org.freedesktop.DBus.Error.UnknownInterface",
            format!("unknown interface: {iface}"),
        );
    }
    ok_reply(
        id,
        json!([{
            "Timezone": {"t":"s","v":"America/Chicago"},
            "LocalRTC": {"t":"b","v":false},
            "CanNTP": {"t":"b","v":true},
            "NTP": {"t":"b","v":true},
            "NTPSynchronized": {"t":"b","v":true},
            "TimeUSec": {"t":"t","v":1700006400000000u64},
            "RTCTimeUSec": {"t":"t","v":1700006400000000u64},
        }]),
    )
}
