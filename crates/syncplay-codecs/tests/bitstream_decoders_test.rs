//! Unit and integration test suite for pure-Rust video and audio bitstream decoders.

use syncplay_codecs::audio::{
    unpack_opus_packet, unpack_pcm, unpack_vorbis_audio_packet, AudioCodec, AudioDecoder,
    OpusBandwidth, OpusMode, OpusToc, PcmFormat, VorbisIdentificationHeader,
};
use syncplay_codecs::color::{
    convert_rgba_to_terminal, convert_yuv420p_to_terminal, ColorStandard, TerminalTrueColorBuffer,
};
use syncplay_codecs::frame::{VideoFrameRGBA, VideoFrameYUV420p};
use syncplay_codecs::h264::{
    extract_nal_units, parse_annex_b, parse_avcc, unescape_rbsp, AnnexBScanner, H264Decoder,
    NalHeader, NalUnitType, Pps, SliceHeader, SliceType, Sps,
};
use syncplay_common::reader::BitReader;

/// In-test helper to construct standard-compliant H.264 bitstream test vectors.
struct BitstreamBuilder {
    bits: Vec<bool>,
}

impl BitstreamBuilder {
    fn new() -> Self {
        Self { bits: Vec::new() }
    }

    fn write_bit(&mut self, bit: bool) {
        self.bits.push(bit);
    }

    fn write_bits(&mut self, val: u64, count: usize) {
        for i in (0..count).rev() {
            self.bits.push(((val >> i) & 1) != 0);
        }
    }

    fn write_ue(&mut self, val: u32) {
        let v = (val as u64) + 1;
        let m = 63 - v.leading_zeros() as usize;
        for _ in 0..m {
            self.write_bit(false);
        }
        self.write_bits(v, m + 1);
    }

    fn write_se(&mut self, val: i32) {
        let k = if val <= 0 {
            ((-val) as u32) * 2
        } else {
            (val as u32) * 2 - 1
        };
        self.write_ue(k);
    }

    fn build_rbsp(&mut self) -> Vec<u8> {
        // Append rbsp_trailing_bits (1 followed by zeros up to byte boundary)
        self.write_bit(true);
        while !self.bits.len().is_multiple_of(8) {
            self.write_bit(false);
        }
        let mut bytes = Vec::new();
        for chunk in self.bits.as_chunks::<8>().0 {
            let mut byte = 0u8;
            for (idx, &b) in chunk.iter().enumerate() {
                if b {
                    byte |= 1 << (7 - idx);
                }
            }
            bytes.push(byte);
        }
        bytes
    }
}

/// Generates a valid H.264 Baseline SPS RBSP for the given dimensions.
fn build_synthetic_sps_rbsp(
    width_mbs: u32,
    height_mbs: u32,
    crop: Option<(u32, u32, u32, u32)>,
) -> Vec<u8> {
    let mut b = BitstreamBuilder::new();
    b.write_bits(66, 8); // profile_idc: Baseline (66)
    b.write_bits(0, 8); // constraint_set_flags
    b.write_bits(31, 8); // level_idc: Level 3.1 (31)
    b.write_ue(0); // seq_parameter_set_id: 0
    b.write_ue(0); // log2_max_frame_num_minus4: 0 (4 bits)
    b.write_ue(0); // pic_order_cnt_type: 0
    b.write_ue(0); // log2_max_pic_order_cnt_lsb_minus4: 0 (4 bits)
    b.write_ue(1); // max_num_ref_frames: 1
    b.write_bit(false); // gaps_in_frame_num_value_allowed_flag: 0
    b.write_ue(width_mbs - 1); // pic_width_in_mbs_minus1
    b.write_ue(height_mbs - 1); // pic_height_in_map_units_minus1
    b.write_bit(true); // frame_mbs_only_flag: 1 (progressive)
    b.write_bit(true); // direct_8x8_inference_flag: 1

    if let Some((left, right, top, bottom)) = crop {
        b.write_bit(true); // frame_cropping_flag: 1
        b.write_ue(left);
        b.write_ue(right);
        b.write_ue(top);
        b.write_ue(bottom);
    } else {
        b.write_bit(false); // frame_cropping_flag: 0
    }

    b.write_bit(false); // vui_parameters_present_flag: 0
    b.build_rbsp()
}

