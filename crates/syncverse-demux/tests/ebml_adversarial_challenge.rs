//! Comprehensive Adversarial Empirical Challenge Suite for Matroska / EBML Demuxer.
//!
//! Directly exercises:
//! 1. Frame lacing unpacking: fuzzing, truncated buffers, VINT overflows, and signed delta edge cases.
//! 2. Cues-based seeking across boundary conditions: before first cue, after last cue, between cues, 0 cues, u64::MAX.
//! 3. Multi-track interleaved packet queue invariants: track isolation, FIFO queue order, keyframe propagation,
//!    and corruption resilience.

use std::io::Cursor;
use syncverse_demux::{
    build_synthetic_full_mkv, compute_simpleblock_pts, unpack_lacing, DemuxError,
    Demuxer, EbmlDemuxer, VintError, SYNTHETIC_EBML_VECTOR,
};

// ============================================================================
// 1. FRAME LACING ADVERSARIAL STRESS & FUZZING
// ============================================================================

#[test]
fn test_lacing_mode_0_no_lacing_boundaries() {
    // Normal payload
    let p = vec![1, 2, 3, 4, 5];
    let res = unpack_lacing(0, &p).unwrap();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0], vec![1, 2, 3, 4, 5]);

    // Empty payload yields single empty frame
    let empty = unpack_lacing(0, &[]).unwrap();
    assert_eq!(empty.len(), 1);
    assert!(empty[0].is_empty());
}

#[test]
fn test_lacing_mode_1_xiph_stress_and_boundaries() {
    // Empty payload rejected
    assert!(matches!(
        unpack_lacing(1, &[]),
        Err(DemuxError::CorruptedBlock(_))
    ));

    // Truncated header: specifies 3 frames (payload[0] = 2), but only 1 size byte provided
    assert!(matches!(
        unpack_lacing(1, &[2, 50]),
        Err(DemuxError::CorruptedBlock(_))
    ));

    // Truncated during 255-accumulation: ends before terminating < 255 byte
    assert!(matches!(
        unpack_lacing(1, &[1, 255, 255]),
        Err(DemuxError::CorruptedBlock(_))
    ));

    // Laced frame sizes exceed payload length
    // 2 frames (payload[0]=1), frame 0 size = 100, but total payload is only 10 bytes
    assert!(matches!(
        unpack_lacing(1, &[1, 100, 1, 2, 3, 4, 5, 6, 7, 8]),
        Err(DemuxError::CorruptedBlock(_))
    ));

    // Valid Xiph 3-frame lace with 255 accumulation
    // Frame 0: 255 + 10 = 265 bytes
    // Frame 1: 5 bytes
    // Frame 2: remaining 3 bytes
    let mut payload = vec![2]; // 3 frames
    payload.push(255);
    payload.push(10);
    payload.push(5);
    payload.extend(vec![0xAA; 265]);
    payload.extend(vec![0xBB; 5]);
    payload.extend(vec![0xCC; 3]);

    let frames = unpack_lacing(1, &payload).unwrap();
    assert_eq!(frames.len(), 3);
    assert_eq!(frames[0].len(), 265);
    assert_eq!(frames[1].len(), 5);
    assert_eq!(frames[2].len(), 3);
    assert_eq!(frames[0][0], 0xAA);
    assert_eq!(frames[1][0], 0xBB);
    assert_eq!(frames[2][0], 0xCC);
}

