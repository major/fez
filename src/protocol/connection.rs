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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::Transport;
    use std::process::{Command, Stdio};

    struct CommandTransport {
        program: &'static str,
        args: &'static [&'static str],
    }

    impl Transport for CommandTransport {
        fn command(&self) -> Command {
            let mut command = Command::new(self.program);
            command.args(self.args);
            command
        }

        fn host_label(&self) -> String {
            "test-host".to_string()
        }
    }

    #[test]
    fn spawn_failure_preserves_spawn_error() {
        let transport = CommandTransport {
            program: "/definitely/not/a/cockpit-bridge",
            args: &[],
        };

        let result = BridgeConnection::spawn(&transport);

        assert!(matches!(result, Err(FezError::Spawn { .. })));
        if let Err(FezError::Spawn { program, .. }) = result {
            assert_eq!(program, "/definitely/not/a/cockpit-bridge");
        }
    }

    #[test]
    fn command_transport_reports_test_host() {
        let transport = CommandTransport {
            program: "/bin/true",
            args: &[],
        };

        assert_eq!(transport.host_label(), "test-host");
    }

    #[test]
    fn recv_reports_bridge_closed_when_stdout_ends() {
        let transport = CommandTransport {
            program: "/bin/sh",
            args: &["-c", "exit 0"],
        };
        let connection = BridgeConnection::spawn(&transport).unwrap();

        let error = connection.recv().unwrap_err();

        assert!(matches!(error, FezError::BridgeClosed));
    }

    #[test]
    fn drop_kills_child_process() {
        let transport = CommandTransport {
            program: "/bin/sleep",
            args: &["60"],
        };
        let connection = BridgeConnection::spawn(&transport).unwrap();
        let pid = connection.child.id();

        drop(connection);

        let status = Command::new("/bin/kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(!status.success(), "child process {pid} should be gone");
    }
}