/// Generates a valid H.264 PPS RBSP.
fn build_synthetic_pps_rbsp(sps_id: u32, pps_id: u32) -> Vec<u8> {
    let mut b = BitstreamBuilder::new();
    b.write_ue(pps_id); // pic_parameter_set_id
    b.write_ue(sps_id); // seq_parameter_set_id
    b.write_bit(false); // entropy_coding_mode_flag: false (CAVLC)
    b.write_bit(false); // bottom_field_pic_order_in_frame_present_flag
    b.write_ue(0); // num_slice_groups_minus1: 0
    b.write_ue(0); // num_ref_idx_l0_default_active_minus1: 0
    b.write_ue(0); // num_ref_idx_l1_default_active_minus1: 0
    b.write_bit(false); // weighted_pred_flag: 0
    b.write_bits(0, 2); // weighted_bipred_idc: 0
    b.write_se(0); // pic_init_qp_minus26: 0
    b.write_se(0); // pic_init_qs_minus26: 0
    b.write_se(0); // chroma_qp_index_offset: 0
    b.write_bit(false); // deblocking_filter_control_present_flag: 0
    b.write_bit(false); // constrained_intra_pred_flag: 0
    b.write_bit(false); // redundant_pic_cnt_present_flag: 0
    b.build_rbsp()
}

/// Generates a valid H.264 IDR Slice Header RBSP.
fn build_synthetic_idr_slice_rbsp(pps_id: u32, frame_num: u64, idr_id: u32) -> Vec<u8> {
    let mut b = BitstreamBuilder::new();
    b.write_ue(0); // first_mb_in_slice: 0
    b.write_ue(7); // slice_type: 7 (I-slice, raw 7 % 5 = 2)
    b.write_ue(pps_id); // pic_parameter_set_id
    b.write_bits(frame_num, 4); // frame_num (4 bits per log2_max_frame_num_minus4=0)
    b.write_ue(idr_id); // idr_pic_id
    b.write_bits(0, 4); // pic_order_cnt_lsb (4 bits)
    b.write_bit(false); // no_output_of_prior_pics_flag
    b.write_bit(false); // long_term_reference_flag
    b.write_se(0); // slice_qp_delta: 0

    // Add dummy macroblock data payload
    for i in 0..64 {
        b.write_bits(i % 16, 4);
    }
    b.build_rbsp()
}

// ============================================================================
// 1. Bitstream Exp-Golomb Tests
// ============================================================================

#[test]
fn test_exp_golomb_decoding_exhaustive() {
    let mut builder = BitstreamBuilder::new();
    let test_values_ue = [0, 1, 2, 3, 4, 15, 31, 63, 127, 255, 1023, 65535];
    for &v in &test_values_ue {
        builder.write_ue(v);
    }

    let test_values_se = [0, 1, -1, 2, -2, 10, -10, 100, -100, 1000, -1000];
    for &v in &test_values_se {
        builder.write_se(v);
    }

    let rbsp = builder.build_rbsp();
    let mut reader = BitReader::new(&rbsp);

    for &expected in &test_values_ue {
        let val = reader.read_ue().expect("failed to read ue(v)");
        assert_eq!(val, expected, "Mismatch in ue(v) decoding");
    }

    for &expected in &test_values_se {
        let val = reader.read_se().expect("failed to read se(v)");
        assert_eq!(val, expected, "Mismatch in se(v) decoding");
    }
}

#[test]
fn test_truncated_exp_golomb_te() {
    let mut builder = BitstreamBuilder::new();
    // For max == 1: value 0 is encoded as 1, value 1 is encoded as 0
    builder.write_bit(true); // represents 0
    builder.write_bit(false); // represents 1
    // For max > 1: identical to ue(v)
    builder.write_ue(5);

    let rbsp = builder.build_rbsp();
    let mut reader = BitReader::new(&rbsp);

    assert_eq!(reader.read_te(1).unwrap(), 0);
    assert_eq!(reader.read_te(1).unwrap(), 1);
    assert_eq!(reader.read_te(10).unwrap(), 5);
}

