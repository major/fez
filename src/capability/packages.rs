//! Capability descriptors for `packages` commands.

use super::{input, Descriptor};

/// Return descriptors for all `packages.*` capabilities.
pub(super) fn descriptors() -> Vec<Descriptor> {
    vec![
        Descriptor {
            id: "packages.list".into(),
            summary: "List packages".into(),
            long: "List installed (default) or available packages. Use --available to list \
        available packages. Use --name to keep packages whose name contains the given substring, \
        and --repo to restrict packages by exact repo id (repeatable union). Use --limit and \
        --offset to page large results; JSON output echoes filters plus total, returned, limit, \
        offset, and next_offset metadata. Read-only."
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
                "--name".into(),
                "--limit".into(),
                "--offset".into(),
            ],
            examples: vec![
                "fez packages list --json".into(),
                "fez packages list --available --name nginx --limit 20".into(),
                "fez packages list --available --repo fedora --offset 20 --limit 20".into(),
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
        the plan; --dry-run stops after the plan. Privileged. Uses dnf5daemon, falling \
        back to PackageKit when dnf5daemon is absent (sizes are unavailable on the \
        PackageKit backend; the envelope marks backend and carries a hint). Exits 9 only \
        if both backends are missing, 10 if the resolved transaction is refused by \
        removal guardrails (use --force to override)."
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
    ]
}
