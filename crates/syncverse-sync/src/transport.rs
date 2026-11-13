//! Real-time non-blocking UDP watch-party network transport.
//!
//! Provides the low-level `UdpSyncTransport` wrapper as well as the high-level
//! `HostServer` and `ClientNode` orchestration engines for synchronized watch parties.

use std::collections::HashMap;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::barrier::{compute_target_unpause_server, BarrierPhase, BarrierStateMachine};
use crate::crypto::RoomCryptoSession;
use crate::error::SyncError;
use crate::ntp::{ProbeSample, Rfc5905Filter};
use crate::pll::{PllCorrection, PllSpeedController, PllZone};
use crate::timeline::RoomState;
use crate::wire::{
    ChatMessagePayload, PacketPayload, PeerControlAction, PeerControlPayload, PeerStatusPayload,
    ReactionPayload, WirePacket, FLAG_BARRIER_COMMIT,
};

/// Non-blocking UDP transport wrapper for watch-party synchronization.
pub struct UdpSyncTransport {
    socket: UdpSocket,
    crypto: Option<Arc<RoomCryptoSession>>,
}

impl UdpSyncTransport {
    /// Binds a UDP socket to the specified address in non-blocking mode.
    pub fn bind<A: ToSocketAddrs>(addr: A) -> Result<Self, SyncError> {
        let socket = UdpSocket::bind(addr).map_err(|e| SyncError::Io(e.to_string()))?;
        socket
            .set_nonblocking(true)
            .map_err(|e| SyncError::Io(e.to_string()))?;
        Ok(Self {
            socket,
            crypto: None,
        })
    }

    /// Configures or clears the room encryption session for this transport.
    pub fn set_crypto(&mut self, crypto: Option<Arc<RoomCryptoSession>>) {
        self.crypto = crypto;
    }

    /// Builder-style helper to attach an optional room encryption session.
    pub fn with_crypto(mut self, crypto: Option<Arc<RoomCryptoSession>>) -> Self {
        self.crypto = crypto;
        self
    }

    /// Returns `true` if packet encryption is enabled on this transport.
    pub fn is_encrypted(&self) -> bool {
        self.crypto.is_some()
    }

    /// Returns the local socket address.
    pub fn local_addr(&self) -> Result<SocketAddr, SyncError> {
        self.socket
            .local_addr()
            .map_err(|e| SyncError::Io(e.to_string()))
    }

    /// Sends a wire packet to the specified remote target address.
    pub fn send_packet(&self, packet: &WirePacket, target: SocketAddr) -> Result<usize, SyncError> {
        let bytes = match &self.crypto {
            Some(crypto) => packet.to_encrypted_bytes(crypto)?,
            None => packet.to_bytes(),
        };
        self.socket
            .send_to(&bytes, target)
            .map_err(|e| SyncError::Io(e.to_string()))
    }

    /// Polls for an incoming wire packet without blocking.
    ///
    /// Returns `Ok(Some((packet, addr)))` if a valid packet arrived,
    /// `Ok(None)` if no datagrams are currently available (`WouldBlock`),
    /// or `Err(...)` if a network or deserialization error occurred.
    pub fn poll_packet(&self) -> Result<Option<(WirePacket, SocketAddr)>, SyncError> {
        let mut buf = [0u8; 2048];
        match self.socket.recv_from(&mut buf) {
            Ok((n, addr)) => {
                let packet =
                    WirePacket::from_encrypted_bytes(&buf[..n], self.crypto.as_deref())?;
                Ok(Some((packet, addr)))
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(SyncError::Io(e.to_string())),
        }
    }

    /// Returns an immutable reference to the underlying `UdpSocket`.
    pub fn socket(&self) -> &UdpSocket {
        &self.socket
    }
}

/// Tracked client connection state on the host server.
#[derive(Debug, Clone)]
pub struct ClientSession {
    /// Assigned client identifier.
    pub client_id: u32,
    /// Remote socket address.
    pub addr: SocketAddr,
    /// Measured round-trip time in milliseconds.
    pub rtt_ms: u64,
    /// Last received instant.
    pub last_seen: Instant,
    /// Whether client completed pre-roll decode and reported ready for current barrier session.
    pub barrier_preroll_ready: bool,
    /// Persistent readiness state of the peer ("Ready to Watch" checkbox).
    pub is_user_ready: bool,
    /// Whether peer currently has media loaded.
    pub has_media: bool,
    /// Loaded media file basename.
    pub filename: String,
    /// Media container duration in milliseconds.
    pub duration_ms: u64,
    /// Media file size on disk in bytes.
    pub file_size_bytes: u64,
    /// Peer username.
    pub username: String,
    /// SHA-256 fingerprint hash of the loaded media.
    pub media_hash: [u8; 32],
}

/// Host watch-party server managing UDP transport, NTP timekeeping, and barrier synchronization.
pub struct HostServer {
    transport: UdpSyncTransport,
    start_instant: Instant,
    clients: HashMap<SocketAddr, ClientSession>,
    newly_joined_peers: Vec<SocketAddr>,
    host_peer_status: Option<PeerStatusPayload>,
    pending_peer_controls: Vec<(SocketAddr, PeerControlPayload)>,
    room_sequence: u64,
    ref_media_pts_ms: u64,
    target_speed: f64,
    is_paused: bool,
    barrier: BarrierStateMachine,
    barrier_session_id: u32,
    barrier_prepare_started_at: Option<Instant>,
    scheduled_unpause_server_ms: Option<u64>,
    chat_messages: Vec<ChatMessagePayload>,
    next_chat_seq: u32,
    reactions: Vec<ReactionPayload>,
    pending_probes: HashMap<u32, (SocketAddr, u64)>,
    next_probe_id: u32,
}

impl HostServer {
    /// Binds a new `HostServer` to the specified local socket address.
    pub fn bind<A: ToSocketAddrs>(addr: A) -> Result<Self, SyncError> {
        let transport = UdpSyncTransport::bind(addr)?;
        Ok(Self {
            transport,
            start_instant: Instant::now(),
            clients: HashMap::new(),
            newly_joined_peers: Vec::new(),
            host_peer_status: None,
            pending_peer_controls: Vec::new(),
            room_sequence: 1,
            ref_media_pts_ms: 0,
            target_speed: 1.0,
            is_paused: true,
            barrier: BarrierStateMachine::default(),
            barrier_session_id: 0,
            barrier_prepare_started_at: None,
            scheduled_unpause_server_ms: None,
            chat_messages: Vec::new(),
            next_chat_seq: 1,
            reactions: Vec::new(),
            pending_probes: HashMap::new(),
            next_probe_id: 1,
        })
    }

    /// Configures or clears the room encryption session on the host transport.
    pub fn set_crypto(&mut self, crypto: Option<Arc<RoomCryptoSession>>) {
        self.transport.set_crypto(crypto);
    }

    /// Builder-style helper to attach an optional room encryption session.
    pub fn with_crypto(mut self, crypto: Option<Arc<RoomCryptoSession>>) -> Self {
        self.transport.set_crypto(crypto);
        self
    }

    /// Returns `true` if packet encryption is enabled on this host server.
    pub fn is_encrypted(&self) -> bool {
        self.transport.is_encrypted()
    }

    /// Returns the local bound socket address.
    pub fn local_addr(&self) -> Result<SocketAddr, SyncError> {
        self.transport.local_addr()
    }

    /// Returns the current monotonic server time in nanoseconds.
    #[inline]
    pub fn current_server_nanos(&self) -> u64 {
        self.start_instant.elapsed().as_nanos() as u64
    }

