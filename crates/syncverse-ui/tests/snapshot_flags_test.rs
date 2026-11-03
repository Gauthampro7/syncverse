//! Comprehensive test suite for `PlayerUiSnapshot` memory layout invariants,
//! bitflag definitions, helper methods, and triple-buffered atomic state exchange.
//!
//! Validates:
//! 1. Memory layout invariant: `std::mem::size_of::<PlayerUiSnapshot>() == 72` bytes.
//!    Layout fits comfortably inside a 128-byte cache line pair, eliminating torn reads.
//! 2. Exact bitflag values:
//!    - `SNAPSHOT_FLAG_MUTED == 0x1000` (Bit 12)
//!    - `SNAPSHOT_FLAG_LOOP_ENABLED == 0x2000` (Bit 13)
//! 3. Bitflag orthogonality and Hamming weight independence:
//!    - No overlap with existing status flags (`GATE_BLOCKED`, `FORCE_PLAY`, `LOCAL_READY`, `HAS_MISMATCH`).
//!    - No interference with playback state enum (bits 0..7).
//! 4. Comprehensive behavior of `is_muted()` and `is_loop_enabled()` helper methods:
//!    - Default state (all flags false).
//!    - Isolated flag assertions.
//!    - Exhaustive $2^6 = 64$ bitflag permutation matrix.
//!    - Preservation across all playback states (PAUSED, PLAYING, SEEKING, BUFFERING).
//! 5. Lock-free `TripleBufferSnapshotExchange` publication and polling with new flags.

#![deny(warnings)]

use std::mem::{align_of, size_of};

use syncverse_ui::snapshot::{
    PlayerUiSnapshot, TripleBufferSnapshotExchange, PLAYBACK_STATE_BUFFERING, PLAYBACK_STATE_PAUSED,
    PLAYBACK_STATE_PLAYING, PLAYBACK_STATE_SEEKING, SNAPSHOT_FLAG_FORCE_PLAY,
    SNAPSHOT_FLAG_GATE_BLOCKED, SNAPSHOT_FLAG_HAS_MISMATCH, SNAPSHOT_FLAG_LOCAL_READY,
    SNAPSHOT_FLAG_LOOP_ENABLED, SNAPSHOT_FLAG_MUTED,
};

#[test]
fn test_player_ui_snapshot_size_and_alignment_invariants() {
    let size = size_of::<PlayerUiSnapshot>();
    let align = align_of::<PlayerUiSnapshot>();

    // CRITICAL ARCHITECTURAL INVARIANT: PlayerUiSnapshot must remain exactly 72 bytes
    assert_eq!(
        size, 72,
        "PlayerUiSnapshot must remain exactly 72 bytes to fit inside a 128-byte cache-line pair"
    );
    assert!(
        size <= 128,
        "PlayerUiSnapshot must comfortably fit within a 128-byte cache-line pair"
    );
    assert_eq!(
        align, 8,
        "PlayerUiSnapshot 64-bit word alignment must be 8 bytes"
    );
}

#[test]
fn test_snapshot_bitflag_constants_values_and_orthogonality() {
    // 1. Verify exact constant values specified by architectural contract
    assert_eq!(
        SNAPSHOT_FLAG_MUTED, 0x1000,
        "SNAPSHOT_FLAG_MUTED must be 0x1000 (bit 12)"
    );
    assert_eq!(
        SNAPSHOT_FLAG_LOOP_ENABLED, 0x2000,
        "SNAPSHOT_FLAG_LOOP_ENABLED must be 0x2000 (bit 13)"
    );

    // 2. Each flag must have a Hamming weight of exactly 1 (single bit asserted)
    assert_eq!(SNAPSHOT_FLAG_MUTED.count_ones(), 1);
    assert_eq!(SNAPSHOT_FLAG_LOOP_ENABLED.count_ones(), 1);
    assert_eq!(SNAPSHOT_FLAG_GATE_BLOCKED.count_ones(), 1);
    assert_eq!(SNAPSHOT_FLAG_FORCE_PLAY.count_ones(), 1);
    assert_eq!(SNAPSHOT_FLAG_LOCAL_READY.count_ones(), 1);
    assert_eq!(SNAPSHOT_FLAG_HAS_MISMATCH.count_ones(), 1);

    // 3. Orthogonality test: pairwise bitwise AND must be strictly 0
    let all_flags = [
        SNAPSHOT_FLAG_GATE_BLOCKED,
        SNAPSHOT_FLAG_FORCE_PLAY,
        SNAPSHOT_FLAG_LOCAL_READY,
        SNAPSHOT_FLAG_HAS_MISMATCH,
        SNAPSHOT_FLAG_MUTED,
        SNAPSHOT_FLAG_LOOP_ENABLED,
    ];

    for i in 0..all_flags.len() {
        for j in (i + 1)..all_flags.len() {
            assert_eq!(
                all_flags[i] & all_flags[j],
                0,
                "Flags at index {} and {} collide: 0x{:04X} & 0x{:04X} != 0",
                i,
                j,
                all_flags[i],
                all_flags[j]
            );
        }
    }

    // 4. No flag may overlap with playback state bits 0..7 (mask 0x00FF)
    let playback_state_mask: u64 = 0x00FF;
    for &flag in &all_flags {
        assert_eq!(
            flag & playback_state_mask,
            0,
            "Flag 0x{:04X} collides with playback state bits 0..7",
            flag
        );
    }
}

