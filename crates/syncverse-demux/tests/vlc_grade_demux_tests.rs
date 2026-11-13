//! Integration tests for VLC-grade UniversalDemuxer enhancements, dispositions,
//! stream discard filtering, microsecond PTS precision, and stream-targeted keyframe seeking.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use syncverse_demux::{
    build_synthetic_full_mp4, Demuxer, EbmlDemuxer, IsoBmffDemuxer, TrackInfo, TrackType,
    UniversalDemuxer,
};

static TEST_DIR: OnceLock<PathBuf> = OnceLock::new();

fn test_dir() -> &'static Path {
    TEST_DIR
        .get_or_init(|| {
            let dir = std::env::temp_dir().join("syncverse_vlc_demux_tests");
            std::fs::create_dir_all(&dir).expect("create temp test dir");
            dir
        })
        .as_path()
}

fn run_ffmpeg(args: &[&str], out_path: &Path) {
    if out_path.exists() {
        return;
    }
    let tmp_path = out_path.with_extension("tmp.mkv");
    let tmp_path = if let Some(ext) = out_path.extension().and_then(|e| e.to_str()) {
        out_path.with_file_name(format!(
            "tmp_{}.{}",
            out_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("out"),
            ext
        ))
    } else {
        tmp_path
    };
    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y").args(args).arg(&tmp_path);
    let output = cmd.output().expect("execute ffmpeg");
    assert!(
        output.status.success(),
        "ffmpeg failed for {:?}: {}",
        out_path,
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::rename(&tmp_path, out_path).expect("rename fixture");
}

#[test]
fn test_track_info_default_and_ebml_disposition_flags() {
    let default_info = TrackInfo::default();
    assert_eq!(default_info.track_type, TrackType::Unknown);
    assert!(!default_info.is_default);
    assert!(!default_info.is_forced);
    assert_eq!(default_info.track_id, 0);

    // Build a minimal synthetic EBML MKV track entry with FlagDefault (0x88 = 1) and FlagForced (0x55AA = 1)
    let mut track_entry = Vec::new();
    // TrackNumber (0xD7) = 3
    track_entry.extend_from_slice(&[0xD7, 0x81, 0x03]);
    // TrackType (0x83) = 17 (Subtitle)
    track_entry.extend_from_slice(&[0x83, 0x81, 0x11]);
    // FlagDefault (0x88) = 1
    track_entry.extend_from_slice(&[0x88, 0x81, 0x01]);
    // FlagForced (0x55AA) = 1
    track_entry.extend_from_slice(&[0x55, 0xAA, 0x81, 0x01]);
    // CodecID (0x86) = "S_TEXT/UTF8"
    let codec = b"S_TEXT/UTF8";
    track_entry.push(0x86);
    track_entry.push(0x80 | (codec.len() as u8));
    track_entry.extend_from_slice(codec);

    let mut tracks_elem = vec![0xAE, 0x80 | (track_entry.len() as u8)];
    tracks_elem.extend_from_slice(&track_entry);

    let mut seg_body = Vec::new();
    // Tracks (0x1654AE6B)
    seg_body.extend_from_slice(&[0x16, 0x54, 0xAE, 0x6B, 0x80 | (tracks_elem.len() as u8)]);
    seg_body.extend_from_slice(&tracks_elem);

    // EBML Header
    let ebml_body = [
        0x42, 0x86, 0x81, 0x01, 0x42, 0x82, 0x88, b'm', b'a', b't', b'r', b'o', b's', b'k', b'a',
    ];
    let mut mkv = vec![0x1A, 0x45, 0xDF, 0xA3, 0x80 | (ebml_body.len() as u8)];
    mkv.extend_from_slice(&ebml_body);
    mkv.extend_from_slice(&[0x18, 0x53, 0x80, 0x67, 0x80 | (seg_body.len() as u8)]);
    mkv.extend_from_slice(&seg_body);

    let mut demuxer = EbmlDemuxer::new();
    let info = demuxer.init(Box::new(Cursor::new(mkv))).expect("ebml init");
    assert_eq!(info.tracks.len(), 1);
    assert_eq!(info.tracks[0].track_type, TrackType::Subtitle);
    assert!(info.tracks[0].is_default);
    assert!(info.tracks[0].is_forced);
}

#[test]
fn test_attached_pic_cover_art_is_skipped_and_discarded() {
    let mp4_path = test_dir().join("cover_art_v2.mp4");
    run_ffmpeg(
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=1:size=320x240:rate=25",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=1",
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:size=64x64:d=0.04",
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-map",
            "2:v",
            "-c:v:0",
            "libx264",
            "-c:a",
            "aac",
            "-c:v:1",
            "mjpeg",
            "-disposition:v:1",
            "attached_pic",
        ],
        &mp4_path,
    );

    let mut demuxer = UniversalDemuxer::open_file(&mp4_path).expect("open cover_art_v2.mp4");
    let info = demuxer.container_info().expect("info").clone();

    let video_tracks: Vec<_> = info
        .tracks
        .iter()
        .filter(|t| t.track_type == TrackType::Video)
        .collect();
    assert_eq!(
        video_tracks.len(),
        1,
        "Cover art attached_pic stream must NOT be listed as a Video track: {:?}",
        info.tracks
    );
    assert_eq!(video_tracks[0].width, Some(320));
    assert_eq!(video_tracks[0].height, Some(240));

    let audio_tracks: Vec<_> = info
        .tracks
        .iter()
        .filter(|t| t.track_type == TrackType::Audio)
        .collect();
    assert_eq!(audio_tracks.len(), 1);
    assert_eq!(audio_tracks[0].channels, Some(1));

    // Even after calling set_active_streams, attached_pic stream must stay discarded
    demuxer.set_active_streams(
        Some(video_tracks[0].track_id),
        Some(audio_tracks[0].track_id),
        false,
    );

    // Ensure no packets are emitted for any stream ID outside the two active tracks
    let valid_ids: Vec<u64> = info.tracks.iter().map(|t| t.track_id).collect();
    let mut seen_count = 0usize;
    while let Some(pkt) = demuxer.read_packet().expect("read_packet") {
        assert!(
            valid_ids.contains(&pkt.track_id),
            "Received packet for skipped/attached_pic stream {}",
            pkt.track_id
        );
        seen_count += 1;
    }
    assert!(seen_count >= 50, "expected >= 50 packets, got {}", seen_count);
}

