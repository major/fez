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

impl Descriptor {
    /// Render the descriptor as a complete plain-text block for `fez describe`
    /// (no `--json`).
    ///
    /// Top-level help promises `describe` prints inputs, output kind, flags,
    /// and privileged status; this carries the same essential metadata the
    /// JSON form does so an agent reading text output can act safely without
    /// switching to JSON (issue #62).
    #[must_use]
    pub fn render_text(&self) -> String {
        let mut s = format!("{}: {}\n\n{}\n\n", self.id, self.summary, self.long);
        s.push_str(&format!("privileged: {}\n", self.privileged));
        s.push_str(&format!("output: {}\n", self.output_kind));

        if !self.inputs.is_empty() {
            s.push_str("\ninputs:\n");
            for i in &self.inputs {
                let req = if i.required { "required" } else { "optional" };
                s.push_str(&format!("  {}: {} {}", i.name, i.ty, req));
                if let Some(default) = &i.default {
                    s.push_str(&format!(" (default: {default})"));
                }
                s.push('\n');
            }
        }

        if !self.flags.is_empty() {
            s.push_str("\nflags:\n");
            for f in &self.flags {
                s.push_str(&format!("  {f}\n"));
            }
        }

        s.push_str("\nexamples:\n");
        for ex in &self.examples {
            s.push_str(&format!("  {ex}\n"));
        }
        s
    }
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
        // Include the required <UNIT>: agents copy examples verbatim, and an
        // example without it fails with "required arguments were not provided"
        // (issue #53).
        examples: vec![format!("fez {} sshd.service --json", id.replace('.', " "))],
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
available packages. --repo restricts to packages whose repo id exactly matches the given \
value; it is repeatable and unions (a package is kept if its repo id equals any given \
value). The applied filter is echoed back in the `repos` field. Read-only."
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
until confirmed. Privileged. --force is not required for confirm itself."
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
            inputs: vec![input("state", true)],
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
            inputs: vec![input("state", true)],
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

    #[test]
    fn render_text_includes_all_metadata() {
        let d = find("services.start").unwrap();
        let text = d.render_text();
        assert!(text.contains("services.start: Start a unit"));
        assert!(text.contains("privileged: true"));
        assert!(text.contains("output: ServiceMutation"));
        assert!(text.contains("inputs:"));
        assert!(text.contains("unit: string required"));
        assert!(text.contains("flags:"));
        assert!(text.contains("--force"));
        assert!(text.contains("examples:"));
        assert!(text.contains("fez services start sshd.service --json"));
    }

    #[test]
    fn render_text_marks_readonly_not_privileged() {
        let d = find("services.list").unwrap();
        let text = d.render_text();
        assert!(text.contains("privileged: false"));
        assert!(text.contains("output: ServiceList"));
    }

    #[test]
    fn render_text_optional_input_shows_default() {
        // Find any descriptor with an optional input carrying a default, and
        // confirm the rendered line annotates it. If none exists this is a
        // no-op (the format is still covered by the required-input case).
        for d in registry() {
            for i in &d.inputs {
                if let Some(default) = &i.default {
                    let text = d.render_text();
                    assert!(
                        text.contains(&format!("(default: {default})")),
                        "{}: optional input {} default not rendered",
                        d.id,
                        i.name
                    );
                }
            }
        }
    }
}
