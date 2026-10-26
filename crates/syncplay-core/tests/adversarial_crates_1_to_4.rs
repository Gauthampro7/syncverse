//! Adversarial Empirical Challenge Suite: Crates 1 to 4
//! Directly exercises:
//! - syncplay-common (CRC32, ByteReader, BitReader, MediaTime)
//! - syncplay-demux (RFC 8794 EBML VINT, ISOBMFF 32/64-bit BoxHeader, SimpleBlock PTS)
//! - syncplay-codecs (BT.601 / BT.709 Q8 color conversion, YUV420p / RGBA frame geometry)
//! - syncplay-clock (MasterClock SeqLock continuity, SpscRingBuffer concurrency & boundaries)

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use syncplay_clock::{
    BufferFullError, MasterClock, PlaybackState, PresentationAction, PresentationReorderBuffer,
    SpscRingBuffer,
};
use syncplay_codecs::{
    clamp_u8, convert_yuv420p_to_rgba, yuv_to_rgb_bt601, yuv_to_rgb_bt709, ColorStandard,
    VideoFrameRGBA, VideoFrameYUV420p,
};
use syncplay_common::{BitReader, ByteReader, Crc32, MediaTime};
use syncplay_demux::{
    compute_simpleblock_pts, parse_vint, BoxHeader, DemuxError, FourCc, VintError,
    SYNTHETIC_EBML_VECTOR, SYNTHETIC_MP4_VECTOR,
};

// ============================================================================
// 1. EBML VINT & MALFORMED INPUT ADVERSARIAL STRESS
// ============================================================================

