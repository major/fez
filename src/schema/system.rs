//! Capability descriptors for `system` commands.

use super::Descriptor;

/// Return descriptors for all `system.*` capabilities.
pub(super) fn descriptors() -> Vec<Descriptor> {
    vec![
        Descriptor {
            id: "system.show",
            summary: "Show the system overview",
            long: "Run this first. Gather the system overview in a single call: \
        hostname, machine ID, boot ID, OS identity and version, kernel, hardware \
        vendor and model, firmware, timezone, and NTP synchronization status. \
        Read-only: no privilege escalation. Data comes from systemd's hostname1 \
        and timedate1 D-Bus interfaces, present on every Fedora and RHEL system \
        with no extra packages.",
            privileged: false,
            output_kind: "SystemOverview",
            inputs: vec![],
            flags: vec!["--host", "--json"],
            examples: vec![
                "fez system show --json".into(),
                "fez --host web1 system show --json".into(),
            ],
        },
        Descriptor {
            id: "system.metrics",
            summary: "Show a live performance snapshot",
            long: "Collect a one-shot PCP performance snapshot: CPU usage, memory \
        utilisation, load averages (1/5/15 min), aggregate disk IOPS, and \
        per-interface network throughput. Requires two one-second samples for \
        rate-derived counters. Read-only: no privilege escalation.",
            privileged: false,
            output_kind: "SystemMetrics",
            inputs: vec![],
            flags: vec!["--host", "--json"],
            examples: vec![
                "fez system metrics --json".into(),
                "fez --host db1 system metrics --json".into(),
            ],
        },
        Descriptor {
            id: "system.sessions",
            summary: "List active login sessions",
            long: "List all active login sessions from systemd-logind. Shows session \
        ID, username, session type (tty/x11/wayland), remote host if applicable, \
        and state. Read-only: no privilege escalation.",
            privileged: false,
            output_kind: "SessionList",
            inputs: vec![],
            flags: vec!["--host", "--json"],
            examples: vec![
                "fez system sessions --json".into(),
                "fez --host web1 system sessions".into(),
            ],
        },
        Descriptor {
            id: "system.users",
            summary: "List logged-in users",
            long: "List users with active login sessions from systemd-logind. Shows \
        UID and username. Read-only: no privilege escalation.",
            privileged: false,
            output_kind: "UserList",
            inputs: vec![],
            flags: vec!["--host", "--json"],
            examples: vec!["fez system users --json".into()],
        },
        Descriptor {
            id: "system.inhibitors",
            summary: "List shutdown/sleep inhibitors",
            long: "List active inhibitor locks from systemd-logind. Shows what is \
        inhibited (shutdown, sleep, etc.), which application holds the lock, \
        why, and the lock mode (block or delay). Read-only.",
            privileged: false,
            output_kind: "InhibitorList",
            inputs: vec![],
            flags: vec!["--host", "--json"],
            examples: vec!["fez system inhibitors --json".into()],
        },
        Descriptor {
            id: "system.boot-entries",
            summary: "List boot loader entries",
            long: "List BLS (Boot Loader Specification) entries from systemd-logind's \
        BootLoaderEntries property. Shows the filename of each entry. \
        Read-only: no privilege escalation.",
            privileged: false,
            output_kind: "BootEntryList",
            inputs: vec![],
            flags: vec!["--host", "--json"],
            examples: vec!["fez system boot-entries --json".into()],
        },
        Descriptor {
            id: "system.reboot",
            summary: "Reboot the host",
            long: "Reboot the host via systemd-logind. Requires --force to confirm \
        and privilege escalation through cockpit. Checks CanReboot first; \
        returns exit 9 if the host does not support reboot via logind. \
        Audited as system-reboot.",
            privileged: true,
            output_kind: "PowerAction",
            inputs: vec![],
            flags: vec!["--host", "--json", "--force"],
            examples: vec![
                "fez system reboot --force".into(),
                "fez --host web1 system reboot --force --json".into(),
            ],
        },
        Descriptor {
            id: "system.poweroff",
            summary: "Power off the host",
            long: "Power off the host via systemd-logind. Requires --force to confirm \
        and privilege escalation through cockpit. Checks CanPowerOff first. \
        Audited as system-poweroff.",
            privileged: true,
            output_kind: "PowerAction",
            inputs: vec![],
            flags: vec!["--host", "--json", "--force"],
            examples: vec!["fez system poweroff --force".into()],
        },
        Descriptor {
            id: "system.suspend",
            summary: "Suspend the host",
            long: "Suspend the host via systemd-logind. Requires --force to confirm \
        and privilege escalation through cockpit. Checks CanSuspend first; \
        returns exit 9 if suspend is not available. Audited as system-suspend.",
            privileged: true,
            output_kind: "PowerAction",
            inputs: vec![],
            flags: vec!["--host", "--json", "--force"],
            examples: vec!["fez system suspend --force".into()],
        },
        Descriptor {
            id: "system.subscription",
            summary: "Show RHEL subscription status",
            long: "Show the RHEL subscription status from the subscription-manager \
        D-Bus interface (com.redhat.RHSM1). Displays consumer UUID, \
        entitlement status, installed products, and system purpose. \
        RHEL only: returns exit 9 on Fedora or when subscription-manager \
        is not installed.",
            privileged: false,
            output_kind: "SubscriptionStatus",
            inputs: vec![],
            flags: vec!["--host", "--json"],
            examples: vec![
                "fez system subscription --json".into(),
                "fez --host rhel1 system subscription".into(),
            ],
        },
        Descriptor {
            id: "system.firmware.list",
            summary: "List firmware devices",
            long: "List all firmware-updatable devices from fwupd. Shows device name, \
        vendor, current version, and whether the device is updatable. \
        Read-only. Returns exit 9 when fwupd is not installed.",
            privileged: false,
            output_kind: "FirmwareDeviceList",
            inputs: vec![],
            flags: vec!["--host", "--json"],
            examples: vec!["fez system firmware list --json".into()],
        },
        Descriptor {
            id: "system.firmware.security",
            summary: "Show host firmware security posture (HSI)",
            long: "Show the Host Security ID (HSI) score and individual security \
        attributes from fwupd. HSI levels range from 0 (insecure) to 4 \
        (hardened). Each attribute shows its result and required HSI level. \
        Read-only.",
            privileged: false,
            output_kind: "FirmwareSecurityReport",
            inputs: vec![],
            flags: vec!["--host", "--json"],
            examples: vec!["fez system firmware security --json".into()],
        },
        Descriptor {
            id: "system.firmware.upgrades",
            summary: "List available firmware upgrades",
            long: "List available firmware upgrades for all updatable devices from \
        fwupd. Shows the device name, current version, available version, \
        and upgrade description. Returns an empty list if no upgrades are \
        available. Read-only.",
            privileged: false,
            output_kind: "FirmwareUpgradeList",
            inputs: vec![],
            flags: vec!["--host", "--json"],
            examples: vec!["fez system firmware upgrades --json".into()],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_descriptor_contract_stays_stable() {
        let descriptors = descriptors();
        let ids: Vec<&str> = descriptors.iter().map(|d| d.id).collect();
        assert_eq!(
            ids,
            [
                "system.show",
                "system.metrics",
                "system.sessions",
                "system.users",
                "system.inhibitors",
                "system.boot-entries",
                "system.reboot",
                "system.poweroff",
                "system.suspend",
                "system.subscription",
                "system.firmware.list",
                "system.firmware.security",
                "system.firmware.upgrades",
            ]
        );

        let show = &descriptors[0];
        assert_eq!(show.flags, ["--host", "--json"]);
        assert!(show.inputs.is_empty());
        assert!(!show.privileged);
        assert_eq!(show.output_kind, "SystemOverview");

        let metrics = &descriptors[1];
        assert_eq!(metrics.flags, ["--host", "--json"]);
        assert!(metrics.inputs.is_empty());
        assert!(!metrics.privileged);
        assert_eq!(metrics.output_kind, "SystemMetrics");
    }
}
