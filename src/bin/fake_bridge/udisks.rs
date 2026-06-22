//! Canned UDisks2 (`org.freedesktop.UDisks2`) replies for storage tests.

use super::{err_reply, ok_reply, ROOT_PATH};
use serde_json::{json, Value};

/// UDisks2 manager object path.
pub(super) const UDISKS_MGR_PATH: &str = "/org/freedesktop/UDisks2/Manager";
/// Block device path prefix.
const BLOCK_PREFIX: &str = "/org/freedesktop/UDisks2/block_devices/";
/// Drive path prefix.
const DRIVE_PREFIX: &str = "/org/freedesktop/UDisks2/drives/";
/// Canned drive path.
const DRIVE_PATH: &str = "/org/freedesktop/UDisks2/drives/Samsung_SSD_990_PRO_4TB_S7KGNU0X524885F";

/// Canned UDisks2 reply for a call against a UDisks2 object path.
///
/// Canned topology:
/// - `nvme0n1` - whole disk, GPT partition table, 4 TB NVMe SSD.
/// - `nvme0n1p1` - partition 1, vfat, mounted at /boot/efi.
/// - `nvme0n1p2` - partition 2, ext4, mounted at /boot.
/// - `nvme0n1p3` - partition 3, crypto_LUKS, LUKS2 encrypted.
/// - `dm_2d0` - dm device, ext4, LUKS cleartext, mounted at /.
pub(super) fn udisks_reply(
    path: &str,
    _iface: &str,
    method: &str,
    args: &[Value],
    id: &Value,
) -> Value {
    if path == UDISKS_MGR_PATH {
        return manager_reply(method, id);
    }
    if let Some(dev) = path.strip_prefix(BLOCK_PREFIX) {
        return block_reply(dev, method, args, id);
    }
    if path.starts_with(DRIVE_PREFIX) {
        return drive_reply(method, args, id);
    }
    udisks_unknown(path, method, id)
}

fn manager_reply(method: &str, id: &Value) -> Value {
    match method {
        "GetBlockDevices" => ok_reply(
            id,
            json!([[
                format!("{BLOCK_PREFIX}nvme0n1"),
                format!("{BLOCK_PREFIX}nvme0n1p1"),
                format!("{BLOCK_PREFIX}nvme0n1p2"),
                format!("{BLOCK_PREFIX}nvme0n1p3"),
                format!("{BLOCK_PREFIX}dm_2d0"),
            ]]),
        ),
        "GetDrives" => ok_reply(id, json!([[DRIVE_PATH]])),
        _ => udisks_unknown(UDISKS_MGR_PATH, method, id),
    }
}

fn block_reply(dev: &str, method: &str, args: &[Value], id: &Value) -> Value {
    if method != "GetAll" {
        return udisks_unknown(dev, method, id);
    }
    // GetAll takes the target interface name as its first argument.
    let target_iface = args.first().and_then(Value::as_str).unwrap_or("");
    match target_iface {
        "org.freedesktop.UDisks2.Block" => block_props(dev, id),
        "org.freedesktop.UDisks2.Filesystem" => fs_props(dev, id),
        "org.freedesktop.UDisks2.Partition" => partition_props(dev, id),
        "org.freedesktop.UDisks2.PartitionTable" => ptable_props(dev, id),
        "org.freedesktop.UDisks2.Encrypted" => encrypted_props(dev, id),
        _ => err_reply(
            id,
            "org.freedesktop.DBus.Error.UnknownInterface",
            format!("no fake for {target_iface} on {dev}"),
        ),
    }
}

/// Encode a device path string as the UDisks2 byte-array format (JSON array of u8 + NUL).
fn encode_ay(path: &str) -> Value {
    let mut bytes: Vec<Value> = path.bytes().map(|b| json!(b)).collect();
    bytes.push(json!(0));
    Value::Array(bytes)
}

fn block_props(dev: &str, id: &Value) -> Value {
    let (dev_path, drive, id_type, id_label, id_uuid, id_usage, size, ro, system) = match dev {
        "nvme0n1" => (
            "/dev/nvme0n1",
            DRIVE_PATH,
            "",
            "",
            "",
            "",
            4_000_787_030_016u64,
            false,
            true,
        ),
        "nvme0n1p1" => (
            "/dev/nvme0n1p1",
            DRIVE_PATH,
            "vfat",
            "EFI",
            "D7E3-1F6A",
            "filesystem",
            629_145_600u64,
            false,
            true,
        ),
        "nvme0n1p2" => (
            "/dev/nvme0n1p2",
            DRIVE_PATH,
            "ext4",
            "",
            "510d6b6e-7088-4593-8da4-777d0e8ed7b7",
            "filesystem",
            1_073_741_824u64,
            false,
            true,
        ),
        "nvme0n1p3" => (
            "/dev/nvme0n1p3",
            DRIVE_PATH,
            "crypto_LUKS",
            "",
            "abcd-1234",
            "crypto",
            3_998_082_957_312u64,
            false,
            true,
        ),
        "dm_2d0" => (
            "/dev/dm-0",
            ROOT_PATH,
            "ext4",
            "fedora_root",
            "11111111-2222-3333-4444-555555555555",
            "filesystem",
            3_998_066_180_096u64,
            false,
            true,
        ),
        _ => {
            return err_reply(
                id,
                "org.freedesktop.DBus.Error.UnknownObject",
                format!("no fake block device {dev}"),
            )
        }
    };
    ok_reply(
        id,
        json!([{
            "Device": {"t":"ay","v": encode_ay(dev_path)},
            "PreferredDevice": {"t":"ay","v": encode_ay(dev_path)},
            "Drive": {"t":"o","v": drive},
            "IdType": {"t":"s","v": id_type},
            "IdLabel": {"t":"s","v": id_label},
            "IdUUID": {"t":"s","v": id_uuid},
            "IdUsage": {"t":"s","v": id_usage},
            "Size": {"t":"t","v": size},
            "ReadOnly": {"t":"b","v": ro},
            "HintSystem": {"t":"b","v": system},
        }]),
    )
}

