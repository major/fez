//! fez: an agent-friendly front end for systemd operations over local and SSH
//! transports.
//!
//! The crate is structured as a library plus thin binaries so the fake bridge
//! and integration tests can reuse the protocol modules.

/// JSON-lines audit logging of attempted and completed mutations.
pub mod audit;
/// Concrete capability implementations (the commands fez runs).
pub mod capabilities;
/// Machine-readable descriptors of the capability surface.
pub mod capability;
/// Command-line argument definitions.
pub mod cli;
mod dispatch;
/// The `fez/v1` JSON response envelope.
pub mod envelope;
/// Crate-wide error type and exit-code mapping.
pub mod error;
/// The agent bootstrap contract printed by `fez guide`.
pub mod guide;
/// Model Context Protocol server support.
pub mod mcp;
/// Wire protocol between fez and the bridge.
pub mod protocol;
/// Guardrails around destructive operations (protected units, confirmations).
pub mod safety;
/// Local and SSH transports for reaching the bridge.
pub mod transport;

/// Entry point: parse-to-exit. Returns the process exit code.
pub fn run(cli: cli::Cli) -> i32 {
    dispatch::run(cli)
}
