//! Canned systemd and cockpit.Superuser replies.

use super::{err_reply, ok_reply};
use serde_json::{json, Value};

/// systemd1 root object path.
const ROOT_PATH: &str = "/org/freedesktop/systemd1";

/// Reply to a systemd / cockpit.Superuser / misc D-Bus method call.
///
/// This is the catch-all dispatcher for methods not handled by the service-
/// specific modules (NM, firewalld, PK, dnf5daemon). It covers:
/// - `cockpit.Superuser.Bridges` property read (`Get`)
/// - `cockpit.Superuser.Start` (escalation mechanism)
/// - systemd1 Manager methods (`ListUnits`, `GetUnit`, `LoadUnit`, `GetAll`,
///   lifecycle verbs, `Reload`, `EnableUnitFiles`, `DisableUnitFiles`)
/// - dnf5daemon `close_session` (not an `a{sv}` method, so it falls through
///   to here instead of the dnf module)
pub(super) fn systemd_reply(
    method: &str,
    args: &[Value],
    bridges: &[(String, bool)],
    escalated: &mut bool,
    id: &Value,
) -> Value {
    match method {
        "Get" => {
            let names: Vec<Value> = bridges.iter().map(|(n, _)| json!(n)).collect();
            ok_reply(id, json!([{"t":"as","v":names}]))
        }
        "Start" => {
            let name = args.first().and_then(Value::as_str).unwrap_or("");
            match bridges.iter().find(|(n, _)| n == name) {
                Some((_, true)) => {
                    *escalated = true;
                    ok_reply(id, json!([]))
                }
                _ => err_reply(id, "cockpit.Superuser.Error", format!("mechanism {name:?} cannot start")),
            }
        }
        "ListUnits" => ok_reply(id, json!([[
            ["sshd.service","OpenSSH server daemon","loaded","active","running","",
             "/org/freedesktop/systemd1/unit/sshd_2eservice",0,"",ROOT_PATH],
            ["chronyd.service","NTP client/server","loaded","inactive","dead","",
             "/org/freedesktop/systemd1/unit/chronyd_2eservice",0,"",ROOT_PATH],
        ]])),
        "GetUnit" | "LoadUnit" => ok_reply(id, json!(["/org/freedesktop/systemd1/unit/sshd_2eservice"])),
        "GetAll" => ok_reply(id, json!([{
            "Id":{"t":"s","v":"sshd.service"},
            "Description":{"t":"s","v":"OpenSSH server daemon"},
            "LoadState":{"t":"s","v":"loaded"},
            "ActiveState":{"t":"s","v":"active"},
            "SubState":{"t":"s","v":"running"},
            "UnitFileState":{"t":"s","v":"enabled"}
        }])),
        "StartUnit" | "StopUnit" | "RestartUnit" | "ReloadUnit" => {
            ok_reply(id, json!(["/org/freedesktop/systemd1/job/42"]))
        }
        "Reload" => ok_reply(id, json!([])),
        "EnableUnitFiles" => ok_reply(id, json!([
            true,
            [["symlink",
              "/etc/systemd/system/multi-user.target.wants/chronyd.service",
              "/usr/lib/systemd/system/chronyd.service"]],
        ])),
        "DisableUnitFiles" => ok_reply(id, json!([
            [["unlink",
              "/etc/systemd/system/multi-user.target.wants/chronyd.service",
              ""]],
        ])),
        "close_session" => ok_reply(id, json!([true])),
        other => err_reply(id, "org.freedesktop.DBus.Error.UnknownMethod", format!("no fake for {other}")),
    }
}
