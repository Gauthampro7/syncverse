//! Comprehensive unit and integration tests for container demuxing subsystem.
//! Tests Matroska/EBML and ISOBMFF/MP4 end-to-end demuxing, multi-track parsing,
//! frame lacing, sample table resolving, and keyframe seeking.

use std::io::Cursor;
use syncverse_demux::{
    build_synthetic_full_mkv, build_synthetic_full_mp4, unpack_lacing, DemuxError,
    Demuxer, EbmlDemuxer, IsoBmffDemuxer, SampleTableResolver, TrackType,
    SYNTHETIC_EBML_VECTOR, SYNTHETIC_MP4_VECTOR,
};

#[test]
fn test_synthetic_full_mkv_demux_and_seeking() {
    let mkv_bytes = build_synthetic_full_mkv();
    let mut cursor = Cursor::new(mkv_bytes.clone());

    // 1. Probe
    let probe_res = EbmlDemuxer::probe(&mut cursor).expect("probe failed");
    assert!(probe_res, "MKV probe must succeed for full synthetic MKV");

    // 2. Init & Track inventory
    let mut demuxer = EbmlDemuxer::new();
    let info = demuxer
        .init(Box::new(cursor))
        .expect("demuxer init failed");

    assert_eq!(info.timescale, 1_000_000, "Timescale must be 1ms per tick");
    assert_eq!(info.duration_ms, Some(1000), "Duration must be 1000ms");
    assert_eq!(info.tracks.len(), 2, "Must contain exactly 2 tracks");

    let v_track = &info.tracks[0];
    assert_eq!(v_track.track_id, 1);
    assert_eq!(v_track.track_type, TrackType::Video);
    assert_eq!(v_track.codec_id, "V_VP9");
    assert_eq!(v_track.width, Some(1920));
    assert_eq!(v_track.height, Some(1080));

    let a_track = &info.tracks[1];
    assert_eq!(a_track.track_id, 2);
    assert_eq!(a_track.track_type, TrackType::Audio);
    assert_eq!(a_track.codec_id, "A_OPUS");
    assert_eq!(a_track.sample_rate, Some(48000.0));
    assert_eq!(a_track.channels, Some(2));

    // Verify parsed Cues
    assert_eq!(demuxer.cues().len(), 2, "Must parse 2 Cue points");
    assert_eq!(demuxer.cues()[0].pts_ms, 0);
    assert_eq!(demuxer.cues()[1].pts_ms, 500);

    // 3. Sequential packet extraction
    let p1 = demuxer.read_packet().unwrap().expect("packet 1");
    assert_eq!(p1.track_id, 1);
    assert_eq!(p1.pts_ms, 0);
    assert!(p1.is_keyframe);
    assert_eq!(p1.data, vec![0x01, 0x02, 0x03, 0x04]);

    let p2 = demuxer.read_packet().unwrap().expect("packet 2");
    assert_eq!(p2.track_id, 2);
    assert_eq!(p2.pts_ms, 0);
    assert!(p2.is_keyframe);
    assert_eq!(p2.data, vec![0xAA, 0xBB, 0xCC]);

    let p3 = demuxer.read_packet().unwrap().expect("packet 3");
    assert_eq!(p3.track_id, 1);
    assert_eq!(p3.pts_ms, 500);
    assert!(p3.is_keyframe);
    assert_eq!(p3.data, vec![0x05, 0x06, 0x07, 0x08]);

    let p4 = demuxer.read_packet().unwrap().expect("packet 4");
    assert_eq!(p4.track_id, 2);
    assert_eq!(p4.pts_ms, 500);
    assert!(p4.is_keyframe);
    assert_eq!(p4.data, vec![0xDD, 0xEE, 0xFF]);

    let p5 = demuxer.read_packet().unwrap();
    assert!(p5.is_none(), "Must reach clean EOF at end of stream");

    // 4. Seek to keyframe at 500ms
    let landed = demuxer.seek_to(500).expect("seek to 500ms failed");
    assert_eq!(landed, 500, "Must land on keyframe at 500ms");

    let p_after_seek = demuxer.read_packet().unwrap().expect("packet after seek");
    assert_eq!(p_after_seek.track_id, 1);
    assert_eq!(p_after_seek.pts_ms, 500);
    assert_eq!(p_after_seek.data, vec![0x05, 0x06, 0x07, 0x08]);

    // 5. Seek to non-keyframe at 250ms -> lands on preceding keyframe at 0ms
    let landed_0 = demuxer.seek_to(250).expect("seek to 250ms failed");
    assert_eq!(landed_0, 0, "Must land on preceding keyframe at 0ms");

    let p_after_seek_0 = demuxer.read_packet().unwrap().expect("packet after seek 0");
    assert_eq!(p_after_seek_0.track_id, 1);
    assert_eq!(p_after_seek_0.pts_ms, 0);
    assert_eq!(p_after_seek_0.data, vec![0x01, 0x02, 0x03, 0x04]);
}

