//! Structured audit for mutations (Section 8, layer 4). Records are written to a
//! pluggable sink; the default writes to the systemd journal via its native
//! protocol over a datagram socket. Debug/test builds may select `off` or
//! `file:<path>` via `FEZ_AUDIT`; release builds keep journald.
use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

/// A stable MESSAGE_ID (128-bit hex, journald convention) tagging fez mutations.
const FEZ_MUTATION_MESSAGE_ID: &str = "fe2c0ffee0000400a8b1mutati0naud1";

/// One audit event describing an attempted or completed mutation.
#[derive(Serialize, Clone, Debug)]
pub struct AuditRecord {
    /// Identity that initiated the operation (see [`actor`]).
    pub actor: String,
    /// Host the operation targets.
    pub target_host: String,
    /// The mutation performed (e.g. `start`, `stop`, `restart`).
    pub operation: String,
    /// The systemd unit the operation acts on.
    pub unit: String,
    /// "attempt" | "ok" | "error"
    pub result: String,
    /// Error detail when `result` is `"error"`, otherwise omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Correlation id linking all records for one invocation (see [`correlation_id`]).
    pub correlation_id: String,
    /// Record creation time as Unix milliseconds.
    pub timestamp_unix_ms: u128,
}

impl AuditRecord {
    fn priority(&self) -> &'static str {
        match self.result.as_str() {
            "error" => "3",   // err
            "attempt" => "5", // notice
            _ => "6",         // info
        }
    }
}

/// The result of one mutation step, replacing a stringly-typed `result` plus a
/// loosely-coupled `error`. The error payload exists only on [`Outcome::Error`],
/// so a record can never claim success while carrying an error (or vice versa).
#[derive(Clone, Debug)]
pub enum Outcome {
    /// The mutation is about to run (audited before any side effect).
    Attempt,
    /// The mutation completed successfully.
    Ok,
    /// The mutation failed; the payload is the error detail.
    Error(String),
}

impl Outcome {
    /// The wire `result` string (`"attempt"`, `"ok"`, or `"error"`).
    fn result(&self) -> &'static str {
        match self {
            Outcome::Attempt => "attempt",
            Outcome::Ok => "ok",
            Outcome::Error(_) => "error",
        }
    }

    /// The error detail, present only for [`Outcome::Error`].
    fn into_error(self) -> Option<String> {
        match self {
            Outcome::Error(e) => Some(e),
            _ => None,
        }
    }
}

/// The invocation-shared context for a sequence of audit records. One mutation
/// produces several records (attempt, then ok/error) that agree on actor, host,
/// operation, unit, and correlation id; capture those once here and stamp each
/// [`Outcome`] via [`AuditContext::record`].
#[derive(Clone, Debug)]
pub struct AuditContext {
    actor: String,
    target_host: String,
    operation: String,
    unit: String,
    correlation_id: String,
}

impl AuditContext {
    /// Capture the fields shared by every record for one mutation invocation.
    pub fn new(
        actor: &str,
        target_host: &str,
        operation: &str,
        unit: &str,
        correlation_id: &str,
    ) -> Self {
        AuditContext {
            actor: actor.into(),
            target_host: target_host.into(),
            operation: operation.into(),
            unit: unit.into(),
            correlation_id: correlation_id.into(),
        }
    }

    /// Build a record for `outcome`, stamping the current Unix-millisecond time.
    #[must_use]
    pub fn record(&self, outcome: Outcome) -> AuditRecord {
        let timestamp_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let result = outcome.result().to_string();
        AuditRecord {
            actor: self.actor.clone(),
            target_host: self.target_host.clone(),
            operation: self.operation.clone(),
            unit: self.unit.clone(),
            result,
            error: outcome.into_error(),
            correlation_id: self.correlation_id.clone(),
            timestamp_unix_ms,
        }
    }
}

