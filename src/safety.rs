//! Pre-flight safety decisions: protected-unit policy and TTY-gated confirmation.
//! Pure functions only — no I/O, no bridge — so the policy is exhaustively testable.

use crate::error::{FezError, Result};

/// Default protected-unit patterns. A bare name matches exactly; a `*`-suffixed
/// pattern matches by prefix. These guard the agent's own access path
/// (SSH + Cockpit) and `fez`'s own unit (Section 8, layer 3).
const PROTECTED: &[&str] = &[
    "sshd.service",
    "sshd.socket",
    "ssh.service",
    "ssh.socket",
    "cockpit*",
    "fez*",
];

fn matches_pattern(pattern: &str, unit: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => unit.starts_with(prefix),
        None => unit == pattern,
    }
}

/// The first protected pattern this unit matches, if any.
pub fn protected_match(unit: &str) -> Option<&'static str> {
    PROTECTED.iter().copied().find(|p| matches_pattern(p, unit))
}

/// Refuse a mutation on a protected unit unless `force` is set.
pub fn check_protected(unit: &str, force: bool) -> Result<()> {
    if !force && protected_match(unit).is_some() {
        return Err(FezError::Protected {
            unit: unit.to_string(),
        });
    }
    Ok(())
}

/// Default protected-package patterns. A bare name matches exactly; a
/// `*`-suffixed pattern matches by prefix. These guard the host's bootability
/// (kernel, bootloader, early-boot, package-manager libraries) and the agent's
/// own access path (SSH + Cockpit + fez's transport).
const PROTECTED_PACKAGES: &[&str] = &[
    "kernel*",
    "systemd*",
    "glibc",
    "dnf*",
    "rpm*",
    "sudo",
    "openssh-server",
    "cockpit*",
    "dbus*",
    "coreutils*",
    "bash",
    "grub2*",
    "shim*",
    "dracut*",
    "linux-firmware",
];

/// Maximum packages a removal plan may remove before it is treated as a
/// dangerous cascade (refused without `--force`).
const CASCADE_LIMIT: usize = 20;

/// The first protected-package pattern this package name matches, if any.
pub fn protected_package_match(name: &str) -> Option<&'static str> {
    PROTECTED_PACKAGES
        .iter()
        .copied()
        .find(|p| matches_pattern(p, name))
}

/// Refuse a resolved removal plan that removes a protected package or exceeds
/// the cascade limit, unless `force` is set.
///
/// # Errors
///
/// Returns [`FezError::DangerousTransaction`] when `removed` contains a
/// protected package or has more than [`CASCADE_LIMIT`] entries and `force`
/// is false.
pub fn check_removal_plan(removed: &[String], force: bool) -> Result<()> {
    if force {
        return Ok(());
    }
    if let Some(p) = removed
        .iter()
        .find(|n| protected_package_match(n).is_some())
    {
        return Err(FezError::DangerousTransaction {
            reason: format!("removes protected package {p}"),
            removed: removed.to_vec(),
        });
    }
    if removed.len() > CASCADE_LIMIT {
        return Err(FezError::DangerousTransaction {
            reason: format!(
                "removes {} packages (cascade limit {CASCADE_LIMIT})",
                removed.len()
            ),
            removed: removed.to_vec(),
        });
    }
    Ok(())
}

/// Refuse removing a firewall service that carries the active session, unless
/// `force` is set. `session_services` is the set treated as session-critical
/// (always includes `ssh`; the capability supplies it).
///
/// # Errors
///
/// Returns [`FezError::Protected`] (exit 8) when `service` is in
/// `session_services` and `force` is false.
pub fn check_firewall_service_removal(
    service: &str,
    session_services: &[String],
    force: bool,
) -> Result<()> {
    if !force && session_services.iter().any(|s| s == service) {
        return Err(FezError::Protected {
            unit: format!("firewall service {service} (carries the active session)"),
        });
    }
    Ok(())
}

/// Refuse removing a firewall port that carries the active session, unless
/// `force` is set. `session_ports` is the set treated as session-critical (the
/// `SSH_CONNECTION` server port when present).
///
/// # Errors
///
/// Returns [`FezError::Protected`] (exit 8) when `port` is in `session_ports`
/// and `force` is false.
pub fn check_firewall_port_removal(port: u16, session_ports: &[u16], force: bool) -> Result<()> {
    if !force && session_ports.contains(&port) {
        return Err(FezError::Protected {
            unit: format!("firewall port {port} (carries the active session)"),
        });
    }
    Ok(())
}

