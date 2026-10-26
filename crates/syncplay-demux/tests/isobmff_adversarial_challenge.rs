//! Empirical Adversarial Challenge Test Suite for ISOBMFF / MP4 Demuxer (`syncplay-demux`).
//! Stress tests 64-bit largesize bounds, co64 64-bit chunk offsets, corrupted stsc runs,
//! multi-track interleaved reading from mdat, and keyframe seeking with stss and fallbacks.

use std::io::Cursor;
use syncplay_demux::{
    build_synthetic_full_mp4, BoxHeader, DemuxError, Demuxer, FourCc, IsoBmffDemuxer,
    SampleTableResolver, TrackType, SYNTHETIC_MP4_VECTOR,
};

// ---------------------------------------------------------------------------
// Helper: MP4 Box Builder for constructing synthetic/adversarial MP4 byte streams
// ---------------------------------------------------------------------------
struct Mp4BoxBuilder;

impl Mp4BoxBuilder {
    fn make_box(fourcc: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let size = (8 + payload.len()) as u32;
        let mut out = Vec::with_capacity(size as usize);
        out.extend_from_slice(&size.to_be_bytes());
        out.extend_from_slice(fourcc);
        out.extend_from_slice(payload);
        out
    }

    fn make_largesize_box(fourcc: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let size = (16 + payload.len()) as u64;
        let mut out = Vec::with_capacity(size as usize);
        out.extend_from_slice(&1u32.to_be_bytes()); // size = 1 indicates largesize
        out.extend_from_slice(fourcc);
        out.extend_from_slice(&size.to_be_bytes()); // 64-bit largesize
        out.extend_from_slice(payload);
        out
    }

    fn make_ftyp() -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(b"isom"); // major_brand
        payload.extend_from_slice(&0x00000200u32.to_be_bytes()); // minor_version
        payload.extend_from_slice(b"isomiso2mp41"); // compatible brands
        Self::make_box(b"ftyp", &payload)
    }

    fn make_mvhd(timescale: u32, duration_ticks: u32) -> Vec<u8> {
        let mut payload = vec![0u8; 100];
        payload[0] = 0; // version 0
        // timescale at bytes 12..16
        payload[12..16].copy_from_slice(&timescale.to_be_bytes());
        // duration at bytes 16..20
        payload[16..20].copy_from_slice(&duration_ticks.to_be_bytes());
        // rate 1.0 (0x00010000) at 20..24
        payload[20..24].copy_from_slice(&0x00010000u32.to_be_bytes());
        // volume 1.0 (0x0100) at 24..26
        payload[24..26].copy_from_slice(&0x0100u16.to_be_bytes());
        // next_track_id at 96..100
        payload[96..100].copy_from_slice(&10u32.to_be_bytes());
        Self::make_box(b"mvhd", &payload)
    }
}

// ===========================================================================
// 1. ADVERSARIAL CHALLENGE: BOX HEADER PARSER FUZZING & BOUNDS VALIDATION
// ===========================================================================
#[test]
fn test_adversarial_box_header_bounds_and_fuzzing() {
    // 1.1 Valid standard 32-bit boxes
    for size in [8u32, 16, 24, 100, 1024, 65536, 10_000_000] {
        let mut buf = [0u8; 8];
        buf[0..4].copy_from_slice(&size.to_be_bytes());
        buf[4..8].copy_from_slice(b"free");
        let hdr = BoxHeader::parse(&buf).expect("valid standard box size");
        assert_eq!(hdr.fourcc, FourCc(*b"free"));
        assert_eq!(hdr.header_size, 8);
        assert_eq!(hdr.total_size, size as u64);
    }

    // 1.2 Valid 64-bit largesize bounds
    // Exact lower boundary: largesize == 16
    let mut min_large = [0u8; 16];
    min_large[0..4].copy_from_slice(&1u32.to_be_bytes());
    min_large[4..8].copy_from_slice(b"mdat");
    min_large[8..16].copy_from_slice(&16u64.to_be_bytes());
    let hdr = BoxHeader::parse(&min_large).expect("largesize == 16 must succeed");
    assert_eq!(hdr.header_size, 16);
    assert_eq!(hdr.total_size, 16);

    // Large sizes (4GB, 100GB, exact upper boundary i64::MAX)
    const I64_MAX: u64 = i64::MAX as u64;
    for valid_large in [17u64, 4_294_967_296, 100_000_000_000, I64_MAX] {
        let mut buf = [0u8; 16];
        buf[0..4].copy_from_slice(&1u32.to_be_bytes());
        buf[4..8].copy_from_slice(b"mdat");
        buf[8..16].copy_from_slice(&valid_large.to_be_bytes());
        let hdr = BoxHeader::parse(&buf).expect("valid largesize must succeed");
        assert_eq!(hdr.header_size, 16);
        assert_eq!(hdr.total_size, valid_large);
    }

    // 1.3 Malformed 64-bit largesize (< 16)
    for bad_large in [0u64, 1, 2, 7, 8, 12, 14, 15] {
        let mut buf = [0u8; 16];
        buf[0..4].copy_from_slice(&1u32.to_be_bytes());
        buf[4..8].copy_from_slice(b"mdat");
        buf[8..16].copy_from_slice(&bad_large.to_be_bytes());
        let res = BoxHeader::parse(&buf);
        assert!(
            matches!(res, Err(DemuxError::MalformedBox(_))),
            "largesize < 16 ({}) must return MalformedBox, got: {:?}",
            bad_large,
            res
        );
    }

    // 1.4 Malformed 64-bit largesize (> i64::MAX)
    for overflow_large in [I64_MAX + 1, I64_MAX + 100, u64::MAX - 1, u64::MAX] {
        let mut buf = [0u8; 16];
        buf[0..4].copy_from_slice(&1u32.to_be_bytes());
        buf[4..8].copy_from_slice(b"mdat");
        buf[8..16].copy_from_slice(&overflow_large.to_be_bytes());
        let res = BoxHeader::parse(&buf);
        assert!(
            matches!(res, Err(DemuxError::MalformedBox(_))),
            "largesize > i64::MAX ({}) must return MalformedBox, got: {:?}",
            overflow_large,
            res
        );
    }

    // 1.5 Malformed 32-bit standard box size (size != 0 && size < 8)
    for bad_size32 in [2u32, 3, 4, 5, 6, 7] {
        let mut buf = [0u8; 8];
        buf[0..4].copy_from_slice(&bad_size32.to_be_bytes());
        buf[4..8].copy_from_slice(b"moov");
        let res = BoxHeader::parse(&buf);
        assert!(
            matches!(res, Err(DemuxError::MalformedBox(_))),
            "size < 8 ({}) must return MalformedBox, got: {:?}",
            bad_size32,
            res
        );
    }

    // 1.6 Truncated input buffers
    // Less than 8 bytes available
    for len in 0..8 {
        let buf = vec![0xAAu8; len];
        let res = BoxHeader::parse(&buf);
        assert!(
            matches!(res, Err(DemuxError::UnexpectedEof { needed: 8, available }) if available == len),
            "buffer len {} < 8 must return UnexpectedEof {{ needed: 8, available: {} }}, got: {:?}",
            len, len, res
        );
    }

    // Size == 1 but less than 16 bytes available
    for len in 8..16 {
        let mut buf = vec![0u8; len];
        buf[0..4].copy_from_slice(&1u32.to_be_bytes());
        buf[4..8].copy_from_slice(b"mdat");
        let res = BoxHeader::parse(&buf);
        assert!(
            matches!(res, Err(DemuxError::UnexpectedEof { needed: 16, available }) if available == len),
            "largesize buffer len {} < 16 must return UnexpectedEof {{ needed: 16, available: {} }}, got: {:?}",
            len, len, res
        );
    }

    // 1.7 Size == 0 extends to end of input slice
    for len in [8usize, 12, 100, 512, 4096] {
        let mut buf = vec![0u8; len];
        buf[0..4].copy_from_slice(&0u32.to_be_bytes()); // size = 0
        buf[4..8].copy_from_slice(b"mdat");
        let hdr = BoxHeader::parse(&buf).expect("size 0 should succeed");
        assert_eq!(hdr.header_size, 8);
        assert_eq!(hdr.total_size, len as u64);
    }

    // 1.8 Fuzzing robustness: pseudo-random byte permutations must never panic
    let mut prng_state = 0x12345678u64;
    for _ in 0..10_000 {
        prng_state = prng_state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let len = ((prng_state >> 32) % 32) as usize;
        let mut random_bytes = vec![0u8; len];
        for b in &mut random_bytes {
            prng_state = prng_state.wrapping_mul(6364136223846793005).wrapping_add(1);
            *b = (prng_state >> 24) as u8;
        }
        // Must not panic!
        let _ = BoxHeader::parse(&random_bytes);
    }
}

