//! Journal capability: query systemd journal entries via `journalctl`.

use crate::capabilities::View;
use crate::error::Result;
use crate::protocol::client::BridgeClient;
use crate::protocol::variant::Variant;
use serde::Deserialize;
use serde_json::{json, Value};

/// Parsed flags for `fez journal`.
#[derive(Debug)]
pub struct JournalArgs<'a> {
    /// Filter by systemd units (repeatable `--unit` flags).
    pub units: &'a [String],
    /// Show entries since the specified date/time (`--since`).
    pub since: Option<&'a str>,
    /// Show entries until the specified date/time (`--until`).
    pub until: Option<&'a str>,
    /// Filter by syslog priority (`--priority`).
    pub priority: Option<&'a str>,
    /// Maximum number of lines to display (`--lines`).
    pub lines: u32,
    /// Boot ID filter (`--boot`): `None` = all boots, `Some(None)` = current boot, `Some(Some(id))` = specific boot.
    pub boot: Option<Option<i64>>,
    /// Filter by pattern-matching message text (`--grep`).
    pub grep: Option<&'a str>,
    /// List available boots instead of entries (`--list-boots`).
    pub list_boots: bool,
    /// List available journal fields instead of entries (`--list-fields`).
    pub list_fields: bool,
    /// Additional fields to include in output (`--output-fields`).
    pub output_fields: &'a [String],
}

/// A single journal entry parsed from `journalctl --output=json`.
///
/// Default fields are always extracted. Additional fields from
/// `--output-fields` are captured in the `extra` map.
#[derive(Debug, Default, Deserialize)]
struct JournalEntry {
    #[serde(rename = "__REALTIME_TIMESTAMP", default)]
    timestamp: Variant<String>,
    #[serde(rename = "_HOSTNAME", default)]
    hostname: Variant<String>,
    #[serde(rename = "SYSLOG_IDENTIFIER", default)]
    identifier: Variant<String>,
    #[serde(rename = "_PID", default)]
    pid: Variant<String>,
    #[serde(rename = "PRIORITY", default)]
    priority: Variant<String>,
    #[serde(rename = "MESSAGE", default)]
    message: Variant<String>,
    /// All fields, used to extract --output-fields extras.
    #[serde(flatten)]
    all_fields: std::collections::HashMap<String, Value>,
}

/// Boot entry from `journalctl --list-boots --output=json`.
#[derive(Debug, Deserialize)]
struct BootEntry {
    index: i64,
    boot_id: String,
    first_entry: u64,
    last_entry: u64,
}

/// Run the journal capability.
pub fn run(
    client: &mut BridgeClient,
    host: String,
    _as_json: bool,
    args: &JournalArgs,
) -> Result<View> {
    if args.list_boots {
        return run_list_boots(client, host);
    }
    if args.list_fields {
        return run_list_fields(client, host);
    }
    run_entries(client, host, args)
}

fn run_list_boots(client: &mut BridgeClient, host: String) -> Result<View> {
    let argv = vec!["journalctl", "--list-boots", "--output=json", "--no-pager"];
    let blob = client.stream_collect(&argv)?;
    let mut boots = Vec::new();
    let mut human = String::new();
    for line in blob.split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
        if let Ok(boot) = serde_json::from_slice::<BootEntry>(line) {
            human.push_str(&format!(
                "{:>3}  {}  {} — {}\n",
                boot.index,
                &boot.boot_id[..12.min(boot.boot_id.len())],
                format_timestamp(boot.first_entry),
                format_timestamp(boot.last_entry),
            ));
            boots.push(json!({
                "id": boot.index,
                "boot_id": boot.boot_id,
                "first": format_timestamp(boot.first_entry),
                "last": format_timestamp(boot.last_entry),
            }));
        }
    }
    Ok(View::new(
        "JournalBoots",
        host,
        json!({"boots": boots}),
        human,
    ))
}

fn run_list_fields(client: &mut BridgeClient, host: String) -> Result<View> {
    let argv = vec!["journalctl", "--fields", "--no-pager"];
    let blob = client.stream_collect(&argv)?;
    let text = String::from_utf8_lossy(&blob);
    let mut fields: Vec<String> = text
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    fields.sort();
    let human = fields.join("\n");
    Ok(View::new(
        "JournalFields",
        host,
        json!({"fields": fields}),
        human,
    ))
}