/// Refuse any default-zone change unless `force` is set. fez cannot rank zone
/// strictness statelessly, so every default-zone change is gated.
///
/// # Errors
///
/// Returns [`FezError::Protected`] (exit 8) when `force` is false.
pub fn check_firewall_default_zone(force: bool) -> Result<()> {
    if !force {
        return Err(FezError::Protected {
            unit: "firewall default zone change".into(),
        });
    }
    Ok(())
}

/// Refuse enabling panic mode (drops all traffic) unless `force` is set.
///
/// # Errors
///
/// Returns [`FezError::Protected`] (exit 8) when `force` is false.
pub fn check_firewall_panic_on(force: bool) -> Result<()> {
    if !force {
        return Err(FezError::Protected {
            unit: "firewall panic mode".into(),
        });
    }
    Ok(())
}

/// Refuse to disable masquerade without `--force`.
///
/// Disabling masquerade drops SNAT for forwarded traffic, which can silently
/// break a gateway host's forwarded clients. It does not touch the fez
/// session's own input path (that is not forwarded), so this is a flat
/// deliberate-action gate, not a session-criticality check. Enabling
/// masquerade is unguarded.
///
/// # Errors
///
/// Returns [`FezError::Protected`] (exit 8) when `force` is false.
pub fn check_firewall_masquerade_off(force: bool) -> Result<()> {
    if !force {
        return Err(FezError::Protected {
            unit: "firewall masquerade disable".into(),
        });
    }
    Ok(())
}

/// Refuse a reload that would discard uncommitted runtime drift, unless `force`
/// is set. With no drift the reload is harmless and always allowed.
///
/// # Errors
///
/// Returns [`FezError::Protected`] (exit 8) when `has_drift` is true and
/// `force` is false.
pub fn check_firewall_reload(has_drift: bool, force: bool) -> Result<()> {
    if has_drift && !force {
        return Err(FezError::Protected {
            unit: "firewall reload (would discard uncommitted runtime changes)".into(),
        });
    }
    Ok(())
}