// ===========================================================================
// 2. ADVERSARIAL CHALLENGE: 64-BIT CHUNK OFFSETS (`co64`) & OVERFLOW CHECKS
// ===========================================================================
#[test]
fn test_adversarial_co64_64bit_chunk_offsets() {
    let mut resolver = SampleTableResolver::new();
    // 3 chunks with 64-bit offsets exceeding 4GB ($> 2^{32}$)
    let chunk_0 = 0x1_0000_0000u64; // 4 GB
    let chunk_1 = 0x2_8000_0000u64; // 10 GB
    let chunk_2 = 0x5_0000_0000u64; // 20 GB
    resolver.chunk_offsets = vec![chunk_0, chunk_1, chunk_2];

    // stsc: Chunk 1 has 2 samples, Chunk 2 has 3 samples, Chunk 3 has 1 sample
    resolver.add_stsc_entry(1, 2, 1).unwrap();
    resolver.add_stsc_entry(2, 3, 1).unwrap();
    resolver.add_stsc_entry(3, 1, 1).unwrap();

    // stsz: sample sizes
    resolver.stsz = vec![
        1000, 2000, // chunk 0: samples 0, 1
        500, 600, 700, // chunk 1: samples 2, 3, 4
        4000, // chunk 2: sample 5
    ];

    // Verify sample offsets in 64-bit address space
    assert_eq!(resolver.compute_sample_offset(0).unwrap(), chunk_0);
    assert_eq!(
        resolver.compute_sample_offset(1).unwrap(),
        chunk_0 + 1000
    );
    assert_eq!(resolver.compute_sample_offset(2).unwrap(), chunk_1);
    assert_eq!(
        resolver.compute_sample_offset(3).unwrap(),
        chunk_1 + 500
    );
    assert_eq!(
        resolver.compute_sample_offset(4).unwrap(),
        chunk_1 + 500 + 600
    );
    assert_eq!(resolver.compute_sample_offset(5).unwrap(), chunk_2);

    // Verify sample 6 exceeds chunks -> DemuxError::NotFound
    assert!(matches!(
        resolver.compute_sample_offset(6),
        Err(DemuxError::NotFound(_))
    ));

    // Boundary test: Integer overflow defense
    let mut overflow_resolver = SampleTableResolver::new();
    overflow_resolver.chunk_offsets = vec![u64::MAX - 50];
    overflow_resolver.add_stsc_entry(1, 2, 1).unwrap();
    overflow_resolver.stsz = vec![100, 100]; // sample 0 is 100 bytes, sample 1 offset = (u64::MAX - 50) + 100 -> overflow!

    // Sample 0 at chunk base offset succeeds
    assert_eq!(
        overflow_resolver.compute_sample_offset(0).unwrap(),
        u64::MAX - 50
    );
    // Sample 1 offset calculation must return IntegerOverflow rather than panicking or wrapping
    let res = overflow_resolver.compute_sample_offset(1);
    assert!(
        matches!(res, Err(DemuxError::IntegerOverflow)),
        "Chunk offset overflow must return DemuxError::IntegerOverflow, got: {:?}",
        res
    );
}

