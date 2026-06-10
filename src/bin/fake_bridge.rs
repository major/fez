//! Minimal cockpit-bridge stand-in for fez integration tests.
use fez::protocol::frame::{read_frame, write_frame, Frame};
use serde_json::{json, Value};
use std::io::{self, Write};

/// Fixed dnf5daemon session path the fake hands back from `open_session`.
///
/// Real dnf5daemon returns a dynamic, server-allocated path; the fake pins a
/// constant. Production code reads the returned path from `open_session`'s
/// reply and reuses it for subsequent calls, so it works against either.
const SESSION_PATH: &str = "/org/rpm/dnf/v0/session/fake";

/// Build a dnf5daemon package/repo `a{sv}` attribute map, mirroring the
/// systemd `GetAll` variant-wrapping (`{"t":<sig>,"v":<value>}`) so the dnf
/// reply value representation matches the rest of the fake exactly.
fn dnf_package(name: &str, evr: &str, arch: &str, repo_id: &str, install_size: u64) -> Value {
    json!({
        "name":         {"t":"s","v":name},
        "evr":          {"t":"s","v":evr},
        "arch":         {"t":"s","v":arch},
        "repo_id":      {"t":"s","v":repo_id},
        "install_size": {"t":"t","v":install_size},
        "summary":      {"t":"s","v":format!("{name} package")},
    })
}

/// Build a dnf5daemon repository `a{sv}` attribute map.
fn dnf_repo(id: &str, name: &str, enabled: bool) -> Value {
    json!({
        "id":      {"t":"s","v":id},
        "name":    {"t":"s","v":name},
        "enabled": {"t":"b","v":enabled},
    })
}

/// Build the `Goal.resolve` `transaction_items` array, keyed by the
/// `FEZ_FAKE_PLAN` env var so one fake serves every guardrail test case.
///
/// Each item is the tuple `(object_type, action, reason, item_attrs, object)`
/// where `action` is a string (`"Install"`/`"Remove"`) and `object` is the
/// package `a{sv}` map. The Task 5 parser keys off `action` (index 1) and
/// `object` (index 4).
fn fake_resolve_items() -> Value {
    fn installed(name: &str) -> Value {
        json!([
            "Package",
            "Install",
            "User",
            {},
            dnf_package(name, "1.0-1.fc40", "x86_64", "fedora", 1024),
        ])
    }
    fn removed(name: &str) -> Value {
        json!([
            "Package",
            "Remove",
            "Dependency",
            {},
            dnf_package(name, "1.0-1.fc40", "x86_64", "@System", 1024),
        ])
    }
    match std::env::var("FEZ_FAKE_PLAN").as_deref() {
        Ok("protected") => json!([removed("glibc")]),
        Ok("cascade") => {
            // 21 = CASCADE_LIMIT (20) + 1, to trip the cascade guardrail.
            let items: Vec<Value> = (0..21).map(|i| removed(&format!("pkg{i}"))).collect();
            Value::Array(items)
        }
        // FEZ_FAKE_PLAN unset (the Err(_) case, the common path in tests)
        // defaults to an install plan.
        Ok("install") | Err(_) => json!([installed("htop")]),
        // Any unrecognized plan (including the documented "small" case) yields
        // a single non-protected removal (guardrails pass).
        Ok(_) => json!([removed("htop")]),
    }
}

fn send_control(out: &mut impl Write, v: &Value) {
    let mut payload = serde_json::to_vec(v).unwrap();
    payload.push(b'\n');
    write_frame(
        out,
        &Frame {
            channel: String::new(),
            payload,
        },
    )
    .unwrap();
}

fn send_data(out: &mut impl Write, channel: &str, v: &Value) {
    let mut payload = serde_json::to_vec(v).unwrap();
    payload.push(b'\n');
    write_frame(out, &Frame::new(channel, payload)).unwrap();
}

