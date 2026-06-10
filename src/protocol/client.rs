use crate::error::{FezError, Result};
use crate::protocol::frame::{read_frame, write_frame, Frame};
use crate::protocol::message::{Control, DbusCall, DbusResponse, IncomingControl};
use crate::transport::Transport;
use serde_json::{json, Value};
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
    /// Waits for the bridge's `init`, then for `superuser-init-done`. Because we
    /// send `init` with `superuser: "none"`, the bridge brings up no root peer
    /// and the superuser negotiation completes trivially: there is no init-time
    /// credential prompt to answer. Escalation is deferred to [`escalate`], run
    /// lazily before the first privileged channel open.
    ///
    /// [`escalate`]: BridgeClient::escalate
    fn await_init(&mut self) -> Result<()> {
        let mut saw_init = false;
        loop {
            let frame = self.recv()?;
            if !frame.channel.is_empty() {
                continue;
            }
            let c: IncomingControl =
                serde_json::from_slice(&frame.payload).map_err(FezError::Decode)?;
            match c.command.as_str() {
                "init" => {
                    saw_init = true;
                }
                // Superuser negotiation finished (trivially, since we deferred
                // escalation). The handshake is done; proceed.
                "superuser-init-done" if saw_init => {
                    return Ok(());
                }
                _ => {}
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
    /// `off` disables escalation, and any other value forces that single
    /// mechanism (no fall-through). Idempotent: a no-op once escalated.
    ///
    /// # Errors
    ///
    /// Returns [`FezError::AccessDenied`] (exit 11) when no mechanism succeeds,
    /// when the host advertises none, or when `FEZ_ESCALATION=off`. Propagates
    /// any non-`Dbus` transport error encountered while talking to the bridge.
    pub fn escalate(&mut self) -> Result<()> {
        if self.escalated {
            return Ok(());
        }
        let denied = || FezError::AccessDenied {
            remediation: ESCALATION_REMEDIATION.into(),
        };
        match std::env::var("FEZ_ESCALATION").ok().as_deref() {
            // Never escalate. Mutations fail; reads are unaffected because they
            // never call escalate().
            Some("off") => return Err(denied()),
            // Force a single named mechanism with no fall-through.
            Some(name) if !name.is_empty() => {
                return match self.superuser_start(name) {
                    Ok(()) => {
                        self.escalated = true;
                        Ok(())
                    }
                    Err(FezError::Dbus { .. }) => Err(denied()),
                    Err(e) => Err(e),
                };
            }
            // Empty or unset: default transparent loop below.
            _ => {}
        }
        let names = self.superuser_bridges()?;
        for name in names {
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
        // `dbus_call` returns the out-argument array (`reply[0]`). Properties.Get
        // has a single out-arg (the variant), so the `as` value is `out[0]`.
        let names = out
            .as_array()
            .and_then(|args| args.first())
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
}

impl Drop for BridgeClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Convert a channel-close `problem` into the matching [`FezError`].
///
/// A privileged channel that the bridge could not escalate closes with
/// `problem: "access-denied"`; surface that as the dedicated [`FezError::AccessDenied`]
/// (exit 11, with remediation) instead of a generic channel problem (exit 4),
/// so privilege failures are distinguishable from missing resources. Any other
/// problem string keeps the generic [`FezError::Problem`] mapping.
fn close_problem_to_error(problem: Option<String>) -> FezError {
    match problem {
        Some(p) if p == "access-denied" => FezError::AccessDenied {
            remediation: ESCALATION_REMEDIATION.into(),
        },
        Some(p) => FezError::Problem(p),
        None => FezError::Problem("channel-closed".into()),
    }
}
