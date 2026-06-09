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
    /// One-line human summary.
    pub summary: String,
    /// Whether invoking the capability requires elevated privileges.
    pub privileged: bool,
    /// The envelope `kind` this capability emits.
    pub output_kind: String,
    /// Inputs the capability accepts.
    pub inputs: Vec<Input>,
    /// Flags the capability honors.
    pub flags: Vec<String>,
    /// An example invocation.
    pub example: String,
}

fn input(name: &str, required: bool) -> Input {
    Input {
        name: name.into(),
        ty: "string".into(),
        required,
        default: None,
    }
}

fn mutation(id: &str, summary: &str, output_kind: &str, extra_flags: &[&str]) -> Descriptor {
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
        privileged: true,
        output_kind: output_kind.into(),
        inputs: vec![input("unit", true)],
        flags,
        example: format!("fez {} --json", id.replace('.', " ")),
    }
}

/// The full set of capability descriptors fez supports.
pub fn registry() -> Vec<Descriptor> {
    vec![
        Descriptor {
            id: "services.list".into(),
            summary: "List systemd units".into(),
            privileged: false,
            output_kind: "ServiceList".into(),
            inputs: vec![input("state", false)],
            flags: vec!["--host".into(), "--json".into(), "--state".into()],
            example: "fez services list --state failed --json".into(),
        },
        Descriptor {
            id: "services.status".into(),
            summary: "Show one unit's status".into(),
            privileged: false,
            output_kind: "ServiceStatus".into(),
            inputs: vec![input("unit", true)],
            flags: vec!["--host".into(), "--json".into()],
            example: "fez services status sshd.service --json".into(),
        },
        Descriptor {
            id: "services.logs".into(),
            summary: "Read a unit's journal".into(),
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
            example: "fez services logs sshd.service --lines 100 --json".into(),
        },
        mutation("services.start", "Start a unit", "ServiceMutation", &[]),
        mutation("services.stop", "Stop a unit", "ServiceMutation", &[]),
        mutation("services.restart", "Restart a unit", "ServiceMutation", &[]),
        mutation(
            "services.reload",
            "Reload a unit's configuration",
            "ServiceMutation",
            &[],
        ),
        mutation(
            "services.enable",
            "Enable a unit",
            "ServiceEnablement",
            &["--now"],
        ),
        mutation(
            "services.disable",
            "Disable a unit",
            "ServiceEnablement",
            &["--now"],
        ),
    ]
}

/// Look up a capability descriptor by its dotted id.
pub fn find(id: &str) -> Option<Descriptor> {
    registry().into_iter().find(|d| d.id == id)
}