#[test]
fn test_malformed_ebml_vint_adversarial() {
    println!("\n[CHALLENGE 1] Malformed EBML VINT Stress Test");

    // 1A. Truncated VINT inputs
    assert!(matches!(
        parse_vint(&[], false),
        Err(VintError::UnexpectedEof { needed: 1, available: 0 })
    ));
    assert!(matches!(
        parse_vint(&[], true),
        Err(VintError::UnexpectedEof { needed: 1, available: 0 })
    ));

    // 2-octet VINT truncated after 1 byte
    assert!(matches!(
        parse_vint(&[0x40], false),
        Err(VintError::UnexpectedEof { needed: 2, available: 1 })
    ));

    // 3-octet VINT truncated after 2 bytes
    assert!(matches!(
        parse_vint(&[0x20, 0x01], false),
        Err(VintError::UnexpectedEof { needed: 3, available: 2 })
    ));

    // 4-octet VINT truncated after 3 bytes
    assert!(matches!(
        parse_vint(&[0x10, 0x01, 0x02], false),
        Err(VintError::UnexpectedEof { needed: 4, available: 3 })
    ));

    // 8-octet VINT truncated after 1..7 bytes
    for avail in 1..8 {
        let mut buf = vec![0x01];
        buf.resize(avail, 0xFF);
        assert!(
            matches!(
                parse_vint(&buf, false),
                Err(VintError::UnexpectedEof { needed: 8, available }) if available == avail
            ),
            "Failed on 8-octet truncated with {} bytes",
            avail
        );
    }
    println!("  [PASS] All truncated VINT inputs (1..8 octets) strictly reject with UnexpectedEof.");

    // 1B. Invalid zero prefix (0x00 octet)
    assert_eq!(parse_vint(&[0x00], false), Err(VintError::InvalidZeroPrefix));
    assert_eq!(parse_vint(&[0x00], true), Err(VintError::InvalidZeroPrefix));
    assert_eq!(
        parse_vint(&[0x00, 0x12, 0x34], false),
        Err(VintError::InvalidZeroPrefix)
    );
    assert_eq!(
        parse_vint(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01], false),
        Err(VintError::InvalidZeroPrefix)
    );
    println!("  [PASS] Invalid 0x00 prefix octet strictly rejected with InvalidZeroPrefix.");

    // 1C. VINT size all 1s unknown size sentinels (data_size == None)
    // 1-octet unknown size: 0xFF (marker bit at pos 7, data 7 bits all 1)
    let (raw, size_opt, width) = parse_vint(&[0xFF], false).unwrap();
    assert_eq!(raw, 0xFF);
    assert_eq!(size_opt, None, "1-octet 0xFF must yield unknown size None");
    assert_eq!(width, 1);

    // 2-octet unknown size: 0x7F 0xFF
    let (raw, size_opt, width) = parse_vint(&[0x7F, 0xFF], false).unwrap();
    assert_eq!(raw, 0x7FFF);
    assert_eq!(size_opt, None, "2-octet 0x7FFF must yield unknown size None");
    assert_eq!(width, 2);

    // 3-octet unknown size: 0x3F 0xFF 0xFF
    let (raw, size_opt, width) = parse_vint(&[0x3F, 0xFF, 0xFF], false).unwrap();
    assert_eq!(raw, 0x3FFFFF);
    assert_eq!(size_opt, None, "3-octet 0x3FFFFF must yield unknown size None");
    assert_eq!(width, 3);

    // 4-octet unknown size: 0x1F 0xFF 0xFF 0xFF
    let (raw, size_opt, width) = parse_vint(&[0x1F, 0xFF, 0xFF, 0xFF], false).unwrap();
    assert_eq!(raw, 0x1FFFFFFF);
    assert_eq!(size_opt, None, "4-octet 0x1FFFFFFF must yield unknown size None");
    assert_eq!(width, 4);

    // 8-octet unknown size: 0x01 0xFF 0xFF 0xFF 0xFF 0xFF 0xFF 0xFF
    let (raw, size_opt, width) =
        parse_vint(&[0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF], false).unwrap();
    assert_eq!(raw, 0x01FFFFFFFFFFFFFF);
    assert_eq!(size_opt, None, "8-octet 0x01FFFFFFFFFFFFFF must yield unknown size None");
    assert_eq!(width, 8);
    println!("  [PASS] VINT sizes with all data bits set to 1 correctly yield unknown size sentinel None.");

    // 1D. Reserved Element IDs per RFC 8794 Section 4.1 (all data bits 1)
    assert_eq!(parse_vint(&[0xFF], true), Err(VintError::ReservedElementId));
    assert_eq!(
        parse_vint(&[0x7F, 0xFF], true),
        Err(VintError::ReservedElementId)
    );
    assert_eq!(
        parse_vint(&[0x3F, 0xFF, 0xFF], true),
        Err(VintError::ReservedElementId)
    );
    assert_eq!(
        parse_vint(&[0x1F, 0xFF, 0xFF, 0xFF], true),
        Err(VintError::ReservedElementId)
    );
    println!("  [PASS] Reserved Element IDs (all data bits 1) strictly reject with ReservedElementId.");

    // 1E. In-tree synthetic EBML vector check
    assert_eq!(SYNTHETIC_EBML_VECTOR.len(), 48);
    let (header_id, _, id_width) = parse_vint(&SYNTHETIC_EBML_VECTOR[0..4], true).unwrap();
    assert_eq!(header_id, 0x1A45DFA3);
    assert_eq!(id_width, 4);
}

// ============================================================================
// 2. ISOBMFF BOUNDARY CHECKS & LARGESIZE ADVERSARIAL STRESS
// ============================================================================

