//! PackageKit fallback backend (`org.freedesktop.PackageKit`).
//!
//! Used when dnf5daemon (`org.rpm.dnf.v0`) is absent (notably RHEL 10). Drives
//! PackageKit's per-transaction D-Bus API: a transaction object path is created,
//! a method is called on it, and the results arrive as a stream of signals
//! collected by [`crate::protocol::client::BridgeClient::dbus_call_collect`].
//!
//! PackageKit's plan carries no install/download sizes (only `info`,
//! `package_id`, `summary` per package), so size fields are emitted as `null`
//! and every payload carries `"backend":"packagekit"` plus a hint so callers
//! can see the schema is degraded relative to the dnf5daemon path.
use crate::error::{FezError, Result};
use crate::protocol::client::BridgeClient;
use serde_json::{json, Value};

/// Well-known bus name of the PackageKit daemon.
const PK_NAME: &str = "org.freedesktop.PackageKit";
/// Object path of the PackageKit daemon's root controller.
const PK_PATH: &str = "/org/freedesktop/PackageKit";
/// Root controller interface (carries `CreateTransaction`).
const PK_IFACE: &str = "org.freedesktop.PackageKit";
/// Per-transaction interface (carries the operation methods + result signals).
const TX_IFACE: &str = "org.freedesktop.PackageKit.Transaction";

/// PackageKit transaction flag: execute for real (no special handling).
const TF_NONE: u64 = 0;
/// PackageKit transaction flag: simulate only (resolve the plan, change nothing).
const TF_SIMULATE: u64 = 2;

/// PackageKit filter bit: only installed packages (`PK_FILTER_ENUM_INSTALLED`).
const FILTER_INSTALLED: u64 = 1 << 2;
/// PackageKit filter bit: newest version only (`PK_FILTER_ENUM_NEWEST`).
const FILTER_NEWEST: u64 = 1 << 16;
/// PackageKit filter: no filtering (`PK_FILTER_ENUM_NONE`).
const FILTER_NONE: u64 = 0;

/// PackageKit `info` enum: package will be installed.
const INFO_INSTALLING: u64 = 8;
/// PackageKit `info` enum: package will be removed.
const INFO_REMOVING: u64 = 9;
/// PackageKit `info` enum: package will be updated.
const INFO_UPDATING: u64 = 7;
/// PackageKit `info` enum: package will be obsoleted (treated as a removal).
const INFO_OBSOLETING: u64 = 11;
/// PackageKit `info` enum: package will be downgraded.
const INFO_DOWNGRADING: u64 = 13;

/// PackageKit error enum: `PK_ERROR_ENUM_NOT_AUTHORIZED`.
const PK_ERROR_NOT_AUTHORIZED: u64 = 6;

/// A render-ready PackageKit result, assembled into a `packages::View` by the
/// caller (which supplies the host).
pub struct PkView {
    /// Envelope `kind` (e.g. `PackageList`, `PackagePlan`, `PackageMutation`).
    pub kind: &'static str,
    /// The `fez/v1` data payload, carrying `"backend":"packagekit"`.
    pub data: Value,
    /// Human-readable rendering.
    pub human: String,
    /// Optional envelope hints (carries the degraded-schema note).
    pub hints: Option<Value>,
}

/// One package parsed from a PackageKit `Package(info, package_id, summary)`
/// signal. `package_id` is `name;version;arch;data` (data = repo or `installed`).
struct PkPackage {
    info: u64,
    name: String,
    version: String,
    arch: String,
    data: String,
    summary: String,
}

impl PkPackage {
    /// Parse a `Package` signal's `[info, package_id, summary]` args.
    ///
    /// Returns `None` if the args are malformed (missing `info` or `package_id`).
    fn from_signal(args: &[Value]) -> Option<Self> {
        let info = args.first()?.as_u64()?;
        let pid = args.get(1)?.as_str()?;
        let summary = args
            .get(2)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let mut parts = pid.splitn(4, ';');
        let name = parts.next()?.to_string();
        let version = parts.next().unwrap_or("").to_string();
        let arch = parts.next().unwrap_or("").to_string();
        let data = parts.next().unwrap_or("").to_string();
        Some(PkPackage {
            info,
            name,
            version,
            arch,
            data,
            summary,
        })
    }

