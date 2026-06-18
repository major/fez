//! Machine-readable descriptions of every capability fez exposes, used to
//! advertise the command surface (ids, inputs, flags, examples) to agents.
use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};
use serde_json::{json, Value};

pub mod help;

mod firewall;
mod network;
mod packages;
mod schemas;
mod services;

/// A single named input a capability accepts.
#[derive(Serialize, Clone)]
pub struct Input {
    /// Input name as used on the command line.
    pub name: &'static str,
    /// Input value type (currently always `"string"`).
    #[serde(rename = "type")]
    pub ty: &'static str,
    /// Whether the input must be supplied.
    pub required: bool,
    /// Default value used when the input is omitted, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<&'static str>,
    /// Allowed values for constrained inputs, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub choices: Option<Vec<&'static str>>,
}

#[derive(Serialize, Clone)]
pub(crate) struct FlagSchema {
    pub(crate) name: &'static str,
    #[serde(rename = "type")]
    pub(crate) ty: &'static str,
    pub(crate) description: &'static str,
    pub(crate) repeatable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) default: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) choices: Option<Vec<&'static str>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) conflicts_with: Vec<&'static str>,
}

/// A complete description of one capability.
#[derive(Clone)]
pub struct Descriptor {
    /// Dotted capability id (e.g. `services.start`).
    pub id: &'static str,
    /// One-line human summary (maps to clap `about`).
    pub summary: &'static str,
    /// Full description (maps to clap `long_about`).
    pub long: &'static str,
    /// Whether invoking the capability requires elevated privileges.
    pub privileged: bool,
    /// The envelope `kind` this capability emits.
    pub output_kind: &'static str,
    /// Inputs the capability accepts.
    pub inputs: Vec<Input>,
    /// Flags the capability honors.
    pub flags: Vec<&'static str>,
    /// Example invocations (maps to clap `after_help`).
    pub examples: Vec<String>,
}

impl Serialize for Descriptor {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut s = serializer.serialize_struct("Descriptor", 11)?;
        s.serialize_field("id", &self.id)?;
        s.serialize_field("summary", &self.summary)?;
        s.serialize_field("long", &self.long)?;
        s.serialize_field("privileged", &self.privileged)?;
        s.serialize_field("output_kind", &self.output_kind)?;
        s.serialize_field("output", &self.output_schema())?;
        s.serialize_field("inputs", &self.inputs)?;
        s.serialize_field("flags", &self.flags)?;
        s.serialize_field("flag_schema", &self.flag_schema())?;
        s.serialize_field("examples", &self.examples)?;
        s.end()
    }
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
                if let Some(choices) = &i.choices {
                    s.push_str(&format!(" choices: {}", choices.join(", ")));
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

    pub(crate) fn flag_schema(&self) -> Vec<FlagSchema> {
        self.flags
            .iter()
            .map(|flag| flag_schema(self.id, flag))
            .collect()
    }

    fn output_schema(&self) -> Value {
        let mut output = json!({
            "kind": self.output_kind,
            "schema": schemas::output_schema(self.output_kind),
            "error": schemas::error_schema(),
            "error_envelope": schemas::error_envelope_schema(),
        });
        let has_dry_run = self.flags.contains(&"--dry-run");
        if let Some(alternates) = schemas::alternate_output_schemas(self.output_kind, has_dry_run) {
            output["alternates"] = alternates;
        }
        output
    }
}

fn input(name: &'static str, required: bool) -> Input {
    Input {
        name,
        ty: "string",
        required,
        default: None,
        choices: None,
    }
}

fn input_choices(name: &'static str, required: bool, choices: &[&'static str]) -> Input {
    Input {
        name,
        ty: "string",
        required,
        default: None,
        choices: Some(choices.to_vec()),
    }
}

/// Static flag-spec table. Each row: (flag, ty, description, repeatable, default, conflicts_with).
/// Adding a new flag requires only a new row here.
#[rustfmt::skip]
#[allow(clippy::type_complexity)] // ponytail: flat tuple table; struct when a second consumer appears
static FLAG_TABLE: &[(&str, &str, &str, bool, Option<&str>, &[&str])] = &[
    ("--host",      "string",  "Target host. Defaults to localhost.",                                        false, Some("localhost"), &[]),
    ("--json",      "boolean", "Emit a fez/v1 JSON envelope.",                                               false, None,             &[]),
    ("--dry-run",   "boolean", "Resolve and report the planned mutation without applying it.",                false, None,             &[]),
    ("--force",     "boolean", "Override command-specific safety guardrails.",                               false, None,             &[]),
    ("--state",     "string",  "Filter by state.",                                                           false, None,             &[]),
    ("--since",     "string",  "Only include log entries since this journalctl time expression.",             false, None,             &[]),
    ("--priority",  "string",  "Only include log entries at this priority or higher.",                       false, None,             &[]),
    ("--lines",     "integer", "Limit log output to the last N entries.",                                    false, None,             &[]),
    ("--follow",    "boolean", "Stream new log entries.",                                                    false, None,             &[]),
    ("--now",       "boolean", "Start or stop the unit immediately with the enablement change.",             false, None,             &[]),
    ("--installed", "boolean", "List installed packages.",                                                   false, Some("true"),     &["--available"]),
    ("--available", "boolean", "List available packages.",                                                   false, None,             &["--installed"]),
    ("--repo",      "string",  "Restrict packages to this exact repository id.",                             true,  None,             &[]),
    ("--enabled",   "boolean", "Show only enabled repositories.",                                            false, Some("true"),     &["--disabled", "--all"]),
    ("--disabled",  "boolean", "Show only disabled repositories.",                                           false, None,             &["--enabled", "--all"]),
    ("--all",       "boolean", "Include all entries instead of the default subset.",                         false, None,             &[]),
    ("--zone",      "string",  "Firewall zone to target. Defaults to the target host's default zone.",       false, None,             &[]),
    ("--timeout",   "integer", "Auto-revert the runtime firewall change after this many seconds.",           false, None,             &[]),
];

