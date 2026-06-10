//! Capability implementations: the concrete commands fez exposes.

/// systemd service management capabilities.
pub mod services;

/// RPM package management capabilities (via dnf5daemon).
pub mod packages;

/// NetworkManager inspection capabilities.
pub mod network;
