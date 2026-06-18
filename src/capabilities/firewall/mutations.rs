//! Firewall mutations: add/remove services and ports, set-default-zone,
//! reload, confirm, panic, and masquerade.

use super::zone::{
    compute_drift, effective_zone, is_config_info_denied, parse_port_spec, permanent_zone,
    runtime_zone, session_ports, session_services,
};
use super::{arg_str, fw_call, open_channel, Mutation, FW_IFACE, FW_ZONE_IFACE};
use crate::capabilities::{CapabilityContext, View};
use crate::cli::Cli;
use crate::error::Result;
use crate::protocol::client::BridgeClient;
use serde_json::{json, Value};

/// What kind of view an executed firewall mutation produces.
enum MutationResult {
    /// A runtime zone change (add/remove service/port, set-default-zone).
    Change {
        zone: String,
        change: String,
        timeout: Option<u32>,
    },
    /// Reload permanent config into runtime.
    Reload,
    /// Panic mode toggle.
    Panic { on: bool },
    /// Masquerade toggle.
    Masquerade {
        zone: String,
        on: bool,
        timeout: Option<u32>,
    },
}

/// A firewall mutation to audit-wrap and execute.
struct FirewallMutation {
    call: AuditedFirewallCall,
    result: MutationResult,
}

/// Audit the attempt, run the D-Bus call, audit the result, and build the view.
fn execute_mutation(ctx: &mut CapabilityContext<'_>, m: FirewallMutation) -> Result<View> {
    let operation = m.call.operation.clone();
    run_audited(ctx, m.call)?;
    let host = ctx.host;
    Ok(match m.result {
        MutationResult::Change {
            zone,
            change,
            timeout,
        } => change_view(host, &operation, &zone, &change, timeout),
        MutationResult::Reload => reload_view(host),
        MutationResult::Panic { on } => panic_view(host, on),
        MutationResult::Masquerade { zone, on, timeout } => {
            masquerade_view(host, &zone, on, timeout)
        }
    })
}

