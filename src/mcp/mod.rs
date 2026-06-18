//! An MCP gateway over stdio: newline-delimited JSON-RPC 2.0 advertising three
//! frugal meta-tools (`list_capabilities`, `describe_capability`, `invoke`) by
//! default, with optional expanded per-capability tools for clients that prefer
//! strict JSON schemas up front.
pub mod jsonrpc;

use crate::mcp::jsonrpc::{Request, Response};
use crate::schema;
use serde_json::{json, Value};
use std::io::{BufRead, Write};

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = "fez";

/// Global CLI flags. `invoke` supplies these only from its structured top-level
/// fields (`host`/`dry_run`/`force`) plus the always-appended `--json`, never
/// from the free-form `inputs` object.
const GLOBAL_FLAGS: [&str; 4] = ["--host", "--json", "--dry-run", "--force"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServerContext {
    default_host: String,
    expanded_tools: bool,
}

impl ServerContext {
    fn new(default_host: String, expanded_tools: bool) -> Self {
        Self {
            default_host,
            expanded_tools,
        }
    }

    fn default_localhost() -> Self {
        Self::new("localhost".to_string(), false)
    }

    fn default_host(&self) -> &str {
        &self.default_host
    }

    fn expanded_tools(&self) -> bool {
        self.expanded_tools
    }
}

/// Run the MCP server: read newline-delimited JSON-RPC from stdin, write one
/// response line per request to stdout, until EOF. Returns the process exit code.
pub fn run() -> i32 {
    run_with_context(&ServerContext::default_localhost())
}

/// Run the MCP server using `default_host` for invoke calls that omit
/// `arguments.host`.
pub fn run_with_host(default_host: &str, expanded_tools: bool) -> i32 {
    run_with_context(&ServerContext::new(
        default_host.to_string(),
        expanded_tools,
    ))
}

fn run_with_context(context: &ServerContext) -> i32 {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(resp) = handle_line_with_context(&line, context) {
            let Ok(json) = serde_json::to_string(&resp) else {
                eprintln!("MCP response serialization failed");
                return 2;
            };
            if writeln!(stdout, "{json}").is_err() {
                break;
            }
            let _ = stdout.flush();
        }
    }
    0
}

/// Handle one input line. Returns `None` for notifications (no reply per
/// JSON-RPC 2.0). Parse failures yield a `-32700` error with a null id.
#[cfg(test)]
fn handle_line(line: &str) -> Option<Response> {
    handle_line_with_context(line, &ServerContext::default_localhost())
}

fn handle_line_with_context(line: &str, context: &ServerContext) -> Option<Response> {
    let req: Request = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(_) => return Some(Response::err(Value::Null, -32700, "parse error")),
    };
    if req.is_notification() {
        return None; // notifications/initialized, notifications/cancelled, ...
    }
    let id = req.id.clone().unwrap_or(Value::Null);
    let resp = match req.method.as_str() {
        "initialize" => Response::ok(id, initialize_result(&req.params, context)),
        "ping" => Response::ok(id, json!({})),
        "tools/list" => Response::ok(id, json!({ "tools": tool_list(context) })),
        "tools/call" => match tools_call(&req.params, context) {
            Ok(result) => Response::ok(id, result),
            Err((code, msg)) => Response::err(id, code, &msg),
        },
        _ => Response::err(id, -32601, "method not found"),
    };
    Some(resp)
}

fn initialize_result(params: &Value, context: &ServerContext) -> Value {
    let version = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(PROTOCOL_VERSION);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": SERVER_NAME,
            "version": env!("CARGO_PKG_VERSION"),
            "defaultTargetHost": context.default_host()
        }
    })
}