    /// The NEVRA-ish label `name-version.arch` for plan rendering.
    fn label(&self) -> String {
        format!("{}-{}.{}", self.name, self.version, self.arch)
    }

    /// The full `name;version;arch;data` package_id (mutations need this form).
    fn package_id(&self) -> String {
        format!("{};{};{};{}", self.name, self.version, self.arch, self.data)
    }

    /// The repo/data field; PackageKit puts the repo id (or `installed`) here.
    fn repo(&self) -> &str {
        &self.data
    }
}

/// Pull the `Package` rows out of a collected signal stream, in arrival order.
fn packages_from(signals: &[(String, Vec<Value>)]) -> Vec<PkPackage> {
    signals
        .iter()
        .filter(|(member, _)| member == "Package")
        .filter_map(|(_, args)| PkPackage::from_signal(args))
        .collect()
}

/// Map a PackageKit `ErrorCode(code, details)` in the stream, if any, to a
/// [`FezError`].
///
/// # Errors
///
/// Returns [`FezError::AccessDenied`] (exit 11) when the stream carries an
/// `ErrorCode` with `PK_ERROR_ENUM_NOT_AUTHORIZED`, or [`FezError::Dbus`] for
/// any other PackageKit error.
fn check_stream(signals: &[(String, Vec<Value>)]) -> Result<()> {
    if let Some((_, args)) = signals.iter().find(|(m, _)| m == "ErrorCode") {
        let code = args.first().and_then(Value::as_u64).unwrap_or(0);
        let details = args
            .get(1)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if code == PK_ERROR_NOT_AUTHORIZED {
            return Err(FezError::AccessDenied {
                remediation: format!(
                    "PackageKit denied the operation ({details}). Ensure privilege escalation is available (passwordless sudo or a polkit rule) and retry."
                ),
            });
        }
        return Err(FezError::Dbus {
            name: "org.freedesktop.PackageKit.Error".into(),
            message: details,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_package_signal() {
        let args = vec![
            json!(8),
            json!("htop;3.4.1-3.fc44;x86_64;fedora"),
            json!("Interactive process viewer"),
        ];
        let p = PkPackage::from_signal(&args).unwrap();
        assert_eq!(p.info, 8);
        assert_eq!(p.name, "htop");
        assert_eq!(p.version, "3.4.1-3.fc44");
        assert_eq!(p.arch, "x86_64");
        assert_eq!(p.repo(), "fedora");
        assert_eq!(p.label(), "htop-3.4.1-3.fc44.x86_64");
        assert_eq!(p.package_id(), "htop;3.4.1-3.fc44;x86_64;fedora");
    }

    #[test]
    fn malformed_package_signal_is_none() {
        assert!(PkPackage::from_signal(&[]).is_none());
        assert!(PkPackage::from_signal(&[json!(8)]).is_none());
    }

    #[test]
    fn error_code_six_is_access_denied() {
        let signals = vec![
            (
                "ErrorCode".to_string(),
                vec![json!(6), json!("not authorized")],
            ),
            ("Finished".to_string(), vec![json!(4), json!(10)]),
        ];
        let err = check_stream(&signals).unwrap_err();
        assert!(matches!(err, FezError::AccessDenied { .. }));
    }

    #[test]
    fn other_error_code_is_dbus() {
        let signals = vec![(
            "ErrorCode".to_string(),
            vec![json!(4), json!("package not found")],
        )];
        let err = check_stream(&signals).unwrap_err();
        assert!(matches!(err, FezError::Dbus { .. }));
    }

    #[test]
    fn clean_stream_is_ok() {
        let signals = vec![
            (
                "Package".to_string(),
                vec![json!(8), json!("htop;1;x86_64;fedora"), json!("")],
            ),
            ("Finished".to_string(), vec![json!(1), json!(20)]),
        ];
        assert!(check_stream(&signals).is_ok());
    }
}
