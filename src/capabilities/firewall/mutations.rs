//! Firewall mutations: add/remove services and ports, set-default-zone,
//! reload, confirm, panic, and masquerade.

use super::zone::{
    compute_drift, effective_zone, is_config_info_denied, parse_port_spec, permanent_zone,
    runtime_zone, session_ports, session_services,
};
use super::{arg_str, fw_call, open_channel, Mutation, FW_IFACE, FW_ZONE_IFACE};
use crate::capabilities::View;
use crate::cli::Cli;
use crate::error::Result;
use crate::protocol::client::BridgeClient;
use serde_json::{json, Value};

/// Run a privileged firewalld mutation: open the privileged channel, apply the
/// protected guards, audit attempt/result around the runtime-only call, and
/// attach the confirm hint.
pub(super) fn mutate(
    cli: &Cli,
    client: &mut BridgeClient,
    host: String,
    action: Mutation<'_>,
) -> Result<View> {
    let channel = open_channel(client, true)?;
    let ch = channel.as_str();
    match action {
        Mutation::AddService {
            service,
            zone,
            timeout,
        } => mutate_add_service(client, ch, host, service, zone, timeout),
        Mutation::RemoveService { service, zone } => {
            mutate_remove_service(cli, client, ch, host, service, zone)
        }
        Mutation::AddPort {
            port,
            zone,
            timeout,
        } => mutate_add_port(client, ch, host, port, zone, timeout),
        Mutation::RemovePort { port, zone } => {
            mutate_remove_port(cli, client, ch, host, port, zone)
        }
        Mutation::SetDefaultZone { zone } => mutate_set_default_zone(cli, client, ch, host, zone),
        Mutation::Reload => mutate_reload(cli, client, ch, host),
        Mutation::Confirm => mutate_confirm(client, ch, host),
        Mutation::Panic { state } => mutate_panic(cli, client, ch, host, state),
        Mutation::Masquerade {
            state,
            zone,
            timeout,
        } => mutate_masquerade(cli, client, ch, host, state, zone, timeout),
    }
}

/// `firewall add-service`: open a service in a zone (runtime-only).
fn mutate_add_service(
    client: &mut BridgeClient,
    channel: &str,
    host: String,
    service: &str,
    zone: Option<&str>,
    timeout: Option<u32>,
) -> Result<View> {
    let zone = effective_zone(client, channel, zone)?;
    let t = i64::from(timeout.unwrap_or(0));
    run_audited(
        client,
        channel,
        &host,
        AuditedFirewallCall::new(
            "add-service",
            format!("{zone}:{service}"),
            FW_ZONE_IFACE,
            "addService",
            json!([zone, service, t]),
        ),
    )?;
    Ok(change_view(
        host,
        "add-service",
        &zone,
        &format!("service {service}"),
        timeout,
    ))
}

/// `firewall remove-service`: close a service in a zone, gated by the
/// lockout guard ([`crate::safety::check_firewall_service_removal`]).
fn mutate_remove_service(
    cli: &Cli,
    client: &mut BridgeClient,
    channel: &str,
    host: String,
    service: &str,
    zone: Option<&str>,
) -> Result<View> {
    let zone = effective_zone(client, channel, zone)?;
    crate::safety::check_firewall_service_removal(service, &session_services(), cli.force)?;
    run_audited(
        client,
        channel,
        &host,
        AuditedFirewallCall::new(
            "remove-service",
            format!("{zone}:{service}"),
            FW_ZONE_IFACE,
            "removeService",
            json!([zone, service]),
        ),
    )?;
    Ok(change_view(
        host,
        "remove-service",
        &zone,
        &format!("service {service}"),
        None,
    ))
}