    /// Returns the current monotonic server wall time in milliseconds.
    #[inline]
    pub fn current_server_wall_ms(&self) -> u64 {
        self.start_instant.elapsed().as_millis() as u64
    }

    /// Registers or updates a client connection with known RTT.
    pub fn register_client(&mut self, addr: SocketAddr, client_id: u32, rtt_ms: u64) {
        self.clients.insert(
            addr,
            ClientSession {
                client_id,
                addr,
                rtt_ms,
                last_seen: Instant::now(),
                barrier_preroll_ready: false,
                is_user_ready: false,
                has_media: false,
                filename: String::new(),
                duration_ms: 0,
                file_size_bytes: 0,
                username: String::new(),
                media_hash: [0u8; 32],
            },
        );
    }

    /// Updates the measured RTT for a connected client address.
    pub fn update_client_rtt(&mut self, addr: SocketAddr, rtt_ms: u64) {
        if let Some(client) = self.clients.get_mut(&addr) {
            client.rtt_ms = rtt_ms;
            client.last_seen = Instant::now();
        }
    }

    /// Sends an NTP probe request to a specific client address to measure round-trip time.
    pub fn send_ntp_probe_to(&mut self, target: SocketAddr) -> Result<u32, SyncError> {
        let probe_id = self.next_probe_id;
        self.next_probe_id = self.next_probe_id.wrapping_add(1);
        let t1 = self.current_server_nanos();
        self.pending_probes.insert(probe_id, (target, t1));
        let packet = WirePacket::ntp_request(probe_id, t1);
        self.transport.send_packet(&packet, target)?;
        Ok(probe_id)
    }

    /// Returns the maximum RTT across all connected clients: $\max_i(\text{RTT}_i)$.
    pub fn max_rtt_ms(&self) -> u64 {
        self.clients.values().map(|c| c.rtt_ms).max().unwrap_or(0)
    }

    /// Returns an immutable reference to the map of tracked clients.
    pub fn clients(&self) -> &HashMap<SocketAddr, ClientSession> {
        &self.clients
    }

    /// Updates the media playback state and reference presentation position.
    pub fn set_media_state(&mut self, ref_media_pts_ms: u64, target_speed: f64, paused: bool) {
        self.ref_media_pts_ms = ref_media_pts_ms;
        self.target_speed = target_speed;
        self.is_paused = paused;
    }

    /// Returns whether media playback is currently paused on the host.
    pub fn is_paused(&self) -> bool {
        self.is_paused
    }

    /// Returns the reference media presentation position in milliseconds.
    pub fn ref_media_pts_ms(&self) -> u64 {
        self.ref_media_pts_ms
    }

    /// Polls for incoming UDP datagrams, automatically handling NTP probes, barrier acks, and seeks.
    pub fn poll_and_process(&mut self) -> Result<Option<(WirePacket, SocketAddr)>, SyncError> {
        let res = self.transport.poll_packet()?;
        if let Some((ref packet, addr)) = res {
            let now = Instant::now();
            let next_client_id = (self.clients.len() + 1) as u32;
            let is_new_client = !self.clients.contains_key(&addr);
            self.clients
                .entry(addr)
                .and_modify(|c| c.last_seen = now)
                .or_insert_with(|| ClientSession {
                    client_id: next_client_id,
                    addr,
                    rtt_ms: 0,
                    last_seen: now,
                    barrier_preroll_ready: false,
                    is_user_ready: false,
                    has_media: false,
                    filename: String::new(),
                    duration_ms: 0,
                    file_size_bytes: 0,
                    username: String::new(),
                    media_hash: [0u8; 32],
                });
            if is_new_client {
                self.newly_joined_peers.push(addr);
            }

            match &packet.payload {
                PacketPayload::NtpProbeRequest(req) => {
                    let t2 = self.current_server_nanos();
                    let t3 = self.current_server_nanos();
                    let resp = WirePacket::ntp_response(req.probe_id, req.t1_client_send_ns, t2, t3);
                    self.transport.send_packet(&resp, addr)?;
                }
                PacketPayload::NtpProbeResponse(resp) => {
                    if let Some((target, t1)) = self.pending_probes.remove(&resp.probe_id) {
                        if target == addr {
                            let t4 = self.current_server_nanos();
                            let server_delay = t4.saturating_sub(t1);
                            let client_proc = resp.t3_server_send_ns.saturating_sub(resp.t2_server_recv_ns);
                            let rtt_ns = server_delay.saturating_sub(client_proc);
                            let rtt_ms = rtt_ns / 1_000_000;
                            self.update_client_rtt(addr, rtt_ms);
                        }
                    }
                }
                PacketPayload::BarrierReady(ready) => {
                    if ready.session_id == self.barrier_session_id {
                        if let Some(client) = self.clients.get_mut(&addr) {
                            client.barrier_preroll_ready = true;
                        }
                    }
                }
                PacketPayload::SeekCommand(seek) => {
                    self.ref_media_pts_ms = seek.target_pts_ms;
                    let peer_id = self.clients.get(&addr).map(|c| c.client_id).unwrap_or(0);
                    let rate_q16 = (self.target_speed.clamp(0.1, 8.0) * 65536.0).round() as u32;
                    self.pending_peer_controls.push((
                        addr,
                        PeerControlPayload {
                            peer_id,
                            action: PeerControlAction::Seek,
                            media_pts_ms: seek.target_pts_ms,
                            rate_q16,
                        },
                    ));
                    self.broadcast_room_state()?;
                }
                PacketPayload::PeerControl(ctrl) => {
                    if ctrl.action == PeerControlAction::RequestSyncSnapshot {
                        self.newly_joined_peers.retain(|&a| a != addr);
                        self.send_welcome_to_peer(addr)?;
                    } else {
                        self.pending_peer_controls.push((addr, *ctrl));
                    }
                }
                PacketPayload::ChatMessage(chat) => {
                    self.chat_messages.push(chat.clone());
                    // Relay chat message to all other connected peers except sender
                    let targets: Vec<SocketAddr> = self
                        .clients
                        .keys()
                        .filter(|&&c_addr| c_addr != addr)
                        .copied()
                        .collect();
                    for target in targets {
                        let _ = self.transport.send_packet(packet, target);
                    }
                }
                PacketPayload::PeerStatus(status) => {
                    if let Some(client) = self.clients.get_mut(&addr) {
                        client.client_id = status.client_id;
                        client.has_media = status.has_media;
                        client.is_user_ready = status.is_ready;
                        client.duration_ms = status.duration_ms;
                        client.file_size_bytes = status.file_size_bytes;
                        client.username = status.username.clone();
                        client.filename = status.filename.clone();
                        client.media_hash = status.media_hash;
                    }
                    // Relay PeerStatus packet to all other connected peers except sender
                    let targets: Vec<SocketAddr> = self
                        .clients
                        .keys()
                        .filter(|&&c_addr| c_addr != addr)
                        .copied()
                        .collect();
                    for target in targets {
                        let _ = self.transport.send_packet(packet, target);
                    }
                }
                PacketPayload::Reaction(reaction) => {
                    self.reactions.push(reaction.clone());
                    // Relay reaction to all other connected peers except sender
                    let targets: Vec<SocketAddr> = self
                        .clients
                        .keys()
                        .filter(|&&c_addr| c_addr != addr)
                        .copied()
                        .collect();
                    for target in targets {
                        let _ = self.transport.send_packet(packet, target);
                    }
                }
                _ => {}
            }
        }
        Ok(res)
    }

