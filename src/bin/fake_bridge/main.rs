//! Minimal cockpit-bridge stand-in for fez integration tests.

mod dnf;
mod fw;
mod hosttime;
mod nm;
mod pk;
mod resolve;
mod systemd;
mod udisks;

use fez::protocol::frame::{read_frame, write_frame, Frame};
use serde_json::{json, Value};
use std::io::{self, Write};

/// D-Bus null/empty object-path sentinel used when a device has no config object.
const ROOT_PATH: &str = "/";

/// Build a D-Bus method-reply frame: `{"reply":[out_args],"id":id}`.
///
/// `out_args` is the out-argument array; pass `json!([value])` for a single
/// return value and `json!([])` for void methods.
fn ok_reply(id: &Value, out_args: Value) -> Value {
    json!({"reply": [out_args], "id": id})
}

/// Build a D-Bus error-reply frame: `{"error":[name,[msg]],"id":id}`.
fn err_reply(id: &Value, name: &str, msg: String) -> Value {
    json!({"error": [name, [msg]], "id": id})
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

/// The host's escalation mechanisms as modeled by `FEZ_FAKE_BRIDGES`.
///
/// Grammar: comma-separated `name:outcome` pairs, outcome `ok` or `err`, e.g.
/// `sudo:ok`, `sudo:err,polkit:ok`. Default (unset) models a normal
/// passwordless-sudo host (`[("sudo", true)]`).
fn fake_bridges() -> Vec<(String, bool)> {
    let raw = match std::env::var("FEZ_FAKE_BRIDGES") {
        Ok(v) => v,
        Err(_) => return vec![("sudo".to_string(), true)],
    };
    raw.split(',')
        .filter(|s| !s.is_empty())
        .map(|entry| {
            let (name, outcome) = entry.split_once(':').unwrap_or((entry, "ok"));
            (name.to_string(), outcome == "ok")
        })
        .collect()
}

fn main() -> io::Result<()> {
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();

    if let Some(bytes) = std::env::var("FEZ_FAKE_STDERR_BYTES")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
    {
        let chunk = vec![b'x'; bytes];
        io::stderr().lock().write_all(&chunk)?;
    }

    let bridges = fake_bridges();
    let mut escalated = false;
    let mut privileged_channels: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    send_control(&mut stdout, &json!({"command":"init","version":1}));

    while let Some(frame) = read_frame(&mut stdin)? {
        if frame.channel.is_empty() {
            let ctrl: Value = serde_json::from_slice(&frame.payload).unwrap_or(Value::Null);
            let command = ctrl.get("command").and_then(Value::as_str);
            if let Some("init") = command {
                let requests_escalation = match ctrl.get("superuser") {
                    None | Some(Value::Null) => false,
                    Some(Value::String(s)) => s != "none",
                    Some(_) => true,
                };
                if requests_escalation {
                    send_control(&mut stdout, &json!({"command":"superuser-init-done"}));
                }
                continue;
            }
            if let Some("open") = command {
                handle_open(&ctrl, &mut stdout, &mut escalated, &mut privileged_channels)?;
            }
        } else {
            handle_data(
                &frame,
                &mut stdout,
                &bridges,
                &mut escalated,
                &privileged_channels,
            );
        }
    }
    Ok(())
}

/// Handle a control `open` command: channel setup, escalation, streaming.
fn handle_open(
    ctrl: &Value,
    stdout: &mut impl Write,
    escalated: &mut bool,
    privileged_channels: &mut std::collections::HashSet<String>,
) -> io::Result<()> {
    let channel = ctrl
        .get("channel")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let payload = ctrl.get("payload").and_then(Value::as_str).unwrap_or("");
    let open_name = ctrl.get("name").and_then(Value::as_str).unwrap_or("");

    if std::env::var_os("FEZ_FAKE_NO_RESOLVED").is_some()
        && open_name == "org.freedesktop.resolve1"
    {
        send_control(
            stdout,
            &json!({"command":"close","channel":channel,"problem":"not-found"}),
        );
        return Ok(());
    }

    if std::env::var_os("FEZ_FAKE_FIREWALLD_UNREACHABLE").is_some()
        && open_name == "org.fedoraproject.FirewallD1"
    {
        send_control(
            stdout,
            &json!({"command":"close","channel":channel,"problem":"not-found"}),
        );
        return Ok(());
    }

    if payload == "metrics1" && std::env::var_os("FEZ_FAKE_NO_PCP").is_some() {
        send_control(
            stdout,
            &json!({"command":"close","channel":channel,"problem":"not-supported"}),
        );
        return Ok(());
    }

    let privileged = ctrl.get("superuser").and_then(Value::as_str) == Some("require");
    let force_deny = std::env::var_os("FEZ_FAKE_DENY_PRIVILEGED").is_some();
    let deny_privileged = !*escalated || force_deny;
    if privileged && deny_privileged {
        send_control(
            stdout,
            &json!({"command":"close","channel":channel,"problem":"access-denied"}),
        );
        return Ok(());
    }
    if privileged {
        privileged_channels.insert(channel.clone());
    }
    send_control(stdout, &json!({"command":"ready","channel":channel}));
    match payload {
        "stream" => {
            let mut blob = serde_json::to_vec(&json!({
                "__REALTIME_TIMESTAMP":"1700000000000000","PRIORITY":"6",
                "SYSLOG_IDENTIFIER":"sshd","MESSAGE":"Server listening on port 22.","_PID":"1001"
            }))
            .unwrap();
            blob.push(b'\n');
            blob.extend_from_slice(
                &serde_json::to_vec(&json!({
                    "__REALTIME_TIMESTAMP":"1700000001000000","PRIORITY":"6",
                    "SYSLOG_IDENTIFIER":"sshd","MESSAGE":"Accepted publickey for fedora","_PID":"1002"
                }))
                .unwrap(),
            );
            blob.push(b'\n');
            blob.extend_from_slice(b"not-json\n");
            write_frame(stdout, &Frame::new(&channel, blob))?;
            send_control(stdout, &json!({"command":"done","channel":channel}));
            send_control(stdout, &json!({"command":"close","channel":channel}));
        }
        "metrics1" => {
            handle_metrics1(stdout, &channel)?;
        }
        _ => {}
    }
    Ok(())
}

/// Emit canned `metrics1` channel data: meta + two samples, then done/close.
fn handle_metrics1(stdout: &mut impl Write, channel: &str) -> io::Result<()> {
    // Meta message (JSON object on data channel)
    let meta = json!({
        "source": "direct",
        "interval": 1000,
        "timestamp": 1_700_000_000_000_i64,
        "metrics": [
            {"name": "kernel.all.load", "units": "", "semantics": "instant",
             "instances": ["1 minute", "5 minute", "15 minute"]},
            {"name": "kernel.all.cpu.user", "derive": "rate",
             "units": "millisec", "semantics": "counter"},
            {"name": "kernel.all.cpu.sys", "derive": "rate",
             "units": "millisec", "semantics": "counter"},
            {"name": "kernel.all.cpu.idle", "derive": "rate",
             "units": "millisec", "semantics": "counter"},
            {"name": "mem.physmem", "units": "Kbyte", "semantics": "discrete"},
            {"name": "mem.util.available", "units": "Kbyte", "semantics": "instant"},
            {"name": "disk.all.total", "derive": "rate",
             "units": "count", "semantics": "counter"},
            {"name": "network.interface.total.bytes", "derive": "rate",
             "units": "byte", "semantics": "counter",
             "instances": ["lo", "enp1s0", "enp2s0"]},
        ],
        "now": 1_700_000_000_000_i64,
    });
    let meta_bytes = serde_json::to_vec(&meta).unwrap();
    write_frame(stdout, &Frame::new(channel, meta_bytes))?;

    // Sample 1: rate values are false (no prior sample for delta)
    let sample1 = json!([[
        [0.42, 0.38, 0.35],
        false,
        false,
        false,
        16_384_000.0,
        12_000_000.0,
        false,
        [false, false, false]
    ]]);
    let s1_bytes = serde_json::to_vec(&sample1).unwrap();
    write_frame(stdout, &Frame::new(channel, s1_bytes))?;

    // Sample 2: real rate values
    let sample2 = json!([[
        [0.45, 0.38, 0.35],
        250.0,
        75.0,
        9675.0,
        16_384_000.0,
        12_000_000.0,
        42.5,
        [1024.0, 50000.0, 0.0]
    ]]);
    let s2_bytes = serde_json::to_vec(&sample2).unwrap();
    write_frame(stdout, &Frame::new(channel, s2_bytes))?;

    send_control(stdout, &json!({"command":"done","channel":channel}));
    send_control(stdout, &json!({"command":"close","channel":channel}));
    Ok(())
}

/// Handle a data frame: extract the D-Bus call and dispatch to the right
/// service module.
fn handle_data(
    frame: &Frame,
    stdout: &mut impl Write,
    bridges: &[(String, bool)],
    escalated: &mut bool,
    privileged_channels: &std::collections::HashSet<String>,
) {
    let msg: Value = serde_json::from_slice(&frame.payload).unwrap_or(Value::Null);
    let Some(call) = msg.get("call").and_then(Value::as_array) else {
        return;
    };
    let id = msg.get("id").cloned().unwrap_or(json!("0"));
    let path = call.first().and_then(Value::as_str).unwrap_or("");
    let iface = call.get(1).and_then(Value::as_str).unwrap_or("");
    let method = call.get(2).and_then(Value::as_str).unwrap_or("");
    let args = call
        .get(3)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let dnf_options_method = matches!(
        method,
        "open_session" | "list" | "install" | "remove" | "upgrade" | "resolve" | "do_transaction"
    );

    let reply = if path == hosttime::HOSTNAME_PATH || path == hosttime::TIMEDATE_PATH {
        hosttime::hosttime_reply(path, method, &args, &id)
    } else if path.starts_with(fw::FW_PATH) {
        let on_privileged = privileged_channels.contains(&frame.channel);
        fw::fw_reply(path, iface, method, &args, on_privileged, &id)
    } else if path.starts_with("/org/freedesktop/UDisks2") {
        udisks::udisks_reply(path, iface, method, &args, &id)
    } else if path.starts_with("/org/freedesktop/resolve1") {
        resolve::resolve_reply(path, iface, method, &args, &id)
    } else if path.starts_with(nm::NM_MGR_PATH) {
        nm::nm_reply(path, method, &id)
    } else if path == pk::PK_PATH && method == "CreateTransaction" {
        pk::pk_create_transaction(&id)
    } else if path == pk::PK_TX_PATH {
        pk::pk_emit(stdout, &frame.channel, method);
        return;
    } else if dnf_options_method {
        if let Some(err) = dnf::reject_unwrapped_options(&args, &id) {
            send_data(stdout, &frame.channel, &err);
            return;
        }
        dnf::dnf_reply(method, iface, &id)
    } else {
        systemd::systemd_reply(method, &args, bridges, escalated, &id)
    };
    send_data(stdout, &frame.channel, &reply);
}