/// `firewall add-port`: open a `port/proto` in a zone (runtime-only).
fn mutate_add_port(
    client: &mut BridgeClient,
    channel: &str,
    host: String,
    port: &str,
    zone: Option<&str>,
    timeout: Option<u32>,
) -> Result<View> {
    let spec = parse_port_spec(port)?;
    let zone = effective_zone(client, channel, zone)?;
    let t = i64::from(timeout.unwrap_or(0));
    let label = spec.label();
    run_audited(
        client,
        channel,
        &host,
        AuditedFirewallCall::new(
            "add-port",
            format!("{zone}:{label}"),
            FW_ZONE_IFACE,
            "addPort",
            json!([zone, spec.port.to_string(), spec.proto, t]),
        ),
    )?;
    Ok(change_view(
        host,
        "add-port",
        &zone,
        &format!("port {label}"),
        timeout,
    ))
}

/// `firewall remove-port`: close a `port/proto` in a zone, gated by the
/// lockout guard ([`crate::safety::check_firewall_port_removal`]).
fn mutate_remove_port(
    cli: &Cli,
    client: &mut BridgeClient,
    channel: &str,
    host: String,
    port: &str,
    zone: Option<&str>,
) -> Result<View> {
    let spec = parse_port_spec(port)?;
    let zone = effective_zone(client, channel, zone)?;
    crate::safety::check_firewall_port_removal(spec.port, &session_ports(), cli.force)?;
    let label = spec.label();
    run_audited(
        client,
        channel,
        &host,
        AuditedFirewallCall::new(
            "remove-port",
            format!("{zone}:{label}"),
            FW_ZONE_IFACE,
            "removePort",
            json!([zone, spec.port.to_string(), spec.proto]),
        ),
    )?;
    Ok(change_view(
        host,
        "remove-port",
        &zone,
        &format!("port {label}"),
        None,
    ))
}

/// `firewall set-default-zone`: change the default zone, gated by
/// [`crate::safety::check_firewall_default_zone`].
fn mutate_set_default_zone(
    cli: &Cli,
    client: &mut BridgeClient,
    channel: &str,
    host: String,
    zone: &str,
) -> Result<View> {
    crate::safety::check_firewall_default_zone(cli.force)?;
    run_audited(
        client,
        channel,
        &host,
        AuditedFirewallCall::new(
            "set-default-zone",
            zone,
            FW_IFACE,
            "setDefaultZone",
            json!([zone]),
        ),
    )?;
    Ok(change_view(
        host,
        "set-default-zone",
        zone,
        "default zone",
        None,
    ))
}

