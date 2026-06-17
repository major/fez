//! Machine-readable descriptions of every capability fez exposes, used to
//! advertise the command surface (ids, inputs, flags, examples) to agents.
use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};
use serde_json::{json, Value};

pub mod help;

mod firewall;
mod network;
mod packages;
mod services;

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
    /// Allowed values for constrained inputs, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub choices: Option<Vec<String>>,
}

#[derive(Serialize, Clone)]
pub(crate) struct FlagSchema {
    pub(crate) name: String,
    #[serde(rename = "type")]
    pub(crate) ty: String,
    pub(crate) description: String,
    pub(crate) repeatable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) default: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) choices: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) conflicts_with: Vec<String>,
}

/// A complete description of one capability.
#[derive(Clone)]
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
            .map(|flag| flag_schema(&self.id, flag))
            .collect()
    }

    fn output_schema(&self) -> Value {
        let mut output = json!({
            "kind": self.output_kind,
            "schema": output_schema(&self.output_kind),
            "error": error_schema(),
            "error_envelope": error_envelope_schema(),
        });
        if let Some(alternates) = alternate_output_schemas(self) {
            output["alternates"] = alternates;
        }
        output
    }
}

fn string_prop() -> Value {
    json!({"type": "string"})
}

fn integer_prop() -> Value {
    json!({"type": "integer"})
}

fn boolean_prop() -> Value {
    json!({"type": "boolean"})
}

fn array_prop() -> Value {
    json!({"type": "array"})
}

fn array_of(item: Value) -> Value {
    json!({"type": "array", "items": item})
}

fn nullable_integer_prop() -> Value {
    json!({"type": ["integer", "null"]})
}

fn nullable_boolean_prop() -> Value {
    json!({"type": ["boolean", "null"]})
}

fn nullable_string_prop() -> Value {
    json!({"type": ["string", "null"]})
}

fn nullable_object_prop() -> Value {
    json!({"type": ["object", "null"]})
}

fn object_schema(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
    })
}

fn type_prop(ty: &str) -> Value {
    json!({"type": ty})
}

fn table_schema(
    columns: &[(&str, &str)],
    extra_properties: Value,
    required_extra: &[&str],
) -> Value {
    let column_names: Vec<&str> = columns.iter().map(|(name, _)| *name).collect();
    let row_items: Vec<Value> = columns.iter().map(|(_, ty)| type_prop(ty)).collect();
    let mut properties = json!({
        "columns": {"type": "array", "items": {"type": "string"}, "const": column_names},
        "rows": {"type": "array", "items": {"type": "array", "prefixItems": row_items}},
        "count": integer_prop(),
    });
    if let (Some(base), Some(extra)) = (properties.as_object_mut(), extra_properties.as_object()) {
        base.extend(extra.clone());
    }
    let mut required = vec!["columns", "rows", "count"];
    required.extend(required_extra.iter().copied());
    object_schema(properties, &required)
}

fn package_table_schema(extra_properties: Value, required_extra: &[&str]) -> Value {
    let mut properties = extra_properties;
    if let Some(map) = properties.as_object_mut() {
        map.insert("backend".into(), string_prop());
    }
    let mut required = required_extra.to_vec();
    required.push("backend");
    table_schema(PACKAGE_COLUMNS, properties, &required)
}

const SERVICE_LIST_COLUMNS: &[(&str, &str)] = &[
    ("name", "string"),
    ("description", "string"),
    ("load_state", "string"),
    ("active_state", "string"),
    ("sub_state", "string"),
];

const PACKAGE_COLUMNS: &[(&str, &str)] = &[
    ("name", "string"),
    ("evr", "string"),
    ("arch", "string"),
    ("repo_id", "string"),
    ("install_size", "integer"),
    ("summary", "string"),
];

const REPO_COLUMNS: &[(&str, &str)] =
    &[("id", "string"), ("name", "string"), ("enabled", "boolean")];