// ===========================================================================
// 3. ADVERSARIAL CHALLENGE: CORRUPTED AND MALFORMED `stsc` RUNS
// ===========================================================================
#[test]
fn test_adversarial_corrupted_stsc_runs() {
    let mut resolver = SampleTableResolver::new();

    // 3.1 Rejection of zero samples_per_chunk
    assert!(matches!(
        resolver.add_stsc_entry(1, 0, 1),
        Err(DemuxError::CorruptSampleTable(_))
    ));

    // 3.2 Compute sample offset with empty stsc or empty chunk_offsets
    assert!(matches!(
        resolver.compute_sample_offset(0),
        Err(DemuxError::CorruptSampleTable(_))
    ));
    resolver.chunk_offsets = vec![1000];
    assert!(matches!(
        resolver.compute_sample_offset(0),
        Err(DemuxError::CorruptSampleTable(_))
    ));
    resolver.chunk_offsets.clear();
    resolver.add_stsc_entry(1, 1, 1).unwrap();
    assert!(matches!(
        resolver.compute_sample_offset(0),
        Err(DemuxError::CorruptSampleTable(_))
    ));

    // 3.3 Corrupted stsc missing chunk 1 (first entry starts at chunk 2)
    let mut missing_c1 = SampleTableResolver::new();
    missing_c1.chunk_offsets = vec![1000, 2000];
    missing_c1.add_stsc_entry(2, 4, 1).unwrap(); // first_chunk is 2, not 1!
    missing_c1.stsz = vec![100; 10];

    let res_c1 = missing_c1.compute_sample_offset(0);
    assert!(
        matches!(res_c1, Err(DemuxError::CorruptSampleTable(ref msg)) if msg.contains("no stsc entry for chunk 1")),
        "Missing chunk 1 must return CorruptSampleTable error, got: {:?}",
        res_c1
    );

    // 3.4 Multi-chunk run-length stress test with gaps between declared chunks
    let mut run_resolver = SampleTableResolver::new();
    // 10 chunks total
    run_resolver.chunk_offsets = (0..10).map(|i| 10_000 + i * 1_000).collect();
    // Chunks 1..3: 2 samples per chunk (chunks 1, 2, 3 -> 6 samples)
    run_resolver.add_stsc_entry(1, 2, 1).unwrap();
    // Chunks 4..7: 3 samples per chunk (chunks 4, 5, 6, 7 -> 12 samples)
    run_resolver.add_stsc_entry(4, 3, 1).unwrap();
    // Chunks 8..10: 1 sample per chunk (chunks 8, 9, 10 -> 3 samples)
    run_resolver.add_stsc_entry(8, 1, 1).unwrap();
    // Total samples across 10 chunks: 6 + 12 + 3 = 21 samples
    run_resolver.uniform_sample_size = Some(100);

    // Verify sample indices across run transitions:
    // Chunk 1: samples 0, 1 -> offset 10000, 10100
    assert_eq!(run_resolver.compute_sample_offset(0).unwrap(), 10000);
    assert_eq!(run_resolver.compute_sample_offset(1).unwrap(), 10100);
    // Chunk 2: samples 2, 3 -> offset 11000, 11100
    assert_eq!(run_resolver.compute_sample_offset(2).unwrap(), 11000);
    assert_eq!(run_resolver.compute_sample_offset(3).unwrap(), 11100);
    // Chunk 3: samples 4, 5 -> offset 12000, 12100
    assert_eq!(run_resolver.compute_sample_offset(4).unwrap(), 12000);
    assert_eq!(run_resolver.compute_sample_offset(5).unwrap(), 12100);
    // Chunk 4 (transition to 3 per chunk): samples 6, 7, 8 -> offset 13000, 13100, 13200
    assert_eq!(run_resolver.compute_sample_offset(6).unwrap(), 13000);
    assert_eq!(run_resolver.compute_sample_offset(7).unwrap(), 13100);
    assert_eq!(run_resolver.compute_sample_offset(8).unwrap(), 13200);
    // Chunk 7: samples 15, 16, 17 -> offset 16000, 16100, 16200
    assert_eq!(run_resolver.compute_sample_offset(15).unwrap(), 16000);
    assert_eq!(run_resolver.compute_sample_offset(17).unwrap(), 16200);
    // Chunk 8 (transition to 1 per chunk): sample 18 -> offset 17000
    assert_eq!(run_resolver.compute_sample_offset(18).unwrap(), 17000);
    // Chunk 10: sample 20 -> offset 19000
    assert_eq!(run_resolver.compute_sample_offset(20).unwrap(), 19000);
    // Sample 21 exceeds 21 total samples -> NotFound
    assert!(matches!(
        run_resolver.compute_sample_offset(21),
        Err(DemuxError::NotFound(_))
    ));
}