#[test]
fn test_lacing_mode_2_fixed_stress_and_boundaries() {
    // Empty payload rejected
    assert!(matches!(
        unpack_lacing(2, &[]),
        Err(DemuxError::CorruptedBlock(_))
    ));

    // Single frame count byte (payload[0] = 0 -> 1 frame), 0 remaining bytes -> 1 empty frame
    let res = unpack_lacing(2, &[0]).unwrap();
    assert_eq!(res.len(), 1);
    assert!(res[0].is_empty());

    // Indivisible remaining bytes: 4 frames (payload[0] = 3), 5 remaining bytes (5 % 4 != 0)
    assert!(matches!(
        unpack_lacing(2, &[3, 1, 2, 3, 4, 5]),
        Err(DemuxError::CorruptedBlock(_))
    ));

    // Perfectly divisible: 4 frames (payload[0] = 3), 8 remaining bytes -> 2 bytes per frame
    let frames = unpack_lacing(2, &[3, 1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
    assert_eq!(frames.len(), 4);
    assert_eq!(frames[0], vec![1, 2]);
    assert_eq!(frames[1], vec![3, 4]);
    assert_eq!(frames[2], vec![5, 6]);
    assert_eq!(frames[3], vec![7, 8]);
}

#[test]
fn test_lacing_mode_3_ebml_defect_reserved_element_id_rejection() {
    // Verified remediation:
    // In EBML lacing, signed delta VINTs use parse_vint(..., false) so that
    // deltas with all data bits set to 1 (e.g. 0xFF for +64 delta, 0x7FFF for +8192)
    // are cleanly decoded rather than falsely rejected as ReservedElementId.
    let mut payload = vec![2, 0x8A, 0xFF];
    payload.extend(vec![0xAA; 10]); // Frame 0: 10 bytes
    payload.extend(vec![0xBB; 74]); // Frame 1: 10 + 64 = 74 bytes
    payload.extend(vec![0xCC; 5]);  // Frame 2: 5 bytes

    let res = unpack_lacing(3, &payload);
    assert!(
        res.is_ok(),
        "Signed delta 0xFF must be accepted and decoded cleanly: {:?}",
        res.err()
    );
    let frames = res.unwrap();
    assert_eq!(frames.len(), 3);
    assert_eq!(frames[0].len(), 10);
    assert_eq!(frames[1].len(), 74);
    assert_eq!(frames[2].len(), 5);
    assert_eq!(frames[0][0], 0xAA);
    assert_eq!(frames[1][0], 0xBB);
    assert_eq!(frames[2][0], 0xCC);

    // Also test 2-octet signed delta with all data bits set to 1 (0x7FFF -> +8192)
    let mut payload2 = vec![2, 0x8A, 0x7F, 0xFF];
    payload2.extend(vec![0x11; 10]);   // Frame 0: 10 bytes
    payload2.extend(vec![0x22; 8202]); // Frame 1: 10 + 8192 = 8202 bytes
    payload2.extend(vec![0x33; 4]);    // Frame 2: 4 bytes
    let frames2 = unpack_lacing(3, &payload2).unwrap();
    assert_eq!(frames2.len(), 3);
    assert_eq!(frames2[0].len(), 10);
    assert_eq!(frames2[1].len(), 8202);
    assert_eq!(frames2[2].len(), 4);
}

#[test]
fn test_lacing_mode_3_ebml_defect_frame_count_one_generates_two_frames() {
    // Verified remediation:
    // When payload[0] == 0 (frame_count = 1), EBML lacing emits exactly 1 frame
    // without appending a phantom trailing empty frame.
    let payload = vec![0, 0x85, 1, 2, 3, 4, 5];
    let res = unpack_lacing(3, &payload).unwrap();
    assert_eq!(
        res.len(),
        1,
        "EBML lacing with frame_count=1 must output exactly 1 frame"
    );
    assert_eq!(res[0], vec![0x85, 1, 2, 3, 4, 5]);

    // Test with plain frame payload
    let payload2 = vec![0, 1, 2, 3, 4, 5];
    let res2 = unpack_lacing(3, &payload2).unwrap();
    assert_eq!(res2.len(), 1);
    assert_eq!(res2[0], vec![1, 2, 3, 4, 5]);

    // Single byte (0 remaining payload bytes) -> 1 empty frame
    let payload3 = vec![0];
    let res3 = unpack_lacing(3, &payload3).unwrap();
    assert_eq!(res3.len(), 1);
    assert!(res3[0].is_empty());
}

#[test]
fn test_lacing_mode_3_ebml_delta_negative_size_rejection() {
    // Frame 0 size = 5 bytes (0x85)
    // Frame 1 delta: -10 bytes
    // In 1 octet (w=1): val - 63 = -10 => val = 53 (0x35)
    // raw_vint with marker bit 0x80 = 0x80 | 0x35 = 0xB5
    // new_size = 5 + (-10) = -5 -> rejected as negative frame size!
    let payload = vec![2, 0x85, 0xB5, 1, 2, 3];
    let res = unpack_lacing(3, &payload);
    assert!(
        matches!(res, Err(DemuxError::CorruptedBlock(ref msg)) if msg.contains("negative EBML laced frame size")),
        "Expected negative frame size rejection, got: {:?}",
        res
    );
}

#[test]
fn test_lacing_mode_3_ebml_multi_octet_deltas() {
    // 2-octet signed delta:
    // Frame 0: 100 bytes (2-octet VINT: 0x40, 0x64)
    // Frame 1 delta: +50 bytes
    // For w=2: bias = (1 << 13) - 1 = 8191.
    // val = 8191 + 50 = 8241 = 0x2031.
    // raw_vint with marker bit at bit 14 (0x4000) = 0x4000 | 0x2031 = 0x6031.
    // First byte = 0x60, second byte = 0x31.
    let mut payload = vec![2, 0x40, 0x64, 0x60, 0x31];
    payload.extend(vec![0x11; 100]); // Frame 0: 100 bytes
    payload.extend(vec![0x22; 150]); // Frame 1: 100 + 50 = 150 bytes
    payload.extend(vec![0x33; 20]);  // Frame 2: 20 bytes

    let frames = unpack_lacing(3, &payload).unwrap();
    assert_eq!(frames.len(), 3);
    assert_eq!(frames[0].len(), 100);
    assert_eq!(frames[1].len(), 150);
    assert_eq!(frames[2].len(), 20);
    assert_eq!(frames[0][0], 0x11);
    assert_eq!(frames[1][0], 0x22);
    assert_eq!(frames[2][0], 0x33);
}

#[test]
fn test_lacing_mode_3_ebml_truncated_deltas() {
    // Truncated delta header: specifies 3 frames, but truncated before delta VINT
    assert!(matches!(
        unpack_lacing(3, &[2, 0x85]),
        Err(DemuxError::CorruptedBlock(_))
    ));

    // Zero-prefix byte in delta VINT
    assert!(matches!(
        unpack_lacing(3, &[2, 0x85, 0x00]),
        Err(DemuxError::Vint(VintError::InvalidZeroPrefix))
    ));

    // Truncated multi-byte delta: first byte indicates 2-byte delta (0x40), but no 2nd byte
    assert!(matches!(
        unpack_lacing(3, &[2, 0x85, 0x40]),
        Err(DemuxError::UnexpectedEof { .. })
    ));
}

#[test]
fn test_lacing_fuzzing_no_panics() {
    // Deterministic pseudo-random fuzz generator
    let mut state: u32 = 0xDEADBEEF;
    let mut rng = || -> u8 {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        (state >> 24) as u8
    };

    for mode in 0..=4 {
        for len in 0..128 {
            let mut buf = vec![0u8; len];
            for b in &mut buf {
                *b = rng();
            }
            // Ensure unpack_lacing NEVER panics, only returns Ok or Err
            let _ = unpack_lacing(mode, &buf);
        }
    }
}

// ============================================================================
// 2. CUES-BASED SEEKING ACROSS BOUNDARY CONDITIONS
// ============================================================================

#[test]
fn test_seeking_all_boundaries_full_mkv() {
    let mkv = build_synthetic_full_mkv();
    let mut demuxer = EbmlDemuxer::new();
    let info = demuxer.init(Box::new(Cursor::new(mkv))).unwrap();
    assert_eq!(info.tracks.len(), 2);
    assert_eq!(demuxer.cues().len(), 2);
    assert_eq!(demuxer.cues()[0].pts_ms, 0);
    assert_eq!(demuxer.cues()[1].pts_ms, 500);

    // Boundary 1: Seek to 0ms (first cue)
    let pts = demuxer.seek_to(0).unwrap();
    assert_eq!(pts, 0);
    let p = demuxer.read_packet().unwrap().expect("p0");
    assert_eq!(p.pts_ms, 0);
    assert_eq!(p.track_id, 1);

    // Boundary 2: Seek between 0ms and 500ms (e.g. 1ms, 250ms, 499ms) -> all land on 0ms
    for target in [1, 100, 250, 499] {
        let pts = demuxer.seek_to(target).unwrap();
        assert_eq!(pts, 0, "Target {} must land on preceding keyframe at 0ms", target);
        let p = demuxer.read_packet().unwrap().expect("packet");
        assert_eq!(p.pts_ms, 0);
    }

    // Boundary 3: Seek exactly on 500ms (second cue)
    let pts = demuxer.seek_to(500).unwrap();
    assert_eq!(pts, 500);
    let p = demuxer.read_packet().unwrap().expect("p500");
    assert_eq!(p.pts_ms, 500);
    assert_eq!(p.track_id, 1);

    // Boundary 4: Seek past last cue (501ms, 1000ms, 60_000ms, u64::MAX) -> all land on 500ms
    for target in [501, 1000, 10_000, u64::MAX] {
        let pts = demuxer.seek_to(target).unwrap();
        assert_eq!(pts, 500, "Target {} must land on last keyframe at 500ms", target);
        let p = demuxer.read_packet().unwrap().expect("packet");
        assert_eq!(p.pts_ms, 500);
    }
}

#[test]
fn test_seeking_zero_cues_fallback() {
    let mut demuxer = EbmlDemuxer::new();
    let _info = demuxer.init(Box::new(Cursor::new(SYNTHETIC_EBML_VECTOR.to_vec()))).unwrap();
    assert!(demuxer.cues().is_empty());

    // Seeking with 0 cues should always land on 0ms
    for target in [0, 100, 500, 10_000, u64::MAX] {
        let pts = demuxer.seek_to(target).unwrap();
        assert_eq!(pts, 0, "With 0 cues, seek({}) must land on 0ms", target);
    }
}

#[test]
fn test_rapid_alternating_seeks() {
    let mkv = build_synthetic_full_mkv();
    let mut demuxer = EbmlDemuxer::new();
    demuxer.init(Box::new(Cursor::new(mkv))).unwrap();

    for _ in 0..20 {
        let p500 = demuxer.seek_to(500).unwrap();
        assert_eq!(p500, 500);
        let pkt = demuxer.read_packet().unwrap().unwrap();
        assert_eq!(pkt.pts_ms, 500);

        let p0 = demuxer.seek_to(0).unwrap();
        assert_eq!(p0, 0);
        let pkt = demuxer.read_packet().unwrap().unwrap();
        assert_eq!(pkt.pts_ms, 0);
    }
}

// ============================================================================
// 3. MULTI-TRACK INTERLEAVED PACKET QUEUE INVARIANTS
// ============================================================================

#[test]
fn test_multi_track_sequential_interleaving_invariants() {
    let mkv = build_synthetic_full_mkv();
    let mut demuxer = EbmlDemuxer::new();
    demuxer.init(Box::new(Cursor::new(mkv))).unwrap();

    // The synthetic MKV contains:
    // Cluster 1 (0ms):
    //   Track 1 (video): PTS 0ms, keyframe=true
    //   Track 2 (audio): PTS 0ms, keyframe=true
    // Cluster 2 (500ms):
    //   Track 1 (video): PTS 500ms, keyframe=true
    //   Track 2 (audio): PTS 500ms, keyframe=true

    let p1 = demuxer.read_packet().unwrap().expect("p1");
    assert_eq!(p1.track_id, 1);
    assert_eq!(p1.pts_ms, 0);
    assert!(p1.is_keyframe);
    assert_eq!(p1.data, vec![0x01, 0x02, 0x03, 0x04]);

    let p2 = demuxer.read_packet().unwrap().expect("p2");
    assert_eq!(p2.track_id, 2);
    assert_eq!(p2.pts_ms, 0);
    assert!(p2.is_keyframe);
    assert_eq!(p2.data, vec![0xAA, 0xBB, 0xCC]);

    let p3 = demuxer.read_packet().unwrap().expect("p3");
    assert_eq!(p3.track_id, 1);
    assert_eq!(p3.pts_ms, 500);
    assert!(p3.is_keyframe);
    assert_eq!(p3.data, vec![0x05, 0x06, 0x07, 0x08]);

    let p4 = demuxer.read_packet().unwrap().expect("p4");
    assert_eq!(p4.track_id, 2);
    assert_eq!(p4.pts_ms, 500);
    assert!(p4.is_keyframe);
    assert_eq!(p4.data, vec![0xDD, 0xEE, 0xFF]);

    let p5 = demuxer.read_packet().unwrap();
    assert!(p5.is_none(), "Must reach clean EOF when all packets are read");
}

#[test]
fn test_packet_queue_flushing_on_seek() {
    let mkv = build_synthetic_full_mkv();
    let mut demuxer = EbmlDemuxer::new();
    demuxer.init(Box::new(Cursor::new(mkv))).unwrap();

    // Read only packet 1 (track 1, 0ms)
    let p1 = demuxer.read_packet().unwrap().expect("p1");
    assert_eq!(p1.track_id, 1);

    // Seek to 500ms before reading packet 2
    let landed = demuxer.seek_to(500).unwrap();
    assert_eq!(landed, 500);

    // After seek, the next packet MUST be from 500ms (packet 2 at 0ms must NOT be emitted!)
    let p_after = demuxer.read_packet().unwrap().expect("p_after");
    assert_eq!(p_after.track_id, 1);
    assert_eq!(p_after.pts_ms, 500);
}

#[test]
fn test_simpleblock_pts_timescale_invariants() {
    // 1. Standard timescale (1ms)
    let (ns, ms) = compute_simpleblock_pts(5000, -250, 1_000_000).unwrap();
    assert_eq!(ns, 4_750_000_000);
    assert_eq!(ms, 4750);

    // 2. Division with euclid floor for negative PTS
    let (_ns, ms) = compute_simpleblock_pts(-10, 0, 1_000_000).unwrap();
    assert_eq!(ms, -10);

    // 3. Overflow protection on max values
    assert_eq!(
        compute_simpleblock_pts(i64::MAX, 1, 1_000_000),
        Err(DemuxError::IntegerOverflow)
    );
    assert_eq!(
        compute_simpleblock_pts(1, 0, u64::MAX),
        Err(DemuxError::IntegerOverflow)
    );
}
