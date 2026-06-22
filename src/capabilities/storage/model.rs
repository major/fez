//! Typed models for UDisks2 D-Bus property sets.

use crate::error::{FezError, Result};
use crate::protocol::variant::Variant;
use serde_json::Value;

/// Block device properties from `org.freedesktop.UDisks2.Block`.
#[derive(Debug, Default, serde::Deserialize)]
struct BlockProps {
    #[serde(rename = "PreferredDevice", default)]
    preferred_device: Variant<Value>,
    #[serde(rename = "Drive", default)]
    drive: Variant<String>,
    #[serde(rename = "IdType", default)]
    id_type: Variant<String>,
    #[serde(rename = "IdLabel", default)]
    id_label: Variant<String>,
    #[serde(rename = "IdUUID", default)]
    id_uuid: Variant<String>,
    #[serde(rename = "IdUsage", default)]
    id_usage: Variant<String>,
    #[serde(rename = "Size", default)]
    size: Variant<u64>,
    #[serde(rename = "ReadOnly", default)]
    read_only: Variant<bool>,
    #[serde(rename = "HintSystem", default)]
    hint_system: Variant<bool>,
}

/// Partition properties from `org.freedesktop.UDisks2.Partition`.
#[derive(Debug, Default, serde::Deserialize)]
struct PartitionProps {
    #[serde(rename = "Number", default)]
    number: Variant<u64>,
    #[serde(rename = "Type", default)]
    part_type: Variant<String>,
    #[serde(rename = "Name", default)]
    name: Variant<String>,
    #[serde(rename = "Size", default)]
    size: Variant<u64>,
    #[serde(rename = "Offset", default)]
    offset: Variant<u64>,
}

/// Partition table properties from `org.freedesktop.UDisks2.PartitionTable`.
#[derive(Debug, Default, serde::Deserialize)]
struct PartitionTableProps {
    #[serde(rename = "Type", default)]
    table_type: Variant<String>,
}

/// Filesystem properties from `org.freedesktop.UDisks2.Filesystem`.
#[derive(Debug, Default, serde::Deserialize)]
struct FilesystemProps {
    #[serde(rename = "MountPoints", default)]
    mount_points: Variant<Value>,
}

/// Drive properties from `org.freedesktop.UDisks2.Drive`.
#[derive(Debug, Default, serde::Deserialize)]
struct DriveProps {
    #[serde(rename = "Model", default)]
    model: Variant<String>,
    #[serde(rename = "Serial", default)]
    serial: Variant<String>,
    #[serde(rename = "Vendor", default)]
    vendor: Variant<String>,
    #[serde(rename = "Revision", default)]
    revision: Variant<String>,
    #[serde(rename = "Size", default)]
    size: Variant<u64>,
    #[serde(rename = "RotationRate", default)]
    rotation_rate: Variant<i64>,
    #[serde(rename = "Removable", default)]
    removable: Variant<bool>,
    #[serde(rename = "ConnectionBus", default)]
    connection_bus: Variant<String>,
}

/// Encrypted properties from `org.freedesktop.UDisks2.Encrypted`.
#[derive(Debug, Default, serde::Deserialize)]
struct EncryptedProps {
    #[serde(rename = "CleartextDevice", default)]
    cleartext_device: Variant<String>,
    #[serde(rename = "MetadataSize", default)]
    metadata_size: Variant<u64>,
}

/// NVMe controller health properties from `org.freedesktop.UDisks2.NVMe.Controller`.
#[derive(Debug, Default, serde::Deserialize)]
struct NvmeControllerProps {
    #[serde(rename = "SmartCriticalWarning", default)]
    smart_critical_warning: Variant<Vec<String>>,
    #[serde(rename = "SmartPowerOnHours", default)]
    smart_power_on_hours: Variant<u64>,
    #[serde(rename = "SmartTemperature", default)]
    smart_temperature: Variant<u64>,
    #[serde(rename = "SmartSelftestStatus", default)]
    smart_selftest_status: Variant<String>,
    #[serde(rename = "State", default)]
    state: Variant<String>,
    #[serde(rename = "NVMeRevision", default)]
    nvme_revision: Variant<String>,
}

// ── Public typed structs (rendered into envelope / human output) ─────────

/// A block device in the `storage list` output.
#[derive(Debug, Clone, serde::Serialize)]
pub(super) struct BlockDevice {
    pub(super) device: String,
    pub(super) size: u64,
    pub(super) fs_type: String,
    pub(super) label: String,
    pub(super) uuid: String,
    pub(super) mountpoint: String,
    pub(super) read_only: bool,
}