/// Run a mutation under the standard audit envelope: write an `attempt`
/// record, execute `action`, then write an `ok`/`error` record from its result,
/// and return that result unchanged.
///
/// The sink comes from [`sink_from_env`] and the [`AuditContext`] is built from
/// [`actor`]/[`correlation_id`] plus the supplied `host`/`operation`/`unit`,
/// so every mutating capability emits the identical attempt+result pair without
/// repeating the wiring. `action` runs exactly once.
///
/// # Errors
///
/// Propagates `action`'s error unchanged; the audit writes are best-effort and
/// never alter the returned [`crate::error::Result`].
pub fn run_audited<T, F>(
    host: &str,
    operation: &str,
    unit: &str,
    action: F,
) -> crate::error::Result<T>
where
    F: FnOnce() -> crate::error::Result<T>,
{
    run_audited_with(sink_from_env().as_ref(), host, operation, unit, action)
}

/// [`run_audited`] against an explicit sink. Carries the attempt/result wiring;
/// [`run_audited`] is the production wrapper that supplies [`sink_from_env`].
///
/// # Errors
///
/// Propagates `action`'s error unchanged; the [`AuditSink::write`] calls are
/// best-effort and never alter the returned [`crate::error::Result`].
pub fn run_audited_with<T, F>(
    sink: &dyn AuditSink,
    host: &str,
    operation: &str,
    unit: &str,
    action: F,
) -> crate::error::Result<T>
where
    F: FnOnce() -> crate::error::Result<T>,
{
    let ctx = AuditContext::new(&actor(), host, operation, unit, &correlation_id());
    sink.write(&ctx.record(Outcome::Attempt));
    let result = action();
    match &result {
        Ok(_) => sink.write(&ctx.record(Outcome::Ok)),
        Err(e) => sink.write(&ctx.record(Outcome::Error(e.to_string()))),
    }
    result
}

/// The actor identity from OS user ids, not spoofable environment names.
pub fn actor() -> String {
    // SAFETY: `getuid` has no preconditions and does not access Rust-managed memory.
    let uid = unsafe { libc::getuid() };
    // SAFETY: `geteuid` has no preconditions and does not access Rust-managed memory.
    let euid = unsafe { libc::geteuid() };
    actor_from_uids(uid, euid)
}

fn actor_from_uids(uid: libc::uid_t, euid: libc::uid_t) -> String {
    if uid == euid {
        format!("uid:{uid}")
    } else {
        format!("uid:{uid} euid:{euid}")
    }
}

/// A best-effort-unique correlation id for one invocation's records.
pub fn correlation_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:x}-{pid:x}-{seq:x}")
}

/// Encode one journal field in the systemd native protocol. Values without a
/// newline use `KEY=VALUE\n`; values with a newline use the binary form:
/// `KEY\n` + little-endian u64 length + raw bytes + `\n`.
fn push_field(out: &mut Vec<u8>, key: &str, value: &str) {
    out.extend_from_slice(key.as_bytes());
    if value.contains('\n') {
        // Binary form: KEY\n + LE u64 length + raw bytes + \n.
        out.push(b'\n');
        out.extend_from_slice(&(value.len() as u64).to_le_bytes());
    } else {
        // Plain form: KEY=VALUE\n.
        out.push(b'=');
    }
    out.extend_from_slice(value.as_bytes());
    out.push(b'\n');
}

/// Encode an audit record as systemd journal native-protocol fields.
pub fn encode_journal_fields(rec: &AuditRecord) -> Vec<u8> {
    let mut out = Vec::new();
    let message = format!(
        "fez {} {} on {}: {}",
        rec.operation, rec.unit, rec.target_host, rec.result
    );
    push_field(&mut out, "MESSAGE", &message);
    push_field(&mut out, "MESSAGE_ID", FEZ_MUTATION_MESSAGE_ID);
    push_field(&mut out, "SYSLOG_IDENTIFIER", "fez");
    push_field(&mut out, "PRIORITY", rec.priority());
    push_field(&mut out, "FEZ_ACTOR", &rec.actor);
    push_field(&mut out, "FEZ_TARGET_HOST", &rec.target_host);
    push_field(&mut out, "FEZ_OPERATION", &rec.operation);
    push_field(&mut out, "FEZ_UNIT", &rec.unit);
    push_field(&mut out, "FEZ_RESULT", &rec.result);
    push_field(&mut out, "FEZ_CORRELATION_ID", &rec.correlation_id);
    if let Some(err) = &rec.error {
        push_field(&mut out, "FEZ_ERROR", err);
    }
    out
}

/// A destination that persists [`AuditRecord`]s.
pub trait AuditSink {
    /// Write one record. Implementations are best-effort and must not panic.
    fn write(&self, rec: &AuditRecord);
}