#[test]
fn test_default_snapshot_helper_methods() {
    let snap = PlayerUiSnapshot::default();

    assert!(!snap.is_muted(), "Default snapshot must not be muted");
    assert!(
        !snap.is_loop_enabled(),
        "Default snapshot must not have loop enabled"
    );
    assert!(
        !snap.is_playing(),
        "Default snapshot must have paused playback state"
    );
    assert!(
        !snap.is_gate_blocked(),
        "Default snapshot must not be gate blocked"
    );
    assert!(
        !snap.is_force_play(),
        "Default snapshot must not have force play active"
    );
    assert!(
        !snap.is_local_ready(),
        "Default snapshot must not have local ready asserted"
    );
    assert!(
        !snap.has_media_mismatch(),
        "Default snapshot must not have media mismatch"
    );
}

#[test]
fn test_muted_and_loop_flags_isolation() {
    // 1. Assert only muted
    let mut snap = PlayerUiSnapshot {
        playback_state: SNAPSHOT_FLAG_MUTED,
        ..Default::default()
    };
    assert!(snap.is_muted());
    assert!(!snap.is_loop_enabled());
    assert!(!snap.is_playing());
    assert!(!snap.is_gate_blocked());

    // 2. Assert only loop enabled
    snap.playback_state = SNAPSHOT_FLAG_LOOP_ENABLED;
    assert!(!snap.is_muted());
    assert!(snap.is_loop_enabled());
    assert!(!snap.is_playing());
    assert!(!snap.is_gate_blocked());

    // 3. Assert both muted and loop enabled
    snap.playback_state = SNAPSHOT_FLAG_MUTED | SNAPSHOT_FLAG_LOOP_ENABLED;
    assert!(snap.is_muted());
    assert!(snap.is_loop_enabled());
    assert!(!snap.is_playing());

    // 4. Clear muted, keep loop enabled
    snap.playback_state &= !SNAPSHOT_FLAG_MUTED;
    assert!(!snap.is_muted());
    assert!(snap.is_loop_enabled());

    // 5. Clear loop enabled
    snap.playback_state &= !SNAPSHOT_FLAG_LOOP_ENABLED;
    assert!(!snap.is_muted());
    assert!(!snap.is_loop_enabled());
}

#[test]
fn test_flags_across_all_playback_states() {
    let states = [
        (PLAYBACK_STATE_PAUSED, false),
        (PLAYBACK_STATE_PLAYING, true),
        (PLAYBACK_STATE_SEEKING, false),
        (PLAYBACK_STATE_BUFFERING, false),
    ];

    for (state, is_playing_expected) in states {
        // State alone
        let mut snap = PlayerUiSnapshot {
            playback_state: state,
            ..Default::default()
        };
        assert_eq!(snap.is_playing(), is_playing_expected);
        assert!(!snap.is_muted());
        assert!(!snap.is_loop_enabled());

        // State + Muted
        snap.playback_state = state | SNAPSHOT_FLAG_MUTED;
        assert_eq!(snap.is_playing(), is_playing_expected);
        assert!(snap.is_muted());
        assert!(!snap.is_loop_enabled());

        // State + Loop Enabled
        snap.playback_state = state | SNAPSHOT_FLAG_LOOP_ENABLED;
        assert_eq!(snap.is_playing(), is_playing_expected);
        assert!(!snap.is_muted());
        assert!(snap.is_loop_enabled());

        // State + Both Flags
        snap.playback_state = state | SNAPSHOT_FLAG_MUTED | SNAPSHOT_FLAG_LOOP_ENABLED;
        assert_eq!(snap.is_playing(), is_playing_expected);
        assert!(snap.is_muted());
        assert!(snap.is_loop_enabled());
    }
}