/// Full detail for one block device in `storage show`.
#[derive(Debug, Clone, serde::Serialize)]
pub(super) struct BlockDeviceDetail {
    pub(super) device: String,
    pub(super) size: u64,
    pub(super) fs_type: String,
    pub(super) fs_usage: String,
    pub(super) label: String,
    pub(super) uuid: String,
    pub(super) mountpoints: Vec<String>,
    pub(super) read_only: bool,
    pub(super) system: bool,
    pub(super) drive: Option<DriveInfo>,
    pub(super) partition: Option<PartitionInfo>,
    pub(super) partition_table: Option<PartitionTableInfo>,
    pub(super) encrypted: Option<EncryptedInfo>,
}

/// Drive detail surfaced in `storage show` and `storage health`.
#[derive(Debug, Clone, serde::Serialize)]
pub(super) struct DriveInfo {
    pub(super) model: String,
    pub(super) serial: String,
    pub(super) vendor: String,
    pub(super) revision: String,
    pub(super) size: u64,
    pub(super) rotation_rate: i64,
    pub(super) removable: bool,
    pub(super) connection_bus: String,
}

/// Partition detail surfaced in `storage show`.
#[derive(Debug, Clone, serde::Serialize)]
pub(super) struct PartitionInfo {
    pub(super) number: u64,
    #[serde(rename = "type")]
    pub(super) part_type: String,
    pub(super) name: String,
    pub(super) size: u64,
    pub(super) offset: u64,
}

/// Partition table info for whole-disk devices.
#[derive(Debug, Clone, serde::Serialize)]
pub(super) struct PartitionTableInfo {
    #[serde(rename = "type")]
    pub(super) table_type: String,
}

/// LUKS / encrypted device info.
#[derive(Debug, Clone, serde::Serialize)]
pub(super) struct EncryptedInfo {
    pub(super) cleartext_device: String,
    pub(super) metadata_size: u64,
}

/// Drive health from `storage health`.
#[derive(Debug, Clone, serde::Serialize)]
pub(super) struct DriveHealth {
    pub(super) path: String,
    pub(super) model: String,
    pub(super) serial: String,
    pub(super) size: u64,
    pub(super) rotation_rate: i64,
    pub(super) temperature_kelvin: u64,
    pub(super) power_on_hours: u64,
    pub(super) critical_warnings: Vec<String>,
    pub(super) selftest_status: String,
    pub(super) nvme_revision: String,
    pub(super) state: String,
}

// ── Decode helpers ──────────────────────────────────────────────────────

/// Decode a UDisks2 byte-array device path (`ay`) into a UTF-8 string.
///
/// UDisks2 sends device paths like `/dev/nvme0n1` as an array of byte values
/// with a trailing NUL. This extracts the string from either the
/// variant-wrapped `{"t":"ay","v":[...]}` form or a raw JSON array of ints.
pub(super) fn decode_ay_path(val: &Value) -> String {
    let arr = match val {
        Value::Array(a) => a,
        _ => return String::new(),
    };
    let bytes: Vec<u8> = arr
        .iter()
        .filter_map(|v| v.as_u64().and_then(|n| u8::try_from(n).ok()))
        .collect();
    // Strip trailing NUL bytes.
    let end = bytes.iter().rposition(|&b| b != 0).map_or(0, |pos| pos + 1);
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// Decode mount points from UDisks2's `MountPoints` property (`aay`).
///
/// Each mount point is a byte-array with a trailing NUL, wrapped in an outer
/// array. The variant envelope is already stripped by the `Variant<Value>`
/// layer, so we just need to handle the inner `aay`.
pub(super) fn decode_mount_points(val: &Value) -> Vec<String> {
    let arr = match val {
        Value::Array(a) => a,
        _ => return Vec::new(),
    };
    arr.iter()
        .map(decode_ay_path)
        .filter(|s| !s.is_empty())
        .collect()
}

/// Treat the UDisks2 null object path `"/"` as absent.
fn udisks_path(s: &str) -> Option<&str> {
    if s.is_empty() || s == "/" {
        None
    } else {
        Some(s)
    }
}

/// Describe the rotation rate for human output.
pub(super) fn rotation_description(rate: i64) -> &'static str {
    match rate {
        0 => "SSD/NVMe",
        -1 => "unknown",
        _ => "rotating",
    }
}

impl BlockDevice {
    /// Build a list-view block device from D-Bus property values.
    ///
    /// # Errors
    ///
    /// Returns a decode error if the block or filesystem JSON does not match
    /// the expected property schema.
    pub(super) fn from_value(block_val: Value, fs_val: Option<Value>) -> Result<Self> {
        let props: BlockProps = serde_json::from_value(block_val).map_err(FezError::Decode)?;
        let device = decode_ay_path(&props.preferred_device.0);
        let mountpoint = fs_val
            .map(|v| -> Result<String> {
                let fp: FilesystemProps = serde_json::from_value(v).map_err(FezError::Decode)?;
                Ok(decode_mount_points(&fp.mount_points.0)
                    .into_iter()
                    .next()
                    .unwrap_or_default())
            })
            .transpose()?
            .unwrap_or_default();
        Ok(Self {
            device,
            size: props.size.0,
            fs_type: props.id_type.0,
            label: props.id_label.0,
            uuid: props.id_uuid.0,
            mountpoint,
            read_only: props.read_only.0,
        })
    }
}

