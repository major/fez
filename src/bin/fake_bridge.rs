//! Minimal cockpit-bridge stand-in for fez integration tests.
use fez::protocol::frame::{read_frame, write_frame, Frame};
use serde_json::{json, Value};
use std::io::{self, Write};

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
