//! Read-only system overview: hostname + time gathered in a single command.

use super::{
    HOSTNAME_NAME, HOSTNAME_PATH, LOCALE_NAME, LOCALE_PATH, PROPS_IFACE, TIMEDATE_NAME,
    TIMEDATE_PATH,
};
use crate::capabilities::View;
use crate::error::{FezError, Result};
use crate::protocol::client::{variant_value, BridgeClient};
use serde_json::{json, Value};

/// Gather the full system overview and return a `SystemOverview` view.
///
/// Opens three unprivileged D-Bus channels (hostname1, timedate1, and locale1),
/// calls `hostname1.Describe()` for identity/hardware/OS, `timedate1 GetAll`
/// for time/NTP, and `locale1 GetAll` for locale/keyboard settings, then merges
/// all three into a single flat overview.
pub(super) fn show(client: &mut BridgeClient, host: &str) -> Result<View> {
    let hostname = gather_hostname(client)?;
    let time = gather_time(client)?;
    let locale = gather_locale(client)?;

    let data = merge_overview(&hostname, &time, &locale);
    let human = render_human(&data);

    Ok(View::new("SystemOverview", host, data, human))
}

/// Call `hostname1.Describe()` and parse the returned JSON string.
///
/// `Describe` returns a single `s` out-arg containing a JSON object as a
/// string. The bridge delivers it as `[string_value]`; we parse the inner
/// JSON to get a structured map.
fn gather_hostname(client: &mut BridgeClient) -> Result<Value> {
    let channel = client.dbus_open(HOSTNAME_NAME)?;
    let out = client.dbus_call(
        &channel,
        HOSTNAME_PATH,
        HOSTNAME_NAME,
        "Describe",
        json!([]),
    )?;
    let json_str = out
        .as_array()
        .and_then(|a| a.first())
        .and_then(Value::as_str)
        .ok_or_else(|| FezError::Problem("hostname1.Describe returned no value".into()))?;
    serde_json::from_str(json_str).map_err(FezError::Decode)
}

/// Read `timedate1` properties via `GetAll`.
///
/// Returns the `a{sv}` property dict. Each value is variant-wrapped
/// (`{"t":<sig>,"v":<value>}`) by cockpit-bridge.
fn gather_time(client: &mut BridgeClient) -> Result<Value> {
    let channel = client.dbus_open(TIMEDATE_NAME)?;
    let out = client.dbus_call(
        &channel,
        TIMEDATE_PATH,
        PROPS_IFACE,
        "GetAll",
        json!([TIMEDATE_NAME]),
    )?;
    out.get(0)
        .cloned()
        .ok_or_else(|| FezError::Problem("timedate1 GetAll returned no value".into()))
}

/// Read `locale1` properties via `GetAll`.
fn gather_locale(client: &mut BridgeClient) -> Result<Value> {
    let channel = client.dbus_open(LOCALE_NAME)?;
    let out = client.dbus_call(
        &channel,
        LOCALE_PATH,
        PROPS_IFACE,
        "GetAll",
        json!([LOCALE_NAME]),
    )?;
    out.get(0)
        .cloned()
        .ok_or_else(|| FezError::Problem("locale1 GetAll returned no value".into()))
}

