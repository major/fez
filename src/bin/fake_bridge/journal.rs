//! Canned journal stream responses for fake bridge.

use serde_json::json;
use std::io::Write;

use crate::{send_control, write_frame, Frame};

/// All canned journal entries.
fn canned_entries() -> Vec<serde_json::Value> {
    vec![
        json!({
            "__REALTIME_TIMESTAMP": "1700000000000000",
            "PRIORITY": "6",
            "SYSLOG_IDENTIFIER": "sshd",
            "MESSAGE": "Server listening on port 22.",
            "_PID": "1001",
            "_HOSTNAME": "testbox",
            "_SYSTEMD_UNIT": "sshd.service",
            "_BOOT_ID": "boot-current",
            "_COMM": "sshd",
            "_EXE": "/usr/sbin/sshd"
        }),
        json!({
            "__REALTIME_TIMESTAMP": "1700000001000000",
            "PRIORITY": "6",
            "SYSLOG_IDENTIFIER": "sshd",
            "MESSAGE": "Accepted publickey for fedora",
            "_PID": "1002",
            "_HOSTNAME": "testbox",
            "_SYSTEMD_UNIT": "sshd.service",
            "_BOOT_ID": "boot-current",
            "_COMM": "sshd",
            "_EXE": "/usr/sbin/sshd"
        }),
        json!({
            "__REALTIME_TIMESTAMP": "1700000002000000",
            "PRIORITY": "3",
            "SYSLOG_IDENTIFIER": "sshd",
            "MESSAGE": "Connection closed by invalid user",
            "_PID": "1003",
            "_HOSTNAME": "testbox",
            "_SYSTEMD_UNIT": "sshd.service",
            "_BOOT_ID": "boot-current",
            "_COMM": "sshd",
            "_EXE": "/usr/sbin/sshd"
        }),
        json!({
            "__REALTIME_TIMESTAMP": "1700000003000000",
            "PRIORITY": "6",
            "SYSLOG_IDENTIFIER": "chronyd",
            "MESSAGE": "Selected source 192.168.1.1",
            "_PID": "2001",
            "_HOSTNAME": "testbox",
            "_SYSTEMD_UNIT": "chronyd.service",
            "_BOOT_ID": "boot-current",
            "_COMM": "chronyd",
            "_EXE": "/usr/sbin/chronyd"
        }),
        json!({
            "__REALTIME_TIMESTAMP": "1700000004000000",
            "PRIORITY": "4",
            "SYSLOG_IDENTIFIER": "chronyd",
            "MESSAGE": "System clock wrong by 1.5 seconds",
            "_PID": "2002",
            "_HOSTNAME": "testbox",
            "_SYSTEMD_UNIT": "chronyd.service",
            "_BOOT_ID": "boot-current",
            "_COMM": "chronyd",
            "_EXE": "/usr/sbin/chronyd"
        }),
        json!({
            "__REALTIME_TIMESTAMP": "1699000000000000",
            "PRIORITY": "6",
            "SYSLOG_IDENTIFIER": "sshd",
            "MESSAGE": "Server listening on port 22.",
            "_PID": "1000",
            "_HOSTNAME": "testbox",
            "_SYSTEMD_UNIT": "sshd.service",
            "_BOOT_ID": "boot-prev",
            "_COMM": "sshd",
            "_EXE": "/usr/sbin/sshd"
        }),
    ]
}

/// Handle a `stream` open whose `spawn` argv starts with `journalctl`.
pub(crate) fn handle_stream(
    stdout: &mut impl Write,
    channel: &str,
    argv: &[String],
) -> std::io::Result<()> {
    if argv.contains(&"--list-boots".to_string()) {
        return handle_list_boots(stdout, channel);
    }
    if argv.contains(&"--fields".to_string()) {
        return handle_list_fields(stdout, channel);
    }
    handle_entries(stdout, channel, argv)
}

