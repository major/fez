//! Capability descriptors for `services` commands.

use super::{enablement, input, mutation, Descriptor};

/// Return descriptors for all `services.*` capabilities.
pub(super) fn descriptors() -> Vec<Descriptor> {
    vec![
        Descriptor {
            id: "services.list".into(),
            summary: "List systemd units".into(),
            long: "List systemd units on the target host. Use --state to filter by \
        active state (e.g. active, failed, inactive). Read-only; never mutates."
                .into(),
            privileged: false,
            output_kind: "ServiceList".into(),
            inputs: vec![input("state", false)],
            flags: vec!["--host".into(), "--json".into(), "--state".into()],
            examples: vec![
                "fez services list --state failed --json".into(),
                "fez --host web01 services list".into(),
            ],
        },
        Descriptor {
            id: "services.status".into(),
            summary: "Show one unit's status".into(),
            long: "Show the current status of a single systemd unit (active state, \
        sub-state, enablement). Read-only."
                .into(),
            privileged: false,
            output_kind: "ServiceStatus".into(),
            inputs: vec![input("unit", true)],
            flags: vec!["--host".into(), "--json".into()],
            examples: vec!["fez services status sshd.service --json".into()],
        },
        Descriptor {
            id: "services.logs".into(),
            summary: "Read a unit's journal".into(),
            long: "Read journal entries for a unit. Filter with --since and --priority \
        (journalctl syntax), cap with --lines, or stream with --follow. Read-only."
                .into(),
            privileged: false,
            output_kind: "LogEntries".into(),
            inputs: vec![input("unit", true)],
            flags: vec![
                "--host".into(),
                "--json".into(),
                "--since".into(),
                "--priority".into(),
                "--lines".into(),
                "--follow".into(),
            ],
            examples: vec![
                "fez services logs sshd.service --lines 100 --json".into(),
                "fez services logs nginx.service --since '1 hour ago' --priority err".into(),
            ],
        },
        mutation(
            "services.start",
            "Start a unit",
            "Start a systemd unit immediately. Privileged. Protected units are \
        refused unless --force is supplied. Exits 8 on a protected-unit refusal.",
            "ServiceMutation",
            &[],
        ),
        mutation(
            "services.stop",
            "Stop a unit",
            "Stop a running systemd unit. Privileged. Protected units are refused \
        unless --force is supplied (exit 8).",
            "ServiceMutation",
            &[],
        ),
        mutation(
            "services.restart",
            "Restart a unit",
            "Restart a systemd unit. Privileged. Protected units are refused unless \
        --force is supplied (exit 8).",
            "ServiceMutation",
            &[],
        ),
        mutation(
            "services.reload",
            "Reload a unit's configuration",
            "Ask a unit to reload its configuration without a full restart. \
        Privileged. Protected units are refused unless --force is supplied (exit 8).",
            "ServiceMutation",
            &[],
        ),
        enablement(
            "services.enable",
            "Enable a unit",
            "Enable a unit so it starts at boot. Add --now to also start it \
        immediately. Privileged. Protected units are refused unless --force is supplied (exit 8).",
        ),
        enablement(
            "services.disable",
            "Disable a unit",
            "Disable a unit so it no longer starts at boot. Add --now to also \
        stop it immediately. Privileged. Protected units are refused unless --force is supplied (exit 8).",
        ),
    ]
}
