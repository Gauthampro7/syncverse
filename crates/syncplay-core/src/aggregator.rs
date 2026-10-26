//! Centralized Telemetry Aggregator compiling subsystem metrics and publishing UI snapshots.

use syncplay_ui::{PlayerUiSnapshot, TripleBufferSnapshotExchange};

/// Subsystem metrics collected across decoding, networking, and presentation threads.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SubsystemMetrics {
    /// Monotonic timestamp in nanoseconds.
    pub monotonic_timestamp: u64,
    /// Current media presentation position in nanoseconds.
    pub media_position_pts: u64,
    /// Total duration in nanoseconds.
    pub media_duration_pts: u64,
    /// Playback state (0: Paused, 1: Playing, 2: Seeking, 3: Buffering).
    pub playback_state: u64,
    /// Filtered round-trip time in nanoseconds.
    pub network_rtt_nanos: u64,
    /// Filtered clock offset theta in nanoseconds.
    pub clock_offset_nanos: i64,
    /// PLL phase error e(t) in nanoseconds.
    pub phase_error_nanos: i64,
    /// Commanded playback speed ratio (e.g. 1.0).
    pub speed_ratio: f64,
    /// Number of peers in the room.
    pub peer_count: u16,
    /// Number of peers ready in active barrier session.
    pub peers_ready: u16,
    /// Monotonic room sequence number.
    pub room_sequence: u64,
}

/// Centralized Telemetry Aggregator publishing unified snapshots to `syncplay-ui`.
pub struct TelemetryAggregator<'a> {
    exchange: &'a TripleBufferSnapshotExchange,
}

impl<'a> TelemetryAggregator<'a> {
    /// Constructs a `TelemetryAggregator` bound to the given triple-buffer snapshot exchange.
    pub fn new(exchange: &'a TripleBufferSnapshotExchange) -> Self {
        Self { exchange }
    }

    /// Compiles subsystem metrics and publishes a unified 72-byte `PlayerUiSnapshot`.
    pub fn publish_snapshot(&self, metrics: &SubsystemMetrics) {
        let pll_speed_ratio_q16 = (metrics.speed_ratio * 65536.0).clamp(0.0, u32::MAX as f64) as u32;

        let snapshot = PlayerUiSnapshot {
            monotonic_timestamp: metrics.monotonic_timestamp,
            media_position_pts: metrics.media_position_pts,
            media_duration_pts: metrics.media_duration_pts,
            playback_state: metrics.playback_state,
            network_rtt_nanos: metrics.network_rtt_nanos,
            clock_offset_nanos: metrics.clock_offset_nanos,
            phase_error_nanos: metrics.phase_error_nanos,
            pll_speed_ratio_q16,
            peer_count: metrics.peer_count,
            peers_ready: metrics.peers_ready,
            room_sequence: metrics.room_sequence,
        };

        self.exchange.publish(snapshot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telemetry_aggregator_publish_and_poll() {
        let exchange = TripleBufferSnapshotExchange::new();
        let aggregator = TelemetryAggregator::new(&exchange);

        let metrics = SubsystemMetrics {
            monotonic_timestamp: 1000,
            media_position_pts: 25000 * 1_000_000,
            media_duration_pts: 120000 * 1_000_000,
            playback_state: 1, // Playing
            network_rtt_nanos: 250_000_000,
            clock_offset_nanos: 50_000_000,
            phase_error_nanos: 12_000_000,
            speed_ratio: 1.025,
            peer_count: 4,
            peers_ready: 4,
            room_sequence: 1000,
        };

        aggregator.publish_snapshot(&metrics);
        let snap = exchange.poll_latest_snapshot();

        assert_eq!(snap.monotonic_timestamp, 1000);
        assert_eq!(snap.media_position_pts, 25000 * 1_000_000);
        assert_eq!(snap.playback_state, 1);
        assert_eq!(snap.peer_count, 4);
        assert_eq!(snap.room_sequence, 1000);
        assert!(snap.is_internally_consistent());
    }
}
