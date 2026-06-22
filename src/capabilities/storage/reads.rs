//! Read-only storage queries against UDisks2.

use super::model::{
    decode_ay_path, format_size, rotation_description, BlockDevice, BlockDeviceDetail, DriveHealth,
};
use super::{
    BLOCK_IFACE, DRIVE_IFACE, ENCRYPTED_IFACE, FS_IFACE, NVME_CTRL_IFACE, PARTITION_IFACE,
    PROPS_IFACE, PTABLE_IFACE, UDISKS_MGR_IFACE, UDISKS_MGR_PATH,
};
use crate::capabilities::{CapabilityContext, View};
use crate::error::{FezError, Result};
use crate::protocol::client::BridgeClient;
use serde_json::{json, Value};

/// `GetAll` the properties of a UDisks2 object, returning the unwrapped dict.
fn get_all(client: &mut BridgeClient, channel: &str, path: &str, iface: &str) -> Result<Value> {
    let out = client.dbus_call(channel, path, PROPS_IFACE, "GetAll", json!([iface]))?;
    out.get(0)
        .cloned()
        .ok_or_else(|| FezError::Problem(format!("GetAll({iface}) returned no value")))
}

/// Try `GetAll` and return `None` on `UnknownInterface` (interface not present
/// on this object), `Some(val)` on success, or propagate other errors.
fn try_get_all(
    client: &mut BridgeClient,
    channel: &str,
    path: &str,
    iface: &str,
) -> Result<Option<Value>> {
    match get_all(client, channel, path, iface) {
        Ok(v) => Ok(Some(v)),
        Err(FezError::Dbus { ref name, .. })
            if name.contains("UnknownInterface") || name.contains("UnknownProperty") =>
        {
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

/// Call `GetBlockDevices` on the UDisks2 manager, returning object paths.
fn block_device_paths(client: &mut BridgeClient, channel: &str) -> Result<Vec<String>> {
    let out = client.dbus_call(
        channel,
        UDISKS_MGR_PATH,
        UDISKS_MGR_IFACE,
        "GetBlockDevices",
        json!([{}]),
    )?;
    let arr = out
        .get(0)
        .and_then(Value::as_array)
        .ok_or_else(|| FezError::Problem("GetBlockDevices returned a non-array".into()))?;
    Ok(arr
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect())
}

/// Call `GetDrives` on the UDisks2 manager, returning drive object paths.
fn drive_paths(client: &mut BridgeClient, channel: &str) -> Result<Vec<String>> {
    let out = client.dbus_call(
        channel,
        UDISKS_MGR_PATH,
        UDISKS_MGR_IFACE,
        "GetDrives",
        json!([{}]),
    )?;
    let arr = out
        .get(0)
        .and_then(Value::as_array)
        .ok_or_else(|| FezError::Problem("GetDrives returned a non-array".into()))?;
    Ok(arr
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect())
}

/// `storage list` — inventory of block devices with basic info.
///
/// # Errors
///
/// Returns an error if the D-Bus call to enumerate block devices fails or
/// if any device properties cannot be decoded.
pub(super) fn list(ctx: &mut CapabilityContext<'_>) -> Result<View> {
    let paths = block_device_paths(ctx.client, ctx.channel)?;

    let mut devices = Vec::new();
    for path in &paths {
        let block = get_all(ctx.client, ctx.channel, path, BLOCK_IFACE)?;
        let fs = try_get_all(ctx.client, ctx.channel, path, FS_IFACE)?;
        devices.push(BlockDevice::from_value(block, fs)?);
    }

    // Sort by device path for stable output.
    devices.sort_by(|a, b| a.device.cmp(&b.device));

    let mut human = format!(
        "{:<18} {:>10} {:<8} {:<16} {}\n",
        "DEVICE", "SIZE", "FSTYPE", "LABEL", "MOUNTPOINT"
    );
    for d in &devices {
        human.push_str(&format!(
            "{:<18} {:>10} {:<8} {:<16} {}\n",
            d.device,
            format_size(d.size),
            d.fs_type,
            d.label,
            d.mountpoint,
        ));
    }

    let columns = [
        "device",
        "size",
        "fs_type",
        "label",
        "uuid",
        "mountpoint",
        "read_only",
    ];
    let rows: Vec<Value> = devices
        .iter()
        .map(|d| {
            json!([
                d.device,
                d.size,
                d.fs_type,
                d.label,
                d.uuid,
                d.mountpoint,
                d.read_only,
            ])
        })
        .collect();

    Ok(View::new(
        "StorageDeviceList",
        ctx.host,
        crate::envelope::table_data(&columns, rows),
        human,
    ))
}

/// `storage show <device>` — full detail for one block device.
///
/// # Errors
///
/// Returns `NotFound` if the target device does not exist, or an error if
/// D-Bus calls to retrieve device properties fail.
pub(super) fn show(ctx: &mut CapabilityContext<'_>, target: &str) -> Result<View> {
    let paths = block_device_paths(ctx.client, ctx.channel)?;

    // Find the block device whose decoded PreferredDevice path ends with the
    // target name (so both `/dev/sda1` and `sda1` match).
    let mut found_path: Option<String> = None;
    let mut found_block: Option<Value> = None;
    for path in &paths {
        let block = get_all(ctx.client, ctx.channel, path, BLOCK_IFACE)?;
        // Peek at the preferred device to match.
        let pdev = block
            .get("PreferredDevice")
            .and_then(|v| v.get("v").or(Some(v)))
            .cloned()
            .unwrap_or(Value::Null);
        let dev_str = decode_ay_path(&pdev);
        if dev_str == target || dev_str.ends_with(&format!("/{target}")) {
            found_path = Some(path.clone());
            found_block = Some(block);
            break;
        }
    }
    let (obj_path, block_val) = match (found_path, found_block) {
        (Some(p), Some(b)) => (p, b),
        _ => return Err(FezError::NotFound(format!("block device {target}"))),
    };

    let fs = try_get_all(ctx.client, ctx.channel, &obj_path, FS_IFACE)?;
    let partition = try_get_all(ctx.client, ctx.channel, &obj_path, PARTITION_IFACE)?;
    let ptable = try_get_all(ctx.client, ctx.channel, &obj_path, PTABLE_IFACE)?;
    let encrypted = try_get_all(ctx.client, ctx.channel, &obj_path, ENCRYPTED_IFACE)?;

    // Resolve drive path from block properties.
    let drive_path = block_val
        .get("Drive")
        .and_then(|v| v.get("v").or(Some(v)))
        .and_then(Value::as_str)
        .unwrap_or("/");
    let drive = if drive_path != "/" && !drive_path.is_empty() {
        try_get_all(ctx.client, ctx.channel, drive_path, DRIVE_IFACE)?
    } else {
        None
    };

    let detail =
        BlockDeviceDetail::from_values(block_val, fs, drive, partition, ptable, encrypted)?;
    let human = render_show_human(&detail);
    let data = serde_json::to_value(&detail).map_err(FezError::Decode)?;

    Ok(View::new("StorageDeviceDetail", ctx.host, data, human))
}

/// `storage health [<drive>]` — SMART/NVMe health for drives.
///
/// # Errors
///
/// Returns `NotFound` if a filter is given but no drives match, or an error
/// if D-Bus calls to retrieve drive health data fail.
pub(super) fn health(ctx: &mut CapabilityContext<'_>, filter: Option<&str>) -> Result<View> {
    let paths = drive_paths(ctx.client, ctx.channel)?;

    let mut results: Vec<DriveHealth> = Vec::new();
    for path in &paths {
        let drive_val = get_all(ctx.client, ctx.channel, path, DRIVE_IFACE)?;
        // Try NVMe controller interface; skip drives without it.
        let nvme_val = match try_get_all(ctx.client, ctx.channel, path, NVME_CTRL_IFACE)? {
            Some(v) => v,
            None => continue,
        };
        let health = DriveHealth::from_values(path.clone(), drive_val, nvme_val)?;
        // If a filter is specified, match on model, serial, or path suffix.
        if let Some(f) = filter {
            let f_lower = f.to_lowercase();
            let matches = health.model.to_lowercase().contains(&f_lower)
                || health.serial.to_lowercase().contains(&f_lower)
                || path.to_lowercase().contains(&f_lower);
            if !matches {
                continue;
            }
        }
        results.push(health);
    }

    if let Some(f) = filter {
        if results.is_empty() {
            return Err(FezError::NotFound(format!("drive matching {f}")));
        }
    }

    let mut human = String::new();
    for h in &results {
        let temp_c = if h.temperature_kelvin > 0 {
            format!("{}°C", h.temperature_kelvin.saturating_sub(273))
        } else {
            "N/A".into()
        };
        human.push_str(&format!("Drive:       {}\n", h.model));
        human.push_str(&format!("Serial:      {}\n", h.serial));
        human.push_str(&format!("Size:        {}\n", format_size(h.size)));
        human.push_str(&format!(
            "Type:        {}\n",
            rotation_description(h.rotation_rate)
        ));
        human.push_str(&format!("Temperature: {temp_c}\n"));
        human.push_str(&format!("Power-on:    {} hours\n", h.power_on_hours));
        human.push_str(&format!("Self-test:   {}\n", h.selftest_status));
        human.push_str(&format!("State:       {}\n", h.state));
        if h.critical_warnings.is_empty() {
            human.push_str("Warnings:    none\n");
        } else {
            human.push_str(&format!(
                "Warnings:    {}\n",
                h.critical_warnings.join(", ")
            ));
        }
        human.push('\n');
    }

    let data = serde_json::to_value(&results).map_err(FezError::Decode)?;
    Ok(View::new("StorageHealth", ctx.host, data, human))
}

/// Render the human form of `storage show`.
fn render_show_human(d: &BlockDeviceDetail) -> String {
    let mut out = String::new();
    out.push_str(&format!("Device:      {}\n", d.device));
    out.push_str(&format!("Size:        {}\n", format_size(d.size)));
    out.push_str(&format!("Filesystem:  {}\n", d.fs_type));
    out.push_str(&format!("Usage:       {}\n", d.fs_usage));
    out.push_str(&format!("Label:       {}\n", d.label));
    out.push_str(&format!("UUID:        {}\n", d.uuid));
    out.push_str(&format!("Read-only:   {}\n", d.read_only));
    out.push_str(&format!("System:      {}\n", d.system));
    if d.mountpoints.is_empty() {
        out.push_str("Mountpoints: (none)\n");
    } else {
        out.push_str(&format!("Mountpoints: {}\n", d.mountpoints.join(", ")));
    }
    if let Some(pt) = &d.partition_table {
        out.push_str(&format!("Part. table: {}\n", pt.table_type));
    }
    if let Some(p) = &d.partition {
        out.push_str(&format!(
            "Partition:   #{} ({}, {})\n",
            p.number,
            if p.name.is_empty() {
                &p.part_type
            } else {
                &p.name
            },
            format_size(p.size),
        ));
    }
    if let Some(e) = &d.encrypted {
        let ct = if e.cleartext_device == "/" || e.cleartext_device.is_empty() {
            "locked"
        } else {
            &e.cleartext_device
        };
        out.push_str(&format!("Encrypted:   cleartext={ct}\n"));
    }
    if let Some(drv) = &d.drive {
        out.push_str(&format!("Drive:       {} ({})\n", drv.model, drv.serial));
        out.push_str(&format!(
            "Type:        {}\n",
            rotation_description(drv.rotation_rate)
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::storage::model::{
        DriveInfo, EncryptedInfo, PartitionInfo, PartitionTableInfo,
    };

    #[test]
    fn render_show_human_covers_all_sections() {
        let detail = BlockDeviceDetail {
            device: "/dev/nvme0n1p1".into(),
            size: 629145600,
            fs_type: "vfat".into(),
            fs_usage: "filesystem".into(),
            label: "EFI".into(),
            uuid: "D7E3-1F6A".into(),
            mountpoints: vec!["/boot/efi".into()],
            read_only: false,
            system: true,
            drive: Some(DriveInfo {
                model: "Samsung SSD 990 PRO".into(),
                serial: "S7K".into(),
                vendor: String::new(),
                revision: "4B2Q".into(),
                size: 4_000_787_030_016,
                rotation_rate: 0,
                removable: false,
                connection_bus: String::new(),
            }),
            partition: Some(PartitionInfo {
                number: 1,
                part_type: "c12a7328-f81f-11d2-ba4b-00a0c93ec93b".into(),
                name: "EFI System Partition".into(),
                size: 629145600,
                offset: 1048576,
            }),
            partition_table: None,
            encrypted: None,
        };
        let human = render_show_human(&detail);
        assert!(human.contains("Device:      /dev/nvme0n1p1"));
        assert!(human.contains("EFI System Partition"));
        assert!(human.contains("Samsung SSD 990 PRO"));
        assert!(human.contains("SSD/NVMe"));
        assert!(human.contains("/boot/efi"));
    }

    #[test]
    fn render_show_human_encrypted_locked() {
        let detail = BlockDeviceDetail {
            device: "/dev/nvme0n1p3".into(),
            size: 1024,
            fs_type: "crypto_LUKS".into(),
            fs_usage: "crypto".into(),
            label: String::new(),
            uuid: "abc".into(),
            mountpoints: vec![],
            read_only: false,
            system: true,
            drive: None,
            partition: None,
            partition_table: None,
            encrypted: Some(EncryptedInfo {
                cleartext_device: "/".into(),
                metadata_size: 16777216,
            }),
        };
        let human = render_show_human(&detail);
        assert!(human.contains("Encrypted:   cleartext=locked"));
    }

    #[test]
    fn render_show_human_partition_table() {
        let detail = BlockDeviceDetail {
            device: "/dev/nvme0n1".into(),
            size: 4_000_787_030_016,
            fs_type: String::new(),
            fs_usage: String::new(),
            label: String::new(),
            uuid: String::new(),
            mountpoints: vec![],
            read_only: false,
            system: true,
            drive: None,
            partition: None,
            partition_table: Some(PartitionTableInfo {
                table_type: "gpt".into(),
            }),
            encrypted: None,
        };
        let human = render_show_human(&detail);
        assert!(human.contains("Part. table: gpt"));
    }
}
