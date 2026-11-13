//! Watch-party networking runtime state, peer status helpers, and chat/control dispatch.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use syncverse_chat::{ChatRecord, RoomAnnouncement};
use syncverse_clock::MasterClock;
use syncverse_sync::{
    ClientNode, HostServer, PeerControlAction, PeerStatusPayload, RoomCryptoSession,
    UpnpMappingGuard, UpnpStatus,
};
use syncverse_ui::UiEvent;

/// Active watch-party role and networking engine.
pub(crate) enum WatchPartyRole {
    /// Standalone local playback with no watch-party networking active.
    None,
    /// Authoritative watch-party host room server.
    Host {
        /// UDP watch-party host server.
        server: HostServer,
        /// Room name identifier.
        _room: String,
        /// Local host display username.
        local_username: String,
        /// Cached client display names keyed by socket address.
        client_names: HashMap<SocketAddr, String>,
        /// Last timestamp when `RoomStateBroadcast` was emitted.
        last_state_broadcast: Instant,
        /// Last timestamp when UI `RoomMembersUpdated` was emitted.
        last_member_update: Instant,
        /// Peer count on last `RoomMembersUpdated` emission (triggers immediate UI update on join/leave).
        last_peer_count: usize,
        /// Last timestamp when NTP RTT probes were sent to peers.
        last_probe_sent: Instant,
        /// Receiver for asynchronous UPnP port-mapping completion.
        upnp_rx: Option<Receiver<(Option<UpnpMappingGuard>, UpnpStatus)>>,
        /// Active UPnP lease guard released cleanly when leaving/shutting down.
        _upnp_guard: Option<UpnpMappingGuard>,
        /// Listening UDP port for UPnP notifications.
        upnp_port: u16,
        /// Optional room encryption session shared across peers.
        _crypto: Option<Arc<RoomCryptoSession>>,
        /// Pending scheduled server unpause target timestamp during two-phase barrier.
        pending_commit_unpause_ms: Option<u64>,
        /// Whether phase-3 commit confirmation has been broadcast for the pending barrier.
        barrier_committed: bool,
    },
    /// Watch-party client synchronized against a remote host server.
    Client {
        /// UDP client node managing NTP filtering and PLL steering.
        client: ClientNode,
        /// Room name identifier.
        _room: String,
        /// Local client display username.
        local_username: String,
        /// Host display username fallback before PeerStatus arrives.
        host_username: String,
        /// Last timestamp when an NTP probe was sent.
        last_probe_sent: Instant,
        /// Last timestamp when UI `RoomMembersUpdated` was emitted.
        last_member_update: Instant,
        /// Peer status count on last `RoomMembersUpdated` emission (triggers immediate UI update on join/welcome).
        last_peer_count: usize,
        /// Optional room encryption session shared across peers.
        _crypto: Option<Arc<RoomCryptoSession>>,
        /// Whether initial sync snap/lock against the room timeline has completed.
        initial_sync_locked: bool,
        /// Timestamp of the last 5Hz PLL controller evaluation.
        last_pll_update: Instant,
    },
}

/// Extracts the local sender name for chat announcements and system records.
pub(crate) fn get_sender_name(party_role: &WatchPartyRole) -> String {
    match party_role {
        WatchPartyRole::Host { local_username, .. } => local_username.clone(),
        WatchPartyRole::Client { local_username, .. } => local_username.clone(),
        WatchPartyRole::None => std::env::var("USER").unwrap_or_else(|_| "You".to_string()),
    }
}

/// Broadcasts a standardized chat announcement locally and across connected room peers.
pub(crate) fn broadcast_chat_announcement(
    text: &str,
    now: Instant,
    clock: &MasterClock,
    party_role: &mut WatchPartyRole,
    event_tx: &Sender<UiEvent>,
) {
    let wall_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let media_ms = clock.current_pts(now).max(0.0) as u64;
    let rec = RoomAnnouncement::create_record(wall_ms, media_ms, text);
    let wire_bytes = rec.to_network_bytes();

    match party_role {
        WatchPartyRole::Host { server, .. } => {
            let _ = server.broadcast_chat_message(&wire_bytes);
        }
        WatchPartyRole::Client { client, .. } => {
            let _ = client.send_chat_message_with_pts(&wire_bytes, media_ms);
        }
        WatchPartyRole::None => {}
    }

    let _ = event_tx.send(UiEvent::NewChatRecord(rec));
}

