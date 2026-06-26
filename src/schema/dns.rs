//! Capability descriptors for `dns` commands.

use super::{input, Descriptor};

/// Return descriptors for all `dns.*` capabilities.
pub(super) fn descriptors() -> Vec<Descriptor> {
    vec![
        Descriptor {
            id: "dns.status",
            summary: "Show DNS resolver configuration and cache statistics",
            long: "Show the systemd-resolved configuration: DNS servers (global and per-link), \
        DNSSEC mode, DNS-over-TLS, LLMNR, multicast DNS, resolv.conf mode, and cache hit/miss \
        statistics. By default only links with DNS servers configured are shown; use --all to \
        include every link. Read-only: no privilege escalation. Requires systemd-resolved.",
            privileged: false,
            output_kind: "DnsStatus",
            inputs: vec![],
            flags: vec!["--host", "--json", "--all"],
            examples: vec![
                "fez dns status --json".into(),
                "fez dns status --all".into(),
                "fez --host web1 dns status --json".into(),
            ],
        },
        Descriptor {
            id: "dns.flush",
            summary: "Flush the DNS resolver cache",
            long: "Clear the systemd-resolved DNS cache. Unprivileged: the default polkit policy \
        allows FlushCaches without authentication. Audited as a mutation (destroys cache state).",
            privileged: false,
            output_kind: "DnsFlush",
            inputs: vec![],
            flags: vec!["--host", "--json"],
            examples: vec![
                "fez dns flush --json".into(),
                "fez --host db1 dns flush".into(),
            ],
        },
        Descriptor {
            id: "dns.query",
            summary: "Resolve a hostname to addresses",
            long: "Resolve a hostname to IPv4 and IPv6 addresses via systemd-resolved's \
        ResolveHostname method. Returns the canonical name and all resolved addresses. \
        Returns exit 4 (not-found) for NXDOMAIN. Read-only: no privilege escalation.",
            privileged: false,
            output_kind: "DnsQuery",
            inputs: vec![input("hostname", true)],
            flags: vec!["--host", "--json"],
            examples: vec![
                "fez dns query example.com --json".into(),
                "fez --host web1 dns query internal.corp --json".into(),
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_descriptor_contract_stays_stable() {
        let descriptors = descriptors();
        let ids: Vec<&str> = descriptors.iter().map(|d| d.id).collect();
        assert_eq!(ids, ["dns.status", "dns.flush", "dns.query"]);

        let status = &descriptors[0];
        assert_eq!(status.flags, ["--host", "--json", "--all"]);
        assert!(status.inputs.is_empty());
        assert!(!status.privileged);
        assert_eq!(status.output_kind, "DnsStatus");

        let flush = &descriptors[1];
        assert!(flush.inputs.is_empty());
        assert!(!flush.privileged);
        assert_eq!(flush.output_kind, "DnsFlush");

        let query = &descriptors[2];
        assert_eq!(query.inputs.len(), 1);
        assert_eq!(query.inputs[0].name, "hostname");
        assert!(query.inputs[0].required);
        assert_eq!(query.output_kind, "DnsQuery");
    }
}