#[test]
fn test_synthetic_full_mp4_demux_and_seeking() {
    let mp4_bytes = build_synthetic_full_mp4();
    let mut cursor = Cursor::new(mp4_bytes.clone());

    // 1. Probe
    let probe_res = IsoBmffDemuxer::probe(&mut cursor).expect("probe failed");
    assert!(probe_res, "MP4 probe must succeed for full synthetic MP4");

    // 2. Init & Track inventory
    let mut demuxer = IsoBmffDemuxer::new();
    let info = demuxer
        .init(Box::new(cursor))
        .expect("demuxer init failed");

    assert_eq!(info.timescale, 1000);
    assert_eq!(info.duration_ms, Some(1000));
    assert_eq!(info.tracks.len(), 2, "Must contain video and audio tracks");

    let v_track = &info.tracks[0];
    assert_eq!(v_track.track_id, 1);
    assert_eq!(v_track.track_type, TrackType::Video);
    assert_eq!(v_track.codec_id, "avc1");
    assert_eq!(v_track.width, Some(1920));
    assert_eq!(v_track.height, Some(1080));

    let a_track = &info.tracks[1];
    assert_eq!(a_track.track_id, 2);
    assert_eq!(a_track.track_type, TrackType::Audio);
    assert_eq!(a_track.codec_id, "mp4a");
    assert_eq!(a_track.sample_rate, Some(48000.0));
    assert_eq!(a_track.channels, Some(2));

    // 3. Interleaved sequential reading from mdat
    let p1 = demuxer.read_packet().unwrap().expect("packet 1");
    assert_eq!(p1.track_id, 1);
    assert_eq!(p1.pts_ms, 0);
    assert!(p1.is_keyframe);
    assert_eq!(p1.data.len(), 8);

    let p2 = demuxer.read_packet().unwrap().expect("packet 2");
    assert_eq!(p2.track_id, 2);
    assert_eq!(p2.pts_ms, 0);
    assert!(p2.is_keyframe);
    assert_eq!(p2.data.len(), 6);

    let p3 = demuxer.read_packet().unwrap().expect("packet 3");
    assert_eq!(p3.track_id, 1);
    assert_eq!(p3.pts_ms, 500);
    assert!(!p3.is_keyframe); // sample 2 is not keyframe
    assert_eq!(p3.data.len(), 8);

    let p4 = demuxer.read_packet().unwrap().expect("packet 4");
    assert_eq!(p4.track_id, 2);
    assert_eq!(p4.pts_ms, 500);
    assert!(p4.is_keyframe); // audio samples are all keyframe
    assert_eq!(p4.data.len(), 6);

    let p5 = demuxer.read_packet().unwrap();
    assert!(p5.is_none(), "Clean EOF when all samples are read");

    // 4. Seek test (sample 0 is keyframe, target 500ms seeks to preceding keyframe at 0ms)
    let landed = demuxer.seek_to(500).expect("seek failed");
    assert_eq!(landed, 0);

    let p_seek = demuxer.read_packet().unwrap().expect("packet after seek");
    assert_eq!(p_seek.track_id, 1);
    assert_eq!(p_seek.pts_ms, 0);
    assert!(p_seek.is_keyframe);
}

#[test]
fn test_lacing_adversarial_error_cases() {
    // Truncated / empty lacing
    assert!(matches!(
        unpack_lacing(1, &[]),
        Err(DemuxError::CorruptedBlock(_))
    ));
    assert!(matches!(
        unpack_lacing(2, &[]),
        Err(DemuxError::CorruptedBlock(_))
    ));
    assert!(matches!(
        unpack_lacing(3, &[]),
        Err(DemuxError::CorruptedBlock(_))
    ));

    // Unsupported lacing mode (e.g. 4)
    assert!(matches!(
        unpack_lacing(4, &[1, 2, 3]),
        Err(DemuxError::CorruptedBlock(_))
    ));

    // Fixed lacing payload not divisible
    assert!(matches!(
        unpack_lacing(2, &[2, 1, 2, 3, 4]), // frame_count = 3, remaining = 4 (not divisible by 3)
        Err(DemuxError::CorruptedBlock(_))
    ));
}

#[test]
fn test_sample_table_resolver_adversarial() {
    let mut resolver = SampleTableResolver::new();
    // Zero samples_per_chunk rejection
    assert!(matches!(
        resolver.add_stsc_entry(1, 0, 1),
        Err(DemuxError::CorruptSampleTable(_))
    ));

    // Compute sample offset on empty tables
    assert!(matches!(
        resolver.compute_sample_offset(0),
        Err(DemuxError::CorruptSampleTable(_))
    ));

    // Compute sample size on empty stsz
    assert!(matches!(
        resolver.get_sample_size(0),
        Err(DemuxError::NotFound(_))
    ));
}

#[test]
fn test_backward_compatibility_minimal_vectors() {
    // 1. SYNTHETIC_EBML_VECTOR (48B)
    assert_eq!(SYNTHETIC_EBML_VECTOR.len(), 48);
    let mut cur_ebml = Cursor::new(SYNTHETIC_EBML_VECTOR.to_vec());
    assert!(EbmlDemuxer::probe(&mut cur_ebml).unwrap());
    let mut demuxer_ebml = EbmlDemuxer::new();
    let info_ebml = demuxer_ebml.init(Box::new(cur_ebml)).unwrap();
    assert_eq!(info_ebml.tracks.len(), 1);
    assert_eq!(info_ebml.tracks[0].codec_id, "V_VP9");
    assert!(demuxer_ebml.read_packet().unwrap().is_none());

    // 2. SYNTHETIC_MP4_VECTOR (132B)
    assert_eq!(SYNTHETIC_MP4_VECTOR.len(), 132);
    let mut cur_mp4 = Cursor::new(SYNTHETIC_MP4_VECTOR.to_vec());
    assert!(IsoBmffDemuxer::probe(&mut cur_mp4).unwrap());
    let mut demuxer_mp4 = IsoBmffDemuxer::new();
    let info_mp4 = demuxer_mp4.init(Box::new(cur_mp4)).unwrap();
    assert_eq!(info_mp4.timescale, 1000);
    assert_eq!(info_mp4.duration_ms, Some(10000));
    assert_eq!(info_mp4.tracks.len(), 1);
    assert_eq!(info_mp4.tracks[0].codec_id, "avc1");
    assert!(demuxer_mp4.read_packet().unwrap().is_none());
}