impl BlockDeviceDetail {
    /// Build a detail-view block device from D-Bus property values.
    ///
    /// # Errors
    ///
    /// Returns a decode error if any of the provided JSON values do not match
    /// their expected property schemas.
    pub(super) fn from_values(
        block_val: Value,
        fs_val: Option<Value>,
        drive_val: Option<Value>,
        partition_val: Option<Value>,
        ptable_val: Option<Value>,
        encrypted_val: Option<Value>,
    ) -> Result<Self> {
        let props: BlockProps = serde_json::from_value(block_val).map_err(FezError::Decode)?;
        let device = decode_ay_path(&props.preferred_device.0);
        let mountpoints = fs_val
            .as_ref()
            .map(|v| -> Result<Vec<String>> {
                let fp: FilesystemProps =
                    serde_json::from_value(v.clone()).map_err(FezError::Decode)?;
                Ok(decode_mount_points(&fp.mount_points.0))
            })
            .transpose()?
            .unwrap_or_default();
        let drive = match (udisks_path(&props.drive.0), drive_val) {
            (Some(_), Some(dv)) => Some(DriveInfo::from_value(dv)?),
            _ => None,
        };
        let partition = partition_val.map(PartitionInfo::from_value).transpose()?;
        let partition_table = ptable_val.map(PartitionTableInfo::from_value).transpose()?;
        let encrypted = encrypted_val.map(EncryptedInfo::from_value).transpose()?;
        Ok(Self {
            device,
            size: props.size.0,
            fs_type: props.id_type.0,
            fs_usage: props.id_usage.0,
            label: props.id_label.0,
            uuid: props.id_uuid.0,
            mountpoints,
            read_only: props.read_only.0,
            system: props.hint_system.0,
            drive,
            partition,
            partition_table,
            encrypted,
        })
    }
}

impl DriveInfo {
    /// Build drive info from D-Bus property values.
    ///
    /// # Errors
    ///
    /// Returns a decode error if the JSON does not match the expected schema.
    pub(super) fn from_value(val: Value) -> Result<Self> {
        let props: DriveProps = serde_json::from_value(val).map_err(FezError::Decode)?;
        Ok(Self {
            model: props.model.0,
            serial: props.serial.0,
            vendor: props.vendor.0,
            revision: props.revision.0,
            size: props.size.0,
            rotation_rate: props.rotation_rate.0,
            removable: props.removable.0,
            connection_bus: props.connection_bus.0,
        })
    }
}

impl PartitionInfo {
    fn from_value(val: Value) -> Result<Self> {
        let props: PartitionProps = serde_json::from_value(val).map_err(FezError::Decode)?;
        Ok(Self {
            number: props.number.0,
            part_type: props.part_type.0,
            name: props.name.0,
            size: props.size.0,
            offset: props.offset.0,
        })
    }
}

impl PartitionTableInfo {
    fn from_value(val: Value) -> Result<Self> {
        let props: PartitionTableProps = serde_json::from_value(val).map_err(FezError::Decode)?;
        Ok(Self {
            table_type: props.table_type.0,
        })
    }
}

impl EncryptedInfo {
    fn from_value(val: Value) -> Result<Self> {
        let props: EncryptedProps = serde_json::from_value(val).map_err(FezError::Decode)?;
        Ok(Self {
            cleartext_device: props.cleartext_device.0,
            metadata_size: props.metadata_size.0,
        })
    }
}

impl DriveHealth {
    /// Build drive health from D-Bus drive and NVMe controller property values.
    ///
    /// # Errors
    ///
    /// Returns a decode error if the JSON does not match the expected schema.
    pub(super) fn from_values(path: String, drive_val: Value, nvme_val: Value) -> Result<Self> {
        let drive: DriveProps = serde_json::from_value(drive_val).map_err(FezError::Decode)?;
        let nvme: NvmeControllerProps =
            serde_json::from_value(nvme_val).map_err(FezError::Decode)?;
        Ok(Self {
            path,
            model: drive.model.0,
            serial: drive.serial.0,
            size: drive.size.0,
            rotation_rate: drive.rotation_rate.0,
            temperature_kelvin: nvme.smart_temperature.0,
            power_on_hours: nvme.smart_power_on_hours.0,
            critical_warnings: nvme.smart_critical_warning.0,
            selftest_status: nvme.smart_selftest_status.0,
            nvme_revision: nvme.nvme_revision.0,
            state: nvme.state.0,
        })
    }
}