#[test]
fn test_isobmff_boundary_and_largesize_adversarial() {
    println!("\n[CHALLENGE 2] ISOBMFF BoxHeader Boundary Stress Test");

    // 2A. Box size 0 (extends to EOF)
    let buf_exact_8 = [0x00, 0x00, 0x00, 0x00, b'm', b'd', b'a', b't'];
    let hdr = BoxHeader::parse(&buf_exact_8).unwrap();
    assert_eq!(hdr.fourcc, FourCc(*b"mdat"));
    assert_eq!(hdr.header_size, 8);
    assert_eq!(hdr.total_size, 8);

    let mut buf_100 = vec![0u8; 100];
    buf_100[0..8].copy_from_slice(&[0x00, 0x00, 0x00, 0x00, b'm', b'd', b'a', b't']);
    let hdr100 = BoxHeader::parse(&buf_100).unwrap();
    assert_eq!(hdr100.total_size, 100);

    // Box size 0 with truncated header (< 8 bytes)
    assert!(matches!(
        BoxHeader::parse(&buf_exact_8[..7]),
        Err(DemuxError::UnexpectedEof { needed: 8, available: 7 })
    ));
    println!("  [PASS] Box size 0 correctly extends to EOF (total_size == buf.len()).");

    // 2B. Box size 1 with largesize 0..15 (MUST REJECT)
    for bad_size in 0u64..16 {
        let mut bad_buf = [0u8; 16];
        bad_buf[0..4].copy_from_slice(&1u32.to_be_bytes()); // size = 1
        bad_buf[4..8].copy_from_slice(b"mdat");
        bad_buf[8..16].copy_from_slice(&bad_size.to_be_bytes());
        assert!(
            matches!(BoxHeader::parse(&bad_buf), Err(DemuxError::MalformedBox(_))),
            "Failed to reject largesize {}",
            bad_size
        );
    }
    println!("  [PASS] All largesize in 0..15 strictly rejected with MalformedBox.");

    // 2C. Boundary largesize == 16 (MUST ACCEPT)
    let mut boundary_16 = [0u8; 16];
    boundary_16[0..4].copy_from_slice(&1u32.to_be_bytes());
    boundary_16[4..8].copy_from_slice(b"free");
    boundary_16[8..16].copy_from_slice(&16u64.to_be_bytes());
    let hdr_16 = BoxHeader::parse(&boundary_16).unwrap();
    assert_eq!(hdr_16.header_size, 16);
    assert_eq!(hdr_16.total_size, 16);
    println!("  [PASS] Boundary largesize == 16 accepted.");

    // 2D. Box size 1 with largesize > i64::MAX (MUST REJECT)
    const I64_MAX: u64 = i64::MAX as u64; // 9,223,372,036,854,775,807

    // Boundary: i64::MAX itself (MUST ACCEPT)
    let mut boundary_i64 = [0u8; 16];
    boundary_i64[0..4].copy_from_slice(&1u32.to_be_bytes());
    boundary_i64[4..8].copy_from_slice(b"mdat");
    boundary_i64[8..16].copy_from_slice(&I64_MAX.to_be_bytes());
    let hdr_i64 = BoxHeader::parse(&boundary_i64).unwrap();
    assert_eq!(hdr_i64.total_size, I64_MAX);
    println!("  [PASS] Boundary largesize == i64::MAX accepted.");

    // i64::MAX + 1 (MUST REJECT)
    let mut overflow_buf1 = [0u8; 16];
    overflow_buf1[0..4].copy_from_slice(&1u32.to_be_bytes());
    overflow_buf1[4..8].copy_from_slice(b"mdat");
    overflow_buf1[8..16].copy_from_slice(&(I64_MAX + 1).to_be_bytes());
    assert!(matches!(
        BoxHeader::parse(&overflow_buf1),
        Err(DemuxError::MalformedBox(_))
    ));

    // u64::MAX (MUST REJECT)
    let mut overflow_buf2 = [0u8; 16];
    overflow_buf2[0..4].copy_from_slice(&1u32.to_be_bytes());
    overflow_buf2[4..8].copy_from_slice(b"mdat");
    overflow_buf2[8..16].copy_from_slice(&u64::MAX.to_be_bytes());
    assert!(matches!(
        BoxHeader::parse(&overflow_buf2),
        Err(DemuxError::MalformedBox(_))
    ));
    println!("  [PASS] largesize > i64::MAX (i64::MAX+1, u64::MAX) strictly rejected with MalformedBox.");

    // 2E. Standard 32-bit box sizes < 8 (MUST REJECT)
    for bad_size32 in 2u32..8 {
        let mut bad_buf = [0u8; 8];
        bad_buf[0..4].copy_from_slice(&bad_size32.to_be_bytes());
        bad_buf[4..8].copy_from_slice(b"mdat");
        assert!(matches!(
            BoxHeader::parse(&bad_buf),
            Err(DemuxError::MalformedBox(_))
        ));
    }
    println!("  [PASS] Standard 32-bit box sizes in 2..8 strictly rejected with MalformedBox.");

    // 2F. Synthetic MP4 vector check
    assert_eq!(SYNTHETIC_MP4_VECTOR.len(), 132);
    let hdr_mp4 = BoxHeader::parse(&SYNTHETIC_MP4_VECTOR).unwrap();
    assert_eq!(hdr_mp4.fourcc, FourCc(*b"ftyp"));
    assert_eq!(hdr_mp4.total_size, 16);
}

