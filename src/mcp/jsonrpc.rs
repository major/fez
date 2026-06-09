//! Minimal JSON-RPC 2.0 types for the MCP stdio transport. A request without an
//! `id` is a notification and receives no response (JSON-RPC 2.0 §4.1).
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// An incoming JSON-RPC message. `id` absent => notification.
#[derive(Debug, Deserialize)]
pub struct Request {
    /// Request id; absent for notifications.
    #[serde(default)]
    pub id: Option<Value>,
    /// JSON-RPC method name.
    pub method: String,
    /// Method parameters; defaults to JSON null when omitted.
    #[serde(default)]
    pub params: Value,
}

impl Request {
    /// True when this message is a notification (has no `id`).
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

/// An outgoing JSON-RPC response. Exactly one of `result`/`error` is set.
#[derive(Debug, Serialize)]
pub struct Response {
    /// Protocol marker, always `"2.0"`.
    pub jsonrpc: &'static str,
    /// Id echoed from the originating request.
    pub id: Value,
    /// Success result, mutually exclusive with `error`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Error detail, mutually exclusive with `result`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

/// A JSON-RPC error object.
#[derive(Debug, Serialize)]
pub struct RpcError {
    /// Numeric JSON-RPC error code.
    pub code: i64,
    /// Human-readable error message.
    pub message: String,
}

impl Response {
    /// Build a success response carrying `result`.
    pub fn ok(id: Value, result: Value) -> Self {
        Response {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }
    /// Build an error response with the given code and message.
    pub fn err(id: Value, code: i64, message: &str) -> Self {
        Response {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_request_with_id() {
        let r: Request = serde_json::from_value(json!({
            "jsonrpc":"2.0","id":1,"method":"tools/list","params":{}
        }))
        .unwrap();
        assert_eq!(r.method, "tools/list");
        assert!(!r.is_notification());
    }

    #[test]
    fn parses_notification_without_id() {
        let r: Request = serde_json::from_value(json!({
            "jsonrpc":"2.0","method":"notifications/initialized"
        }))
        .unwrap();
        assert!(r.is_notification());
    }

    #[test]
    fn ok_response_omits_error() {
        let v = serde_json::to_value(Response::ok(json!(1), json!({"ok":true}))).unwrap();
        assert_eq!(v, json!({"jsonrpc":"2.0","id":1,"result":{"ok":true}}));
    }

    #[test]
    fn err_response_omits_result() {
        let v = serde_json::to_value(Response::err(json!(2), -32601, "method not found")).unwrap();
        assert_eq!(
            v,
            json!({"jsonrpc":"2.0","id":2,"error":{"code":-32601,"message":"method not found"}})
        );
    }
}
