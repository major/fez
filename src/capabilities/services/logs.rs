use crate::capabilities::View;
use crate::error::Result;
use crate::protocol::client::BridgeClient;
use crate::protocol::variant::Variant;
use serde::Deserialize;
use serde_json::{json, Value};

/// A single journald entry from `journalctl --output=json`.
///
/// journald fields arrive as flat scalars (no variant envelope); [`Variant`]
/// passes them through unchanged. Defaults keep partial entries decodable.
#[derive(Debug, Default, Deserialize)]
struct JournalLine {
    #[serde(rename = "__REALTIME_TIMESTAMP", default)]
    timestamp: Variant<String>,
    #[serde(rename = "PRIORITY", default)]
    priority: Variant<String>,
    #[serde(rename = "SYSLOG_IDENTIFIER", default)]
    identifier: Variant<String>,
    #[serde(rename = "MESSAGE", default)]
    message: Variant<String>,
    #[serde(rename = "_PID", default)]
    pid: Variant<String>,
}

/// Reads or follows journal entries for a systemd unit.
///
/// # Errors
///
/// Returns an error if the bridge client cannot run or stream `journalctl`.
#[allow(clippy::too_many_arguments)]
pub(super) fn run(
    client: &mut BridgeClient,
    host: String,
    as_json: bool,
    unit: &str,
    since: Option<&str>,
    priority: Option<&str>,
    lines: Option<u32>,
    follow: bool,
) -> Result<View> {
    let lines_s = lines.map(|n| n.to_string());
    let mut argv: Vec<&str> = vec!["journalctl", "--output=json", "--no-pager", "--unit", unit];
    if let Some(x) = since {
        argv.extend(["--since", x]);
    }
    if let Some(x) = priority {
        argv.extend(["--priority", x]);
    }
    if let Some(x) = lines_s.as_deref() {
        argv.extend(["--lines", x]);
    }
    if follow {
        argv.push("--follow");
    }

    if follow {
        // stream live; print each parsed line as it arrives.
        client.stream_each(&argv, |chunk| {
            for line in chunk.split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
                // journalctl can mix diagnostics with JSON; malformed lines are skipped.
                if let Ok(entry) = serde_json::from_slice::<JournalLine>(line) {
                    if as_json {
                        println!(
                            "{}",
                            serde_json::to_string(&log_entry(&entry)).unwrap_or_default()
                        );
                    } else {
                        println!("{}", log_human_line(&entry));
                    }
                }
            }
        })?;
        return Ok(View::new("LogEntries", host, Value::Null, String::new()).pre_rendered());
    }

    let blob = client.stream_collect(&argv)?;
    let mut entries = Vec::new();
    let mut human = String::new();
    for line in blob.split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
        if let Ok(entry) = serde_json::from_slice::<JournalLine>(line) {
            human.push_str(&log_human_line(&entry));
            human.push('\n');
            entries.push(log_entry(&entry));
        }
    }
    Ok(View::new(
        "LogEntries",
        host,
        json!({"unit": unit, "entries": entries}),
        human,
    ))
}

fn log_entry(entry: &JournalLine) -> Value {
    json!({
        "timestamp": entry.timestamp.0,
        "priority": entry.priority.0,
        "identifier": entry.identifier.0,
        "message": entry.message.0,
        "pid": entry.pid.0,
    })
}

fn log_human_line(entry: &JournalLine) -> String {
    format!(
        "{}  {}: {}",
        entry.timestamp.0, entry.identifier.0, entry.message.0
    )
}

#[cfg(test)]
mod tests {
    use super::JournalLine;
    use serde_json::json;

    // journald JSON fields are flat strings, not variants; Variant<T> passes
    // them through unchanged and absent fields default to empty.
    #[test]
    fn journal_line_reads_flat_fields() {
        let entry: JournalLine = serde_json::from_value(json!({
            "MESSAGE": "hello",
            "SYSLOG_IDENTIFIER": "sshd",
        }))
        .unwrap();
        assert_eq!(entry.message.0, "hello");
        assert_eq!(entry.identifier.0, "sshd");
        assert_eq!(entry.priority.0, "");
    }
}
