//! Machine-readable descriptions of every capability fez exposes, used to
//! advertise the command surface (ids, inputs, flags, examples) to agents.
use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};
use serde_json::{json, Value};

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
            inputs: vec![input_choices("state", true, &["on", "off"])],
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
            inputs: vec![input_choices("state", true, &["on", "off"])],
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