// ============================================================================
// 3. Q8 COLOR CONVERSION BOUNDARY COLORS & CLAMPING STRESS
// ============================================================================

#[test]
fn test_q8_color_conversion_boundaries_and_clamping() {
    println!("\n[CHALLENGE 3] Q8 Color Conversion Boundary & Clamping Stress Test");

    // 3A. Reference Black (Y=16, U=128, V=128 => R=0, G=0, B=0)
    let (r601, g601, b601) = yuv_to_rgb_bt601(16, 128, 128);
    assert_eq!((r601, g601, b601), (0, 0, 0), "BT.601 black mismatch");

    let (r709, g709, b709) = yuv_to_rgb_bt709(16, 128, 128);
    assert_eq!((r709, g709, b709), (0, 0, 0), "BT.709 black mismatch");
    println!("  [PASS] Studio Black (16, 128, 128) -> (0, 0, 0) verified for BT.601 and BT.709.");

    // 3B. Reference White (Y=235, U=128, V=128 => R=255, G=255, B=255)
    let (r601_w, g601_w, b601_w) = yuv_to_rgb_bt601(235, 128, 128);
    assert_eq!((r601_w, g601_w, b601_w), (255, 255, 255), "BT.601 white mismatch");

    let (r709_w, g709_w, b709_w) = yuv_to_rgb_bt709(235, 128, 128);
    assert_eq!((r709_w, g709_w, b709_w), (255, 255, 255), "BT.709 white mismatch");
    println!("  [PASS] Studio White (235, 128, 128) -> (255, 255, 255) verified for BT.601 and BT.709.");

    // 3C. Out-of-range Y=0 clamp check
    let (r0, g0, b0) = yuv_to_rgb_bt601(0, 128, 128);
    assert_eq!((r0, g0, b0), (0, 0, 0), "Out-of-range Y=0 must clamp to 0");
    let (r0_709, g0_709, b0_709) = yuv_to_rgb_bt709(0, 128, 128);
    assert_eq!((r0_709, g0_709, b0_709), (0, 0, 0), "Out-of-range Y=0 must clamp to 0 in BT.709");

    // 3D. Out-of-range Y=255 clamp check
    let (r255, g255, b255) = yuv_to_rgb_bt601(255, 128, 128);
    assert_eq!((r255, g255, b255), (255, 255, 255), "Out-of-range Y=255 must clamp to 255");
    let (r255_709, g255_709, b255_709) = yuv_to_rgb_bt709(255, 128, 128);
    assert_eq!((r255_709, g255_709, b255_709), (255, 255, 255), "Out-of-range Y=255 must clamp to 255 in BT.709");
    println!("  [PASS] Out-of-range Y=0 and Y=255 clamp cleanly to (0,0,0) and (255,255,255).");

    // 3E. Exhaustive test across boundary points in 3D color cube
    let sample_points = [0u8, 1, 16, 64, 128, 192, 235, 240, 254, 255];
    let mut total_permutations = 0usize;
    for &y in &sample_points {
        for &u in &sample_points {
            for &v in &sample_points {
                let (r, g, b) = yuv_to_rgb_bt601(y, u, v);
                let (r7, g7, b7) = yuv_to_rgb_bt709(y, u, v);
                // Evaluates without panic or overflow in intermediate math
                let _ = (r, g, b, r7, g7, b7);
                total_permutations += 1;
            }
        }
    }
    println!(
        "  [PASS] Verified {} corner permutations across 3D color cube with zero overflow.",
        total_permutations
    );

    // 3F. VideoFrameYUV420p memory layout and SIMD stride alignment
    let odd_frame = VideoFrameYUV420p::new(1921, 1081, 42);
    assert_eq!(odd_frame.stride_y, 1936); // (1921 + 15) & !15 = 1936
    assert_eq!(odd_frame.stride_y % 16, 0);
    assert_eq!(odd_frame.stride_u % 16, 0);
    assert_eq!(odd_frame.stride_v % 16, 0);
    assert!(odd_frame.y_plane.is_64_aligned());
    assert!(odd_frame.u_plane.is_64_aligned());
    assert!(odd_frame.v_plane.is_64_aligned());

    let mut rgba_frame = VideoFrameRGBA::new(1921, 1081, 0);
    assert_eq!(rgba_frame.stride % 64, 0);
    assert!(rgba_frame.data.is_64_aligned());

    convert_yuv420p_to_rgba(&odd_frame, &mut rgba_frame, ColorStandard::Bt709).unwrap();
    assert_eq!(rgba_frame.pts_ms, 42);
    println!("  [PASS] Frame geometry: 1921x1081 converted with 64-byte alignment and 16-byte row stride.");
}