#[test]
fn test_dispositions_active_streams_discard_and_subtitle_params() {
    let srt_path = test_dir().join("sample.srt");
    if !srt_path.exists() {
        std::fs::write(
            &srt_path,
            "1\n00:00:00,000 --> 00:00:01,500\nForced hello line\n",
        )
        .expect("write srt");
    }

    let mkv_path = test_dir().join("multi_track_disp.mkv");
    let srt_str = srt_path.to_str().expect("utf8 path");
    run_ffmpeg(
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=2:size=320x240:rate=25",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=2",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=880:duration=2",
            "-f",
            "srt",
            "-i",
            srt_str,
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-map",
            "2:a",
            "-map",
            "3:s",
            "-c:v",
            "libx264",
            "-g",
            "25",
            "-c:a",
            "aac",
            "-c:s",
            "subrip",
            "-disposition:v:0",
            "default",
            "-disposition:a:0",
            "default",
            "-disposition:a:1",
            "0",
            "-disposition:s:0",
            "default+forced",
            "-metadata:s:a:0",
            "language=eng",
            "-metadata:s:a:1",
            "language=jpn",
        ],
        &mkv_path,
    );

    let mut demuxer = UniversalDemuxer::open_file(&mkv_path).expect("open multi_track_disp.mkv");
    let info = demuxer.container_info().expect("info").clone();

    let v_track = info
        .tracks
        .iter()
        .find(|t| t.track_type == TrackType::Video)
        .expect("video track");
    let audio_tracks: Vec<_> = info
        .tracks
        .iter()
        .filter(|t| t.track_type == TrackType::Audio)
        .collect();
    assert_eq!(audio_tracks.len(), 2);
    let s_track = info
        .tracks
        .iter()
        .find(|t| t.track_type == TrackType::Subtitle)
        .expect("subtitle track");

    assert!(v_track.is_default);
    assert!(audio_tracks[0].is_default);
    assert!(!audio_tracks[0].is_forced);
    assert!(!audio_tracks[1].is_default);
    assert!(s_track.is_default);
    assert!(s_track.is_forced);

    // Subtitle & general stream params helpers
    assert!(demuxer.subtitle_stream_params(s_track.track_id).is_some());
    assert!(demuxer.subtitle_stream_params(v_track.track_id).is_none());
    assert!(demuxer.stream_params(v_track.track_id).is_some());
    assert!(demuxer.stream_params(999).is_none());

    // Activate only video + second audio track, discarding first audio and subtitles
    demuxer.set_active_streams(Some(v_track.track_id), Some(audio_tracks[1].track_id), false);

    let mut v_count = 0usize;
    let mut a2_count = 0usize;
    while let Some(pkt) = demuxer.read_packet().expect("read_packet") {
        assert_ne!(
            pkt.track_id, audio_tracks[0].track_id,
            "Discarded audio track 0 should not emit packets"
        );
        assert_ne!(
            pkt.track_id, s_track.track_id,
            "Discarded subtitle track should not emit packets"
        );
        if pkt.track_id == v_track.track_id {
            v_count += 1;
        } else if pkt.track_id == audio_tracks[1].track_id {
            a2_count += 1;
        }
    }
    assert!(v_count > 10);
    assert!(a2_count > 10);

    // Re-enable subtitles and seek back to 0: verify subtitle packet can be read when keep_subtitles = true
    demuxer.set_active_streams(Some(v_track.track_id), Some(audio_tracks[1].track_id), true);
    let landed = demuxer.seek_to(0).expect("seek to 0");
    assert!(landed <= 500);
    let mut saw_sub = false;
    while let Some(pkt) = demuxer.read_packet().expect("read_packet") {
        if pkt.track_id == s_track.track_id {
            saw_sub = true;
        }
    }
    assert!(saw_sub, "Subtitle packet should be demuxed when keep_subtitles=true");
}

