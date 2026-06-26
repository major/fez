//! Live PCP performance snapshot: CPU, memory, load, disk I/O, network.

use crate::capabilities::View;
use crate::error::Result;
use crate::protocol::client::{BridgeClient, MetricRequest};
use serde_json::{json, Value};

/// PCP metrics requested for the system snapshot.
const METRIC_REQUESTS: &[MetricRequest<'static>] = &[
    MetricRequest {
        name: "kernel.all.load",
        derive: None,
    },
    MetricRequest {
        name: "kernel.all.cpu.user",
        derive: Some("rate"),
    },
    MetricRequest {
        name: "kernel.all.cpu.sys",
        derive: Some("rate"),
    },
    MetricRequest {
        name: "kernel.all.cpu.idle",
        derive: Some("rate"),
    },
    MetricRequest {
        name: "mem.physmem",
        derive: None,
    },
    MetricRequest {
        name: "mem.util.available",
        derive: None,
    },
    MetricRequest {
        name: "disk.all.total",
        derive: Some("rate"),
    },
    MetricRequest {
        name: "network.interface.total.bytes",
        derive: Some("rate"),
    },
];

/// Interval between samples in milliseconds.
const INTERVAL_MS: u64 = 1000;

/// Number of samples to collect (2 needed for rate derivation).
const SAMPLE_COUNT: u64 = 2;

/// Gather a one-shot performance snapshot and return a `SystemMetrics` view.
///
/// # Errors
///
/// Returns an error if PCP metrics collection fails (e.g. `python3-pcp`
/// is not installed on the target).
pub(super) fn show(client: &mut BridgeClient, host: &str) -> Result<View> {
    let snapshot = client.metrics_collect(METRIC_REQUESTS, INTERVAL_MS, SAMPLE_COUNT)?;

    let data = build_data(&snapshot.meta, &snapshot.samples);
    let human = render_human(&data);

    Ok(View::new("SystemMetrics", host, data, human))
}

/// Build structured JSON from the meta descriptor and the last sample.
///
/// Uses the last sample because rate-derived metrics return `false` on the
/// first sample (no prior value for delta computation).
fn build_data(meta: &Value, samples: &[Value]) -> Value {
    let metrics_meta = meta.get("metrics").and_then(Value::as_array);
    let sample = samples.last();

    let mut load = json!({});
    let mut cpu = json!({});
    let mut memory = json!({});
    let mut disk = json!({});
    let mut network = json!([]);

    if let (Some(meta_arr), Some(sample_arr)) = (metrics_meta, sample.and_then(Value::as_array)) {
        for (i, metric_meta) in meta_arr.iter().enumerate() {
            let name = metric_meta
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("");
            let value = sample_arr.get(i);

            match name {
                "kernel.all.load" => {
                    let instances = metric_meta.get("instances").and_then(Value::as_array);
                    if let (Some(insts), Some(Value::Array(vals))) = (instances, value) {
                        for (j, inst) in insts.iter().enumerate() {
                            let key = inst.as_str().unwrap_or("").replace(' ', "_");
                            if let Some(v) = vals.get(j) {
                                load[&key] = v.clone();
                            }
                        }
                    }
                }
                "kernel.all.cpu.user" => {
                    cpu["user_ms_per_s"] = value.cloned().unwrap_or(Value::Null);
                }
                "kernel.all.cpu.sys" => {
                    cpu["system_ms_per_s"] = value.cloned().unwrap_or(Value::Null);
                }
                "kernel.all.cpu.idle" => {
                    cpu["idle_ms_per_s"] = value.cloned().unwrap_or(Value::Null);
                }
                "mem.physmem" => {
                    memory["total_kb"] = value.cloned().unwrap_or(Value::Null);
                }
                "mem.util.available" => {
                    memory["available_kb"] = value.cloned().unwrap_or(Value::Null);
                }
                "disk.all.total" => {
                    disk["iops"] = value.cloned().unwrap_or(Value::Null);
                }
                "network.interface.total.bytes" => {
                    let instances = metric_meta.get("instances").and_then(Value::as_array);
                    if let (Some(insts), Some(Value::Array(vals))) = (instances, value) {
                        let mut ifaces = Vec::new();
                        for (j, inst) in insts.iter().enumerate() {
                            let iface_name = inst.as_str().unwrap_or("unknown");
                            let bytes_per_s = vals.get(j).cloned().unwrap_or(Value::Null);
                            ifaces.push(json!({
                                "interface": iface_name,
                                "bytes_per_s": bytes_per_s,
                            }));
                        }
                        network = json!(ifaces);
                    }
                }
                _ => {}
            }
        }
    }

    // Compute memory usage percentage
    if let (Some(total), Some(avail)) = (
        memory.get("total_kb").and_then(Value::as_f64),
        memory.get("available_kb").and_then(Value::as_f64),
    ) {
        if total > 0.0 {
            let used_pct = ((total - avail) / total) * 100.0;
            memory["used_percent"] = json!((used_pct * 10.0).round() / 10.0);
        }
    }

    // Compute CPU usage percentage (user+sys out of user+sys+idle)
    if let (Some(user), Some(sys), Some(idle)) = (
        cpu.get("user_ms_per_s").and_then(Value::as_f64),
        cpu.get("system_ms_per_s").and_then(Value::as_f64),
        cpu.get("idle_ms_per_s").and_then(Value::as_f64),
    ) {
        let total = user + sys + idle;
        if total > 0.0 {
            let used_pct = ((user + sys) / total) * 100.0;
            cpu["used_percent"] = json!((used_pct * 10.0).round() / 10.0);
        }
    }

    json!({
        "load": load,
        "cpu": cpu,
        "memory": memory,
        "disk": disk,
        "network": network,
    })
}

