//! Machine-readable descriptions of every capability fez exposes, used to
//! advertise the command surface (ids, inputs, flags, examples) to agents.
use serde::Serialize;

pub mod help;

/// A single named input a capability accepts.
#[derive(Serialize, Clone)]
pub struct Input {
    /// Input name as used on the command line.
    pub name: String,
    /// Input value type (currently always `"string"`).
    #[serde(rename = "type")]
    pub ty: String,
    /// Whether the input must be supplied.
    pub required: bool,
    /// Default value used when the input is omitted, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

/// A complete description of one capability.
#[derive(Serialize, Clone)]
pub struct Descriptor {
    /// Dotted capability id (e.g. `services.start`).
    pub id: String,
    /// One-line human summary (maps to clap `about`).
    pub summary: String,
    /// Full description (maps to clap `long_about`).
    pub long: String,
    /// Whether invoking the capability requires elevated privileges.
    pub privileged: bool,
    /// The envelope `kind` this capability emits.
    pub output_kind: String,
    /// Inputs the capability accepts.
    pub inputs: Vec<Input>,
    /// Flags the capability honors.
    pub flags: Vec<String>,
    /// Example invocations (maps to clap `after_help`).
    pub examples: Vec<String>,
}

fn input(name: &str, required: bool) -> Input {
    Input {
        name: name.into(),
        ty: "string".into(),
        required,
        default: None,
    }
}

fn mutation(
    id: &str,
    summary: &str,
    long: &str,
    output_kind: &str,
    extra_flags: &[&str],
) -> Descriptor {
    let mut flags = vec![
        "--host".to_string(),
        "--json".to_string(),
        "--dry-run".to_string(),
        "--force".to_string(),
    ];
    flags.extend(extra_flags.iter().map(|f| f.to_string()));
    Descriptor {
        id: id.into(),
        summary: summary.into(),
        long: long.into(),
        privileged: true,
        output_kind: output_kind.into(),
        inputs: vec![input("unit", true)],
        flags,
        examples: vec![format!("fez {} --json", id.replace('.', " "))],
    }
}

fn enablement(id: &str, summary: &str, long: &str) -> Descriptor {
    let verb = id.rsplit('.').next().expect("capability id has a verb");
    Descriptor {
        id: id.into(),
        summary: summary.into(),
        long: long.into(),
        privileged: true,
        output_kind: "ServiceEnablement".into(),
        inputs: vec![input("unit", true)],
        flags: vec![
            "--host".into(),
            "--json".into(),
            "--dry-run".into(),
            "--force".into(),
            "--now".into(),
        ],
        examples: vec![
            format!("fez services {verb} chronyd.service --json"),
            format!("fez services {verb} chronyd.service --now"),
        ],
    }
}

/// The full set of capability descriptors fez supports.
pub fn registry() -> Vec<Descriptor> {
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
        Descriptor {
            id: "packages.list".into(),
            summary: "List packages".into(),
            long: "List installed (default) or available packages. Use --available to list \
available packages, --repo to restrict by repository. Read-only."
                .into(),
            privileged: false,
            output_kind: "PackageList".into(),
            inputs: vec![],
            flags: vec![
                "--host".into(),
                "--json".into(),
                "--installed".into(),
                "--available".into(),
                "--repo".into(),
            ],
            examples: vec![
                "fez packages list --json".into(),
                "fez packages list --available --repo fedora".into(),
            ],
        },
        Descriptor {
            id: "packages.info".into(),
            summary: "Show one package's attributes".into(),
            long: "Show the full attributes of a single package (version, arch, repo, size, \
summary). Read-only."
                .into(),
            privileged: false,
            output_kind: "PackageInfo".into(),
            inputs: vec![input("spec", true)],
            flags: vec!["--host".into(), "--json".into()],
            examples: vec!["fez packages info bash --json".into()],
        },
        Descriptor {
            id: "packages.search".into(),
            summary: "Search packages".into(),
            long: "Search available packages by name, summary, or provides. Read-only.".into(),
            privileged: false,
            output_kind: "PackageSearch".into(),
            inputs: vec![input("pattern", true)],
            flags: vec!["--host".into(), "--json".into()],
            examples: vec!["fez packages search nginx --json".into()],
        },
        Descriptor {
            id: "packages.check-update".into(),
            summary: "List available upgrades".into(),
            long: "List packages with available upgrades. Read-only.".into(),
            privileged: false,
            output_kind: "PackageUpdates".into(),
            inputs: vec![],
            flags: vec!["--host".into(), "--json".into()],
            examples: vec!["fez packages check-update --json".into()],
        },
        Descriptor {
            id: "packages.repolist".into(),
            summary: "List repositories".into(),
            long: "List repositories and their enabled state. Use --enabled (default), \
--disabled, or --all. Read-only."
                .into(),
            privileged: false,
            output_kind: "RepoList".into(),
            inputs: vec![],
            flags: vec![
                "--host".into(),
                "--json".into(),
                "--enabled".into(),
                "--disabled".into(),
                "--all".into(),
            ],
            examples: vec!["fez packages repolist --all --json".into()],
        },
        Descriptor {
            id: "packages.install".into(),
            summary: "Install packages".into(),
            long: "Install one or more packages. Resolves the transaction first and surfaces \
the plan; --dry-run stops after the plan. Privileged. Exits 9 if dnf5daemon is \
missing, 10 if the resolved transaction is refused by removal guardrails (use \
--force to override)."
                .into(),
            privileged: true,
            output_kind: "PackageMutation".into(),
            inputs: vec![input("specs", true)],
            flags: vec![
                "--host".into(),
                "--json".into(),
                "--dry-run".into(),
                "--force".into(),
            ],
            examples: vec![
                "fez packages install htop --json".into(),
                "fez packages install nginx --dry-run".into(),
            ],
        },
        Descriptor {
            id: "packages.remove".into(),
            summary: "Remove packages".into(),
            long: "Remove one or more packages. Resolves first and applies removal guardrails: \
a protected package or a cascade larger than the limit is refused unless --force \
is supplied (exit 10). --dry-run surfaces the plan without removing. Privileged."
                .into(),
            privileged: true,
            output_kind: "PackageMutation".into(),
            inputs: vec![input("specs", true)],
            flags: vec![
                "--host".into(),
                "--json".into(),
                "--dry-run".into(),
                "--force".into(),
            ],
            examples: vec![
                "fez packages remove htop --json".into(),
                "fez packages remove oldpkg --dry-run".into(),
            ],
        },
        Descriptor {
            id: "packages.upgrade".into(),
            summary: "Upgrade packages".into(),
            long: "Upgrade named packages, or all packages when no spec is given. Resolves \
first and surfaces the plan; --dry-run stops after the plan. Privileged. Refusals \
from removal guardrails (replaced/obsoleted packages) exit 10 unless --force is \
supplied."
                .into(),
            privileged: true,
            output_kind: "PackageMutation".into(),
            inputs: vec![input("specs", false)],
            flags: vec![
                "--host".into(),
                "--json".into(),
                "--dry-run".into(),
                "--force".into(),
            ],
            examples: vec![
                "fez packages upgrade --json".into(),
                "fez packages upgrade nginx --force".into(),
            ],
        },
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

/// Look up a capability descriptor by its dotted id.
pub fn find(id: &str) -> Option<Descriptor> {
    registry().into_iter().find(|d| d.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_descriptor_has_long_and_examples() {
        for d in registry() {
            assert!(!d.long.trim().is_empty(), "{} missing long", d.id);
            assert!(!d.examples.is_empty(), "{} has no examples", d.id);
            for ex in &d.examples {
                assert!(ex.starts_with("fez "), "{}: bad example {:?}", d.id, ex);
            }
        }
    }

    #[test]
    fn protected_capabilities_document_force() {
        for d in registry() {
            if d.privileged {
                assert!(
                    d.long.contains("--force") || d.examples.iter().any(|e| e.contains("--force")),
                    "{}: privileged capability should mention --force",
                    d.id
                );
            }
        }
    }

    #[test]
    fn enable_disable_have_now_example() {
        for id in ["services.enable", "services.disable"] {
            let d = find(id).unwrap();
            assert!(
                d.examples.iter().any(|e| e.contains("--now")),
                "{id}: needs --now example"
            );
        }
    }
}