// ===========================================================================
// 4. ADVERSARIAL CHALLENGE: MULTI-TRACK INTERLEAVED READING FROM MDAT
// ===========================================================================
#[test]
fn test_adversarial_multi_track_interleaved_reading() {
    // Construct a synthetic 3-track MP4 container in memory:
    // Track 1: Video (avc1), 4 samples (PTS: 0, 250, 500, 750)
    // Track 2: Audio (mp4a), 4 samples (PTS: 0, 250, 500, 750)
    // Track 3: Subtitle (text), 2 samples (PTS: 0, 500)
    //
    // The samples are physically interleaved in mdat in offset order:
    // Offset 1000: Video Sample 0 (8 bytes: [0x10, ...])
    // Offset 1010: Audio Sample 0 (6 bytes: [0x20, ...])
    // Offset 1020: Subtitle Sample 0 (4 bytes: [0x30, ...])
    // Offset 1030: Video Sample 1 (8 bytes: [0x11, ...])
    // Offset 1040: Audio Sample 1 (6 bytes: [0x21, ...])
    // Offset 1050: Video Sample 2 (8 bytes: [0x12, ...])
    // Offset 1060: Audio Sample 2 (6 bytes: [0x22, ...])
    // Offset 1070: Subtitle Sample 1 (4 bytes: [0x31, ...])
    // Offset 1080: Video Sample 3 (8 bytes: [0x13, ...])
    // Offset 1090: Audio Sample 3 (6 bytes: [0x23, ...])

    let mut file_bytes = Vec::new();

    // 1. ftyp box
    file_bytes.extend_from_slice(&Mp4BoxBuilder::make_ftyp());

    // 2. Dummy padding to push mdat offsets to predetermined values
    let pad_target = 1000usize;
    if file_bytes.len() < pad_target {
        let pad_len = pad_target - file_bytes.len() - 8;
        let pad_box = Mp4BoxBuilder::make_box(b"free", &vec![0u8; pad_len]);
        file_bytes.extend_from_slice(&pad_box);
    }
    assert_eq!(file_bytes.len(), 1000, "Header padding alignment check");

    // 3. mdat box containing all sample data
    let mut mdat_payload = Vec::new();
    // 1000: Video 0 (offset relative to mdat payload start = 8 - 8 = 0)
    mdat_payload.extend_from_slice(&[0x10; 8]); // 1008
    mdat_payload.extend_from_slice(&[0x00; 2]); // pad to 1010
    // 1010: Audio 0
    mdat_payload.extend_from_slice(&[0x20; 6]); // 1016
    mdat_payload.extend_from_slice(&[0x00; 4]); // pad to 1020
    // 1020: Subtitle 0
    mdat_payload.extend_from_slice(&[0x30; 4]); // 1024
    mdat_payload.extend_from_slice(&[0x00; 6]); // pad to 1030
    // 1030: Video 1
    mdat_payload.extend_from_slice(&[0x11; 8]); // 1038
    mdat_payload.extend_from_slice(&[0x00; 2]); // pad to 1040
    // 1040: Audio 1
    mdat_payload.extend_from_slice(&[0x21; 6]); // 1046
    mdat_payload.extend_from_slice(&[0x00; 4]); // pad to 1050
    // 1050: Video 2
    mdat_payload.extend_from_slice(&[0x12; 8]); // 1058
    mdat_payload.extend_from_slice(&[0x00; 2]); // pad to 1060
    // 1060: Audio 2
    mdat_payload.extend_from_slice(&[0x22; 6]); // 1066
    mdat_payload.extend_from_slice(&[0x00; 4]); // pad to 1070
    // 1070: Subtitle 1
    mdat_payload.extend_from_slice(&[0x31; 4]); // 1074
    mdat_payload.extend_from_slice(&[0x00; 6]); // pad to 1080
    // 1080: Video 3
    mdat_payload.extend_from_slice(&[0x13; 8]); // 1088
    mdat_payload.extend_from_slice(&[0x00; 2]); // pad to 1090
    // 1090: Audio 3
    mdat_payload.extend_from_slice(&[0x23; 6]); // 1096
    mdat_payload.extend_from_slice(&[0x00; 4]); // 1100

    let mdat_box = Mp4BoxBuilder::make_box(b"mdat", &mdat_payload);
    // Note: mdat box header is 8 bytes at 1000..1008, so sample data starts at 1008!
    // Let's adjust chunk offsets to match actual payload offsets:
    // Video: 1008, 1038, 1058, 1088
    // Audio: 1018, 1048, 1068, 1098
    // Subtitle: 1028, 1078
    // Re-pack mdat payload cleanly without header confusion:
    file_bytes.truncate(1000);
    // Actually, mdat box at 1000 has 8 bytes header (1000..1008).
    // Samples start at 1008:
    let v0_off = 1008u64;
    let a0_off = 1018u64;
    let s0_off = 1028u64;
    let v1_off = 1038u64;
    let a1_off = 1048u64;
    let v2_off = 1058u64;
    let a2_off = 1068u64;
    let s1_off = 1078u64;
    let v3_off = 1088u64;
    let a3_off = 1098u64;

    file_bytes.extend_from_slice(&mdat_box);

    // 4. Build moov box with 3 trak boxes
    let mut moov_payload = Vec::new();
    moov_payload.extend_from_slice(&Mp4BoxBuilder::make_mvhd(1000, 1000));

    // Helper to build a trak box
    let make_trak = |track_id: u32,
                     handler: &[u8; 4],
                     codec_fourcc: &[u8; 4],
                     chunk_offsets: &[u64],
                     sample_sizes: &[u32],
                     timescale: u32,
                     sample_deltas: &[(u32, u32)],
                     sync_samples: Option<&[u32]>|
     -> Vec<u8> {
        let mut tkhd_payload = vec![0u8; 84];
        tkhd_payload[12..16].copy_from_slice(&track_id.to_be_bytes());
        tkhd_payload[76..80].copy_from_slice(&(1920u32 << 16).to_be_bytes());
        tkhd_payload[80..84].copy_from_slice(&(1080u32 << 16).to_be_bytes());
        let tkhd_box = Mp4BoxBuilder::make_box(b"tkhd", &tkhd_payload);

        let mut mdhd_payload = vec![0u8; 24];
        mdhd_payload[12..16].copy_from_slice(&timescale.to_be_bytes());
        mdhd_payload[16..20].copy_from_slice(&1000u32.to_be_bytes());
        let mdhd_box = Mp4BoxBuilder::make_box(b"mdhd", &mdhd_payload);

        let mut hdlr_payload = vec![0u8; 24];
        hdlr_payload[8..12].copy_from_slice(handler);
        let hdlr_box = Mp4BoxBuilder::make_box(b"hdlr", &hdlr_payload);

        // stsd
        let mut entry_body = vec![0u8; 80];
        // For video: dimensions at 24..28
        entry_body[24..26].copy_from_slice(&1920u16.to_be_bytes());
        entry_body[26..28].copy_from_slice(&1080u16.to_be_bytes());
        // For audio: ch at 16..18, sr at 24..28
        entry_body[16..18].copy_from_slice(&2u16.to_be_bytes());
        entry_body[24..28].copy_from_slice(&(48000u32 << 16).to_be_bytes());

        let mut stsd_entry = Vec::new();
        stsd_entry.extend_from_slice(&((8 + entry_body.len()) as u32).to_be_bytes());
        stsd_entry.extend_from_slice(codec_fourcc);
        stsd_entry.extend_from_slice(&entry_body);

        let mut stsd_payload = Vec::new();
        stsd_payload.extend_from_slice(&0u32.to_be_bytes()); // version & flags
        stsd_payload.extend_from_slice(&1u32.to_be_bytes()); // entry_count = 1
        stsd_payload.extend_from_slice(&stsd_entry);
        let stsd_box = Mp4BoxBuilder::make_box(b"stsd", &stsd_payload);

        // stts
        let mut stts_payload = Vec::new();
        stts_payload.extend_from_slice(&0u32.to_be_bytes());
        stts_payload.extend_from_slice(&(sample_deltas.len() as u32).to_be_bytes());
        for &(count, delta) in sample_deltas {
            stts_payload.extend_from_slice(&count.to_be_bytes());
            stts_payload.extend_from_slice(&delta.to_be_bytes());
        }
        let stts_box = Mp4BoxBuilder::make_box(b"stts", &stts_payload);

        // stsc (1 sample per chunk)
        let mut stsc_payload = Vec::new();
        stsc_payload.extend_from_slice(&0u32.to_be_bytes());
        stsc_payload.extend_from_slice(&1u32.to_be_bytes()); // 1 entry
        stsc_payload.extend_from_slice(&1u32.to_be_bytes()); // first_chunk = 1
        stsc_payload.extend_from_slice(&1u32.to_be_bytes()); // samples_per_chunk = 1
        stsc_payload.extend_from_slice(&1u32.to_be_bytes()); // sample_desc_idx = 1
        let stsc_box = Mp4BoxBuilder::make_box(b"stsc", &stsc_payload);

        // stsz
        let mut stsz_payload = Vec::new();
        stsz_payload.extend_from_slice(&0u32.to_be_bytes());
        stsz_payload.extend_from_slice(&0u32.to_be_bytes()); // uniform = 0
        stsz_payload.extend_from_slice(&(sample_sizes.len() as u32).to_be_bytes());
        for &sz in sample_sizes {
            stsz_payload.extend_from_slice(&sz.to_be_bytes());
        }
        let stsz_box = Mp4BoxBuilder::make_box(b"stsz", &stsz_payload);

        // stco
        let mut stco_payload = Vec::new();
        stco_payload.extend_from_slice(&0u32.to_be_bytes());
        stco_payload.extend_from_slice(&(chunk_offsets.len() as u32).to_be_bytes());
        for &off in chunk_offsets {
            stco_payload.extend_from_slice(&(off as u32).to_be_bytes());
        }
        let stco_box = Mp4BoxBuilder::make_box(b"stco", &stco_payload);

        // stss (optional)
        let mut stbl_payload = Vec::new();
        stbl_payload.extend_from_slice(&stsd_box);
        stbl_payload.extend_from_slice(&stts_box);
        if let Some(syncs) = sync_samples {
            let mut stss_payload = Vec::new();
            stss_payload.extend_from_slice(&0u32.to_be_bytes());
            stss_payload.extend_from_slice(&(syncs.len() as u32).to_be_bytes());
            for &s in syncs {
                stss_payload.extend_from_slice(&s.to_be_bytes());
            }
            let stss_box = Mp4BoxBuilder::make_box(b"stss", &stss_payload);
            stbl_payload.extend_from_slice(&stss_box);
        }
        stbl_payload.extend_from_slice(&stsc_box);
        stbl_payload.extend_from_slice(&stsz_box);
        stbl_payload.extend_from_slice(&stco_box);
        let stbl_box = Mp4BoxBuilder::make_box(b"stbl", &stbl_payload);

        let minf_box = Mp4BoxBuilder::make_box(b"minf", &stbl_box);
        let mut mdia_payload = Vec::new();
        mdia_payload.extend_from_slice(&mdhd_box);
        mdia_payload.extend_from_slice(&hdlr_box);
        mdia_payload.extend_from_slice(&minf_box);
        let mdia_box = Mp4BoxBuilder::make_box(b"mdia", &mdia_payload);

        let mut trak_payload = Vec::new();
        trak_payload.extend_from_slice(&tkhd_box);
        trak_payload.extend_from_slice(&mdia_box);
        Mp4BoxBuilder::make_box(b"trak", &trak_payload)
    };

    // Track 1 (Video): chunks [v0, v1, v2, v3], stss [1, 3] (keyframes at sample 0 and 2)
    let trak_v = make_trak(
        1,
        b"vide",
        b"avc1",
        &[v0_off, v1_off, v2_off, v3_off],
        &[8, 8, 8, 8],
        1000,
        &[(4, 250)],
        Some(&[1, 3]), // 1-based: sample 0 and sample 2 are keyframes
    );
    // Track 2 (Audio): chunks [a0, a1, a2, a3], no stss (all samples are keyframes)
    let trak_a = make_trak(
        2,
        b"soun",
        b"mp4a",
        &[a0_off, a1_off, a2_off, a3_off],
        &[6, 6, 6, 6],
        1000,
        &[(4, 250)],
        None,
    );
    // Track 3 (Subtitle): chunks [s0, s1], no stss
    let trak_s = make_trak(
        3,
        b"subt",
        b"text",
        &[s0_off, s1_off],
        &[4, 4],
        1000,
        &[(2, 500)],
        None,
    );

    moov_payload.extend_from_slice(&trak_v);
    moov_payload.extend_from_slice(&trak_a);
    moov_payload.extend_from_slice(&trak_s);

    let moov_box = Mp4BoxBuilder::make_box(b"moov", &moov_payload);
    file_bytes.extend_from_slice(&moov_box);

    // 5. Test Probe and Demux Init
    let cursor = Cursor::new(file_bytes);
    let mut demuxer = IsoBmffDemuxer::new();
    let info = demuxer.init(Box::new(cursor)).expect("demuxer init failed");

    assert_eq!(info.tracks.len(), 3, "Must discover all 3 tracks");
    assert_eq!(info.tracks[0].track_type, TrackType::Video);
    assert_eq!(info.tracks[1].track_type, TrackType::Audio);
    assert_eq!(info.tracks[2].track_type, TrackType::Subtitle);

    // 6. Test Sequential Multi-Track Interleaved Extraction
    // Expected reading sequence based on strictly ascending file offsets:
    // Packet 0: Track 1 (V0) @ 1008
    // Packet 1: Track 2 (A0) @ 1018
    // Packet 2: Track 3 (S0) @ 1028
    // Packet 3: Track 1 (V1) @ 1038
    // Packet 4: Track 2 (A1) @ 1048
    // Packet 5: Track 1 (V2) @ 1058
    // Packet 6: Track 2 (A2) @ 1068
    // Packet 7: Track 3 (S1) @ 1078
    // Packet 8: Track 1 (V3) @ 1088
    // Packet 9: Track 2 (A3) @ 1098
    // Packet 10: None (Clean EOF)

    let expected = [
        (1, 0, true, 8, 0x10),    // V0
        (2, 0, true, 6, 0x20),    // A0
        (3, 0, true, 4, 0x30),    // S0
        (1, 250, false, 8, 0x11),  // V1 (not keyframe)
        (2, 250, true, 6, 0x21),  // A1
        (1, 500, true, 8, 0x12),  // V2 (keyframe)
        (2, 500, true, 6, 0x22),  // A2
        (3, 500, true, 4, 0x31),  // S1
        (1, 750, false, 8, 0x13),  // V3 (not keyframe)
        (2, 750, true, 6, 0x23),  // A3
    ];

    for (idx, &(exp_id, exp_pts, exp_key, exp_len, exp_byte)) in expected.iter().enumerate() {
        let pkt = demuxer
            .read_packet()
            .unwrap()
            .unwrap_or_else(|| panic!("Expected packet {} for track {}", idx, exp_id));

        assert_eq!(
            pkt.track_id, exp_id,
            "Packet {} track ID mismatch (got {}, expected {})",
            idx, pkt.track_id, exp_id
        );
        assert_eq!(
            pkt.pts_ms, exp_pts,
            "Packet {} PTS mismatch (got {}, expected {})",
            idx, pkt.pts_ms, exp_pts
        );
        assert_eq!(
            pkt.is_keyframe, exp_key,
            "Packet {} keyframe flag mismatch (got {}, expected {})",
            idx, pkt.is_keyframe, exp_key
        );
        assert_eq!(
            pkt.data.len(),
            exp_len,
            "Packet {} length mismatch",
            idx
        );
        assert_eq!(
            pkt.data[0], exp_byte,
            "Packet {} payload byte mismatch",
            idx
        );
    }

    // After all 10 packets, must return clean EOF Ok(None) repeatedly
    assert!(demuxer.read_packet().unwrap().is_none());
    assert!(demuxer.read_packet().unwrap().is_none());

    // 7. Stress test seeking in multi-track stream:
    // Seek to 500ms (Video has keyframe at 500ms!)
    let landed = demuxer.seek_to(500).expect("seek to 500 failed");
    assert_eq!(landed, 500, "Must land on keyframe at 500ms");

    // Next packets should resume from 500ms!
    // Video sample 2 (500ms) @ 1058
    // Audio sample 2 (500ms) @ 1068
    // Subtitle sample 1 (500ms) @ 1078
    let p_v2 = demuxer.read_packet().unwrap().expect("packet after seek 1");
    assert_eq!(p_v2.track_id, 1);
    assert_eq!(p_v2.pts_ms, 500);
    assert!(p_v2.is_keyframe);

    let p_a2 = demuxer.read_packet().unwrap().expect("packet after seek 2");
    assert_eq!(p_a2.track_id, 2);
    assert_eq!(p_a2.pts_ms, 500);

    let p_s1 = demuxer.read_packet().unwrap().expect("packet after seek 3");
    assert_eq!(p_s1.track_id, 3);
    assert_eq!(p_s1.pts_ms, 500);
}

