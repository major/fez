//! Control and D-Bus message types exchanged with the bridge.
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Control messages we SEND (empty-channel JSON, tagged by `command`).
#[derive(Debug, Serialize)]
#[serde(tag = "command", rename_all = "kebab-case")]
pub enum Control {
    /// Initial handshake message.
    Init {
        /// Protocol version.
        version: u32,
        /// Host label.
        host: String,
        /// Eagerly start a privileged superuser peer at init time. cockpit
        /// only routes `superuser: "require"` channels once such a peer exists;
        /// without this the bridge denies every privileged channel
        /// (`access-denied`). `{"id": "sudo"}` selects the sudo escalation
        /// bridge, which works headlessly when the invoking user has
        /// passwordless sudo.
        #[serde(skip_serializing_if = "Option::is_none")]
        superuser: Option<Value>,
    },
    /// Open a new channel.
    Open {
        /// Channel id to allocate.
        channel: String,
        /// Channel payload type (e.g. `dbus-json3`, `stream`).
        payload: String,
        /// Additional open options (bus, name, spawn, ...).
        #[serde(flatten)]
        options: Map<String, Value>,
    },
    /// Signal end of input on a channel.
    Done {
        /// Channel being finished.
        channel: String,
    },
    /// Close a channel, optionally reporting a problem.
    Close {
        /// Channel to close.
        channel: String,
        /// Problem kind if the close is abnormal.
        #[serde(skip_serializing_if = "Option::is_none")]
        problem: Option<String>,
    },
}

impl Control {
    /// Build an `Open` control for `channel` with the given payload type.
    pub fn open(channel: &str, payload: &str) -> Control {
        Control::Open {
            channel: channel.into(),
            payload: payload.into(),
            options: Map::new(),
        }
    }
    /// Builder helper for Open options (bus, name, spawn, ...).
    pub fn opt(mut self, key: &str, value: Value) -> Control {
        if let Control::Open {
            ref mut options, ..
        } = self
        {
            options.insert(key.to_string(), value);
        }
        self
    }
    /// Serialize this control message to JSON bytes.
    ///
    /// Returns a safe-close frame on serialization failure so the bridge sees a
    /// well-formed command rather than receiving nothing.
    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_else(|_| {
            serde_json::json!({"command":"close","channel":"","problem":"internal-error"})
                .to_string()
                .into_bytes()
        })
    }
}

/// A dbus method call plus the channel it rides on.
#[derive(Debug)]
pub struct DbusCall {
    /// Channel the call is sent on.
    pub channel: String,
    /// Per-call cookie correlating the response.
    pub id: String,
    /// The serializable call body.
    pub body: DbusCallBody,
}

/// The wire body of a D-Bus call: `(path, interface, method, args)` plus an id.
#[derive(Debug, Serialize)]
pub struct DbusCallBody {
    /// Tuple of object path, interface, method name, and argument array.
    pub call: (String, String, String, Value),
    /// Correlation id echoed in the response.
    pub id: String,
}

impl DbusCall {
    /// Build a call to `method` on `iface` at `path` with `args`, assigning a
    /// fresh correlation cookie.
    pub fn new(channel: &str, path: &str, iface: &str, method: &str, args: Value) -> DbusCall {
        // A per-call cookie. Monotonic-enough for one short-lived connection.
        let id = format!("{}", next_cookie());
        DbusCall {
            channel: channel.into(),
            id: id.clone(),
            body: DbusCallBody {
                call: (path.into(), iface.into(), method.into(), args),
                id,
            },
        }
    }
    /// Serialize the call body to JSON bytes.
    ///
    /// Returns a valid no-op DbusCallBody on serialization failure so the
    /// bridge receives well-formed JSON instead of malformed bytes.
    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec(&self.body).unwrap_or_else(|_| {
            serde_json::json!({
                "call": ["", "", "", []],
                "id": "serialize-error"
            })
            .to_string()
            .into_bytes()
        })
    }
}

fn next_cookie() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(1);
    N.fetch_add(1, Ordering::Relaxed)
}

/// A dbus response (reply or error) parsed from a data frame.
#[derive(Debug, Deserialize)]
pub struct DbusResponse {
    /// Reply arguments when the call succeeded.
    #[serde(default)]
    pub reply: Option<Vec<Value>>,
    /// Error tuple when the call failed.
    #[serde(default)]
    pub error: Option<Vec<Value>>,
    /// Correlation id matching the originating call.
    #[serde(default)]
    pub id: Option<String>,
}

impl DbusResponse {
    /// The out-argument array (`reply[0]`), if present.
    pub fn out_args(&self) -> Option<&Value> {
        self.reply.as_ref().and_then(|r| r.first())
    }
    /// The D-Bus error name, if this response is an error.
    pub fn dbus_error_name(&self) -> Option<&str> {
        self.error
            .as_ref()
            .and_then(|e| e.first())
            .and_then(|v| v.as_str())
    }
    /// The human-readable D-Bus error message, if present.
    pub fn dbus_error_message(&self) -> Option<String> {
        self.error
            .as_ref()
            .and_then(|e| e.get(1))
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }
}