    /// Drains and returns all pending received reactions from connected peers.
    pub fn poll_reactions(&mut self) -> Vec<ReactionPayload> {
        std::mem::take(&mut self.reactions)
    }

    /// Broadcasts a floating reaction from the host to all registered client peers.
    pub fn broadcast_reaction(
        &mut self,
        emoji: &str,
        media_pts_ms: u64,
        username: &str,
    ) -> Result<usize, SyncError> {
        let packet = WirePacket::reaction(0, media_pts_ms, emoji, username);
        let mut sent_count = 0;
        let targets: Vec<SocketAddr> = self.clients.keys().copied().collect();
        for addr in targets {
            if self.transport.send_packet(&packet, addr).is_ok() {
                sent_count += 1;
            }
        }
        Ok(sent_count)
    }

    /// Prunes stale client sessions that have not communicated within the specified timeout.
    ///
    /// Returns the client IDs of pruned peers.
    pub fn prune_stale_clients(&mut self, timeout: std::time::Duration) -> Vec<u32> {
        let mut pruned = Vec::new();
        let mut pruned_addrs = Vec::new();
        self.clients.retain(|addr, client| {
            if client.last_seen.elapsed() > timeout {
                pruned.push(client.client_id);
                pruned_addrs.push(*addr);
                false
            } else {
                true
            }
        });
        if !pruned_addrs.is_empty() {
            self.newly_joined_peers
                .retain(|a| !pruned_addrs.contains(a));
        }
        pruned
    }

    /// Drains and returns the socket addresses of peers newly discovered since the last drain.
    pub fn drain_newly_joined_peers(&mut self) -> Vec<SocketAddr> {
        std::mem::take(&mut self.newly_joined_peers)
    }

    /// Sends an arbitrary `WirePacket` directly to a single peer socket address.
    pub fn send_to_peer(&self, addr: SocketAddr, packet: &WirePacket) -> Result<(), SyncError> {
        self.transport.send_packet(packet, addr).map(|_| ())
    }

    /// Caches the host's own `PeerStatusPayload` so newly joining peers receive it immediately.
    pub fn set_host_peer_status(&mut self, status: PeerStatusPayload) {
        self.host_peer_status = Some(status);
    }

    /// Drains and returns all pending client playback control commands (`Pause`, `Play`, `Seek`, `SetSpeed`).
    pub fn poll_peer_controls(&mut self) -> Vec<(SocketAddr, PeerControlPayload)> {
        std::mem::take(&mut self.pending_peer_controls)
    }

    fn build_room_state_packet(&self) -> WirePacket {
        let wall_ms = self.current_server_wall_ms();
        let speed_q16 = (self.target_speed.clamp(0.1, 8.0) * 65536.0).round() as u32;
        WirePacket::room_state(
            self.room_sequence,
            self.ref_media_pts_ms,
            speed_q16,
            wall_ms,
            self.is_paused,
        )
    }

    fn send_welcome_to_peer(&self, peer_addr: SocketAddr) -> Result<usize, SyncError> {
        let mut sent_packets = 0;

        if let Some(ref status) = self.host_peer_status {
            let host_pkt = WirePacket::peer_status_with_hash(
                status.client_id,
                status.has_media,
                status.is_ready,
                status.duration_ms,
                status.file_size_bytes,
                &status.username,
                &status.filename,
                status.media_hash,
            );
            self.transport.send_packet(&host_pkt, peer_addr)?;
            sent_packets += 1;
        }

        for client in self.clients.values() {
            if client.addr == peer_addr {
                continue;
            }
            let peer_pkt = WirePacket::peer_status_with_hash(
                client.client_id,
                client.has_media,
                client.is_user_ready,
                client.duration_ms,
                client.file_size_bytes,
                &client.username,
                &client.filename,
                client.media_hash,
            );
            self.transport.send_packet(&peer_pkt, peer_addr)?;
            sent_packets += 1;
        }

        let room_pkt = self.build_room_state_packet();
        self.transport.send_packet(&room_pkt, peer_addr)?;
        sent_packets += 1;

        Ok(sent_packets)
    }

    /// Drains `newly_joined_peers` and unicasts the host status (if cached), existing peer
    /// statuses, and the current `RoomStateBroadcast` immediately to each new peer.
    ///
    /// Returns the number of newly joined peers welcomed.
    pub fn flush_welcome_to_new_peers(&mut self) -> Result<usize, SyncError> {
        let new_peers = std::mem::take(&mut self.newly_joined_peers);
        let mut welcomed = 0;
        for addr in new_peers {
            if self.clients.contains_key(&addr) {
                self.send_welcome_to_peer(addr)?;
                welcomed += 1;
            }
        }
        Ok(welcomed)
    }

    /// Drains and returns all pending received chat messages from connected peers.
    pub fn poll_chat_messages(&mut self) -> Vec<ChatMessagePayload> {
        std::mem::take(&mut self.chat_messages)
    }

    /// Broadcasts a chat message from the host to all registered client peers.
    ///
    /// Returns the number of peers to which the packet was successfully sent.
    pub fn broadcast_chat_message(&mut self, payload: &[u8]) -> Result<usize, SyncError> {
        let seq = self.next_chat_seq;
        self.next_chat_seq = self.next_chat_seq.wrapping_add(1);
        let wall_ms = self.current_server_wall_ms();
        let media_ms = self.ref_media_pts_ms;

        let packet = WirePacket::chat_message(seq, wall_ms, media_ms, payload.to_vec());
        let mut sent_count = 0;
        let targets: Vec<SocketAddr> = self.clients.keys().copied().collect();
        for addr in targets {
            if self.transport.send_packet(&packet, addr).is_ok() {
                sent_count += 1;
            }
        }
        Ok(sent_count)
    }

    /// Broadcasts authoritative `RoomState` updates to all registered clients.
    pub fn broadcast_room_state(&mut self) -> Result<usize, SyncError> {
        self.room_sequence = self.room_sequence.wrapping_add(1);
        let wall_ms = self.current_server_wall_ms();
        let speed_q16 = (self.target_speed * 65536.0) as u32;

        let packet = WirePacket::room_state(
            self.room_sequence,
            self.ref_media_pts_ms,
            speed_q16,
            wall_ms,
            self.is_paused,
        );

        let mut sent_count = 0;
        let targets: Vec<SocketAddr> = self.clients.keys().copied().collect();
        for addr in targets {
            if self.transport.send_packet(&packet, addr).is_ok() {
                sent_count += 1;
            }
        }
        Ok(sent_count)
    }

    /// Initiates a two-phase barrier unpause sequence.
    ///
    /// Schedules:
    /// `T_target_server = t_server_now + 1.5 * max_i(RTT_i) + t_preroll_budget + 100ms`
    pub fn prepare_barrier_unpause(&mut self, preroll_budget_ms: u32) -> Result<u64, SyncError> {
        let max_rtt = self.max_rtt_ms();
        let now_ms = self.current_server_wall_ms();
        let target_unpause_server_ms =
            compute_target_unpause_server(now_ms, max_rtt, preroll_budget_ms as u64);

        self.barrier_session_id = self.barrier_session_id.wrapping_add(1);
        self.barrier_prepare_started_at = Some(Instant::now());
        self.scheduled_unpause_server_ms = Some(target_unpause_server_ms);

        // Reset barrier preroll readiness on all clients
        for client in self.clients.values_mut() {
            client.barrier_preroll_ready = false;
        }

        self.barrier.prepare_unpause(now_ms, max_rtt, self.ref_media_pts_ms);

        // Broadcast BarrierPrepare with flags = FLAG_NONE
        let packet = WirePacket::barrier_prepare(
            self.barrier_session_id,
            target_unpause_server_ms,
            preroll_budget_ms,
            false,
        );

        let targets: Vec<SocketAddr> = self.clients.keys().copied().collect();
        for addr in targets {
            let _ = self.transport.send_packet(&packet, addr);
        }

        Ok(target_unpause_server_ms)
    }

