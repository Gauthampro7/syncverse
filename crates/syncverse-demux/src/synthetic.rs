//! Synthetic container test vector generators for Matroska (MKV) and ISOBMFF (MP4).
//!
//! Provides in-tree multi-track streams with video and audio tracks, seek indices (Cues/stss),
//! and sequential media payloads for hermetic testing.

use std::sync::OnceLock;

// ============================================================================
// MATROSKA / EBML SYNTHETIC GENERATOR
// ============================================================================

fn encode_vint(id: u64, is_id: bool) -> Vec<u8> {
    if is_id {
        if id <= 0xFF {
            vec![id as u8]
        } else if id <= 0xFFFF {
            vec![(id >> 8) as u8, (id & 0xFF) as u8]
        } else if id <= 0xFF_FFFF {
            vec![
                (id >> 16) as u8,
                ((id >> 8) & 0xFF) as u8,
                (id & 0xFF) as u8,
            ]
        } else {
            vec![
                (id >> 24) as u8,
                ((id >> 16) & 0xFF) as u8,
                ((id >> 8) & 0xFF) as u8,
                (id & 0xFF) as u8,
            ]
        }
    } else if id < 0x7F {
        vec![0x80 | (id as u8)]
    } else if id < 0x3FFF {
        vec![0x40 | ((id >> 8) as u8), (id & 0xFF) as u8]
    } else if id < 0x1F_FFFF {
        vec![
            0x20 | ((id >> 16) as u8),
            ((id >> 8) & 0xFF) as u8,
            (id & 0xFF) as u8,
        ]
    } else {
        vec![
            0x10 | ((id >> 24) as u8),
            ((id >> 16) & 0xFF) as u8,
            ((id >> 8) & 0xFF) as u8,
            (id & 0xFF) as u8,
        ]
    }
}

fn write_elem(buf: &mut Vec<u8>, id: u64, data: &[u8]) {
    buf.extend(encode_vint(id, true));
    buf.extend(encode_vint(data.len() as u64, false));
    buf.extend_from_slice(data);
}

fn write_uint_elem(buf: &mut Vec<u8>, id: u64, val: u64) {
    if val <= 0xFF {
        write_elem(buf, id, &[val as u8]);
    } else if val <= 0xFFFF {
        write_elem(buf, id, &[(val >> 8) as u8, (val & 0xFF) as u8]);
    } else if val <= 0xFF_FFFF {
        write_elem(
            buf,
            id,
            &[
                (val >> 16) as u8,
                ((val >> 8) & 0xFF) as u8,
                (val & 0xFF) as u8,
            ],
        );
    } else {
        write_elem(buf, id, &val.to_be_bytes());
    }
}

fn write_float32_elem(buf: &mut Vec<u8>, id: u64, val: f32) {
    write_elem(buf, id, &val.to_be_bytes());
}

fn write_str_elem(buf: &mut Vec<u8>, id: u64, s: &str) {
    write_elem(buf, id, s.as_bytes());
}