/// Broadcasts local peer media status to room peers and caches host status for new joiners.
pub(crate) fn broadcast_local_peer_status(
    has_media: bool,
    is_ready: bool,
    duration_ms: u64,
    file_size_bytes: u64,
    filename: &str,
    media_hash: [u8; 32],
    party_role: &mut WatchPartyRole,
) {
    match party_role {
        WatchPartyRole::Host {
            server,
            local_username,
            ..
        } => {
            let status = PeerStatusPayload::with_hash(
                0,
                has_media,
                is_ready,
                duration_ms,
                file_size_bytes,
                local_username.clone(),
                filename.to_string(),
                media_hash,
            );
            server.set_host_peer_status(status.clone());
            let _ = server.broadcast_peer_status(&status);
        }
        WatchPartyRole::Client {
            client,
            local_username,
            ..
        } => {
            let _ = client.send_peer_status_with_hash(
                has_media,
                is_ready,
                duration_ms,
                file_size_bytes,
                local_username,
                filename,
                media_hash,
            );
        }
        WatchPartyRole::None => {}
    }
}

/// Sends a bidirectional [`PeerControlAction`] from a watch-party client to the host.
#[inline]
pub(crate) fn send_client_peer_control(
    party_role: &mut WatchPartyRole,
    action: PeerControlAction,
    media_pts_ms: u64,
    rate: f64,
) {
    if let WatchPartyRole::Client { client, .. } = party_role {
        let _ = client.send_peer_control(action, media_pts_ms, rate);
    }
}

/// Polls non-blocking UPnP completion on the host without stalling playback or networking.
pub(crate) fn poll_host_upnp_status(
    party_role: &mut WatchPartyRole,
    clock: &MasterClock,
    event_tx: &Sender<UiEvent>,
) -> bool {
    let WatchPartyRole::Host {
        upnp_rx,
        _upnp_guard,
        upnp_port,
        ..
    } = party_role
    else {
        return false;
    };

    let Some(rx) = upnp_rx.as_ref() else {
        return false;
    };

    match rx.try_recv() {
        Ok((guard, status)) => {
            *_upnp_guard = guard;
            let port = *upnp_port;
            *upnp_rx = None;

            let wall_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let current_pts = clock.current_pts(Instant::now()).max(0.0) as u64;

            match status {
                UpnpStatus::Active { external_ip, port } => {
                    let upnp_msg = format!(
                        "🌐 UPnP Port Forwarded: {}:{}",
                        external_ip.as_deref().unwrap_or("WAN"),
                        port
                    );
                    let upnp_rec = ChatRecord::new_text(
                        wall_ms,
                        current_pts,
                        [0u8; 16],
                        "System".to_string(),
                        &upnp_msg,
                    );
                    let _ = event_tx.send(UiEvent::NewChatRecord(upnp_rec));
                    let _ = event_tx.send(UiEvent::UpnpStatusUpdated {
                        is_mapped: true,
                        external_ip,
                        port,
                        message: "Port mapped successfully".to_string(),
                    });
                }
                UpnpStatus::Disabled { reason } => {
                    let _ = event_tx.send(UiEvent::UpnpStatusUpdated {
                        is_mapped: false,
                        external_ip: None,
                        port,
                        message: reason,
                    });
                }
            }
            true
        }
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
            let port = *upnp_port;
            *upnp_rx = None;
            let _ = event_tx.send(UiEvent::UpnpStatusUpdated {
                is_mapped: false,
                external_ip: None,
                port,
                message: "UPnP worker disconnected".to_string(),
            });
            true
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => false,
    }
}
