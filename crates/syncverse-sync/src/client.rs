//! Watch-party client node orchestration.
//!
//! Provides the `ClientNode` type for synchronizing with a remote host server,
//! evaluating the RFC 5905 filter, steering playback with the PI PLL, and sending
//! and receiving in-band chat messages.
//!
//! This module also owns the **client connection state machine** that makes a room behave
//! like a real session instead of fire-and-forget UDP:
//!
//! ```text
//!   Idle --begin_join--> Connecting --HelloAck--> Connected <--HelloAck / host traffic--+
//!                          |   |                     |                                 |
//!                          |   +--reject/timeout-->  | silence_timeout                 |
//!                          |        Failed(reason)   v                                 |
//!                          |                    Reconnecting{attempt} -----------------+
//!                          |                         | gives up after 60 s
//!                          |                         v
//!                          |                    Failed(HostLost)
//!                          +--Bye(HostClosed) / leave()--> Closed
//! ```
//!
//! The pure transition logic lives in `LinkMachine`, which takes an explicit `Instant` so
//! it can be unit-tested deterministically; [`ClientNode`] merely performs its I/O actions.

use std::time::{Duration, Instant};

use thiserror::Error;

use crate::crypto::random_u32;
use crate::wire::{ByeReason, HelloStatus, PacketPayload, WirePacket};

pub use crate::transport::ClientNode;

/// Tunable timeouts of the room resilience layer (handshake, heartbeat, reconnect, prune).
///
/// [`ResilienceConfig::default`] holds the production values; [`ResilienceConfig::fast`]
/// scales everything down so integration tests complete in about a second.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResilienceConfig {
    /// Interval between `Hello` retransmissions while connecting (default 250 ms).
    pub hello_retry_interval: Duration,
    /// How long a `Hello` may stay unanswered before the client probes for a v0.4 host that
    /// implements no handshake (default 1.5 s).
    pub legacy_fallback_after: Duration,
    /// Total time budget of the initial connect before `Failed(Timeout)` (default 5 s).
    pub connect_timeout: Duration,
    /// Steady-state heartbeat period; also keeps NAT mappings warm (default 2 s).
    pub heartbeat_interval: Duration,
    /// Host silence after which the client enters `Reconnecting` (default 6 s).
    pub silence_timeout: Duration,
    /// First reconnect back-off delay (default 1 s); doubles per attempt (1, 2, 4, 8, 15 ...).
    pub backoff_initial: Duration,
    /// Upper bound of a single reconnect back-off delay (default 15 s).
    pub backoff_max: Duration,
    /// Total time spent reconnecting before `Failed(HostLost)` (default 60 s).
    pub reconnect_give_up_after: Duration,
    /// Host-side backstop: silent clients are pruned after this long (default 12 s).
    pub host_prune_timeout: Duration,
}

impl Default for ResilienceConfig {
    fn default() -> Self {
        Self {
            hello_retry_interval: Duration::from_millis(250),
            legacy_fallback_after: Duration::from_millis(1500),
            connect_timeout: Duration::from_secs(5),
            heartbeat_interval: Duration::from_secs(2),
            silence_timeout: Duration::from_secs(6),
            backoff_initial: Duration::from_secs(1),
            backoff_max: Duration::from_secs(15),
            reconnect_give_up_after: Duration::from_secs(60),
            host_prune_timeout: Duration::from_secs(12),
        }
    }
}

impl ResilienceConfig {
    /// A configuration with every timeout scaled down for tests (silence timeout 600 ms).
    pub fn fast() -> Self {
        Self {
            hello_retry_interval: Duration::from_millis(30),
            legacy_fallback_after: Duration::from_millis(150),
            connect_timeout: Duration::from_millis(600),
            heartbeat_interval: Duration::from_millis(100),
            silence_timeout: Duration::from_millis(600),
            backoff_initial: Duration::from_millis(50),
            backoff_max: Duration::from_millis(200),
            reconnect_give_up_after: Duration::from_secs(5),
            host_prune_timeout: Duration::from_millis(1200),
        }
    }