/// Control messages we RECEIVE (permissive).
#[derive(Debug, Deserialize)]
pub struct IncomingControl {
    /// The control command name (e.g. `close`, `done`, `init`).
    pub command: String,
    /// Channel the command refers to, if any.
    #[serde(default)]
    pub channel: Option<String>,
    /// Problem kind when the command reports a failure.
    #[serde(default)]
    pub problem: Option<String>,
    /// Authorization challenge string on an `authorize` control command
    /// (e.g. a sudo password prompt the bridge wants the client to answer).
    #[serde(default)]
    pub challenge: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serializes_init_without_superuser() {
        let v = serde_json::to_value(Control::Init {
            version: 1,
            host: "localhost".into(),
            superuser: None,
        })
        .unwrap();
        // Omitted entirely when None so we never imply a privileged session.
        assert_eq!(v, json!({"command":"init","version":1,"host":"localhost"}));
    }

    #[test]
    fn serializes_init_with_superuser() {
        let v = serde_json::to_value(Control::Init {
            version: 1,
            host: "localhost".into(),
            superuser: Some(json!({"id": "sudo"})),
        })
        .unwrap();
        assert_eq!(
            v,
            json!({
                "command":"init","version":1,"host":"localhost",
                "superuser":{"id":"sudo"}
            })
        );
    }

    #[test]
    fn serializes_open_with_options() {
        let open = Control::open("ch1", "dbus-json3")
            .opt("bus", json!("system"))
            .opt("name", json!("org.freedesktop.systemd1"));
        let v = serde_json::to_value(open).unwrap();
        assert_eq!(
            v,
            json!({
                "command":"open","channel":"ch1","payload":"dbus-json3",
                "bus":"system","name":"org.freedesktop.systemd1"
            })
        );
    }

    #[test]
    fn serializes_internal_bus_open() {
        // The internal-bus open (used to reach cockpit.Superuser) carries
        // bus:"internal", no name, and is never privileged.
        let open = Control::open("ch9", "dbus-json3").opt("bus", json!("internal"));
        let v = serde_json::to_value(open).unwrap();
        assert_eq!(
            v,
            json!({
                "command":"open","channel":"ch9","payload":"dbus-json3",
                "bus":"internal"
            })
        );
        let obj = v.as_object().unwrap();
        assert!(!obj.contains_key("name"));
        assert!(!obj.contains_key("superuser"));
    }

    #[test]
    fn serializes_dbus_call() {
        let call = DbusCall::new(
            "ch1",
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
            "ListUnits",
            json!([]),
        );
        let v = serde_json::to_value(&call.body).unwrap();
        assert_eq!(
            v,
            json!({
                "call":["/org/freedesktop/systemd1","org.freedesktop.systemd1.Manager","ListUnits",[]],
                "id": call.id
            })
        );
    }

    #[test]
    fn parses_dbus_reply() {
        let msg: DbusResponse = serde_json::from_value(json!({
            "reply":[[ [["sshd.service","OpenSSH","loaded","active","running"]] ]],
            "id":"7"
        }))
        .unwrap();
        assert_eq!(msg.id.as_deref(), Some("7"));
        assert!(msg.error.is_none());
        assert!(msg.reply.is_some());
    }

    #[test]
    fn parses_dbus_error() {
        let msg: DbusResponse = serde_json::from_value(json!({
            "error":["org.freedesktop.DBus.Error.UnknownMethod",["nope"]],
            "id":"7"
        }))
        .unwrap();
        assert_eq!(
            msg.dbus_error_name(),
            Some("org.freedesktop.DBus.Error.UnknownMethod")
        );
    }

    #[test]
    fn parses_incoming_control() {
        let c: IncomingControl = serde_json::from_value(json!({
            "command":"close","channel":"ch1","problem":"not-found"
        }))
        .unwrap();
        assert_eq!(c.command, "close");
        assert_eq!(c.channel.as_deref(), Some("ch1"));
        assert_eq!(c.problem.as_deref(), Some("not-found"));
    }

    #[test]
    fn serializes_done_control() {
        let v = serde_json::to_value(Control::Done {
            channel: "ch1".into(),
        })
        .unwrap();
        assert_eq!(v, json!({"command":"done","channel":"ch1"}));
    }

    #[test]
    fn serializes_close_control_with_and_without_problem() {
        let with = serde_json::to_value(Control::Close {
            channel: "ch1".into(),
            problem: Some("not-found".into()),
        })
        .unwrap();
        assert_eq!(
            with,
            json!({"command":"close","channel":"ch1","problem":"not-found"})
        );

        let without = serde_json::to_value(Control::Close {
            channel: "ch2".into(),
            problem: None,
        })
        .unwrap();
        assert_eq!(without, json!({"command":"close","channel":"ch2"}));
    }

    #[test]
    fn control_to_json_round_trips() {
        let bytes = Control::Done {
            channel: "ch1".into(),
        }
        .to_json();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v, json!({"command":"done","channel":"ch1"}));
    }

    #[test]
    fn dbus_response_out_args_none_when_empty() {
        let resp: DbusResponse = serde_json::from_value(json!({"id":"1"})).unwrap();
        assert!(resp.out_args().is_none());
        assert!(resp.dbus_error_name().is_none());
        assert!(resp.dbus_error_message().is_none());
    }
}