    /// Returns true if all registered clients have reported readiness for the current barrier session.
    pub fn are_all_clients_ready(&self) -> bool {
        if self.clients.is_empty() {
            return true;
        }
        self.clients.values().all(|c| c.barrier_preroll_ready)
    }

    /// Returns true if all registered clients have media loaded and have marked themselves ready.
    pub fn are_all_users_ready(&self) -> bool {
        if self.clients.is_empty() {
            return true;
        }
        self.clients.values().all(|c| c.is_user_ready && c.has_media)
    }

    /// Returns a list of unready client IDs and usernames (clients without media or marked not ready).
    pub fn get_unready_clients(&self) -> Vec<(u32, String)> {
        self.clients
            .values()
            .filter(|c| !c.is_user_ready || !c.has_media)
            .map(|c| (c.client_id, c.username.clone()))
            .collect()
    }

    /// Returns a snapshot of all connected clients' peer statuses.
    pub fn peer_statuses(&self) -> Vec<PeerStatusPayload> {
        self.clients
            .values()
            .map(|c| PeerStatusPayload {
                client_id: c.client_id,
                has_media: c.has_media,
                is_ready: c.is_user_ready,
                duration_ms: c.duration_ms,
                file_size_bytes: c.file_size_bytes,
                username: c.username.clone(),
                filename: c.filename.clone(),
                media_hash: c.media_hash,
            })
            .collect()
    }

    /// Broadcasts a `PeerStatus` packet to all registered clients.
    pub fn broadcast_peer_status(
        &mut self,
        status: &PeerStatusPayload,
    ) -> Result<usize, SyncError> {
        let packet = WirePacket::peer_status_with_hash(
            status.client_id,
            status.has_media,
            status.is_ready,
            status.duration_ms,
            status.file_size_bytes,
            &status.username,
            &status.filename,
            status.media_hash,
        );
        let mut sent_count = 0;
        let targets: Vec<SocketAddr> = self.clients.keys().copied().collect();
        for addr in targets {
            if self.transport.send_packet(&packet, addr).is_ok() {
                sent_count += 1;
            }
        }
        Ok(sent_count)
    }

    /// Commits the unpause barrier sequence, broadcasting commit confirmation to all peers.
    pub fn commit_barrier_unpause(
        &mut self,
        target_unpause_server_ms: u64,
    ) -> Result<usize, SyncError> {
        let packet = WirePacket::barrier_prepare(
            self.barrier_session_id,
            target_unpause_server_ms,
            0,
            true, // is_commit
        );

        let mut sent_count = 0;
        let targets: Vec<SocketAddr> = self.clients.keys().copied().collect();
        for addr in targets {
            if self.transport.send_packet(&packet, addr).is_ok() {
                sent_count += 1;
            }
        }

        self.is_paused = false;
        self.barrier_prepare_started_at = None;
        self.barrier.commit(target_unpause_server_ms);

        Ok(sent_count)
    }

    /// Resets any active barrier prepare/commit state so dead or cancelled unpauses do not linger.
    pub fn reset_barrier(&mut self) {
        self.barrier_prepare_started_at = None;
        self.scheduled_unpause_server_ms = None;
        self.barrier = BarrierStateMachine::default();
        for client in self.clients.values_mut() {
            client.barrier_preroll_ready = false;
        }
    }

    /// Returns `true` if a barrier prepare phase has been waiting longer than `timeout`.
    pub fn is_barrier_prepare_timed_out(&self, timeout: Duration) -> bool {
        self.barrier_prepare_started_at
            .is_some_and(|started| started.elapsed() >= timeout)
    }

    /// Checks if the scheduled barrier unpause time has arrived on the server.
    pub fn check_scheduled_unpause(&mut self) -> bool {
        if let Some(target_ms) = self.scheduled_unpause_server_ms {
            if self.current_server_wall_ms() >= target_ms {
                self.scheduled_unpause_server_ms = None;
                self.is_paused = false;
                return true;
            }
        }
        false
    }

    /// Returns current barrier phase.
    pub fn barrier_phase(&self) -> BarrierPhase {
        self.barrier.phase()
    }
}

/// Client watch-party node communicating with the host server.
pub struct ClientNode {
    transport: UdpSyncTransport,
    host_addr: SocketAddr,
    client_id: u32,
    start_instant: Instant,
    synthetic_offset_ns: i64,
    filter: Rfc5905Filter,
    pll: PllSpeedController,
    barrier: BarrierStateMachine,
    pending_probes: HashMap<u32, u64>,
    next_probe_id: u32,
    latest_room_state: Option<RoomState>,
    local_pts_ms: u64,
    scheduled_unpause_local_ms: Option<u64>,
    is_playing: bool,
    chat_messages: Vec<ChatMessagePayload>,
    next_chat_seq: u32,
    reactions: Vec<ReactionPayload>,
    peer_statuses: HashMap<u32, PeerStatusPayload>,
    pending_peer_statuses: Vec<PeerStatusPayload>,
    probes_sent: usize,
    ntp_accepted_count: usize,
}

impl ClientNode {
    /// Binds a new `ClientNode` with the specified local address, target host address, and client ID.
    ///
    /// If `host_addr` is IPv6 and `bind_addr` resolves to `0.0.0.0:0`, transparently binds `[::]:0`.
    pub fn bind<A: ToSocketAddrs>(
        bind_addr: A,
        host_addr: SocketAddr,
        client_id: u32,
    ) -> Result<Self, SyncError> {
        let transport = if host_addr.is_ipv6() {
            let is_v4_unspecified = bind_addr
                .to_socket_addrs()
                .ok()
                .and_then(|mut it| it.next())
                .is_some_and(|a| {
                    matches!(a, SocketAddr::V4(v4) if v4.ip().is_unspecified() && v4.port() == 0)
                });
            if is_v4_unspecified {
                UdpSyncTransport::bind("[::]:0")?
            } else {
                UdpSyncTransport::bind(bind_addr)?
            }
        } else {
            UdpSyncTransport::bind(bind_addr)?
        };
        Ok(Self {
            transport,
            host_addr,
            client_id,
            start_instant: Instant::now(),
            synthetic_offset_ns: 0,
            filter: Rfc5905Filter::default(),
            pll: PllSpeedController::new(),
            barrier: BarrierStateMachine::default(),
            pending_probes: HashMap::new(),
            next_probe_id: 1,
            latest_room_state: None,
            local_pts_ms: 0,
            scheduled_unpause_local_ms: None,
            is_playing: false,
            chat_messages: Vec::new(),
            next_chat_seq: 1,
            reactions: Vec::new(),
            peer_statuses: HashMap::new(),
            pending_peer_statuses: Vec::new(),
            probes_sent: 0,
            ntp_accepted_count: 0,
        })
    }

    /// Binds a new `ClientNode` using the address family matching `server_addr` (`[::]:0` vs `0.0.0.0:0`).
    pub fn bind_for_target(server_addr: SocketAddr, peer_id: u32) -> Result<Self, SyncError> {
        let bind_addr = if server_addr.is_ipv6() {
            "[::]:0"
        } else {
            "0.0.0.0:0"
        };
        Self::bind(bind_addr, server_addr, peer_id)
    }

