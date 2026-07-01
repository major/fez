//! Capability descriptors for `journal` commands.

use super::Descriptor;

/// Return descriptors for all `journal.*` capabilities.
pub(super) fn descriptors() -> Vec<Descriptor> {
    vec![Descriptor {
        id: "journal",
        summary: "Query the systemd journal",
        long: "Query systemd journal entries on the target host. Mirrors journalctl \
            flags for familiar filtering. Returns the last 25 entries by default to \
            keep context windows manageable. Use --lines to adjust. Use --list-boots \
            to discover available boot IDs. Use --list-fields to discover available \
            journal field names for --output-fields. Read-only: no privilege escalation.",
        privileged: false,
        output_kind: "JournalEntries",
        inputs: vec![],
        flags: vec![
            "--host",
            "--json",
            "--unit",
            "--since",
            "--until",
            "--priority",
            "--lines",
            "--boot",
            "--grep",
            "--list-boots",
            "--list-fields",
            "--output-fields",
        ],
        examples: vec![
            "fez journal --json".into(),
            "fez journal --unit sshd.service --lines 50".into(),
            "fez journal --priority err --since '1 hour ago'".into(),
            "fez journal --boot --grep 'Failed password'".into(),
            "fez journal --list-boots".into(),
            "fez journal --list-fields".into(),
            "fez journal --unit sshd --output-fields _COMM,_EXE".into(),
        ],
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_descriptor_contract() {
        let descs = descriptors();
        assert_eq!(descs.len(), 1);
        let d = &descs[0];
        assert_eq!(d.id, "journal");
        assert!(!d.privileged);
        assert_eq!(d.output_kind, "JournalEntries");
        assert!(d.flags.contains(&"--unit"));
        assert!(d.flags.contains(&"--list-boots"));
        assert!(d.flags.contains(&"--list-fields"));
        assert!(d.flags.contains(&"--output-fields"));
        assert!(d.examples.len() >= 5);
    }
}
