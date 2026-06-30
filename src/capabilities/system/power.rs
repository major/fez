//! Power actions (reboot, poweroff, suspend) via logind.
//!
//! All are privileged mutations requiring `--force` and cockpit escalation.
//! `CanX` is checked first to surface "not available" before attempting.

use crate::audit;
use crate::capabilities::View;
use crate::error::{FezError, Result};
use crate::protocol::client::BridgeClient;
use serde_json::json;

const LOGIN1_NAME: &str = "org.freedesktop.login1";
const LOGIN1_PATH: &str = "/org/freedesktop/login1";
const LOGIN1_IFACE: &str = "org.freedesktop.login1.Manager";

/// Execute a power action after safety checks.
pub(super) fn run(
    client: &mut BridgeClient,
    host: &str,
    action: &str,
    force: bool,
) -> Result<View> {
    // Protected-op gate: require --force
    if !force {
        return Err(FezError::Protected {
            unit: format!("system {action} (add --force to confirm)"),
        });
    }

    // Check capability first (unprivileged)
    let can_method = match action {
        "reboot" => "CanReboot",
        "poweroff" => "CanPowerOff",
        "suspend" => "CanSuspend",
        _ => unreachable!(),
    };
    let channel = client.dbus_open(LOGIN1_NAME)?;
    let out = client.dbus_call(&channel, LOGIN1_PATH, LOGIN1_IFACE, can_method, json!([]))?;
    let answer = out.get(0).and_then(|v| v.as_str()).unwrap_or("na");

    if answer == "na" {
        return Err(FezError::DependencyMissing {
            component: format!("logind {action}"),
            dbus_name: LOGIN1_NAME.into(),
            remediation: format!("This host does not support {action} via logind"),
        });
    }

    // Escalate and execute
    let exec_method = match action {
        "reboot" => "Reboot",
        "poweroff" => "PowerOff",
        "suspend" => "Suspend",
        _ => unreachable!(),
    };
    let priv_channel = client.dbus_open_privileged(LOGIN1_NAME)?;
    let host_owned = host.to_string();
    let operation = format!("system-{action}");
    audit::run_audited(&host_owned, &operation, action, || {
        client.dbus_call(
            &priv_channel,
            LOGIN1_PATH,
            LOGIN1_IFACE,
            exec_method,
            json!([true]),
        )?;
        Ok(())
    })?;

    let data = json!({"action": action, "host": host});
    let human = format!("{action} initiated on {host}\n");
    Ok(View::new("PowerAction", host, data, human))
}