    /// Delay after reconnect attempt `attempt` (1-based) before the next one starts.
    pub fn backoff_after(&self, attempt: u32) -> Duration {
        let shift = attempt.saturating_sub(1).min(16);
        let scaled = self
            .backoff_initial
            .checked_mul(1u32 << shift)
            .unwrap_or(self.backoff_max);
        scaled.min(self.backoff_max)
    }

    /// Accelerated heartbeat cadence used while the host has been quiet for over one interval.
    ///
    /// Probing faster than the steady-state period makes a 30 % lossy link statistically
    /// unable to trip the silence timeout.
    pub fn probe_cadence(&self) -> Duration {
        (self.silence_timeout / 20).clamp(Duration::from_millis(50), Duration::from_millis(500))
    }
}

/// Why a join attempt or an established session ended in failure.
#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectFailReason {
    /// The host never answered the handshake within the connect budget.
    #[error("host did not respond")]
    Timeout,
    /// The host could not authenticate the join (wrong room password / encryption mismatch).
    #[error("authentication failed (wrong room password?)")]
    AuthFailed,
    /// The host refuses additional peers.
    #[error("host is full")]
    HostFull,
    /// The host speaks an incompatible protocol version.
    #[error("incompatible host protocol version")]
    VersionUnsupported,
    /// The session was established but the host stayed unreachable until reconnecting gave up.
    #[error("host lost")]
    HostLost,
}

/// Why a session was closed cleanly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReason {
    /// This client left via [`ClientNode::leave`].
    LocalLeave,
    /// The host announced that it closed the room.
    HostClosed,
}

/// Externally visible state of the client's link to the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientConnectionState {
    /// No handshake was started: legacy implicit mode (no heartbeat, no timeout).
    Idle,
    /// `Hello` sent; waiting for `HelloAck`.
    Connecting,
    /// Host acknowledged and is alive.
    Connected,
    /// Host went silent; retrying with back-off. `attempt` starts at 1.
    Reconnecting {
        /// 1-based reconnect attempt counter.
        attempt: u32,
    },
    /// Gave up; see the reason.
    Failed(ConnectFailReason),
    /// Cleanly closed.
    Closed(CloseReason),
}

/// Event emitted by the connection state machine, drained with
/// [`ClientNode::poll_connection_events`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionEvent {
    /// The state changed to the contained value.
    StateChanged(ClientConnectionState),
    /// The host does not implement the handshake (v0.4); the client joined implicitly.
    LegacyHostFallback,
    /// A reconnect succeeded and local state (status, NTP, snapshot request) was re-sent.
    Rejoined {
        /// The host process restarted while we were away (new host epoch).
        host_restarted: bool,
    },
}

/// A peer that left or was removed from the host's room.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepartedPeer {
    /// Client identifier the peer held.
    pub client_id: u32,
    /// Socket address the peer used.
    pub addr: std::net::SocketAddr,
    /// Last announced display name (may be empty).
    pub username: String,
    /// Why it departed (`Leave` for a Bye, `Timeout` for a prune).
    pub reason: ByeReason,
}

/// I/O the owning [`ClientNode`] must perform on behalf of the state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LinkAction {
    /// Send a `Hello` with this nonce.
    Hello {
        /// Join attempt identifier.
        nonce: u32,
    },
    /// Send a heartbeat with this sequence number.
    Heartbeat(u32),
    /// Answer a host heartbeat.
    HeartbeatReply(u32),
    /// Send a legacy NTP probe (proof of life for a v0.4 host).
    LegacyProbe,
    /// Adopt the client id assigned by the host.
    AdoptClientId(u32),
    /// A reconnect succeeded: re-send cached status, probes and a snapshot request.
    Rejoin {
        /// Host epoch changed during the outage.
        host_restarted: bool,
    },
}

