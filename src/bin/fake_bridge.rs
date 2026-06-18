//! Minimal cockpit-bridge stand-in for fez integration tests.
use fez::protocol::frame::{read_frame, write_frame, Frame};
use serde_json::{json, Value};
use std::io::{self, Write};

/// Fixed dnf5daemon session path the fake hands back from `open_session`.
///
/// Real dnf5daemon returns a dynamic, server-allocated path; the fake pins a
/// constant. Production code reads the returned path from `open_session`'s
/// reply and reuses it for subsequent calls, so it works against either.
const SESSION_PATH: &str = "/org/rpm/dnf/v0/session/fake";

/// Build a dnf5daemon package/repo `a{sv}` attribute map, mirroring the
/// systemd `GetAll` variant-wrapping (`{"t":<sig>,"v":<value>}`) so the dnf
/// reply value representation matches the rest of the fake exactly.
fn dnf_package(name: &str, evr: &str, arch: &str, repo_id: &str, install_size: u64) -> Value {
    json!({
        "name":         {"t":"s","v":name},
        "evr":          {"t":"s","v":evr},
        "arch":         {"t":"s","v":arch},
        "repo_id":      {"t":"s","v":repo_id},
        "install_size": {"t":"t","v":install_size},
        "summary":      {"t":"s","v":format!("{name} package")},
    })
}

/// Reject an `a{sv}` options dict whose values are not variant-wrapped.
///
/// Real cockpit-bridge marshals every value of an `a{sv}` argument as a D-Bus
/// variant, which on the wire is an explicit `{"t":<sig>,"v":<value>}` object.
/// A bare JSON scalar makes the marshaller raise `'bool' object is not
/// subscriptable` (or the type-specific equivalent). The fake mirrors that so
/// the integration tests catch a regression where fez sends bare scalars.
///
/// Returns `Some(error_reply)` when a bare value is found, `None` otherwise.
fn reject_unwrapped_options(args: &[Value], id: &Value) -> Option<Value> {
    let opts = args.last()?.as_object()?;
    for (key, val) in opts {
        let wrapped = val
            .as_object()
            .is_some_and(|o| o.contains_key("t") && o.contains_key("v"));
        if !wrapped {
            return Some(json!({"error":[
                "org.freedesktop.DBus.Error.InvalidArgs",
                [format!(
                    "a{{sv}} value for key {key:?} is not a variant ({{\"t\",\"v\"}}); \
                     cockpit-bridge would raise a marshalling TypeError"
                )]
            ],"id": id}));
        }
    }
    None
}

/// Canned reply for a dnf5daemon (`org.rpm.dnf.v0`) method.
///
/// Split out from the systemd match so the caller can validate the `a{sv}`
/// options argument (via [`reject_unwrapped_options`]) before dispatching.
fn dnf_reply(method: &str, iface: &str, id: &Value) -> Value {
    const SERVICE_UNKNOWN: &str = "org.freedesktop.DBus.Error.ServiceUnknown";
    const UNKNOWN_METHOD: &str = "org.freedesktop.DBus.Error.UnknownMethod";
    match method {
        // SessionManager.open_session -> (session_object_path).
        // FEZ_FAKE_NO_DNF5 simulates the daemon being absent: the bus name
        // fails to activate, yielding ServiceUnknown.
        "open_session" => {
            if std::env::var_os("FEZ_FAKE_NO_DNF5").is_some() {
                err_reply(id, SERVICE_UNKNOWN, "The name org.rpm.dnf.v0 was not provided by any .service files".into())
            } else {
                ok_reply(id, json!([SESSION_PATH]))
            }
        }
        // rpm.Repo.list(options) -> (repositories). Shares the method name
        // `list` with Rpm.list; disambiguated by iface.
        "list" if iface.ends_with(".rpm.Repo") => ok_reply(id, json!([[
            dnf_repo("fedora", "Fedora", true),
            dnf_repo("updates-testing", "Fedora - Testing", false),
        ]])),
        // rpm.Rpm.list(options) -> (packages).
        //
        // The daemon's Rpm.list has no server-side repo filter (only `install`
        // /`upgrade` take `repo_ids`, for resolution), so the fake always
        // returns the full multi-repo set and the client filters by `repo_id`.
        // `vim` lives in `updates` so `--repo fedora` must drop it and
        // `--repo updates` must keep only it (issue #59).
        "list" => {
            let packages = match std::env::var("FEZ_FAKE_PACKAGE_COUNT")
                .ok()
                .and_then(|s| s.parse::<usize>().ok())
            {
                Some(count) => (0..count)
                    .map(|i| dnf_package(&format!("pkg{i:04}"), "1.0-1.fc40", "x86_64", "fedora", 1024))
                    .collect(),
                None => vec![
                    dnf_package("bash", "5.2.26-1.fc40", "x86_64", "fedora", 7_340_032),
                    dnf_package("htop", "3.3.0-1.fc40", "x86_64", "fedora", 245_760),
                    dnf_package("nginx", "1.24.0-7.fc40", "x86_64", "fedora", 1_572_864),
                    dnf_package("vim-enhanced", "9.1.0-1.fc40", "x86_64", "updates", 3_145_728),
                ],
            };
            ok_reply(id, json!([packages]))
        }
        // Staging calls: install/remove/upgrade return nothing.
        "install" | "remove" | "upgrade" => ok_reply(id, json!([])),
        // Goal.resolve(options) -> (transaction_items, result). result 0 == no problems.
        "resolve" => ok_reply(id, json!([fake_resolve_items(), 0])),
        // Goal.do_transaction(options) -> ().
        "do_transaction" => ok_reply(id, json!([])),
        other => err_reply(id, UNKNOWN_METHOD, format!("no fake for {other}")),
    }
}

