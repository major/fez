//! JSON Schema builders for output kinds, error envelopes, and shared
//! property helpers. Used by [`super::Descriptor::output_schema`].

use serde_json::{json, Value};

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

/// Build alternate output schemas for capabilities with `--dry-run`.
///
/// `has_dry_run` should be true when the capability lists the `--dry-run` flag.
pub(super) fn alternate_output_schemas(output_kind: &str, has_dry_run: bool) -> Option<Value> {
    if !has_dry_run {
        return None;
    }
    let alternate = match output_kind {
        "PackageMutation" => json!({"kind": "PackagePlan", "schema": package_mutation_schema()}),
        "ServiceMutation" | "ServiceEnablement" => {
            json!({"kind": "DryRun", "schema": dry_run_schema()})
        }
        _ => return None,
    };
    Some(json!([alternate]))
}

fn service_status_schema() -> Value {
    object_schema(
        json!({
            "id": string_prop(), "description": string_prop(),
            "load_state": string_prop(), "active_state": string_prop(),
            "sub_state": string_prop(), "unit_file_state": string_prop(),
        }),
        &["id", "load_state", "active_state", "sub_state"],
    )
}

fn log_entries_schema() -> Value {
    object_schema(
        json!({
            "unit": string_prop(),
            "entries": array_of(object_schema(json!({
                "timestamp": string_prop(), "priority": string_prop(),
                "identifier": string_prop(), "message": string_prop(), "pid": string_prop(),
            }), &["timestamp", "priority", "identifier", "message", "pid"])),
        }),
        &["unit", "entries"],
    )
}

fn service_mutation_schema() -> Value {
    object_schema(
        json!({
            "operation": string_prop(), "unit": string_prop(),
            "host": string_prop(), "job": nullable_string_prop(),
        }),
        &["operation", "unit", "host"],
    )
}

fn service_enablement_schema() -> Value {
    object_schema(
        json!({
            "operation": string_prop(), "unit": string_prop(),
            "host": string_prop(), "now": boolean_prop(), "changes": array_prop(),
        }),
        &["operation", "unit", "host", "now", "changes"],
    )
}

fn package_list_schema() -> Value {
    package_table_schema(
        json!({
            "scope": string_prop(), "repos": array_prop(), "name": nullable_string_prop(),
            "total": integer_prop(), "returned": integer_prop(),
            "limit": nullable_integer_prop(), "offset": integer_prop(),
            "next_offset": nullable_integer_prop(),
        }),
        &["scope", "repos", "name", "total", "returned", "limit", "offset", "next_offset"],
    )
}

fn repo_list_schema() -> Value {
    let mut properties = json!({"backend": string_prop()});
    table_schema(REPO_COLUMNS, properties.take(), &["backend"])
}

fn network_device_detail_schema() -> Value {
    object_schema(
        json!({
            "interface": string_prop(), "type": string_prop(),
            "state": string_prop(), "mac": string_prop(), "mtu": integer_prop(),
            "ipv4": object_schema(json!({
                "addresses": array_prop(), "gateway": string_prop(),
                "dns": array_prop(), "domains": array_prop(),
            }), &["addresses", "gateway", "dns", "domains"]),
            "ipv6": object_schema(json!({"addresses": array_prop()}), &["addresses"]),
            "connection": json!({"type": ["object", "null"], "properties": {
                "id": string_prop(), "type": string_prop(), "default": boolean_prop(),
            }}),
            "dhcp4": nullable_object_prop(),
        }),
        &["interface", "type", "state", "mac", "mtu", "ipv4", "ipv6", "connection", "dhcp4"],
    )
}

fn firewall_status_schema() -> Value {
    object_schema(
        json!({
            "running": boolean_prop(), "default_zone": string_prop(),
            "panic_mode": boolean_prop(), "masquerade": boolean_prop(),
            "pending_changes": array_prop(), "pending_changes_available": boolean_prop(),
        }),
        &["running", "default_zone", "panic_mode", "masquerade", "pending_changes", "pending_changes_available"],
    )
}

fn firewall_zone_schema() -> Value {
    object_schema(
        json!({
            "zone": string_prop(), "services": array_prop(), "ports": array_prop(),
            "interfaces": array_prop(), "sources": array_prop(), "masquerade": boolean_prop(),
        }),
        &["zone", "services", "ports", "interfaces", "sources", "masquerade"],
    )
}

fn firewall_change_schema() -> Value {
    object_schema(
        json!({
            "operation": string_prop(), "zone": nullable_string_prop(),
            "change": nullable_string_prop(), "persisted": boolean_prop(),
            "panic_mode": nullable_boolean_prop(), "timeout": nullable_integer_prop(),
            "masquerade": nullable_boolean_prop(),
        }),
        &["operation", "persisted"],
    )
}

fn firewall_confirm_schema() -> Value {
    object_schema(json!({"operation": string_prop(), "persisted": boolean_prop()}), &["operation", "persisted"])
}

/// Dispatch output kind to its JSON Schema. Adding a new kind requires only a
/// new named function above and one entry in this match.
pub(super) fn output_schema(kind: &str) -> Value {
    match kind {
        "ServiceList"          => table_schema(SERVICE_LIST_COLUMNS, json!({}), &[]),
        "ServiceStatus"        => service_status_schema(),
        "LogEntries"           => log_entries_schema(),
        "ServiceMutation"      => service_mutation_schema(),
        "ServiceEnablement"    => service_enablement_schema(),
        "PackageList"          => package_list_schema(),
        "PackageInfo"          => package_info_schema(),
        "PackageSearch"        => package_table_schema(json!({"pattern": string_prop()}), &["pattern"]),
        "PackageUpdates"       => package_table_schema(json!({}), &[]),
        "RepoList"             => repo_list_schema(),
        "PackageMutation"      => package_mutation_schema(),
        "NetworkDeviceList"    => table_schema(NETWORK_LIST_COLUMNS, json!({}), &[]),
        "NetworkDeviceDetail"  => network_device_detail_schema(),
        "FirewallStatus"       => firewall_status_schema(),
        "FirewallZoneList"     => table_schema(FIREWALL_ZONE_LIST_COLUMNS, json!({}), &[]),
        "FirewallZone"         => firewall_zone_schema(),
        "FirewallServiceCatalog" => object_schema(json!({"services": array_prop()}), &["services"]),
        "FirewallChange"       => firewall_change_schema(),
        "FirewallConfirm"      => firewall_confirm_schema(),
        _                      => object_schema(json!({}), &[]),
    }
}

/// JSON Schema for the error payload inside a `fez/v1` error envelope.
pub(super) fn error_schema() -> Value {
    object_schema(
        json!({
            "code": string_prop(),
            "message": string_prop(),
            "detail": nullable_object_prop(),
        }),
        &["code", "message"],
    )
}

/// JSON Schema for the full error envelope.
pub(super) fn error_envelope_schema() -> Value {
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
