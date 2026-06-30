#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // read_frame parses length-prefixed cockpit-bridge wire frames.
    // Malformed input must not panic or OOM.
    let mut cursor = std::io::Cursor::new(data);
    let _ = fez::protocol::frame::read_frame(&mut cursor);
});