/// Render a human-readable performance summary.
fn render_human(data: &Value) -> String {
    let mut lines = Vec::new();

    // Load
    if let Some(load) = data.get("load") {
        let l1 = load.get("1_minute").and_then(Value::as_f64);
        let l5 = load.get("5_minute").and_then(Value::as_f64);
        let l15 = load.get("15_minute").and_then(Value::as_f64);
        if let (Some(l1), Some(l5), Some(l15)) = (l1, l5, l15) {
            lines.push(format!("Load average: {l1:.2}, {l5:.2}, {l15:.2}"));
        }
    }

    // CPU
    if let Some(cpu) = data.get("cpu") {
        if let Some(pct) = cpu.get("used_percent").and_then(Value::as_f64) {
            lines.push(format!("CPU: {pct:.1}% used"));
        }
    }

    // Memory
    if let Some(mem) = data.get("memory") {
        let total = mem.get("total_kb").and_then(Value::as_f64);
        let avail = mem.get("available_kb").and_then(Value::as_f64);
        let pct = mem.get("used_percent").and_then(Value::as_f64);
        if let (Some(t), Some(a), Some(p)) = (total, avail, pct) {
            let total_gb = t / 1_048_576.0;
            let avail_gb = a / 1_048_576.0;
            lines.push(format!(
                "Memory: {p:.1}% used ({avail_gb:.1} GB available / {total_gb:.1} GB total)"
            ));
        }
    }

    // Disk
    if let Some(disk) = data.get("disk") {
        if let Some(iops) = disk.get("iops").and_then(Value::as_f64) {
            lines.push(format!("Disk I/O: {iops:.1} ops/s"));
        }
    }

    // Network — show top 5 by traffic, skip lo
    if let Some(Value::Array(ifaces)) = data.get("network") {
        let mut active: Vec<(&str, f64)> = ifaces
            .iter()
            .filter_map(|iface| {
                let name = iface.get("interface").and_then(Value::as_str)?;
                let bps = iface.get("bytes_per_s").and_then(Value::as_f64)?;
                if name == "lo" {
                    return None;
                }
                Some((name, bps))
            })
            .collect();
        active.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        if !active.is_empty() {
            lines.push("Network:".to_string());
            for (name, bps) in active.iter().take(5) {
                let display = format_bytes_rate(*bps);
                lines.push(format!("  {name}: {display}"));
            }
            let remaining = active.len().saturating_sub(5);
            if remaining > 0 {
                lines.push(format!("  ... and {remaining} more interfaces"));
            }
        }
    }

    lines.join("\n")
}