/// `firewall reload`: re-apply permanent config. Reads live runtime-vs-
/// permanent drift so the safety guard ([`crate::safety::check_firewall_reload`])
/// can warn about discarding uncommitted runtime changes.
fn mutate_reload(
    cli: &Cli,
    client: &mut BridgeClient,
    channel: &str,
    host: String,
) -> Result<View> {
    let default_zone = arg_str(&fw_call(
        client,
        channel,
        FW_IFACE,
        "getDefaultZone",
        json!([]),
    )?);
    let runtime = runtime_zone(client, channel, &default_zone)?;
    let has_drift = match permanent_zone(client, channel, &default_zone) {
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
    run_audited(
        client,
        channel,
        &host,
        AuditedFirewallCall::new("reload", "firewall", FW_IFACE, "reload", json!([])),
    )?;
    Ok(reload_view(host))
}

/// `firewall confirm`: commit runtime changes to permanent
/// (`runtimeToPermanent`).
fn mutate_confirm(client: &mut BridgeClient, channel: &str, host: String) -> Result<View> {
    run_audited(
        client,
        channel,
        &host,
        AuditedFirewallCall::new(
            "confirm",
            "firewall",
            FW_IFACE,
            "runtimeToPermanent",
            json!([]),
        ),
    )?;
    Ok(confirm_view(host))
}

/// `firewall panic on|off`: toggle panic mode. Turning it *on* drops all
/// traffic, so it is gated by [`crate::safety::check_firewall_panic_on`].
fn mutate_panic(
    cli: &Cli,
    client: &mut BridgeClient,
    channel: &str,
    host: String,
    state: &str,
) -> Result<View> {
    let on = state == "on";
    if on {
        crate::safety::check_firewall_panic_on(cli.force)?;
    }
    let method = if on {
        "enablePanicMode"
    } else {
        "disablePanicMode"
    };
    run_audited(
        client,
        channel,
        &host,
        AuditedFirewallCall::new(
            format!("panic-{state}"),
            "firewall",
            FW_IFACE,
            method,
            json!([]),
        ),
    )?;
    Ok(panic_view(host, on))
}

/// `firewall masquerade on|off`: toggle NAT masquerade in a zone. Turning it
/// *off* is gated by [`crate::safety::check_firewall_masquerade_off`].
fn mutate_masquerade(
    cli: &Cli,
    client: &mut BridgeClient,
    channel: &str,
    host: String,
    state: &str,
    zone: Option<&str>,
    timeout: Option<u32>,
) -> Result<View> {
    let on = state == "on";
    let zone = effective_zone(client, channel, zone)?;
    if !on {
        crate::safety::check_firewall_masquerade_off(cli.force)?;
    }
    let (method, args) = if on {
        let t = i64::from(timeout.unwrap_or(0));
        ("addMasquerade", json!([zone, t]))
    } else {
        ("removeMasquerade", json!([zone]))
    };
    run_audited(
        client,
        channel,
        &host,
        AuditedFirewallCall::new(
            format!("masquerade-{state}"),
            zone.as_str(),
            FW_ZONE_IFACE,
            method,
            args,
        ),
    )?;
    Ok(masquerade_view(
        host,
        &zone,
        on,
        if on { timeout } else { None },
    ))
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
fn run_audited(
    client: &mut BridgeClient,
    channel: &str,
    host: &str,
    call: AuditedFirewallCall,
) -> Result<()> {
    let sink = crate::audit::sink_from_env();
    let ctx = crate::audit::AuditContext::new(
        &crate::audit::actor(),
        host,
        &call.operation,
        &call.target,
        &crate::audit::correlation_id(),
    );
    sink.write(&ctx.record(crate::audit::Outcome::Attempt));
    let exec = fw_call(client, channel, call.iface, call.method, call.args);
    match &exec {
        Ok(_) => sink.write(&ctx.record(crate::audit::Outcome::Ok)),
        Err(e) => sink.write(&ctx.record(crate::audit::Outcome::Error(e.to_string()))),
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
struct RuntimeChangeData {
    operation: String,
    zone: String,
    change: String,
    timeout: Option<u32>,
}

impl RuntimeChangeData {
    fn new(operation: &str, zone: &str, change: &str, timeout: Option<u32>) -> Self {
        Self {
            operation: operation.to_string(),
            zone: zone.to_string(),
            change: change.to_string(),
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
struct MasqueradeChangeData {
    zone: String,
    on: bool,
    timeout: Option<u32>,
}

impl MasqueradeChangeData {
    fn new(zone: &str, on: bool, timeout: Option<u32>) -> Self {
        Self {
            zone: zone.to_string(),
            on,
            timeout,
        }
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
fn change_view(host: String, op: &str, zone: &str, what: &str, timeout: Option<u32>) -> View {
    let change = RuntimeChangeData::new(op, zone, what, timeout);
    View::new("FirewallChange", host, change.data(), change.human()).with_hints(confirm_hint())
}

/// Build the `FirewallChange` view for `reload`.
fn reload_view(host: String) -> View {
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
fn confirm_view(host: String) -> View {
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
fn panic_view(host: String, on: bool) -> View {
    let change = PanicChangeData { on };
    View::new("FirewallChange", host, change.data(), change.human())
}

/// Build the `FirewallChange` view for `masquerade on|off`.
fn masquerade_view(host: String, zone: &str, on: bool, timeout: Option<u32>) -> View {
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
