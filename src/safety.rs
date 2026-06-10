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
/// and the agent's own access path (SSH + Cockpit + fez's transport).
const PROTECTED_PACKAGES: &[&str] = &[
    "kernel*",
    "systemd*",
    "glibc",
    "dnf*",
    "rpm",
    "sudo",
    "openssh-server",
    "cockpit*",
    "dbus*",
    "coreutils*",
    "bash",
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
    fn removal_plan_allows_empty() {
        assert!(check_removal_plan(&[], false).is_ok());
    }
}