/// Pure, clock-injected connection state machine behind [`ClientNode`].
#[derive(Debug)]
pub(crate) struct LinkMachine {
    cfg: ResilienceConfig,
    state: ClientConnectionState,
    nonce: u32,
    phase_started: Instant,
    last_hello: Option<Instant>,
    last_legacy_probe: Option<Instant>,
    last_rx: Instant,
    last_hb: Instant,
    hb_seq: u32,
    attempt: u32,
    next_try: Instant,
    legacy_evidence: bool,
    legacy: bool,
    host_epoch: Option<u32>,
    events: Vec<ConnectionEvent>,
}

impl LinkMachine {
    /// Creates an idle machine (legacy implicit mode).
    pub(crate) fn new(cfg: ResilienceConfig, now: Instant) -> Self {
        Self {
            cfg,
            state: ClientConnectionState::Idle,
            nonce: 0,
            phase_started: now,
            last_hello: None,
            last_legacy_probe: None,
            last_rx: now,
            last_hb: now,
            hb_seq: 0,
            attempt: 0,
            next_try: now,
            legacy_evidence: false,
            legacy: false,
            host_epoch: None,
            events: Vec::new(),
        }
    }

    pub(crate) fn config(&self) -> &ResilienceConfig {
        &self.cfg
    }

    pub(crate) fn set_config(&mut self, cfg: ResilienceConfig) {
        self.cfg = cfg;
    }

    pub(crate) fn state(&self) -> ClientConnectionState {
        self.state
    }

    pub(crate) fn is_legacy(&self) -> bool {
        self.legacy
    }

    pub(crate) fn host_epoch(&self) -> Option<u32> {
        self.host_epoch
    }

    /// Time until the next reconnect attempt, if currently reconnecting.
    pub(crate) fn retry_in(&self, now: Instant) -> Option<Duration> {
        matches!(self.state, ClientConnectionState::Reconnecting { .. })
            .then(|| self.next_try.saturating_duration_since(now))
    }

    pub(crate) fn drain_events(&mut self) -> Vec<ConnectionEvent> {
        std::mem::take(&mut self.events)
    }

    fn set_state(&mut self, state: ClientConnectionState) {
        if self.state != state {
            self.state = state;
            self.events.push(ConnectionEvent::StateChanged(state));
        }
    }

    /// Starts (or restarts) the join handshake.
    pub(crate) fn begin_join(&mut self, now: Instant) -> Vec<LinkAction> {
        self.nonce = random_u32().max(1);
        self.phase_started = now;
        self.last_hello = Some(now);
        self.last_legacy_probe = None;
        self.last_rx = now;
        self.legacy_evidence = false;
        self.legacy = false;
        self.attempt = 0;
        self.set_state(ClientConnectionState::Connecting);
        vec![LinkAction::Hello { nonce: self.nonce }]
    }

    /// Marks the session closed after a local leave.
    pub(crate) fn leave(&mut self) {
        if matches!(
            self.state,
            ClientConnectionState::Idle
                | ClientConnectionState::Connecting
                | ClientConnectionState::Connected
                | ClientConnectionState::Reconnecting { .. }
        ) {
            self.set_state(ClientConnectionState::Closed(CloseReason::LocalLeave));
        }
    }

    fn fail(&mut self, reason: ConnectFailReason) {
        self.set_state(ClientConnectionState::Failed(reason));
    }

    fn enter_reconnecting(&mut self, now: Instant) -> Vec<LinkAction> {
        self.nonce = random_u32().max(1);
        self.phase_started = now;
        self.attempt = 1;
        self.last_hello = Some(now);
        self.next_try = now + self.cfg.backoff_after(1);
        self.set_state(ClientConnectionState::Reconnecting { attempt: 1 });
        let mut actions = vec![LinkAction::Hello { nonce: self.nonce }];
        if self.legacy {
            actions.push(LinkAction::LegacyProbe);
        }
        actions
    }

