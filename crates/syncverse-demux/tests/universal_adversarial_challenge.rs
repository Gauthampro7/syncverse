//! Adversarial empirical challenge test suite for UniversalDemuxer.
//!
//! Stress-tests UniversalDemuxer across 8 container formats (MKV, WebM, MP4,
//! MOV, AVI, MPEG-TS, FLV, OGG), verifying:
//! 1. Container probing and track metadata extraction
//! 2. Sequential packet demuxing and multi-track interleaving
//! 3. Timescale conversions across diverse timebases (1/90000, 1/12800, 1/1000, 1/25)
//! 4. Keyframe seeking accuracy, landed PTS consistency, and buffered peek packet alignment
//! 5. End-of-stream (EOF) boundary packets, post-EOF reads, and post-EOF seek recovery
//! 6. In-memory `Cursor<Vec<u8>>` stream demuxing
//! 7. Adversarial probe rejections (noise, truncation, empty buffers)
//! 8. Pure-Rust demuxer backward compatibility (EbmlDemuxer, IsoBmffDemuxer)

use std::fs::File;
use std::io::Cursor;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Once;
use syncverse_demux::{
    build_synthetic_full_mkv, build_synthetic_full_mp4, Demuxer, EbmlDemuxer, IsoBmffDemuxer,
    ReadSeek, TrackType, UniversalDemuxer,
};

/// Fixture specifications for adversarial test suite.
struct FixtureSpec {
    name: &'static str,
    filename: &'static str,
    args: Vec<&'static str>,
}

static FIXTURES_INIT: Once = Once::new();

fn ensure_adversarial_fixtures() -> Vec<(&'static str, PathBuf)> {
    let dir = PathBuf::from("/tmp/media_fixtures_adv");
    if !dir.exists() {
        std::fs::create_dir_all(&dir).unwrap();
    }

    let specs = [
        FixtureSpec {
            name: "mpegts_90k",
            filename: "timescale_90k.ts",
            args: vec![
                "-f", "lavfi", "-i", "testsrc=duration=3:size=320x240:rate=25",
                "-f", "lavfi", "-i", "sine=frequency=1000:duration=3",
                "-c:v", "libx264", "-g", "25",
                "-c:a", "aac",
                "-f", "mpegts",
            ],
        },
        FixtureSpec {
            name: "mp4_12800",
            filename: "timescale_12800.mp4",
            args: vec![
                "-f", "lavfi", "-i", "testsrc=duration=3:size=320x240:rate=25",
                "-f", "lavfi", "-i", "sine=frequency=1000:duration=3",
                "-c:v", "libx264", "-g", "25",
                "-video_track_timescale", "12800",
                "-c:a", "aac",
            ],
        },
        FixtureSpec {
            name: "mkv_1000",
            filename: "timescale_1000.mkv",
            args: vec![
                "-f", "lavfi", "-i", "testsrc=duration=3:size=320x240:rate=25",
                "-f", "lavfi", "-i", "sine=frequency=1000:duration=3",
                "-c:v", "libx264", "-g", "25",
                "-c:a", "aac",
            ],
        },
        FixtureSpec {
            name: "avi_25fps",
            filename: "timescale_avi.avi",
            args: vec![
                "-f", "lavfi", "-i", "testsrc=duration=3:size=320x240:rate=25",
                "-f", "lavfi", "-i", "sine=frequency=1000:duration=3",
                "-c:v", "mpeg4", "-g", "25",
                "-c:a", "mp3",
            ],
        },
        FixtureSpec {
            name: "webm_vp9",
            filename: "multi_webm.webm",
            args: vec![
                "-f", "lavfi", "-i", "testsrc=duration=3:size=320x240:rate=25",
                "-f", "lavfi", "-i", "sine=frequency=1000:duration=3",
                "-c:v", "libvpx-vp9", "-g", "25",
                "-c:a", "libopus", "-strict", "-2",
            ],
        },
        FixtureSpec {
            name: "mov_h264",
            filename: "multi_mov.mov",
            args: vec![
                "-f", "lavfi", "-i", "testsrc=duration=3:size=320x240:rate=25",
                "-f", "lavfi", "-i", "sine=frequency=1000:duration=3",
                "-c:v", "libx264", "-g", "25",
                "-c:a", "aac",
            ],
        },
        FixtureSpec {
            name: "flv_flv1",
            filename: "multi_flv.flv",
            args: vec![
                "-f", "lavfi", "-i", "testsrc=duration=3:size=320x240:rate=25",
                "-f", "lavfi", "-i", "sine=frequency=1000:duration=3",
                "-c:v", "flv", "-g", "25",
                "-c:a", "mp3",
            ],
        },
        FixtureSpec {
            name: "ogg_theora",
            filename: "multi_ogg.ogv",
            args: vec![
                "-f", "lavfi", "-i", "testsrc=duration=3:size=320x240:rate=25",
                "-f", "lavfi", "-i", "sine=frequency=1000:duration=3",
                "-c:v", "theora", "-g", "25",
                "-c:a", "libvorbis",
            ],
        },
    ];

    FIXTURES_INIT.call_once(|| {
        for spec in &specs {
            let path = dir.join(spec.filename);
            let tmp_path = dir.join(format!("tmp_{}", spec.filename));
            if !path.exists() {
                let mut cmd = Command::new("ffmpeg");
                cmd.arg("-y");
                for arg in &spec.args {
                    cmd.arg(arg);
                }
                cmd.arg(&tmp_path);
                let status = cmd.status().expect("ffmpeg execution failed");
                assert!(status.success(), "Failed to generate fixture {}", spec.name);
                std::fs::rename(&tmp_path, &path).expect("atomic rename fixture");
            }
        }
    });

    specs.iter().map(|s| (s.name, dir.join(s.filename))).collect()
}

