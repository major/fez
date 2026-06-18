//! Canned PackageKit signal stream for transaction methods.

use super::{err_reply, ok_reply, send_data};
use serde_json::{json, Value};
use std::io::Write;

/// PackageKit root controller object path (carries `CreateTransaction`).
pub(super) const PK_PATH: &str = "/org/freedesktop/PackageKit";
/// Canned PackageKit transaction object path the fake hands back.
pub(super) const PK_TX_PATH: &str = "/org/freedesktop/PackageKit/Transaction/1";
/// PackageKit per-transaction interface (the signals carry this interface).
const PK_TX_IFACE: &str = "org.freedesktop.PackageKit.Transaction";

/// Handle a `CreateTransaction` call on the PackageKit root controller.
///
/// `FEZ_FAKE_NO_PACKAGEKIT` models the daemon being absent (ServiceUnknown).
pub(super) fn pk_create_transaction(id: &Value) -> Value {
    if std::env::var_os("FEZ_FAKE_NO_PACKAGEKIT").is_some() {
        err_reply(id, "org.freedesktop.DBus.Error.ServiceUnknown", "The name org.freedesktop.PackageKit was not provided by any .service files".into())
    } else {
        ok_reply(id, json!([PK_TX_PATH]))
    }
}

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
pub(super) fn pk_emit(out: &mut impl Write, channel: &str, method: &str) {
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
            send_signal(out, channel, "Package", &json!([2, nginx, "High performance web server"]));
        }
        "InstallPackages" | "UpdatePackages" => {
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
    if std::env::var("FEZ_FAKE_PK_ERROR").as_deref() == Ok("notauth") {
        send_signal(out, channel, "ErrorCode", &json!([6, "not authorized to perform operation"]));
        send_signal(out, channel, "Finished", &json!([4, 10]));
        return;
    }
    send_signal(out, channel, "Finished", &json!([1, 20]));
}
