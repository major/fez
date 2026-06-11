//! End-to-end MCP coverage: drive a live `fez mcp` process through a full
//! JSON-RPC conversation, with `invoke` re-execing `fez` against the fake bridge.
use assert_cmd::Command;
use predicates::str::contains;
use serde_json::{json, Value};

mod common;
use common::fez_fake;

fn fez_mcp() -> Command {
    // The mcp server inherits FEZ_BRIDGE and passes it to the `fez` child it
    // re-execs for `invoke`, so the whole round-trip uses the fake bridge.
    let mut c = fez_fake();
    c.arg("mcp");
    c
}

fn fez_mcp_expanded() -> Command {
    let mut c = fez_fake();
    c.arg("mcp").arg("--expanded-tools");
    c
}

fn fez_mcp_host(host: &str) -> Command {
    let mut c = fez_fake();
    c.arg("--host").arg(host).arg("mcp");
    c
}

fn response_line(stdout: &[u8]) -> Value {
    let text = std::str::from_utf8(stdout).expect("utf8 stdout");
    serde_json::from_str(text.lines().next().expect("one response line")).expect("json response")
}

const CONVERSATION: &str = concat!(
    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#,
    "\n",
    r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
    "\n",
    r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
    "\n",
    r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"invoke","arguments":{"capability":"services.list"}}}"#,
    "\n",
);

#[test]
fn full_conversation_initializes_lists_and_invokes() {
    fez_mcp()
        .write_stdin(CONVERSATION)
        .assert()
        .success()
        // initialize result
        .stdout(contains("\"serverInfo\""))
        .stdout(contains("\"name\":\"fez\""))
        // tools/list result
        .stdout(contains("list_capabilities"))
        .stdout(contains("invoke"))
        // invoke result carries the real fez/v1 ServiceList envelope (the
        // envelope is embedded as the escaped `text` of a content block)
        .stdout(contains("ServiceList"))
        .stdout(contains("sshd.service"));
}

#[test]
fn tools_list_reports_server_default_host() {
    let convo = concat!(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "\n",);
    fez_mcp_host("web1.example.com")
        .write_stdin(convo)
        .assert()
        .success()
        .stdout(contains("default target host: web1.example.com"));
}

#[test]
fn expanded_tools_list_includes_strict_capability_schemas() {
    let convo = concat!(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "\n");
    let output = fez_mcp_expanded()
        .write_stdin(convo)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let response = response_line(&output);
    let tools = response["result"]["tools"].as_array().unwrap();
    assert!(tools.iter().any(|tool| tool["name"] == "invoke"));

    let status = tools
        .iter()
        .find(|tool| tool["name"] == "services_status")
        .expect("services_status tool");
    assert_eq!(status["inputSchema"]["required"], json!(["unit"]));
    assert_eq!(
        status["inputSchema"]["properties"]["unit"]["type"],
        "string"
    );

    let panic = tools
        .iter()
        .find(|tool| tool["name"] == "firewall_panic")
        .expect("firewall_panic tool");
    assert_eq!(panic["inputSchema"]["required"], json!(["state"]));
    assert_eq!(
        panic["inputSchema"]["properties"]["state"]["enum"],
        json!(["on", "off"])
    );
    assert_eq!(
        panic["inputSchema"]["properties"]["force"]["type"],
        "boolean"
    );

    let start = tools
        .iter()
        .find(|tool| tool["name"] == "services_start")
        .expect("services_start tool");
    assert_eq!(
        start["inputSchema"]["properties"]["dry_run"]["type"],
        "boolean"
    );
    assert_eq!(
        start["inputSchema"]["properties"]["force"]["type"],
        "boolean"
    );

    let packages_list = tools
        .iter()
        .find(|tool| tool["name"] == "packages_list")
        .expect("packages_list tool");
    assert_eq!(
        packages_list["inputSchema"]["properties"]["repo"]["type"],
        "array"
    );
    assert_eq!(
        packages_list["inputSchema"]["properties"]["repo"]["items"]["type"],
        "string"
    );

    let packages_install = tools
        .iter()
        .find(|tool| tool["name"] == "packages_install")
        .expect("packages_install tool");
    assert_eq!(
        packages_install["inputSchema"]["properties"]["specs"]["oneOf"],
        json!([
            {"type": "string"},
            {"type": "array", "items": {"type": "string"}}
        ])
    );
}

#[test]
fn expanded_capability_tool_invokes_without_freeform_inputs_wrapper() {
    let convo = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"services_status","arguments":{"unit":"sshd.service"}}}"#,
        "\n",
    );
    fez_mcp_expanded()
        .write_stdin(convo)
        .assert()
        .success()
        .stdout(contains("ServiceStatus"))
        .stdout(contains("sshd.service"));
}

#[test]
fn expanded_capability_tool_expands_repeatable_flags() {
    let convo = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"packages_list","arguments":{"repo":["fedora","updates"]}}}"#,
        "\n",
    );
    fez_mcp_expanded()
        .write_stdin(convo)
        .assert()
        .success()
        .stdout(contains("PackageList"))
        .stdout(contains("bash"))
        .stdout(contains("vim-enhanced"));
}

#[test]
fn invoke_unknown_capability_is_tool_error() {
    let convo = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"invoke","arguments":{"capability":"does.not.exist"}}}"#,
        "\n",
    );
    fez_mcp()
        .write_stdin(convo)
        .assert()
        .success()
        // A well-formed invoke naming an unknown capability is a tool error
        // (isError), not a JSON-RPC protocol error, matching describe_capability.
        .stdout(contains("\"isError\":true"))
        .stdout(contains("unknown capability"));
}