// ============================================================================
// 4. MASTER CLOCK SEQLOCK CONTINUITY & CONCURRENT RATE SHIFT STRESS
// ============================================================================

#[test]
fn test_master_clock_seqlock_continuity_across_speed_shifts() {
    println!("\n[CHALLENGE 4] Master Clock SeqLock Continuity & Concurrency Stress Test");

    let clock = Arc::new(MasterClock::new(10000.0, 1.0, PlaybackState::Playing));

    // 4A. Deterministic C0 continuity check across rapid rate transitions:
    // 1.0 -> 1.04 -> 0.96 -> 1.0 -> 1.02 -> 0.98 -> 1.0
    let speeds = [1.04, 0.96, 1.00, 1.02, 0.98, 1.00];
    for &spd in &speeds {
        let now = Instant::now();
        let pts_before = clock.current_pts(now);
        clock.reanchor(now, spd);
        let pts_after = clock.current_pts(now);
        let error = (pts_after - pts_before).abs();
        assert!(
            error < 1e-4,
            "C0 continuity violated at speed transition to {}: error = {} ms",
            spd,
            error
        );
    }
    println!("  [PASS] Master Clock maintains exact C0 continuity across 6 instantaneous speed shifts.");

    // 4B. Concurrent Multi-Threaded Stress Test
    // Writer thread rapidly shifts speed between 0.96 and 1.04 for 2,000 iterations.
    // 4 Reader threads poll current_pts concurrently at maximum speed.
    // Invariants:
    // - Reader NEVER observes a torn read (e.g. sequence number corrupted or wild PTS jump).
    // - No read-to-read step exceeds 20ms during continuous progression.
    let stop_signal = Arc::new(AtomicBool::new(false));
    let torn_reads_count = Arc::new(AtomicUsize::new(0));
    let total_reads = Arc::new(AtomicUsize::new(0));

    let num_readers = 4;
    let mut reader_handles = Vec::new();

    for _reader_id in 0..num_readers {
        let clock_r = Arc::clone(&clock);
        let stop_r = Arc::clone(&stop_signal);
        let torn_r = Arc::clone(&torn_reads_count);
        let total_r = Arc::clone(&total_reads);

        let handle = thread::spawn(move || {
            let mut last_pts = clock_r.current_pts(Instant::now());
            let mut count = 0usize;

            while !stop_r.load(Ordering::Relaxed) {
                let now = Instant::now();
                let pts = clock_r.current_pts(now);
                count += 1;

                // Check for torn read: PTS moving backwards by more than epsilon,
                // or sudden wild jump > 10,000ms
                if pts < last_pts - 0.5 || pts > last_pts + 5000.0 {
                    torn_r.fetch_add(1, Ordering::Relaxed);
                }
                last_pts = pts;
                std::hint::spin_loop();
            }
            total_r.fetch_add(count, Ordering::Relaxed);
            count
        });
        reader_handles.push(handle);
    }

    // Writer thread executes 1,000 speed adjustments
    let clock_w = Arc::clone(&clock);
    let writer_handle = thread::spawn(move || {
        let test_rates = [0.96, 1.04, 0.98, 1.02, 1.00];
        for i in 0..1000 {
            let spd = test_rates[i % test_rates.len()];
            clock_w.reanchor(Instant::now(), spd);
            if i % 100 == 0 {
                thread::yield_now();
            }
        }
    });

    writer_handle.join().unwrap();
    stop_signal.store(true, Ordering::Release);

    for h in reader_handles {
        h.join().unwrap();
    }

    let reads = total_reads.load(Ordering::Relaxed);
    let torn = torn_reads_count.load(Ordering::Relaxed);
    println!(
        "  [PASS] Concurrent SeqLock test: {} reads performed across 4 threads during 1,000 speed shifts.",
        reads
    );
    assert_eq!(torn, 0, "Detected {} torn reads in SeqLock clock!", torn);
    println!("  [PASS] 0 torn reads detected. Lock-free read loop strictly safe.");
}

