//! Firewall management over firewalld (`org.fedoraproject.FirewallD1`).
//!
//! Reads (status/list/show/services) open an unprivileged `dbus-json3` channel;
//! mutations (add/remove service/port, set-default-zone, reload, confirm,
//! panic) open a privileged one and escalate. fez holds no state: the
//! runtime-vs-permanent split that guards against lockout is firewalld's own,
//! read live each call and committed only via `runtimeToPermanent`.

mod mutations;
mod reads;
mod zone;

use crate::capabilities::{render, CapabilityContext, View};
use crate::cli::{Cli, FirewallAction};
use crate::error::{is_service_unknown, FezError, Result};
use crate::protocol::client::BridgeClient;
use serde_json::Value;

const FW_NAME: &str = "org.fedoraproject.FirewallD1";
const FW_PATH: &str = "/org/fedoraproject/FirewallD1";
const FW_IFACE: &str = "org.fedoraproject.FirewallD1";
const FW_ZONE_IFACE: &str = "org.fedoraproject.FirewallD1.zone";
const FW_CONFIG_PATH: &str = "/org/fedoraproject/FirewallD1/config";
const FW_CONFIG_IFACE: &str = "org.fedoraproject.FirewallD1.config";
const FW_CONFIG_ZONE_IFACE: &str = "org.fedoraproject.FirewallD1.config.zone";

/// Route a parsed `firewall` action to its handler and return the exit code.
///
/// Error hints (for `DependencyMissing` and `UnsupportedApi`) come from
/// [`FezError::hints`] and are applied uniformly by [`render`].
pub fn dispatch(cli: &Cli, action: &FirewallAction) -> i32 {
    render(cli, run(cli, action))
}

/// The [`FezError::DependencyMissing`] returned when firewalld is absent.
fn dependency_missing() -> FezError {
    FezError::DependencyMissing {
        component: "firewalld".into(),
        dbus_name: FW_NAME.into(),
        remediation: "Check if firewalld is running: fez services status firewalld.service --json. If absent or stopped, install and start it: dnf install firewalld && systemctl enable --now firewalld.service.".into(),
    }
}

/// Validate a firewall service name before it reaches firewalld D-Bus calls.
///
/// Rejects empty, over-long, control-character, or `-`-prefixed names.
/// Service names like `http`, `ssh`, and `cockpit` pass through.
///
/// # Errors
///
/// Returns [`FezError::Usage`] (exit 2) when the name is empty, exceeds
/// [`MAX_LEN`], starts with `-`, or contains a control character.
pub(crate) fn validate_firewall_service(name: &str) -> Result<()> {
    const MAX_LEN: usize = 128;
    if name.is_empty() {
        return Err(FezError::Usage(
            "firewall service name must not be empty".into(),
        ));
    }
    if name.len() > MAX_LEN {
        return Err(FezError::Usage(format!(
            "firewall service name too long ({} > {MAX_LEN})",
            name.len()
        )));
    }
    if name.starts_with('-') {
        return Err(FezError::Usage(format!(
            "firewall service name must not start with '-': {name}"
        )));
    }
    for ch in name.chars() {
        if ch.is_control() {
            return Err(FezError::Usage(format!(
                "firewall service name contains control character U+{:04X}",
                ch as u32
            )));
        }
    }
    Ok(())
}

/// A read-only firewall subcommand and its borrowed arguments.
#[derive(Debug, PartialEq, Eq)]
enum ReadAction<'a> {
    Status,
    List,
    Show { zone: &'a str },
    Services,
}

/// A firewall mutation subcommand and its borrowed arguments.
#[derive(Debug, PartialEq, Eq)]
enum Mutation<'a> {
    AddService {
        service: &'a str,
        zone: Option<&'a str>,
        timeout: Option<u32>,
    },
    RemoveService {
        service: &'a str,
        zone: Option<&'a str>,
    },
    AddPort {
        port: &'a str,
        zone: Option<&'a str>,
        timeout: Option<u32>,
    },
    RemovePort {
        port: &'a str,
        zone: Option<&'a str>,
    },
    SetDefaultZone {
        zone: &'a str,
    },
    Reload,
    Confirm,
    Panic {
        state: &'a str,
    },
    Masquerade {
        state: &'a str,
        zone: Option<&'a str>,
        timeout: Option<u32>,
    },
}