#[test]
fn test_exhaustive_bitflag_permutations() {
    let flags = [
        SNAPSHOT_FLAG_GATE_BLOCKED,
        SNAPSHOT_FLAG_FORCE_PLAY,
        SNAPSHOT_FLAG_LOCAL_READY,
        SNAPSHOT_FLAG_HAS_MISMATCH,
        SNAPSHOT_FLAG_MUTED,
        SNAPSHOT_FLAG_LOOP_ENABLED,
    ];

    let num_combinations = 1 << flags.len(); // 2^6 = 64

    for base_state in [PLAYBACK_STATE_PAUSED, PLAYBACK_STATE_PLAYING] {
        for mask_idx in 0..num_combinations {
            let mut state = base_state;
            for (bit, &flag) in flags.iter().enumerate() {
                if (mask_idx & (1 << bit)) != 0 {
                    state |= flag;
                }
            }

            let snap = PlayerUiSnapshot {
                playback_state: state,
                ..Default::default()
            };

            // Verify bit 0 (GATE_BLOCKED)
            let expect_gate = (mask_idx & (1 << 0)) != 0;
            assert_eq!(snap.is_gate_blocked(), expect_gate);

            // Verify bit 1 (FORCE_PLAY)
            let expect_force = (mask_idx & (1 << 1)) != 0;
            assert_eq!(snap.is_force_play(), expect_force);

            // Verify bit 2 (LOCAL_READY)
            let expect_ready = (mask_idx & (1 << 2)) != 0;
            assert_eq!(snap.is_local_ready(), expect_ready);

            // Verify bit 3 (HAS_MISMATCH)
            let expect_mismatch = (mask_idx & (1 << 3)) != 0;
            assert_eq!(snap.has_media_mismatch(), expect_mismatch);

            // Verify bit 4 (MUTED)
            let expect_muted = (mask_idx & (1 << 4)) != 0;
            assert_eq!(snap.is_muted(), expect_muted);

            // Verify bit 5 (LOOP_ENABLED)
            let expect_loop = (mask_idx & (1 << 5)) != 0;
            assert_eq!(snap.is_loop_enabled(), expect_loop);

            // Verify playback state preservation
            assert_eq!(snap.is_playing(), base_state == PLAYBACK_STATE_PLAYING);
        }
    }
}

#[test]
fn test_bitwise_mutation_lifecycle() {
    let mut snap = PlayerUiSnapshot::default();

    // 1. Initial paused state
    assert!(!snap.is_playing());
    assert!(!snap.is_muted());
    assert!(!snap.is_loop_enabled());

    // 2. User presses 'M' to mute
    snap.playback_state |= SNAPSHOT_FLAG_MUTED;
    assert!(snap.is_muted());

    // 3. User begins playback (state becomes Playing)
    snap.playback_state = (snap.playback_state & !0xFF) | PLAYBACK_STATE_PLAYING;
    assert!(snap.is_playing());
    assert!(snap.is_muted());

    // 4. User toggles loop mode via 'L'
    snap.playback_state ^= SNAPSHOT_FLAG_LOOP_ENABLED;
    assert!(snap.is_loop_enabled());
    assert!(snap.is_playing());
    assert!(snap.is_muted());

    // 5. User un-mutes
    snap.playback_state &= !SNAPSHOT_FLAG_MUTED;
    assert!(!snap.is_muted());
    assert!(snap.is_playing());
    assert!(snap.is_loop_enabled());

    // 6. User toggles loop mode off
    snap.playback_state ^= SNAPSHOT_FLAG_LOOP_ENABLED;
    assert!(!snap.is_loop_enabled());
    assert!(snap.is_playing());
    assert!(!snap.is_muted());
}

#[test]
fn test_triple_buffer_exchange_propagation_with_flags() {
    let exchange = TripleBufferSnapshotExchange::new();

    // 1. Initial snapshot polled from empty exchange
    let init_snap = exchange.poll_latest_snapshot();
    assert!(!init_snap.is_muted());
    assert!(!init_snap.is_loop_enabled());

    // 2. Publish snapshot with Muted and Loop Enabled flags asserted
    let published = PlayerUiSnapshot {
        monotonic_timestamp: 42,
        media_position_pts: 1_000_000_000,
        media_duration_pts: 60_000_000_000,
        playback_state: PLAYBACK_STATE_PLAYING
            | SNAPSHOT_FLAG_MUTED
            | SNAPSHOT_FLAG_LOOP_ENABLED
            | SNAPSHOT_FLAG_LOCAL_READY,
        room_sequence: 42,
        ..Default::default()
    };
    exchange.publish(published);

    // 3. Poll latest snapshot and verify all flags propagate atomically
    let polled = exchange.poll_latest_snapshot();
    assert_eq!(polled.monotonic_timestamp, 42);
    assert!(polled.is_playing());
    assert!(polled.is_muted());
    assert!(polled.is_loop_enabled());
    assert!(polled.is_local_ready());
    assert!(!polled.is_gate_blocked());
    assert!(!polled.has_media_mismatch());
    assert!(polled.is_internally_consistent());
    assert_eq!(size_of::<PlayerUiSnapshot>(), 72);
}