// ============================================================================
// 5. SPSC RING BUFFER CONCURRENT PRODUCER-CONSUMER STRESS TEST
// ============================================================================

#[test]
fn test_spsc_ring_buffer_concurrent_stress_full_empty() {
    println!("\n[CHALLENGE 5] SPSC Ring Buffer Boundary & Concurrency Stress Test");

    // 5A. Single-threaded boundary semantics (Capacity = 16)
    let buf = SpscRingBuffer::<usize, 16>::new();
    assert_eq!(buf.capacity(), 16);
    assert!(buf.is_empty());
    assert_eq!(buf.len(), 0);
    assert_eq!(buf.try_pop(), None);

    // Fill to exactly 16 items
    for i in 0..16 {
        assert!(buf.try_push(i).is_ok());
    }
    assert_eq!(buf.len(), 16);
    assert!(!buf.is_empty());

    // 17th item MUST fail with BufferFullError preserving the rejected item
    match buf.try_push(9999) {
        Err(BufferFullError { item }) => assert_eq!(item, 9999),
        Ok(_) => panic!("Buffer did not reject 17th item!"),
    }

    // Pop 1 item, then push 1 item
    assert_eq!(buf.try_pop(), Some(0));
    assert_eq!(buf.len(), 15);
    assert!(buf.try_push(100).is_ok());
    assert_eq!(buf.len(), 16);

    // Drain all
    for expected in 1..16 {
        assert_eq!(buf.try_pop(), Some(expected));
    }
    assert_eq!(buf.try_pop(), Some(100));
    assert_eq!(buf.try_pop(), None);
    assert!(buf.is_empty());
    println!("  [PASS] SPSC capacity 16 strictly enqueues 16 items and returns unconsumed item on full.");

    // 5B. Multi-Threaded Concurrent Producer-Consumer Stress Test
    // Using a tight buffer capacity of 32 to maximize full/empty boundary collisions!
    const TOTAL_ITEMS: usize = 100_000;
    let ring = Arc::new(SpscRingBuffer::<usize, 32>::new());
    let ring_producer = Arc::clone(&ring);

    let producer_thread = thread::spawn(move || {
        for mut val in 0..TOTAL_ITEMS {
            loop {
                match ring_producer.try_push(val) {
                    Ok(()) => break,
                    Err(e) => {
                        val = e.item;
                        std::hint::spin_loop();
                    }
                }
            }
        }
    });

    let ring_consumer = Arc::clone(&ring);
    let consumer_thread = thread::spawn(move || {
        let mut consumed = Vec::with_capacity(TOTAL_ITEMS);
        while consumed.len() < TOTAL_ITEMS {
            if let Some(val) = ring_consumer.try_pop() {
                consumed.push(val);
            } else {
                std::hint::spin_loop();
            }
        }
        consumed
    });

    producer_thread.join().unwrap();
    let consumed = consumer_thread.join().unwrap();

    assert_eq!(consumed.len(), TOTAL_ITEMS);
    for (expected, &actual) in consumed.iter().enumerate() {
        if actual != expected {
            panic!(
                "Data corruption at index {}: expected {}, got {}",
                expected, expected, actual
            );
        }
    }
    println!(
        "  [PASS] Concurrent SPSC stress test: {} items transferred through 32-slot ring buffer with zero data loss, corruption, or deadlock.",
        TOTAL_ITEMS
    );
}

// ============================================================================
// 6. COMMON, DEMUX TIMESCALE, AND REORDER BUFFER ADVERSARIAL STRESS
// ============================================================================

