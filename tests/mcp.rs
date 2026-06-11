//! End-to-end MCP coverage: drive a live `fez mcp` process through a full
//! JSON-RPC conversation, with `invoke` re-execing `fez` against the fake bridge.
use assert_cmd::Command;
use predicates::str::contains;

mod common;
use common::fez_fake;

fn fez_mcp() -> Command {
    // The mcp server inherits FEZ_BRIDGE and passes it to the `fez` child it
    // re-execs for `invoke`, so the whole round-trip uses the fake bridge.
    let mut c = fez_fake();
    c.arg("mcp");
    c
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
