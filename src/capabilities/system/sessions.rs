//! Logind session, user, inhibitor, and boot-entry reads.
//!
//! All reads are unprivileged — logind does not gate `List*` methods.

use crate::capabilities::View;
use crate::error::{FezError, Result};
use crate::protocol::client::{variant_value, BridgeClient};
use serde_json::{json, Value};

const LOGIN1_NAME: &str = "org.freedesktop.login1";
const LOGIN1_PATH: &str = "/org/freedesktop/login1";
const LOGIN1_IFACE: &str = "org.freedesktop.login1.Manager";

const PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";
const SESSION_IFACE: &str = "org.freedesktop.login1.Session";

/// List active sessions with per-session detail from GetAll.
pub(super) fn list(client: &mut BridgeClient, host: &str) -> Result<View> {
    let channel = client.dbus_open(LOGIN1_NAME)?;
    let out = client.dbus_call(
        &channel,
        LOGIN1_PATH,
        LOGIN1_IFACE,
        "ListSessions",
        json!([]),
    )?;
    let raw = out
        .get(0)
        .and_then(Value::as_array)
        .ok_or_else(|| FezError::Problem("ListSessions returned no array".into()))?;

    let mut sessions = Vec::new();
    for entry in raw {
        let arr = entry
            .as_array()
            .ok_or_else(|| FezError::Problem("session entry is not an array".into()))?;
        let obj_path = arr.get(4).and_then(Value::as_str).unwrap_or("");
        if obj_path.is_empty() {
            continue;
        }
        // Get full session properties
        let props = client.dbus_call(
            &channel,
            obj_path,
            PROPS_IFACE,
            "GetAll",
            json!([SESSION_IFACE]),
        )?;
        let p = props.get(0).unwrap_or(&Value::Null);
        let vv = |key: &str| variant_value(p.get(key).unwrap_or(&Value::Null));

        sessions.push(json!({
            "id": vv("Id").as_str().unwrap_or(""),
            "user": vv("Name").as_str().unwrap_or(""),
            "uid": vv("User").as_array().and_then(|a| a.first()).and_then(Value::as_u64).unwrap_or(0),
            "type": vv("Type").as_str().unwrap_or(""),
            "remote": vv("Remote").as_bool().unwrap_or(false),
            "remote_host": vv("RemoteHost").as_str().unwrap_or(""),
            "state": vv("State").as_str().unwrap_or(""),
            "service": vv("Service").as_str().unwrap_or(""),
            "class": vv("Class").as_str().unwrap_or(""),
        }));
    }

    let data = json!({"sessions": sessions});
    let human = render_sessions(&sessions);
    Ok(View::new("SessionList", host, data, human))
}

fn render_sessions(sessions: &[Value]) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "{:<6} {:<10} {:<6} {:<20} {:<8}\n",
        "ID", "USER", "TYPE", "REMOTE", "STATE"
    ));
    for sess in sessions {
        let remote_display = if sess["remote"].as_bool() == Some(true) {
            sess["remote_host"].as_str().unwrap_or("yes")
        } else {
            "-"
        };
        s.push_str(&format!(
            "{:<6} {:<10} {:<6} {:<20} {:<8}\n",
            sess["id"].as_str().unwrap_or(""),
            sess["user"].as_str().unwrap_or(""),
            sess["type"].as_str().unwrap_or(""),
            remote_display,
            sess["state"].as_str().unwrap_or(""),
        ));
    }
    s
}

/// List logged-in users.
pub(super) fn users(client: &mut BridgeClient, host: &str) -> Result<View> {
    let channel = client.dbus_open(LOGIN1_NAME)?;
    let out = client.dbus_call(&channel, LOGIN1_PATH, LOGIN1_IFACE, "ListUsers", json!([]))?;
    let raw = out
        .get(0)
        .and_then(Value::as_array)
        .ok_or_else(|| FezError::Problem("ListUsers returned no array".into()))?;

    let users: Vec<Value> = raw
        .iter()
        .filter_map(|entry| {
            let arr = entry.as_array()?;
            Some(json!({
                "uid": arr.first()?.as_u64()?,
                "user": arr.get(1)?.as_str()?,
            }))
        })
        .collect();

    let data = json!({"users": users});
    let human = render_users(&users);
    Ok(View::new("UserList", host, data, human))
}

fn render_users(users: &[Value]) -> String {
    let mut s = String::new();
    s.push_str(&format!("{:<8} {}\n", "UID", "USER"));
    for u in users {
        s.push_str(&format!(
            "{:<8} {}\n",
            u["uid"].as_u64().unwrap_or(0),
            u["user"].as_str().unwrap_or(""),
        ));
    }
    s
}

/// List shutdown/sleep inhibitors.
pub(super) fn inhibitors(client: &mut BridgeClient, host: &str) -> Result<View> {
    let channel = client.dbus_open(LOGIN1_NAME)?;
    let out = client.dbus_call(
        &channel,
        LOGIN1_PATH,
        LOGIN1_IFACE,
        "ListInhibitors",
        json!([]),
    )?;
    let raw = out
        .get(0)
        .and_then(Value::as_array)
        .ok_or_else(|| FezError::Problem("ListInhibitors returned no array".into()))?;

    let inhibitors: Vec<Value> = raw
        .iter()
        .filter_map(|entry| {
            let arr = entry.as_array()?;
            Some(json!({
                "what": arr.first()?.as_str()?,
                "who": arr.get(1)?.as_str()?,
                "why": arr.get(2)?.as_str()?,
                "mode": arr.get(3)?.as_str()?,
                "uid": arr.get(4)?.as_u64()?,
                "pid": arr.get(5)?.as_u64()?,
            }))
        })
        .collect();

    let data = json!({"inhibitors": inhibitors});
    let human = render_inhibitors(&inhibitors);
    Ok(View::new("InhibitorList", host, data, human))
}

fn render_inhibitors(inhibitors: &[Value]) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "{:<12} {:<20} {:<40} {}\n",
        "WHAT", "WHO", "WHY", "MODE"
    ));
    for inh in inhibitors {
        s.push_str(&format!(
            "{:<12} {:<20} {:<40} {}\n",
            inh["what"].as_str().unwrap_or(""),
            inh["who"].as_str().unwrap_or(""),
            inh["why"].as_str().unwrap_or(""),
            inh["mode"].as_str().unwrap_or(""),
        ));
    }
    s
}

/// List boot loader entries.
pub(super) fn boot_entries(client: &mut BridgeClient, host: &str) -> Result<View> {
    let channel = client.dbus_open(LOGIN1_NAME)?;
    let out = client.dbus_call(
        &channel,
        LOGIN1_PATH,
        PROPS_IFACE,
        "Get",
        json!([LOGIN1_IFACE, "BootLoaderEntries"]),
    )?;
    let prop = out.get(0).unwrap_or(&Value::Null);
    let entries: Vec<String> = variant_value(prop)
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();

    let data = json!({"entries": entries});
    let human = entries.join("\n") + if entries.is_empty() { "" } else { "\n" };
    Ok(View::new("BootEntryList", host, data, human))
}