// ── Size formatting ─────────────────────────────────────────────────────

/// Format a byte count as a human-readable size string.
pub(super) fn format_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;
    const TIB: u64 = GIB * 1024;
    if bytes >= TIB {
        format!("{:.1} TiB", bytes as f64 / TIB as f64)
    } else if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decode_ay_path_strips_nul_and_converts() {
        let path = json!([47, 100, 101, 118, 47, 115, 100, 97, 0]);
        assert_eq!(decode_ay_path(&path), "/dev/sda");
    }

    #[test]
    fn decode_ay_path_handles_empty() {
        assert_eq!(decode_ay_path(&json!([])), "");
        assert_eq!(decode_ay_path(&json!("not-array")), "");
    }

    #[test]
    fn decode_mount_points_handles_aay() {
        let mounts = json!([[47, 98, 111, 111, 116, 0], [47, 0]]);
        let result = decode_mount_points(&mounts);
        assert_eq!(result, vec!["/boot", "/"]);
    }

    #[test]
    fn udisks_path_treats_slash_as_absent() {
        assert_eq!(udisks_path("/"), None);
        assert_eq!(udisks_path(""), None);
        assert_eq!(
            udisks_path("/org/freedesktop/UDisks2/drives/X"),
            Some("/org/freedesktop/UDisks2/drives/X")
        );
    }

    #[test]
    fn format_size_tiers() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KiB");
        assert_eq!(format_size(1024 * 1024), "1.0 MiB");
        assert_eq!(format_size(2 * 1024 * 1024 * 1024), "2.0 GiB");
        assert_eq!(format_size(4_000_787_030_016), "3.6 TiB");
    }

    #[test]
    fn rotation_description_decodes_rates() {
        assert_eq!(rotation_description(0), "SSD/NVMe");
        assert_eq!(rotation_description(-1), "unknown");
        assert_eq!(rotation_description(7200), "rotating");
    }

    #[test]
    fn block_device_from_value_decodes() {
        let block = json!({
            "PreferredDevice": {"t":"ay","v":[47,100,101,118,47,115,100,97,0]},
            "Device": {"t":"ay","v":[47,100,101,118,47,115,100,97,0]},
            "Drive": {"t":"o","v":"/"},
            "IdType": {"t":"s","v":"ext4"},
            "IdLabel": {"t":"s","v":"root"},
            "IdUUID": {"t":"s","v":"abc-123"},
            "IdUsage": {"t":"s","v":"filesystem"},
            "Size": {"t":"t","v":1073741824},
            "ReadOnly": {"t":"b","v":false},
            "HintSystem": {"t":"b","v":true},
        });
        let dev = BlockDevice::from_value(block, None).unwrap();
        assert_eq!(dev.device, "/dev/sda");
        assert_eq!(dev.fs_type, "ext4");
        assert_eq!(dev.size, 1073741824);
        assert!(!dev.read_only);
    }

    #[test]
    fn drive_health_from_values_decodes() {
        let drive = json!({
            "Model": {"t":"s","v":"Samsung SSD 990 PRO 4TB"},
            "Serial": {"t":"s","v":"S7K"},
            "Vendor": {"t":"s","v":""},
            "Revision": {"t":"s","v":"4B2Q"},
            "Size": {"t":"t","v":4000787030016u64},
            "RotationRate": {"t":"i","v":0},
            "Removable": {"t":"b","v":false},
            "ConnectionBus": {"t":"s","v":""},
            "Id": {"t":"s","v":"Samsung-SSD"},
            "Seat": {"t":"s","v":"seat0"},
            "SortKey": {"t":"s","v":"00coldplug"},
        });
        let nvme = json!({
            "SmartCriticalWarning": {"t":"as","v":[]},
            "SmartPowerOnHours": {"t":"t","v":5522},
            "SmartTemperature": {"t":"q","v":322},
            "SmartSelftestStatus": {"t":"s","v":"success"},
            "SmartUpdated": {"t":"t","v":1700000000u64},
            "State": {"t":"s","v":"live"},
            "NVMeRevision": {"t":"s","v":"2.0"},
        });
        let health =
            DriveHealth::from_values("/org/freedesktop/UDisks2/drives/X".into(), drive, nvme)
                .unwrap();
        assert_eq!(health.model, "Samsung SSD 990 PRO 4TB");
        assert_eq!(health.power_on_hours, 5522);
        assert_eq!(health.temperature_kelvin, 322);
        assert_eq!(health.selftest_status, "success");
    }
}