/// Build a dnf5daemon repository `a{sv}` attribute map.
fn dnf_repo(id: &str, name: &str, enabled: bool) -> Value {
    json!({
        "id":      {"t":"s","v":id},
        "name":    {"t":"s","v":name},
        "enabled": {"t":"b","v":enabled},
    })
}

/// Build the `Goal.resolve` `transaction_items` array, keyed by the
/// `FEZ_FAKE_PLAN` env var so one fake serves every guardrail test case.
///
/// Each item is the tuple `(object_type, action, reason, item_attrs, object)`
/// where `action` is a string (`"Install"`/`"Remove"`) and `object` is the
/// package `a{sv}` map. The Task 5 parser keys off `action` (index 1) and
/// `object` (index 4).
fn fake_resolve_items() -> Value {
    fn installed(name: &str) -> Value {
        json!([
            "Package",
            "Install",
            "User",
            {},
            dnf_package(name, "1.0-1.fc40", "x86_64", "fedora", 1024),
        ])
    }
    fn removed(name: &str) -> Value {
        json!([
            "Package",
            "Remove",
            "Dependency",
            {},
            dnf_package(name, "1.0-1.fc40", "x86_64", "@System", 1024),
        ])
    }
    match std::env::var("FEZ_FAKE_PLAN").as_deref() {
        Ok("protected") => json!([removed("glibc")]),
        Ok("cascade") => {
            // 21 = CASCADE_LIMIT (20) + 1, to trip the cascade guardrail.
            let items: Vec<Value> = (0..21).map(|i| removed(&format!("pkg{i}"))).collect();
            Value::Array(items)
        }
        // FEZ_FAKE_PLAN unset (the Err(_) case, the common path in tests)
        // defaults to an install plan.
        Ok("install") | Err(_) => json!([installed("htop")]),
        // Any unrecognized plan (including the documented "small" case) yields
        // a single non-protected removal (guardrails pass).
        Ok(_) => json!([removed("htop")]),
    }
}

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

/// NetworkManager root manager object path.
const NM_MGR_PATH: &str = "/org/freedesktop/NetworkManager";