    /// Configures or clears the room encryption session on this client transport.
    pub fn set_crypto(&mut self, crypto: Option<Arc<RoomCryptoSession>>) {
        self.transport.set_crypto(crypto);
    }

    /// Builder-style helper to attach an optional room encryption session.
    pub fn with_crypto(mut self, crypto: Option<Arc<RoomCryptoSession>>) -> Self {
        self.transport.set_crypto(crypto);
        self
    }

    /// Returns `true` if packet encryption is enabled on this client node.
    pub fn is_encrypted(&self) -> bool {
        self.transport.is_encrypted()
    }

    /// Configures an artificial clock offset for testing and simulation.
    pub fn with_clock_offset(mut self, offset_ns: i64) -> Self {
        self.synthetic_offset_ns = offset_ns;
        self
    }

    /// Returns the local bound socket address.
    pub fn local_addr(&self) -> Result<SocketAddr, SyncError> {
        self.transport.local_addr()
    }

    /// Returns the configured host server address.
    pub fn host_addr(&self) -> SocketAddr {
        self.host_addr
    }

    /// Returns the assigned client identifier.
    pub fn client_id(&self) -> u32 {
        self.client_id
    }

    /// Returns current monotonic client time in nanoseconds, including synthetic offset.
    pub fn current_local_nanos(&self) -> u64 {
        let base_ns = self.start_instant.elapsed().as_nanos() as i64;
        let effective_ns = base_ns.saturating_add(self.synthetic_offset_ns);
        if effective_ns < 0 {
            0
        } else {
            effective_ns as u64
        }
    }

    /// Returns current monotonic client wall time in milliseconds.
    pub fn current_local_wall_ms(&self) -> u64 {
        self.current_local_nanos() / 1_000_000
    }

    /// Sends an NTP probe request (`0x01`) to the host server.
    pub fn send_ntp_probe(&mut self) -> Result<u32, SyncError> {
        let probe_id = self.next_probe_id;
        self.next_probe_id = self.next_probe_id.wrapping_add(1);

        let t1 = self.current_local_nanos();
        self.pending_probes.insert(probe_id, t1);

        let packet = WirePacket::ntp_request(probe_id, t1);
        self.transport.send_packet(&packet, self.host_addr)?;
        self.probes_sent = self.probes_sent.saturating_add(1);

        Ok(probe_id)
    }

    /// Returns the count of accepted NTP probe samples recorded by this client's filter.
    pub fn ntp_accepted_count(&self) -> usize {
        self.ntp_accepted_count
    }

    /// Returns the recommended interval between NTP probes (`75ms` during initial startup burst,
    /// transitioning to `1000ms` steady-state once 6 samples are accepted or 12 probes sent).
    pub fn recommended_ntp_probe_interval(&self) -> Duration {
        if self.ntp_accepted_count < 6 && self.probes_sent < 12 {
            Duration::from_millis(75)
        } else {
            Duration::from_millis(1000)
        }
    }

    /// Returns `true` when playback position should snap immediately rather than slew via PI PLL.
    pub fn should_snap_immediately(
        &self,
        drift_ms: u64,
        is_playing: bool,
        initial_sync_locked: bool,
    ) -> bool {
        (!initial_sync_locked && drift_ms > 40)
            || (!is_playing && drift_ms > 40)
            || drift_ms > 1500
    }

    /// Returns the estimated filtered clock offset $\hat{\theta}$ in nanoseconds from the RFC 5905 filter.
    pub fn filtered_offset_nanos(&self) -> Option<f64> {
        self.filter.get_filtered_offset()
    }

    /// Returns the median RTT in nanoseconds from the RFC 5905 filter window.
    pub fn median_rtt_nanos(&self) -> f64 {
        self.filter.get_median_rtt()
    }

    /// Returns the median RTT in milliseconds.
    pub fn median_rtt_ms(&self) -> u64 {
        (self.filter.get_median_rtt() / 1_000_000.0) as u64
    }

    /// Returns the current local media presentation position in milliseconds.
    pub fn local_pts_ms(&self) -> u64 {
        self.local_pts_ms
    }

    /// Updates the local media presentation position.
    pub fn set_local_pts_ms(&mut self, pts_ms: u64) {
        self.local_pts_ms = pts_ms;
    }

    /// Advances the local presentation position by the given speed ratio and elapsed time.
    pub fn advance_playback(&mut self, speed: f64, dt_secs: f64) {
        let advanced_ms = (speed * dt_secs * 1000.0) as u64;
        self.local_pts_ms = self.local_pts_ms.saturating_add(advanced_ms);
    }

    /// Returns true if the client is currently in active playback mode.
    pub fn is_playing(&self) -> bool {
        self.is_playing
    }

    /// Sets playback active/paused state.
    pub fn set_playing(&mut self, playing: bool) {
        self.is_playing = playing;
    }

    /// Returns the latest received room state, if any.
    pub fn latest_room_state(&self) -> Option<&RoomState> {
        self.latest_room_state.as_ref()
    }

    /// Polls for incoming datagrams and dispatches them according to protocol logic.
    pub fn poll_and_process(&mut self) -> Result<Option<WirePacket>, SyncError> {
        let res = self.transport.poll_packet()?;
        if let Some((ref packet, addr)) = res {
            if addr == self.host_addr {
                match &packet.payload {
                    PacketPayload::NtpProbeResponse(resp) => {
                        let t1 = self
                            .pending_probes
                            .remove(&resp.probe_id)
                            .unwrap_or(resp.t1_client_send_ns);
                        let t4 = self.current_local_nanos();
                        let sample =
                            ProbeSample::new(t1, resp.t2_server_recv_ns, resp.t3_server_send_ns, t4);
                        if self.filter.process_sample(sample) {
                            self.ntp_accepted_count = self.ntp_accepted_count.saturating_add(1);
                        }
                    }
                    PacketPayload::RoomStateBroadcast(room_payload) => {
                        let target_speed = (room_payload.target_speed_q16 as f64) / 65536.0;
                        let is_newer = self
                            .latest_room_state
                            .as_ref()
                            .is_none_or(|curr| room_payload.sequence > curr.seq);

                        if is_newer {
                            let room = RoomState {
                                t_ref_server_nanos: room_payload.ref_server_wall_ms * 1_000_000,
                                tau_ref_media_ms: room_payload.ref_media_pts_ms,
                                r_target: target_speed,
                                is_paused: room_payload.paused != 0,
                                seq: room_payload.sequence,
                            };
                            self.latest_room_state = Some(room);
                        }
                    }
                    PacketPayload::BarrierPrepare(prep) => {
                        if (packet.flags & FLAG_BARRIER_COMMIT) != 0 {
                            // Phase 3: Server commit confirmation
                            self.barrier.commit(prep.target_unpause_server_ms);
                        } else {
                            // Phase 1: Server prepare broadcast
                            // Translate target server timestamp to local wall time:
                            // T_local = T_target_server - theta_hat
                            let theta_hat_ms =
                                self.filter.get_filtered_offset().unwrap_or(0.0) / 1_000_000.0;
                            let target_local_ms =
                                ((prep.target_unpause_server_ms as f64) - theta_hat_ms).max(0.0) as u64;

                            self.scheduled_unpause_local_ms = Some(target_local_ms);
                            self.barrier.client_ready(self.client_id as u64);

                            // Send BarrierReady (Phase 2)
                            let ready_pkt = WirePacket::barrier_ready(
                                prep.session_id,
                                self.client_id,
                                prep.preroll_budget_ms.min(200),
                            );
                            let _ = self.transport.send_packet(&ready_pkt, self.host_addr);
                        }
                    }
                    PacketPayload::SeekCommand(seek) => {
                        self.local_pts_ms = seek.target_pts_ms;
                        self.pll.reset();
                    }
                    PacketPayload::ChatMessage(chat) => {
                        self.chat_messages.push(chat.clone());
                    }
                    PacketPayload::PeerStatus(status) => {
                        self.peer_statuses.insert(status.client_id, status.clone());
                        self.pending_peer_statuses.push(status.clone());
                    }
                    PacketPayload::NtpProbeRequest(req) => {
                        let t2 = self.current_local_nanos();
                        let t3 = self.current_local_nanos();
                        let resp = WirePacket::ntp_response(req.probe_id, req.t1_client_send_ns, t2, t3);
                        let _ = self.transport.send_packet(&resp, self.host_addr);
                    }
                    PacketPayload::Reaction(reaction) => {
                        self.reactions.push(reaction.clone());
                    }
                    _ => {}
                }
            }
        }
        Ok(res.map(|(p, _)| p))
    }

