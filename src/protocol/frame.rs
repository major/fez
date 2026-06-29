//! Length-prefixed wire framing (cockpit-style) used between fez and the bridge.
use std::io::{self, Read, Write};

/// One framed message: a channel id plus an opaque payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Channel id; empty for control frames.
    pub channel: String,
    /// Raw payload bytes.
    pub payload: Vec<u8>,
}

impl Frame {
    /// Build a frame for `channel` carrying `payload`.
    pub fn new(channel: impl Into<String>, payload: impl Into<Vec<u8>>) -> Self {
        Frame {
            channel: channel.into(),
            payload: payload.into(),
        }
    }
    /// A control-channel frame (empty channel id) carrying a JSON value.
    /// A trailing newline is appended to the JSON, matching cockpit's own client.
    pub fn control(json: &[u8]) -> Self {
        let mut payload = Vec::with_capacity(json.len() + 1);
        payload.extend_from_slice(json);
        payload.push(b'\n');
        Frame {
            channel: String::new(),
            payload,
        }
    }
}

/// Encode and write one frame to `w`, flushing afterward.
pub fn write_frame<W: Write>(w: &mut W, frame: &Frame) -> io::Result<()> {
    let mut message = Vec::with_capacity(frame.channel.len() + 1 + frame.payload.len());
    message.extend_from_slice(frame.channel.as_bytes());
    message.push(b'\n');
    message.extend_from_slice(&frame.payload);
    writeln!(w, "{}", message.len())?;
    w.write_all(&message)?;
    w.flush()
}

/// Read one frame. `Ok(None)` on a clean EOF at a frame boundary.
pub fn read_frame<R: Read>(r: &mut R) -> io::Result<Option<Frame>> {
    let mut len_buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        if r.read(&mut byte)? == 0 {
            if len_buf.is_empty() {
                return Ok(None);
            }
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "eof in frame length",
            ));
        }
        if byte[0] == b'\n' {
            break;
        }
        len_buf.push(byte[0]);
    }
    let len: usize = std::str::from_utf8(&len_buf)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad frame length"))?;
    const MAX_FRAME: usize = 16 * 1024 * 1024;
    if len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    let mut message = vec![0u8; len];
    r.read_exact(&mut message)?;
    let nl = message.iter().position(|&b| b == b'\n').ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "frame missing channel separator",
        )
    })?;
    let channel = String::from_utf8_lossy(&message[..nl]).into_owned();
    let payload = message[nl + 1..].to_vec();
    Ok(Some(Frame { channel, payload }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn encodes_protocol_doc_example() {
        let mut out = Vec::new();
        write_frame(&mut out, &Frame::new("a5", b"abc".to_vec())).unwrap();
        assert_eq!(out, b"6\na5\nabc");
    }

    #[test]
    fn control_frame_has_empty_channel() {
        let mut out = Vec::new();
        write_frame(&mut out, &Frame::new("", b"{}".to_vec())).unwrap();
        // message = "\n{}" (len 3)
        assert_eq!(out, b"3\n\n{}");
    }

    #[test]
    fn round_trips() {
        let frames = [
            Frame::new("", b"{\"command\":\"init\"}".to_vec()),
            Frame::new("ch1", b"payload bytes".to_vec()),
        ];
        let mut buf = Vec::new();
        for f in &frames {
            write_frame(&mut buf, f).unwrap();
        }
        let mut cur = Cursor::new(buf);
        for f in &frames {
            assert_eq!(read_frame(&mut cur).unwrap().unwrap(), *f);
        }
        assert_eq!(read_frame(&mut cur).unwrap(), None); // clean EOF
    }

    #[test]
    fn rejects_oversized_frame() {
        // A length prefix claiming 17 MB must be rejected before allocation.
        let header = format!("{}\n", 17 * 1024 * 1024);
        let mut cur = Cursor::new(header.into_bytes());
        let err = read_frame(&mut cur).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("too large"));
    }

    #[test]
    fn rejects_missing_channel_separator() {
        // len=3, message "abc" has no newline -> invalid
        let mut cur = Cursor::new(b"3\nabc".to_vec());
        assert!(read_frame(&mut cur).is_err());
    }
}
