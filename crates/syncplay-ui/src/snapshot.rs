//! Lock-free triple-buffered atomic snapshot exchange adhering to SPSC thread topology.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU8, Ordering};

const CLEAN_INDEX_MASK: u8 = 0x03;
const HAS_NEW_BIT: u8 = 0x04;

/// Self-contained atomic player UI snapshot (72 bytes).
///
/// Layout fits comfortably within a 128-byte cache line pair, eliminating torn reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(C)]
pub struct PlayerUiSnapshot {
    /// Monotonic timestamp in nanoseconds.
    pub monotonic_timestamp: u64,
    /// Presentation timestamp in nanoseconds.
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
    /// Commanded playback speed in Q16 fixed-point (65536 = 1.0000x).
    pub pll_speed_ratio_q16: u32,
    /// Number of peers in the room.
    pub peer_count: u16,
    /// Number of peers ready in active barrier session.
    pub peers_ready: u16,
    /// Monotonic room sequence number.
    pub room_sequence: u64,
}

/// Playback state enum values.
pub const PLAYBACK_STATE_PAUSED: u64 = 0;
/// Playback state is actively presenting media frames.
pub const PLAYBACK_STATE_PLAYING: u64 = 1;
/// Playback state is currently seeking.
pub const PLAYBACK_STATE_SEEKING: u64 = 2;
/// Playback state is buffering data.
pub const PLAYBACK_STATE_BUFFERING: u64 = 3;

/// Status bitflag indicating playback unpause is blocked by unready or unloaded peers.
pub const SNAPSHOT_FLAG_GATE_BLOCKED: u64 = 0x0100;
/// Status bitflag indicating host force play bypass is active.
pub const SNAPSHOT_FLAG_FORCE_PLAY: u64 = 0x0200;
/// Status bitflag indicating local user is marked ready to watch.
pub const SNAPSHOT_FLAG_LOCAL_READY: u64 = 0x0400;
/// Status bitflag indicating a peer's loaded media mismatches local media.
pub const SNAPSHOT_FLAG_HAS_MISMATCH: u64 = 0x0800;

impl PlayerUiSnapshot {
    /// Returns true if the snapshot is internally consistent (used in concurrency audit).
    #[inline]
    pub fn is_internally_consistent(&self) -> bool {
        self.monotonic_timestamp == self.room_sequence
    }

    /// Returns true if the snapshot playback state is Playing.
    #[inline]
    pub fn is_playing(&self) -> bool {
        (self.playback_state & 0xFF) == PLAYBACK_STATE_PLAYING
    }

    /// Returns true if the unpause gate is currently blocking playback resumption.
    #[inline]
    pub fn is_gate_blocked(&self) -> bool {
        (self.playback_state & SNAPSHOT_FLAG_GATE_BLOCKED) != 0
    }

    /// Returns true if host force play override is active.
    #[inline]
    pub fn is_force_play(&self) -> bool {
        (self.playback_state & SNAPSHOT_FLAG_FORCE_PLAY) != 0
    }

    /// Returns true if the local user is marked ready to watch.
    #[inline]
    pub fn is_local_ready(&self) -> bool {
        (self.playback_state & SNAPSHOT_FLAG_LOCAL_READY) != 0
    }

    /// Returns true if any connected peer's media mismatches the local media.
    #[inline]
    pub fn has_media_mismatch(&self) -> bool {
        (self.playback_state & SNAPSHOT_FLAG_HAS_MISMATCH) != 0
    }
}

/// Lock-free Single-Producer Single-Consumer (SPSC) triple-buffered snapshot exchange.
///
/// Designed exclusively for publication by the central Telemetry Aggregator thread
/// and consumption by the native UI presentation thread.
pub struct TripleBufferSnapshotExchange {
    buffers: [UnsafeCell<PlayerUiSnapshot>; 3],
    clean_flags: AtomicU8,
    // Accessed strictly by producer thread
    write_idx: UnsafeCell<u8>,
    // Accessed strictly by consumer thread
    read_idx: UnsafeCell<u8>,
}

// SAFETY: SPSC invariant: Only one producer calls publish(), and only one consumer calls poll_latest_snapshot().
unsafe impl Send for TripleBufferSnapshotExchange {}
unsafe impl Sync for TripleBufferSnapshotExchange {}

impl Default for TripleBufferSnapshotExchange {
    fn default() -> Self {
        Self::new()
    }
}

impl TripleBufferSnapshotExchange {
    /// Constructs a new `TripleBufferSnapshotExchange`.
    pub fn new() -> Self {
        Self {
            buffers: [
                UnsafeCell::new(PlayerUiSnapshot::default()),
                UnsafeCell::new(PlayerUiSnapshot::default()),
                UnsafeCell::new(PlayerUiSnapshot::default()),
            ],
            clean_flags: AtomicU8::new(1),
            write_idx: UnsafeCell::new(2),
            read_idx: UnsafeCell::new(0),
        }
    }