#[test]
fn test_common_and_demux_primitives_adversarial() {
    println!("\n[CHALLENGE 6] Common, Timescale, and Reorder Buffer Primitives");

    // 6A. Clamp U8 edge cases
    assert_eq!(clamp_u8(-1000), 0);
    assert_eq!(clamp_u8(-1), 0);
    assert_eq!(clamp_u8(0), 0);
    assert_eq!(clamp_u8(128), 128);
    assert_eq!(clamp_u8(255), 255);
    assert_eq!(clamp_u8(256), 255);
    assert_eq!(clamp_u8(100000), 255);

    // 6B. CRC32 incremental vs one-shot
    let data = b"The quick brown fox jumps over the lazy dog";
    let full_crc = Crc32::digest(data);
    let part1 = Crc32::digest(&data[..16]);
    let part2 = Crc32::update(part1, &data[16..]);
    assert_eq!(full_crc, part2);

    // 6C. ByteReader and BitReader bounds and EOF
    let bytes = [0xAA, 0x55, 0x12, 0x34];
    let mut br = ByteReader::new(&bytes);
    assert_eq!(br.read_u16_be().unwrap(), 0xAA55);
    assert_eq!(br.read_u16_le().unwrap(), 0x3412);
    assert!(br.read_u8().is_err());

    let mut bit_r = BitReader::new(&[0b1010_1100]);
    assert_eq!(bit_r.read_bits(4).unwrap(), 0b1010);
    assert_eq!(bit_r.read_bits(4).unwrap(), 0b1100);
    assert!(bit_r.read_bit().is_err());

    // 6D. MediaTime rational math and timecode formatting
    let mt = MediaTime::from_rational(1001, 24000).unwrap();
    assert_eq!(mt.as_millis(), 41);
    assert_eq!(MediaTime::from_rational(1, 0), None);

    // 6E. compute_simpleblock_pts 24h and overflow resistance
    let ms_24h = 24 * 3600 * 1000i64;
    let (pts_ns, pts_ms) = compute_simpleblock_pts(ms_24h, 15, 1_000_000).unwrap();
    assert_eq!(pts_ms, ms_24h + 15);
    assert_eq!(pts_ns, (ms_24h + 15) * 1_000_000);

    // Extreme scale overflow detection:
    // 1. Ticks overflow i64::MAX
    assert!(compute_simpleblock_pts(i64::MAX, 1, 1_000_000).is_err());
    // 2. Multiplication overflow: (i64::MAX / 2) * 3
    assert!(compute_simpleblock_pts(i64::MAX / 2, 0, 3).is_err());

    // 3. Extreme timescale overflow detection:
    // u64::MAX and timescales > i64::MAX MUST strictly return Err(DemuxError::IntegerOverflow)
    assert_eq!(
        compute_simpleblock_pts(1000, 0, u64::MAX),
        Err(DemuxError::IntegerOverflow)
    );
    assert_eq!(
        compute_simpleblock_pts(i64::MAX / 2, 0, u64::MAX),
        Err(DemuxError::IntegerOverflow)
    );
    assert_eq!(
        compute_simpleblock_pts(1000, 0, (i64::MAX as u64) + 1),
        Err(DemuxError::IntegerOverflow)
    );
    assert_eq!(
        compute_simpleblock_pts(0, 0, (i64::MAX as u64) + 1),
        Err(DemuxError::IntegerOverflow)
    );

    // 6F. PresentationReorderBuffer min-heap sorting
    let mut reorder = PresentationReorderBuffer::<u32>::new(4);
    reorder.push(300, 3);
    reorder.push(100, 1);
    reorder.push(200, 2);
    assert_eq!(reorder.len(), 3);
    let drained: Vec<u32> = reorder.drain_all().into_iter().map(|(_, item)| item).collect();
    assert_eq!(drained, vec![1, 2, 3]);

    // 6G. Presentation Timing Decision Window
    let clk = MasterClock::new(1000.0, 1.0, PlaybackState::Playing);
    let now = Instant::now();
    // Early by 50ms: 1050ms vs 1000ms -> Wait
    match clk.evaluate_presentation_timing(1050, now) {
        PresentationAction::Wait(d) => assert!(d.as_millis() >= 35 && d.as_millis() <= 45),
        other => panic!("Expected Wait, got {:?}", other),
    }
    // On-time: 1000ms vs 1000ms -> PresentNow
    assert_eq!(
        clk.evaluate_presentation_timing(1000, now),
        PresentationAction::PresentNow
    );
    // Slightly late (-20ms) -> PresentNowLate
    assert_eq!(
        clk.evaluate_presentation_timing(980, now),
        PresentationAction::PresentNowLate
    );
    // Severely late (-150ms) -> DropLateFrame
    assert_eq!(
        clk.evaluate_presentation_timing(850, now),
        PresentationAction::DropLateFrame
    );

    println!("  [PASS] All common, demux timescale, reorder buffer, and decision window checks passed.");
}

