//! Capability descriptors for `firewall` commands.

use super::{input, input_choices, Descriptor};

/// Return descriptors for all `firewall.*` capabilities.
pub(super) fn descriptors() -> Vec<Descriptor> {
    vec![
        Descriptor {
            id: "firewall.status".into(),
            summary: "Show firewall status".into(),
            long: "Show firewalld state, the default zone, the panic-mode flag, and any \
        uncommitted runtime-vs-permanent drift (pending_changes). Read-only."
                .into(),
            privileged: false,
            output_kind: "FirewallStatus".into(),
            inputs: vec![],
            flags: vec!["--host".into(), "--json".into()],
            examples: vec!["fez firewall status --json".into()],
        },
        Descriptor {
            id: "firewall.list".into(),
            summary: "List firewall zones".into(),
            long: "List all firewalld zones with a per-zone summary (default flag, \
        services, ports, interfaces). Read-only."
                .into(),
            privileged: false,
            output_kind: "FirewallZoneList".into(),
            inputs: vec![],
            flags: vec!["--host".into(), "--json".into()],
            examples: vec!["fez firewall list --json".into()],
        },
        Descriptor {
            id: "firewall.show".into(),
            summary: "Show one zone's detail".into(),
            long: "Show one zone's full firewall detail: services, ports, interfaces, \
        and sources. Read-only. Exits 4 for an unknown zone."
                .into(),
            privileged: false,
            output_kind: "FirewallZone".into(),
            inputs: vec![input("zone", true)],
            flags: vec!["--host".into(), "--json".into()],
            examples: vec!["fez firewall show public --json".into()],
        },
        Descriptor {
            id: "firewall.services".into(),
            summary: "List the firewall service catalog".into(),
            long: "List the service names firewalld knows about (the valid arguments \
        to add-service). Read-only."
                .into(),
            privileged: false,
            output_kind: "FirewallServiceCatalog".into(),
            inputs: vec![],
            flags: vec!["--host".into(), "--json".into()],
            examples: vec!["fez firewall services --json".into()],
        },
        Descriptor {
            id: "firewall.add-service".into(),
            summary: "Add a service to a zone".into(),
            long: "Add a service to a zone at runtime only. Use --zone to target a zone \
        (the default zone otherwise) and --timeout to auto-revert after N seconds. The change \
        is NOT permanent until `fez firewall confirm`. Privileged. An unknown service is \
        rejected by firewalld (exit 7). Protected ops elsewhere need --force."
                .into(),
            privileged: true,
            output_kind: "FirewallChange".into(),
            inputs: vec![input("service", true)],
            flags: vec![
                "--host".into(),
                "--json".into(),
                "--zone".into(),
                "--timeout".into(),
                "--force".into(),
            ],
            examples: vec![
                "fez firewall add-service http --json".into(),
                "fez firewall add-service http --zone public --timeout 60".into(),
            ],
        },
        Descriptor {
            id: "firewall.remove-service".into(),
            summary: "Remove a service from a zone".into(),
            long: "Remove a service from a zone at runtime only. Removing the ssh \
        service (which carries the active session) is refused unless --force is supplied \
        (exit 8). NOT permanent until `fez firewall confirm`. Privileged."
                .into(),
            privileged: true,
            output_kind: "FirewallChange".into(),
            inputs: vec![input("service", true)],
            flags: vec![
                "--host".into(),
                "--json".into(),
                "--zone".into(),
                "--force".into(),
            ],
            examples: vec!["fez firewall remove-service http --json".into()],
        },
        Descriptor {
            id: "firewall.add-port".into(),
            summary: "Add a port to a zone".into(),
            long: "Add a port (port/proto, e.g. 8080/tcp) to a zone at runtime only. \
        Use --zone and --timeout. NOT permanent until `fez firewall confirm`. Privileged. \
        Protected ops elsewhere need --force."
                .into(),
            privileged: true,
            output_kind: "FirewallChange".into(),
            inputs: vec![input("port", true)],
            flags: vec![
                "--host".into(),
                "--json".into(),
                "--zone".into(),
                "--timeout".into(),
                "--force".into(),
            ],
            examples: vec!["fez firewall add-port 8080/tcp --json".into()],
        },
        Descriptor {
            id: "firewall.remove-port".into(),
            summary: "Remove a port from a zone".into(),
            long: "Remove a port (port/proto) from a zone at runtime only. Removing the \
        port that carries the active SSH session is refused unless --force is supplied \
        (exit 8). NOT permanent until `fez firewall confirm`. Privileged."
                .into(),
            privileged: true,
            output_kind: "FirewallChange".into(),
            inputs: vec![input("port", true)],
            flags: vec![
                "--host".into(),
                "--json".into(),
                "--zone".into(),
                "--force".into(),
            ],
            examples: vec!["fez firewall remove-port 8080/tcp --json".into()],
        },
        Descriptor {
            id: "firewall.set-default-zone".into(),
            summary: "Set the default zone".into(),
            long: "Set the default firewall zone. Every default-zone change is gated \
        and refused unless --force is supplied (exit 8), because a different default can \
        sever a connection that relied on the old zone. Runtime only until confirm. Privileged."
                .into(),
            privileged: true,
            output_kind: "FirewallChange".into(),
            inputs: vec![input("zone", true)],
            flags: vec!["--host".into(), "--json".into(), "--force".into()],
            examples: vec!["fez firewall set-default-zone internal --force --json".into()],
        },
        Descriptor {
            id: "firewall.reload".into(),
            summary: "Reload permanent config into runtime".into(),
            long: "Reload the permanent config into runtime, discarding any uncommitted \
        runtime changes. With uncommitted drift present the reload is refused unless --force \
        is supplied (exit 8), since it would lose that work. With no drift it runs freely. \
        Privileged."
                .into(),
            privileged: true,
            output_kind: "FirewallChange".into(),
            inputs: vec![],
            flags: vec!["--host".into(), "--json".into(), "--force".into()],
            examples: vec!["fez firewall reload --json".into()],
        },
        Descriptor {
            id: "firewall.confirm".into(),
            summary: "Persist runtime config to permanent".into(),
            long: "Commit the current runtime firewall config to permanent \
        (runtimeToPermanent). This is the only persistence path; mutations are runtime-only \
        until confirmed. Privileged. --force is accepted for global consistency but is optional for confirm itself."
                .into(),
            privileged: true,
            output_kind: "FirewallConfirm".into(),
            inputs: vec![],
            flags: vec!["--host".into(), "--json".into(), "--force".into()],
            examples: vec!["fez firewall confirm --json".into()],
        },
        Descriptor {
            id: "firewall.panic".into(),
            summary: "Toggle panic mode".into(),
            long: "Toggle panic mode. `panic on` drops ALL traffic and is refused unless \
        --force is supplied (exit 8); `panic off` re-enables traffic. Runtime only. Privileged."
                .into(),
            privileged: true,
            output_kind: "FirewallChange".into(),
            inputs: vec![input_choices("state", true, &["on", "off"])],
            flags: vec!["--host".into(), "--json".into(), "--force".into()],
            examples: vec![
                "fez firewall panic off --json".into(),
                "fez firewall panic on --force".into(),
            ],
        },
        Descriptor {
            id: "firewall.masquerade".into(),
            summary: "Enable or disable masquerade (SNAT) for a zone".into(),
            long: "Enable or disable masquerade (source NAT for forwarded traffic) on a \
        zone. Use --zone to target a zone (the default zone otherwise) and --timeout to \
        auto-revert after N seconds (ignored for `off`). Runtime only; NOT permanent until \
        `fez firewall confirm`. Enabling is unguarded; disabling is refused unless --force is \
        supplied (exit 8), because dropping SNAT can sever a gateway's forwarded clients. \
        Privileged."
                .into(),
            privileged: true,
            output_kind: "FirewallChange".into(),
            inputs: vec![input_choices("state", true, &["on", "off"])],
            flags: vec![
                "--host".into(),
                "--json".into(),
                "--zone".into(),
                "--timeout".into(),
                "--force".into(),
            ],
            examples: vec![
                "fez firewall masquerade on --json".into(),
                "fez firewall masquerade off --zone public --force".into(),
            ],
        },
    ]
}