fn handle_list_boots(stdout: &mut impl Write, channel: &str) -> std::io::Result<()> {
    let boots = vec![
        json!({"index": 0, "boot_id": "boot-current", "first_entry": 1700000000000000_u64, "last_entry": 1700000004000000_u64}),
        json!({"index": -1, "boot_id": "boot-prev", "first_entry": 1699000000000000_u64, "last_entry": 1699000099000000_u64}),
    ];
    let mut blob = Vec::new();
    for b in &boots {
        blob.extend_from_slice(&serde_json::to_vec(b).unwrap());
        blob.push(b'\n');
    }
    write_frame(stdout, &Frame::new(channel, blob))?;
    send_control(stdout, &json!({"command":"done","channel":channel}));
    send_control(stdout, &json!({"command":"close","channel":channel}));
    Ok(())
}

fn handle_list_fields(stdout: &mut impl Write, channel: &str) -> std::io::Result<()> {
    let fields = "MESSAGE\nPRIORITY\nSYSLOG_IDENTIFIER\n_PID\n_HOSTNAME\n\
                  _SYSTEMD_UNIT\n_BOOT_ID\n_COMM\n_EXE\n__REALTIME_TIMESTAMP\n";
    write_frame(
        stdout,
        &Frame::new(channel, fields.as_bytes().to_vec()),
    )?;
    send_control(stdout, &json!({"command":"done","channel":channel}));
    send_control(stdout, &json!({"command":"close","channel":channel}));
    Ok(())
}

fn handle_entries(
    stdout: &mut impl Write,
    channel: &str,
    argv: &[String],
) -> std::io::Result<()> {
    let mut entries = canned_entries();

    // --unit filtering
    let unit_filters: Vec<&str> = argv
        .windows(2)
        .filter(|w| w[0] == "--unit")
        .map(|w| w[1].as_str())
        .collect();
    if !unit_filters.is_empty() {
        entries.retain(|e| {
            e["_SYSTEMD_UNIT"]
                .as_str()
                .is_some_and(|u| unit_filters.contains(&u))
        });
    }

    // --priority filtering
    if let Some(pos) = argv.iter().position(|a| a == "--priority") {
        if let Some(p_str) = argv.get(pos + 1) {
            if let Ok(max_pri) = p_str.parse::<u32>() {
                entries.retain(|e| {
                    e["PRIORITY"]
                        .as_str()
                        .and_then(|s| s.parse::<u32>().ok())
                        .is_some_and(|p| p <= max_pri)
                });
            }
        }
    }

    // --boot filtering
    if let Some(pos) = argv.iter().position(|a| a == "-b" || a == "--boot") {
        let boot_val = argv
            .get(pos + 1)
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(0);
        let boot_id = if boot_val == 0 {
            "boot-current"
        } else {
            "boot-prev"
        };
        entries.retain(|e| e["_BOOT_ID"].as_str() == Some(boot_id));
    }

    // --grep filtering (substring match)
    if let Some(pos) = argv.iter().position(|a| a == "--grep") {
        if let Some(pat) = argv.get(pos + 1) {
            entries.retain(|e| {
                e["MESSAGE"]
                    .as_str()
                    .is_some_and(|m| m.contains(pat.as_str()))
            });
        }
    }

    // --since filtering (only @epoch form for determinism)
    if let Some(pos) = argv.iter().position(|a| a == "--since") {
        if let Some(since_str) = argv.get(pos + 1) {
            if let Some(epoch) = since_str.strip_prefix('@') {
                if let Ok(ts) = epoch.parse::<u64>() {
                    let ts_us = ts * 1_000_000;
                    entries.retain(|e| {
                        e["__REALTIME_TIMESTAMP"]
                            .as_str()
                            .and_then(|s| s.parse::<u64>().ok())
                            .is_some_and(|t| t >= ts_us)
                    });
                }
            }
        }
    }

    // --lines truncation (take last N)
    if let Some(pos) = argv.iter().position(|a| a == "--lines" || a == "-n") {
        if let Some(n_str) = argv.get(pos + 1) {
            if let Ok(n) = n_str.parse::<usize>() {
                let len = entries.len();
                if n < len {
                    entries = entries.split_off(len - n);
                }
            }
        }
    }

    let mut blob = Vec::new();
    for entry in &entries {
        blob.extend_from_slice(&serde_json::to_vec(entry).unwrap());
        blob.push(b'\n');
    }
    blob.extend_from_slice(b"not-json\n");
    write_frame(stdout, &Frame::new(channel, blob))?;
    send_control(stdout, &json!({"command":"done","channel":channel}));
    send_control(stdout, &json!({"command":"close","channel":channel}));
    Ok(())
}