/// The read/mutate split of a parsed [`FirewallAction`].
#[derive(Debug, PartialEq, Eq)]
enum Plan<'a> {
    Read(ReadAction<'a>),
    Mutate(Mutation<'a>),
}

/// Map the flat clap enum onto a typed read/mutate plan.
fn classify(action: &FirewallAction) -> Plan<'_> {
    match action {
        FirewallAction::Status => Plan::Read(ReadAction::Status),
        FirewallAction::List => Plan::Read(ReadAction::List),
        FirewallAction::Show { zone } => Plan::Read(ReadAction::Show { zone }),
        FirewallAction::Services => Plan::Read(ReadAction::Services),
        FirewallAction::AddService {
            service,
            zone,
            timeout,
        } => Plan::Mutate(Mutation::AddService {
            service,
            zone: zone.as_deref(),
            timeout: *timeout,
        }),
        FirewallAction::RemoveService { service, zone } => Plan::Mutate(Mutation::RemoveService {
            service,
            zone: zone.as_deref(),
        }),
        FirewallAction::AddPort {
            port,
            zone,
            timeout,
        } => Plan::Mutate(Mutation::AddPort {
            port,
            zone: zone.as_deref(),
            timeout: *timeout,
        }),
        FirewallAction::RemovePort { port, zone } => Plan::Mutate(Mutation::RemovePort {
            port,
            zone: zone.as_deref(),
        }),
        FirewallAction::SetDefaultZone { zone } => Plan::Mutate(Mutation::SetDefaultZone { zone }),
        FirewallAction::Reload => Plan::Mutate(Mutation::Reload),
        FirewallAction::Confirm => Plan::Mutate(Mutation::Confirm),
        FirewallAction::Panic { state } => Plan::Mutate(Mutation::Panic { state }),
        FirewallAction::Masquerade {
            state,
            zone,
            timeout,
        } => Plan::Mutate(Mutation::Masquerade {
            state,
            zone: zone.as_deref(),
            timeout: *timeout,
        }),
    }
}

/// Connect to the bridge and dispatch the requested action.
fn run(cli: &Cli, action: &FirewallAction) -> Result<View> {
    let mut client = crate::capabilities::connect(cli)?;
    let host = client.host().to_string();
    match classify(action) {
        Plan::Read(read) => {
            let ch = open_channel(&mut client, false)?;
            let mut ctx = CapabilityContext {
                client: &mut client,
                channel: &ch,
                host: &host,
            };
            match read {
                ReadAction::Status => reads::status(&mut ctx),
                ReadAction::List => reads::list(&mut ctx),
                ReadAction::Show { zone } => reads::show(&mut ctx, zone),
                ReadAction::Services => reads::services(&mut ctx),
            }
        }
        Plan::Mutate(mutation) => mutations::mutate(cli, &mut client, &host, mutation),
    }
}

/// Open a firewalld `dbus-json3` channel (privileged for mutations).
///
/// firewalld activation failure (the service is absent) surfaces on the first
/// method call, not at open time; the caller probes it via [`fw_call`], which
/// maps ServiceUnknown to [`dependency_missing`]. A privileged open escalates
/// first and can itself fail with `AccessDenied` (exit 11).
///
/// # Errors
///
/// Propagates any channel-open or escalation error from the bridge client.
fn open_channel(client: &mut BridgeClient, privileged: bool) -> Result<String> {
    if privileged {
        client.dbus_open_privileged(FW_NAME)
    } else {
        client.dbus_open(FW_NAME)
    }
}

/// Call a firewalld method on the main object, mapping ServiceUnknown to the
/// dependency-missing error.
fn fw_call(
    client: &mut BridgeClient,
    channel: &str,
    iface: &str,
    method: &str,
    args: Value,
) -> Result<Value> {
    fw_call_path(client, channel, FW_PATH, iface, method, args)
}