#[test]
fn test_pts_us_precision_and_stream_targeted_keyframe_seek() {
    let mp4_path = test_dir().join("ntsc_seek.mp4");
    run_ffmpeg(
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=3:size=320x240:rate=30000/1001",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=1000:duration=3",
            "-c:v",
            "libx264",
            "-g",
            "30",
            "-video_track_timescale",
            "90000",
            "-c:a",
            "aac",
        ],
        &mp4_path,
    );

    let mut demuxer = UniversalDemuxer::open_file(&mp4_path).expect("open ntsc_seek.mp4");
    let info = demuxer.container_info().expect("info").clone();
    let v_id = info
        .tracks
        .iter()
        .find(|t| t.track_type == TrackType::Video)
        .expect("video")
        .track_id;

    let mut saw_sub_ms_precision = false;
    for _ in 0..30 {
        let pkt = demuxer
            .read_packet()
            .expect("read")
            .expect("packet");
        assert_eq!(pkt.pts_ms, (pkt.pts_us.max(0) as u64) / 1000);
        if pkt.track_id == v_id && pkt.pts_us % 1000 != 0 {
            saw_sub_ms_precision = true;
        }
    }
    assert!(
        saw_sub_ms_precision,
        "29.97fps stream must produce non-millisecond-aligned pts_us values"
    );

    // Seek to 1500ms targeting video stream and verify post-seek packet is preserved (not dropped)
    let landed_ms = demuxer.seek_to(1500).expect("seek_to(1500)");
    assert!(landed_ms <= 1600, "landed_ms={} too high", landed_ms);

    let first_post_seek = demuxer
        .read_packet()
        .expect("read post-seek")
        .expect("first post-seek packet must not be dropped");
    assert_eq!(first_post_seek.pts_ms, landed_ms);

    // Verify we receive a video keyframe near landed_ms
    let mut found_video_keyframe = first_post_seek.track_id == v_id && first_post_seek.is_keyframe;
    if !found_video_keyframe {
        for _ in 0..10 {
            if let Some(pkt) = demuxer.read_packet().expect("read") {
                if pkt.track_id == v_id && pkt.is_keyframe {
                    found_video_keyframe = true;
                    break;
                }
            }
        }
    }
    assert!(found_video_keyframe, "Seek must land on or immediately before a video keyframe");

    // Also verify peeked_packet is cleared if its stream gets discarded by set_active_streams
    let _ = demuxer.seek_to(500).expect("seek_to(500)");
    demuxer.set_active_streams(None, None, false);
    assert!(
        demuxer.read_packet().expect("read after discarding all").is_none(),
        "Discarding all streams must drop buffered peeked_packet and yield None"
    );

    // Verify IsoBmffDemuxer also populates pts_us accurately
    let mut mp4_demuxer = IsoBmffDemuxer::new();
    mp4_demuxer
        .init(Box::new(Cursor::new(build_synthetic_full_mp4())))
        .expect("isobmff init");
    let pkt = mp4_demuxer
        .read_packet()
        .expect("isobmff read")
        .expect("isobmff packet");
    assert_eq!(pkt.pts_us, (pkt.pts_ms as i64) * 1000);
}
