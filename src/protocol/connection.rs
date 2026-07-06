//! Internal bridge process lifecycle and framed I/O.

use crate::error::{FezError, Result};
use crate::protocol::frame::{read_frame, write_frame, Frame};
use crate::transport::Transport;
use std::io;
use std::process::{Child, ChildStdin, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Spawned bridge process plus framed stdin/stdout plumbing.
pub(super) struct BridgeConnection {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Frame>,
}

impl BridgeConnection {
    /// Spawn the bridge through `transport` and start detached pipe-drain threads.
    pub(super) fn spawn(transport: &dyn Transport) -> Result<Self> {
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

        Ok(Self { child, stdin, rx })
    }

    /// Send one frame to the bridge.
    pub(super) fn send_frame(&mut self, frame: &Frame) -> Result<()> {
        write_frame(&mut self.stdin, frame).map_err(FezError::Io)
    }

    /// Receive one frame from the bridge, using the standard bridge timeout.
    pub(super) fn recv(&self) -> Result<Frame> {
        match self.rx.recv_timeout(DEFAULT_TIMEOUT) {
            Ok(f) => Ok(f),
            Err(RecvTimeoutError::Timeout) => Err(FezError::Timeout),
            Err(RecvTimeoutError::Disconnected) => Err(FezError::BridgeClosed),
        }
    }
}

impl Drop for BridgeConnection {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