/// Builds a byte-exact multi-track Matroska container with VP9 video, Opus audio, Cues, and Clusters.
pub fn build_synthetic_full_mkv() -> Vec<u8> {
    let mut ebml_header_body = Vec::new();
    write_uint_elem(&mut ebml_header_body, 0x4286, 1); // EBMLVersion = 1
    write_uint_elem(&mut ebml_header_body, 0x42F7, 1); // EBMLReadVersion = 1
    write_uint_elem(&mut ebml_header_body, 0x42F2, 4); // EBMLMaxIDLength = 4
    write_uint_elem(&mut ebml_header_body, 0x42F3, 8); // EBMLMaxSizeLength = 8
    write_str_elem(&mut ebml_header_body, 0x4282, "matroska"); // DocType = "matroska"
    write_uint_elem(&mut ebml_header_body, 0x4287, 4); // DocTypeVersion = 4

    let mut full_file = Vec::new();
    write_elem(&mut full_file, 0x1A45_DFA3, &ebml_header_body);

    // Info element
    let mut info_body = Vec::new();
    write_uint_elem(&mut info_body, 0x2AD7B1, 1_000_000); // TimestampScale = 1,000,000 ns (1ms)
    write_float32_elem(&mut info_body, 0x4489, 1000.0); // Duration = 1000.0 ms

    // Tracks element
    let mut tracks_body = Vec::new();

    // Track 1: Video
    let mut t1_video = Vec::new();
    write_uint_elem(&mut t1_video, 0xB0, 1920); // PixelWidth
    write_uint_elem(&mut t1_video, 0xBA, 1080); // PixelHeight

    let mut t1 = Vec::new();
    write_uint_elem(&mut t1, 0xD7, 1); // TrackNumber = 1
    write_uint_elem(&mut t1, 0x73C5, 1); // TrackUID = 1
    write_uint_elem(&mut t1, 0x83, 1); // TrackType = 1 (Video)
    write_str_elem(&mut t1, 0x86, "V_VP9"); // CodecID
    write_elem(&mut t1, 0xE0, &t1_video);
    write_elem(&mut tracks_body, 0xAE, &t1);

    // Track 2: Audio
    let mut t2_audio = Vec::new();
    write_float32_elem(&mut t2_audio, 0xB5, 48000.0); // SamplingFrequency = 48000 Hz
    write_uint_elem(&mut t2_audio, 0x9F, 2); // Channels = 2

    let mut t2 = Vec::new();
    write_uint_elem(&mut t2, 0xD7, 2); // TrackNumber = 2
    write_uint_elem(&mut t2, 0x73C5, 2); // TrackUID = 2
    write_uint_elem(&mut t2, 0x83, 2); // TrackType = 2 (Audio)
    write_str_elem(&mut t2, 0x86, "A_OPUS"); // CodecID
    write_elem(&mut t2, 0xE1, &t2_audio);
    write_elem(&mut tracks_body, 0xAE, &t2);

    // Cluster 1 (t = 0 ms)
    let mut cluster1_body = Vec::new();
    write_uint_elem(&mut cluster1_body, 0xE7, 0); // Cluster Timecode = 0

    // Video SimpleBlock: Track 1, rel_timecode = 0, flags = 0x80 (keyframe), data = [0x01, 0x02, 0x03, 0x04]
    let mut sb1_video = vec![0x81, 0x00, 0x00, 0x80];
    sb1_video.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]);
    write_elem(&mut cluster1_body, 0xA3, &sb1_video);

    // Audio SimpleBlock: Track 2, rel_timecode = 0, flags = 0x80 (keyframe), data = [0xAA, 0xBB, 0xCC]
    let mut sb1_audio = vec![0x82, 0x00, 0x00, 0x80];
    sb1_audio.extend_from_slice(&[0xAA, 0xBB, 0xCC]);
    write_elem(&mut cluster1_body, 0xA3, &sb1_audio);

    // Cluster 2 (t = 500 ms)
    let mut cluster2_body = Vec::new();
    write_uint_elem(&mut cluster2_body, 0xE7, 500); // Cluster Timecode = 500

    // Video SimpleBlock: Track 1, rel_timecode = 0, flags = 0x80 (keyframe), data = [0x05, 0x06, 0x07, 0x08]
    let mut sb2_video = vec![0x81, 0x00, 0x00, 0x80];
    sb2_video.extend_from_slice(&[0x05, 0x06, 0x07, 0x08]);
    write_elem(&mut cluster2_body, 0xA3, &sb2_video);

    // Audio SimpleBlock: Track 2, rel_timecode = 0, flags = 0x80 (keyframe), data = [0xDD, 0xEE, 0xFF]
    let mut sb2_audio = vec![0x82, 0x00, 0x00, 0x80];
    sb2_audio.extend_from_slice(&[0xDD, 0xEE, 0xFF]);
    write_elem(&mut cluster2_body, 0xA3, &sb2_audio);

    // Measure prefix size to compute exact cue cluster positions
    let mut prefix_segment = Vec::new();
    write_elem(&mut prefix_segment, 0x1549A966, &info_body);
    write_elem(&mut prefix_segment, 0x1654AE6B, &tracks_body);

    let mut c1_encoded = Vec::new();
    write_elem(&mut c1_encoded, 0x1F43B675, &cluster1_body);

    let mut c2_encoded = Vec::new();
    write_elem(&mut c2_encoded, 0x1F43B675, &cluster2_body);

    // Placeholder Cues with 2 seek points
    let mut cues_body = Vec::new();

    // Cue 1 (t = 0)
    let mut cp1_tp = Vec::new();
    write_uint_elem(&mut cp1_tp, 0xF7, 1); // CueTrack = 1
    write_uint_elem(&mut cp1_tp, 0xF1, 0); // Placeholder CueClusterPosition

    let mut cp1 = Vec::new();
    write_uint_elem(&mut cp1, 0xB3, 0); // CueTime = 0
    write_elem(&mut cp1, 0xB7, &cp1_tp);
    write_elem(&mut cues_body, 0xBB, &cp1);

    // Cue 2 (t = 500)
    let mut cp2_tp = Vec::new();
    write_uint_elem(&mut cp2_tp, 0xF7, 1); // CueTrack = 1
    write_uint_elem(&mut cp2_tp, 0xF1, 0); // Placeholder CueClusterPosition

    let mut cp2 = Vec::new();
    write_uint_elem(&mut cp2, 0xB3, 500); // CueTime = 500
    write_elem(&mut cp2, 0xB7, &cp2_tp);
    write_elem(&mut cues_body, 0xBB, &cp2);

    let mut cues_encoded = Vec::new();
    write_elem(&mut cues_encoded, 0x1C53BB6B, &cues_body);

    let cues_size = cues_encoded.len();
    let cluster1_pos = (prefix_segment.len() + cues_size) as u64;
    let cluster2_pos = cluster1_pos + c1_encoded.len() as u64;

    // Rebuild Cues with exact cluster offsets
    let mut real_cues_body = Vec::new();

    let mut r_cp1_tp = Vec::new();
    write_uint_elem(&mut r_cp1_tp, 0xF7, 1);
    write_uint_elem(&mut r_cp1_tp, 0xF1, cluster1_pos);
    let mut r_cp1 = Vec::new();
    write_uint_elem(&mut r_cp1, 0xB3, 0);
    write_elem(&mut r_cp1, 0xB7, &r_cp1_tp);
    write_elem(&mut real_cues_body, 0xBB, &r_cp1);

    let mut r_cp2_tp = Vec::new();
    write_uint_elem(&mut r_cp2_tp, 0xF7, 1);
    write_uint_elem(&mut r_cp2_tp, 0xF1, cluster2_pos);
    let mut r_cp2 = Vec::new();
    write_uint_elem(&mut r_cp2, 0xB3, 500);
    write_elem(&mut r_cp2, 0xB7, &r_cp2_tp);
    write_elem(&mut real_cues_body, 0xBB, &r_cp2);

    let mut real_cues_encoded = Vec::new();
    write_elem(&mut real_cues_encoded, 0x1C53BB6B, &real_cues_body);

    // Assemble entire Segment
    let mut segment_body = Vec::new();
    segment_body.extend(prefix_segment);
    segment_body.extend(real_cues_encoded);
    segment_body.extend(c1_encoded);
    segment_body.extend(c2_encoded);

    write_elem(&mut full_file, 0x18538067, &segment_body);
    full_file
}

