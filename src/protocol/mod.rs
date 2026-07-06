//! Wire protocol: framing, message types, and the bridge client.

/// Bridge client that drives D-Bus and stream channels.
pub mod client;
/// Internal bridge process lifecycle and framed I/O.
mod connection;
/// Length-prefixed frame encoding and decoding.
pub mod frame;
/// Control and D-Bus message types.
pub mod message;
/// Transparent decoding of cockpit `dbus-json3` variant envelopes.
pub mod variant;
