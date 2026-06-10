//! The `fez/v1` JSON response envelope shared by every command's `--json` output.
use serde::Serialize;
use serde_json::Value;

/// The envelope schema version string emitted in `apiVersion`.
pub const API_VERSION: &str = "fez/v1";

/// The machine-readable response wrapper for every command.
#[derive(Serialize)]
pub struct Envelope {
    /// Schema version, always [`API_VERSION`].
    #[serde(rename = "apiVersion")]
    pub api_version: &'static str,
    /// The payload kind (e.g. `ServiceList`, `Error`).
    pub kind: String,
    /// Host the response pertains to.
    pub host: String,
    /// Whether the operation succeeded.
    pub status: Status,
    /// Success payload, present when `status` is `ok`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    /// Error payload, present when `status` is `error`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiError>,
    /// Optional machine-actionable hints (e.g. a reverse command).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hints: Option<Value>,
}

/// Outcome of an operation.
#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// The operation succeeded.
    Ok,
    /// The operation failed; see the envelope's `error`.
    Error,
}

/// Structured error detail carried in an error envelope.
#[derive(Serialize)]
pub struct ApiError {
    /// Stable machine-readable error code.
    pub code: String,
    /// Human-readable error message.
    pub message: String,
    /// Optional extra structured detail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<Value>,
}

impl Envelope {
    /// Build a success envelope wrapping `data`.
    pub fn ok(kind: &str, host: &str, data: Value) -> Self {
        Envelope {
            api_version: API_VERSION,
            kind: kind.into(),
            host: host.into(),
            status: Status::Ok,
            data: Some(data),
            error: None,
            hints: None,
        }
    }
    /// Build an error envelope carrying `err`.
    pub fn error(kind: &str, host: &str, err: ApiError) -> Self {
        Envelope {
            api_version: API_VERSION,
            kind: kind.into(),
            host: host.into(),
            status: Status::Error,
            data: None,
            error: Some(err),
            hints: None,
        }
    }
    /// Attach machine-actionable hints (e.g. the reversibility hint, Section 8).
    pub fn with_hints(mut self, hints: Value) -> Self {
        self.hints = Some(hints);
        self
    }
    /// Serialize the envelope to a pretty-printed JSON string.
    ///
    /// Returns a valid `fez/v1` error envelope on serialization failure so that
    /// callers always receive syntactically correct JSON.
    pub fn to_json_string(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| {
            r#"{
  "apiVersion": "fez/v1",
  "kind": "Error",
  "host": "",
  "status": "error",
  "error": {
    "code": "internal",
    "message": "envelope serialization failed"
  }
}"#
            .to_string()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ok_envelope_shape() {
        let e = Envelope::ok("ServiceList", "localhost", json!({"units":[]}));
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            json!({
                "apiVersion":"fez/v1","kind":"ServiceList","host":"localhost",
                "status":"ok","data":{"units":[]}
            })
        );
    }

    #[test]
    fn error_envelope_shape() {
        let e = Envelope::error(
            "Error",
            "h1",
            ApiError {
                code: "not-found".into(),
                message: "no unit".into(),
                detail: None,
            },
        );
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            json!({
                "apiVersion":"fez/v1","kind":"Error","host":"h1",
                "status":"error","error":{"code":"not-found","message":"no unit"}
            })
        );
    }

    #[test]
    fn ok_envelope_with_hints() {
        let e = Envelope::ok(
            "ServiceMutation",
            "localhost",
            json!({"unit": "nginx.service"}),
        )
        .with_hints(json!({"reverse": "fez services start nginx.service"}));
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            json!({
                "apiVersion":"fez/v1","kind":"ServiceMutation","host":"localhost",
                "status":"ok","data":{"unit":"nginx.service"},
                "hints":{"reverse":"fez services start nginx.service"}
            })
        );
    }
}