// ============================================================================
// ISOBMFF / MP4 SYNTHETIC GENERATOR
// ============================================================================

fn write_box(buf: &mut Vec<u8>, fourcc: &[u8; 4], data: &[u8]) {
    let size = (8 + data.len()) as u32;
    buf.extend_from_slice(&size.to_be_bytes());
    buf.extend_from_slice(fourcc);
    buf.extend_from_slice(data);
}

fn write_full_box(buf: &mut Vec<u8>, fourcc: &[u8; 4], version: u8, flags: [u8; 3], data: &[u8]) {
    let size = (12 + data.len()) as u32;
    buf.extend_from_slice(&size.to_be_bytes());
    buf.extend_from_slice(fourcc);
    buf.push(version);
    buf.extend_from_slice(&flags);
    buf.extend_from_slice(data);
}

/// Builds a byte-exact multi-track ISOBMFF / MP4 container with AVC video, AAC audio, and mdat payloads.
pub fn build_synthetic_full_mp4() -> Vec<u8> {
    // 1. ftyp box
    let mut ftyp_data = Vec::new();
    ftyp_data.extend_from_slice(b"isom"); // major_brand
    ftyp_data.extend_from_slice(&0x0000_0200u32.to_be_bytes()); // minor_version
    ftyp_data.extend_from_slice(b"isomiso2mp41"); // compatible brands
    let mut ftyp_box = Vec::new();
    write_box(&mut ftyp_box, b"ftyp", &ftyp_data);

    // We will place mdat after ftyp and moov.
    // Video: 2 samples of 8 bytes each
    // Audio: 2 samples of 6 bytes each
    let v_s0 = [0x00u8, 0x00, 0x00, 0x04, 0x65, 0x88, 0x84, 0x00];
    let v_s1 = [0x00u8, 0x00, 0x00, 0x04, 0x41, 0x9A, 0x24, 0x00];
    let a_s0 = [0x4Fu8, 0x70, 0x75, 0x73, 0x01, 0x02];
    let a_s1 = [0x4Fu8, 0x70, 0x75, 0x73, 0x03, 0x04];

    // Interleaved chunks:
    // Chunk 1: Video Sample 0 (8B)
    // Chunk 2: Audio Sample 0 (6B)
    // Chunk 3: Video Sample 1 (8B)
    // Chunk 4: Audio Sample 1 (6B)
    let mut mdat_data = Vec::new();
    mdat_data.extend_from_slice(&v_s0);
    mdat_data.extend_from_slice(&a_s0);
    mdat_data.extend_from_slice(&v_s1);
    mdat_data.extend_from_slice(&a_s1);

    // Construct moov box helper given chunk offsets
    let build_moov = |v_offsets: [u32; 2], a_offsets: [u32; 2]| -> Vec<u8> {
        let mut mvhd_data = Vec::new();
        mvhd_data.extend_from_slice(&0u32.to_be_bytes()); // creation
        mvhd_data.extend_from_slice(&0u32.to_be_bytes()); // mod
        mvhd_data.extend_from_slice(&1000u32.to_be_bytes()); // timescale = 1000
        mvhd_data.extend_from_slice(&1000u32.to_be_bytes()); // duration = 1000
        mvhd_data.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // rate = 1.0
        mvhd_data.extend_from_slice(&0x0100u16.to_be_bytes()); // volume = 1.0
        mvhd_data.extend_from_slice(&[0u8; 10]); // reserved
        // 36-byte unity matrix
        mvhd_data.extend_from_slice(&[
            0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00,
        ]);
        mvhd_data.extend_from_slice(&[0u8; 24]); // pre_defined
        mvhd_data.extend_from_slice(&3u32.to_be_bytes()); // next_track_id

        let mut mvhd_box = Vec::new();
        write_full_box(&mut mvhd_box, b"mvhd", 0, [0; 3], &mvhd_data);

        // --- TRAK 1: Video ---
        let mut tkhd1_data = Vec::new();
        tkhd1_data.extend_from_slice(&0u32.to_be_bytes());
        tkhd1_data.extend_from_slice(&0u32.to_be_bytes());
        tkhd1_data.extend_from_slice(&1u32.to_be_bytes()); // track_id = 1
        tkhd1_data.extend_from_slice(&0u32.to_be_bytes());
        tkhd1_data.extend_from_slice(&1000u32.to_be_bytes()); // duration
        tkhd1_data.extend_from_slice(&[0u8; 8]);
        tkhd1_data.extend_from_slice(&0u16.to_be_bytes());
        tkhd1_data.extend_from_slice(&0u16.to_be_bytes());
        tkhd1_data.extend_from_slice(&0u16.to_be_bytes());
        tkhd1_data.extend_from_slice(&0u16.to_be_bytes());
        // 36-byte matrix
        tkhd1_data.extend_from_slice(&[
            0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00,
        ]);
        tkhd1_data.extend_from_slice(&(1920u32 << 16).to_be_bytes()); // width = 1920
        tkhd1_data.extend_from_slice(&(1080u32 << 16).to_be_bytes()); // height = 1080
        let mut tkhd1_box = Vec::new();
        write_full_box(&mut tkhd1_box, b"tkhd", 0, [0, 0, 1], &tkhd1_data);

        // mdia 1
        let mut mdhd1_data = Vec::new();
        mdhd1_data.extend_from_slice(&0u32.to_be_bytes());
        mdhd1_data.extend_from_slice(&0u32.to_be_bytes());
        mdhd1_data.extend_from_slice(&1000u32.to_be_bytes()); // timescale
        mdhd1_data.extend_from_slice(&1000u32.to_be_bytes()); // duration
        mdhd1_data.extend_from_slice(&0u16.to_be_bytes()); // language
        mdhd1_data.extend_from_slice(&0u16.to_be_bytes());
        let mut mdhd1_box = Vec::new();
        write_full_box(&mut mdhd1_box, b"mdhd", 0, [0; 3], &mdhd1_data);

        let mut hdlr1_data = Vec::new();
        hdlr1_data.extend_from_slice(&0u32.to_be_bytes()); // pre_defined
        hdlr1_data.extend_from_slice(b"vide"); // handler
        hdlr1_data.extend_from_slice(&[0u8; 12]);
        hdlr1_data.extend_from_slice(b"VideoHandler\0");
        let mut hdlr1_box = Vec::new();
        write_full_box(&mut hdlr1_box, b"hdlr", 0, [0; 3], &hdlr1_data);

        // stbl 1
        let mut stsd1_entry = Vec::new();
        stsd1_entry.extend_from_slice(&[0u8; 6]);
        stsd1_entry.extend_from_slice(&1u16.to_be_bytes()); // data_ref = 1
        stsd1_entry.extend_from_slice(&[0u8; 16]);
        stsd1_entry.extend_from_slice(&1920u16.to_be_bytes()); // width = 1920
        stsd1_entry.extend_from_slice(&1080u16.to_be_bytes()); // height = 1080
        stsd1_entry.extend_from_slice(&0x0048_0000u32.to_be_bytes()); // 72 dpi
        stsd1_entry.extend_from_slice(&0x0048_0000u32.to_be_bytes()); // 72 dpi
        stsd1_entry.extend_from_slice(&0u32.to_be_bytes());
        stsd1_entry.extend_from_slice(&1u16.to_be_bytes()); // frame_count = 1
        stsd1_entry.extend_from_slice(&[0u8; 32]); // compressor name
        stsd1_entry.extend_from_slice(&0x0018u16.to_be_bytes()); // depth = 24
        stsd1_entry.extend_from_slice(&0xFFFFu16.to_be_bytes()); // pre_defined = -1
        let mut avc1_box = Vec::new();
        write_box(&mut avc1_box, b"avc1", &stsd1_entry);

        let mut stsd1_data = Vec::new();
        stsd1_data.extend_from_slice(&1u32.to_be_bytes()); // entry_count = 1
        stsd1_data.extend_from_slice(&avc1_box);
        let mut stsd1_box = Vec::new();
        write_full_box(&mut stsd1_box, b"stsd", 0, [0; 3], &stsd1_data);

        // stts: 2 samples of delta 500
        let mut stts1_data = Vec::new();
        stts1_data.extend_from_slice(&1u32.to_be_bytes()); // 1 entry
        stts1_data.extend_from_slice(&2u32.to_be_bytes()); // count = 2
        stts1_data.extend_from_slice(&500u32.to_be_bytes()); // delta = 500
        let mut stts1_box = Vec::new();
        write_full_box(&mut stts1_box, b"stts", 0, [0; 3], &stts1_data);

        // stss: sample 1 is keyframe
        let mut stss1_data = Vec::new();
        stss1_data.extend_from_slice(&1u32.to_be_bytes()); // 1 entry
        stss1_data.extend_from_slice(&1u32.to_be_bytes()); // sample 1 (1-based)
        let mut stss1_box = Vec::new();
        write_full_box(&mut stss1_box, b"stss", 0, [0; 3], &stss1_data);

        // stsc: 1 entry, first_chunk = 1, samples_per_chunk = 1, desc = 1
        let mut stsc1_data = Vec::new();
        stsc1_data.extend_from_slice(&1u32.to_be_bytes());
        stsc1_data.extend_from_slice(&1u32.to_be_bytes()); // first_chunk = 1
        stsc1_data.extend_from_slice(&1u32.to_be_bytes()); // samples_per_chunk = 1
        stsc1_data.extend_from_slice(&1u32.to_be_bytes()); // desc = 1
        let mut stsc1_box = Vec::new();
        write_full_box(&mut stsc1_box, b"stsc", 0, [0; 3], &stsc1_data);

        // stsz: sample_size = 0, count = 2, sizes = [8, 8]
        let mut stsz1_data = Vec::new();
        stsz1_data.extend_from_slice(&0u32.to_be_bytes()); // non-uniform
        stsz1_data.extend_from_slice(&2u32.to_be_bytes()); // count = 2
        stsz1_data.extend_from_slice(&8u32.to_be_bytes());
        stsz1_data.extend_from_slice(&8u32.to_be_bytes());
        let mut stsz1_box = Vec::new();
        write_full_box(&mut stsz1_box, b"stsz", 0, [0; 3], &stsz1_data);

        // stco: chunk_offsets = v_offsets
        let mut stco1_data = Vec::new();
        stco1_data.extend_from_slice(&2u32.to_be_bytes()); // 2 chunks
        stco1_data.extend_from_slice(&v_offsets[0].to_be_bytes());
        stco1_data.extend_from_slice(&v_offsets[1].to_be_bytes());
        let mut stco1_box = Vec::new();
        write_full_box(&mut stco1_box, b"stco", 0, [0; 3], &stco1_data);

        let mut stbl1_data = Vec::new();
        stbl1_data.extend_from_slice(&stsd1_box);
        stbl1_data.extend_from_slice(&stts1_box);
        stbl1_data.extend_from_slice(&stss1_box);
        stbl1_data.extend_from_slice(&stsc1_box);
        stbl1_data.extend_from_slice(&stsz1_box);
        stbl1_data.extend_from_slice(&stco1_box);
        let mut stbl1_box = Vec::new();
        write_box(&mut stbl1_box, b"stbl", &stbl1_data);

        let mut minf1_data = Vec::new();
        minf1_data.extend_from_slice(&stbl1_box);
        let mut minf1_box = Vec::new();
        write_box(&mut minf1_box, b"minf", &minf1_data);

        let mut mdia1_data = Vec::new();
        mdia1_data.extend_from_slice(&mdhd1_box);
        mdia1_data.extend_from_slice(&hdlr1_box);
        mdia1_data.extend_from_slice(&minf1_box);
        let mut mdia1_box = Vec::new();
        write_box(&mut mdia1_box, b"mdia", &mdia1_data);

        let mut trak1_data = Vec::new();
        trak1_data.extend_from_slice(&tkhd1_box);
        trak1_data.extend_from_slice(&mdia1_box);
        let mut trak1_box = Vec::new();
        write_box(&mut trak1_box, b"trak", &trak1_data);

        // --- TRAK 2: Audio ---
        let mut tkhd2_data = Vec::new();
        tkhd2_data.extend_from_slice(&0u32.to_be_bytes());
        tkhd2_data.extend_from_slice(&0u32.to_be_bytes());
        tkhd2_data.extend_from_slice(&2u32.to_be_bytes()); // track_id = 2
        tkhd2_data.extend_from_slice(&0u32.to_be_bytes());
        tkhd2_data.extend_from_slice(&1000u32.to_be_bytes()); // duration
        tkhd2_data.extend_from_slice(&[0u8; 8]);
        tkhd2_data.extend_from_slice(&0u16.to_be_bytes());
        tkhd2_data.extend_from_slice(&0u16.to_be_bytes());
        tkhd2_data.extend_from_slice(&0u16.to_be_bytes());
        tkhd2_data.extend_from_slice(&0u16.to_be_bytes());
        tkhd2_data.extend_from_slice(&[
            0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00,
        ]);
        tkhd2_data.extend_from_slice(&0u32.to_be_bytes()); // width = 0
        tkhd2_data.extend_from_slice(&0u32.to_be_bytes()); // height = 0
        let mut tkhd2_box = Vec::new();
        write_full_box(&mut tkhd2_box, b"tkhd", 0, [0, 0, 1], &tkhd2_data);

        // mdia 2
        let mut mdhd2_data = Vec::new();
        mdhd2_data.extend_from_slice(&0u32.to_be_bytes());
        mdhd2_data.extend_from_slice(&0u32.to_be_bytes());
        mdhd2_data.extend_from_slice(&1000u32.to_be_bytes()); // timescale = 1000
        mdhd2_data.extend_from_slice(&1000u32.to_be_bytes()); // duration = 1000
        mdhd2_data.extend_from_slice(&0u16.to_be_bytes());
        mdhd2_data.extend_from_slice(&0u16.to_be_bytes());
        let mut mdhd2_box = Vec::new();
        write_full_box(&mut mdhd2_box, b"mdhd", 0, [0; 3], &mdhd2_data);

        let mut hdlr2_data = Vec::new();
        hdlr2_data.extend_from_slice(&0u32.to_be_bytes());
        hdlr2_data.extend_from_slice(b"soun"); // handler
        hdlr2_data.extend_from_slice(&[0u8; 12]);
        hdlr2_data.extend_from_slice(b"SoundHandler\0");
        let mut hdlr2_box = Vec::new();
        write_full_box(&mut hdlr2_box, b"hdlr", 0, [0; 3], &hdlr2_data);

        // stbl 2
        let mut stsd2_entry = Vec::new();
        stsd2_entry.extend_from_slice(&[0u8; 6]);
        stsd2_entry.extend_from_slice(&1u16.to_be_bytes()); // data_ref = 1
        stsd2_entry.extend_from_slice(&[0u8; 8]); // reserved
        stsd2_entry.extend_from_slice(&2u16.to_be_bytes()); // channels = 2
        stsd2_entry.extend_from_slice(&16u16.to_be_bytes()); // samplesize = 16
        stsd2_entry.extend_from_slice(&0u32.to_be_bytes()); // predefined
        stsd2_entry.extend_from_slice(&(48000u32 << 16).to_be_bytes()); // samplerate = 48000
        let mut mp4a_box = Vec::new();
        write_box(&mut mp4a_box, b"mp4a", &stsd2_entry);

        let mut stsd2_data = Vec::new();
        stsd2_data.extend_from_slice(&1u32.to_be_bytes());
        stsd2_data.extend_from_slice(&mp4a_box);
        let mut stsd2_box = Vec::new();
        write_full_box(&mut stsd2_box, b"stsd", 0, [0; 3], &stsd2_data);

        let mut stts2_data = Vec::new();
        stts2_data.extend_from_slice(&1u32.to_be_bytes());
        stts2_data.extend_from_slice(&2u32.to_be_bytes());
        stts2_data.extend_from_slice(&500u32.to_be_bytes());
        let mut stts2_box = Vec::new();
        write_full_box(&mut stts2_box, b"stts", 0, [0; 3], &stts2_data);

        let mut stsc2_data = Vec::new();
        stsc2_data.extend_from_slice(&1u32.to_be_bytes());
        stsc2_data.extend_from_slice(&1u32.to_be_bytes());
        stsc2_data.extend_from_slice(&1u32.to_be_bytes());
        stsc2_data.extend_from_slice(&1u32.to_be_bytes());
        let mut stsc2_box = Vec::new();
        write_full_box(&mut stsc2_box, b"stsc", 0, [0; 3], &stsc2_data);

        let mut stsz2_data = Vec::new();
        stsz2_data.extend_from_slice(&0u32.to_be_bytes());
        stsz2_data.extend_from_slice(&2u32.to_be_bytes());
        stsz2_data.extend_from_slice(&6u32.to_be_bytes());
        stsz2_data.extend_from_slice(&6u32.to_be_bytes());
        let mut stsz2_box = Vec::new();
        write_full_box(&mut stsz2_box, b"stsz", 0, [0; 3], &stsz2_data);

        let mut stco2_data = Vec::new();
        stco2_data.extend_from_slice(&2u32.to_be_bytes());
        stco2_data.extend_from_slice(&a_offsets[0].to_be_bytes());
        stco2_data.extend_from_slice(&a_offsets[1].to_be_bytes());
        let mut stco2_box = Vec::new();
        write_full_box(&mut stco2_box, b"stco", 0, [0; 3], &stco2_data);

        let mut stbl2_data = Vec::new();
        stbl2_data.extend_from_slice(&stsd2_box);
        stbl2_data.extend_from_slice(&stts2_box);
        stbl2_data.extend_from_slice(&stsc2_box);
        stbl2_data.extend_from_slice(&stsz2_box);
        stbl2_data.extend_from_slice(&stco2_box);
        let mut stbl2_box = Vec::new();
        write_box(&mut stbl2_box, b"stbl", &stbl2_data);

        let mut minf2_data = Vec::new();
        minf2_data.extend_from_slice(&stbl2_box);
        let mut minf2_box = Vec::new();
        write_box(&mut minf2_box, b"minf", &minf2_data);

        let mut mdia2_data = Vec::new();
        mdia2_data.extend_from_slice(&mdhd2_box);
        mdia2_data.extend_from_slice(&hdlr2_box);
        mdia2_data.extend_from_slice(&minf2_box);
        let mut mdia2_box = Vec::new();
        write_box(&mut mdia2_box, b"mdia", &mdia2_data);

        let mut trak2_data = Vec::new();
        trak2_data.extend_from_slice(&tkhd2_box);
        trak2_data.extend_from_slice(&mdia2_box);
        let mut trak2_box = Vec::new();
        write_box(&mut trak2_box, b"trak", &trak2_data);

        // moov body
        let mut moov_data = Vec::new();
        moov_data.extend_from_slice(&mvhd_box);
        moov_data.extend_from_slice(&trak1_box);
        moov_data.extend_from_slice(&trak2_box);

        let mut moov_box = Vec::new();
        write_box(&mut moov_box, b"moov", &moov_data);
        moov_box
    };

    // Calculate fixed size of moov
    let dummy_moov = build_moov([0, 0], [0, 0]);
    let moov_size = dummy_moov.len() as u32;

    let ftyp_size = ftyp_box.len() as u32;
    let mdat_hdr_size = 8u32;
    let mdat_payload_offset = ftyp_size + moov_size + mdat_hdr_size;

    let v0_off = mdat_payload_offset;
    let a0_off = v0_off + 8;
    let v1_off = a0_off + 6;
    let a1_off = v1_off + 8;

    let real_moov = build_moov([v0_off, v1_off], [a0_off, a1_off]);
    assert_eq!(real_moov.len(), dummy_moov.len());

    let mut mdat_box = Vec::new();
    write_box(&mut mdat_box, b"mdat", &mdat_data);

    let mut full_file = Vec::new();
    full_file.extend_from_slice(&ftyp_box);
    full_file.extend_from_slice(&real_moov);
    full_file.extend_from_slice(&mdat_box);
    full_file
}

static FULL_MKV: OnceLock<Vec<u8>> = OnceLock::new();
static FULL_MP4: OnceLock<Vec<u8>> = OnceLock::new();

/// Returns a reference to the global synthetic full MKV byte vector.
pub fn get_synthetic_full_mkv() -> &'static [u8] {
    FULL_MKV.get_or_init(build_synthetic_full_mkv).as_slice()
}

/// Returns a reference to the global synthetic full MP4 byte vector.
pub fn get_synthetic_full_mp4() -> &'static [u8] {
    FULL_MP4.get_or_init(build_synthetic_full_mp4).as_slice()
}