/// Call a firewalld method on an explicit object path, mapping low-level
/// transport/D-Bus failures to actionable firewall errors via [`map_fw_error`].
fn fw_call_path(
    client: &mut BridgeClient,
    channel: &str,
    path: &str,
    iface: &str,
    method: &str,
    args: Value,
) -> Result<Value> {
    client
        .dbus_call(channel, path, iface, method, args)
        .map_err(|e| map_fw_error(e, method))
}

/// Map a raw bridge/D-Bus failure to an actionable firewall error (issue #60).
///
/// firewalld is D-Bus-activated, so an absent or failed service is not
/// observably distinct from "installed but stopped": both surface as the name
/// being unreachable. We therefore collapse all of those to
/// [`dependency_missing`] (whose remediation covers install **and**
/// enable+start) rather than inventing a `service-inactive` code fez cannot
/// reliably detect:
/// - `Dbus { ServiceUnknown | NameHasNoOwner }`: name not activatable.
/// - `Problem("not-found")`: cockpit closed the channel because the name could
///   not be reached (the symptom reported in #60).
/// - `Problem("not-supported")`: the bus refused the name.
///
/// A `Dbus { UnknownMethod }` means firewalld is reachable but too old to
/// expose the method; that maps to [`FezError::UnsupportedApi`] carrying the
/// method name, so a caller treats the feature as unsupported instead of
/// recommending an install. All other errors pass through unchanged, so the
/// raw cause is preserved when it is already actionable (e.g. `AccessDenied`).
fn map_fw_error(e: FezError, method: &str) -> FezError {
    match e {
        FezError::Dbus { ref name, .. } if is_service_unknown(name) => dependency_missing(),
        FezError::Dbus { ref name, .. } if name.contains("UnknownMethod") => {
            FezError::UnsupportedApi(method.to_string())
        }
        FezError::ChannelNotFound(_) | FezError::ChannelNotSupported(_) => dependency_missing(),
        other => other,
    }
}