fn flag_schema(capability_id: &str, flag: &'static str) -> FlagSchema {
    // Per-capability override: packages.repolist --all has different semantics.
    let row = if flag == "--all" && capability_id == "packages.repolist" {
        (
            "--all",
            "boolean",
            "Show all repositories.",
            false,
            None,
            ["--enabled", "--disabled"].as_slice(),
        )
    } else {
        FLAG_TABLE
            .iter()
            .find(|(f, ..)| *f == flag)
            .map(|&(f, ty, desc, rep, def, cw)| (f, ty, desc, rep, def, cw))
            .unwrap_or((
                "",
                "string",
                "Capability-specific flag.",
                false,
                None,
                [].as_slice(),
            ))
    };
    let (_, ty, description, repeatable, default, conflicts_with) = row;
    FlagSchema {
        name: flag,
        ty,
        description,
        repeatable,
        default,
        choices: None,
        conflicts_with: conflicts_with.to_vec(),
    }
}

fn mutation(
    id: &'static str,
    summary: &'static str,
    long: &'static str,
    output_kind: &'static str,
    extra_flags: &[&'static str],
) -> Descriptor {
    let mut flags = vec!["--host", "--json", "--dry-run", "--force"];
    flags.extend_from_slice(extra_flags);
    Descriptor {
        id,
        summary,
        long,
        privileged: true,
        output_kind,
        inputs: vec![input("unit", true)],
        flags,
        // Include the required <UNIT>: agents copy examples verbatim, and an
        // example without it fails with "required arguments were not provided"
        // (issue #53).
        examples: vec![format!("fez {} sshd.service --json", id.replace('.', " "))],
    }
}

fn enablement(id: &'static str, summary: &'static str, long: &'static str) -> Descriptor {
    let verb = id.rsplit('.').next().expect("capability id has a verb");
    Descriptor {
        id,
        summary,
        long,
        privileged: true,
        output_kind: "ServiceEnablement",
        inputs: vec![input("unit", true)],
        flags: vec!["--host", "--json", "--dry-run", "--force", "--now"],
        examples: vec![
            format!("fez services {verb} chronyd.service --json"),
            format!("fez services {verb} chronyd.service --now"),
        ],
    }
}

/// The full set of capability descriptors fez supports.
pub fn registry() -> Vec<Descriptor> {
    let mut descriptors = Vec::new();
    descriptors.extend(services::descriptors());
    descriptors.extend(packages::descriptors());
    descriptors.extend(network::descriptors());
    descriptors.extend(firewall::descriptors());
    descriptors
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
    fn every_descriptor_has_output_schema() {
        for d in registry() {
            let output = d.output_schema();
            assert_eq!(output["kind"], d.output_kind, "{} kind mismatch", d.id);
            assert_eq!(
                output["schema"]["type"], "object",
                "{} missing schema",
                d.id
            );
            assert_eq!(
                output["error"]["type"], "object",
                "{} missing error schema",
                d.id
            );
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
    fn render_text_optional_input_shows_default_and_choices() {
        // Synthesize a descriptor with an optional input that carries both a
        // default and a choices list so the branches in render_text are
        // exercised.
        let d = Descriptor {
            id: "test.synthetic",
            summary: "synthetic",
            long: "",
            privileged: false,
            output_kind: "ServiceList",
            inputs: vec![Input {
                name: "scope",
                ty: "string",
                required: false,
                default: Some("installed"),
                choices: Some(vec!["installed", "available"]),
            }],
            flags: vec![],
            examples: vec!["fez test.synthetic".into()],
        };
        let text = d.render_text();
        assert!(
            text.contains("(default: installed)"),
            "default not rendered: {text}"
        );
        assert!(
            text.contains("choices: installed, available"),
            "choices not rendered: {text}"
        );
    }
}
