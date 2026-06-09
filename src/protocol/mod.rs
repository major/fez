//! Wire protocol: framing, message types, and the bridge client.

/// Bridge client that drives D-Bus and stream channels.
pub mod client;
/// Length-prefixed frame encoding and decoding.
pub mod frame;
/// Control and D-Bus message types.
pub mod message;