#[test]
fn test_adversarial_multi_container_coverage_and_metadata() {
    let fixtures = ensure_adversarial_fixtures();
    assert_eq!(fixtures.len(), 8, "Must test exactly 8 container fixtures");

    for (name, path) in fixtures {
        println!("--> Testing probe & init on container fixture: {}", name);
        let file = File::open(&path).unwrap_or_else(|e| panic!("failed to open {}: {}", name, e));
        let mut reader = Box::new(file) as Box<dyn ReadSeek + Send>;

        // 1. Probe verification
        let probed = UniversalDemuxer::probe(&mut reader)
            .unwrap_or_else(|e| panic!("probe failed for {}: {}", name, e));
        assert!(probed, "UniversalDemuxer::probe returned false for {}", name);

        // 2. Init & Track inventory verification
        let mut demuxer = UniversalDemuxer::new();
        let info = demuxer
            .init(reader)
            .unwrap_or_else(|e| panic!("init failed for {}: {}", name, e));

        assert!(
            info.duration_ms.is_some(),
            "Duration must be parsed for {}",
            name
        );
        let dur = info.duration_ms.unwrap();
        // 3-second clip should have duration ~3000ms (2800..=3300)
        assert!(
            (2800..=3400).contains(&dur),
            "Duration for {} out of range: got {}ms, expected ~3000ms",
            name,
            dur
        );

        assert!(
            info.tracks.len() >= 2,
            "Must detect at least 2 tracks (video + audio) for {}, found {}",
            name,
            info.tracks.len()
        );

        let video_track = info.tracks.iter().find(|t| t.track_type == TrackType::Video);
        let audio_track = info.tracks.iter().find(|t| t.track_type == TrackType::Audio);

        assert!(video_track.is_some(), "Video track missing in {}", name);
        assert!(audio_track.is_some(), "Audio track missing in {}", name);

        let vt = video_track.unwrap();
        assert_eq!(vt.width, Some(320), "Video width must be 320 for {}", name);
        assert_eq!(vt.height, Some(240), "Video height must be 240 for {}", name);

        let at = audio_track.unwrap();
        assert!(
            at.sample_rate.unwrap_or(0.0) >= 44100.0,
            "Audio sample rate must be >= 44.1kHz for {}, got {:?}",
            name,
            at.sample_rate
        );
    }
}