// ===========================================================================
// 5. ADVERSARIAL CHALLENGE: KEYFRAME SEEKING WITH STSS AND FALLBACK CASES
// ===========================================================================
#[test]
fn test_adversarial_keyframe_seeking_and_fallbacks() {
    let mp4_bytes = build_synthetic_full_mp4();

    // 5.1 Test seeking boundaries on full MP4
    // Synthetic full MP4 has keyframe at 0ms (sample 0) and non-keyframe at 500ms (sample 1)
    let cursor = Cursor::new(mp4_bytes.clone());
    let mut demuxer = IsoBmffDemuxer::new();
    demuxer.init(Box::new(cursor)).expect("init failed");

    // Seek to 0ms -> lands at 0ms
    assert_eq!(demuxer.seek_to(0).unwrap(), 0);
    let p = demuxer.read_packet().unwrap().expect("packet");
    assert_eq!(p.pts_ms, 0);
    assert!(p.is_keyframe);

    // Seek to 250ms (between 0 and 500) -> lands at preceding keyframe at 0ms
    assert_eq!(demuxer.seek_to(250).unwrap(), 0);
    let p = demuxer.read_packet().unwrap().expect("packet");
    assert_eq!(p.pts_ms, 0);
    assert!(p.is_keyframe);

    // Seek to 500ms (sample 1 is non-keyframe) -> must land on preceding keyframe at 0ms!
    assert_eq!(demuxer.seek_to(500).unwrap(), 0);
    let p = demuxer.read_packet().unwrap().expect("packet");
    assert_eq!(p.pts_ms, 0);
    assert!(p.is_keyframe);

    // Seek far past end of stream (100_000ms) -> lands at last preceding keyframe at 0ms
    assert_eq!(demuxer.seek_to(100_000).unwrap(), 0);
    let p = demuxer.read_packet().unwrap().expect("packet");
    assert_eq!(p.pts_ms, 0);

    // Seek to u64::MAX -> lands at last preceding keyframe at 0ms
    assert_eq!(demuxer.seek_to(u64::MAX).unwrap(), 0);
    let p = demuxer.read_packet().unwrap().expect("packet");
    assert_eq!(p.pts_ms, 0);

    // 5.2 Test stream with NO stss box (all samples are keyframes)
    // Construct sample table with sync_samples empty
    let mut no_stss_resolver = SampleTableResolver::new();
    no_stss_resolver.chunk_offsets = vec![1000, 2000, 3000, 4000];
    no_stss_resolver.add_stsc_entry(1, 1, 1).unwrap();
    no_stss_resolver.uniform_sample_size = Some(100);
    no_stss_resolver.stts = vec![(4, 1000)]; // 4 samples, 1000ms each (0, 1000, 2000, 3000)
    // sync_samples is left empty!
    assert!(no_stss_resolver.sync_samples.is_empty());

    let (_dts, pts) = no_stss_resolver.compute_sample_timings(1000);
    assert_eq!(pts, vec![0, 1000, 2000, 3000]);

    // When sync_samples is empty, every sample is treated as keyframe
    // Verify seek logic directly through IsoBmffDemuxer
    // We construct a minimal in-memory MP4 with no stss box:
    let make_no_stss_mp4 = || -> Vec<u8> {
        let mut file = Vec::new();
        file.extend_from_slice(&Mp4BoxBuilder::make_ftyp());

        // Pad to 1000
        let pad = Mp4BoxBuilder::make_box(b"free", &vec![0u8; 1000 - file.len() - 8]);
        file.extend_from_slice(&pad);

        // mdat with 4 samples at 1008, 1108, 1208, 1308
        let mdat_box = Mp4BoxBuilder::make_box(b"mdat", &vec![0xAAu8; 400]);
        file.extend_from_slice(&mdat_box);

        // moov
        let mut moov_payload = Vec::new();
        moov_payload.extend_from_slice(&Mp4BoxBuilder::make_mvhd(1000, 4000));

        let mut tkhd_payload = vec![0u8; 84];
        tkhd_payload[12..16].copy_from_slice(&1u32.to_be_bytes());
        tkhd_payload[76..80].copy_from_slice(&(1920u32 << 16).to_be_bytes());
        tkhd_payload[80..84].copy_from_slice(&(1080u32 << 16).to_be_bytes());
        let tkhd_box = Mp4BoxBuilder::make_box(b"tkhd", &tkhd_payload);

        let mut mdhd_payload = vec![0u8; 24];
        mdhd_payload[12..16].copy_from_slice(&1000u32.to_be_bytes());
        mdhd_payload[16..20].copy_from_slice(&4000u32.to_be_bytes());
        let mdhd_box = Mp4BoxBuilder::make_box(b"mdhd", &mdhd_payload);

        let mut hdlr_payload = vec![0u8; 24];
        hdlr_payload[8..12].copy_from_slice(b"vide");
        let hdlr_box = Mp4BoxBuilder::make_box(b"hdlr", &hdlr_payload);

        // stsd
        let mut entry_body = vec![0u8; 80];
        entry_body[24..26].copy_from_slice(&1920u16.to_be_bytes());
        entry_body[26..28].copy_from_slice(&1080u16.to_be_bytes());
        let mut stsd_entry = Vec::new();
        stsd_entry.extend_from_slice(&((8 + entry_body.len()) as u32).to_be_bytes());
        stsd_entry.extend_from_slice(b"avc1");
        stsd_entry.extend_from_slice(&entry_body);
        let mut stsd_payload = Vec::new();
        stsd_payload.extend_from_slice(&0u32.to_be_bytes());
        stsd_payload.extend_from_slice(&1u32.to_be_bytes());
        stsd_payload.extend_from_slice(&stsd_entry);
        let stsd_box = Mp4BoxBuilder::make_box(b"stsd", &stsd_payload);

        // stts: 4 samples with 1000 ticks each
        let mut stts_payload = Vec::new();
        stts_payload.extend_from_slice(&0u32.to_be_bytes());
        stts_payload.extend_from_slice(&1u32.to_be_bytes());
        stts_payload.extend_from_slice(&4u32.to_be_bytes());
        stts_payload.extend_from_slice(&1000u32.to_be_bytes());
        let stts_box = Mp4BoxBuilder::make_box(b"stts", &stts_payload);

        // stsc: 1 sample per chunk
        let mut stsc_payload = Vec::new();
        stsc_payload.extend_from_slice(&0u32.to_be_bytes());
        stsc_payload.extend_from_slice(&1u32.to_be_bytes());
        stsc_payload.extend_from_slice(&1u32.to_be_bytes());
        stsc_payload.extend_from_slice(&1u32.to_be_bytes());
        stsc_payload.extend_from_slice(&1u32.to_be_bytes());
        let stsc_box = Mp4BoxBuilder::make_box(b"stsc", &stsc_payload);

        // stsz: uniform 100 bytes
        let mut stsz_payload = Vec::new();
        stsz_payload.extend_from_slice(&0u32.to_be_bytes());
        stsz_payload.extend_from_slice(&100u32.to_be_bytes()); // uniform size 100
        stsz_payload.extend_from_slice(&4u32.to_be_bytes()); // 4 samples
        let stsz_box = Mp4BoxBuilder::make_box(b"stsz", &stsz_payload);

        // stco: 4 chunk offsets
        let mut stco_payload = Vec::new();
        stco_payload.extend_from_slice(&0u32.to_be_bytes());
        stco_payload.extend_from_slice(&4u32.to_be_bytes());
        for i in 0..4 {
            stco_payload.extend_from_slice(&(1008u32 + i * 100).to_be_bytes());
        }
        let stco_box = Mp4BoxBuilder::make_box(b"stco", &stco_payload);

        // NOTICE: NO STSS BOX AT ALL!
        let mut stbl_payload = Vec::new();
        stbl_payload.extend_from_slice(&stsd_box);
        stbl_payload.extend_from_slice(&stts_box);
        stbl_payload.extend_from_slice(&stsc_box);
        stbl_payload.extend_from_slice(&stsz_box);
        stbl_payload.extend_from_slice(&stco_box);
        let stbl_box = Mp4BoxBuilder::make_box(b"stbl", &stbl_payload);

        let minf_box = Mp4BoxBuilder::make_box(b"minf", &stbl_box);
        let mut mdia_payload = Vec::new();
        mdia_payload.extend_from_slice(&mdhd_box);
        mdia_payload.extend_from_slice(&hdlr_box);
        mdia_payload.extend_from_slice(&minf_box);
        let mdia_box = Mp4BoxBuilder::make_box(b"mdia", &mdia_payload);

        let mut trak_payload = Vec::new();
        trak_payload.extend_from_slice(&tkhd_box);
        trak_payload.extend_from_slice(&mdia_box);
        let trak_box = Mp4BoxBuilder::make_box(b"trak", &trak_payload);

        moov_payload.extend_from_slice(&trak_box);
        let moov_box = Mp4BoxBuilder::make_box(b"moov", &moov_payload);
        file.extend_from_slice(&moov_box);
        file
    };

    let no_stss_file = make_no_stss_mp4();
    let cursor_no_stss = Cursor::new(no_stss_file);
    let mut demuxer_no_stss = IsoBmffDemuxer::new();
    demuxer_no_stss.init(Box::new(cursor_no_stss)).expect("init no-stss failed");

    // All samples are keyframes!
    // Seeking to 2000ms lands exactly at 2000ms (sample 2)!
    let landed = demuxer_no_stss.seek_to(2000).unwrap();
    assert_eq!(landed, 2000, "With no stss, seek to 2000ms must land on sample 2 (2000ms)");
    let p = demuxer_no_stss.read_packet().unwrap().expect("packet");
    assert_eq!(p.pts_ms, 2000);
    assert!(p.is_keyframe, "Sample with no stss must be keyframe");

    // Seeking to 1500ms lands at 1000ms (sample 1)!
    let landed = demuxer_no_stss.seek_to(1500).unwrap();
    assert_eq!(landed, 1000, "With no stss, seek to 1500ms must land on sample 1 (1000ms)");
    let p = demuxer_no_stss.read_packet().unwrap().expect("packet");
    assert_eq!(p.pts_ms, 1000);
    assert!(p.is_keyframe);

    // Seeking to 3000ms lands on 3000ms!
    let landed = demuxer_no_stss.seek_to(3000).unwrap();
    assert_eq!(landed, 3000);
    let p = demuxer_no_stss.read_packet().unwrap().expect("packet");
    assert_eq!(p.pts_ms, 3000);
    assert!(p.is_keyframe);

    // 5.3 Seeking on 0-sample container returns target_pts_ms without error
    let cur_zero = Cursor::new(SYNTHETIC_MP4_VECTOR.to_vec());
    let mut demuxer_zero = IsoBmffDemuxer::new();
    demuxer_zero.init(Box::new(cur_zero)).unwrap();
    assert_eq!(demuxer_zero.seek_to(12345).unwrap(), 12345);
}