// ============================================================================
// 2. NAL Unit Parsing & RBSP Un-Escaping Tests
// ============================================================================

#[test]
fn test_rbsp_emulation_prevention_unescaping() {
    // Sequence with emulation prevention bytes: [0x00, 0x00, 0x03, 0x00/01/02/03]
    let escaped = [
        0xAA, 0x00, 0x00, 0x03, 0x00, 0xBB, 0x00, 0x00, 0x03, 0x01, 0xCC, 0x00, 0x00, 0x03,
        0x02, 0xDD, 0x00, 0x00, 0x03, 0x03, 0xEE,
    ];
    let unescaped = unescape_rbsp(&escaped);
    let expected = [
        0xAA, 0x00, 0x00, 0x00, 0xBB, 0x00, 0x00, 0x01, 0xCC, 0x00, 0x00, 0x02, 0xDD, 0x00,
        0x00, 0x03, 0xEE,
    ];
    assert_eq!(unescaped, expected);

    // Non-emulation pattern: 0x00, 0x01, 0x03 (should not drop 0x03)
    let normal = [0x00, 0x01, 0x03, 0x04];
    assert_eq!(unescape_rbsp(&normal), normal);
}

#[test]
fn test_annex_b_scanner_start_codes() {
    // Annex B stream with 3-byte and 4-byte start codes and leading zeros
    let stream = [
        0x00, 0x00, 0x00, 0x01, 0x67, 0x42, 0x00, 0x1F, // 4-byte start code, SPS NAL (0x67)
        0x00, 0x00, 0x01, 0x68, 0xCE, 0x38, // 3-byte start code, PPS NAL (0x68)
        0x00, 0x00, 0x00, 0x01, 0x65, 0x88, 0x84, // 4-byte start code, IDR Slice (0x65)
    ];

    let mut scanner = AnnexBScanner::new(&stream);

    let nal1 = scanner.next_nal().expect("expected SPS NAL");
    assert_eq!(nal1, &[0x67, 0x42, 0x00, 0x1F]);

    let nal2 = scanner.next_nal().expect("expected PPS NAL");
    assert_eq!(nal2, &[0x68, 0xCE, 0x38]);

    let nal3 = scanner.next_nal().expect("expected IDR NAL");
    assert_eq!(nal3, &[0x65, 0x88, 0x84]);

    assert!(scanner.next_nal().is_none());

    let all_nals = parse_annex_b(&stream);
    assert_eq!(all_nals.len(), 3);
    assert_eq!(all_nals[0], nal1);
    assert_eq!(all_nals[1], nal2);
    assert_eq!(all_nals[2], nal3);
}

#[test]
fn test_avcc_length_prefixed_nals() {
    let nal1 = [0x67, 0x42, 0x00, 0x1F];
    let nal2 = [0x68, 0xCE];

    let mut packet = Vec::new();
    // 4-byte length prefix for NAL 1
    packet.extend_from_slice(&(nal1.len() as u32).to_be_bytes());
    packet.extend_from_slice(&nal1);
    // 4-byte length prefix for NAL 2
    packet.extend_from_slice(&(nal2.len() as u32).to_be_bytes());
    packet.extend_from_slice(&nal2);

    let parsed = parse_avcc(&packet, 4).expect("valid AVCC packet");
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0], nal1);
    assert_eq!(parsed[1], nal2);

    // Test automatic extraction
    let auto_nals = extract_nal_units(&packet);
    assert_eq!(auto_nals.len(), 2);
    assert_eq!(auto_nals[0], nal1);
}

#[test]
fn test_nal_header_validation() {
    // Valid SPS NAL: 0x67 = forbidden_zero(0), ref_idc(3), unit_type(7)
    let hdr = NalHeader::parse(0x67).unwrap();
    assert!(!hdr.forbidden_zero_bit);
    assert_eq!(hdr.nal_ref_idc, 3);
    assert_eq!(hdr.unit_type, NalUnitType::Sps);

    // Corrupted NAL with forbidden_zero_bit = 1: 0xE7
    assert!(NalHeader::parse(0xE7).is_err());
}

