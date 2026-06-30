//! Canned fwupd replies for firmware tests.

use super::{err_reply, ok_reply};
use serde_json::{json, Value};

pub(super) const FWUPD_PATH: &str = "/org/freedesktop/fwupd";

/// Canned reply for fwupd D-Bus calls.
pub(super) fn fwupd_reply(
    path: &str,
    _iface: &str,
    method: &str,
    args: &[Value],
    id: &Value,
) -> Value {
    match (path, method) {
        (FWUPD_PATH, "GetDevices") => get_devices(id),
        (FWUPD_PATH, "GetHostSecurityAttrs") => get_host_security_attrs(id),
        (FWUPD_PATH, "GetUpgrades") => get_upgrades(args, id),
        (FWUPD_PATH, "Get") => get_property(args, id),
        _ => err_reply(
            id,
            "org.freedesktop.DBus.Error.UnknownMethod",
            format!("no fwupd fake for {path} {method}"),
        ),
    }
}

fn get_devices(id: &Value) -> Value {
    ok_reply(
        id,
        json!([[
            {
                "Name": {"t":"s","v":"UEFI Device Firmware"},
                "DeviceId": {"t":"s","v":"abc123"},
                "Vendor": {"t":"s","v":"Dell Inc."},
                "Version": {"t":"s","v":"1.10.1"},
                "Flags": {"t":"t","v":2},
            },
            {
                "Name": {"t":"s","v":"System Firmware"},
                "DeviceId": {"t":"s","v":"def456"},
                "Vendor": {"t":"s","v":"Dell Inc."},
                "Version": {"t":"s","v":"2.5.0"},
                "Flags": {"t":"t","v":0},
            },
        ]]),
    )
}

fn get_host_security_attrs(id: &Value) -> Value {
    ok_reply(
        id,
        json!([[
            {
                "AppstreamId": {"t":"s","v":"org.fwupd.hsi.Tpm"},
                "Name": {"t":"s","v":"TPM v2.0"},
                "HsiResult": {"t":"s","v":"enabled"},
                "HsiLevel": {"t":"u","v":1},
            },
            {
                "AppstreamId": {"t":"s","v":"org.fwupd.hsi.SecureBoot"},
                "Name": {"t":"s","v":"Secure Boot"},
                "HsiResult": {"t":"s","v":"enabled"},
                "HsiLevel": {"t":"u","v":1},
            },
            {
                "AppstreamId": {"t":"s","v":"org.fwupd.hsi.Iommu"},
                "Name": {"t":"s","v":"IOMMU"},
                "HsiResult": {"t":"s","v":"not-enabled"},
                "HsiLevel": {"t":"u","v":3},
            },
        ]]),
    )
}

fn get_upgrades(args: &[Value], id: &Value) -> Value {
    let device_id = args.first().and_then(Value::as_str).unwrap_or("");
    if device_id == "abc123" {
        ok_reply(
            id,
            json!([[
                {
                    "Name": {"t":"s","v":"UEFI Device Firmware"},
                    "Version": {"t":"s","v":"1.11.0"},
                    "Description": {"t":"s","v":"Security fix"},
                    "Vendor": {"t":"s","v":"Dell Inc."},
                },
            ]]),
        )
    } else {
        // Non-updatable or unknown device: empty upgrades
        ok_reply(id, json!([[]]))
    }
}

fn get_property(args: &[Value], id: &Value) -> Value {
    let prop = args.get(1).and_then(Value::as_str).unwrap_or("");
    if prop == "HostSecurityId" {
        ok_reply(id, json!([{"t":"s","v":"HSI:1 (v2.1.3)"}]))
    } else {
        err_reply(
            id,
            "org.freedesktop.DBus.Error.UnknownProperty",
            format!("no fake for {prop}"),
        )
    }
}
