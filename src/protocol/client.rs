use crate::error::{FezError, Result};
use crate::protocol::frame::{read_frame, write_frame, Frame};
use crate::protocol::message::{Control, DbusCall, DbusResponse, DbusSignal, IncomingControl};
use crate::transport::Transport;
use serde_json::{json, Value};
use std::io;
use std::process::{Child, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Object path of the bridge's superuser controller on the internal bus.
///
/// cockpit-bridge exports `SuperuserRoutingRule` at `/superuser` on its
/// in-process internal bus (`bridge.py`:
/// `self.internal_bus.export('/superuser', self.superuser_rule)`), with
/// interface [`SUPERUSER_IFACE`].
const SUPERUSER_PATH: &str = "/superuser";

/// D-Bus interface of the bridge's superuser controller.
///
/// Defined in cockpit's `superuser.py`
/// (`SuperuserRoutingRule(..., interface='cockpit.Superuser')`). It exposes the
/// `Bridges` property (`as`, the ordered list of viable escalation mechanisms)
/// and the `Start(s)` method (start a named mechanism).
const SUPERUSER_IFACE: &str = "cockpit.Superuser";

/// Guidance returned when privilege escalation to root fails.
///
/// fez tries every escalation mechanism the bridge advertises (sudo, polkit)
/// and only reports this after all of them fail. The common causes: the
/// standalone bridge ships no superuser bridge definitions (install
/// `cockpit-system`), sudo wants a password fez does not supply (configure
/// passwordless sudo), or no polkit rule grants this user the privileged
/// action. The message names both mechanisms so the operator knows either path
/// is viable.
const ESCALATION_REMEDIATION: &str = "fez could not escalate to root: no superuser mechanism succeeded. Install the cockpit-system package (it ships the sudo/pkexec superuser bridge definitions), then either configure passwordless sudo (NOPASSWD) for this user or grant a polkit rule allowing this user the privileged cockpit action, and retry. fez does not supply sudo passwords";
const SAFE_ESCALATION_MECHANISMS: &[&str] = &["sudo", "pkexec", "polkit"];

/// A single PCP metric to request from the bridge's `metrics1` channel.
pub struct MetricRequest<'a> {
    /// PCP metric name (e.g. `"kernel.all.load"`).
    pub name: &'a str,
    /// Optional derivation mode (e.g. `"rate"` for counter-to-rate conversion).
    pub derive: Option<&'a str>,
}

/// Raw result from a `metrics1` channel: the meta descriptor plus collected
/// data samples.
pub struct MetricsSnapshot {
    /// The meta message: metric names, units, semantics, instance lists.
    pub meta: Value,
    /// Collected data arrays (one per sample interval).
    pub samples: Vec<Value>,
}

/// A live connection to a spawned bridge process, multiplexing D-Bus and
/// stream channels over its stdio.
pub struct BridgeClient {
    child: Child,
    stdin: std::process::ChildStdin,
    rx: Receiver<Frame>,
    host: String,
    next_channel: u64,
    /// Whether a root peer has been brought up via `cockpit.Superuser.Start`.
    /// Escalation is performed lazily and at most once per connection.
    escalated: bool,
}

impl BridgeClient {
    /// Spawn the bridge via `transport`, perform the init handshake, and return
    /// a ready client.
    pub fn connect(transport: &dyn Transport) -> Result<BridgeClient> {
        let mut cmd = transport.command();
        let program = cmd.get_program().to_string_lossy().into_owned();
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|source| FezError::Spawn { program, source })?;
        let stdin = child.stdin.take().expect("piped stdin");
        let mut stdout = child.stdout.take().expect("piped stdout");
        let mut stderr = child.stderr.take().expect("piped stderr");

        // Always consume stderr so a noisy bridge or SSH transport cannot block
        // on a full pipe while the client waits for stdout frames.
        let _stderr_drain = thread::spawn(move || {
            let _ = io::copy(&mut stderr, &mut io::sink());
        });

        let (tx, rx) = mpsc::channel::<Frame>();
        thread::spawn(move || {
            while let Ok(Some(frame)) = read_frame(&mut stdout) {
                if tx.send(frame).is_err() {
                    break;
                }
            }
        });

