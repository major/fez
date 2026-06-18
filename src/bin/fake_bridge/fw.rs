//! Canned firewalld (`org.fedoraproject.FirewallD1`) replies.

use super::{err_reply, ok_reply};
use serde_json::{json, Value};

/// firewalld root object path.
pub(super) const FW_PATH: &str = "/org/fedoraproject/FirewallD1";
/// firewalld permanent-config object path.
const FW_CONFIG_PATH: &str = "/org/fedoraproject/FirewallD1/config";
/// Root firewalld interface (runtime ops; polkit `info` action = unprivileged).
const FW_IFACE: &str = "org.fedoraproject.FirewallD1";
/// Runtime zone interface (owns `getZones`/`getServices`/`getPorts` per zone).
const FW_ZONE_IFACE: &str = "org.fedoraproject.FirewallD1.zone";
/// Permanent-config interface (owns `getZoneByName`/`getZoneNames`).
const FW_CONFIG_IFACE: &str = "org.fedoraproject.FirewallD1.config";
/// Permanent-config per-zone interface (owns the config-zone getters/setters).
const FW_CONFIG_ZONE_IFACE: &str = "org.fedoraproject.FirewallD1.config.zone";

/// Canned firewalld reply.
///
/// Dispatches by object path **and interface**, mirroring real firewalld: a
/// method called on the wrong interface yields `UnknownMethod`.
///
/// Seeds a `public`/`internal`/`drop` topology where runtime `public` carries
/// `9090/tcp` that permanent `public` lacks, so drift is non-empty out of the
/// box. Runtime `public` masquerade is likewise seeded on while permanent is
/// off.
pub(super) fn fw_reply(
    path: &str,
    iface: &str,
    method: &str,
    args: &[Value],
    on_privileged: bool,
    id: &Value,
) -> Value {
    if std::env::var_os("FEZ_FAKE_NO_FIREWALLD").is_some() {
        return err_reply(
            id,
            "org.freedesktop.DBus.Error.ServiceUnknown",
            "The name org.fedoraproject.FirewallD1 was not provided by any .service files".into(),
        );
    }
    if path == FW_CONFIG_PATH && std::env::var_os("FEZ_FAKE_CONFIG_UNKNOWN_METHOD").is_some() {
        return fw_unknown(method, id);
    }
    if path.starts_with(FW_CONFIG_PATH) && std::env::var_os("FEZ_FAKE_CONFIG_INFO_DENIED").is_some()
    {
        return fw_config_info_denied(id);
    }
    // Permanent-config per-zone object: /config/zone/<n>.
    if path.starts_with(&format!("{FW_CONFIG_PATH}/zone/")) {
        if !on_privileged {
            return fw_access_denied(id);
        }
        return match (iface, method) {
            (FW_CONFIG_ZONE_IFACE, "getServices") => {
                ok_reply(id, json!([["ssh", "dhcpv6-client"]]))
            }
            (FW_CONFIG_ZONE_IFACE, "getPorts") => ok_reply(id, json!([[]])),
            (FW_CONFIG_ZONE_IFACE, "getMasquerade") => ok_reply(id, json!([false])),
            (_, other) => fw_unknown(other, id),
        };
    }
    if path == FW_CONFIG_PATH {
        if !on_privileged {
            return fw_access_denied(id);
        }
        return match (iface, method) {
            (FW_CONFIG_IFACE, "getZoneByName") => {
                ok_reply(id, json!([format!("{FW_CONFIG_PATH}/zone/0")]))
            }
            (_, other) => fw_unknown(other, id),
        };
    }
    // Main object — dispatch on (interface, method).
    let zone = args.first().and_then(Value::as_str).unwrap_or("");
    match (iface, method) {
        (FW_IFACE, "getDefaultZone") => ok_reply(id, json!(["public"])),
        (FW_IFACE, "listServices") => ok_reply(
            id,
            json!([["ssh", "http", "https", "cockpit", "dhcpv6-client"]]),
        ),
        (FW_IFACE, "queryPanicMode") => {
            ok_reply(id, json!([std::env::var_os("FEZ_FAKE_PANIC").is_some()]))
        }
        (FW_ZONE_IFACE, "getZones") => ok_reply(id, json!([["public", "internal", "drop"]])),
        (FW_ZONE_IFACE, "getServices") => ok_reply(id, json!([["ssh", "dhcpv6-client"]])),
        (FW_ZONE_IFACE, "getPorts") => {
            let removed = std::env::var_os("FEZ_FAKE_PORT_REMOVED").is_some();
            if zone == "public" && !removed {
                ok_reply(id, json!([[["9090", "tcp"]]]))
            } else {
                ok_reply(id, json!([[]]))
            }
        }
        (FW_ZONE_IFACE, "getInterfaces") => {
            if zone == "public" {
                ok_reply(id, json!([["enp1s0"]]))
            } else {
                ok_reply(id, json!([[]]))
            }
        }
        (FW_ZONE_IFACE, "getSources") => ok_reply(id, json!([[]])),
        (FW_ZONE_IFACE, "getMasquerade") => {
            if std::env::var_os("FEZ_FAKE_NO_MASQUERADE").is_some() {
                return fw_unknown("getMasquerade", id);
            }
            if zone == "public" {
                ok_reply(id, json!([true]))
            } else {
                ok_reply(id, json!([false]))
            }
        }
        (
            FW_ZONE_IFACE,
            "addService" | "removeService" | "addPort" | "removePort" | "addMasquerade"
            | "removeMasquerade",
        ) => ok_reply(id, json!([zone])),
        (FW_IFACE, "setDefaultZone" | "reload" | "runtimeToPermanent")
        | (FW_IFACE, "enablePanicMode" | "disablePanicMode") => ok_reply(id, json!([])),
        (_, other) => fw_unknown(other, id),
    }
}

fn fw_unknown(method: &str, id: &Value) -> Value {
    err_reply(
        id,
        "org.freedesktop.DBus.Error.UnknownMethod",
        format!("no firewalld fake for {method}"),
    )
}

fn fw_access_denied(id: &Value) -> Value {
    err_reply(
        id,
        "org.freedesktop.DBus.Error.AccessDenied",
        "permanent config read requires authorization (PK_ACTION_CONFIG)".into(),
    )
}

fn fw_config_info_denied(id: &Value) -> Value {
    err_reply(
        id,
        "org.fedoraproject.FirewallD1.NotAuthorizedException",
        "Not Authorized(polkit): org.fedoraproject.FirewallD1.config.info".into(),
    )
}