/// Canned NetworkManager (`org.freedesktop.NetworkManager`) reply for a call
/// against a NM object path.
///
/// NM reuses the generic `Get`/`GetAll` property methods across many object
/// types, so the fake disambiguates by **object path** rather than method name
/// (unlike the systemd/dnf surfaces). Every `a{sv}`/variant value is
/// variant-wrapped (`{"t":<sig>,"v":<value>}`) exactly like real cockpit-bridge
/// and the systemd `GetAll` arm, so the client's unwrap path is exercised.
///
/// Canned topology (returned by `GetDevices`):
/// - `/Devices/1` `enp1s0`: ethernet (type 1), activated (100), managed; full
///   IPv4 (`/IP4Config/1`) + IPv6 (`/IP6Config/1`) + active connection
///   (`/ActiveConnection/1`) + DHCP lease (`/DHCP4Config/1`).
/// - `/Devices/2` `enp2s0`: ethernet (type 1), unavailable (20), managed; no IP
///   config (object path `"/"`) to exercise the null-config guard.
/// - `/Devices/3` `lo`: loopback (type 32), unmanaged (10); kept by the default
///   filter on device type even though it is unmanaged.
/// - `/Devices/9` `veth0`: veth (type 20), unmanaged (10); hidden by the default
///   filter, shown only with `--all`.
fn nm_reply(path: &str, method: &str, id: &Value) -> Value {
    // Manager: GetDevices -> (ao). Also answers Get/GetAll for global props,
    // but fez only calls GetDevices on the manager for these two actions.
    if path == NM_MGR_PATH {
        return match method {
            "GetDevices" => ok_reply(id, json!([[
                format!("{NM_MGR_PATH}/Devices/1"),
                format!("{NM_MGR_PATH}/Devices/2"),
                format!("{NM_MGR_PATH}/Devices/3"),
                format!("{NM_MGR_PATH}/Devices/9"),
            ]])),
            other => nm_unknown(other, id),
        };
    }
    // Device objects: GetAll -> a{sv} device properties.
    if let Some(n) = path.strip_prefix(&format!("{NM_MGR_PATH}/Devices/")) {
        return match method {
            "GetAll" => nm_device_props(n, id),
            other => nm_unknown(other, id),
        };
    }
    // IP4Config objects: GetAll -> AddressData/Gateway/NameserverData/Domains.
    if path.starts_with(&format!("{NM_MGR_PATH}/IP4Config/")) {
        return match method {
            "GetAll" => ok_reply(id, json!([{
                "AddressData": {"t":"aa{sv}","v":[
                    {"address":{"t":"s","v":"192.168.10.20"},"prefix":{"t":"u","v":24}}
                ]},
                "Gateway": {"t":"s","v":"192.168.10.1"},
                "NameserverData": {"t":"aa{sv}","v":[
                    {"address":{"t":"s","v":"192.168.10.1"}},
                    {"address":{"t":"s","v":"1.1.1.1"}}
                ]},
                "Domains": {"t":"as","v":["lan"]},
            }])),
            other => nm_unknown(other, id),
        };
    }
    // IP6Config objects: GetAll -> AddressData/Gateway.
    if path.starts_with(&format!("{NM_MGR_PATH}/IP6Config/")) {
        return match method {
            "GetAll" => ok_reply(id, json!([{
                "AddressData": {"t":"aa{sv}","v":[
                    {"address":{"t":"s","v":"fd00::20"},"prefix":{"t":"u","v":64}}
                ]},
                "Gateway": {"t":"s","v":"fd00::1"},
            }])),
            other => nm_unknown(other, id),
        };
    }
    // Active connection objects: GetAll -> Id/Type/Default.
    if path.starts_with(&format!("{NM_MGR_PATH}/ActiveConnection/")) {
        return match method {
            "GetAll" => ok_reply(id, json!([{
                "Id": {"t":"s","v":"enp1s0"},
                "Type": {"t":"s","v":"802-3-ethernet"},
                "Default": {"t":"b","v":true},
            }])),
            other => nm_unknown(other, id),
        };
    }
    // DHCP4 config objects: GetAll -> Options (a{sv}).
    if path.starts_with(&format!("{NM_MGR_PATH}/DHCP4Config/")) {
        return match method {
            "GetAll" => ok_reply(id, json!([{
                "Options": {"t":"a{sv}","v":{
                    "routers": {"t":"s","v":"192.168.10.1"},
                    "ip_address": {"t":"s","v":"192.168.10.20"},
                }},
            }])),
            other => nm_unknown(other, id),
        };
    }
    nm_unknown(method, id)
}

/// Build the `a{sv}` device property map for `Devices/<n>` GetAll.
fn nm_device_props(n: &str, id: &Value) -> Value {
    let nm = NM_MGR_PATH;
    // (interface, type, state, managed, ip4, ip6, active, dhcp4)
    let (iface, dtype, state, managed, ip4, ip6, active, dhcp4) = match n {
        "1" => (
            "enp1s0",
            1u32,
            100u32,
            true,
            format!("{nm}/IP4Config/1"),
            format!("{nm}/IP6Config/1"),
            format!("{nm}/ActiveConnection/1"),
            format!("{nm}/DHCP4Config/1"),
        ),
        "2" => (
            "enp2s0",
            1u32,
            20u32,
            true,
            ROOT_PATH.to_string(),
            ROOT_PATH.to_string(),
            ROOT_PATH.to_string(),
            ROOT_PATH.to_string(),
        ),
        "3" => (
            "lo",
            32u32,
            10u32,
            false,
            ROOT_PATH.to_string(),
            ROOT_PATH.to_string(),
            ROOT_PATH.to_string(),
            ROOT_PATH.to_string(),
        ),
        _ => (
            "veth0",
            20u32,
            10u32,
            false,
            ROOT_PATH.to_string(),
            ROOT_PATH.to_string(),
            ROOT_PATH.to_string(),
            ROOT_PATH.to_string(),
        ),
    };
    ok_reply(id, json!([{
        "Interface": {"t":"s","v":iface},
        "DeviceType": {"t":"u","v":dtype},
        "State": {"t":"u","v":state},
        "Managed": {"t":"b","v":managed},
        "HwAddress": {"t":"s","v":"52:54:00:12:34:56"},
        "Mtu": {"t":"u","v":1500},
        "Ip4Config": {"t":"o","v":ip4},
        "Ip6Config": {"t":"o","v":ip6},
        "ActiveConnection": {"t":"o","v":active},
        "Dhcp4Config": {"t":"o","v":dhcp4},
    }]))
}

/// Unknown-method D-Bus error for a NM call the fake does not model.
fn nm_unknown(method: &str, id: &Value) -> Value {
    err_reply(id, "org.freedesktop.DBus.Error.UnknownMethod", format!("no NM fake for {method}"))
}

/// firewalld root object path.
const FW_PATH: &str = "/org/fedoraproject/FirewallD1";
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