    fn become_connected(&mut self, now: Instant, legacy: bool) {
        self.legacy = legacy;
        self.last_rx = now;
        self.last_hb = now;
        self.attempt = 0;
        self.set_state(ClientConnectionState::Connected);
        if legacy {
            self.events.push(ConnectionEvent::LegacyHostFallback);
        }
    }

    fn restore(&mut self, now: Instant, host_restarted: bool) -> Vec<LinkAction> {
        self.last_rx = now;
        self.last_hb = now;
        self.attempt = 0;
        self.set_state(ClientConnectionState::Connected);
        self.events
            .push(ConnectionEvent::Rejoined { host_restarted });
        vec![LinkAction::Rejoin { host_restarted }]
    }

    /// Feeds a validated packet received from the host address.
    pub(crate) fn on_host_packet(&mut self, now: Instant, packet: &WirePacket) -> Vec<LinkAction> {
        let mut actions = Vec::new();
        match &packet.payload {
            PacketPayload::HelloAck(ack) => {
                if ack.status != HelloStatus::Accepted {
                    if self.state == ClientConnectionState::Connecting {
                        self.fail(match ack.status {
                            HelloStatus::AuthFailed => ConnectFailReason::AuthFailed,
                            HelloStatus::HostFull => ConnectFailReason::HostFull,
                            _ => ConnectFailReason::VersionUnsupported,
                        });
                    }
                    return actions;
                }
                if ack.nonce != self.nonce {
                    return actions;
                }
                self.last_rx = now;
                actions.push(LinkAction::AdoptClientId(ack.client_id));
                match self.state {
                    ClientConnectionState::Connecting => {
                        self.host_epoch = Some(ack.host_epoch);
                        self.become_connected(now, false);
                    }
                    ClientConnectionState::Reconnecting { .. } => {
                        let restarted = self.host_epoch.is_some_and(|e| e != ack.host_epoch);
                        self.host_epoch = Some(ack.host_epoch);
                        self.legacy = false;
                        actions.extend(self.restore(now, restarted));
                    }
                    ClientConnectionState::Connected => {
                        // Late duplicate ack, or a legacy session upgrading to the handshake.
                        self.host_epoch = Some(ack.host_epoch);
                        self.legacy = false;
                    }
                    _ => {}
                }
            }
            PacketPayload::Bye(bye) => match bye.reason {
                ByeReason::HostClosed => {
                    if matches!(
                        self.state,
                        ClientConnectionState::Idle
                            | ClientConnectionState::Connecting
                            | ClientConnectionState::Connected
                            | ClientConnectionState::Reconnecting { .. }
                    ) {
                        self.set_state(ClientConnectionState::Closed(CloseReason::HostClosed));
                    }
                }
                ByeReason::NotJoined | ByeReason::Timeout => {
                    self.last_rx = now;
                    if self.state == ClientConnectionState::Connected {
                        actions.extend(self.enter_reconnecting(now));
                    }
                }
                ByeReason::Leave => {}
            },
            PacketPayload::Heartbeat(hb) => {
                self.last_rx = now;
                if !hb.is_reply {
                    actions.push(LinkAction::HeartbeatReply(hb.seq));
                }
                if matches!(self.state, ClientConnectionState::Reconnecting { .. }) {
                    actions.extend(self.restore(now, false));
                }
            }
            _ => {
                self.last_rx = now;
                match self.state {
                    ClientConnectionState::Connecting => self.legacy_evidence = true,
                    ClientConnectionState::Reconnecting { .. } => {
                        actions.extend(self.restore(now, false));
                    }
                    _ => {}
                }
            }
        }
        actions
    }

