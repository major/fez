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
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_descriptor_contract_stays_stable() {
        let descriptors = descriptors();
        let ids: Vec<&str> = descriptors.iter().map(|d| d.id).collect();
        assert_eq!(ids, ["system.show", "system.metrics"]);

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