/// Discards records (used when `FEZ_AUDIT=off`).
pub struct NoopSink;
impl AuditSink for NoopSink {
    fn write(&self, _rec: &AuditRecord) {}
}

/// Appends each record as a JSON line (used by tests and the E2E harness).
pub struct FileSink {
    /// File the JSON lines are appended to.
    pub path: std::path::PathBuf,
}
impl AuditSink for FileSink {
    fn write(&self, rec: &AuditRecord) {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(&self.path)
        {
            if f.set_permissions(std::fs::Permissions::from_mode(0o600))
                .is_err()
            {
                return;
            }
            if let Ok(line) = serde_json::to_string(rec) {
                let _ = writeln!(f, "{line}");
            }
        }
    }
}

/// Writes to the systemd journal via its native datagram protocol. Best-effort:
/// audit must never break a mutation, so a missing socket is silently ignored.
pub struct JournalSink;
impl AuditSink for JournalSink {
    fn write(&self, rec: &AuditRecord) {
        let buf = encode_journal_fields(rec);
        if let Ok(sock) = std::os::unix::net::UnixDatagram::unbound() {
            let _ = sock.send_to(&buf, "/run/systemd/journal/socket");
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum SinkSelection {
    Journal,
    Noop,
    File(std::path::PathBuf),
}

fn select_sink(raw: Option<&str>, allow_unsafe_env: bool) -> SinkSelection {
    if !allow_unsafe_env {
        return SinkSelection::Journal;
    }
    match raw {
        Some("off") | Some("0") => SinkSelection::Noop,
        Some(v) if v.starts_with("file:") => {
            SinkSelection::File(std::path::PathBuf::from(&v["file:".len()..]))
        }
        _ => SinkSelection::Journal,
    }
}

/// Select a sink from the `FEZ_AUDIT` environment variable in debug/test builds.
pub fn sink_from_env() -> Box<dyn AuditSink> {
    match select_sink(
        std::env::var("FEZ_AUDIT").ok().as_deref(),
        cfg!(debug_assertions),
    ) {
        SinkSelection::Journal => Box::new(JournalSink),
        SinkSelection::Noop => Box::new(NoopSink),
        SinkSelection::File(path) => Box::new(FileSink { path }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> AuditContext {
        AuditContext::new("alice", "localhost", "stop", "chronyd.service", "abc-1-0")
    }

    fn rec(result: &str, error: Option<String>) -> AuditRecord {
        let outcome = match (result, error) {
            ("attempt", _) => Outcome::Attempt,
            ("ok", _) => Outcome::Ok,
            ("error", Some(e)) => Outcome::Error(e),
            ("error", None) => Outcome::Error(String::new()),
            (other, _) => panic!("unexpected result {other}"),
        };
        ctx().record(outcome)
    }

    #[test]
    fn context_records_share_invocation_fields() {
        let c = ctx();
        let attempt = c.record(Outcome::Attempt);
        let ok = c.record(Outcome::Ok);
        assert_eq!(attempt.actor, "alice");
        assert_eq!(attempt.target_host, "localhost");
        assert_eq!(attempt.operation, "stop");
        assert_eq!(attempt.unit, "chronyd.service");
        assert_eq!(attempt.correlation_id, "abc-1-0");
        // Same context produces records that agree on every shared field.
        assert_eq!(attempt.operation, ok.operation);
        assert_eq!(attempt.correlation_id, ok.correlation_id);
    }

    #[test]
    fn outcome_maps_to_result_and_error() {
        assert_eq!(ctx().record(Outcome::Attempt).result, "attempt");
        assert_eq!(ctx().record(Outcome::Attempt).error, None);
        assert_eq!(ctx().record(Outcome::Ok).result, "ok");
        assert_eq!(ctx().record(Outcome::Ok).error, None);
        let err = ctx().record(Outcome::Error("boom".into()));
        assert_eq!(err.result, "error");
        assert_eq!(err.error.as_deref(), Some("boom"));
    }

    #[test]
    fn encodes_plain_fields() {
        let bytes = encode_journal_fields(&rec("ok", None));
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("SYSLOG_IDENTIFIER=fez\n"));
        assert!(text.contains("FEZ_UNIT=chronyd.service\n"));
        assert!(text.contains("FEZ_OPERATION=stop\n"));
        assert!(text.contains("FEZ_RESULT=ok\n"));
        assert!(text.contains("PRIORITY=6\n"));
        // No FEZ_ERROR field when error is None.
        assert!(!text.contains("FEZ_ERROR"));
    }

    #[test]
    fn encodes_newline_value_in_binary_form() {
        let bytes = encode_journal_fields(&rec("error", Some("line one\nline two".into())));
        // Find the FEZ_ERROR key written in binary form: `FEZ_ERROR\n` + u64 LE len.
        let needle = b"FEZ_ERROR\n";
        let pos = bytes
            .windows(needle.len())
            .position(|w| w == needle)
            .expect("FEZ_ERROR key present in binary form");
        let len_start = pos + needle.len();
        let len_bytes: [u8; 8] = bytes[len_start..len_start + 8].try_into().unwrap();
        assert_eq!(
            u64::from_le_bytes(len_bytes) as usize,
            "line one\nline two".len()
        );
    }

    #[test]
    fn file_sink_appends_json_lines() {
        let path =
            std::env::temp_dir().join(format!("fez-audit-test-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let sink = FileSink { path: path.clone() };
        sink.write(&rec("attempt", None));
        sink.write(&rec("ok", None));
        let body = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"result\":\"attempt\""));
        assert!(lines[1].contains("\"result\":\"ok\""));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn actor_uses_numeric_uids_not_environment_names() {
        assert_eq!(actor_from_uids(1000, 1000), "uid:1000");
        assert_eq!(actor_from_uids(1000, 0), "uid:1000 euid:0");
    }

    #[test]
    fn release_audit_sink_ignores_env_overrides() {
        assert_eq!(select_sink(Some("off"), false), SinkSelection::Journal);
        assert_eq!(
            select_sink(Some("file:/tmp/fez-audit.jsonl"), false),
            SinkSelection::Journal
        );
    }

    #[test]
    fn file_sink_refuses_symlink() {
        let target = audit_temp_path("symlink-target");
        let link = audit_temp_path("symlink-link");
        let _ = std::fs::remove_file(&target);
        let _ = std::fs::remove_file(&link);
        std::fs::write(&target, "").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        FileSink { path: link.clone() }.write(&rec("ok", None));

        assert_eq!(std::fs::read_to_string(&target).unwrap(), "");
        let _ = std::fs::remove_file(&link);
        let _ = std::fs::remove_file(&target);
    }

    #[test]
    fn file_sink_tightens_existing_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let path = audit_temp_path("perms");
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, "").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();

        FileSink { path: path.clone() }.write(&rec("ok", None));

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn priority_varies_by_result() {
        assert_eq!(rec("error", Some("x".into())).priority(), "3");
        assert_eq!(rec("attempt", None).priority(), "5");
        assert_eq!(rec("ok", None).priority(), "6");
    }

    fn audit_temp_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "fez-run-audited-{tag}-{}-{:?}.jsonl",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    #[test]
    fn run_audited_writes_attempt_then_ok_on_success() {
        let path = audit_temp_path("ok");
        let _ = std::fs::remove_file(&path);
        let sink = FileSink { path: path.clone() };
        let out: crate::error::Result<i32> =
            run_audited_with(&sink, "localhost", "stop", "chronyd.service", || Ok(42));
        assert_eq!(out.unwrap(), 42);
        let body = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"result\":\"attempt\""));
        assert!(lines[1].contains("\"result\":\"ok\""));
        assert!(lines[0].contains("\"operation\":\"stop\""));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn run_audited_writes_attempt_then_error_on_failure() {
        let path = audit_temp_path("err");
        let _ = std::fs::remove_file(&path);
        let sink = FileSink { path: path.clone() };
        let out: crate::error::Result<i32> =
            run_audited_with(&sink, "localhost", "start", "sshd.service", || {
                Err(crate::error::FezError::Aborted)
            });
        assert!(out.is_err());
        let body = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"result\":\"attempt\""));
        assert!(lines[1].contains("\"result\":\"error\""));
        assert!(lines[1].contains("aborted by user"));
        let _ = std::fs::remove_file(&path);
    }
}
