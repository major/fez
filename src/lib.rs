//! fez: an agent-friendly front end for systemd operations over local and SSH
//! transports.
//!
//! The crate is structured as a library plus thin binaries so the fake bridge
//! and integration tests can reuse the protocol modules.

#![deny(missing_docs)]

/// JSON-lines audit logging of attempted and completed mutations.
pub mod audit;
/// Concrete capability implementations (the commands fez runs).
pub mod capabilities;
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
/// Machine-readable descriptors of the capability surface.
pub mod schema;
/// Local and SSH transports for reaching the bridge.
pub mod transport;

/// Restore the default `SIGPIPE` disposition on Unix.
///
/// Rust installs `SIG_IGN` for `SIGPIPE` at startup, which turns a closed
/// downstream pipe (`fez ... | head`) into an `EPIPE` write error. The
/// `print!`/`println!` macros then `panic!`, so fez exits 101 with a backtrace
/// hint instead of stopping quietly. Resetting the disposition to `SIG_DFL`
/// makes the process die from `SIGPIPE` the Unix way, matching coreutils, git,
/// and most other CLIs. This is a no-op on non-Unix targets.
///
/// Call this once, as early as possible in `main`, before any stdout writes.
pub fn reset_sigpipe() {
    #[cfg(unix)]
    unix_sigpipe::reset();
}

/// Unix `SIGPIPE` handling, isolated so the FFI stays in one place.
#[cfg(unix)]
mod unix_sigpipe {
    /// `SIGPIPE` signal number (13 on Linux and macOS).
    const SIGPIPE: i32 = 13;
    /// `SIG_DFL` handler sentinel: take the default action.
    const SIG_DFL: usize = 0;
    /// `SIG_IGN` handler sentinel: ignore the signal (Rust's startup default).
    #[cfg(test)]
    const SIG_IGN: usize = 1;

    extern "C" {
        /// libc `signal(2)`: install `handler` for `signum`, returning the
        /// previous handler.
        fn signal(signum: i32, handler: usize) -> usize;
    }

    /// Set `SIGPIPE` to `SIG_DFL`, returning the previous disposition.
    pub fn reset() -> usize {
        // SAFETY: `signal` has a stable C ABI. We pass the well-known `SIGPIPE`
        // number and the `SIG_DFL` sentinel; the call has no memory effects we
        // rely on, and we simply forward its return value (the old handler).
        unsafe { signal(SIGPIPE, SIG_DFL) }
    }

    #[cfg(test)]
    mod tests {
        use super::{reset, SIG_DFL, SIG_IGN};

        #[test]
        fn reset_moves_disposition_from_ignore_to_default() {
            // Rust installs SIG_IGN at startup, so the first reset reports that
            // as the previous disposition; a second reset then sees SIG_DFL,
            // proving the first call took effect.
            let previous = reset();
            let after = reset();
            assert!(previous == SIG_IGN || previous == SIG_DFL);
            assert_eq!(after, SIG_DFL);
        }

        #[test]
        fn public_reset_sigpipe_is_callable() {
            // Exercise the public wrapper so it stays covered; idempotent and
            // safe to call repeatedly within the test process.
            crate::reset_sigpipe();
            assert_eq!(reset(), SIG_DFL);
        }
    }
}

/// Entry point: parse-to-exit. Returns the process exit code.
pub fn run(cli: cli::Cli) -> i32 {
    dispatch::run(cli)
}