fn fs_props(dev: &str, id: &Value) -> Value {
    let mounts = match dev {
        "nvme0n1p1" => json!([encode_ay("/boot/efi")]),
        "nvme0n1p2" => json!([encode_ay("/boot")]),
        "dm_2d0" => json!([encode_ay("/")]),
        // nvme0n1 and nvme0n1p3 have no filesystem
        _ => {
            return err_reply(
                id,
                "org.freedesktop.DBus.Error.UnknownInterface",
                format!("no Filesystem on {dev}"),
            )
        }
    };
    ok_reply(
        id,
        json!([{
            "MountPoints": {"t":"aay","v": mounts},
            "Size": {"t":"t","v": 0},
        }]),
    )
}

fn partition_props(dev: &str, id: &Value) -> Value {
    let (number, ptype, name, size, offset) = match dev {
        "nvme0n1p1" => (
            1u64,
            "c12a7328-f81f-11d2-ba4b-00a0c93ec93b",
            "EFI System Partition",
            629_145_600u64,
            1_048_576u64,
        ),
        "nvme0n1p2" => (2, "", "", 1_073_741_824, 630_194_176),
        "nvme0n1p3" => (3, "", "", 3_998_082_957_312u64, 1_703_936_000),
        _ => {
            return err_reply(
                id,
                "org.freedesktop.DBus.Error.UnknownInterface",
                format!("no Partition on {dev}"),
            )
        }
    };
    ok_reply(
        id,
        json!([{
            "Number": {"t":"u","v": number},
            "Type": {"t":"s","v": ptype},
            "Name": {"t":"s","v": name},
            "Size": {"t":"t","v": size},
            "Offset": {"t":"t","v": offset},
            "Table": {"t":"o","v": format!("{BLOCK_PREFIX}nvme0n1")},
        }]),
    )
}

fn ptable_props(dev: &str, id: &Value) -> Value {
    if dev == "nvme0n1" {
        ok_reply(
            id,
            json!([{
                "Type": {"t":"s","v":"gpt"},
            }]),
        )
    } else {
        err_reply(
            id,
            "org.freedesktop.DBus.Error.UnknownInterface",
            format!("no PartitionTable on {dev}"),
        )
    }
}

fn encrypted_props(dev: &str, id: &Value) -> Value {
    if dev == "nvme0n1p3" {
        ok_reply(
            id,
            json!([{
                "CleartextDevice": {"t":"o","v": format!("{BLOCK_PREFIX}dm_2d0")},
                "MetadataSize": {"t":"t","v": 16_777_216},
            }]),
        )
    } else {
        err_reply(
            id,
            "org.freedesktop.DBus.Error.UnknownInterface",
            format!("no Encrypted on {dev}"),
        )
    }
}

fn drive_reply(method: &str, args: &[Value], id: &Value) -> Value {
    if method != "GetAll" {
        return udisks_unknown(DRIVE_PATH, method, id);
    }
    let target_iface = args.first().and_then(Value::as_str).unwrap_or("");
    match target_iface {
        "org.freedesktop.UDisks2.Drive" => ok_reply(
            id,
            json!([{
                "Model": {"t":"s","v":"Samsung SSD 990 PRO 4TB"},
                "Serial": {"t":"s","v":"S7KGNU0X524885F"},
                "Vendor": {"t":"s","v":""},
                "Revision": {"t":"s","v":"4B2QJXD7"},
                "Size": {"t":"t","v": 4_000_787_030_016u64},
                "RotationRate": {"t":"i","v": 0},
                "Removable": {"t":"b","v": false},
                "ConnectionBus": {"t":"s","v":""},
                "Id": {"t":"s","v":"Samsung-SSD-990-PRO-4TB-S7KGNU0X524885F"},
                "Seat": {"t":"s","v":"seat0"},
                "SortKey": {"t":"s","v":"00coldplug/00fixed/nvme0"},
            }]),
        ),
        "org.freedesktop.UDisks2.NVMe.Controller" => ok_reply(
            id,
            json!([{
                "SmartCriticalWarning": {"t":"as","v":[]},
                "SmartPowerOnHours": {"t":"t","v": 5522},
                "SmartTemperature": {"t":"q","v": 322},
                "SmartSelftestStatus": {"t":"s","v":"success"},
                "SmartUpdated": {"t":"t","v": 1700000000u64},
                "State": {"t":"s","v":"live"},
                "NVMeRevision": {"t":"s","v":"2.0"},
            }]),
        ),
        _ => err_reply(
            id,
            "org.freedesktop.DBus.Error.UnknownInterface",
            format!("no fake for drive iface {target_iface}"),
        ),
    }
}

fn udisks_unknown(path: &str, method: &str, id: &Value) -> Value {
    err_reply(
        id,
        "org.freedesktop.DBus.Error.UnknownMethod",
        format!("no UDisks2 fake for {path} {method}"),
    )
}
