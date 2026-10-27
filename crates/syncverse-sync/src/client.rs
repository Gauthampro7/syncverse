//! Watch-party client node orchestration.
//!
//! Provides the `ClientNode` type for synchronizing with a remote host server,
//! evaluating the RFC 5905 filter, steering playback with the PI PLL, and sending
//! and receiving in-band chat messages.

pub use crate::transport::ClientNode;