/// Format bytes/s as a human-readable rate.
fn format_bytes_rate(bps: f64) -> String {
    if bps >= 1_073_741_824.0 {
        format!("{:.1} GB/s", bps / 1_073_741_824.0)
    } else if bps >= 1_048_576.0 {
        format!("{:.1} MB/s", bps / 1_048_576.0)
    } else if bps >= 1024.0 {
        format!("{:.1} KB/s", bps / 1024.0)
    } else {
        format!("{:.0} B/s", bps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Simulate a realistic PCP meta + samples pair and verify `build_data` output.
    #[test]
    fn build_data_parses_snapshot() {
        let meta = json!({
            "metrics": [
                {"name": "kernel.all.load", "instances": ["1 minute", "5 minute", "15 minute"]},
                {"name": "kernel.all.cpu.user"},
                {"name": "kernel.all.cpu.sys"},
                {"name": "kernel.all.cpu.idle"},
                {"name": "mem.physmem"},
                {"name": "mem.util.available"},
                {"name": "disk.all.total"},
                {"name": "network.interface.total.bytes", "instances": ["lo", "eth0"]},
                {"name": "some.future.metric"},
            ]
        });
        // Sample 0: rate metrics return false
        let sample0 = json!([
            [0.5, 0.3, 0.2],
            false,
            false,
            false,
            16384000,
            8192000,
            false,
            [false, false],
            false
        ]);
        // Sample 1: rate metrics have values
        let sample1 = json!([
            [0.5, 0.3, 0.2],
            250.0,
            50.0,
            700.0,
            16384000,
            8192000,
            42.5,
            [1024.0, 5120.0],
            99.0
        ]);

        let data = build_data(&meta, &[sample0, sample1]);

        assert_eq!(data["load"]["1_minute"], 0.5);
        assert_eq!(data["load"]["5_minute"], 0.3);
        assert_eq!(data["load"]["15_minute"], 0.2);
        assert_eq!(data["cpu"]["user_ms_per_s"], 250.0);
        assert_eq!(data["cpu"]["system_ms_per_s"], 50.0);
        assert_eq!(data["cpu"]["idle_ms_per_s"], 700.0);
        assert_eq!(data["cpu"]["used_percent"], 30.0);
        assert_eq!(data["memory"]["total_kb"], 16384000);
        assert_eq!(data["memory"]["available_kb"], 8192000);
        assert_eq!(data["memory"]["used_percent"], 50.0);
        assert_eq!(data["disk"]["iops"], 42.5);
        assert_eq!(data["network"][0]["interface"], "lo");
        assert_eq!(data["network"][1]["interface"], "eth0");
        assert_eq!(data["network"][1]["bytes_per_s"], 5120.0);
    }

    #[test]
    fn build_data_handles_empty_samples() {
        let meta = json!({"metrics": []});
        let data = build_data(&meta, &[]);
        assert!(data["load"].is_object());
        assert!(data["network"].is_array());
    }

    #[test]
    fn render_human_formats_output() {
        let data = json!({
            "load": {"1_minute": 1.5, "5_minute": 1.2, "15_minute": 0.8},
            "cpu": {"user_ms_per_s": 250.0, "system_ms_per_s": 50.0, "idle_ms_per_s": 700.0, "used_percent": 30.0},
            "memory": {"total_kb": 16777216, "available_kb": 8388608, "used_percent": 50.0},
            "disk": {"iops": 42.5},
            "network": [
                {"interface": "lo", "bytes_per_s": 100.0},
                {"interface": "eth0", "bytes_per_s": 5120.0}
            ],
        });
        let human = render_human(&data);
        assert!(human.contains("Load average: 1.50, 1.20, 0.80"));
        assert!(human.contains("CPU: 30.0% used"));
        assert!(human.contains("Memory: 50.0% used"));
        assert!(human.contains("Disk I/O: 42.5 ops/s"));
        assert!(human.contains("eth0: 5.0 KB/s"));
        assert!(!human.contains("lo:"), "loopback should be filtered");
    }

    #[test]
    fn render_human_truncates_long_interface_list() {
        let mut ifaces = vec![json!({"interface": "lo", "bytes_per_s": 100.0})];
        for i in 0..8 {
            ifaces.push(
                json!({"interface": format!("eth{i}"), "bytes_per_s": (8 - i) as f64 * 1000.0}),
            );
        }
        let data = json!({
            "load": {}, "cpu": {}, "memory": {}, "disk": {},
            "network": ifaces,
        });
        let human = render_human(&data);
        assert!(human.contains("eth0:"), "top interface shown");
        assert!(human.contains("... and 3 more interfaces"));
        assert!(!human.contains("lo:"), "loopback filtered");
    }

    #[test]
    fn format_bytes_rate_scales() {
        assert_eq!(format_bytes_rate(500.0), "500 B/s");
        assert_eq!(format_bytes_rate(2048.0), "2.0 KB/s");
        assert_eq!(format_bytes_rate(2_097_152.0), "2.0 MB/s");
        assert_eq!(format_bytes_rate(2_147_483_648.0), "2.0 GB/s");
    }
}
