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
}