const NETWORK_LIST_COLUMNS: &[(&str, &str)] = &[
    ("interface", "string"),
    ("type", "string"),
    ("state", "string"),
    ("ip4", "string"),
    ("ip6", "string"),
    ("mac", "string"),
];

const FIREWALL_ZONE_LIST_COLUMNS: &[(&str, &str)] = &[
    ("zone", "string"),
    ("default", "boolean"),
    ("services", "string"),
    ("ports", "string"),
    ("interfaces", "string"),
];

fn package_info_schema() -> Value {
    object_schema(
        json!({
            "name": string_prop(),
            "evr": string_prop(),
            "arch": string_prop(),
            "repo_id": string_prop(),
            "install_size": nullable_integer_prop(),
            "summary": string_prop(),
            "backend": string_prop(),
        }),
        &[
            "name",
            "evr",
            "arch",
            "repo_id",
            "install_size",
            "summary",
            "backend",
        ],
    )
}

fn package_mutation_schema() -> Value {
    object_schema(
        json!({
            "operation": string_prop(),
            "specs": array_prop(),
            "dry_run": boolean_prop(),
            "install": array_prop(),
            "remove": array_prop(),
            "upgrade": array_prop(),
            "downgrade": array_prop(),
            "install_size_total": nullable_integer_prop(),
            "counts": object_schema(json!({
                "install": integer_prop(),
                "remove": integer_prop(),
                "upgrade": integer_prop(),
                "downgrade": integer_prop(),
            }), &["install", "remove", "upgrade", "downgrade"]),
            "backend": string_prop(),
        }),
        &[
            "operation",
            "specs",
            "dry_run",
            "install",
            "remove",
            "upgrade",
            "downgrade",
            "install_size_total",
            "counts",
            "backend",
        ],
    )
}

fn dry_run_schema() -> Value {
    object_schema(
        json!({
            "operation": string_prop(),
            "unit": string_prop(),
            "host": string_prop(),
            "privileged": boolean_prop(),
            "command": string_prop(),
        }),
        &["operation", "unit", "host", "privileged", "command"],
    )
}

fn alternate_output_schemas(descriptor: &Descriptor) -> Option<Value> {
    if !descriptor.flags.iter().any(|flag| flag == "--dry-run") {
        return None;
    }
    let alternate = match descriptor.output_kind.as_str() {
        "PackageMutation" => json!({"kind": "PackagePlan", "schema": package_mutation_schema()}),
        "ServiceMutation" | "ServiceEnablement" => {
            json!({"kind": "DryRun", "schema": dry_run_schema()})
        }
        _ => return None,
    };
    Some(json!([alternate]))
}

