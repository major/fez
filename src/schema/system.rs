//! Capability descriptors for `system` commands.

use super::Descriptor;

/// Return descriptors for all `system.*` capabilities.
pub(super) fn descriptors() -> Vec<Descriptor> {
    vec![Descriptor {
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
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_descriptor_contract_stays_stable() {
        let descriptors = descriptors();
        let ids: Vec<&str> = descriptors.iter().map(|d| d.id).collect();
        assert_eq!(ids, ["system.show"]);

        let show = &descriptors[0];
        assert_eq!(show.flags, ["--host", "--json"]);
        assert!(show.inputs.is_empty());
        assert!(!show.privileged);
        assert_eq!(show.output_kind, "SystemOverview");
    }
}
