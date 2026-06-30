//! Canned RHSM (subscription-manager) replies for subscription tests.

use super::{err_reply, ok_reply};
use serde_json::{json, Value};

const CONSUMER_PATH: &str = "/com/redhat/RHSM1/Consumer";
const ENTITLEMENT_PATH: &str = "/com/redhat/RHSM1/Entitlement";
const PRODUCTS_PATH: &str = "/com/redhat/RHSM1/Products";
const SYSPURPOSE_PATH: &str = "/com/redhat/RHSM1/Syspurpose";

/// Canned reply for RHSM D-Bus calls.
pub(super) fn rhsm_reply(
    path: &str,
    _iface: &str,
    method: &str,
    _args: &[Value],
    id: &Value,
) -> Value {
    match (path, method) {
        (CONSUMER_PATH, "GetUuid") => ok_reply(id, json!(["12345678-abcd-1234-abcd-123456789abc"])),
        (ENTITLEMENT_PATH, "GetStatus") => ok_reply(id, json!(["Current"])),
        (PRODUCTS_PATH, "ListInstalledProducts") => {
            let products_json = serde_json::to_string(&json!([
                {
                    "id": "479",
                    "name": "Red Hat Enterprise Linux for x86_64",
                    "version": "10",
                    "arch": "x86_64",
                    "status": "Subscribed",
                }
            ]))
            .expect("serialize products");
            ok_reply(id, json!([products_json]))
        }
        (SYSPURPOSE_PATH, "GetSyspurpose") => ok_reply(
            id,
            json!(["{\"role\":\"Red Hat Enterprise Linux Server\"}"]),
        ),
        _ => err_reply(
            id,
            "org.freedesktop.DBus.Error.UnknownMethod",
            format!("no RHSM fake for {path} {method}"),
        ),
    }
}