fn output_schema(kind: &str) -> Value {
    match kind {
        "ServiceList" => table_schema(SERVICE_LIST_COLUMNS, json!({}), &[]),
        "ServiceStatus" => object_schema(
            json!({
                "id": string_prop(),
                "description": string_prop(),
                "load_state": string_prop(),
                "active_state": string_prop(),
                "sub_state": string_prop(),
                "unit_file_state": string_prop(),
            }),
            &["id", "load_state", "active_state", "sub_state"],
        ),
        "LogEntries" => object_schema(
            json!({
                "unit": string_prop(),
                "entries": array_of(object_schema(json!({
                    "timestamp": string_prop(),
                    "priority": string_prop(),
                    "identifier": string_prop(),
                    "message": string_prop(),
                    "pid": string_prop(),
                }), &["timestamp", "priority", "identifier", "message", "pid"])),
            }),
            &["unit", "entries"],
        ),
        "ServiceMutation" => object_schema(
            json!({
                "operation": string_prop(),
                "unit": string_prop(),
                "host": string_prop(),
                "job": nullable_string_prop(),
            }),
            &["operation", "unit", "host"],
        ),
        "ServiceEnablement" => object_schema(
            json!({
                "operation": string_prop(),
                "unit": string_prop(),
                "host": string_prop(),
                "now": boolean_prop(),
                "changes": array_prop(),
            }),
            &["operation", "unit", "host", "now", "changes"],
        ),
        "PackageList" => package_table_schema(
            json!({
                "scope": string_prop(),
                "repos": array_prop(),
                "name": nullable_string_prop(),
                "total": integer_prop(),
                "returned": integer_prop(),
                "limit": nullable_integer_prop(),
                "offset": integer_prop(),
                "next_offset": nullable_integer_prop(),
            }),
            &[
                "scope",
                "repos",
                "name",
                "total",
                "returned",
                "limit",
                "offset",
                "next_offset",
            ],
        ),
        "PackageInfo" => package_info_schema(),
        "PackageSearch" => package_table_schema(json!({"pattern": string_prop()}), &["pattern"]),
        "PackageUpdates" => package_table_schema(json!({}), &[]),
        "RepoList" => {
            let mut properties = json!({"backend": string_prop()});
            table_schema(REPO_COLUMNS, properties.take(), &["backend"])
        }
        "PackageMutation" => package_mutation_schema(),
        "NetworkDeviceList" => table_schema(NETWORK_LIST_COLUMNS, json!({}), &[]),
        "NetworkDeviceDetail" => object_schema(
            json!({
                "interface": string_prop(),
                "type": string_prop(),
                "state": string_prop(),
                "mac": string_prop(),
                "mtu": integer_prop(),
                "ipv4": object_schema(json!({
                    "addresses": array_prop(),
                    "gateway": string_prop(),
                    "dns": array_prop(),
                    "domains": array_prop(),
                }), &["addresses", "gateway", "dns", "domains"]),
                "ipv6": object_schema(json!({"addresses": array_prop()}), &["addresses"]),
                "connection": json!({"type": ["object", "null"], "properties": {
                    "id": string_prop(),
                    "type": string_prop(),
                    "default": boolean_prop(),
                }}),
                "dhcp4": nullable_object_prop(),
            }),
            &[
                "interface",
                "type",
                "state",
                "mac",
                "mtu",
                "ipv4",
                "ipv6",
                "connection",
                "dhcp4",
            ],
        ),
        "FirewallStatus" => object_schema(
            json!({
                "running": boolean_prop(),
                "default_zone": string_prop(),
                "panic_mode": boolean_prop(),
                "masquerade": boolean_prop(),
                "pending_changes": array_prop(),
                "pending_changes_available": boolean_prop(),
            }),
            &[
                "running",
                "default_zone",
                "panic_mode",
                "masquerade",
                "pending_changes",
                "pending_changes_available",
            ],
        ),
        "FirewallZoneList" => table_schema(FIREWALL_ZONE_LIST_COLUMNS, json!({}), &[]),
        "FirewallZone" => object_schema(
            json!({
                "zone": string_prop(),
                "services": array_prop(),
                "ports": array_prop(),
                "interfaces": array_prop(),
                "sources": array_prop(),
                "masquerade": boolean_prop(),
            }),
            &[
                "zone",
                "services",
                "ports",
                "interfaces",
                "sources",
                "masquerade",
            ],
        ),
        "FirewallServiceCatalog" => object_schema(json!({"services": array_prop()}), &["services"]),
        "FirewallChange" => object_schema(
            json!({
                "operation": string_prop(),
                "zone": nullable_string_prop(),
                "change": nullable_string_prop(),
                "persisted": boolean_prop(),
                "panic_mode": nullable_boolean_prop(),
                "timeout": nullable_integer_prop(),
                "masquerade": nullable_boolean_prop(),
            }),
            &["operation", "persisted"],
        ),
        "FirewallConfirm" => object_schema(
            json!({
                "operation": string_prop(),
                "persisted": boolean_prop(),
            }),
            &["operation", "persisted"],
        ),
        _ => object_schema(json!({}), &[]),
    }
}

fn error_schema() -> Value {
    object_schema(
        json!({
            "code": string_prop(),
            "message": string_prop(),
            "detail": nullable_object_prop(),
        }),
        &["code", "message"],
    )
}

