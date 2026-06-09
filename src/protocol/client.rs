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

    fn await_init(&mut self) -> Result<()> {
        loop {
            let frame = self.recv()?;
            if frame.channel.is_empty() {
                let c: IncomingControl =
                    serde_json::from_slice(&frame.payload).map_err(FezError::Decode)?;
                if c.command == "init" {
                    return Ok(());
                }
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
                    return Err(FezError::Problem(
                        c.problem.unwrap_or_else(|| "channel-closed".into()),
                    ));
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
                    if c.command == "close" {
                        if let Some(p) = c.problem {
                            return Err(FezError::Problem(p));
                        }
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
                    if c.command == "close" {
                        if let Some(p) = c.problem {
                            return Err(FezError::Problem(p));
                        }
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