        let mut client = BridgeClient {
            child,
            stdin,
            rx,
            host: transport.host_label(),
            next_channel: 1,
            escalated: false,
        };
        client.send_control(&Control::Init {
            version: 1,
            host: "localhost".into(),
            // Defer escalation: bring up no root peer at init. fez selects a
            // working mechanism later via `escalate()` (cockpit.Superuser.Start)
            // so it can fall through sudo -> polkit instead of pinning sudo.
            superuser: Some(json!("none")),
        })?;
        client.await_init()?;
        Ok(client)
    }

    fn send_control(&mut self, c: &Control) -> Result<()> {
        write_frame(&mut self.stdin, &Frame::control(&c.to_json())).map_err(FezError::Io)
    }

    fn recv(&self) -> Result<Frame> {
        match self.rx.recv_timeout(DEFAULT_TIMEOUT) {
            Ok(f) => Ok(f),
            Err(RecvTimeoutError::Timeout) => Err(FezError::Timeout),
            Err(RecvTimeoutError::Disconnected) => Err(FezError::BridgeClosed),
        }
    }

    /// Complete the bridge handshake.
    ///
    /// Waits for the bridge's `init` reply, which completes the handshake.
    /// Because we send `init` with `superuser: "none"`, the bridge brings up no
    /// root peer at init time and runs no superuser negotiation, so it emits no
    /// `superuser-init-done` (cockpit's `SuperuserRoutingRule.init` is only
    /// invoked, and only then fires `superuser-init-done`, when init carries a
    /// `superuser` object). Waiting for that message here would hang against a
    /// real bridge. Escalation is deferred to [`escalate`], run lazily before
    /// the first privileged channel open.
    ///
    /// [`escalate`]: BridgeClient::escalate
    fn await_init(&mut self) -> Result<()> {
        loop {
            let frame = self.recv()?;
            if !frame.channel.is_empty() {
                continue;
            }
            let c: IncomingControl =
                serde_json::from_slice(&frame.payload).map_err(FezError::Decode)?;
            // The bridge's `init` reply opens the transport. We deferred
            // escalation (`superuser: "none"`), so there is no further superuser
            // negotiation to await; the handshake is done.
            if c.command == "init" {
                return Ok(());
            }
        }
    }

    fn alloc_channel(&mut self) -> String {
        let c = format!("c{}", self.next_channel);
        self.next_channel += 1;
        c
    }

    /// Open an unprivileged D-Bus channel to `name` and return its channel id.
    pub fn dbus_open(&mut self, name: &str) -> Result<String> {
        self.open_dbus(name, false)
    }

    /// Open a privileged D-Bus channel (`superuser: "require"`); the bridge
    /// performs the sudo/polkit escalation and spawns a root peer (Section 5).
    pub fn dbus_open_privileged(&mut self, name: &str) -> Result<String> {
        self.open_dbus(name, true)
    }

    fn open_dbus(&mut self, name: &str, privileged: bool) -> Result<String> {
        // A privileged channel routes to a root peer, which only exists once we
        // have escalated. Drive escalation lazily before the first such open;
        // reads (privileged == false) never escalate.
        if privileged && !self.escalated {
            self.escalate()?;
        }
        let channel = self.alloc_channel();
        let mut open = Control::open(&channel, "dbus-json3")
            .opt("bus", json!("system"))
            .opt("name", json!(name));
        if privileged {
            open = open.opt("superuser", json!("require"));
        }
        self.send_control(&open)?;
        Ok(channel)
    }

    /// Bring up a root peer by selecting a working escalation mechanism.
    ///
    /// With init sent as `superuser: "none"`, no root peer exists until fez
    /// asks for one. This reads the bridge's advertised mechanisms
    /// ([`BridgeClient::superuser_bridges`]) and tries each via
    /// [`BridgeClient::superuser_start`] in order until one succeeds, so a host
    /// with password-only sudo but a working polkit rule still escalates. The
    /// `FEZ_ESCALATION` environment variable overrides the default loop:
    /// `off` disables escalation, and any other non-empty value forces that
    /// single mechanism only when it is safe and bridge-advertised (no
    /// fall-through). Idempotent: a no-op once escalated.
    ///
    /// # Errors
    ///
    /// Returns [`FezError::AccessDenied`] (exit 11) when no mechanism succeeds,
    /// when the host advertises none, when `FEZ_ESCALATION=off`, or when
    /// `FEZ_ESCALATION` names an unsafe or non-advertised mechanism. Propagates
    /// any non-`Dbus` transport error encountered while talking to the bridge.
    pub fn escalate(&mut self) -> Result<()> {
        if self.escalated {
            return Ok(());
        }
        let denied = || FezError::AccessDenied {
            remediation: ESCALATION_REMEDIATION.into(),
        };
        let forced = std::env::var("FEZ_ESCALATION").ok();
        if forced.as_deref() == Some("off") {
            // Never escalate. Mutations fail; reads are unaffected because they
            // never call escalate().
            return Err(denied());
        }
        let names = self.superuser_bridges()?;
        if let Some(name) = forced.as_deref().filter(|name| !name.is_empty()) {
            // Force a single safe, advertised mechanism with no fall-through.
            if !forced_escalation_is_advertised(name, &names) {
                return Err(denied());
            }
            return match self.superuser_start(name) {
                Ok(()) => {
                    self.escalated = true;
                    Ok(())
                }
                Err(FezError::Dbus { .. }) => Err(denied()),
                Err(e) => Err(e),
            };
        }
        for name in names {
            if !safe_escalation_mechanism(&name) {
                continue;
            }
            match self.superuser_start(&name) {
                Ok(()) => {
                    self.escalated = true;
                    return Ok(());
                }
                // This mechanism could not start (e.g. it needs an unanswerable
                // credential); try the next advertised one.
                Err(FezError::Dbus { .. }) => continue,
                Err(e) => return Err(e),
            }
        }
        Err(denied())
    }

    /// Open a D-Bus channel to the bridge's internal bus.
    ///
    /// The internal bus hosts the bridge's own controllers (notably
    /// `cockpit.Superuser`). It carries no `name` (the bridge is the peer) and
    /// is never privileged: the controller decides escalation, it is not itself
    /// reached through a root peer (cockpit `dbus.py`: `bus == 'internal'`).
    fn open_dbus_internal(&mut self) -> Result<String> {
        let channel = self.alloc_channel();
        let open = Control::open(&channel, "dbus-json3").opt("bus", json!("internal"));
        self.send_control(&open)?;
        Ok(channel)
    }

    /// List the escalation mechanisms the bridge considers viable on this host.
    ///
    /// Reads the `cockpit.Superuser` `Bridges` property (signature `as`) over
    /// the internal bus. The list is the bridge's own ordered, validity-filtered
    /// set of mechanism names (e.g. `["sudo", "pkexec"]`); an empty list means
    /// the host has no usable escalation mechanism.
    ///
    /// # Errors
    ///
    /// Returns [`FezError::Dbus`] if the property read fails, or any transport
    /// error from opening the internal channel or reading the reply.
    pub fn superuser_bridges(&mut self) -> Result<Vec<String>> {
        let channel = self.open_dbus_internal()?;
        let out = self.dbus_call(
            &channel,
            SUPERUSER_PATH,
            "org.freedesktop.DBus.Properties",
            "Get",
            json!([SUPERUSER_IFACE, "Bridges"]),
        )?;
        // `dbus_call` returns the out-argument array (`reply[0]`).
        // `Properties.Get` has a single `v` out-arg, so the `as` value arrives
        // variant-wrapped: `out = [{"t":"as","v":["sudo",...]}]`. Unwrap the
        // `{"t","v"}` envelope to reach the array (cockpit-bridge does not
        // unwrap it for us; treating `out[0]` as the array directly yields an
        // empty list and a spurious exit-11 deny).
        let names = out
            .as_array()
            .and_then(|args| args.first())
            .map(variant_value)
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        Ok(names)
    }

    /// Ask the bridge to start the named escalation mechanism.
    ///
    /// Calls `cockpit.Superuser.Start(name)` over the internal bus. On success
    /// the bridge has brought up a root peer, and subsequent
    /// `superuser: "require"` channels route to it. A mechanism that needs a
    /// credential fez cannot supply surfaces as a D-Bus error, not a hang.
    ///
    /// # Errors
    ///
    /// Returns [`FezError::Dbus`] when the bridge rejects the start (e.g. the
    /// mechanism needs an unanswerable credential), or any transport error.
    pub fn superuser_start(&mut self, name: &str) -> Result<()> {
        let channel = self.open_dbus_internal()?;
        self.dbus_call(
            &channel,
            SUPERUSER_PATH,
            SUPERUSER_IFACE,
            "Start",
            json!([name]),
        )?;
        Ok(())
    }

    /// Returns the out-argument array (`reply[0]`). Index `[0]` for the first return value.
    pub fn dbus_call(
        &mut self,
        channel: &str,
        path: &str,
        iface: &str,
        method: &str,
        args: Value,
    ) -> Result<Value> {
        let call = DbusCall::new(channel, path, iface, method, args);
        write_frame(&mut self.stdin, &Frame::new(channel, call.to_json())).map_err(FezError::Io)?;
        loop {
            let frame = self.recv()?;
            if frame.channel.is_empty() {
                let c: IncomingControl =
                    serde_json::from_slice(&frame.payload).map_err(FezError::Decode)?;
                if c.command == "close" && c.channel.as_deref() == Some(channel) {
                    return Err(close_problem_to_error(c.problem));
                }
                continue;
            }
            if frame.channel != channel {
                continue;
            }
            let resp: DbusResponse =
                serde_json::from_slice(&frame.payload).map_err(FezError::Decode)?;
            if resp.id.as_deref() != Some(&call.id) {
                continue; // signal/notify or stale; ignore
            }
            if let Some(name) = resp.dbus_error_name() {
                return Err(FezError::Dbus {
                    name: name.into(),
                    message: resp.dbus_error_message().unwrap_or_default(),
                });
            }
            return Ok(resp.out_args().cloned().unwrap_or(Value::Null));
        }
    }

    /// Send a D-Bus method call on `channel` and collect the signals it emits
    /// until a `Finished` signal (or a channel close) terminates the stream.
    ///
    /// PackageKit transactions report their result as a stream of signals on
    /// the transaction object path rather than as a method reply, so the
    /// request/reply [`BridgeClient::dbus_call`] cannot observe them. This sends
    /// the call, then accumulates every `signal` frame on `channel` whose path
    /// matches `path`, returning the raw `(member, args)` pairs in arrival
    /// order. The method-call reply itself (an empty reply) is ignored; only
    /// signals carry the payload. A `Finished` signal ends collection.
    ///
    /// # Errors
    ///
    /// Returns [`FezError::BridgeClosed`] / [`FezError::Timeout`] on transport
    /// failure, [`FezError::Decode`] on a malformed frame, or the mapped close
    /// problem if the channel closes with an error before `Finished`.
    pub fn dbus_call_collect(
        &mut self,
        channel: &str,
        path: &str,
        iface: &str,
        method: &str,
        args: Value,
    ) -> Result<Vec<(String, Vec<Value>)>> {
        let call = DbusCall::new(channel, path, iface, method, args);
        write_frame(&mut self.stdin, &Frame::new(channel, call.to_json())).map_err(FezError::Io)?;
        let mut collected: Vec<(String, Vec<Value>)> = Vec::new();
        loop {
            let frame = self.recv()?;
            if frame.channel.is_empty() {
                let c: IncomingControl =
                    serde_json::from_slice(&frame.payload).map_err(FezError::Decode)?;
                if c.command == "close" && c.channel.as_deref() == Some(channel) {
                    return Err(close_problem_to_error(c.problem));
                }
                continue;
            }
            if frame.channel != channel {
                continue;
            }
            // A signal frame? Decode and accumulate; stop on Finished.
            let sig: DbusSignal =
                serde_json::from_slice(&frame.payload).map_err(FezError::Decode)?;
            let Some(member) = sig.member() else {
                // Not a signal (e.g. the empty method reply); ignore.
                continue;
            };
            if sig.path() != Some(path) {
                continue; // signal from a different transaction object
            }
            let member = member.to_string();
            let args = sig.args().cloned().unwrap_or_default();
            let finished = member == "Finished";
            collected.push((member, args));
            if finished {
                return Ok(collected);
            }
        }
    }

    /// Open a `stream` channel running `argv` and buffer its output until `done`.
    pub fn stream_collect(&mut self, argv: &[&str]) -> Result<Vec<u8>> {
        let channel = self.alloc_channel();
        self.send_control(&Control::open(&channel, "stream").opt("spawn", json!(argv)))?;
        let mut buf = Vec::new();
        loop {
            let frame = self.recv()?;
            if frame.channel == channel {
                buf.extend_from_slice(&frame.payload);
            } else if frame.channel.is_empty() {
                let c: IncomingControl =
                    serde_json::from_slice(&frame.payload).map_err(FezError::Decode)?;
                if c.channel.as_deref() == Some(&channel) {
                    if c.command == "close" && c.problem.is_some() {
                        return Err(close_problem_to_error(c.problem));
                    }
                    if c.command == "done" || c.command == "close" {
                        return Ok(buf);
                    }
                }
            }
        }
    }

    /// Open a `stream` channel and invoke `on_chunk` for each data frame until `done`.
    pub fn stream_each<F: FnMut(&[u8])>(&mut self, argv: &[&str], mut on_chunk: F) -> Result<()> {
        let channel = self.alloc_channel();
        self.send_control(&Control::open(&channel, "stream").opt("spawn", json!(argv)))?;
        loop {
            let frame = self.recv()?;
            if frame.channel == channel {
                on_chunk(&frame.payload);
            } else if frame.channel.is_empty() {
                let c: IncomingControl =
                    serde_json::from_slice(&frame.payload).map_err(FezError::Decode)?;
                if c.channel.as_deref() == Some(&channel) {
                    if c.command == "close" && c.problem.is_some() {
                        return Err(close_problem_to_error(c.problem));
                    }
                    if c.command == "done" || c.command == "close" {
                        return Ok(());
                    }
                }
            }
        }
    }

    /// The host label associated with this connection.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Open a `metrics1` channel, collect `sample_count` data samples, and
    /// return the meta descriptor plus raw sample arrays.
    ///
    /// Uses `source: "direct"` (local PCP context, no pmcd daemon required).
    /// Rate-derived metrics need at least 2 samples for a meaningful delta;
    /// the first sample's rate values are `false`.
    ///
    /// # Errors
    ///
    /// Returns [`FezError::DependencyMissing`] when `python3-pcp` is not
    /// installed (bridge closes the channel with `not-supported`).
    /// Returns [`FezError::Timeout`] / [`FezError::BridgeClosed`] on
    /// transport failure, [`FezError::Decode`] on a malformed frame, or
    /// the mapped close problem for other channel errors.
    pub fn metrics_collect(
        &mut self,
        metrics: &[MetricRequest<'_>],
        interval_ms: u64,
        sample_count: u64,
    ) -> Result<MetricsSnapshot> {
        let channel = self.alloc_channel();
        let metric_specs: Vec<Value> = metrics
            .iter()
            .map(|m| {
                let mut spec = json!({"name": m.name});
                if let Some(d) = m.derive {
                    spec["derive"] = json!(d);
                }
                spec
            })
            .collect();

        self.send_control(
            &Control::open(&channel, "metrics1")
                .opt("source", json!("direct"))
                .opt("interval", json!(interval_ms))
                .opt("limit", json!(sample_count))
                .opt("metrics", json!(metric_specs)),
        )?;

        let mut meta = Value::Null;
        let mut samples: Vec<Value> = Vec::new();

        loop {
            let frame = self.recv()?;
            if frame.channel == channel {
                let parsed: Value =
                    serde_json::from_slice(&frame.payload).map_err(FezError::Decode)?;
                if parsed.is_object() {
                    meta = parsed;
                } else if let Value::Array(batch) = parsed {
                    let remaining = sample_count.saturating_sub(samples.len() as u64) as usize;
                    samples.extend(batch.into_iter().take(remaining));
                }
                // ponytail: real bridge ignores `limit` for direct source
                // and streams forever; close ourselves once we have enough
                if samples.len() as u64 >= sample_count {
                    let _ = self.send_control(&Control::Close {
                        channel: channel.clone(),
                        problem: None,
                    });
                    return Ok(MetricsSnapshot { meta, samples });
                }
            } else if frame.channel.is_empty() {
                let c: IncomingControl =
                    serde_json::from_slice(&frame.payload).map_err(FezError::Decode)?;
                if c.channel.as_deref() != Some(&channel) {
                    continue;
                }
                match c.command.as_str() {
                    "ready" => {}
                    "done" => return Ok(MetricsSnapshot { meta, samples }),
                    "close" => {
                        if c.problem.as_deref() == Some("not-supported") {
                            return Err(FezError::DependencyMissing {
                                component: "pcp".into(),
                                dbus_name: "metrics1".into(),
                                remediation: "install the pcp and python3-pcp packages on the \
                                              target host: sudo dnf install pcp python3-pcp"
                                    .into(),
                            });
                        }
                        if c.problem.is_some() {
                            return Err(close_problem_to_error(c.problem));
                        }
                        return Ok(MetricsSnapshot { meta, samples });
                    }
                    _ => {}
                }
            }
        }
    }
}

