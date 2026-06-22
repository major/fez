//! Capability descriptors for `storage` commands.

use super::{input, Descriptor};

/// Return descriptors for all `storage.*` capabilities.
pub(super) fn descriptors() -> Vec<Descriptor> {
    vec![
        Descriptor {
            id: "storage.list",
            summary: "List block devices",
            long: "List UDisks2 block devices with filesystem type, label, UUID, size, \
        and mount point. Read-only: no privilege escalation. Requires udisks2 on the target.",
            privileged: false,
            output_kind: "StorageDeviceList",
            inputs: vec![],
            flags: vec!["--host", "--json"],
            examples: vec![
                "fez storage list --json".into(),
                "fez --host web1 storage list".into(),
            ],
        },
        Descriptor {
            id: "storage.show",
            summary: "Show one block device's full detail",
            long: "Show the full detail for one block device: size, filesystem type and \
        usage, label, UUID, mount points, partition info, partition table type, \
        drive model/serial, and LUKS encryption status. Read-only.",
            privileged: false,
            output_kind: "StorageDeviceDetail",
            inputs: vec![input("device", true)],
            flags: vec!["--host", "--json"],
            examples: vec![
                "fez storage show /dev/nvme0n1p1 --json".into(),
                "fez storage show sda1 --json".into(),
            ],
        },
        Descriptor {
            id: "storage.health",
            summary: "Show NVMe/SMART drive health",
            long: "Show drive health data from the NVMe controller interface: temperature, \
        power-on hours, critical warnings, self-test status. Only drives with \
        NVMe controller data are included. Optionally filter by drive model, \
        serial, or path substring. Read-only.",
            privileged: false,
            output_kind: "StorageHealth",
            inputs: vec![],
            flags: vec!["--host", "--json", "--drive"],
            examples: vec![
                "fez storage health --json".into(),
                "fez storage health --drive Samsung --json".into(),
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_descriptor_contract_stays_stable() {
        let descriptors = descriptors();
        let ids: Vec<&str> = descriptors.iter().map(|d| d.id).collect();
        assert_eq!(ids, ["storage.list", "storage.show", "storage.health"]);

        let list = &descriptors[0];
        assert!(!list.privileged);
        assert!(list.inputs.is_empty());

        let show = &descriptors[1];
        assert_eq!(show.inputs.len(), 1);
        assert_eq!(show.inputs[0].name, "device");
        assert!(show.inputs[0].required);

        let health = &descriptors[2];
        assert!(!health.privileged);
        assert!(health.inputs.is_empty());
    }
}