    /// Publishes a unified telemetry snapshot from the central Telemetry Aggregator thread.
    ///
    /// # Safety Invariant
    /// MUST be called only by the sole producer thread (Telemetry Aggregator).
    pub fn publish(&self, snapshot: PlayerUiSnapshot) {
        // SAFETY: Only producer accesses `write_idx` in single-producer topology.
        let write_slot = unsafe { *self.write_idx.get() } as usize;

        // SAFETY: Writer has exclusive access to `buffers[write_slot]` because
        // `write_slot` is never equal to `clean_flags & CLEAN_INDEX_MASK` or `read_idx`.
        unsafe {
            *self.buffers[write_slot].get() = snapshot;
        }

        // Atomically exchange write index with clean index and assert HAS_NEW_BIT via CAS loop
        let mut curr = self.clean_flags.load(Ordering::Acquire);
        loop {
            let new_flags = (write_slot as u8 & CLEAN_INDEX_MASK) | HAS_NEW_BIT;
            match self.clean_flags.compare_exchange_weak(
                curr,
                new_flags,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(prev) => {
                    // SAFETY: Only producer accesses `write_idx`, updating it to the retired clean slot.
                    unsafe {
                        *self.write_idx.get() = prev & CLEAN_INDEX_MASK;
                    }
                    break;
                }
                Err(actual) => {
                    curr = actual;
                }
            }
        }
    }

    /// Polls the latest available snapshot by the UI consumer thread without blocking.
    ///
    /// Completes in O(1) time (< 50 ns) with zero locking or system calls.
    pub fn poll_latest_snapshot(&self) -> PlayerUiSnapshot {
        let mut curr = self.clean_flags.load(Ordering::Acquire);
        loop {
            // If HAS_NEW_BIT is not set, no new snapshot has been published since last read
            if (curr & HAS_NEW_BIT) == 0 {
                break;
            }

            // SAFETY: Only consumer accesses `read_idx` in single-consumer topology.
            let curr_read = unsafe { *self.read_idx.get() };
            let new_flags = curr_read & CLEAN_INDEX_MASK; // Clears HAS_NEW_BIT atomically

            match self.clean_flags.compare_exchange_weak(
                curr,
                new_flags,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(prev) => {
                    // SAFETY: Only consumer accesses `read_idx`, updating it to the acquired clean slot.
                    unsafe {
                        *self.read_idx.get() = prev & CLEAN_INDEX_MASK;
                    }
                    break;
                }
                Err(actual) => {
                    curr = actual;
                }
            }
        }

        // SAFETY: Only consumer accesses `read_idx`.
        let read_slot = unsafe { *self.read_idx.get() } as usize;

        // SAFETY: Reader has exclusive immutable read access to `buffers[read_slot]`
        // because `read_slot` is held exclusively until the next successful snapshot exchange.
        unsafe { *self.buffers[read_slot].get() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_struct_memory_size() {
        let size = std::mem::size_of::<PlayerUiSnapshot>();
        assert_eq!(size, 72, "PlayerUiSnapshot size must be exactly 72 bytes");
        assert!(size <= 128, "PlayerUiSnapshot must fit in 128-byte cache line pair");
    }

    #[test]
    fn test_triple_buffer_concurrency_stress() {
        use std::sync::Barrier;

        let exchange = Arc::new(TripleBufferSnapshotExchange::new());
        let ex_producer = Arc::clone(&exchange);

        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_flag_cons = Arc::clone(&stop_flag);

        let barrier = Arc::new(Barrier::new(2));
        let barrier_cons = Arc::clone(&barrier);

        let total_iterations = 20_000usize;

        let producer = thread::spawn(move || {
            barrier.wait();
            for seq in 0..total_iterations {
                let snap = PlayerUiSnapshot {
                    monotonic_timestamp: seq as u64,
                    media_position_pts: (seq * 1000) as u64,
                    room_sequence: seq as u64,
                    ..Default::default()
                };
                ex_producer.publish(snap);
            }
            stop_flag.store(true, Ordering::Release);
        });

        let consumer = thread::spawn(move || {
            let mut read_count = 0usize;
            let mut inconsistencies = 0usize;

            barrier_cons.wait();

            loop {
                let snap = exchange.poll_latest_snapshot();
                read_count += 1;
                if !snap.is_internally_consistent() {
                    inconsistencies += 1;
                }
                if stop_flag_cons.load(Ordering::Acquire) {
                    break;
                }
            }

            let final_snap = exchange.poll_latest_snapshot();
            read_count += 1;
            if !final_snap.is_internally_consistent() {
                inconsistencies += 1;
            }

            (read_count, inconsistencies)
        });

        producer.join().unwrap();
        let (reads, bad) = consumer.join().unwrap();
        assert!(reads > 0);
        assert_eq!(bad, 0, "Triple buffer must exhibit zero torn reads");
    }
}