/// First out-arg of a reply as a string array.
fn arg_str_vec(out: &Value) -> Vec<String> {
    out.get(0)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// First out-arg of a reply as a single string.
fn arg_str(out: &Value) -> String {
    out.get(0).and_then(Value::as_str).unwrap_or("").to_string()
}

/// First out-arg of a reply as a bool.
fn arg_bool(out: &Value) -> bool {
    out.get(0).and_then(Value::as_bool).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::FirewallAction;
    use crate::error::FezError;

    fn dbus(name: &str) -> FezError {
        FezError::Dbus {
            name: name.into(),
            message: "boom".into(),
        }
    }

    #[test]
    fn map_fw_error_service_unknown_is_dependency_missing() {
        let mapped = map_fw_error(
            dbus("org.freedesktop.DBus.Error.ServiceUnknown"),
            "getZones",
        );
        assert_eq!(mapped.code(), "dependency-missing");
        assert_eq!(mapped.exit_code(), 9);
        assert_eq!(
            map_fw_error(
                dbus("org.freedesktop.DBus.Error.NameHasNoOwner"),
                "getZones"
            )
            .code(),
            "dependency-missing"
        );
    }

    #[test]
    fn map_fw_error_unknown_method_is_unsupported_api() {
        let mapped = map_fw_error(
            dbus("org.freedesktop.DBus.Error.UnknownMethod"),
            "getMasquerade",
        );
        assert_eq!(mapped.code(), "unsupported-api");
        assert_eq!(mapped.exit_code(), 12);
        assert!(matches!(
            mapped,
            FezError::UnsupportedApi(ref m) if m == "getMasquerade"
        ));
    }

    #[test]
    fn map_fw_error_channel_problem_is_dependency_missing() {
        let cases: Vec<FezError> = vec![
            FezError::ChannelNotFound("not-found".into()),
            FezError::ChannelNotSupported("not-supported".into()),
        ];
        for case in cases {
            let mapped = map_fw_error(case, "getZones");
            assert_eq!(
                mapped.code(),
                "dependency-missing",
                "channel error should map to dependency-missing"
            );
        }
    }

    #[test]
    fn map_fw_error_passes_through_unrelated_errors() {
        assert_eq!(
            map_fw_error(
                FezError::ChannelAuthFailed("authentication-failed".into()),
                "getZones"
            )
            .code(),
            "auth-failed"
        );
        let denied = FezError::AccessDenied {
            remediation: "enable sudo".into(),
        };
        assert_eq!(map_fw_error(denied, "getZones").code(), "access-denied");
    }

    #[test]
    fn classify_routes_reads_and_mutations_to_typed_plans() {
        assert!(matches!(
            classify(&FirewallAction::Status),
            Plan::Read(ReadAction::Status)
        ));
        assert!(matches!(
            classify(&FirewallAction::List),
            Plan::Read(ReadAction::List)
        ));
        assert!(matches!(
            classify(&FirewallAction::Show {
                zone: "public".into()
            }),
            Plan::Read(ReadAction::Show { zone: "public" })
        ));
        assert!(matches!(
            classify(&FirewallAction::Services),
            Plan::Read(ReadAction::Services)
        ));
        assert!(matches!(
            classify(&FirewallAction::AddService {
                service: "ssh".into(),
                zone: Some("public".into()),
                timeout: Some(60),
            }),
            Plan::Mutate(Mutation::AddService {
                service: "ssh",
                zone: Some("public"),
                timeout: Some(60),
            })
        ));
        assert!(matches!(
            classify(&FirewallAction::RemoveService {
                service: "ssh".into(),
                zone: Some("public".into()),
            }),
            Plan::Mutate(Mutation::RemoveService {
                service: "ssh",
                zone: Some("public"),
            })
        ));
        assert!(matches!(
            classify(&FirewallAction::AddPort {
                port: "8080/tcp".into(),
                zone: Some("public".into()),
                timeout: Some(60),
            }),
            Plan::Mutate(Mutation::AddPort {
                port: "8080/tcp",
                zone: Some("public"),
                timeout: Some(60),
            })
        ));
        assert!(matches!(
            classify(&FirewallAction::RemovePort {
                port: "8080/tcp".into(),
                zone: Some("public".into()),
            }),
            Plan::Mutate(Mutation::RemovePort {
                port: "8080/tcp",
                zone: Some("public"),
            })
        ));
        assert!(matches!(
            classify(&FirewallAction::SetDefaultZone {
                zone: "internal".into(),
            }),
            Plan::Mutate(Mutation::SetDefaultZone { zone: "internal" })
        ));
        assert!(matches!(
            classify(&FirewallAction::Reload),
            Plan::Mutate(Mutation::Reload)
        ));
        assert!(matches!(
            classify(&FirewallAction::Confirm),
            Plan::Mutate(Mutation::Confirm)
        ));
        assert!(matches!(
            classify(&FirewallAction::Panic { state: "on".into() }),
            Plan::Mutate(Mutation::Panic { state: "on" })
        ));
        assert!(matches!(
            classify(&FirewallAction::Masquerade {
                state: "on".into(),
                zone: Some("public".into()),
                timeout: Some(60),
            }),
            Plan::Mutate(Mutation::Masquerade {
                state: "on",
                zone: Some("public"),
                timeout: Some(60),
            })
        ));
    }

    #[test]
    fn validate_firewall_service_rejects_bad_names() {
        assert!(matches!(
            super::validate_firewall_service(""),
            Err(FezError::Usage(_))
        ));
        assert!(matches!(
            super::validate_firewall_service("--help"),
            Err(FezError::Usage(_))
        ));
        assert!(matches!(
            super::validate_firewall_service("foo\x00bar"),
            Err(FezError::Usage(_))
        ));
        let long = "a".repeat(129);
        assert!(matches!(
            super::validate_firewall_service(&long),
            Err(FezError::Usage(_))
        ));
    }

    #[test]
    fn validate_firewall_service_accepts_valid_names() {
        assert!(super::validate_firewall_service("http").is_ok());
        assert!(super::validate_firewall_service("ssh").is_ok());
        assert!(super::validate_firewall_service("cockpit").is_ok());
    }
}