// ============================================================================
// 3. SPS, PPS, Slice Header and Decoder Reconstruction Tests
// ============================================================================

#[test]
fn test_sps_parsing_and_dimension_calculation() {
    // 320x240 (20x15 macroblocks) without cropping
    let sps_rbsp = build_synthetic_sps_rbsp(20, 15, None);
    let sps = Sps::parse(&sps_rbsp).expect("valid SPS RBSP");
    assert_eq!(sps.profile_idc, 66);
    assert_eq!(sps.level_idc, 31);
    assert_eq!(sps.visible_dimensions(), (320, 240));

    // 1920x1080 (120x68 macroblocks with top/bottom cropping)
    // 68 macroblocks = 1088 lines; crop_bottom = 4 (crop_unit_y = 2 => 8 lines cropped => 1080)
    let sps_cropped = build_synthetic_sps_rbsp(120, 68, Some((0, 0, 0, 4)));
    let sps_1080p = Sps::parse(&sps_cropped).expect("valid 1080p SPS");
    assert_eq!(sps_1080p.visible_dimensions(), (1920, 1080));
}

#[test]
fn test_pps_and_slice_header_parsing() {
    let sps_rbsp = build_synthetic_sps_rbsp(20, 15, None);
    let sps = Sps::parse(&sps_rbsp).unwrap();

    let pps_rbsp = build_synthetic_pps_rbsp(0, 0);
    let pps = Pps::parse(&pps_rbsp).unwrap();
    assert_eq!(pps.pic_parameter_set_id, 0);
    assert!(!pps.entropy_coding_mode_flag); // CAVLC

    let slice_rbsp = build_synthetic_idr_slice_rbsp(0, 0, 1);
    let mut reader = BitReader::new(&slice_rbsp);
    let nal_hdr = NalHeader::parse(0x65).unwrap(); // IDR slice header
    let slice = SliceHeader::parse(&mut reader, &nal_hdr, &sps, &pps).unwrap();

    assert_eq!(slice.slice_type, SliceType::I);
    assert_eq!(slice.pic_parameter_set_id, 0);
    assert_eq!(slice.idr_pic_id, Some(1));
}

#[test]
fn test_h264_decoder_end_to_end_reconstruction() {
    let mut decoder = H264Decoder::new();

    // Assemble an Annex B packet with SPS, PPS, and an IDR slice
    let sps_rbsp = build_synthetic_sps_rbsp(20, 15, None);
    let pps_rbsp = build_synthetic_pps_rbsp(0, 0);
    let slice_rbsp = build_synthetic_idr_slice_rbsp(0, 0, 1);

    let mut packet = Vec::new();
    // SPS NAL (0x67)
    packet.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x67]);
    packet.extend_from_slice(&sps_rbsp);
    // PPS NAL (0x68)
    packet.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x68]);
    packet.extend_from_slice(&pps_rbsp);
    // Slice NAL (0x65)
    packet.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x65]);
    packet.extend_from_slice(&slice_rbsp);

    let decoded = decoder
        .decode_packet(&packet, 1000)
        .expect("decoding must succeed")
        .expect("must produce a VideoFrameYUV420p");

    assert_eq!(decoded.pts_ms, 1000);
    assert_eq!(decoded.width, 320);
    assert_eq!(decoded.height, 240);

    // Verify 16-byte row stride alignment
    assert!(decoded.stride_y >= 320);
    assert_eq!(decoded.stride_y % 16, 0);
    assert_eq!(decoded.stride_u % 16, 0);
    assert_eq!(decoded.stride_v % 16, 0);

    // Verify 64-byte alignment of planes
    assert!(decoded.y_plane.is_64_aligned());
    assert!(decoded.u_plane.is_64_aligned());
    assert!(decoded.v_plane.is_64_aligned());

    // Verify SIMD tail padding: buffer length = stride * height + 16
    assert_eq!(decoded.y_plane.len(), decoded.stride_y * 240 + 16);
    assert_eq!(decoded.u_plane.len(), decoded.stride_u * 120 + 16);
}