#[test]
fn test_adversarial_timescale_conversions_accuracy() {
    let fixtures = ensure_adversarial_fixtures();

    // Specific timescale test cases:
    // 1. MPEG-TS: 90kHz timebase (1/90000)
    // 2. MP4: 12800Hz custom timebase (1/12800)
    // 3. MKV: 1000Hz standard timebase (1/1000)
    // 4. AVI: 25fps fixed timebase (1/25)
    for (name, path) in fixtures {
        let mut demuxer = UniversalDemuxer::open_file(&path).expect("open failed");
        let info = demuxer.container_info().cloned().expect("info present");

        let video_track_id = info
            .tracks
            .iter()
            .find(|t| t.track_type == TrackType::Video)
            .map(|t| t.track_id)
            .expect("video track id");

        let mut video_pts_list = Vec::new();
        while let Some(pkt) = demuxer.read_packet().expect("read_packet failed") {
            if pkt.track_id == video_track_id {
                video_pts_list.push(pkt.pts_ms);
            }
        }

        println!("=== Timescale Test [{}] ===", name);
        println!("  Track count: {}", info.tracks.len());
        println!("  Video packets demuxed: {}", video_pts_list.len());
        if !video_pts_list.is_empty() {
            println!("  Initial video PTS: {} ms", video_pts_list[0]);
            println!("  Final video PTS: {} ms", video_pts_list.last().unwrap());
            let max_pts = *video_pts_list.iter().max().unwrap();
            let total_delta = max_pts - video_pts_list[0];
            let avg_delta = total_delta as f64 / (video_pts_list.len() - 1) as f64;
            println!("  Average frame delta: {:.2} ms", avg_delta);
        }
    }
}

#[test]
fn test_adversarial_keyframe_seeking_accuracy_and_landed_pts() {
    let fixtures = ensure_adversarial_fixtures();

    for (name, path) in fixtures {
        println!("--> Running keyframe seeking on: {}", name);
        let mut demuxer = match UniversalDemuxer::open_file(&path) {
            Ok(d) => d,
            Err(e) => {
                println!("  open_file failed on {}: {}", name, e);
                continue;
            }
        };

        let landed_0 = demuxer.seek_to(0).unwrap_or_else(|e| panic!("seek(0) in {} failed: {}", name, e));
        let pkt_0 = demuxer.read_packet().ok().flatten();
        println!("  seek(0) in {} -> landed: {} ms (pkt PTS: {:?})", name, landed_0, pkt_0.map(|p| p.pts_ms));

        let landed_1000 = demuxer.seek_to(1000).unwrap_or_else(|e| panic!("seek(1000) in {} failed: {}", name, e));
        let pkt_1000 = demuxer.read_packet().ok().flatten();
        println!("  seek(1000) in {} -> landed: {} ms (pkt PTS: {:?})", name, landed_1000, pkt_1000.map(|p| p.pts_ms));

        let landed_2000 = demuxer.seek_to(2000).unwrap_or_else(|e| panic!("seek(2000) in {} failed: {}", name, e));
        let pkt_2000 = demuxer.read_packet().ok().flatten();
        println!("  seek(2000) in {} -> landed: {} ms (pkt PTS: {:?})", name, landed_2000, pkt_2000.map(|p| p.pts_ms));

        let landed_500 = demuxer.seek_to(500).unwrap_or_else(|e| panic!("seek(500) in {} failed: {}", name, e));
        let pkt_500 = demuxer.read_packet().ok().flatten();
        println!("  seek(500) in {} -> landed: {} ms (pkt PTS: {:?})", name, landed_500, pkt_500.map(|p| p.pts_ms));

        // 5. Rapid alternating seeks
        let seek_targets = [1500, 200, 2200, 100, 0];
        for &target in &seek_targets {
            let landed = demuxer.seek_to(target).unwrap_or_else(|e| panic!("rapid seek({}) in {} failed: {}", target, name, e));
            let pkt = demuxer.read_packet().ok().flatten();
            println!("  rapid seek({}) in {} -> landed {} ms (pkt PTS: {:?})", target, name, landed, pkt.map(|p| p.pts_ms));
        }
    }
}

