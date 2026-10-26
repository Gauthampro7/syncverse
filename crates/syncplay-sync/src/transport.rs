//! Real-time non-blocking UDP watch-party network transport.
//!
//! Provides the low-level `UdpSyncTransport` wrapper as well as the high-level
//! `HostServer` and `ClientNode` orchestration engines for synchronized watch parties.

use std::collections::HashMap;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::Instant;

use crate::barrier::{compute_target_unpause_server, BarrierPhase, BarrierStateMachine};
use crate::error::SyncError;
use crate::ntp::{ProbeSample, Rfc5905Filter};
use crate::pll::{PllCorrection, PllSpeedController, PllZone};
use crate::timeline::RoomState;
use crate::wire::{
    ChatMessagePayload, PacketPayload, PeerStatusPayload, WirePacket, FLAG_BARRIER_COMMIT,
};

/// Non-blocking UDP transport wrapper for watch-party synchronization.
pub struct UdpSyncTransport {
    socket: UdpSocket,
}

impl UdpSyncTransport {
    /// Binds a UDP socket to the specified address in non-blocking mode.
    pub fn bind<A: ToSocketAddrs>(addr: A) -> Result<Self, SyncError> {
        let socket = UdpSocket::bind(addr).map_err(|e| SyncError::Io(e.to_string()))?;
        socket
            .set_nonblocking(true)
            .map_err(|e| SyncError::Io(e.to_string()))?;
        Ok(Self { socket })
    }

    /// Returns the local socket address.
    pub fn local_addr(&self) -> Result<SocketAddr, SyncError> {
        self.socket
            .local_addr()
            .map_err(|e| SyncError::Io(e.to_string()))
    }

    /// Sends a wire packet to the specified remote target address.
    pub fn send_packet(&self, packet: &WirePacket, target: SocketAddr) -> Result<usize, SyncError> {
        let bytes = packet.to_bytes();
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
                let packet = WirePacket::from_bytes(&buf[..n])?;
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
}

/// Host watch-party server managing UDP transport, NTP timekeeping, and barrier synchronization.
pub struct HostServer {
    transport: UdpSyncTransport,
    start_instant: Instant,
    clients: HashMap<SocketAddr, ClientSession>,
    room_sequence: u64,
    ref_media_pts_ms: u64,
    target_speed: f64,
    is_paused: bool,
    barrier: BarrierStateMachine,
    barrier_session_id: u32,
    scheduled_unpause_server_ms: Option<u64>,
    chat_messages: Vec<ChatMessagePayload>,
    next_chat_seq: u32,
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
            room_sequence: 1,
            ref_media_pts_ms: 0,
            target_speed: 1.0,
            is_paused: true,
            barrier: BarrierStateMachine::default(),
            barrier_session_id: 0,
            scheduled_unpause_server_ms: None,
            chat_messages: Vec::new(),
            next_chat_seq: 1,
            pending_probes: HashMap::new(),
            next_probe_id: 1,
        })
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
                });

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
                    self.broadcast_room_state()?;
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
                _ => {}
            }
        }
        Ok(res)
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
            })
            .collect()
    }

    /// Broadcasts a `PeerStatus` packet to all registered clients.
    pub fn broadcast_peer_status(
        &mut self,
        status: &PeerStatusPayload,
    ) -> Result<usize, SyncError> {
        let packet = WirePacket::peer_status(
            status.client_id,
            status.has_media,
            status.is_ready,
            status.duration_ms,
            status.file_size_bytes,
            &status.username,
            &status.filename,
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
        self.barrier.commit(target_unpause_server_ms);

        Ok(sent_count)
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
    peer_statuses: HashMap<u32, PeerStatusPayload>,
    pending_peer_statuses: Vec<PeerStatusPayload>,
}

impl ClientNode {
    /// Binds a new `ClientNode` with the specified local address, target host address, and client ID.
    pub fn bind<A: ToSocketAddrs>(
        bind_addr: A,
        host_addr: SocketAddr,
        client_id: u32,
    ) -> Result<Self, SyncError> {
        let transport = UdpSyncTransport::bind(bind_addr)?;
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
            peer_statuses: HashMap::new(),
            pending_peer_statuses: Vec::new(),
        })
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

        Ok(probe_id)
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
                        self.filter.process_sample(sample);
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
                    _ => {}
                }
            }
        }
        Ok(res.map(|(p, _)| p))
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

        let corr = self.pll.update(target_pts, self.local_pts_ms, dt_secs);
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
}