// ============================================================================
// 4. Audio Packet Demuxing & Unpacking Tests
// ============================================================================

#[test]
fn test_audio_pcm_unpacking_formats() {
    let pts = 500;
    let sample_rate = 48000;
    let channels = 2;

    // 1. I16Le test vector: [0.0, 0.5, -1.0, -0.5]
    // 0 -> 0x0000, 16384 (0.5) -> 0x4000, -32768 (-1.0) -> 0x8000, -16384 (-0.5) -> 0xC000
    let i16le_data = [
        0x00, 0x00, // 0.0
        0x00, 0x40, // 0.5
        0x00, 0x80, // -1.0
        0x00, 0xC0, // -0.5
    ];
    let frame = unpack_pcm(&i16le_data, PcmFormat::I16Le, channels, sample_rate, pts).unwrap();
    assert_eq!(frame.samples.len(), 4);
    assert!((frame.samples[0] - 0.0).abs() < 1e-4);
    assert!((frame.samples[1] - 0.5).abs() < 1e-4);
    assert!((frame.samples[2] - (-1.0)).abs() < 1e-4);
    assert!((frame.samples[3] - (-0.5)).abs() < 1e-4);

    // 2. U8 test vector: [0 (128), 127 (-1.0/128), 255 (+127/128)]
    let u8_data = [128, 0, 255];
    let frame_u8 = unpack_pcm(&u8_data, PcmFormat::U8, 1, 44100, pts).unwrap();
    assert_eq!(frame_u8.samples[0], 0.0);
    assert_eq!(frame_u8.samples[1], -1.0);
    assert!((frame_u8.samples[2] - (127.0 / 128.0)).abs() < 1e-4);

    // 3. F32Le test vector
    let f32_data = [
        0.0f32.to_le_bytes(),
        0.75f32.to_le_bytes(),
        (-0.75f32).to_le_bytes(),
        1.0f32.to_le_bytes(),
    ]
    .concat();
    let frame_f32 = unpack_pcm(&f32_data, PcmFormat::F32Le, channels, sample_rate, pts).unwrap();
    assert_eq!(frame_f32.samples, vec![0.0, 0.75, -0.75, 1.0]);

    // 4. NaN rejection
    let nan_data = f32::NAN.to_le_bytes();
    assert!(unpack_pcm(&nan_data, PcmFormat::F32Le, 1, sample_rate, pts).is_err());
}

#[test]
fn test_opus_toc_and_packet_unpacking() {
    // Config 31 (0b00011111): CELT Fullband, 20ms frame duration, stereo (bit 2 = 1), 1 frame (code 0)
    // TOC byte: (31 << 3) | (1 << 2) | 0 = 248 | 4 = 252 (0xFC)
    let toc_byte = 0xFC;
    let toc = OpusToc::parse(toc_byte);
    assert_eq!(toc.mode, OpusMode::CeltOnly);
    assert_eq!(toc.bandwidth, OpusBandwidth::Fullband);
    assert_eq!(toc.frame_duration_ms, 20.0);
    assert!(toc.stereo);
    assert_eq!(toc.frame_count, 1);
    assert_eq!(toc.samples_per_channel, 960); // 20ms at 48kHz = 960 samples

    // Unpack an Opus packet containing this TOC byte
    let packet = [toc_byte, 0x12, 0x34, 0x56, 0x78];
    let frame = unpack_opus_packet(&packet, 48000, 200).expect("valid opus packet");
    assert_eq!(frame.channels, 2);
    assert_eq!(frame.sample_rate, 48000);
    assert_eq!(frame.samples.len(), 960 * 2); // 1920 total interleaved stereo samples
    assert!((frame.duration_secs() - 0.020).abs() < 1e-4);
}