/// Canned firewalld (`org.fedoraproject.FirewallD1`) reply.
///
/// Dispatches by object path **and interface**, mirroring real firewalld: a
/// method called on the wrong interface yields `UnknownMethod`, the way a real
/// bridge would. (An earlier path-only fake masked a client that called
/// `getZones` on the root interface instead of the zone interface; see
/// issue #33.) It also enforces the polkit gate on the permanent `config.*`
/// interface: those reads are `auth_admin_keep` on both server and desktop
/// installs, so they are answered only on a privileged (escalated) channel and
/// otherwise closed `access-denied` (issue #34).
///
/// Seeds a `public`/`internal`/`drop` topology where runtime `public` carries
/// `9090/tcp` that permanent `public` lacks, so drift is non-empty out of the
/// box (`status` reports `+port 9090/tcp`). Runtime `public` masquerade is
/// likewise seeded on while permanent is off, so masquerade drift
/// (`+masquerade`) is non-empty out of the box alongside `9090/tcp`.
/// `FEZ_FAKE_PANIC` starts panic mode on; `FEZ_FAKE_NO_FIREWALLD` makes every
/// call report the service absent (ServiceUnknown); `FEZ_FAKE_PORT_REMOVED`
/// drops 9090/tcp from the runtime `public` zone to model the state after a
/// `remove-port`.
fn fw_reply(
    path: &str,
    iface: &str,
    method: &str,
    args: &[Value],
    on_privileged: bool,
    id: &Value,
) -> Value {
    if std::env::var_os("FEZ_FAKE_NO_FIREWALLD").is_some() {
        return err_reply(id, "org.freedesktop.DBus.Error.ServiceUnknown", "The name org.fedoraproject.FirewallD1 was not provided by any .service files".into());
    }
    if path == FW_CONFIG_PATH && std::env::var_os("FEZ_FAKE_CONFIG_UNKNOWN_METHOD").is_some() {
        return fw_unknown(method, id);
    }
    if path.starts_with(FW_CONFIG_PATH) && std::env::var_os("FEZ_FAKE_CONFIG_INFO_DENIED").is_some()
    {
        return fw_config_info_denied(id);
    }
    // Permanent-config per-zone object: /config/zone/<n>. Permanent `public`
    // (zone 0) lacks the runtime-only 9090/tcp, which is the seeded drift.
    // The config.zone interface is polkit-gated (PK_ACTION_CONFIG), so deny
    // the read unless it arrives on a privileged channel.
    if path.starts_with(&format!("{FW_CONFIG_PATH}/zone/")) {
        if !on_privileged {
            return fw_access_denied(id);
        }
        return match (iface, method) {
            (FW_CONFIG_ZONE_IFACE, "getServices") => ok_reply(id, json!([["ssh", "dhcpv6-client"]])),
            (FW_CONFIG_ZONE_IFACE, "getPorts") => ok_reply(id, json!([[]])),
            // Permanent `public` masquerade is off; runtime is on, so masquerade
            // drift is non-empty out of the box alongside the 9090/tcp port.
            (FW_CONFIG_ZONE_IFACE, "getMasquerade") => ok_reply(id, json!([false])),
            (_, other) => fw_unknown(other, id),
        };
    }
    if path == FW_CONFIG_PATH {
        if !on_privileged {
            return fw_access_denied(id);
        }
        return match (iface, method) {
            // getZoneByName(name) -> config zone object path.
            (FW_CONFIG_IFACE, "getZoneByName") => ok_reply(id, json!([format!("{FW_CONFIG_PATH}/zone/0")])),
            (_, other) => fw_unknown(other, id),
        };
    }
    // Main object. Zone-scoped methods take the zone name as the first arg.
    // Dispatch on (interface, method): `getZones`/`getServices`/`getPorts`/
    // `getInterfaces`/`getSources` live on the zone interface, while
    // `getDefaultZone`/`listServices`/`queryPanicMode` and the mutations live
    // on the root interface. A method invoked on the wrong interface is
    // UnknownMethod, exactly as real firewalld responds.
    let zone = args.first().and_then(Value::as_str).unwrap_or("");
    match (iface, method) {
        (FW_IFACE, "getDefaultZone") => ok_reply(id, json!(["public"])),
        (FW_IFACE, "listServices") => ok_reply(id, json!([["ssh", "http", "https", "cockpit", "dhcpv6-client"]])),
        (FW_IFACE, "queryPanicMode") => ok_reply(id, json!([std::env::var_os("FEZ_FAKE_PANIC").is_some()])),
        (FW_ZONE_IFACE, "getZones") => ok_reply(id, json!([["public", "internal", "drop"]])),
        // Runtime per-zone reads. `public` carries the drift port 9090/tcp,
        // unless FEZ_FAKE_PORT_REMOVED models the post-removal state where the
        // port is gone from the runtime zone (a follow-up read after
        // `remove-port 9090/tcp`).
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
            if zone == "public" { ok_reply(id, json!([["enp1s0"]])) } else { ok_reply(id, json!([[]])) }
        }
        (FW_ZONE_IFACE, "getSources") => ok_reply(id, json!([[]])),
        // Runtime per-zone masquerade. `public` is seeded on (permanent is off),
        // so masquerade drift is non-empty out of the box. FEZ_FAKE_NO_MASQUERADE
        // models an older firewalld that lacks the getMasquerade method: the call
        // returns UnknownMethod, exercising the unsupported-api mapping (#60).
        (FW_ZONE_IFACE, "getMasquerade") => {
            if std::env::var_os("FEZ_FAKE_NO_MASQUERADE").is_some() {
                return fw_unknown("getMasquerade", id);
            }
            if zone == "public" { ok_reply(id, json!([true])) } else { ok_reply(id, json!([false])) }
        }
        // Mutations return the affected zone name (or void for reload/confirm).
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

/// Unknown-method D-Bus error for a firewalld call the fake does not model
/// (also the response when a method is called on the wrong interface).
fn fw_unknown(method: &str, id: &Value) -> Value {
    err_reply(id, "org.freedesktop.DBus.Error.UnknownMethod", format!("no firewalld fake for {method}"))
}

/// Polkit-denied error for a permanent `config.*` read on an unprivileged
/// channel. Real firewalld raises an `AUTH_FAILED`/`NOT_AUTHORIZED` D-Bus
/// error; cockpit surfaces an unauthorized call by closing the channel
/// `access-denied`, which the client maps to [`FezError::AccessDenied`].
fn fw_access_denied(id: &Value) -> Value {
    err_reply(id, "org.freedesktop.DBus.Error.AccessDenied", "permanent config read requires authorization (PK_ACTION_CONFIG)".into())
}

/// Real-host firewalld error when the permanent config.info polkit action is
/// rejected even though the call arrived over a cockpit privileged channel.
fn fw_config_info_denied(id: &Value) -> Value {
    err_reply(id, "org.fedoraproject.FirewallD1.NotAuthorizedException", "Not Authorized(polkit): org.fedoraproject.FirewallD1.config.info".into())
}

/// The host's escalation mechanisms as modeled by `FEZ_FAKE_BRIDGES`.
///
/// Real cockpit-bridge exposes a `cockpit.Superuser.Bridges` property (the
/// ordered, validity-filtered mechanism names) and a `Start(name)` method that
/// brings up the named root peer. The fake models that surface so escalation
/// can be driven deterministically.
///
/// Grammar: comma-separated `name:outcome` pairs, outcome `ok` or `err`, e.g.
/// `sudo:ok`, `sudo:err,polkit:ok`. Order is preserved (it is the `Bridges`
/// order fez iterates). A bare `name` with no `:outcome` defaults to `ok`.
///
/// Default (var unset) models a normal passwordless-sudo host (`[("sudo",
/// true)]`), so the bulk of the integration tests escalate without ceremony.
/// An explicitly empty value (`FEZ_FAKE_BRIDGES=""`) means the host advertises
/// no mechanism, so privileged channels are denied: that is how the
/// escalation-failure cases opt in.
fn fake_bridges() -> Vec<(String, bool)> {
    let raw = match std::env::var("FEZ_FAKE_BRIDGES") {
        Ok(v) => v,
        // Unset: default to a passwordless-sudo host.
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

/// PackageKit root controller object path (carries `CreateTransaction`).
const PK_PATH: &str = "/org/freedesktop/PackageKit";
/// Canned PackageKit transaction object path the fake hands back.
const PK_TX_PATH: &str = "/org/freedesktop/PackageKit/Transaction/1";
/// PackageKit per-transaction interface (the signals carry this interface).
const PK_TX_IFACE: &str = "org.freedesktop.PackageKit.Transaction";

/// Emit a PackageKit `{"signal":[path, iface, member, args]}` data frame, the
/// way cockpit `dbus-json3` delivers a D-Bus signal on the channel.
fn send_signal(out: &mut impl Write, channel: &str, member: &str, args: &Value) {
    send_data(
        out,
        channel,
        &json!({ "signal": [PK_TX_PATH, PK_TX_IFACE, member, args] }),
    );
}

/// Emit the canned PackageKit signal stream for a transaction method.
///
/// PackageKit reports results as a stream of signals terminated by `Finished`,
/// not a method reply, so this writes the frames directly. Scenario knobs:
/// - `FEZ_FAKE_PK_PLAN=protected`: the `RemovePackages` plan includes a
///   protected package (`systemd`) so the removal guardrail (exit 8) fires.
/// - `FEZ_FAKE_PK_ERROR=notauth`: emit `ErrorCode(6, ...)` so the access-denied
///   (exit 11) mapping is exercised.
fn pk_emit(out: &mut impl Write, channel: &str, method: &str) {
    let installed = [
        (
            1u64,
            "bash;5.2.26-1.fc44;x86_64;installed",
            "GNU Bourne-Again SHell",
        ),
        (
            1u64,
            "htop;3.4.1-3.fc44;x86_64;installed",
            "Interactive process viewer",
        ),
    ];
    let nginx = "nginx;1.27.0-1.fc44;x86_64;fedora";
    match method {
        "GetPackages" => {
            for (info, pid, summ) in installed {
                send_signal(out, channel, "Package", &json!([info, pid, summ]));
            }
        }
        "GetUpdates" => {
            send_signal(
                out,
                channel,
                "Package",
                &json!([11, "htop;3.4.2-1.fc44;x86_64;updates", "Interactive process viewer"]),
            );
        }
        "SearchNames" => {
            send_signal(out, channel, "Package", &json!([2, nginx, "High performance web server"]));
        }
        "GetRepoList" => {
            send_signal(out, channel, "RepoDetail", &json!(["fedora", "Fedora 44", true]));
            send_signal(out, channel, "RepoDetail", &json!(["updates", "Fedora 44 updates", true]));
            send_signal(out, channel, "RepoDetail", &json!(["crb", "CRB", false]));
        }
        "Resolve" => {
            // Resolve a spec to a full package_id for info / mutation.
            send_signal(out, channel, "Package", &json!([2, nginx, "High performance web server"]));
        }
        "InstallPackages" | "UpdatePackages" => {
            // SIMULATE or real: report the install plan (target + a dep).
            send_signal(out, channel, "Package", &json!([12, nginx, "High performance web server"]));
            send_signal(
                out,
                channel,
                "Package",
                &json!([12, "nginx-core;1.27.0-1.fc44;x86_64;fedora", "nginx core files"]),
            );
        }
        "RemovePackages" => {
            send_signal(
                out,
                channel,
                "Package",
                &json!([13, "htop;3.4.1-3.fc44;x86_64;installed", "Interactive process viewer"]),
            );
            if std::env::var("FEZ_FAKE_PK_PLAN").as_deref() == Ok("protected") {
                send_signal(
                    out,
                    channel,
                    "Package",
                    &json!([13, "systemd;255-1.fc44;x86_64;installed", "System and Service Manager"]),
                );
            }
        }
        _ => {}
    }
    // Optional error injection before Finished (exit 4 = failed).
    if std::env::var("FEZ_FAKE_PK_ERROR").as_deref() == Ok("notauth") {
        send_signal(out, channel, "ErrorCode", &json!([6, "not authorized to perform operation"]));
        send_signal(out, channel, "Finished", &json!([4, 10]));
        return;
    }
    send_signal(out, channel, "Finished", &json!([1, 20])); // exit 1 = success
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
    // Tracks whether a cockpit.Superuser.Start has succeeded, i.e. a root peer
    // is "up". A `superuser: "require"` open succeeds only after that.
    let mut escalated = false;
    // Channels opened with `superuser: "require"` route to the root peer. The
    // firewalld `config.*` (permanent) interface is polkit-gated
    // (`auth_admin_keep` on both server and desktop installs), so a permanent
    // read must arrive on one of these privileged channels. Real firewalld
    // checks polkit on the caller; in cockpit's model that is the root peer,
    // i.e. a `superuser: "require"` channel. Reads on a plain channel are
    // denied. Without this set the fake would answer config reads unprivileged
    // and mask a client that forgets to escalate for the drift read.
    let mut privileged_channels: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    send_control(&mut stdout, &json!({"command":"init","version":1}));

    while let Some(frame) = read_frame(&mut stdin)? {
        if frame.channel.is_empty() {
            let ctrl: Value = serde_json::from_slice(&frame.payload).unwrap_or(Value::Null);
            let command = ctrl.get("command").and_then(Value::as_str);
            // The client's `init` carries `superuser: "none"`, so the bridge
            // brings up no root peer at init and just completes the handshake.
            // Escalation happens later, driven by the client via
            // cockpit.Superuser.Start over the internal bus.
            //
            // Real cockpit only runs superuser negotiation (and thus only emits
            // `superuser-init-done`) when init carries an escalation request,
            // i.e. `superuser` is an object or a string other than "none".
            // `SuperuserRoutingRule.init` is never invoked for `superuser:
            // "none"`, so no `superuser-init-done` is sent. Mirror that here so
            // the fake cannot mask a client that wrongly blocks on it.
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
            // close, done: ignore; only `open` needs a response.
            if let Some("open") = command {
                let channel = ctrl
                    .get("channel")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let payload = ctrl.get("payload").and_then(Value::as_str).unwrap_or("");
                let open_name = ctrl.get("name").and_then(Value::as_str).unwrap_or("");
                // FEZ_FAKE_FIREWALLD_UNREACHABLE models a real host where the
                // firewalld D-Bus name cannot be activated (absent or its unit
                // failed): cockpit-bridge closes the channel with `not-found`
                // rather than a Dbus error reply, which is the symptom in #60.
                if std::env::var_os("FEZ_FAKE_FIREWALLD_UNREACHABLE").is_some()
                    && open_name == "org.fedoraproject.FirewallD1"
                {
                    send_control(
                        &mut stdout,
                        &json!({"command":"close","channel":channel,"problem":"not-found"}),
                    );
                    continue;
                }
                // A privileged channel (`superuser: "require"`) the bridge
                // cannot route to root closes with `access-denied` instead of
                // `ready`: that means no cockpit.Superuser.Start has succeeded
                // yet (no root peer exists).
                let privileged = ctrl.get("superuser").and_then(Value::as_str) == Some("require");
                // FEZ_FAKE_DENY_PRIVILEGED models a host where escalation
                // succeeds but the sudoers/polkit policy still rejects the
                // specific privileged channel mid-operation: the bridge closes
                // it with access-denied even after a successful Start.
                let force_deny = std::env::var_os("FEZ_FAKE_DENY_PRIVILEGED").is_some();
                // A privileged channel routes to root, which only exists after a
                // successful cockpit.Superuser.Start (escalated).
                let deny_privileged = !escalated || force_deny;
                if privileged && deny_privileged {
                    send_control(
                        &mut stdout,
                        &json!({"command":"close","channel":channel,"problem":"access-denied"}),
                    );
                    continue;
                }
                if privileged {
                    privileged_channels.insert(channel.clone());
                }
                send_control(&mut stdout, &json!({"command":"ready","channel":channel}));
                if payload == "stream" {
                    let mut blob = serde_json::to_vec(&json!({
                        "__REALTIME_TIMESTAMP":"1700000000000000","PRIORITY":"6",
                        "SYSLOG_IDENTIFIER":"sshd","MESSAGE":"Server listening on port 22.","_PID":"1001"
                    })).unwrap();
                    blob.push(b'\n');
                    blob.extend_from_slice(
                        &serde_json::to_vec(&json!({
                            "__REALTIME_TIMESTAMP":"1700000001000000","PRIORITY":"6",
                            "SYSLOG_IDENTIFIER":"sshd","MESSAGE":"Accepted publickey for fedora","_PID":"1002"
                        }))
                        .unwrap(),
                    );
                    blob.push(b'\n');
                    write_frame(&mut stdout, &Frame::new(&channel, blob))?;
                    send_control(&mut stdout, &json!({"command":"done","channel":channel}));
                    send_control(&mut stdout, &json!({"command":"close","channel":channel}));
                }
            }
        } else {
            let msg: Value = serde_json::from_slice(&frame.payload).unwrap_or(Value::Null);
            if let Some(call) = msg.get("call").and_then(Value::as_array) {
                let id = msg.get("id").cloned().unwrap_or(json!("0"));
                // call = [path, interface, method, args]. The interface is
                // needed to disambiguate dnf5daemon's two `list` methods
                // (rpm.Rpm.list vs rpm.Repo.list) which share a method name.
                let path = call.first().and_then(Value::as_str).unwrap_or("");
                let iface = call.get(1).and_then(Value::as_str).unwrap_or("");
                let method = call.get(2).and_then(Value::as_str).unwrap_or("");
                let args = call
                    .get(3)
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                // dnf5daemon methods whose last argument is an a{sv} options
                // dict. cockpit-bridge marshals each value as a variant, so a
                // bare scalar is a wire error; reject it the way real bridge
                // would (see reject_unwrapped_options).
                let dnf_options_method = matches!(
                    method,
                    "open_session"
                        | "list"
                        | "install"
                        | "remove"
                        | "upgrade"
                        | "resolve"
                        | "do_transaction"
                );
                let reply = if path.starts_with(FW_PATH) {
                    // firewalld surface: dispatched by object path (the main
                    // object vs the config sub-object), like the NM arm. The
                    // single FW_PATH prefix catches FW_CONFIG_PATH too; fw_reply
                    // splits them internally. It also receives the interface
                    // (to reject methods called on the wrong interface, the way
                    // real firewalld does) and whether the channel is privileged
                    // (the permanent `config.*` interface is polkit-gated).
                    let on_privileged = privileged_channels.contains(&frame.channel);
                    fw_reply(path, iface, method, &args, on_privileged, &id)
                } else if path.starts_with(NM_MGR_PATH) {
                    // NetworkManager surface: disambiguated by object path, not
                    // method name (Get/GetAll are reused across object types).
                    nm_reply(path, method, &id)
                } else if path == PK_PATH && method == "CreateTransaction" {
                    // PackageKit root controller. FEZ_FAKE_NO_PACKAGEKIT models
                    // the daemon being absent (ServiceUnknown), so the "both
                    // backends missing" path is testable; otherwise hand back the
                    // canned transaction object path as a normal reply.
                    if std::env::var_os("FEZ_FAKE_NO_PACKAGEKIT").is_some() {
                        err_reply(&id, "org.freedesktop.DBus.Error.ServiceUnknown", "The name org.freedesktop.PackageKit was not provided by any .service files".into())
                    } else {
                        ok_reply(&id, json!([PK_TX_PATH]))
                    }
                } else if path == PK_TX_PATH {
                    // PackageKit transaction methods report via signals, not a
                    // reply. Emit the canned stream then continue (no reply
                    // frame: dbus_call_collect ignores the empty method reply).
                    pk_emit(&mut stdout, &frame.channel, method);
                    continue;
                } else if dnf_options_method {
                    if let Some(err) = reject_unwrapped_options(&args, &id) {
                        send_data(&mut stdout, &frame.channel, &err);
                        continue;
                    }
                    dnf_reply(method, iface, &id)
                } else {
                    match method {
                        // cockpit.Superuser.Bridges property read via
                        // org.freedesktop.DBus.Properties.Get(iface, "Bridges").
                        // `Properties.Get` returns a single `v` out-arg, so the
                        // `as` value is variant-wrapped: {"t":"as","v":[...]}.
                        // Real cockpit-bridge does NOT unwrap it; mirror that so
                        // the client's variant-unwrapping is exercised exactly as
                        // in production (same discipline as `GetAll` below).
                        "Get" => {
                            let names: Vec<Value> = bridges.iter().map(|(n, _)| json!(n)).collect();
                            ok_reply(&id, json!([{"t":"as","v":names}]))
                        }
                        // cockpit.Superuser.Start(name): bring up the named
                        // mechanism. `ok` succeeds (record escalated); `err`
                        // returns a D-Bus error (mirrors a mechanism whose
                        // credential prompt fez cannot answer).
                        "Start" => {
                            let name = args.first().and_then(Value::as_str).unwrap_or("");
                            match bridges.iter().find(|(n, _)| n == name) {
                                Some((_, true)) => {
                                    escalated = true;
                                    ok_reply(&id, json!([]))
                                }
                                _ => err_reply(&id, "cockpit.Superuser.Error", format!("mechanism {name:?} cannot start")),
                            }
                        }
                        // reply[0][0] = units array
                        "ListUnits" => ok_reply(&id, json!([[
                            ["sshd.service","OpenSSH server daemon","loaded","active","running","",
                             "/org/freedesktop/systemd1/unit/sshd_2eservice",0,"",ROOT_PATH],
                            ["chronyd.service","NTP client/server","loaded","inactive","dead","",
                             "/org/freedesktop/systemd1/unit/chronyd_2eservice",0,"",ROOT_PATH],
                        ]])),
                        // reply[0][0] = object path
                        "GetUnit" | "LoadUnit" => ok_reply(&id, json!(["/org/freedesktop/systemd1/unit/sshd_2eservice"])),
                        // reply[0][0] = a{sv} dict. Real cockpit-bridge wraps each
                        // value as a D-Bus variant: {"t":"s","v":"..."}. Mirror that
                        // so the status path is exercised exactly as in production.
                        "GetAll" => ok_reply(&id, json!([{
                            "Id":{"t":"s","v":"sshd.service"},
                            "Description":{"t":"s","v":"OpenSSH server daemon"},
                            "LoadState":{"t":"s","v":"loaded"},
                            "ActiveState":{"t":"s","v":"active"},
                            "SubState":{"t":"s","v":"running"},
                            "UnitFileState":{"t":"s","v":"enabled"}
                        }])),
                        // Lifecycle methods return a job object path: reply[0][0].
                        "StartUnit" | "StopUnit" | "RestartUnit" | "ReloadUnit" => {
                            ok_reply(&id, json!(["/org/freedesktop/systemd1/job/42"]))
                        }
                        // Manager.Reload returns void; fez calls it after
                        // enable/disable to refresh cached unit-file state.
                        "Reload" => ok_reply(&id, json!([])),
                        // EnableUnitFiles returns two out args: carries_install_info (bool)
                        // and a changes array. out_args = reply[0] = [true, [changes]].
                        "EnableUnitFiles" => ok_reply(&id, json!([
                            true,
                            [["symlink",
                              "/etc/systemd/system/multi-user.target.wants/chronyd.service",
                              "/usr/lib/systemd/system/chronyd.service"]],
                        ])),
                        // DisableUnitFiles returns one out arg: a changes array.
                        // out_args = reply[0] = [[changes]].
                        "DisableUnitFiles" => ok_reply(&id, json!([
                            [["unlink",
                              "/etc/systemd/system/multi-user.target.wants/chronyd.service",
                              ""]],
                        ])),
                        // dnf5daemon SessionManager.close_session(path) -> (bool).
                        // Takes a bare object path, not an a{sv} dict, so it is not
                        // a dnf_options_method and lands here.
                        "close_session" => ok_reply(&id, json!([true])),
                        other => err_reply(&id, "org.freedesktop.DBus.Error.UnknownMethod", format!("no fake for {other}")),
                    }
                };
                send_data(&mut stdout, &frame.channel, &reply);
            }
        }
    }
    Ok(())
}
