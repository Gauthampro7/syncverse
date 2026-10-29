//! Long-distance high-RTT network synchronization engine for Syncplay-Alt.

#![deny(warnings)]
#![deny(missing_docs)]

pub mod barrier;
pub mod client;
pub mod crypto;
pub mod error;
pub mod fingerprint;
pub mod host;
pub mod ntp;
pub mod pll;
pub mod timeline;
pub mod transport;
pub mod upnp;
pub mod wire;

pub use barrier::{compute_target_unpause_server, BarrierPhase, BarrierStateMachine};
pub use crypto::RoomCryptoSession;
pub use error::SyncError;
pub use fingerprint::{MediaFingerprint, MediaMatchStatus};
pub use ntp::{ProbeSample, Rfc5905Filter};
pub use pll::{PllCorrection, PllSpeedController, PllZone};
pub use timeline::RoomState;
pub use transport::{ClientNode, ClientSession, HostServer, UdpSyncTransport};
pub use upnp::{UpnpMappingGuard, UpnpStatus};
pub use wire::{
    ChatMessagePayload, PacketPayload, PacketType, PeerStatusPayload, ReactionPayload, WirePacket,
};