/// Whether to interactively confirm: only a human (TTY) running a destructive
/// op without `--force`. Agents (non-TTY) never prompt; layers 1-5 carry them.
pub fn should_prompt(destructive: bool, is_tty: bool, force: bool) -> bool {
    destructive && is_tty && !force
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_exact_and_glob() {
        assert_eq!(protected_match("sshd.service"), Some("sshd.service"));
        assert_eq!(protected_match("cockpit.service"), Some("cockpit*"));
        assert_eq!(protected_match("cockpit.socket"), Some("cockpit*"));
        assert_eq!(protected_match("fez.service"), Some("fez*"));
        assert_eq!(protected_match("chronyd.service"), None);
    }

    #[test]
    fn check_refuses_protected_without_force() {
        let err = check_protected("sshd.service", false).unwrap_err();
        assert_eq!(err.code(), "protected-unit");
    }

    #[test]
    fn check_allows_protected_with_force() {
        assert!(check_protected("sshd.service", true).is_ok());
    }

    #[test]
    fn check_allows_unprotected() {
        assert!(check_protected("chronyd.service", false).is_ok());
    }

    #[test]
    fn prompt_only_for_destructive_human_without_force() {
        assert!(should_prompt(true, true, false)); // destructive, TTY, no force
        assert!(!should_prompt(true, false, false)); // agent: never
        assert!(!should_prompt(true, true, true)); // force overrides
        assert!(!should_prompt(false, true, false)); // non-destructive: never
    }

    #[test]
    fn protected_package_exact_and_prefix() {
        assert_eq!(protected_package_match("glibc"), Some("glibc"));
        assert_eq!(
            protected_package_match("kernel-6.11.3-300.fc41"),
            Some("kernel*")
        );
        assert_eq!(protected_package_match("systemd-libs"), Some("systemd*"));
        assert_eq!(protected_package_match("htop"), None);
    }

    #[test]
    fn removal_plan_refuses_protected_without_force() {
        let removed = vec!["htop".to_string(), "glibc".to_string()];
        let err = check_removal_plan(&removed, false).unwrap_err();
        assert_eq!(err.code(), "dangerous-transaction");
    }

    #[test]
    fn removal_plan_allows_protected_with_force() {
        let removed = vec!["glibc".to_string()];
        assert!(check_removal_plan(&removed, true).is_ok());
    }

    #[test]
    fn removal_plan_refuses_large_cascade_without_force() {
        let removed: Vec<String> = (0..21).map(|i| format!("pkg{i}")).collect();
        let err = check_removal_plan(&removed, false).unwrap_err();
        assert_eq!(err.code(), "dangerous-transaction");
    }

    #[test]
    fn removal_plan_allows_small_cascade() {
        let removed: Vec<String> = (0..5).map(|i| format!("pkg{i}")).collect();
        assert!(check_removal_plan(&removed, false).is_ok());
    }

    #[test]
    fn removal_plan_allows_exactly_cascade_limit() {
        let removed: Vec<String> = (0..20).map(|i| format!("pkg{i}")).collect();
        assert!(check_removal_plan(&removed, false).is_ok());
    }

    #[test]
    fn removal_plan_allows_large_cascade_with_force() {
        let removed: Vec<String> = (0..50).map(|i| format!("pkg{i}")).collect();
        assert!(check_removal_plan(&removed, true).is_ok());
    }

    #[test]
    fn removal_plan_allows_empty() {
        assert!(check_removal_plan(&[], false).is_ok());
    }

    #[test]
    fn firewall_refuses_removing_session_service_without_force() {
        // Removing `ssh` when ssh is session-critical is protected.
        let err = check_firewall_service_removal("ssh", &["ssh".to_string()], false).unwrap_err();
        assert_eq!(err.code(), "protected-unit");
    }

    #[test]
    fn firewall_allows_removing_session_service_with_force() {
        assert!(check_firewall_service_removal("ssh", &["ssh".to_string()], true).is_ok());
    }

    #[test]
    fn firewall_allows_removing_non_session_service() {
        assert!(check_firewall_service_removal("http", &["ssh".to_string()], false).is_ok());
    }

    #[test]
    fn firewall_refuses_removing_session_port_without_force() {
        // The session-carrying port is protected; 22 here stands in for the
        // SSH_CONNECTION server port.
        let err = check_firewall_port_removal(22, &[22], false).unwrap_err();
        assert_eq!(err.code(), "protected-unit");
    }

    #[test]
    fn firewall_allows_removing_session_port_with_force() {
        assert!(check_firewall_port_removal(22, &[22], true).is_ok());
    }

    #[test]
    fn firewall_allows_removing_non_session_port() {
        assert!(check_firewall_port_removal(8080, &[22], false).is_ok());
    }

    #[test]
    fn firewall_refuses_default_zone_change_without_force() {
        assert_eq!(
            check_firewall_default_zone(false).unwrap_err().code(),
            "protected-unit"
        );
    }

    #[test]
    fn firewall_allows_default_zone_change_with_force() {
        assert!(check_firewall_default_zone(true).is_ok());
    }

    #[test]
    fn firewall_refuses_panic_on_without_force() {
        assert_eq!(
            check_firewall_panic_on(false).unwrap_err().code(),
            "protected-unit"
        );
    }

    #[test]
    fn firewall_allows_panic_on_with_force() {
        assert!(check_firewall_panic_on(true).is_ok());
    }

    #[test]
    fn firewall_masquerade_off_requires_force() {
        assert_eq!(
            check_firewall_masquerade_off(false).unwrap_err().code(),
            "protected-unit"
        );
    }

    #[test]
    fn firewall_masquerade_off_allowed_with_force() {
        assert!(check_firewall_masquerade_off(true).is_ok());
    }

    #[test]
    fn firewall_reload_free_without_drift() {
        // No drift: reload is safe, never gated.
        assert!(check_firewall_reload(false, false).is_ok());
    }

    #[test]
    fn firewall_reload_refused_with_drift_without_force() {
        assert_eq!(
            check_firewall_reload(true, false).unwrap_err().code(),
            "protected-unit"
        );
    }

    #[test]
    fn firewall_reload_allowed_with_drift_and_force() {
        assert!(check_firewall_reload(true, true).is_ok());
    }
}
