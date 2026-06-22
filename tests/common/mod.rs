//! Shared test support for the integration suites.
//!
//! Every integration file (`packages`, `services`, `network`, `firewall`)
//! drives the same `fez-fake-bridge` binary and repeats the same scaffolding:
//! point fez at the fake via `FEZ_BRIDGE`, silence the audit sink so tests do
//! not depend on a journal socket, and (for mutation tests) capture the
//! append-only audit log from a unique temp file. This module centralizes that
//! scaffolding so the per-suite files only express what is unique to them.
//!
//! Included via `mod common;` in each integration file. Cargo compiles every
//! file under `tests/` as its own crate, so each suite pulls in only the items
//! it references; `#![allow(dead_code)]` keeps the unused-in-this-crate items
//! from tripping warnings.
#![allow(dead_code)]

use assert_cmd::Command as AssertCommand;
use serde_json::Value;

/// Columns of a `PackageList` envelope, in wire order.
///
/// The `packages list` JSON states its column names once and emits items as
/// positional rows; several tests assert that exact header, so it lives here as
/// a single source of truth.
pub const PKG_COLUMNS: &str =
    "\"columns\":[\"name\",\"evr\",\"arch\",\"repo_id\",\"install_size\",\"summary\"]";

/// A bare `fez` command with no bridge wired up.
///
/// For tests that exercise CLI surface only (help, version, `describe`,
/// `guide`) and never open a bridge channel, so leaving
/// `FEZ_BRIDGE` unset is fine.
#[must_use]
pub fn fez_plain() -> AssertCommand {
    AssertCommand::cargo_bin("fez").unwrap()
}

/// A `fez` command wired to the fake bridge.
///
/// Sets `FEZ_BRIDGE` to the `fez-fake-bridge` test binary so the command talks
/// to the in-repo fake instead of a real `cockpit-bridge`. The audit sink is
/// left at its default; tests that must not touch the journal should use
/// [`fez_fake_quiet`] or chain `.env("FEZ_AUDIT", "off")` themselves.
#[must_use]
pub fn fez_fake() -> AssertCommand {
    let mut c = AssertCommand::cargo_bin("fez").unwrap();
    c.env("FEZ_BRIDGE", env!("CARGO_BIN_EXE_fez-fake-bridge"));
    c
}

/// A [`fez_fake`] command with the audit sink disabled.
///
/// Adds `FEZ_AUDIT=off` so the run does not depend on a journal socket being
/// present. Use for read-only assertions and any mutation test that does not
/// inspect the audit log.
#[must_use]
pub fn fez_fake_quiet() -> AssertCommand {
    let mut c = fez_fake();
    c.env("FEZ_AUDIT", "off");
    c
}

/// A `fez` command wired to the fake bridge with dnf5daemon forced absent, so
/// the PackageKit fallback backend is exercised.
///
/// Inherits [`fez_fake_quiet`]'s bridge wiring and disabled audit sink, and adds
/// `FEZ_FAKE_NO_DNF5` so `open_session` returns ServiceUnknown and fez falls
/// back to PackageKit (the RHEL 10 path).
#[must_use]
pub fn fez_fake_pk() -> AssertCommand {
    let mut c = fez_fake_quiet();
    c.env("FEZ_FAKE_NO_DNF5", "1");
    c
}

/// A scratch audit-log file that cleans itself up on drop.
///
/// Wraps a process-unique temp path: the file is removed on construction (in
/// case a prior run leaked it) and again when the guard drops, so a panic
/// mid-test never leaves a stray `.jsonl` behind. Pass [`AuditLog::env_value`]
/// to `FEZ_AUDIT`, run the command, then read parsed records with
/// [`AuditLog::records`].
pub struct AuditLog {
    path: std::path::PathBuf,
}

impl AuditLog {
    /// Create a guard for a process-unique audit file tagged with `label`.
    ///
    /// `label` distinguishes concurrent suites (e.g. `"audit-it"`,
    /// `"pkg-audit"`) so their temp paths never collide. Any pre-existing file
    /// at the path is removed.
    #[must_use]
    pub fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!("fez-{label}-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&path);
        Self { path }
    }

    /// The `FEZ_AUDIT` value selecting this file as the audit sink.
    #[must_use]
    pub fn env_value(&self) -> String {
        format!("file:{}", self.path.display())
    }

    /// Read the audit log and parse each line as a JSON record.
    ///
    /// # Panics
    ///
    /// Panics if the file is missing or any line is not valid JSON, which in a
    /// test means the command under test failed to write the expected records.
    #[must_use]
    pub fn records(&self) -> Vec<Value> {
        let body = std::fs::read_to_string(&self.path).unwrap();
        body.lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }
}

impl Drop for AuditLog {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