    /// Drains and returns all pending received reactions from the host server.
    pub fn poll_reactions(&mut self) -> Vec<ReactionPayload> {
        std::mem::take(&mut self.reactions)
    }

    /// Sends a floating reaction from this client to the host server.
    pub fn send_reaction(
        &mut self,
        emoji: &str,
        media_pts_ms: u64,
        username: &str,
    ) -> Result<(), SyncError> {
        let packet = WirePacket::reaction(self.client_id, media_pts_ms, emoji, username);
        self.transport.send_packet(&packet, self.host_addr)?;
        Ok(())
    }

    /// Broadcasts local peer media metadata, user readiness, and media hash fingerprint to the host.
    #[allow(clippy::too_many_arguments)]
    pub fn send_peer_status_with_hash(
        &mut self,
        has_media: bool,
        is_ready: bool,
        duration_ms: u64,
        file_size_bytes: u64,
        username: &str,
        filename: &str,
        media_hash: [u8; 32],
    ) -> Result<(), SyncError> {
        let clean_filename = std::path::Path::new(filename)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(filename);

        let packet = WirePacket::peer_status_with_hash(
            self.client_id,
            has_media,
            is_ready,
            duration_ms,
            file_size_bytes,
            username,
            clean_filename,
            media_hash,
        );
        self.transport.send_packet(&packet, self.host_addr)?;
        Ok(())
    }

    /// Drains and returns all pending received chat messages from the host server.
    pub fn poll_chat_messages(&mut self) -> Vec<ChatMessagePayload> {
        std::mem::take(&mut self.chat_messages)
    }

    /// Drains and returns all pending received peer status updates.
    pub fn poll_peer_statuses(&mut self) -> Vec<PeerStatusPayload> {
        std::mem::take(&mut self.pending_peer_statuses)
    }

    /// Returns a reference to all received peer status records indexed by client ID.
    pub fn peer_statuses(&self) -> &HashMap<u32, PeerStatusPayload> {
        &self.peer_statuses
    }

    /// Returns the cached status for a specific client ID, if known.
    pub fn get_peer_status(&self, client_id: u32) -> Option<&PeerStatusPayload> {
        self.peer_statuses.get(&client_id)
    }

    /// Broadcasts local peer media metadata and user readiness to the host server.
    ///
    /// Sanitizes the filename to its basename to avoid leaking local filesystem paths.
    pub fn send_peer_status(
        &mut self,
        has_media: bool,
        is_ready: bool,
        duration_ms: u64,
        file_size_bytes: u64,
        username: &str,
        filename: &str,
    ) -> Result<(), SyncError> {
        let clean_filename = std::path::Path::new(filename)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(filename);

        let packet = WirePacket::peer_status(
            self.client_id,
            has_media,
            is_ready,
            duration_ms,
            file_size_bytes,
            username,
            clean_filename,
        );
        self.transport.send_packet(&packet, self.host_addr)?;
        Ok(())
    }

    /// Sends an in-band chat message payload to the host server.
    ///
    /// Returns the transmitted chat sequence number.
    pub fn send_chat_message(&mut self, payload: &[u8]) -> Result<u32, SyncError> {
        let seq = self.next_chat_seq;
        self.next_chat_seq = self.next_chat_seq.wrapping_add(1);
        let wall_ms = self.current_local_wall_ms();
        let media_ms = self.local_pts_ms;

        let packet = WirePacket::chat_message(seq, wall_ms, media_ms, payload.to_vec());
        self.transport.send_packet(&packet, self.host_addr)?;
        Ok(seq)
    }

    /// Sends an in-band chat message with an explicit media presentation timestamp.
    pub fn send_chat_message_with_pts(
        &mut self,
        payload: &[u8],
        media_pts_ms: u64,
    ) -> Result<u32, SyncError> {
        let seq = self.next_chat_seq;
        self.next_chat_seq = self.next_chat_seq.wrapping_add(1);
        let wall_ms = self.current_local_wall_ms();

        let packet = WirePacket::chat_message(seq, wall_ms, media_pts_ms, payload.to_vec());
        self.transport.send_packet(&packet, self.host_addr)?;
        Ok(seq)
    }

    /// Sends an in-band seek command event to the host server.
    pub fn send_seek(&mut self, target_pts_ms: u64) -> Result<(), SyncError> {
        let event_id = self.next_probe_id;
        self.next_probe_id = self.next_probe_id.wrapping_add(1);
        let packet = WirePacket::seek_command(event_id, target_pts_ms);
        self.transport.send_packet(&packet, self.host_addr)?;
        Ok(())
    }

    /// Sends a bidirectional `PeerControl` command to the host server.
    pub fn send_peer_control(
        &mut self,
        action: PeerControlAction,
        media_pts_ms: u64,
        rate: f64,
    ) -> Result<(), SyncError> {
        let seq = self.next_probe_id;
        self.next_probe_id = self.next_probe_id.wrapping_add(1);
        let packet = WirePacket::peer_control(seq, self.client_id, action, media_pts_ms, rate);
        self.transport.send_packet(&packet, self.host_addr)?;
        Ok(())
    }

    /// Requests an immediate sync snapshot (`RoomStateBroadcast` + `PeerStatus`es) from the host.
    pub fn request_sync_snapshot(&mut self) -> Result<(), SyncError> {
        self.send_peer_control(
            PeerControlAction::RequestSyncSnapshot,
            self.local_pts_ms,
            1.0,
        )
    }

    /// Evaluates the discrete PI PLL speed controller against the reference timeline.
    ///
    /// Modulates playback rate within $r \in [0.96, 1.04]$ to converge phase error into
    /// the $|e| < 15\text{ms}$ Zone 1 deadband.
    pub fn update_pll(&mut self, dt_secs: f64) -> Option<PllCorrection> {
        let room = self.latest_room_state.as_ref()?;
        if room.is_paused {
            return None;
        }

        let theta_hat = self.filter.get_filtered_offset().unwrap_or(0.0);
        let now_nanos = self.current_local_nanos();
        let target_pts = room.compute_target_media_pts(now_nanos, theta_hat);

        let corr = self
            .pll
            .update_with_target_rate(target_pts, self.local_pts_ms, dt_secs, room.r_target);
        if corr.zone == PllZone::HardSeek {
            if let Some(target) = corr.hard_seek_target {
                self.local_pts_ms = target;
            }
        }

        Some(corr)
    }