fn tool_list(context: &ServerContext) -> Value {
    let invoke_description = format!(
        "Invoke a fez capability and return its fez/v1 JSON envelope. Mutations honor the full safety layer (protected units, dry-run, audit). Server default target host: {}. Set arguments.host to override it for one call.",
        context.default_host()
    );
    let mut tools = vec![
        json!({
            "name": "list_capabilities",
            "description": "List fez capability ids (e.g. services.list, services.start). On-demand discovery; nothing is preloaded.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "describe_capability",
            "description": "Return the descriptor for one capability: summary, inputs, output kind, flags, whether it is privileged, and an example.",
            "inputSchema": {
                "type": "object",
                "properties": { "capability": { "type": "string", "description": "Capability id, e.g. services.status" } },
                "required": ["capability"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "invoke",
            "description": invoke_description,
            "inputSchema": {
                "type": "object",
                "properties": {
                    "capability": { "type": "string" },
                    "inputs": { "type": "object", "description": "Capability inputs by name, e.g. {\"unit\": \"sshd.service\"}." },
                    "host": { "type": "string" },
                    "dry_run": { "type": "boolean" },
                    "force": { "type": "boolean" }
                },
                "required": ["capability"],
                "additionalProperties": false
            }
        }),
    ];
    if context.expanded_tools() {
        tools.extend(schema::registry().into_iter().map(expanded_tool));
    }
    Value::Array(tools)
}

fn expanded_tool(d: schema::Descriptor) -> Value {
    json!({
        "name": expanded_tool_name(d.id),
        "description": expanded_tool_description(&d),
        "inputSchema": expanded_tool_schema(&d),
    })
}

fn expanded_tool_name(id: &str) -> String {
    id.replace('.', "_")
}

fn expanded_tool_description(d: &schema::Descriptor) -> String {
    let mut description = format!("{} {}", d.summary, d.long);
    if d.privileged
        || d.flags
            .iter()
            .any(|flag| *flag == "--dry-run" || *flag == "--force")
    {
        description.push_str(
            " Supports dry_run to preview mutations and force to override command-specific safety guardrails when advertised in the input schema.",
        );
    }
    description
}

fn expanded_tool_schema(d: &schema::Descriptor) -> Value {
    let mut properties = serde_json::Map::new();
    let mut required: Vec<Value> = Vec::new();
    for input in &d.inputs {
        properties.insert(input.name.into(), input_schema(d, input));
        if input.required {
            required.push(json!(input.name));
        }
    }
    for flag in d.flag_schema() {
        if flag.name == "--json" {
            continue;
        }
        properties.insert(flag_arg_name(flag.name), flag_schema(&flag));
    }
    let mut schema = json!({
        "type": "object",
        "properties": properties,
        "additionalProperties": false,
    });
    if !required.is_empty() {
        schema["required"] = json!(required);
    }
    schema
}

fn input_schema(d: &schema::Descriptor, input: &schema::Input) -> Value {
    if is_variadic_input(d, input) {
        return json!({
            "oneOf": [
                { "type": input.ty },
                { "type": "array", "items": { "type": input.ty } }
            ]
        });
    }
    let mut schema = json!({ "type": input.ty });
    if let Some(default) = &input.default {
        schema["default"] = json!(default);
    }
    if let Some(choices) = &input.choices {
        schema["enum"] = json!(choices);
    }
    schema
}

fn is_variadic_input(d: &schema::Descriptor, input: &schema::Input) -> bool {
    input.name == "specs" && d.id.starts_with("packages.")
}

fn flag_schema(flag: &schema::FlagSchema) -> Value {
    let mut schema = if flag.repeatable {
        json!({
            "type": "array",
            "items": { "type": flag.ty },
            "description": flag.description,
            "repeatable": true,
        })
    } else {
        json!({ "type": flag.ty, "description": flag.description })
    };
    if let Some(default) = &flag.default {
        schema["default"] = json!(default);
    }
    if let Some(choices) = &flag.choices {
        schema["enum"] = json!(choices);
    }
    if !flag.conflicts_with.is_empty() {
        schema["conflicts_with"] = json!(flag.conflicts_with);
    }
    schema
}

fn flag_arg_name(flag: &str) -> String {
    flag.trim_start_matches("--").replace('-', "_")
}

/// Dispatch a `tools/call`. Err carries a JSON-RPC (code, message); protocol-level
/// failures (e.g. a capability that errors) are reported inside the tool result
/// with `isError: true`, per MCP convention.
fn tools_call(params: &Value, context: &ServerContext) -> Result<Value, (i64, String)> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or((-32602, "missing tool name".to_string()))?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    match name {
        "list_capabilities" => Ok(text_result(&list_capabilities_text(), false)),
        "describe_capability" => {
            let id = args
                .get("capability")
                .and_then(Value::as_str)
                .ok_or((-32602, "missing 'capability'".to_string()))?;
            match schema::find(id) {
                Some(d) => {
                    let text = serde_json::to_string_pretty(&d)
                        .unwrap_or_else(|e| format!("descriptor serialization error: {e}"));
                    Ok(text_result(&text, false))
                }
                None => Ok(text_result(&format!("unknown capability: {id}"), true)),
            }
        }
        "invoke" => {
            let id = args
                .get("capability")
                .and_then(Value::as_str)
                .ok_or((-32602, "missing 'capability'".to_string()))?;
            invoke(id, &args, context)
        }
        other if context.expanded_tools() => match capability_for_tool(other) {
            Some(d) => invoke_expanded(&d, &args, context),
            None => Err((-32602, format!("unknown tool: {other}"))),
        },
        other => Err((-32602, format!("unknown tool: {other}"))),
    }
}

fn capability_for_tool(name: &str) -> Option<schema::Descriptor> {
    schema::registry()
        .into_iter()
        .find(|d| expanded_tool_name(d.id) == name)
}

fn text_result(text: &str, is_error: bool) -> Value {
    json!({ "content": [ { "type": "text", "text": text } ], "isError": is_error })
}

fn list_capabilities_text() -> String {
    schema::registry()
        .into_iter()
        .map(|d| d.id)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Invoke a capability by re-execing `fez` with `--json`, so the call traverses
/// the real CLI path and its safety layer (Plan 2). The child's stdout (the
/// fez/v1 envelope) becomes the tool result; a non-zero exit sets isError.
fn invoke(id: &str, args: &Value, context: &ServerContext) -> Result<Value, (i64, String)> {
    // A well-formed `invoke` call naming an unknown capability is a domain-level
    // failure, not a malformed request: report it as a tool error (isError),
    // consistent with `describe_capability`, rather than a JSON-RPC error.
    let descriptor = match schema::find(id) {
        Some(d) => d,
        None => return Ok(text_result(&format!("unknown capability: {id}"), true)),
    };
    let argv = build_argv_with_context(&descriptor, args, context);
    let exe = std::env::current_exe().map_err(|e| (-32603, format!("locate fez binary: {e}")))?;
    let out = std::process::Command::new(exe)
        .args(&argv)
        .output()
        .map_err(|e| (-32603, format!("spawn fez: {e}")))?;
    let text = if out.stdout.is_empty() {
        String::from_utf8_lossy(&out.stderr).into_owned()
    } else {
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    Ok(text_result(&text, !out.status.success()))
}

fn invoke_expanded(
    descriptor: &schema::Descriptor,
    args: &Value,
    context: &ServerContext,
) -> Result<Value, (i64, String)> {
    let mut inputs = serde_json::Map::new();
    for input in &descriptor.inputs {
        if let Some(value) = args.get(input.name) {
            inputs.insert(input.name.into(), value.clone());
        }
    }
    for flag in &descriptor.flags {
        match *flag {
            "--host" | "--json" | "--dry-run" | "--force" => continue,
            _ => {
                let arg_name = flag_arg_name(flag);
                if let Some(value) = args.get(&arg_name) {
                    inputs.insert(flag.trim_start_matches("--").to_string(), value.clone());
                }
            }
        }
    }
    let mut wrapped = json!({ "inputs": inputs });
    if let Some(value) = args.get("host") {
        wrapped["host"] = value.clone();
    }
    if let Some(value) = args.get("dry_run") {
        wrapped["dry_run"] = value.clone();
    }
    if let Some(value) = args.get("force") {
        wrapped["force"] = value.clone();
    }
    invoke(descriptor.id, &wrapped, context)
}

/// Translate an invoke request into `fez` argv. The capability id becomes the
/// verb path (`services.start` -> ["services","start"]). A named input is a
/// positional argument unless the descriptor declares a matching `--flag`, in
/// which case it is emitted as a flag. A string positional is pushed as one
/// argv entry; an array positional is spread into multiple positional entries
/// (variadic inputs such as `packages.install`'s `specs`), skipping any
/// non-string element. `host`/`dry_run`/`force` map to globals; `--json` is
/// always appended so the result is a fez/v1 envelope.
#[cfg(test)]
fn build_argv(d: &schema::Descriptor, args: &Value) -> Vec<String> {
    build_argv_with_context(d, args, &ServerContext::default_localhost())
}

pub(crate) fn build_argv_with_context(
    d: &schema::Descriptor,
    args: &Value,
    context: &ServerContext,
) -> Vec<String> {
    let mut argv: Vec<String> = d.id.split('.').map(|s| s.to_string()).collect();
    let inputs = args.get("inputs").cloned().unwrap_or_else(|| json!({}));

    // Positional inputs first, in descriptor order, skipping flag-style ones.
    for input in &d.inputs {
        let flag = format!("--{}", input.name);
        if d.flags.iter().any(|f| f == &flag) {
            continue; // handled in the flag pass
        }
        match inputs.get(input.name) {
            Some(Value::String(s)) => argv.push(s.clone()),
            Some(Value::Array(items)) => {
                for item in items {
                    if let Some(s) = item.as_str() {
                        argv.push(s.to_string());
                    }
                }
            }
            _ => {}
        }
    }
    // Remaining inputs that correspond to declared flags. Global flags are
    // excluded here: they come only from the structured top-level fields below,
    // so an `inputs` key cannot shadow them or inject a duplicate `--json`.
    if let Some(obj) = inputs.as_object() {
        for (k, v) in obj {
            let flag = format!("--{k}");
            if GLOBAL_FLAGS.contains(&flag.as_str()) {
                continue;
            }
            if d.flags.iter().any(|f| f == &flag) {
                push_flag(&mut argv, &flag, v);
            }
        }
    }
    if let Some(h) = args.get("host").and_then(Value::as_str) {
        argv.push("--host".into());
        argv.push(h.into());
    } else if context.default_host() != "localhost" {
        argv.push("--host".into());
        argv.push(context.default_host().into());
    }
    if args.get("dry_run").and_then(Value::as_bool) == Some(true) {
        argv.push("--dry-run".into());
    }
    if args.get("force").and_then(Value::as_bool) == Some(true) {
        argv.push("--force".into());
    }
    argv.push("--json".into());
    argv
}

fn push_flag(argv: &mut Vec<String>, flag: &str, v: &Value) {
    match v {
        Value::Array(items) => {
            for item in items {
                push_flag(argv, flag, item);
            }
        }
        Value::Bool(true) => argv.push(flag.to_string()),
        Value::Bool(false) | Value::Null => {}
        Value::String(s) => {
            argv.push(flag.to_string());
            argv.push(s.clone());
        }
        other => {
            argv.push(flag.to_string());
            argv.push(other.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(line: &str) -> Value {
        serde_json::to_value(handle_line(line).expect("response")).unwrap()
    }

    fn expanded_call(line: &str) -> Value {
        let context = ServerContext::new("localhost".to_string(), true);
        serde_json::to_value(handle_line_with_context(line, &context).expect("response")).unwrap()
    }

    #[test]
    fn initialize_returns_server_info_and_echoes_version() {
        let v = call(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#,
        );
        assert_eq!(v["result"]["serverInfo"]["name"], "fez");
        assert_eq!(v["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(v["result"]["capabilities"]["tools"], json!({}));
    }

    #[test]
    fn notification_yields_no_response() {
        assert!(handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
    }

    #[test]
    fn tools_list_advertises_three_meta_tools_in_order() {
        let v = call(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
        let names: Vec<&str> = v["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            vec!["list_capabilities", "describe_capability", "invoke"]
        );
    }

    #[test]
    fn list_capabilities_tool_lists_ids() {
        let v = call(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_capabilities","arguments":{}}}"#,
        );
        let text = v["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("services.list"));
        assert!(text.contains("services.start")); // from Plan 2
        assert_eq!(v["result"]["isError"], false);
    }

    #[test]
    fn describe_capability_tool_returns_descriptor() {
        let v = call(
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"describe_capability","arguments":{"capability":"services.status"}}}"#,
        );
        let text = v["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("ServiceStatus"));
        assert_eq!(v["result"]["isError"], false);
    }

    #[test]
    fn describe_unknown_capability_is_tool_error_not_rpc_error() {
        let v = call(
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"describe_capability","arguments":{"capability":"nope"}}}"#,
        );
        assert!(v.get("result").is_some());
        assert_eq!(v["result"]["isError"], true);
    }

    #[test]
    fn unknown_method_is_method_not_found() {
        let v = call(r#"{"jsonrpc":"2.0","id":6,"method":"frobnicate"}"#);
        assert_eq!(v["error"]["code"], -32601);
    }

    #[test]
    fn unknown_expanded_tool_is_invalid_params() {
        let v = expanded_call(
            r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"not_a_tool","arguments":{}}}"#,
        );
        assert_eq!(v["error"]["code"], -32602);
        assert_eq!(v["error"]["message"], "unknown tool: not_a_tool");
    }

    #[test]
    fn input_schema_includes_default_and_choices() {
        let descriptor = schema::Descriptor {
            id: "test.input",
            summary: "test",
            long: "test",
            privileged: false,
            output_kind: "Test",
            inputs: vec![],
            flags: vec![],
            examples: vec!["fez test".into()],
        };
        let input = schema::Input {
            name: "mode",
            ty: "string",
            required: false,
            default: Some("safe"),
            choices: Some(vec!["safe", "fast"]),
        };
        assert_eq!(
            input_schema(&descriptor, &input),
            json!({"type":"string","default":"safe","enum":["safe","fast"]})
        );
    }

    #[test]
    fn flag_schema_includes_choices() {
        let flag = schema::FlagSchema {
            name: "--mode",
            ty: "string",
            description: "Mode to use.",
            repeatable: false,
            default: None,
            choices: Some(vec!["safe", "fast"]),
            conflicts_with: vec![],
        };
        assert_eq!(
            flag_schema(&flag),
            json!({"type":"string","description":"Mode to use.","enum":["safe","fast"]})
        );
    }

    #[test]
    fn garbage_input_is_parse_error_with_null_id() {
        let v = call("not json at all");
        assert_eq!(v["error"]["code"], -32700);
        assert_eq!(v["id"], Value::Null);
    }

    use serde_json::json as j;

    fn argv_for(id: &str, args: serde_json::Value) -> Vec<String> {
        let d = schema::find(id).unwrap();
        build_argv(&d, &args)
    }

    fn argv_for_with_host(id: &str, default_host: &str, args: serde_json::Value) -> Vec<String> {
        let d = schema::find(id).unwrap();
        let context = ServerContext::new(default_host.to_string(), false);
        build_argv_with_context(&d, &args, &context)
    }

    #[test]
    fn build_argv_status_unit_is_positional() {
        assert_eq!(
            argv_for("services.status", j!({"inputs": {"unit": "sshd.service"}})),
            vec!["services", "status", "sshd.service", "--json"]
        );
    }

    #[test]
    fn build_argv_list_state_is_a_flag() {
        // services.list declares both an input `state` and a `--state` flag,
        // so it must be emitted as a flag, not a positional.
        assert_eq!(
            argv_for("services.list", j!({"inputs": {"state": "failed"}})),
            vec!["services", "list", "--state", "failed", "--json"]
        );
    }

    #[test]
    fn build_argv_start_threads_host_and_dry_run() {
        assert_eq!(
            argv_for(
                "services.start",
                j!({"inputs": {"unit": "nginx.service"}, "host": "web1", "dry_run": true})
            ),
            vec![
                "services",
                "start",
                "nginx.service",
                "--host",
                "web1",
                "--dry-run",
                "--json"
            ]
        );
    }

    #[test]
    fn build_argv_uses_default_host_when_call_omits_host() {
        assert_eq!(
            argv_for_with_host(
                "services.status",
                "web1",
                j!({"inputs": {"unit": "sshd.service"}})
            ),
            vec![
                "services",
                "status",
                "sshd.service",
                "--host",
                "web1",
                "--json"
            ]
        );
    }

    #[test]
    fn build_argv_call_host_overrides_default_host() {
        assert_eq!(
            argv_for_with_host(
                "services.status",
                "web1",
                j!({"inputs": {"unit": "sshd.service"}, "host": "web2"})
            ),
            vec![
                "services",
                "status",
                "sshd.service",
                "--host",
                "web2",
                "--json"
            ]
        );
    }

    #[test]
    fn build_argv_enable_now_is_a_bool_flag() {
        assert_eq!(
            argv_for(
                "services.enable",
                j!({"inputs": {"unit": "chronyd.service", "now": true}})
            ),
            vec!["services", "enable", "chronyd.service", "--now", "--json"]
        );
    }

    #[test]
    fn build_argv_force_maps_to_global_flag() {
        assert_eq!(
            argv_for(
                "services.stop",
                j!({"inputs": {"unit": "sshd.service"}, "force": true})
            ),
            vec!["services", "stop", "sshd.service", "--force", "--json"]
        );
    }

    #[test]
    fn build_argv_inputs_cannot_shadow_global_flags() {
        // Global flags must come only from the structured top-level fields, not
        // from `inputs`: no duplicate `--json`, no smuggled `--force`.
        let argv = argv_for(
            "services.start",
            j!({"inputs": {"unit": "nginx.service", "json": true, "force": true}}),
        );
        assert_eq!(argv, vec!["services", "start", "nginx.service", "--json"]);
        assert_eq!(argv.iter().filter(|a| *a == "--json").count(), 1);
        assert!(!argv.contains(&"--force".to_string()));
    }

    #[test]
    fn build_argv_packages_remove_single_spec_string() {
        assert_eq!(
            argv_for("packages.remove", j!({"inputs": {"specs": "htop"}})),
            vec!["packages", "remove", "htop", "--json"]
        );
    }

    #[test]
    fn build_argv_packages_install_spreads_spec_array() {
        assert_eq!(
            argv_for(
                "packages.install",
                j!({"inputs": {"specs": ["htop", "nginx"]}})
            ),
            vec!["packages", "install", "htop", "nginx", "--json"]
        );
    }

    #[test]
    fn build_argv_packages_install_skips_non_string_spec() {
        assert_eq!(
            argv_for(
                "packages.install",
                j!({"inputs": {"specs": ["htop", 42, "nginx"]}})
            ),
            vec!["packages", "install", "htop", "nginx", "--json"]
        );
    }
}