fn run_entries(client: &mut BridgeClient, host: String, args: &JournalArgs) -> Result<View> {
    // Request one extra entry to detect truncation.
    let fetch_limit = args.lines.saturating_add(1);
    let argv = build_argv(args, fetch_limit);
    let argv_refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();

    let blob = client.stream_collect(&argv_refs)?;

    let mut entries = Vec::new();
    for line in blob.split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
        if let Ok(entry) = serde_json::from_slice::<JournalEntry>(line) {
            entries.push(entry);
        }
    }

    let truncated = entries.len() > args.lines as usize;
    if truncated {
        entries.truncate(args.lines as usize);
    }

    let mut human = String::new();
    let mut json_entries = Vec::new();
    for entry in &entries {
        human.push_str(&format_human_line(entry, args.output_fields));
        human.push('\n');
        json_entries.push(entry_to_json(entry, args.output_fields));
    }

    let mut data = json!({
        "entries": json_entries,
        "lines": args.lines,
        "truncated": truncated,
    });
    if !args.units.is_empty() {
        data["units"] = json!(args.units);
    }

    let mut view = View::new("JournalEntries", host, data, human);
    if truncated {
        view = view.with_hints(json!(vec![
            "Output truncated. Narrow with --since, --grep, --priority, or increase --lines."
        ]));
    }
    Ok(view)
}

/// Build the journalctl argv as owned Strings (avoids lifetime issues with
/// numeric arguments that need `.to_string()`).
fn build_argv(args: &JournalArgs, fetch_limit: u32) -> Vec<String> {
    let mut argv: Vec<String> = vec![
        "journalctl".into(),
        "--output=json".into(),
        "--no-pager".into(),
    ];
    for unit in args.units {
        argv.push("--unit".into());
        argv.push(unit.clone());
    }
    if let Some(since) = args.since {
        argv.push("--since".into());
        argv.push(since.into());
    }
    if let Some(until) = args.until {
        argv.push("--until".into());
        argv.push(until.into());
    }
    if let Some(priority) = args.priority {
        argv.push("--priority".into());
        argv.push(priority.into());
    }
    if let Some(grep) = args.grep {
        argv.push("--grep".into());
        argv.push(grep.into());
    }
    match args.boot {
        Some(Some(id)) => {
            argv.push("--boot".into());
            argv.push(id.to_string());
        }
        Some(None) => {
            argv.push("--boot".into());
            argv.push("0".into());
        }
        None => {}
    }
    argv.push("--lines".into());
    argv.push(fetch_limit.to_string());
    argv
}

fn entry_to_json(entry: &JournalEntry, extra_fields: &[String]) -> Value {
    let mut obj = json!({
        "timestamp": entry.timestamp.0,
        "hostname": entry.hostname.0,
        "identifier": entry.identifier.0,
        "pid": entry.pid.0,
        "priority": priority_name(&entry.priority.0),
        "message": entry.message.0,
    });
    for field in extra_fields {
        if let Some(val) = entry.all_fields.get(field.as_str()) {
            obj[field] = val.clone();
        }
    }
    obj
}

fn format_human_line(entry: &JournalEntry, extra_fields: &[String]) -> String {
    let ts = format_timestamp(entry.timestamp.0.parse::<u64>().unwrap_or(0));
    let pri = priority_name(&entry.priority.0);
    let mut line = format!(
        "{}  {} {}[{}] {}: {}",
        ts, entry.hostname.0, entry.identifier.0, entry.pid.0, pri, entry.message.0
    );
    if !extra_fields.is_empty() {
        let extras: Vec<String> = extra_fields
            .iter()
            .filter_map(|f| {
                entry
                    .all_fields
                    .get(f.as_str())
                    .map(|v| format!("{}={}", f, v.as_str().unwrap_or(&v.to_string())))
            })
            .collect();
        if !extras.is_empty() {
            line.push_str(&format!("  [{}]", extras.join(", ")));
        }
    }
    line
}

/// Convert a microsecond epoch timestamp to a human-readable UTC string.
fn format_timestamp(us: u64) -> String {
    let secs = (us / 1_000_000) as i64;
    let nanos = ((us % 1_000_000) * 1000) as u32;
    // Chrono-free UTC formatting via Hinnant civil date algorithm.

    time_from_epoch(secs, nanos)
}

