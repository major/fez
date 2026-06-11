//! Capability implementations: the concrete commands fez exposes.

/// systemd service management capabilities.
pub mod services;

/// RPM package management capabilities (via dnf5daemon).
pub mod packages;

/// PackageKit fallback package backend (used when dnf5daemon is absent).
pub mod packages_pk;

/// NetworkManager inspection capabilities.
pub mod network;

/// firewalld management capabilities.
pub mod firewall;
