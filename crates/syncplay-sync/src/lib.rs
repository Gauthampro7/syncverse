//! Long-distance high-RTT network synchronization engine for Syncplay-Alt.

#![deny(warnings)]
#![deny(missing_docs)]

pub mod barrier;
pub mod client;
pub mod error;
pub mod host;
pub mod ntp;
pub mod pll;
pub mod timeline;
pub mod transport;
pub mod wire;

pub use barrier::{compute_target_unpause_server, BarrierPhase, BarrierStateMachine};
pub use error::SyncError;
pub use ntp::{ProbeSample, Rfc5905Filter};
pub use pll::{PllCorrection, PllSpeedController, PllZone};
pub use timeline::RoomState;
pub use transport::{ClientNode, ClientSession, HostServer, UdpSyncTransport};
pub use wire::{ChatMessagePayload, PacketPayload, PacketType, WirePacket};