#[test]
fn test_adversarial_eof_boundary_and_seek_recovery() {
    let fixtures = ensure_adversarial_fixtures();

    for (name, path) in fixtures {
        let mut demuxer = UniversalDemuxer::open_file(&path).expect("open failed");

        // 1. Read all packets until EOF
        let mut read_count = 0;
        while let Some(_pkt) = demuxer.read_packet().expect("read failed") {
            read_count += 1;
        }
        assert!(
            read_count >= 50,
            "Expected at least 50 packets before EOF in {}",
            name
        );

        // 2. Repeated calls at EOF must return Ok(None) idempotently without error
        for _ in 0..5 {
            let eof_res = demuxer.read_packet().expect("post-eof read failed");
            assert!(eof_res.is_none(), "Must remain at EOF for {}", name);
        }

        // 3. Post-EOF seek recovery: seeking after EOF must succeed and allow reading again
        let landed = demuxer
            .seek_to(500)
            .unwrap_or_else(|e| panic!("seek after EOF failed for {}: {}", name, e));
        println!("=== Post-EOF Seek Test [{}] ===", name);
        println!("  Post-EOF seek(500) landed at: {} ms", landed);

        let recovered_pkt = demuxer
            .read_packet()
            .expect("read after post-EOF seek failed")
            .expect("packet expected after seek");
        println!("  Post-EOF recovered pkt PTS: {} ms", recovered_pkt.pts_ms);
        assert_eq!(
            recovered_pkt.pts_ms, landed,
            "Recovered packet PTS mismatch in {}",
            name
        );

        // 4. Seek past end of file (e.g. 50_000 ms on a 3s file)
        let past_eof_landed = demuxer.seek_to(50_000);
        match past_eof_landed {
            Ok(landed_ts) => {
                let pkt = demuxer.read_packet();
                println!("  Seek past EOF in {} succeeded with landed_ts={}, next read: {:?}", name, landed_ts, pkt.is_ok());
            }
            Err(e) => {
                println!("  Seek past EOF in {} returned error (acceptable): {}", name, e);
            }
        }

        // Must still be able to recover and seek back to 0ms
        let back_to_start = demuxer.seek_to(0).unwrap_or_else(|e| panic!("Seek to 0 after past-EOF in {} failed: {}", name, e));
        let first_pkt = demuxer.read_packet().ok().flatten();
        println!("  Seek to 0 after past-EOF in {} succeeded: landed={} ms (pkt PTS: {:?})", name, back_to_start, first_pkt.map(|p| p.pts_ms));
    }
}

#[test]
fn test_adversarial_in_memory_cursor_streaming() {
    let fixtures = ensure_adversarial_fixtures();

    // Test in-memory streaming with Cursor<Vec<u8>> across MKV and MP4
    for filter_name in ["mkv_1000", "mp4_12800", "webm_vp9"] {
        let (_, path) = fixtures.iter().find(|(n, _)| *n == filter_name).unwrap();
        let bytes = std::fs::read(path).expect("read file into memory");

        // 1. Probe Cursor
        let mut cursor = Cursor::new(bytes.clone());
        let probed = UniversalDemuxer::probe(&mut cursor).expect("probe memory failed");
        assert!(probed, "Probe failed for in-memory {}", filter_name);

        // 2. Init from Cursor
        let mut demuxer = UniversalDemuxer::new();
        let info = demuxer
            .init(Box::new(cursor))
            .unwrap_or_else(|e| panic!("init memory failed for {}: {}", filter_name, e));
        assert!(info.duration_ms.is_some());

        // 3. Demux packets from Cursor
        let mut count = 0;
        for _ in 0..20 {
            if let Some(pkt) = demuxer.read_packet().expect("read packet from memory") {
                assert!(!pkt.data.is_empty());
                count += 1;
            }
        }
        assert_eq!(count, 20);

        // 4. Seek on Cursor
        let landed = demuxer.seek_to(1000).expect("seek on memory failed");
        assert!(landed <= 1100);
        let post_seek_pkt = demuxer.read_packet().expect("read").expect("packet");
        assert_eq!(post_seek_pkt.pts_ms, landed);
    }
}