    /// Advances timers; returns the I/O to perform.
    pub(crate) fn tick(&mut self, now: Instant) -> Vec<LinkAction> {
        let mut actions = Vec::new();
        match self.state {
            ClientConnectionState::Connecting => {
                let elapsed = now.saturating_duration_since(self.phase_started);
                if elapsed >= self.cfg.connect_timeout {
                    self.fail(ConnectFailReason::Timeout);
                    return actions;
                }
                if elapsed >= self.cfg.legacy_fallback_after {
                    if self.legacy_evidence {
                        self.become_connected(now, true);
                        return actions;
                    }
                    let due = self.last_legacy_probe.is_none_or(|t| {
                        now.saturating_duration_since(t) >= self.cfg.hello_retry_interval * 2
                    });
                    if due {
                        self.last_legacy_probe = Some(now);
                        actions.push(LinkAction::LegacyProbe);
                    }
                }
                let hello_due = self.last_hello.is_none_or(|t| {
                    now.saturating_duration_since(t) >= self.cfg.hello_retry_interval
                });
                if hello_due {
                    self.last_hello = Some(now);
                    actions.push(LinkAction::Hello { nonce: self.nonce });
                }
            }
            ClientConnectionState::Connected => {
                let quiet = now.saturating_duration_since(self.last_rx);
                if quiet >= self.cfg.silence_timeout {
                    return self.enter_reconnecting(now);
                }
                let gap = if quiet >= self.cfg.heartbeat_interval {
                    self.cfg.probe_cadence()
                } else {
                    self.cfg.heartbeat_interval
                };
                if now.saturating_duration_since(self.last_hb) >= gap {
                    self.last_hb = now;
                    if self.legacy {
                        actions.push(LinkAction::LegacyProbe);
                    } else {
                        self.hb_seq = self.hb_seq.wrapping_add(1);
                        actions.push(LinkAction::Heartbeat(self.hb_seq));
                    }
                }
                if self.legacy {
                    // Occasionally retry the handshake so a mis-detected host can upgrade.
                    let due = self.last_hello.is_none_or(|t| {
                        now.saturating_duration_since(t) >= self.cfg.heartbeat_interval * 3
                    });
                    if due {
                        self.last_hello = Some(now);
                        actions.push(LinkAction::Hello { nonce: self.nonce });
                    }
                }
            }
            ClientConnectionState::Reconnecting { .. } => {
                if now.saturating_duration_since(self.phase_started)
                    >= self.cfg.reconnect_give_up_after
                {
                    self.fail(ConnectFailReason::HostLost);
                    return actions;
                }
                if now >= self.next_try {
                    self.attempt += 1;
                    self.last_hello = Some(now);
                    self.next_try = now + self.cfg.backoff_after(self.attempt);
                    self.set_state(ClientConnectionState::Reconnecting {
                        attempt: self.attempt,
                    });
                    actions.push(LinkAction::Hello { nonce: self.nonce });
                    if self.legacy {
                        actions.push(LinkAction::LegacyProbe);
                    }
                }
            }
            ClientConnectionState::Idle
            | ClientConnectionState::Failed(_)
            | ClientConnectionState::Closed(_) => {}
        }
        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ack(nonce: u32, epoch: u32) -> WirePacket {
        WirePacket::hello_ack(nonce, HelloStatus::Accepted, 7, epoch)
    }

    #[test]
    fn backoff_schedule_is_1_2_4_8_15_capped() {
        let cfg = ResilienceConfig::default();
        let secs: Vec<u64> = (1..=7).map(|a| cfg.backoff_after(a).as_secs()).collect();
        assert_eq!(secs, vec![1, 2, 4, 8, 15, 15, 15]);
    }

    #[test]
    fn connect_times_out_without_ack_and_never_connects() {
        let start = Instant::now();
        let mut m = LinkMachine::new(ResilienceConfig::default(), start);
        m.begin_join(start);
        let mut now = start;
        for _ in 0..80 {
            now += Duration::from_millis(100);
            m.tick(now);
            assert_ne!(m.state(), ClientConnectionState::Connected);
        }
        assert_eq!(
            m.state(),
            ClientConnectionState::Failed(ConnectFailReason::Timeout)
        );
    }

    #[test]
    fn silence_then_backoff_then_give_up_after_60s() {
        let start = Instant::now();
        let mut m = LinkMachine::new(ResilienceConfig::default(), start);
        let actions = m.begin_join(start);
        let LinkAction::Hello { nonce } = actions[0] else {
            panic!("expected hello")
        };
        m.on_host_packet(start, &ack(nonce, 9));
        assert_eq!(m.state(), ClientConnectionState::Connected);
        let mut now = start;
        let mut attempts_seen = Vec::new();
        let mut reconnect_entered = None;
        while now - start < Duration::from_secs(80) {
            now += Duration::from_millis(100);
            m.tick(now);
            if let ClientConnectionState::Reconnecting { attempt } = m.state() {
                reconnect_entered.get_or_insert(now);
                if attempts_seen.last() != Some(&attempt) {
                    attempts_seen.push(attempt);
                }
            }
            if matches!(m.state(), ClientConnectionState::Failed(_)) {
                break;
            }
        }
        let entered = reconnect_entered.expect("must reconnect");
        assert!(entered - start >= Duration::from_secs(6));
        assert!(entered - start < Duration::from_millis(6200));
        assert_eq!(&attempts_seen[..5], &[1, 2, 3, 4, 5]);
        assert_eq!(
            m.state(),
            ClientConnectionState::Failed(ConnectFailReason::HostLost)
        );
        let outage = now - entered;
        assert!(outage >= Duration::from_secs(60) && outage < Duration::from_millis(60300));
    }

    #[test]
    fn legacy_host_detected_by_non_handshake_traffic_after_fallback_delay() {
        let start = Instant::now();
        let mut m = LinkMachine::new(ResilienceConfig::default(), start);
        m.begin_join(start);
        // v0.4 host answers an NTP probe early: not yet accepted as connected.
        m.on_host_packet(
            start + Duration::from_millis(100),
            &WirePacket::ntp_response(1, 0, 0, 0),
        );
        m.tick(start + Duration::from_millis(900));
        assert_eq!(m.state(), ClientConnectionState::Connecting);
        m.tick(start + Duration::from_millis(1600));
        assert_eq!(m.state(), ClientConnectionState::Connected);
        assert!(m.is_legacy());
        assert!(m
            .drain_events()
            .contains(&ConnectionEvent::LegacyHostFallback));
    }

    #[test]
    fn reject_status_maps_to_failure_reason() {
        let start = Instant::now();
        let mut m = LinkMachine::new(ResilienceConfig::default(), start);
        m.begin_join(start);
        m.on_host_packet(
            start,
            &WirePacket::hello_ack(0, HelloStatus::AuthFailed, 0, 0),
        );
        assert_eq!(
            m.state(),
            ClientConnectionState::Failed(ConnectFailReason::AuthFailed)
        );
    }

    #[test]
    fn host_restart_detected_through_epoch_change() {
        let start = Instant::now();
        let mut m = LinkMachine::new(ResilienceConfig::fast(), start);
        let LinkAction::Hello { nonce } = m.begin_join(start)[0] else {
            panic!()
        };
        m.on_host_packet(start, &ack(nonce, 1));
        let now = start + Duration::from_millis(700);
        m.tick(now);
        assert!(matches!(
            m.state(),
            ClientConnectionState::Reconnecting { attempt: 1 }
        ));
        let LinkAction::Hello { nonce: n2 } = m.tick(now + Duration::from_millis(60))[0] else {
            panic!("expected re-hello on attempt 2")
        };
        let acts = m.on_host_packet(now + Duration::from_millis(70), &ack(n2, 2));
        assert!(acts.contains(&LinkAction::Rejoin {
            host_restarted: true
        }));
        assert_eq!(m.state(), ClientConnectionState::Connected);
    }
}