/// Run a privileged firewalld mutation: open the privileged channel, apply the
/// protected guards, audit attempt/result around the runtime-only call, and
/// attach the confirm hint.
pub(super) fn mutate(
    cli: &Cli,
    client: &mut BridgeClient,
    host: &str,
    action: Mutation<'_>,
) -> Result<View> {
    let channel = open_channel(client, true)?;
    let mut ctx = CapabilityContext {
        client,
        channel: &channel,
        host,
    };
    match action {
        Mutation::AddService {
            service,
            zone,
            timeout,
        } => {
            let zone = effective_zone(ctx.client, ctx.channel, zone)?;
            let t = i64::from(timeout.unwrap_or(0));
            execute_mutation(
                &mut ctx,
                FirewallMutation {
                    call: AuditedFirewallCall::new(
                        "add-service",
                        format!("{zone}:{service}"),
                        FW_ZONE_IFACE,
                        "addService",
                        json!([zone, service, t]),
                    ),
                    result: MutationResult::Change {
                        zone,
                        change: format!("service {service}"),
                        timeout,
                    },
                },
            )
        }
        Mutation::RemoveService { service, zone } => {
            let zone = effective_zone(ctx.client, ctx.channel, zone)?;
            crate::safety::check_firewall_service_removal(service, &session_services(), cli.force)?;
            execute_mutation(
                &mut ctx,
                FirewallMutation {
                    call: AuditedFirewallCall::new(
                        "remove-service",
                        format!("{zone}:{service}"),
                        FW_ZONE_IFACE,
                        "removeService",
                        json!([zone, service]),
                    ),
                    result: MutationResult::Change {
                        zone,
                        change: format!("service {service}"),
                        timeout: None,
                    },
                },
            )
        }
        Mutation::AddPort {
            port,
            zone,
            timeout,
        } => {
            let spec = parse_port_spec(port)?;
            let zone = effective_zone(ctx.client, ctx.channel, zone)?;
            let t = i64::from(timeout.unwrap_or(0));
            let label = spec.label();
            execute_mutation(
                &mut ctx,
                FirewallMutation {
                    call: AuditedFirewallCall::new(
                        "add-port",
                        format!("{zone}:{label}"),
                        FW_ZONE_IFACE,
                        "addPort",
                        json!([zone, spec.port.to_string(), spec.proto, t]),
                    ),
                    result: MutationResult::Change {
                        zone,
                        change: format!("port {label}"),
                        timeout,
                    },
                },
            )
        }
        Mutation::RemovePort { port, zone } => {
            let spec = parse_port_spec(port)?;
            let zone = effective_zone(ctx.client, ctx.channel, zone)?;
            crate::safety::check_firewall_port_removal(spec.port, &session_ports(), cli.force)?;
            let label = spec.label();
            execute_mutation(
                &mut ctx,
                FirewallMutation {
                    call: AuditedFirewallCall::new(
                        "remove-port",
                        format!("{zone}:{label}"),
                        FW_ZONE_IFACE,
                        "removePort",
                        json!([zone, spec.port.to_string(), spec.proto]),
                    ),
                    result: MutationResult::Change {
                        zone,
                        change: format!("port {label}"),
                        timeout: None,
                    },
                },
            )
        }
        Mutation::SetDefaultZone { zone } => {
            crate::safety::check_firewall_default_zone(cli.force)?;
            execute_mutation(
                &mut ctx,
                FirewallMutation {
                    call: AuditedFirewallCall::new(
                        "set-default-zone",
                        zone,
                        FW_IFACE,
                        "setDefaultZone",
                        json!([zone]),
                    ),
                    result: MutationResult::Change {
                        zone: zone.to_string(),
                        change: "default zone".into(),
                        timeout: None,
                    },
                },
            )
        }
        Mutation::Reload => {
            let default_zone = arg_str(&fw_call(
                ctx.client,
                ctx.channel,
                FW_IFACE,
                "getDefaultZone",
                json!([]),
            )?);
            let runtime = runtime_zone(ctx.client, ctx.channel, &default_zone)?;
            let has_drift = match permanent_zone(ctx.client, ctx.channel, &default_zone) {
                Ok(permanent) => !compute_drift(
                    &runtime.services,
                    &permanent.services,
                    &runtime.ports,
                    &permanent.ports,
                    runtime.masquerade,
                    permanent.masquerade,
                )
                .is_empty(),
                Err(e) if is_config_info_denied(&e) => true,
                Err(e) => return Err(e),
            };
            crate::safety::check_firewall_reload(has_drift, cli.force)?;
            execute_mutation(
                &mut ctx,
                FirewallMutation {
                    call: AuditedFirewallCall::new(
                        "reload",
                        "firewall",
                        FW_IFACE,
                        "reload",
                        json!([]),
                    ),
                    result: MutationResult::Reload,
                },
            )
        }
        Mutation::Confirm => {
            run_audited(
                &mut ctx,
                AuditedFirewallCall::new(
                    "confirm",
                    "firewall",
                    FW_IFACE,
                    "runtimeToPermanent",
                    json!([]),
                ),
            )?;
            Ok(confirm_view(ctx.host))
        }
        Mutation::Panic { state } => {
            let on = state == "on";
            if on {
                crate::safety::check_firewall_panic_on(cli.force)?;
            }
            let method = if on {
                "enablePanicMode"
            } else {
                "disablePanicMode"
            };
            execute_mutation(
                &mut ctx,
                FirewallMutation {
                    call: AuditedFirewallCall::new(
                        format!("panic-{state}"),
                        "firewall",
                        FW_IFACE,
                        method,
                        json!([]),
                    ),
                    result: MutationResult::Panic { on },
                },
            )
        }
        Mutation::Masquerade {
            state,
            zone,
            timeout,
        } => {
            let on = state == "on";
            let zone = effective_zone(ctx.client, ctx.channel, zone)?;
            if !on {
                crate::safety::check_firewall_masquerade_off(cli.force)?;
            }
            let (method, args) = if on {
                let t = i64::from(timeout.unwrap_or(0));
                ("addMasquerade", json!([zone, t]))
            } else {
                ("removeMasquerade", json!([zone]))
            };
            execute_mutation(
                &mut ctx,
                FirewallMutation {
                    call: AuditedFirewallCall::new(
                        format!("masquerade-{state}"),
                        zone.as_str(),
                        FW_ZONE_IFACE,
                        method,
                        args,
                    ),
                    result: MutationResult::Masquerade {
                        zone,
                        on,
                        timeout: if on { timeout } else { None },
                    },
                },
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Audit wiring
// ---------------------------------------------------------------------------

/// A firewalld method call plus the audit metadata that describes it.
struct AuditedFirewallCall {
    operation: String,
    target: String,
    iface: &'static str,
    method: &'static str,
    args: Value,
}

impl AuditedFirewallCall {
    fn new(
        operation: impl Into<String>,
        target: impl Into<String>,
        iface: &'static str,
        method: &'static str,
        args: Value,
    ) -> Self {
        Self {
            operation: operation.into(),
            target: target.into(),
            iface,
            method,
            args,
        }
    }
}

/// Audit the attempt, run the runtime-only firewalld call, audit the result.
fn run_audited(ctx: &mut CapabilityContext<'_>, call: AuditedFirewallCall) -> Result<()> {
    let sink = crate::audit::sink_from_env();
    let audit_ctx = crate::audit::AuditContext::new(
        &crate::audit::actor(),
        ctx.host,
        &call.operation,
        &call.target,
        &crate::audit::correlation_id(),
    );
    sink.write(&audit_ctx.record(crate::audit::Outcome::Attempt));
    let exec = fw_call(ctx.client, ctx.channel, call.iface, call.method, call.args);
    match &exec {
        Ok(_) => sink.write(&audit_ctx.record(crate::audit::Outcome::Ok)),
        Err(e) => sink.write(&audit_ctx.record(crate::audit::Outcome::Error(e.to_string()))),
    }
    exec.map(|_| ())
}

// ---------------------------------------------------------------------------
// View builders
// ---------------------------------------------------------------------------

/// The standard "runtime-only; confirm to persist" hint.
fn confirm_hint() -> Value {
    json!({
        "persisted": false,
        "note": "runtime-only change; run `fez firewall confirm` to persist it",
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeChangeData<'a> {
    operation: &'a str,
    zone: &'a str,
    change: &'a str,
    timeout: Option<u32>,
}

impl<'a> RuntimeChangeData<'a> {
    fn new(operation: &'a str, zone: &'a str, change: &'a str, timeout: Option<u32>) -> Self {
        Self {
            operation,
            zone,
            change,
            timeout,
        }
    }

    fn data(&self) -> Value {
        let mut data = json!({
            "operation": self.operation,
            "zone": self.zone,
            "change": self.change,
            "persisted": false,
        });
        if let Some(timeout) = self.timeout {
            data["timeout"] = json!(timeout);
        }
        data
    }

    fn human(&self) -> String {
        format!(
            "{} {} in zone {} (runtime only)\n",
            self.operation, self.change, self.zone
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PersistedFirewallOperation {
    operation: &'static str,
}

impl PersistedFirewallOperation {
    fn data(self) -> Value {
        json!({"operation": self.operation, "persisted": true})
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PanicChangeData {
    on: bool,
}

impl PanicChangeData {
    fn data(self) -> Value {
        json!({"operation": "panic", "panic_mode": self.on, "persisted": false})
    }

    fn human(self) -> String {
        format!(
            "panic mode {}\n",
            if self.on { "enabled" } else { "disabled" }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MasqueradeChangeData<'a> {
    zone: &'a str,
    on: bool,
    timeout: Option<u32>,
}

impl<'a> MasqueradeChangeData<'a> {
    fn new(zone: &'a str, on: bool, timeout: Option<u32>) -> Self {
        Self { zone, on, timeout }
    }

    fn change(&self) -> &'static str {
        if self.on {
            "+masquerade"
        } else {
            "-masquerade"
        }
    }

    fn data(&self) -> Value {
        let mut data = json!({
            "operation": "masquerade",
            "zone": self.zone,
            "change": self.change(),
            "masquerade": self.on,
            "persisted": false,
        });
        if let Some(timeout) = self.timeout {
            data["timeout"] = json!(timeout);
        }
        data
    }

    fn human(&self) -> String {
        format!(
            "masquerade {} in zone {} (runtime only)\n",
            if self.on { "enabled" } else { "disabled" },
            self.zone
        )
    }
}

/// Build the `FirewallChange` view for an add/remove/set mutation.
fn change_view(host: &str, op: &str, zone: &str, what: &str, timeout: Option<u32>) -> View {
    let change = RuntimeChangeData::new(op, zone, what, timeout);
    View::new("FirewallChange", host, change.data(), change.human()).with_hints(confirm_hint())
}

/// Build the `FirewallChange` view for `reload`.
fn reload_view(host: &str) -> View {
    let operation = PersistedFirewallOperation {
        operation: "reload",
    };
    View::new(
        "FirewallChange",
        host,
        operation.data(),
        "reloaded permanent config into runtime\n".into(),
    )
}

/// Build the `FirewallConfirm` view for `confirm`.
fn confirm_view(host: &str) -> View {
    let operation = PersistedFirewallOperation {
        operation: "confirm",
    };
    View::new(
        "FirewallConfirm",
        host,
        operation.data(),
        "runtime config committed to permanent\n".into(),
    )
}

/// Build the `FirewallChange` view for `panic on|off`.
fn panic_view(host: &str, on: bool) -> View {
    let change = PanicChangeData { on };
    View::new("FirewallChange", host, change.data(), change.human())
}

/// Build the `FirewallChange` view for `masquerade on|off`.
fn masquerade_view(host: &str, zone: &str, on: bool, timeout: Option<u32>) -> View {
    let change = MasqueradeChangeData::new(zone, on, timeout);
    View::new("FirewallChange", host, change.data(), change.human()).with_hints(confirm_hint())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn audited_firewall_call_captures_method_and_audit_metadata() {
        let call = AuditedFirewallCall::new(
            "add-service",
            "public:http",
            FW_ZONE_IFACE,
            "addService",
            json!(["public", "http", 0]),
        );

        assert_eq!(call.operation, "add-service");
        assert_eq!(call.target, "public:http");
        assert_eq!(call.iface, FW_ZONE_IFACE);
        assert_eq!(call.method, "addService");
        assert_eq!(call.args, json!(["public", "http", 0]));
    }

    #[test]
    fn runtime_change_data_preserves_json_contract() {
        let change = RuntimeChangeData::new("add-service", "public", "service http", Some(60));

        assert_eq!(
            change.data(),
            json!({
                "operation": "add-service",
                "zone": "public",
                "change": "service http",
                "persisted": false,
                "timeout": 60,
            })
        );
        assert_eq!(
            change.human(),
            "add-service service http in zone public (runtime only)\n"
        );
    }

    #[test]
    fn persisted_firewall_operation_preserves_json_contract() {
        let reload = PersistedFirewallOperation {
            operation: "reload",
        };
        let confirm = PersistedFirewallOperation {
            operation: "confirm",
        };

        assert_eq!(
            reload.data(),
            json!({"operation": "reload", "persisted": true})
        );
        assert_eq!(
            confirm.data(),
            json!({"operation": "confirm", "persisted": true})
        );
    }

    #[test]
    fn panic_change_data_preserves_json_contract() {
        let change = PanicChangeData { on: true };

        assert_eq!(
            change.data(),
            json!({"operation": "panic", "panic_mode": true, "persisted": false})
        );
        assert_eq!(change.human(), "panic mode enabled\n");
    }

    #[test]
    fn masquerade_change_data_preserves_json_contract() {
        let change = MasqueradeChangeData::new("public", false, None);

        assert_eq!(
            change.data(),
            json!({
                "operation": "masquerade",
                "zone": "public",
                "change": "-masquerade",
                "masquerade": false,
                "persisted": false,
            })
        );
        assert_eq!(
            change.human(),
            "masquerade disabled in zone public (runtime only)\n"
        );
    }
}