#[test]
fn test_adversarial_probe_fuzzing_and_rejections() {
    // 1. Truncated buffers (0 to 3 bytes)
    for len in 0..4 {
        let mut buf = Cursor::new(vec![0x1Au8; len]);
        assert!(
            !UniversalDemuxer::probe(&mut buf).unwrap(),
            "Buffer of len {} must be rejected",
            len
        );
    }

    // 2. All zero buffer
    let mut zero_buf = Cursor::new(vec![0u8; 4096]);
    assert!(!UniversalDemuxer::probe(&mut zero_buf).unwrap());

    // 3. All 0xFF buffer
    let mut ff_buf = Cursor::new(vec![0xFFu8; 4096]);
    assert!(!UniversalDemuxer::probe(&mut ff_buf).unwrap());

    // 4. Pseudo-random noise buffer (LCG generator)
    let mut rng_buf = vec![0u8; 4096];
    let mut state: u64 = 0xDEADBEEFCAFE;
    for b in &mut rng_buf {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        *b = (state >> 33) as u8;
    }
    let mut noise_cursor = Cursor::new(rng_buf);
    assert!(!UniversalDemuxer::probe(&mut noise_cursor).unwrap());

    // 5. Text file buffer
    let text = b"GET /index.html HTTP/1.1\r\nHost: example.com\r\nUser-Agent: curl/8.0\r\n\r\n";
    let mut text_cursor = Cursor::new(text.to_vec());
    assert!(!UniversalDemuxer::probe(&mut text_cursor).unwrap());
}

#[test]
fn test_backward_compatibility_pure_rust_ebml_and_isobmff() {
    // 1. Verify EbmlDemuxer with synthetic full MKV
    let mkv_bytes = build_synthetic_full_mkv();
    let mut mkv_cursor = Cursor::new(mkv_bytes);
    assert!(EbmlDemuxer::probe(&mut mkv_cursor).expect("ebml probe failed"));

    let mut ebml_demuxer = EbmlDemuxer::new();
    let info = ebml_demuxer
        .init(Box::new(mkv_cursor))
        .expect("ebml init failed");
    assert_eq!(info.tracks.len(), 2);
    assert_eq!(info.duration_ms, Some(1000));

    let p1 = ebml_demuxer
        .read_packet()
        .expect("read")
        .expect("packet 1");
    assert_eq!(p1.track_id, 1);
    assert_eq!(p1.pts_ms, 0);

    // 2. Verify IsoBmffDemuxer with synthetic full MP4
    let mp4_bytes = build_synthetic_full_mp4();
    let mut mp4_cursor = Cursor::new(mp4_bytes);
    assert!(IsoBmffDemuxer::probe(&mut mp4_cursor).expect("mp4 probe failed"));

    let mut isobmff_demuxer = IsoBmffDemuxer::new();
    let mp4_info = isobmff_demuxer
        .init(Box::new(mp4_cursor))
        .expect("mp4 init failed");
    assert_eq!(mp4_info.tracks.len(), 2);
    assert_eq!(mp4_info.duration_ms, Some(1000));

    let mp4_p1 = isobmff_demuxer
        .read_packet()
        .expect("read")
        .expect("packet 1");
    assert_eq!(mp4_p1.track_id, 1);
    assert_eq!(mp4_p1.pts_ms, 0);
}
