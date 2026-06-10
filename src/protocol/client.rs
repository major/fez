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
/// Two distinct causes produce the bridge's `access-denied`: (1) the standalone
/// bridge has no superuser bridge configured at all (only `cockpit-bridge` is
/// installed, so the `sudo`/`pkexec` bridge definitions from `cockpit-system`'s
/// shell manifest are absent), or (2) escalation is configured but sudo wants a
/// password fez does not supply. Cover both; the package gap is the common one
/// on minimal hosts.
const SUDO_REMEDIATION: &str = "the target bridge cannot escalate to root: install the cockpit-system package (it ships the sudo/pkexec superuser bridge definitions) and ensure this user has passwordless sudo (NOPASSWD); fez does not supply sudo passwords";

/// A live connection to a spawned bridge process, multiplexing D-Bus and
/// stream channels over its stdio.
pub struct BridgeClient {
    child: Child,
    stdin: std::process::ChildStdin,
    rx: Receiver<Frame>,
    host: String,
    next_channel: u64,
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
        };
        client.send_control(&Control::Init {
            version: 1,
            host: "localhost".into(),
            // Start the sudo superuser peer up front so later
            // `superuser: "require"` channels (mutations) can route to root.
            superuser: Some(json!({"id": "sudo"})),
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
    /// Waits for the bridge's `init`, then drives the superuser negotiation to
    /// completion. Because we sent `init` with `superuser: {id: "sudo"}`, the
    /// bridge tries to start a root peer immediately and always finishes with a
    /// `superuser-init-done` control message (success or failure). If the sudo
    /// peer needs a password, the bridge first sends an `authorize` challenge;
    /// fez does not hold sudo credentials, so we refuse and fail fast with
    /// `AccessDenied` rather than hanging on a prompt that can never be answered.
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
                // The bridge is asking us to answer a credential prompt (sudo
                // password). fez intentionally does not handle passwords yet, so
                // refuse instead of hanging on an unanswerable challenge.
                "authorize" => {
                    return Err(FezError::AccessDenied {
                        remediation: SUDO_REMEDIATION.into(),
                    });
                }
                // Superuser negotiation finished. The root peer may or may not
                // have started; if it did not, privileged channels will close
                // with `access-denied` later (handled in `open_dbus`). Either
                // way the handshake is done and we can proceed.
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
        // Properties.Get returns a single variant out-arg. cockpit's dbus-json3
        // represents a variant as the bare value here (the `as` array), so the
        // out-arg is the string array directly.
        let names = out
            .as_array()
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
            remediation: SUDO_REMEDIATION.into(),
        },
        Some(p) => FezError::Problem(p),
        None => FezError::Problem("channel-closed".into()),
    }
}
