//! Canned dnf5daemon (`org.rpm.dnf.v0`) replies.

use super::{err_reply, ok_reply};
use serde_json::{json, Value};

/// Fixed dnf5daemon session path the fake hands back from `open_session`.
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
/// Returns `Some(error_reply)` when a bare value is found, `None` otherwise.
pub(super) fn reject_unwrapped_options(args: &[Value], id: &Value) -> Option<Value> {
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

/// Canned reply for a dnf5daemon method.
pub(super) fn dnf_reply(method: &str, iface: &str, id: &Value) -> Value {
    const SERVICE_UNKNOWN: &str = "org.freedesktop.DBus.Error.ServiceUnknown";
    const UNKNOWN_METHOD: &str = "org.freedesktop.DBus.Error.UnknownMethod";
    match method {
        "open_session" => {
            if std::env::var_os("FEZ_FAKE_NO_DNF5").is_some() {
                err_reply(id, SERVICE_UNKNOWN, "The name org.rpm.dnf.v0 was not provided by any .service files".into())
            } else {
                ok_reply(id, json!([SESSION_PATH]))
            }
        }
        "list" if iface.ends_with(".rpm.Repo") => ok_reply(id, json!([[
            dnf_repo("fedora", "Fedora", true),
            dnf_repo("updates-testing", "Fedora - Testing", false),
        ]])),
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
        "install" | "remove" | "upgrade" => ok_reply(id, json!([])),
        "resolve" => ok_reply(id, json!([fake_resolve_items(), 0])),
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
            let items: Vec<Value> = (0..21).map(|i| removed(&format!("pkg{i}"))).collect();
            Value::Array(items)
        }
        Ok("install") | Err(_) => json!([installed("htop")]),
        Ok(_) => json!([removed("htop")]),
    }
}