    /// Returns the scheduled unpause local timestamp in milliseconds, if armed.
    pub fn scheduled_unpause_local_ms(&self) -> Option<u64> {
        self.scheduled_unpause_local_ms
    }

    /// Checks if the scheduled local unpause instant has arrived.
    pub fn check_scheduled_unpause(&mut self) -> bool {
        if let Some(target_ms) = self.scheduled_unpause_local_ms {
            if self.current_local_wall_ms() >= target_ms {
                self.scheduled_unpause_local_ms = None;
                self.is_playing = true;
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_udp_transport_loopback() {
        let t1 = UdpSyncTransport::bind("127.0.0.1:0").expect("bind t1");
        let t2 = UdpSyncTransport::bind("127.0.0.1:0").expect("bind t2");

        let addr1 = t1.local_addr().unwrap();
        let addr2 = t2.local_addr().unwrap();

        let req = WirePacket::ntp_request(100, 500_000_000);
        t1.send_packet(&req, addr2).expect("send");

        // Small spin-poll
        let mut received = None;
        for _ in 0..50 {
            if let Ok(Some((pkt, sender))) = t2.poll_packet() {
                assert_eq!(sender, addr1);
                received = Some(pkt);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }

        assert_eq!(received, Some(req));
    }

    #[test]
    fn test_host_server_ntp_response() {
        let mut host = HostServer::bind("127.0.0.1:0").expect("host bind");
        let host_addr = host.local_addr().unwrap();

        let mut client =
            ClientNode::bind("127.0.0.1:0", host_addr, 1).expect("client bind");

        // Client sends NTP probe
        let probe_id = client.send_ntp_probe().expect("send probe");
        assert_eq!(probe_id, 1);

        // Host processes request and responds
        let mut host_processed = false;
        for _ in 0..50 {
            if host.poll_and_process().unwrap().is_some() {
                host_processed = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(host_processed, "Host should have processed NTP request");

        // Client processes response
        let mut client_processed = false;
        for _ in 0..50 {
            if client.poll_and_process().unwrap().is_some() {
                client_processed = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(client_processed, "Client should have processed NTP response");
        assert!(client.filtered_offset_nanos().is_some());
    }

    #[test]
    fn test_host_and_client_chat_messaging_and_relay() {
        let mut host = HostServer::bind("127.0.0.1:0").expect("host bind");
        let host_addr = host.local_addr().unwrap();

        let mut client1 = ClientNode::bind("127.0.0.1:0", host_addr, 1).expect("client1 bind");
        let mut client2 = ClientNode::bind("127.0.0.1:0", host_addr, 2).expect("client2 bind");

        // Send NTP probe from both to register them on the host
        let _ = client1.send_ntp_probe().expect("send probe 1");
        let _ = client2.send_ntp_probe().expect("send probe 2");

        // Host processes both probes
        for _ in 0..50 {
            let _ = host.poll_and_process();
            if host.clients().len() >= 2 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(host.clients().len(), 2, "Both clients should be registered");

        // 1. Client 1 sends a chat message
        let payload1 = b"Hello from Client 1!".to_vec();
        let seq = client1.send_chat_message(&payload1).expect("client1 send");
        assert_eq!(seq, 1);

        // Host processes message: stores it and relays to Client 2 (not back to Client 1)
        let mut host_got_chat = false;
        for _ in 0..50 {
            if let Ok(Some((pkt, _))) = host.poll_and_process() {
                if matches!(pkt.payload, PacketPayload::ChatMessage(_)) {
                    host_got_chat = true;
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(host_got_chat, "Host should have received chat packet");

        let host_msgs = host.poll_chat_messages();
        assert_eq!(host_msgs.len(), 1);
        assert_eq!(host_msgs[0].payload, payload1);

        // Client 2 receives the relayed chat packet
        let mut client2_got_chat = false;
        for _ in 0..50 {
            if let Ok(Some(pkt)) = client2.poll_and_process() {
                if matches!(pkt.payload, PacketPayload::ChatMessage(_)) {
                    client2_got_chat = true;
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(client2_got_chat, "Client 2 should have received relayed chat packet");
        let client2_msgs = client2.poll_chat_messages();
        assert_eq!(client2_msgs.len(), 1);
        assert_eq!(client2_msgs[0].payload, payload1);

        // Client 1 should NOT have received an echo
        let _ = client1.poll_and_process();
        assert!(client1.poll_chat_messages().is_empty(), "Client 1 must not receive echo");

        // 2. Host broadcasts a chat message
        let host_payload = b"Host announcement: welcome!".to_vec();
        let sent_count = host.broadcast_chat_message(&host_payload).expect("host broadcast");
        assert_eq!(sent_count, 2);

        // Both clients receive host broadcast
        for _ in 0..50 {
            let _ = client1.poll_and_process();
            let _ = client2.poll_and_process();
            if !client1.chat_messages.is_empty() && !client2.chat_messages.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }

        let c1_msgs = client1.poll_chat_messages();
        let c2_msgs = client2.poll_chat_messages();
        assert_eq!(c1_msgs.len(), 1);
        assert_eq!(c1_msgs[0].payload, host_payload);
        assert_eq!(c2_msgs.len(), 1);
        assert_eq!(c2_msgs[0].payload, host_payload);
    }

    #[test]
    fn test_encrypted_host_client_roundtrip_and_plaintext_rejection() {
        let crypto = std::sync::Arc::new(
            RoomCryptoSession::from_password("enc-room", "hunter2").unwrap(),
        );
        let mut host = HostServer::bind("127.0.0.1:0")
            .unwrap()
            .with_crypto(Some(crypto.clone()));
        assert!(host.is_encrypted());
        let host_addr = host.local_addr().unwrap();

        let mut client = ClientNode::bind_for_target(host_addr, 7)
            .unwrap()
            .with_crypto(Some(crypto));
        assert!(client.is_encrypted());

        // Encrypted NTP roundtrip
        client.send_ntp_probe().unwrap();
        let mut got_probe = false;
        for _ in 0..50 {
            if host.poll_and_process().unwrap().is_some() {
                got_probe = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(got_probe);

        let mut got_resp = false;
        for _ in 0..50 {
            if client.poll_and_process().unwrap().is_some() {
                got_resp = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(got_resp);
        assert_eq!(client.ntp_accepted_count(), 1);

        // Unencrypted attacker packet sent to encrypted host must be rejected with SyncError::Crypto
        let rogue = UdpSyncTransport::bind("127.0.0.1:0").unwrap();
        let spoof = WirePacket::peer_control(1, 99, PeerControlAction::Pause, 0, 1.0);
        rogue.send_packet(&spoof, host_addr).unwrap();
        std::thread::sleep(Duration::from_millis(5));
        let err = host.poll_and_process();
        assert!(
            matches!(err, Err(SyncError::Crypto(_))),
            "expected SyncError::Crypto on plaintext spoof, got {:?}",
            err
        );
    }

    #[test]
    fn test_ipv6_loopback_and_transparent_v4_unspecified_bind() {
        let mut host = match HostServer::bind("[::1]:0") {
            Ok(h) => h,
            Err(_) => return, // Skip gracefully on environments without IPv6 loopback
        };
        let host_addr = host.local_addr().unwrap();
        assert!(host_addr.is_ipv6());

        // Both bind_for_target and bind("0.0.0.0:0", v6_addr, ..) must succeed
        let mut c1 = ClientNode::bind_for_target(host_addr, 1).unwrap();
        assert!(c1.local_addr().unwrap().is_ipv6());

        let mut c2 = ClientNode::bind("0.0.0.0:0", host_addr, 2).unwrap();
        assert!(c2.local_addr().unwrap().is_ipv6());

        c1.send_ntp_probe().unwrap();
        c2.send_ntp_probe().unwrap();
        for _ in 0..50 {
            let _ = host.poll_and_process();
            if host.clients().len() == 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(host.clients().len(), 2);

        for _ in 0..50 {
            let _ = c1.poll_and_process();
            let _ = c2.poll_and_process();
            if c1.ntp_accepted_count() == 1 && c2.ntp_accepted_count() == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(c1.ntp_accepted_count(), 1);
        assert_eq!(c2.ntp_accepted_count(), 1);
    }

    #[test]
    fn test_peer_join_welcome_request_snapshot_and_peer_controls() {
        let mut host = HostServer::bind("127.0.0.1:0").unwrap();
        let host_addr = host.local_addr().unwrap();

        host.set_media_state(12_345, 1.25, false);
        host.set_host_peer_status(PeerStatusPayload::with_hash(
            0,
            true,
            true,
            120_000,
            5_000_000,
            "HostUser",
            "movie.mkv",
            [0x11; 32],
        ));

        // 1. Client 1 joins via NTP probe: Host must NOT auto-emit extra packets on NtpProbeRequest
        let mut c1 = ClientNode::bind_for_target(host_addr, 10).unwrap();
        c1.send_ntp_probe().unwrap();
        std::thread::sleep(Duration::from_millis(5));
        assert!(host.poll_and_process().unwrap().is_some());

        // Exactly 1 packet (NtpProbeResponse) is queued for c1
        let pkt1 = c1.poll_and_process().unwrap().unwrap();
        assert_eq!(pkt1.packet_type, crate::wire::PacketType::NtpProbeResponse);
        assert!(c1.poll_and_process().unwrap().is_none());

        // Explicit flush_welcome_to_new_peers sends host PeerStatus + RoomStateBroadcast
        assert_eq!(host.flush_welcome_to_new_peers().unwrap(), 1);
        assert!(host.drain_newly_joined_peers().is_empty());
        std::thread::sleep(Duration::from_millis(5));
        while c1.poll_and_process().unwrap().is_some() {}
        assert_eq!(c1.latest_room_state().unwrap().tau_ref_media_ms, 12_345);
        assert_eq!(c1.get_peer_status(0).unwrap().username, "HostUser");

        // c1 sends its own PeerStatus
        c1.send_peer_status(true, true, 120_000, 5_000_000, "Alice", "movie.mkv")
            .unwrap();
        std::thread::sleep(Duration::from_millis(5));
        let _ = host.poll_and_process().unwrap();

        // 2. Client 2 joins and immediately requests sync snapshot
        let mut c2 = ClientNode::bind_for_target(host_addr, 20).unwrap();
        c2.request_sync_snapshot().unwrap();
        std::thread::sleep(Duration::from_millis(5));
        let _ = host.poll_and_process().unwrap();

        // RequestSyncSnapshot cleared c2 from newly_joined_peers and sent welcome immediately
        assert!(host.drain_newly_joined_peers().is_empty());
        std::thread::sleep(Duration::from_millis(5));
        while c2.poll_and_process().unwrap().is_some() {}
        assert_eq!(c2.latest_room_state().unwrap().tau_ref_media_ms, 12_345);
        assert_eq!(c2.get_peer_status(0).unwrap().username, "HostUser");
        assert_eq!(c2.get_peer_status(10).unwrap().username, "Alice");

        // 3. PeerControl & SeekCommand queuing on Host
        c2.send_peer_control(PeerControlAction::Pause, 12_500, 1.0)
            .unwrap();
        c2.send_peer_control(PeerControlAction::SetSpeed, 12_500, 1.5)
            .unwrap();
        c2.send_seek(55_000).unwrap();
        std::thread::sleep(Duration::from_millis(5));
        while host.poll_and_process().unwrap().is_some() {}

        let controls = host.poll_peer_controls();
        assert_eq!(controls.len(), 3);
        assert_eq!(controls[0].1.action, PeerControlAction::Pause);
        assert_eq!(controls[1].1.action, PeerControlAction::SetSpeed);
        assert!((controls[1].1.rate_f64() - 1.5).abs() < 1e-4);
        assert_eq!(controls[2].1.action, PeerControlAction::Seek);
        assert_eq!(controls[2].1.media_pts_ms, 55_000);
        assert!(host.poll_peer_controls().is_empty());
    }

    #[test]
    fn test_fast_ntp_warmup_interval_and_should_snap_immediately() {
        let mut host = HostServer::bind("127.0.0.1:0").unwrap();
        let host_addr = host.local_addr().unwrap();
        let mut client = ClientNode::bind_for_target(host_addr, 1).unwrap();

        assert_eq!(
            client.recommended_ntp_probe_interval(),
            Duration::from_millis(75)
        );
        assert_eq!(client.ntp_accepted_count(), 0);

        for _ in 0..6 {
            client.send_ntp_probe().unwrap();
            std::thread::sleep(Duration::from_millis(2));
            let _ = host.poll_and_process().unwrap();
            std::thread::sleep(Duration::from_millis(2));
            let _ = client.poll_and_process().unwrap();
        }
        assert_eq!(client.ntp_accepted_count(), 6);
        assert_eq!(
            client.recommended_ntp_probe_interval(),
            Duration::from_millis(1000)
        );

        // Also check should_snap_immediately rules
        assert!(client.should_snap_immediately(41, true, false));
        assert!(!client.should_snap_immediately(40, true, false));
        assert!(client.should_snap_immediately(41, false, true));
        assert!(!client.should_snap_immediately(40, false, true));
        assert!(!client.should_snap_immediately(500, true, true));
        assert!(client.should_snap_immediately(1501, true, true));
    }

    #[test]
    fn test_barrier_prepare_timeout_and_stale_client_pruning() {
        let mut host = HostServer::bind("127.0.0.1:0").unwrap();
        let peer_addr: SocketAddr = "127.0.0.1:45678".parse().unwrap();
        host.register_client(peer_addr, 42, 20);

        assert!(!host.is_barrier_prepare_timed_out(Duration::from_millis(10)));
        let target_ms = host.prepare_barrier_unpause(100).unwrap();
        assert!(!host.are_all_clients_ready());

        std::thread::sleep(Duration::from_millis(15));
        assert!(host.is_barrier_prepare_timed_out(Duration::from_millis(10)));

        // Commit clears timeout
        host.commit_barrier_unpause(target_ms).unwrap();
        assert!(!host.is_barrier_prepare_timed_out(Duration::from_millis(10)));

        // Reset also clears timeout
        host.prepare_barrier_unpause(100).unwrap();
        std::thread::sleep(Duration::from_millis(15));
        assert!(host.is_barrier_prepare_timed_out(Duration::from_millis(10)));
        host.reset_barrier();
        assert!(!host.is_barrier_prepare_timed_out(Duration::from_millis(10)));

        // Stale client pruning removes idle dead peer
        std::thread::sleep(Duration::from_millis(20));
        let pruned = host.prune_stale_clients(Duration::from_millis(10));
        assert_eq!(pruned, vec![42]);
        assert!(host.clients().is_empty());
        assert!(host.are_all_clients_ready());
    }
}