/// Convert microsecond timestamps to ISO 8601 UTC strings.
///
/// Uses manual epoch arithmetic to avoid a chrono dependency. Handles dates
/// from 1970-01-01 through 9999-12-31 and accounts for leap years.
fn usec_to_iso(usec: u64) -> String {
    let total_secs = usec / 1_000_000;

    let days = total_secs / 86400;
    let day_secs = total_secs % 86400;
    let hours = day_secs / 3600;
    let minutes = (day_secs % 3600) / 60;
    let seconds = day_secs % 60;

    // Civil date from day count (algorithm from Howard Hinnant).
    let z = days as i64 + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{y:04}-{m:02}-{d:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

/// Parse os-release key=value lines into a JSON object.
fn parse_os_release(lines: &[Value]) -> Value {
    let mut map = serde_json::Map::new();
    for line in lines {
        if let Some(s) = line.as_str() {
            if let Some((k, v)) = s.split_once('=') {
                map.insert(k.to_lowercase(), json!(v));
            }
        }
    }
    Value::Object(map)
}

/// Extract a string field from a JSON object, returning `Value::Null` if absent.
fn str_field(v: &Value, key: &str) -> Value {
    json!(v.get(key).and_then(Value::as_str))
}

/// Extract a variant-wrapped field from a time properties dict.
fn time_str(time: &Value, key: &str) -> Value {
    json!(variant_value(time.get(key).unwrap_or(&Value::Null)).as_str())
}

fn time_bool(time: &Value, key: &str) -> Value {
    json!(variant_value(time.get(key).unwrap_or(&Value::Null)).as_bool())
}

fn time_usec(time: &Value, key: &str) -> u64 {
    variant_value(time.get(key).unwrap_or(&Value::Null))
        .as_u64()
        .unwrap_or(0)
}

/// Merge hostname, time, and locale data into the unified overview object.
fn merge_overview(hostname: &Value, time: &Value, locale: &Value) -> Value {
    let os_release = hostname
        .get("OperatingSystemReleaseData")
        .and_then(Value::as_array)
        .map(|arr| parse_os_release(arr))
        .unwrap_or(json!({}));

    let support_end = hostname
        .get("OperatingSystemSupportEnd")
        .and_then(Value::as_u64)
        .map(usec_to_iso);

    json!({
        "hostname": str_field(hostname, "Hostname"),
        "static_hostname": str_field(hostname, "StaticHostname"),
        "pretty_hostname": str_field(hostname, "PrettyHostname"),
        "hostname_source": str_field(hostname, "HostnameSource"),
        "machine_id": str_field(hostname, "MachineID"),
        "boot_id": str_field(hostname, "BootID"),

        "os": str_field(hostname, "OperatingSystemPrettyName"),
        "os_id": os_release.get("id").and_then(Value::as_str),
        "os_version_id": os_release.get("version_id").and_then(Value::as_str),
        "os_variant_id": os_release.get("variant_id").and_then(Value::as_str),
        "os_cpe": str_field(hostname, "OperatingSystemCPEName"),
        "os_support_end": support_end,
        "os_release": os_release,

        "kernel": str_field(hostname, "KernelName"),
        "kernel_release": str_field(hostname, "KernelRelease"),
        "kernel_version": str_field(hostname, "KernelVersion"),

        "chassis": str_field(hostname, "Chassis"),
        "hardware_vendor": str_field(hostname, "HardwareVendor"),
        "hardware_model": str_field(hostname, "HardwareModel"),

        "firmware_vendor": str_field(hostname, "FirmwareVendor"),
        "firmware_version": str_field(hostname, "FirmwareVersion"),

        "timezone": time_str(time, "Timezone"),
        "ntp_enabled": time_bool(time, "NTP"),
        "ntp_synchronized": time_bool(time, "NTPSynchronized"),
        "local_rtc": time_bool(time, "LocalRTC"),
        "time_utc": usec_to_iso(time_usec(time, "TimeUSec")),
        "rtc_time_utc": usec_to_iso(time_usec(time, "RTCTimeUSec")),

        "locale": variant_value(locale.get("Locale").unwrap_or(&Value::Null))
            .as_array()
            .and_then(|a| a.first())
            .and_then(Value::as_str)
            .unwrap_or(""),
        "keymap": variant_value(locale.get("X11Layout").unwrap_or(&Value::Null))
            .as_str()
            .unwrap_or(""),
        "console_keymap": variant_value(locale.get("VConsoleKeymap").unwrap_or(&Value::Null))
            .as_str()
            .unwrap_or(""),
    })
}

/// Section layout for human rendering: (heading, [(label, json_key)]).
const SECTIONS: &[(&str, &[(&str, &str)])] = &[
    (
        "Host",
        &[
            ("Hostname", "hostname"),
            ("Static hostname", "static_hostname"),
            ("Pretty hostname", "pretty_hostname"),
            ("Hostname source", "hostname_source"),
            ("Machine ID", "machine_id"),
            ("Boot ID", "boot_id"),
            ("Chassis", "chassis"),
        ],
    ),
    (
        "Operating System",
        &[
            ("OS", "os"),
            ("ID", "os_id"),
            ("Version", "os_version_id"),
            ("Variant", "os_variant_id"),
            ("CPE", "os_cpe"),
            ("Support end", "os_support_end"),
        ],
    ),
    (
        "Kernel",
        &[
            ("Name", "kernel"),
            ("Release", "kernel_release"),
            ("Version", "kernel_version"),
        ],
    ),
    (
        "Hardware",
        &[("Vendor", "hardware_vendor"), ("Model", "hardware_model")],
    ),
    (
        "Firmware",
        &[
            ("Vendor", "firmware_vendor"),
            ("Version", "firmware_version"),
        ],
    ),
    (
        "Time",
        &[
            ("Timezone", "timezone"),
            ("NTP enabled", "ntp_enabled"),
            ("NTP synchronized", "ntp_synchronized"),
            ("Local RTC", "local_rtc"),
            ("Time (UTC)", "time_utc"),
            ("RTC time (UTC)", "rtc_time_utc"),
        ],
    ),
    (
        "Locale",
        &[
            ("Locale", "locale"),
            ("Keyboard layout", "keymap"),
            ("Console keymap", "console_keymap"),
        ],
    ),
];

/// Render the human-readable overview from the SECTIONS table.
fn render_human(data: &Value) -> String {
    let mut s = String::new();
    for (heading, fields) in SECTIONS {
        s.push_str(heading);
        s.push('\n');
        for (label, key) in *fields {
            if let Some(v) = data.get(*key) {
                let display = match v {
                    Value::Null => continue,
                    Value::String(sv) if sv.is_empty() => continue,
                    Value::String(sv) => sv.to_string(),
                    Value::Bool(b) => if *b { "yes" } else { "no" }.to_string(),
                    Value::Number(n) => n.to_string(),
                    _ => continue,
                };
                s.push_str(&format!("  {label:20} {display}\n"));
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_os_release_builds_map() {
        let lines = vec![
            json!("ID=fedora"),
            json!("VERSION_ID=44"),
            json!("VARIANT_ID=workstation"),
        ];
        let m = parse_os_release(&lines);
        assert_eq!(m["id"], "fedora");
        assert_eq!(m["version_id"], "44");
        assert_eq!(m["variant_id"], "workstation");
    }

    #[test]
    fn parse_os_release_handles_empty_value() {
        let lines = vec![json!("VERSION_CODENAME=")];
        let m = parse_os_release(&lines);
        assert_eq!(m["version_codename"], "");
    }

    #[test]
    fn usec_to_iso_converts_timestamp() {
        let iso = usec_to_iso(1700006400000000);
        assert!(iso.starts_with("2023-11-15"), "got: {iso}");
        assert!(iso.ends_with('Z'));
    }

    #[test]
    fn usec_to_iso_handles_zero() {
        let iso = usec_to_iso(0);
        assert!(iso.starts_with("1970-01-01"), "got: {iso}");
    }

    #[test]
    fn merge_overview_shapes_data() {
        let hostname = json!({
            "Hostname": "box1",
            "StaticHostname": "box1",
            "PrettyHostname": null,
            "HostnameSource": "static",
            "MachineID": "abc",
            "BootID": "def",
            "OperatingSystemPrettyName": "Fedora 44",
            "OperatingSystemCPEName": "cpe:/o:fedoraproject:fedora:44",
            "OperatingSystemSupportEnd": 1810684800000000u64,
            "OperatingSystemReleaseData": ["ID=fedora", "VERSION_ID=44"],
            "KernelName": "Linux",
            "KernelRelease": "7.0.10",
            "KernelVersion": "#1 SMP",
            "Chassis": "desktop",
            "HardwareVendor": "Dell",
            "HardwareModel": "OptiPlex",
            "FirmwareVendor": "Dell",
            "FirmwareVersion": "1.10",
        });
        let time = json!({
            "Timezone": {"t":"s","v":"America/Chicago"},
            "NTP": {"t":"b","v":true},
            "NTPSynchronized": {"t":"b","v":true},
            "LocalRTC": {"t":"b","v":false},
            "TimeUSec": {"t":"t","v":1700006400000000u64},
            "RTCTimeUSec": {"t":"t","v":1700006400000000u64},
        });
        let locale = json!({
            "Locale": {"t":"as","v":["LANG=en_US.UTF-8"]},
            "X11Layout": {"t":"s","v":"us"},
            "VConsoleKeymap": {"t":"s","v":"us"},
        });
        let data = merge_overview(&hostname, &time, &locale);
        assert_eq!(data["hostname"], "box1");
        assert_eq!(data["os"], "Fedora 44");
        assert_eq!(data["os_id"], "fedora");
        assert_eq!(data["os_version_id"], "44");
        assert_eq!(data["kernel_release"], "7.0.10");
        assert_eq!(data["chassis"], "desktop");
        assert_eq!(data["hardware_vendor"], "Dell");
        assert_eq!(data["timezone"], "America/Chicago");
        assert_eq!(data["ntp_enabled"], true);
        assert_eq!(data["ntp_synchronized"], true);
        assert_eq!(data["local_rtc"], false);
        assert!(data["time_utc"].as_str().unwrap().starts_with("2023-11-15"));
        assert!(data["os_support_end"]
            .as_str()
            .unwrap()
            .starts_with("2027-"));
        assert!(data["pretty_hostname"].is_null());
        assert_eq!(data["locale"], "LANG=en_US.UTF-8");
        assert_eq!(data["keymap"], "us");
        assert_eq!(data["console_keymap"], "us");
    }

    #[test]
    fn render_human_omits_null_and_empty() {
        let data = json!({
            "hostname": "box1",
            "static_hostname": "box1",
            "pretty_hostname": null,
            "hostname_source": "static",
            "machine_id": "abc",
            "boot_id": "def",
            "chassis": "",
            "os": "Fedora 44",
            "os_id": "fedora",
            "os_version_id": "44",
            "os_variant_id": null,
            "os_cpe": "cpe",
            "os_support_end": "2027-05-19T00:00:00Z",
            "kernel": "Linux",
            "kernel_release": "7.0.10",
            "kernel_version": "#1",
            "hardware_vendor": "Dell",
            "hardware_model": "OptiPlex",
            "firmware_vendor": "Dell",
            "firmware_version": "1.10",
            "timezone": "America/Chicago",
            "ntp_enabled": true,
            "ntp_synchronized": true,
            "local_rtc": false,
            "time_utc": "2023-11-15T00:00:00Z",
            "rtc_time_utc": "2023-11-15T00:00:00Z",
            "locale": "LANG=en_US.UTF-8",
            "keymap": "us",
            "console_keymap": "us",
        });
        let human = render_human(&data);
        assert!(human.contains("Hostname"));
        assert!(human.contains("box1"));
        assert!(human.contains("Fedora 44"));
        assert!(human.contains("America/Chicago"));
        assert!(human.contains("yes"));
        assert!(human.contains("no"));
        assert!(!human.contains("Pretty hostname"));
        assert!(!human.contains("Chassis"));
        assert!(!human.contains("Variant"));
        assert!(human.contains("Locale"));
        assert!(human.contains("LANG=en_US.UTF-8"));
    }
}
