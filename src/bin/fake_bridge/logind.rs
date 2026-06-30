//! Canned logind (login1) replies for session/user/inhibitor tests.

use super::{err_reply, ok_reply};
use serde_json::{json, Value};

pub(super) const LOGIN1_PATH: &str = "/org/freedesktop/login1";
const LOGIN1_IFACE: &str = "org.freedesktop.login1.Manager";

const SESSION_42_PATH: &str = "/org/freedesktop/login1/session/_342";
const SESSION_7_PATH: &str = "/org/freedesktop/login1/session/_37";

/// Canned reply for a call against login1.
pub(super) fn logind_reply(
    path: &str,
    iface: &str,
    method: &str,
    _args: &[Value],
    id: &Value,
) -> Value {
    match (path, iface, method) {
        (LOGIN1_PATH, LOGIN1_IFACE, "ListSessions") => list_sessions(id),
        (LOGIN1_PATH, LOGIN1_IFACE, "ListUsers") => list_users(id),
        (LOGIN1_PATH, LOGIN1_IFACE, "ListInhibitors") => list_inhibitors(id),
        (LOGIN1_PATH, "org.freedesktop.DBus.Properties", "Get") => boot_entries(id, _args),
        (SESSION_42_PATH, "org.freedesktop.DBus.Properties", "GetAll") => session_42_props(id),
        (SESSION_7_PATH, "org.freedesktop.DBus.Properties", "GetAll") => session_7_props(id),
        _ => err_reply(
            id,
            "org.freedesktop.DBus.Error.UnknownMethod",
            format!("no logind fake for {path} {iface} {method}"),
        ),
    }
}

fn list_sessions(id: &Value) -> Value {
    // a(susso): session_id, uid, username, seat, object_path
    ok_reply(
        id,
        json!([[
            ["42", 1000, "major", "", SESSION_42_PATH],
            ["7", 0, "root", "seat0", SESSION_7_PATH],
        ]]),
    )
}

fn list_users(id: &Value) -> Value {
    // a(uso): uid, username, object_path
    ok_reply(
        id,
        json!([[
            [1000, "major", "/org/freedesktop/login1/user/_1000"],
            [0, "root", "/org/freedesktop/login1/user/_30"],
        ]]),
    )
}

fn list_inhibitors(id: &Value) -> Value {
    // a(ssssuu): what, who, why, mode, uid, pid
    ok_reply(
        id,
        json!([[[
            "sleep",
            "NetworkManager",
            "NetworkManager needs to turn off networks",
            "delay",
            0,
            1043
        ],]]),
    )
}

fn boot_entries(id: &Value, args: &[Value]) -> Value {
    let prop = args.get(1).and_then(Value::as_str).unwrap_or("");
    if prop == "BootLoaderEntries" {
        ok_reply(
            id,
            json!([{"t":"as","v":[
                "abc-6.12.1-200.fc41.x86_64.conf",
                "abc-6.11.8-100.fc41.x86_64.conf",
            ]}]),
        )
    } else {
        err_reply(
            id,
            "org.freedesktop.DBus.Error.UnknownProperty",
            format!("no fake for property {prop}"),
        )
    }
}

fn session_42_props(id: &Value) -> Value {
    ok_reply(
        id,
        json!([{
            "Id": {"t":"s","v":"42"},
            "Name": {"t":"s","v":"major"},
            "Type": {"t":"s","v":"tty"},
            "Remote": {"t":"b","v":true},
            "RemoteHost": {"t":"s","v":"192.168.1.100"},
            "State": {"t":"s","v":"active"},
            "Service": {"t":"s","v":"sshd"},
            "Class": {"t":"s","v":"user"},
            "User": {"t":"(uo)","v":[1000,"/org/freedesktop/login1/user/_1000"]},
        }]),
    )
}

fn session_7_props(id: &Value) -> Value {
    ok_reply(
        id,
        json!([{
            "Id": {"t":"s","v":"7"},
            "Name": {"t":"s","v":"root"},
            "Type": {"t":"s","v":"tty"},
            "Remote": {"t":"b","v":false},
            "RemoteHost": {"t":"s","v":""},
            "State": {"t":"s","v":"active"},
            "Service": {"t":"s","v":"login"},
            "Class": {"t":"s","v":"user"},
            "User": {"t":"(uo)","v":[0,"/org/freedesktop/login1/user/_30"]},
        }]),
    )
}