impl Drop for BridgeClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Unwrap a D-Bus variant envelope to its inner value.
///
/// cockpit-bridge represents a variant on the wire as `{"t":<sig>,"v":<value>}`
/// (e.g. a `Properties.Get` out-arg or an `a{sv}` dict value). Return the inner
/// `v` when present, otherwise the value unchanged, so callers can treat
/// variant-wrapped and bare values uniformly (same convention as the services
/// status parser).
/// Unwrap a cockpit-bridge D-Bus variant envelope.
///
/// Returns the inner `"v"` value from `{"t":"...", "v":...}`, or the
/// original value if the envelope is absent.
fn safe_escalation_mechanism(name: &str) -> bool {
    SAFE_ESCALATION_MECHANISMS.contains(&name)
}

fn forced_escalation_is_advertised(name: &str, advertised: &[String]) -> bool {
    safe_escalation_mechanism(name) && advertised.iter().any(|advertised| advertised == name)
}

pub(crate) fn variant_value(v: &Value) -> &Value {
    v.get("v").unwrap_or(v)
}

/// Convert a channel-close `problem` into the matching [`FezError`].
///
/// A privileged channel that the bridge could not escalate closes with
/// `problem: "access-denied"`; surface that as the dedicated [`FezError::AccessDenied`]
/// (exit 11, with remediation) instead of a generic channel problem (exit 4),
/// so privilege failures are distinguishable from missing resources. Known
/// problem codes use explicit [`FezError`] variants
/// ([`ChannelNotFound`](FezError::ChannelNotFound),
/// [`ChannelAuthFailed`](FezError::ChannelAuthFailed),
/// [`ChannelNotSupported`](FezError::ChannelNotSupported)); unrecognised codes
/// fall through to the catch-all [`FezError::Problem`] variant.
fn close_problem_to_error(problem: Option<String>) -> FezError {
    match problem {
        Some(p) if p == "access-denied" => FezError::AccessDenied {
            remediation: ESCALATION_REMEDIATION.into(),
        },
        Some(p) if p == "not-found" => FezError::ChannelNotFound(p),
        Some(p) if p == "authentication-failed" => FezError::ChannelAuthFailed(p),
        Some(p) if p == "not-supported" => FezError::ChannelNotSupported(p),
        Some(p) => FezError::Problem(p),
        None => FezError::Problem("channel-closed".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forced_escalation_must_be_advertised() {
        let advertised = vec!["sudo".to_string(), "polkit".to_string()];
        assert!(forced_escalation_is_advertised("sudo", &advertised));
        assert!(!forced_escalation_is_advertised("pkexec", &advertised));
        assert!(!forced_escalation_is_advertised("../../evil", &advertised));
    }
}