// ===========================================================================
// 6. ADVERSARIAL CHALLENGE: UNEXPECTED STREAM CORRUPTIONS & EDGE CASES
// ===========================================================================
#[test]
fn test_adversarial_stream_corruptions() {
    // 6.1 Probe on truncated/empty stream returns false without panic
    let mut empty_cur = Cursor::new(vec![]);
    assert!(!IsoBmffDemuxer::probe(&mut empty_cur).unwrap());

    let mut short_cur = Cursor::new(vec![0u8; 7]);
    assert!(!IsoBmffDemuxer::probe(&mut short_cur).unwrap());

    // 6.2 Probe on non-MP4 bytes (e.g. EBML magic 0x1A45DFA3) returns false
    let mut ebml_cur = Cursor::new(vec![0x1A, 0x45, 0xDF, 0xA3, 0x00, 0x00, 0x00, 0x00]);
    assert!(!IsoBmffDemuxer::probe(&mut ebml_cur).unwrap());

    // 6.3 Timescale 0 in sample timing calculation returns empty vecs (no division by zero)
    let resolver = SampleTableResolver::new();
    let (dts, pts) = resolver.compute_sample_timings(0);
    assert!(dts.is_empty());
    assert!(pts.is_empty());

    // 6.4 Missing stsz size check
    let resolver_no_stsz = SampleTableResolver::new();
    assert!(matches!(
        resolver_no_stsz.get_sample_size(0),
        Err(DemuxError::NotFound(_))
    ));

    // 6.5 Verify parsing of largesize box produced by helper
    let large_mdat = Mp4BoxBuilder::make_largesize_box(b"mdat", &[1, 2, 3, 4]);
    let parsed_large = BoxHeader::parse(&large_mdat).expect("largesize parsing failed");
    assert_eq!(parsed_large.fourcc, FourCc(*b"mdat"));
    assert_eq!(parsed_large.header_size, 16);
    assert_eq!(parsed_large.total_size, 20); // 16 header + 4 payload
}