#[test]
fn test_vorbis_identification_header_and_unpacking() {
    let mut header = Vec::new();
    header.push(0x01); // packet type 1
    header.extend_from_slice(b"vorbis");
    header.extend_from_slice(&0u32.to_le_bytes()); // vorbis_version = 0
    header.push(2); // 2 channels
    header.extend_from_slice(&44100u32.to_le_bytes()); // 44.1 kHz
    header.extend_from_slice(&128000i32.to_le_bytes()); // max bitrate
    header.extend_from_slice(&128000i32.to_le_bytes()); // nominal bitrate
    header.extend_from_slice(&128000i32.to_le_bytes()); // min bitrate
    header.push(0x8B); // blocksize 0 = 11 (2^11 = 2048), blocksize 1 = 8
    header.push(1); // framing flag

    let parsed_hdr = VorbisIdentificationHeader::parse(&header).expect("valid vorbis header");
    assert_eq!(parsed_hdr.audio_channels, 2);
    assert_eq!(parsed_hdr.audio_sample_rate, 44100);
    assert!(parsed_hdr.framing_flag);

    // Unpack audio packet
    let packet = [0x55, 0xAA, 0x33, 0xCC];
    let frame = unpack_vorbis_audio_packet(&packet, &parsed_hdr, 300).expect("valid unpack");
    assert_eq!(frame.channels, 2);
    assert_eq!(frame.sample_rate, 44100);
    assert!(!frame.samples.is_empty());
}

#[test]
fn test_audio_decoder_dispatcher() {
    let mut decoder = AudioDecoder::new(AudioCodec::Pcm(PcmFormat::I16Le), 48000, 2);
    assert_eq!(decoder.sample_rate(), 48000);
    assert_eq!(decoder.channels(), 2);

    let pcm_data = [0x00, 0x00, 0x00, 0x00]; // 1 stereo sample of silence
    let frame = decoder.decode_packet(&pcm_data, 50).unwrap();
    assert_eq!(frame.samples, vec![0.0, 0.0]);
}

// ============================================================================
// 5. Terminal TrueColor Rendering & ANSI String Tests
// ============================================================================

#[test]
fn test_terminal_truecolor_renderer_half_block_and_deduplication() {
    let mut yuv = VideoFrameYUV420p::new(32, 24, 0);
    // Fill top half with saturated red (81, 90, 240) and bottom half with saturated white (235, 128, 128)
    for y in 0..12 {
        for x in 0..32 {
            yuv.y_plane.as_mut_slice()[y * yuv.stride_y + x] = 81;
        }
    }
    for y in 12..24 {
        for x in 0..32 {
            yuv.y_plane.as_mut_slice()[y * yuv.stride_y + x] = 235;
        }
    }
    for y in 0..6 {
        for x in 0..16 {
            yuv.u_plane.as_mut_slice()[y * yuv.stride_u + x] = 90;
            yuv.v_plane.as_mut_slice()[y * yuv.stride_v + x] = 240;
        }
    }
    for y in 6..12 {
        for x in 0..16 {
            yuv.u_plane.as_mut_slice()[y * yuv.stride_u + x] = 128;
            yuv.v_plane.as_mut_slice()[y * yuv.stride_v + x] = 128;
        }
    }

    let mut term_buf = TerminalTrueColorBuffer::new(16, 6);
    convert_yuv420p_to_terminal(&yuv, &mut term_buf, ColorStandard::Bt601).unwrap();

    let mut ansi = String::new();
    term_buf.render_ansi(&mut ansi);

    // Verify ANSI half-block presence and color-change deduplication
    assert!(ansi.contains('▀'));
    assert!(ansi.contains("\x1b[38;2;")); // Foreground truecolor
    assert!(ansi.contains("\x1b[48;2;")); // Background truecolor
    assert!(ansi.contains("\x1b[0m")); // Reset sequence

    // Verify size bounding (< 18KB)
    assert!(
        ansi.len() < 18 * 1024,
        "ANSI rendered frame must be < 18KB, was {} bytes",
        ansi.len()
    );
}

#[test]
fn test_rgba_to_terminal_conversion() {
    let rgba = VideoFrameRGBA::new(20, 20, 0);
    let mut term_buf = TerminalTrueColorBuffer::new(10, 5);
    convert_rgba_to_terminal(&rgba, &mut term_buf).unwrap();

    let mut ansi = String::new();
    term_buf.render_ansi(&mut ansi);
    assert_eq!(term_buf.cells.len(), 50);
    assert!(!ansi.is_empty());
}
