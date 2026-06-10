//! Machine-readable descriptions of every capability fez exposes, used to
//! advertise the command surface (ids, inputs, flags, examples) to agents.
use serde::Serialize;

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

/// The full set of capability descriptors fez supports.
pub fn registry() -> Vec<Descriptor> {
    vec![
        Descriptor {
            id: "services.list".into(),
            summary: "List systemd units".into(),
            long: "List systemd units, optionally filtered by active state.".into(),
            privileged: false,
            output_kind: "ServiceList".into(),
            inputs: vec![input("state", false)],
            flags: vec!["--host".into(), "--json".into(), "--state".into()],
            examples: vec!["fez services list --state failed --json".into()],
        },
        Descriptor {
            id: "services.status".into(),
            summary: "Show one unit's status".into(),
            long: "Show the current status of a single systemd unit.".into(),
            privileged: false,
            output_kind: "ServiceStatus".into(),
            inputs: vec![input("unit", true)],
            flags: vec!["--host".into(), "--json".into()],
            examples: vec!["fez services status sshd.service --json".into()],
        },
        Descriptor {
            id: "services.logs".into(),
            summary: "Read a unit's journal".into(),
            long: "Read journal entries for a single systemd unit.".into(),
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
            examples: vec!["fez services logs sshd.service --lines 100 --json".into()],
        },
        mutation(
            "services.start",
            "Start a unit",
            "Start a systemd unit on the target host.",
            "ServiceMutation",
            &[],
        ),
        mutation(
            "services.stop",
            "Stop a unit",
            "Stop a systemd unit on the target host.",
            "ServiceMutation",
            &[],
        ),
        mutation(
            "services.restart",
            "Restart a unit",
            "Restart a systemd unit on the target host.",
            "ServiceMutation",
            &[],
        ),
        mutation(
            "services.reload",
            "Reload a unit's configuration",
            "Reload a systemd unit's configuration without restarting it.",
            "ServiceMutation",
            &[],
        ),
        mutation(
            "services.enable",
            "Enable a unit",
            "Enable a systemd unit so it starts on boot.",
            "ServiceEnablement",
            &["--now"],
        ),
        mutation(
            "services.disable",
            "Disable a unit",
            "Disable a systemd unit so it no longer starts on boot.",
            "ServiceEnablement",
            &["--now"],
        ),
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
}