fn main() -> io::Result<()> {
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();

    send_control(&mut stdout, &json!({"command":"init","version":1}));

    while let Some(frame) = read_frame(&mut stdin)? {
        if frame.channel.is_empty() {
            let ctrl: Value = serde_json::from_slice(&frame.payload).unwrap_or(Value::Null);
            // init, close, done: ignore; only `open` needs a response.
            if let Some("open") = ctrl.get("command").and_then(Value::as_str) {
                let channel = ctrl
                    .get("channel")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let payload = ctrl.get("payload").and_then(Value::as_str).unwrap_or("");
                send_control(&mut stdout, &json!({"command":"ready","channel":channel}));
                if payload == "stream" {
                    let mut blob = serde_json::to_vec(&json!({
                        "__REALTIME_TIMESTAMP":"1700000000000000","PRIORITY":"6",
                        "SYSLOG_IDENTIFIER":"sshd","MESSAGE":"Server listening on port 22.","_PID":"1001"
                    })).unwrap();
                    blob.push(b'\n');
                    blob.extend_from_slice(
                        &serde_json::to_vec(&json!({
                            "__REALTIME_TIMESTAMP":"1700000001000000","PRIORITY":"6",
                            "SYSLOG_IDENTIFIER":"sshd","MESSAGE":"Accepted publickey for fedora","_PID":"1002"
                        }))
                        .unwrap(),
                    );
                    blob.push(b'\n');
                    write_frame(&mut stdout, &Frame::new(&channel, blob))?;
                    send_control(&mut stdout, &json!({"command":"done","channel":channel}));
                    send_control(&mut stdout, &json!({"command":"close","channel":channel}));
                }
            }
        } else {
            let msg: Value = serde_json::from_slice(&frame.payload).unwrap_or(Value::Null);
            if let Some(call) = msg.get("call").and_then(Value::as_array) {
                let id = msg.get("id").cloned().unwrap_or(json!("0"));
                // call = [path, interface, method, args]. The interface is
                // needed to disambiguate dnf5daemon's two `list` methods
                // (rpm.Rpm.list vs rpm.Repo.list) which share a method name.
                let iface = call.get(1).and_then(Value::as_str).unwrap_or("");
                let method = call.get(2).and_then(Value::as_str).unwrap_or("");
                let reply = match method {
                    // reply[0][0] = units array
                    "ListUnits" => json!({"reply":[[[
                        ["sshd.service","OpenSSH server daemon","loaded","active","running","",
                         "/org/freedesktop/systemd1/unit/sshd_2eservice",0,"","/"],
                        ["chronyd.service","NTP client/server","loaded","inactive","dead","",
                         "/org/freedesktop/systemd1/unit/chronyd_2eservice",0,"","/"]
                    ]]],"id":id}),
                    // reply[0][0] = object path
                    "GetUnit" | "LoadUnit" => {
                        json!({"reply":[["/org/freedesktop/systemd1/unit/sshd_2eservice"]],"id":id})
                    }
                    // reply[0][0] = a{sv} dict. Real cockpit-bridge wraps each
                    // value as a D-Bus variant: {"t":"s","v":"..."}. Mirror that
                    // so the status path is exercised exactly as in production.
                    "GetAll" => json!({"reply":[[{
                        "Id":{"t":"s","v":"sshd.service"},
                        "Description":{"t":"s","v":"OpenSSH server daemon"},
                        "LoadState":{"t":"s","v":"loaded"},
                        "ActiveState":{"t":"s","v":"active"},
                        "SubState":{"t":"s","v":"running"},
                        "UnitFileState":{"t":"s","v":"enabled"}
                    }]],"id":id}),
                    // Lifecycle methods return a job object path: reply[0][0].
                    "StartUnit" | "StopUnit" | "RestartUnit" | "ReloadUnit" => {
                        json!({"reply":[["/org/freedesktop/systemd1/job/42"]],"id":id})
                    }
                    // Manager.Reload returns void; fez calls it after
                    // enable/disable to refresh cached unit-file state.
                    "Reload" => json!({"reply":[[]],"id":id}),
                    // EnableUnitFiles returns two out args: carries_install_info (bool)
                    // and a changes array. out_args = reply[0] = [true, [changes]].
                    "EnableUnitFiles" => json!({"reply":[[
                        true,
                        [["symlink",
                          "/etc/systemd/system/multi-user.target.wants/chronyd.service",
                          "/usr/lib/systemd/system/chronyd.service"]]
                    ]],"id":id}),
                    // DisableUnitFiles returns one out arg: a changes array.
                    // out_args = reply[0] = [[changes]].
                    "DisableUnitFiles" => json!({"reply":[[
                        [["unlink",
                          "/etc/systemd/system/multi-user.target.wants/chronyd.service",
                          ""]]
                    ]],"id":id}),
                    // ---- org.rpm.dnf.v0 (dnf5daemon) ----
                    // SessionManager.open_session -> (session_object_path).
                    // FEZ_FAKE_NO_DNF5 simulates the daemon being absent: the
                    // bus name fails to activate, yielding ServiceUnknown.
                    "open_session" => {
                        if std::env::var_os("FEZ_FAKE_NO_DNF5").is_some() {
                            json!({"error":[
                                "org.freedesktop.DBus.Error.ServiceUnknown",
                                ["The name org.rpm.dnf.v0 was not provided by any .service files"]
                            ],"id":id})
                        } else {
                            json!({"reply":[[SESSION_PATH]],"id":id})
                        }
                    }
                    // SessionManager.close_session(path) -> (bool).
                    "close_session" => json!({"reply":[[true]],"id":id}),
                    // rpm.Repo.list(options) -> (repositories). Shares the
                    // method name `list` with Rpm.list; disambiguated by iface.
                    "list" if iface.ends_with(".rpm.Repo") => json!({"reply":[[[
                        dnf_repo("fedora", "Fedora", true),
                        dnf_repo("updates-testing", "Fedora - Testing", false),
                    ]]],"id":id}),
                    // rpm.Rpm.list(options) -> (packages).
                    "list" => json!({"reply":[[[
                        dnf_package("bash", "5.2.26-1.fc40", "x86_64", "fedora", 7340032),
                        dnf_package("htop", "3.3.0-1.fc40", "x86_64", "fedora", 245760),
                        dnf_package("nginx", "1.24.0-7.fc40", "x86_64", "fedora", 1572864),
                    ]]],"id":id}),
                    // Staging calls: install/remove/upgrade return nothing.
                    "install" | "remove" | "upgrade" => json!({"reply":[[]],"id":id}),
                    // Goal.resolve(options) -> (transaction_items, result).
                    // result 0 == no problems.
                    "resolve" => json!({"reply":[[fake_resolve_items(), 0]],"id":id}),
                    // Goal.do_transaction(options) -> ().
                    "do_transaction" => json!({"reply":[[]],"id":id}),
                    other => json!({"error":[
                        "org.freedesktop.DBus.Error.UnknownMethod",
                        [format!("no fake for {other}")]],"id":id}),
                };
                send_data(&mut stdout, &frame.channel, &reply);
            }
        }
    }
    Ok(())
}