/// Format epoch seconds as `YYYY-MM-DD HH:MM:SS` in UTC without external deps.
fn time_from_epoch(secs: i64, _nanos: u32) -> String {
    // Days since Unix epoch → date, seconds within day → time.
    let days = secs.div_euclid(86400);
    let day_secs = secs.rem_euclid(86400);
    let h = day_secs / 3600;
    let m = (day_secs % 3600) / 60;
    let s = day_secs % 60;

    // Civil date from day count (algorithm from Howard Hinnant).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let yr = if mo <= 2 { y + 1 } else { y };

    format!("{yr:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02}")
}

/// Map numeric priority string to a name.
fn priority_name(p: &str) -> &'static str {
    match p {
        "0" => "emerg",
        "1" => "alert",
        "2" => "crit",
        "3" => "err",
        "4" => "warning",
        "5" => "notice",
        "6" => "info",
        "7" => "debug",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priority_name_maps_all_levels() {
        assert_eq!(priority_name("0"), "emerg");
        assert_eq!(priority_name("3"), "err");
        assert_eq!(priority_name("6"), "info");
        assert_eq!(priority_name("7"), "debug");
        assert_eq!(priority_name("99"), "unknown");
    }

    #[test]
    fn format_timestamp_renders_known_epoch() {
        // 1700000000 = 2023-11-14 22:13:20 UTC
        assert_eq!(
            format_timestamp(1_700_000_000_000_000),
            "2023-11-14 22:13:20"
        );
    }

    #[test]
    fn build_argv_default_has_lines_26() {
        let args = JournalArgs {
            units: &[],
            since: None,
            until: None,
            priority: None,
            lines: 25,
            boot: None,
            grep: None,
            list_boots: false,
            list_fields: false,
            output_fields: &[],
        };
        let argv = build_argv(&args, 26);
        assert!(argv.contains(&"--lines".into()));
        assert!(argv.contains(&"26".into()));
    }

    #[test]
    fn build_argv_with_all_flags() {
        let units = vec!["sshd.service".into()];
        let fields = vec!["_COMM".into()];
        let args = JournalArgs {
            units: &units,
            since: Some("1 hour ago"),
            until: Some("now"),
            priority: Some("err"),
            lines: 50,
            boot: Some(Some(-1)),
            grep: Some("error"),
            list_boots: false,
            list_fields: false,
            output_fields: &fields,
        };
        let argv = build_argv(&args, 51);
        assert!(argv.contains(&"--unit".into()));
        assert!(argv.contains(&"sshd.service".into()));
        assert!(argv.contains(&"--since".into()));
        assert!(argv.contains(&"--until".into()));
        assert!(argv.contains(&"--priority".into()));
        assert!(argv.contains(&"--grep".into()));
        assert!(argv.contains(&"--boot".into()));
        assert!(argv.contains(&"-1".into()));
        assert!(argv.contains(&"--lines".into()));
        assert!(argv.contains(&"51".into()));
    }

    #[test]
    fn entry_to_json_includes_extra_fields() {
        let mut all = std::collections::HashMap::new();
        all.insert("_COMM".to_string(), json!("sshd"));
        all.insert("_EXE".to_string(), json!("/usr/sbin/sshd"));
        let entry = JournalEntry {
            timestamp: Variant("123".into()),
            hostname: Variant("host".into()),
            identifier: Variant("sshd".into()),
            pid: Variant("1".into()),
            priority: Variant("6".into()),
            message: Variant("hello".into()),
            all_fields: all,
        };
        let extras = vec!["_COMM".into()];
        let j = entry_to_json(&entry, &extras);
        assert_eq!(j["_COMM"], "sshd");
        assert!(j.get("_EXE").is_none()); // not requested
    }

    #[test]
    fn journal_entry_parses_with_missing_fields() {
        let entry: JournalEntry = serde_json::from_value(json!({
            "MESSAGE": "hello"
        }))
        .unwrap();
        assert_eq!(entry.message.0, "hello");
        assert_eq!(entry.hostname.0, "");
        assert_eq!(entry.priority.0, "");
    }
}
