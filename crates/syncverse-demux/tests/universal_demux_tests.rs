//! Integration and verification tests for UniversalDemuxer.
//!
//! Verifies container format probing, metadata extraction, sequential packet
//! reading with millisecond timescale conversions, and keyframe seeking across
//! all required container formats: MKV/WebM, MP4/MOV, AVI, MPEG-TS, FLV, OGG.

use std::fs::File;
use std::io::Cursor;
use std::path::PathBuf;
use syncverse_demux::{Demuxer, ReadSeek, TrackType, UniversalDemuxer};

use std::sync::Mutex;
static FIXTURE_LOCK: Mutex<()> = Mutex::new(());

fn ensure_fixtures() -> Vec<(&'static str, PathBuf)> {
    let _guard = FIXTURE_LOCK.lock().unwrap();
    let dir = PathBuf::from("/tmp/media_fixtures");
    if !dir.exists() {
        std::fs::create_dir_all(&dir).unwrap();
    }

    let specs = [
        ("mkv", "sample.mkv", vec!["-f", "lavfi", "-i", "testsrc=duration=1:size=320x240:rate=25", "-f", "lavfi", "-i", "sine=frequency=1000:duration=1", "-c:v", "libx264", "-c:a", "aac"]),
        ("webm", "sample.webm", vec!["-f", "lavfi", "-i", "testsrc=duration=1:size=320x240:rate=25", "-f", "lavfi", "-i", "sine=frequency=1000:duration=1", "-c:v", "libvpx", "-c:a", "libvorbis"]),
        ("mp4", "sample.mp4", vec!["-f", "lavfi", "-i", "testsrc=duration=1:size=320x240:rate=25", "-f", "lavfi", "-i", "sine=frequency=1000:duration=1", "-c:v", "libx264", "-c:a", "aac"]),
        ("mov", "sample.mov", vec!["-f", "lavfi", "-i", "testsrc=duration=1:size=320x240:rate=25", "-f", "lavfi", "-i", "sine=frequency=1000:duration=1", "-c:v", "libx264", "-c:a", "aac"]),
        ("avi", "sample.avi", vec!["-f", "lavfi", "-i", "testsrc=duration=1:size=320x240:rate=25", "-f", "lavfi", "-i", "sine=frequency=1000:duration=1", "-c:v", "mpeg4", "-c:a", "mp3"]),
        ("ts", "sample.ts", vec!["-f", "lavfi", "-i", "testsrc=duration=1:size=320x240:rate=25", "-f", "lavfi", "-i", "sine=frequency=1000:duration=1", "-c:v", "libx264", "-c:a", "aac"]),
        ("flv", "sample.flv", vec!["-f", "lavfi", "-i", "testsrc=duration=1:size=320x240:rate=25", "-f", "lavfi", "-i", "sine=frequency=1000:duration=1", "-c:v", "flv", "-c:a", "mp3"]),
        ("ogv", "sample.ogv", vec!["-f", "lavfi", "-i", "testsrc=duration=1:size=320x240:rate=25", "-f", "lavfi", "-i", "sine=frequency=1000:duration=1", "-c:v", "theora", "-c:a", "libvorbis"]),
    ];

    let mut fixtures = Vec::new();
    for (name, filename, args) in specs {
        let path = dir.join(filename);
        if !path.exists() {
            let tmp_path = dir.join(format!("{}.tmp", filename));
            let mut cmd = std::process::Command::new("ffmpeg");
            cmd.arg("-y");
            for arg in args {
                cmd.arg(arg);
            }
            cmd.arg(&tmp_path);
            let status = cmd.status().expect("Failed to execute ffmpeg to generate test fixture");
            assert!(status.success(), "Failed to generate fixture for {}", name);
            std::fs::rename(&tmp_path, &path).expect("Failed to move fixture to destination");
        }
        fixtures.push((name, path));
    }
    fixtures
}