fn error_envelope_schema() -> Value {
    object_schema(
        json!({
            "apiVersion": string_prop(),
            "kind": {"type": "string", "const": "Error"},
            "host": string_prop(),
            "status": {"type": "string", "const": "error"},
            "error": error_schema(),
            "hints": nullable_object_prop(),
        }),
        &["apiVersion", "kind", "host", "status", "error"],
    )
}

fn input(name: &str, required: bool) -> Input {
    Input {
        name: name.into(),
        ty: "string".into(),
        required,
        default: None,
        choices: None,
    }
}

fn input_choices(name: &str, required: bool, choices: &[&str]) -> Input {
    Input {
        name: name.into(),
        ty: "string".into(),
        required,
        default: None,
        choices: Some(choices.iter().map(|choice| (*choice).to_string()).collect()),
    }
}

fn flag_schema(capability_id: &str, flag: &str) -> FlagSchema {
    let (ty, description, repeatable, default, choices, conflicts_with) = match flag {
        "--host" => (
            "string",
            "Target host. Defaults to localhost.",
            false,
            Some("localhost"),
            None,
            vec![],
        ),
        "--json" => (
            "boolean",
            "Emit a fez/v1 JSON envelope.",
            false,
            None,
            None,
            vec![],
        ),
        "--dry-run" => (
            "boolean",
            "Resolve and report the planned mutation without applying it.",
            false,
            None,
            None,
            vec![],
        ),
        "--force" => (
            "boolean",
            "Override command-specific safety guardrails.",
            false,
            None,
            None,
            vec![],
        ),
        "--state" => ("string", "Filter by state.", false, None, None, vec![]),
        "--since" => (
            "string",
            "Only include log entries since this journalctl time expression.",
            false,
            None,
            None,
            vec![],
        ),
        "--priority" => (
            "string",
            "Only include log entries at this priority or higher.",
            false,
            None,
            None,
            vec![],
        ),
        "--lines" => (
            "integer",
            "Limit log output to the last N entries.",
            false,
            None,
            None,
            vec![],
        ),
        "--follow" => (
            "boolean",
            "Stream new log entries.",
            false,
            None,
            None,
            vec![],
        ),
        "--now" => (
            "boolean",
            "Start or stop the unit immediately with the enablement change.",
            false,
            None,
            None,
            vec![],
        ),
        "--installed" => (
            "boolean",
            "List installed packages.",
            false,
            Some("true"),
            None,
            vec!["--available"],
        ),
        "--available" => (
            "boolean",
            "List available packages.",
            false,
            None,
            None,
            vec!["--installed"],
        ),
        "--repo" => (
            "string",
            "Restrict packages to this exact repository id.",
            true,
            None,
            None,
            vec![],
        ),
        "--enabled" => (
            "boolean",
            "Show only enabled repositories.",
            false,
            Some("true"),
            None,
            vec!["--disabled", "--all"],
        ),
        "--disabled" => (
            "boolean",
            "Show only disabled repositories.",
            false,
            None,
            None,
            vec!["--enabled", "--all"],
        ),
        "--all" if capability_id == "packages.repolist" => (
            "boolean",
            "Show all repositories.",
            false,
            None,
            None,
            vec!["--enabled", "--disabled"],
        ),
        "--all" => (
            "boolean",
            "Include all entries instead of the default subset.",
            false,
            None,
            None,
            vec![],
        ),
        "--zone" => (
            "string",
            "Firewall zone to target. Defaults to the target host's default zone.",
            false,
            None,
            None,
            vec![],
        ),
        "--timeout" => (
            "integer",
            "Auto-revert the runtime firewall change after this many seconds.",
            false,
            None,
            None,
            vec![],
        ),
        _ => (
            "string",
            "Capability-specific flag.",
            false,
            None,
            None,
            vec![],
        ),
    };
    FlagSchema {
        name: flag.to_string(),
        ty: ty.to_string(),
        description: description.to_string(),
        repeatable,
        default: default.map(str::to_string),
        choices: choices.map(|values: &[&str]| values.iter().map(|v| (*v).to_string()).collect()),
        conflicts_with: conflicts_with.into_iter().map(str::to_string).collect(),
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
