//! Capability descriptors for `network` commands.

use super::{input, Descriptor};

/// Return descriptors for all `network.*` capabilities.
pub(super) fn descriptors() -> Vec<Descriptor> {
    vec![
        Descriptor {
            id: "network.list".into(),
            summary: "List network devices".into(),
            long: "List NetworkManager devices with their type, state, primary IPv4/IPv6 \
        address, and MAC. By default unmanaged virtual interfaces (container veth, etc.) are \
        hidden; use --all to show every device. Read-only."
                .into(),
            privileged: false,
            output_kind: "NetworkDeviceList".into(),
            inputs: vec![],
            flags: vec!["--host".into(), "--json".into(), "--all".into()],
            examples: vec![
                "fez network list --json".into(),
                "fez network list --all".into(),
            ],
        },
        Descriptor {
            id: "network.show".into(),
            summary: "Show one device's network detail".into(),
            long: "Show the full network detail for one device: addresses (IPv4 and IPv6), \
        gateway, DNS servers, search domains, routes, MAC, MTU, the active connection profile, \
        and DHCP lease. Read-only."
                .into(),
            privileged: false,
            output_kind: "NetworkDeviceDetail".into(),
            inputs: vec![input("device", true)],
            flags: vec!["--host".into(), "--json".into()],
            examples: vec!["fez network show enp1s0 --json".into()],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_descriptor_contract_stays_stable() {
        let descriptors = descriptors();
        let ids: Vec<&str> = descriptors.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, ["network.list", "network.show"]);

        let list = &descriptors[0];
        assert_eq!(list.flags, ["--host", "--json", "--all"]);
        assert!(list.inputs.is_empty());

        let show = &descriptors[1];
        assert_eq!(show.flags, ["--host", "--json"]);
        assert_eq!(show.inputs.len(), 1);
        assert_eq!(show.inputs[0].name, "device");
        assert!(show.inputs[0].required);
    }
}