#[test]
fn test_universal_probe_and_metadata_across_all_containers() {
    let fixtures = ensure_fixtures();

    for (name, path) in fixtures {
        let file = File::open(&path).expect("failed to open fixture");
        let mut reader = Box::new(file) as Box<dyn ReadSeek + Send>;

        // 1. Probe
        let is_match = UniversalDemuxer::probe(&mut reader)
            .unwrap_or_else(|e| panic!("probe failed for {}: {}", name, e));
        assert!(is_match, "UniversalDemuxer::probe must match {}", name);

        // 2. Init
        let mut demuxer = UniversalDemuxer::new();
        let info = demuxer
            .init(reader)
            .unwrap_or_else(|e| panic!("init failed for {}: {}", name, e));

        assert!(
            info.duration_ms.is_some(),
            "Duration must be parsed for {}",
            name
        );
        let duration = info.duration_ms.unwrap();
        assert!(
            (900..=1200).contains(&duration),
            "Duration for {} should be ~1000ms, got {}ms",
            name,
            duration
        );

        assert!(
            info.tracks.len() >= 2,
            "Must detect at least 2 tracks (video + audio) for {}, found {}",
            name,
            info.tracks.len()
        );

        let has_video = info.tracks.iter().any(|t| t.track_type == TrackType::Video);
        let has_audio = info.tracks.iter().any(|t| t.track_type == TrackType::Audio);
        assert!(has_video, "Must have video track for {}", name);
        assert!(has_audio, "Must have audio track for {}", name);
    }
}

#[test]
fn test_universal_sequential_packet_demuxing() {
    let fixtures = ensure_fixtures();

    for (name, path) in fixtures {
        let demuxer = UniversalDemuxer::open_file(&path)
            .unwrap_or_else(|e| panic!("open_file failed for {}: {}", name, e));
        let mut demuxer = demuxer;

        let mut packet_count = 0;
        let mut video_packet_count = 0;
        let mut audio_packet_count = 0;

        let info = demuxer.container_info().cloned().expect("info present");
        let video_track_id = info
            .tracks
            .iter()
            .find(|t| t.track_type == TrackType::Video)
            .map(|t| t.track_id);
        let audio_track_id = info
            .tracks
            .iter()
            .find(|t| t.track_type == TrackType::Audio)
            .map(|t| t.track_id);

        while let Some(packet) = demuxer.read_packet().expect("read_packet failed") {
            assert!(
                !packet.data.is_empty(),
                "Packet data must not be empty for {}",
                name
            );
            if Some(packet.track_id) == video_track_id {
                video_packet_count += 1;
            } else if Some(packet.track_id) == audio_track_id {
                audio_packet_count += 1;
            }
            packet_count += 1;
        }

        assert!(
            packet_count >= 20,
            "Expected at least 20 packets for 1-second clip in {}, got {}",
            name,
            packet_count
        );
        assert!(
            video_packet_count > 0,
            "Must demux video packets for {}",
            name
        );
        assert!(
            audio_packet_count > 0,
            "Must demux audio packets for {}",
            name
        );
    }
}

#[test]
fn test_universal_keyframe_seeking() {
    let fixtures = ensure_fixtures();

    for (name, path) in fixtures {
        let mut demuxer = UniversalDemuxer::open_file(&path).expect("open failed");

        // Seek to 500ms
        let landed_pts = demuxer.seek_to(500).expect("seek failed");
        assert!(
            landed_pts <= 1500,
            "Landed PTS must be within 1.5s for {}, got {}",
            name,
            landed_pts
        );

        // First packet after seek must exist
        let post_seek_pkt = demuxer
            .read_packet()
            .expect("read post-seek failed")
            .expect("packet expected after seek");
        assert!(
            !post_seek_pkt.data.is_empty(),
            "Post-seek packet payload must not be empty"
        );
    }
}

#[test]
fn test_universal_probe_adversarial_rejections() {
    // 1. Truncated buffer (< 4 bytes)
    let mut short_buf = Cursor::new(vec![0x1A, 0x45]);
    assert!(!UniversalDemuxer::probe(&mut short_buf).unwrap());

    // 2. Arbitrary random text bytes
    let mut text_buf = Cursor::new(b"Hello world, this is a plain text file not media!".to_vec());
    assert!(!UniversalDemuxer::probe(&mut text_buf).unwrap());

    // 3. Zero-filled 1024-byte buffer
    let mut zero_buf = Cursor::new(vec![0u8; 1024]);
    assert!(!UniversalDemuxer::probe(&mut zero_buf).unwrap());
}
