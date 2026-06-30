//! RHEL subscription status via RHSM D-Bus interface.
//!
//! Reads consumer UUID, entitlement status, installed products, and syspurpose.
//! RHSM methods return JSON-encoded strings; the inner JSON is parsed.
//! Absent on Fedora — returns exit 9 with remediation.

use crate::capabilities::View;
use crate::error::{is_service_unknown, FezError, Result};
use crate::protocol::client::BridgeClient;
use serde_json::{json, Value};

const RHSM_NAME: &str = "com.redhat.RHSM1";

const CONSUMER_PATH: &str = "/com/redhat/RHSM1/Consumer";
const CONSUMER_IFACE: &str = "com.redhat.RHSM1.Consumer";

const ENTITLEMENT_PATH: &str = "/com/redhat/RHSM1/Entitlement";
const ENTITLEMENT_IFACE: &str = "com.redhat.RHSM1.Entitlement";

const PRODUCTS_PATH: &str = "/com/redhat/RHSM1/Products";
const PRODUCTS_IFACE: &str = "com.redhat.RHSM1.Products";

const SYSPURPOSE_PATH: &str = "/com/redhat/RHSM1/Syspurpose";
const SYSPURPOSE_IFACE: &str = "com.redhat.RHSM1.Syspurpose";

fn dependency_missing() -> FezError {
    FezError::DependencyMissing {
        component: "subscription-manager".into(),
        dbus_name: RHSM_NAME.into(),
        remediation: "Install subscription-manager (RHEL only)".into(),
    }
}

/// Map RHSM-specific errors to actionable failures.
///
/// RHSM is D-Bus-activated, so an absent service manifests as:
/// - `Dbus { ServiceUnknown }`: name not activatable.
/// - `Problem("not-found")`: cockpit closed the channel (service unreachable).
///
/// Both map to [`dependency_missing`] with RHEL-only remediation.
fn map_rhsm_error(e: FezError) -> FezError {
    match e {
        FezError::Dbus { ref name, .. } if is_service_unknown(name) => dependency_missing(),
        FezError::Problem(ref p) if p == "not-found" || p == "not-supported" => {
            dependency_missing()
        }
        other => other,
    }
}

/// Gather subscription status and return a SubscriptionStatus view.
pub(super) fn show(client: &mut BridgeClient, host: &str) -> Result<View> {
    let channel = client.dbus_open(RHSM_NAME).map_err(map_rhsm_error)?;

    let uuid = get_string(
        client,
        &channel,
        CONSUMER_PATH,
        CONSUMER_IFACE,
        "GetUuid",
        json!([""]),
    )?;
    let status = get_string(
        client,
        &channel,
        ENTITLEMENT_PATH,
        ENTITLEMENT_IFACE,
        "GetStatus",
        json!(["", ""]),
    )?;
    let products_raw = get_string(
        client,
        &channel,
        PRODUCTS_PATH,
        PRODUCTS_IFACE,
        "ListInstalledProducts",
        json!(["", {}, ""]),
    )?;
    let syspurpose_raw = get_string(
        client,
        &channel,
        SYSPURPOSE_PATH,
        SYSPURPOSE_IFACE,
        "GetSyspurpose",
        json!([""]),
    )?;

    let products: Value = serde_json::from_str(&products_raw).unwrap_or(json!([]));
    let syspurpose: Value = serde_json::from_str(&syspurpose_raw).unwrap_or(json!({}));

    let data = json!({
        "consumer_uuid": uuid,
        "status": status,
        "installed_products": products,
        "syspurpose": syspurpose,
    });
    let human = render_human(&uuid, &status, &products, &syspurpose);
    Ok(View::new("SubscriptionStatus", host, data, human))
}

/// Call an RHSM method and extract the first string return value.
fn get_string(
    client: &mut BridgeClient,
    channel: &str,
    path: &str,
    iface: &str,
    method: &str,
    args: Value,
) -> Result<String> {
    let out = client
        .dbus_call(channel, path, iface, method, args)
        .map_err(map_rhsm_error)?;
    out.get(0)
        .and_then(Value::as_str)
        .map(String::from)
        .ok_or_else(|| FezError::Problem(format!("RHSM {method} returned no string")))
}

fn render_human(uuid: &str, status: &str, products: &Value, syspurpose: &Value) -> String {
    let mut s = String::new();
    s.push_str("Subscription\n");
    s.push_str(&format!("  {:<20} {status}\n", "Status"));
    s.push_str(&format!("  {:<20} {uuid}\n", "Consumer UUID"));

    if let Some(role) = syspurpose.get("role").and_then(Value::as_str) {
        s.push_str(&format!("  {:<20} {role}\n", "System purpose"));
    }

    if let Some(prods) = products.as_array() {
        s.push_str("\nInstalled Products\n");
        for p in prods {
            let name = p["name"].as_str().unwrap_or("unknown");
            let version = p["version"].as_str().unwrap_or("");
            let prod_status = p["status"].as_str().unwrap_or("");
            s.push_str(&format!("  {name} {version} ({prod_status})\n"));
        }
    }
    s
}
