//! Adversarial Empirical Challenge Suite for H.264 Bitstream Parser & Decoders.
//!
//! Thoroughly stress-tests:
//! 1. Exp-Golomb bitstream edge cases: extreme leading zero counts (> 31), bitstream truncation,
//!    `read_te` with `max <= 1`, negative `read_se` boundaries.
//! 2. NAL parser fuzzing: corrupt start codes, trailing zeros, malformed AVCC length headers,
//!    emulation prevention bytes `0x000003` at buffer boundaries.
//! 3. SPS/PPS fuzzing: malformed crop offsets, huge macroblock width/height (check for integer
//!    overflow in visible dimensions), unknown NAL types.
//! 4. Memory bounds and safety: fuzzing harnesses ensuring no panics, strongly typed `CodecError`.

#![deny(warnings)]

use std::panic::{catch_unwind, AssertUnwindSafe};
use syncverse_codecs::error::CodecError;
use syncverse_codecs::h264::{
    extract_nal_units, parse_annex_b, parse_avcc, unescape_rbsp, H264Decoder,
    NalHeader, NalUnitType, Pps, Sps,
};
use syncverse_common::error::CommonError;
use syncverse_common::reader::BitReader;

// ============================================================================
// 1. EXP-GOLOMB BITSTREAM EDGE CASES
// ============================================================================

#[test]
fn test_exp_golomb_extreme_leading_zeros() {
    // 1A. Exactly 31 leading zeros followed by 1 and 31 info bits:
    // This is the maximum valid Exp-Golomb representation supported.
    let mut bits_31 = vec![false; 31];
    bits_31.push(true); // stop bit
    bits_31.extend(std::iter::repeat_n(false, 31)); // info bits = 0
    // Pad to byte boundary
    while !bits_31.len().is_multiple_of(8) {
        bits_31.push(false);
    }
    let mut bytes_31 = Vec::new();
    for chunk in bits_31.as_chunks::<8>().0 {
        let mut b = 0u8;
        for (i, &bit) in chunk.iter().enumerate() {
            if bit {
                b |= 1 << (7 - i);
            }
        }
        bytes_31.push(b);
    }
    let mut reader = BitReader::new(&bytes_31);
    let val = reader.read_ue().expect("31 leading zeros should succeed");
    assert_eq!(val, (1u32 << 31) - 1);

    // 1B. Exactly 32 leading zeros: MUST return InvalidData
    let bytes_32 = vec![0x00; 5]; // 40 zero bits
    let mut reader_32 = BitReader::new(&bytes_32);
    let err_32 = reader_32.read_ue().unwrap_err();
    assert!(
        matches!(err_32, CommonError::InvalidData(ref msg) if msg.contains("exceed 31")),
        "Expected InvalidData on > 31 leading zeros, got {:?}",
        err_32
    );

    // 1C. 64 and 1000 leading zeros: MUST return InvalidData
    let bytes_1000 = vec![0x00; 125];
    let mut reader_1000 = BitReader::new(&bytes_1000);
    let err_1000 = reader_1000.read_ue().unwrap_err();
    assert!(
        matches!(err_1000, CommonError::InvalidData(_)),
        "Expected InvalidData on 1000 leading zeros, got {:?}",
        err_1000
    );
}

#[test]
fn test_exp_golomb_bitstream_truncation() {
    // 2A. Truncation while counting leading zeros:
    // 7 zeros followed by EOF (only 7 bits total in buffer)
    let buf_truncated = [0b0000_0000];
    let mut reader = BitReader::new(&buf_truncated[..0]); // 0 bytes
    assert!(matches!(
        reader.read_ue(),
        Err(CommonError::UnexpectedEof { .. })
    ));

    // 2B. Truncation while reading info bits:
    // Leading zeros = 4 ('00001'), requires 4 info bits, but only 2 info bits present ('01')
    // Total 7 bits: 0000101 (padding bit not enough)
    let buf_info_trunc = [0b0000_1010]; // 4 zeros, 1, then '01', then 1 bit
    let mut reader2 = BitReader::new(&buf_info_trunc);
    // Needs 4 info bits, only 3 remaining bits in byte
    assert!(matches!(
        reader2.read_ue(),
        Err(CommonError::UnexpectedEof {
            needed: 4,
            available: 3
        })
    ));

    // 2C. Truncation in read_se:
    let mut reader3 = BitReader::new(&[]);
    assert!(matches!(
        reader3.read_se(),
        Err(CommonError::UnexpectedEof { .. })
    ));

    // 2D. Truncation in read_bits:
    let buf_bits = [0xFF];
    let mut reader4 = BitReader::new(&buf_bits);
    assert!(matches!(
        reader4.read_bits(9),
        Err(CommonError::UnexpectedEof {
            needed: 9,
            available: 8
        })
    ));
    assert!(matches!(
        reader4.read_bits(65),
        Err(CommonError::InvalidData(_))
    ));
}

#[test]
fn test_read_te_boundaries() {
    // 3A. max == 0: MUST return Ok(0) without consuming any bits
    let buf = [0b1100_0000];
    let mut reader = BitReader::new(&buf);
    assert_eq!(reader.remaining_bits(), 8);
    let val0 = reader.read_te(0).unwrap();
    assert_eq!(val0, 0);
    assert_eq!(reader.remaining_bits(), 8, "read_te(0) must consume 0 bits");

    // 3B. max == 1:
    // Bit '1' -> returns 0
    let val1 = reader.read_te(1).unwrap();
    assert_eq!(val1, 0);
    assert_eq!(reader.remaining_bits(), 7);

    // Bit '1' again -> returns 0
    let val1_b = reader.read_te(1).unwrap();
    assert_eq!(val1_b, 0);
    assert_eq!(reader.remaining_bits(), 6);

    // Next bit is '0' -> returns 1
    let val1_c = reader.read_te(1).unwrap();
    assert_eq!(val1_c, 1);
    assert_eq!(reader.remaining_bits(), 5);

    // 3C. max == 1 with EOF: MUST return UnexpectedEof
    let empty_buf = [];
    let mut empty_reader = BitReader::new(&empty_buf);
    assert!(matches!(
        empty_reader.read_te(1),
        Err(CommonError::UnexpectedEof { .. })
    ));

    // 3D. max > 1: Delegates to read_ue()
    // Bit pattern for ue(2) is '011'
    let buf_te2 = [0b0110_0000];
    let mut reader_te2 = BitReader::new(&buf_te2);
    assert_eq!(reader_te2.read_te(5).unwrap(), 2);
}

#[test]
fn test_read_se_negative_and_overflow_boundaries() {
    // 4A. Mapping check:
    // k = 0 -> 0
    // k = 1 -> 1
    // k = 2 -> -1
    // k = 3 -> 2
    // k = 4 -> -2
    // k = 5 -> 3
    // k = 6 -> -3
    let bits = [0b1010_0110, 0b0100_0010, 0b1001_1000, 0b1110_0000];
    let mut reader = BitReader::new(&bits);
    assert_eq!(reader.read_se().unwrap(), 0);
    assert_eq!(reader.read_se().unwrap(), 1);
    assert_eq!(reader.read_se().unwrap(), -1);
    assert_eq!(reader.read_se().unwrap(), 2);
    assert_eq!(reader.read_se().unwrap(), -2);
    assert_eq!(reader.read_se().unwrap(), 3);
    assert_eq!(reader.read_se().unwrap(), -3);

    // 4B. Maximum positive boundary:
    // k = 4294967293 (odd):
    // k.div_ceil(2) = 2147483647 = i32::MAX.
    // 31 leading zeros, 1, info = 2147483646
    let mut b = vec![false; 31];
    b.push(true);
    let info = 2147483646u64;
    for i in (0..31).rev() {
        b.push(((info >> i) & 1) != 0);
    }
    while !b.len().is_multiple_of(8) {
        b.push(false);
    }
    let mut raw = Vec::new();
    for chunk in b.as_chunks::<8>().0 {
        let mut byte = 0u8;
        for (i, &bit) in chunk.iter().enumerate() {
            if bit {
                byte |= 1 << (7 - i);
            }
        }
        raw.push(byte);
    }
    let mut reader_max = BitReader::new(&raw);
    let se_max = reader_max.read_se().expect("i32::MAX boundary should succeed");
    assert_eq!(se_max, i32::MAX);

    // 4C. Maximum negative boundary:
    // k = 4294967294 (even):
    // - (k / 2) = -2147483647
    let mut b_neg = vec![false; 31];
    b_neg.push(true);
    let info_neg = 2147483647u64;
    for i in (0..31).rev() {
        b_neg.push(((info_neg >> i) & 1) != 0);
    }
    while !b_neg.len().is_multiple_of(8) {
        b_neg.push(false);
    }
    let mut raw_neg = Vec::new();
    for chunk in b_neg.as_chunks::<8>().0 {
        let mut byte = 0u8;
        for (i, &bit) in chunk.iter().enumerate() {
            if bit {
                byte |= 1 << (7 - i);
            }
        }
        raw_neg.push(byte);
    }
    let mut reader_neg = BitReader::new(&raw_neg);
    let se_neg = reader_neg.read_se().expect("negative boundary should succeed");
    assert_eq!(se_neg, -2147483647);
}

// ============================================================================
// 2. NAL PARSER FUZZING & START CODE RESILIENCE
// ============================================================================

#[test]
fn test_nal_parser_corrupt_start_codes() {
    // 5A. Empty buffer
    assert!(parse_annex_b(&[]).is_empty());
    assert!(extract_nal_units(&[]).is_empty());

    // 5B. Short non-start-code buffers
    let short_buffers: &[&[u8]] = &[
        &[0x00],
        &[0x00, 0x00],
        &[0x00, 0x01],
        &[0x00, 0x00, 0x02],
        &[0x01, 0x02, 0x03],
        &[0xFF, 0xFF, 0xFF, 0xFF],
    ];
    for &buf in short_buffers {
        assert!(parse_annex_b(buf).is_empty(), "Expected empty for {:?}", buf);
        assert!(extract_nal_units(buf).is_empty(), "Expected empty for {:?}", buf);
    }

    // 5C. Long sequences of zeros with no start code
    let zeros = vec![0x00u8; 4096];
    assert!(parse_annex_b(&zeros).is_empty());
    assert!(extract_nal_units(&zeros).is_empty());

    // 5D. Start code at EOF with no payload
    assert!(parse_annex_b(&[0x00, 0x00, 0x01]).is_empty());
    assert!(parse_annex_b(&[0x00, 0x00, 0x00, 0x01]).is_empty());

    // 5E. Multiple consecutive start codes back-to-back
    let back_to_back = [0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x67, 0x42];
    let nals = parse_annex_b(&back_to_back);
    assert_eq!(nals.len(), 1);
    assert_eq!(nals[0], &[0x67, 0x42]);

    // 5F. 50 zeros followed by start code
    let mut long_prefix = vec![0x00u8; 50];
    long_prefix.extend_from_slice(&[0x01, 0x65, 0x88]);
    let nals_prefix = parse_annex_b(&long_prefix);
    assert_eq!(nals_prefix.len(), 1);
    assert_eq!(nals_prefix[0], &[0x65, 0x88]);
}

#[test]
fn test_nal_parser_trailing_zeros() {
    // 6A. Trailing zeros after NAL at EOF:
    // [0x00, 0x00, 0x01, 0x65, 0xAA, 0x00, 0x00, 0x00]
    let with_trailing = [0x00, 0x00, 0x01, 0x65, 0xAA, 0x00, 0x00, 0x00];
    let nals = parse_annex_b(&with_trailing);
    assert_eq!(nals.len(), 1);
    assert_eq!(nals[0], &[0x65, 0xAA], "Trailing zeros must be trimmed from NAL payload");

    // 6B. Trailing zeros between two NALs:
    let between = [
        0x00, 0x00, 0x01, 0x67, 0x11, 0x00, 0x00, 0x00, 0x01, 0x68, 0x22, 0x00,
    ];
    let nals_between = parse_annex_b(&between);
    assert_eq!(nals_between.len(), 2);
    assert_eq!(nals_between[0], &[0x67, 0x11]);
    assert_eq!(nals_between[1], &[0x68, 0x22]);

    // 6C. NAL with ONLY zeros between start codes:
    let only_zeros = [0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x01, 0x65];
    let nals_zeros = parse_annex_b(&only_zeros);
    assert_eq!(nals_zeros.len(), 1);
    assert_eq!(nals_zeros[0], &[0x65]);
}

#[test]
fn test_malformed_avcc_length_headers() {
    // 7A. Unsupported length sizes (0, 3, 5, 8): MUST return InvalidNal
    for unsupported_size in [0, 3, 5, 8] {
        let err = parse_avcc(&[0x00, 0x01, 0x65], unsupported_size).unwrap_err();
        assert!(
            matches!(err, CodecError::InvalidNal(ref msg) if msg.contains("unsupported AVCC")),
            "Expected InvalidNal on unsupported size {}, got {:?}",
            unsupported_size,
            err
        );
    }

    // 7B. Buffer shorter than length_size:
    let short_data = [0x00, 0x01];
    let res = parse_avcc(&short_data, 4).unwrap();
    assert!(res.is_empty(), "Short data with len < length_size should return empty list");

    // 7C. NAL length exceeding remaining buffer:
    // Header indicates 16 bytes, but only 2 bytes exist
    let exceeding_data = [0x00, 0x00, 0x00, 0x10, 0x65, 0xAA];
    let err_exceed = parse_avcc(&exceeding_data, 4).unwrap_err();
    assert!(
        matches!(err_exceed, CodecError::InvalidNal(ref msg) if msg.contains("exceeds remaining")),
        "Expected InvalidNal on exceeding length, got {:?}",
        err_exceed
    );

    // 7D. Huge NAL length (0xFFFFFFFF) in 4-byte AVCC:
    let huge_data = [0xFF, 0xFF, 0xFF, 0xFF, 0x65];
    let err_huge = parse_avcc(&huge_data, 4).unwrap_err();
    assert!(
        matches!(err_huge, CodecError::InvalidNal(_)),
        "Expected InvalidNal on 0xFFFFFFFF length, got {:?}",
        err_huge
    );

    // 7E. Zero-length NAL handling:
    let zero_len_data = [
        0x00, 0x00, 0x00, 0x00, // nal_len = 0
        0x00, 0x00, 0x00, 0x02, // nal_len = 2
        0x67, 0x42,
    ];
    let nals = parse_avcc(&zero_len_data, 4).unwrap();
    assert_eq!(nals.len(), 1);
    assert_eq!(nals[0], &[0x67, 0x42]);
}

#[test]
fn test_emulation_prevention_boundary_conditions() {
    // 8A. Exactly [0x00, 0x00, 0x03] at buffer boundary
    assert_eq!(unescape_rbsp(&[0x00, 0x00, 0x03]), vec![0x00, 0x00]);

    // 8B. Emulation prevention followed by 0x00, 0x01, 0x02, 0x03
    assert_eq!(unescape_rbsp(&[0x00, 0x00, 0x03, 0x00]), vec![0x00, 0x00, 0x00]);
    assert_eq!(unescape_rbsp(&[0x00, 0x00, 0x03, 0x01]), vec![0x00, 0x00, 0x01]);
    assert_eq!(unescape_rbsp(&[0x00, 0x00, 0x03, 0x02]), vec![0x00, 0x00, 0x02]);
    assert_eq!(unescape_rbsp(&[0x00, 0x00, 0x03, 0x03]), vec![0x00, 0x00, 0x03]);

    // 8C. Incomplete sequences at buffer end
    assert_eq!(unescape_rbsp(&[0x00, 0x00]), vec![0x00, 0x00]);
    assert_eq!(unescape_rbsp(&[0x00]), vec![0x00]);
    assert_eq!(unescape_rbsp(&[]), Vec::<u8>::new());

    // 8D. Cascaded emulation prevention sequences
    assert_eq!(
        unescape_rbsp(&[0x00, 0x00, 0x03, 0x00, 0x00, 0x03]),
        vec![0x00, 0x00, 0x00, 0x00]
    );

    // 8E. Extra zero before emulation prevention
    assert_eq!(
        unescape_rbsp(&[0x00, 0x00, 0x00, 0x03]),
        vec![0x00, 0x00, 0x00]
    );
}

// ============================================================================
// 3. SPS / PPS FUZZING & DIMENSION OVERFLOW INVESTIGATION
// ============================================================================

#[test]
fn test_nal_header_forbidden_bit_and_unknown_types() {
    // 9A. forbidden_zero_bit MUST be rejected with InvalidNal
    let forbidden_header = NalHeader::parse(0x80 | 1);
    assert!(
        matches!(forbidden_header, Err(CodecError::InvalidNal(ref msg)) if msg.contains("forbidden")),
        "Expected InvalidNal on forbidden_zero_bit = 1, got {:?}",
        forbidden_header
    );

    // 9B. Unknown NAL unit types (16..31) must map to NalUnitType::Other
    for id in 16..=31u8 {
        let header = NalHeader::parse(id).unwrap();
        assert_eq!(header.unit_type, NalUnitType::Other(id));
        assert_eq!(header.unit_type.type_id(), id);
    }

    // 9C. NAL type 0 is Unspecified
    let header0 = NalHeader::parse(0).unwrap();
    assert_eq!(header0.unit_type, NalUnitType::Unspecified);
}

#[test]
fn test_sps_visible_dimensions_crop_saturation() {
    // Construct an SPS with frame cropping exceeding dimensions
    let sps = Sps {
        profile_idc: 66,
        constraint_set_flags: 0,
        level_idc: 31,
        seq_parameter_set_id: 0,
        chroma_format_idc: 1, // 4:2:0 -> crop_unit_x = 2, crop_unit_y = 2
        separate_colour_plane_flag: false,
        bit_depth_luma_minus8: 0,
        bit_depth_chroma_minus8: 0,
        qpprime_y_zero_transform_bypass_flag: false,
        seq_scaling_matrix_present_flag: false,
        log2_max_frame_num_minus4: 0,
        pic_order_cnt_type: 0,
        log2_max_pic_order_cnt_lsb_minus4: 0,
        delta_pic_order_always_zero_flag: false,
        offset_for_non_ref_pic: 0,
        offset_for_top_to_bottom_field: 0,
        num_ref_frames_in_pic_order_cnt_cycle: 0,
        offset_for_ref_frame: Vec::new(),
        max_num_ref_frames: 1,
        gaps_in_frame_num_value_allowed_flag: false,
        pic_width_in_mbs_minus1: 0, // raw_width = 16
        pic_height_in_map_units_minus1: 0, // raw_height = 16
        frame_mbs_only_flag: true,
        mb_adaptive_frame_field_flag: false,
        direct_8x8_inference_flag: true,
        frame_cropping_flag: true,
        frame_crop_left_offset: 10,  // crop_x = (10 + 10) * 2 = 40 > 16
        frame_crop_right_offset: 10,
        frame_crop_top_offset: 10,   // crop_y = (10 + 10) * 2 = 40 > 16
        frame_crop_bottom_offset: 10,
        vui_parameters_present_flag: false,
    };

    let (w, h) = sps.visible_dimensions();
    assert_eq!(w, 0, "Crop exceeding width must saturate to 0");
    assert_eq!(h, 0, "Crop exceeding height must saturate to 0");
}

#[test]
fn test_sps_visible_dimensions_integer_overflow_vulnerabilities() {
    // INVESTIGATION: Does Sps::visible_dimensions() panic on arithmetic overflow
    // when pic_width_in_mbs_minus1 or crop offsets are near u32::MAX?

    // Scenario 1: Huge pic_width_in_mbs_minus1 = 0x1000_0000 (268,435,456)
    // (0x1000_0000 + 1) * 16 = 4,294,967,312 > u32::MAX!
    let sps_huge_width = Sps {
        profile_idc: 66,
        constraint_set_flags: 0,
        level_idc: 31,
        seq_parameter_set_id: 0,
        chroma_format_idc: 1,
        separate_colour_plane_flag: false,
        bit_depth_luma_minus8: 0,
        bit_depth_chroma_minus8: 0,
        qpprime_y_zero_transform_bypass_flag: false,
        seq_scaling_matrix_present_flag: false,
        log2_max_frame_num_minus4: 0,
        pic_order_cnt_type: 0,
        log2_max_pic_order_cnt_lsb_minus4: 0,
        delta_pic_order_always_zero_flag: false,
        offset_for_non_ref_pic: 0,
        offset_for_top_to_bottom_field: 0,
        num_ref_frames_in_pic_order_cnt_cycle: 0,
        offset_for_ref_frame: Vec::new(),
        max_num_ref_frames: 1,
        gaps_in_frame_num_value_allowed_flag: false,
        pic_width_in_mbs_minus1: 0x1000_0000,
        pic_height_in_map_units_minus1: 0,
        frame_mbs_only_flag: true,
        mb_adaptive_frame_field_flag: false,
        direct_8x8_inference_flag: true,
        frame_cropping_flag: false,
        frame_crop_left_offset: 0,
        frame_crop_right_offset: 0,
        frame_crop_top_offset: 0,
        frame_crop_bottom_offset: 0,
        vui_parameters_present_flag: false,
    };

    let result_w = catch_unwind(AssertUnwindSafe(|| {
        sps_huge_width.visible_dimensions()
    }));
    assert!(result_w.is_ok(), "sps_huge_width.visible_dimensions() must not panic");
    let (w1, h1) = result_w.unwrap();
    assert_eq!(w1, 16384);
    assert_eq!(h1, 16);

    // Scenario 2: pic_width_in_mbs_minus1 = u32::MAX
    let sps_max_width = Sps {
        pic_width_in_mbs_minus1: u32::MAX,
        ..sps_huge_width.clone()
    };
    let result_max_w = catch_unwind(AssertUnwindSafe(|| {
        sps_max_width.visible_dimensions()
    }));
    assert!(result_max_w.is_ok(), "sps_max_width.visible_dimensions() must not panic");
    let (w2, h2) = result_max_w.unwrap();
    assert_eq!(w2, 16384);
    assert_eq!(h2, 16);

    // Scenario 3: Malformed crop offset overflow:
    // frame_crop_left_offset = 0x8000_0000, frame_crop_right_offset = 0x8000_0000
    let sps_crop_overflow = Sps {
        pic_width_in_mbs_minus1: 10,
        pic_height_in_map_units_minus1: 10,
        frame_cropping_flag: true,
        frame_crop_left_offset: 0x8000_0000,
        frame_crop_right_offset: 0x8000_0000,
        ..sps_huge_width
    };
    let result_crop = catch_unwind(AssertUnwindSafe(|| {
        sps_crop_overflow.visible_dimensions()
    }));
    assert!(result_crop.is_ok(), "sps_crop_overflow.visible_dimensions() must not panic");
    let (w3, _h3) = result_crop.unwrap();
    assert_eq!(w3, 0); // Crop saturates to 0
}

// ============================================================================
// 4. MEMORY BOUNDS & PSEUDO-RANDOM BITSTREAM FUZZING
// ============================================================================

/// Minimal LCG pseudorandom number generator (zero external crates).
struct SimpleRng {
    state: u64,
}

impl SimpleRng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1);
        (self.state >> 32) as u32
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        for chunk in dest.chunks_mut(4) {
            let val = self.next_u32();
            let bytes = val.to_le_bytes();
            for (d, s) in chunk.iter_mut().zip(bytes.iter()) {
                *d = *s;
            }
        }
    }
}

#[test]
fn test_pseudo_random_packet_fuzzing_safety() {
    let mut rng = SimpleRng::new(0xDEADBEEFCAFE1234);

    // Run 2,000 iterations of random bitstream packets across all parser entrypoints
    for i in 0..2000 {
        let len = (rng.next_u32() % 512) as usize;
        let mut buf = vec![0u8; len];
        rng.fill_bytes(&mut buf);

        // Optionally inject start codes
        if len >= 4 && (i % 3 == 0) {
            buf[0] = 0x00;
            buf[1] = 0x00;
            buf[2] = 0x01;
            buf[3] = (rng.next_u32() % 32) as u8;
        } else if len >= 4 && (i % 3 == 1) {
            buf[0] = 0x00;
            buf[1] = 0x00;
            buf[2] = 0x00;
            buf[3] = 0x01;
        }

        // Test unescape_rbsp: must never panic
        let rbsp = unescape_rbsp(&buf);
        assert!(rbsp.len() <= buf.len());

        // Test parse_annex_b: must never panic
        let nals = parse_annex_b(&buf);
        for nal in nals {
            assert!(!nal.is_empty());
        }

        // Test parse_avcc with sizes 1, 2, 4: must never panic
        for &l_size in &[1, 2, 4] {
            let _ = parse_avcc(&buf, l_size);
        }

        // Test extract_nal_units: must never panic
        let extracted = extract_nal_units(&buf);
        let _ = extracted;

        // Test Sps::parse: must never panic
        let _ = Sps::parse(&rbsp);

        // Test Pps::parse: must never panic
        let _ = Pps::parse(&rbsp);

        // Test H264Decoder::decode_packet: must never panic
        let mut decoder = H264Decoder::new();
        let decode_res = catch_unwind(AssertUnwindSafe(|| {
            let _ = decoder.decode_packet(&buf, i as u64 * 33);
        }));
        assert!(
            decode_res.is_ok(),
            "H264Decoder::decode_packet panicked on iteration {} with len {}",
            i,
            len
        );
    }
}

/// In-test helper to construct standard-compliant H.264 bitstream test vectors.
struct TestBitstreamBuilder {
    bits: Vec<bool>,
}

impl TestBitstreamBuilder {
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

fn build_test_sps(sps_id: u32, width_mbs: u32, height_mbs: u32) -> Vec<u8> {
    let mut b = TestBitstreamBuilder::new();
    b.write_bits(66, 8); // Baseline
    b.write_bits(0, 8);
    b.write_bits(31, 8);
    b.write_ue(sps_id);
    b.write_ue(0);
    b.write_ue(0);
    b.write_ue(0);
    b.write_ue(1);
    b.write_bit(false);
    b.write_ue(width_mbs - 1);
    b.write_ue(height_mbs - 1);
    b.write_bit(true);
    b.write_bit(true);
    b.write_bit(false); // frame_cropping_flag
    b.write_bit(false);
    b.build_rbsp()
}

fn build_test_pps(sps_id: u32, pps_id: u32) -> Vec<u8> {
    let mut b = TestBitstreamBuilder::new();
    b.write_ue(pps_id);
    b.write_ue(sps_id);
    b.write_bit(false);
    b.write_bit(false);
    b.write_ue(0);
    b.write_ue(0);
    b.write_ue(0);
    b.write_bit(false);
    b.write_bits(0, 2);
    b.write_se(0);
    b.write_se(0);
    b.write_se(0);
    b.write_bit(false);
    b.write_bit(false);
    b.write_bit(false);
    b.build_rbsp()
}

fn build_test_slice(pps_id: u32, first_mb: u32, frame_num: u64) -> Vec<u8> {
    let mut b = TestBitstreamBuilder::new();
    b.write_ue(first_mb);
    b.write_ue(7); // I slice
    b.write_ue(pps_id);
    b.write_bits(frame_num, 4);
    b.write_ue(1); // idr_pic_id
    b.write_bits(0, 4);
    b.write_bit(false);
    b.write_bit(false);
    b.write_se(0);
    for i in 0..64 {
        b.write_bits(i % 16, 4);
    }
    b.build_rbsp()
}

#[test]
fn test_h264_decoder_dimension_change_prev_frame_out_of_bounds() {
    let mut decoder = H264Decoder::new();

    // 1. Packet 1: 16x16 resolution (1x1 macroblock)
    let sps1 = build_test_sps(0, 1, 1);
    let pps1 = build_test_pps(0, 0);
    let slice1 = build_test_slice(0, 0, 0);

    let mut packet1 = Vec::new();
    packet1.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x67]);
    packet1.extend_from_slice(&sps1);
    packet1.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x68]);
    packet1.extend_from_slice(&pps1);
    packet1.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x65]);
    packet1.extend_from_slice(&slice1);

    let frame1 = decoder.decode_packet(&packet1, 0).expect("packet1 must succeed").expect("frame 1");
    assert_eq!(frame1.width, 16);
    assert_eq!(frame1.height, 16);

    // 2. Packet 2: 64x64 resolution (4x4 macroblocks), but slice starts at macroblock 5!
    // Macroblocks 0..4 should be copied from last_frame.
    // However, last_frame is 16x16! Macroblock 4 is at (mb_x=0, mb_y=1), y=16..31.
    // Indexing y=17 * prev.stride_y(16) + 0 = 272 >= prev.len(272) will PANIC!
    let sps2 = build_test_sps(1, 4, 4);
    let pps2 = build_test_pps(1, 1);
    let slice2 = build_test_slice(1, 5, 1); // first_mb = 5

    let mut packet2 = Vec::new();
    packet2.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x67]);
    packet2.extend_from_slice(&sps2);
    packet2.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x68]);
    packet2.extend_from_slice(&pps2);
    packet2.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x65]);
    packet2.extend_from_slice(&slice2);

    let result = catch_unwind(AssertUnwindSafe(|| {
        decoder.decode_packet(&packet2, 33)
    }));

    assert!(result.is_ok(), "decoder.decode_packet() must not panic on dimension change");
    let decoded = result.unwrap();
    let frame2 = decoded.expect("packet2 must decode successfully").expect("frame 2");
    assert_eq!(frame2.width, 64);
    assert_eq!(frame2.height, 64);
}

#[test]
fn test_scaling_matrix_delta_scale_overflow() {
    // High Profile SPS with seq_scaling_matrix_present_flag = 1
    // and delta_scale = i32::MAX
    let mut b = TestBitstreamBuilder::new();
    b.write_bits(100, 8); // profile_idc: High Profile (100)
    b.write_bits(0, 8);
    b.write_bits(31, 8);
    b.write_ue(0); // seq_parameter_set_id
    b.write_ue(1); // chroma_format_idc: 4:2:0
    b.write_ue(0); // bit_depth_luma_minus8
    b.write_ue(0); // bit_depth_chroma_minus8
    b.write_bit(false); // qpprime_y_zero_transform_bypass_flag
    b.write_bit(true); // seq_scaling_matrix_present_flag = 1
    b.write_bit(true); // seq_scaling_list_present_flag = 1 (first list)
    // delta_scale = i32::MAX
    b.write_se(i32::MAX);
    // Pad remaining
    for _ in 0..15 {
        b.write_se(0);
    }
    for _ in 1..8 {
        b.write_bit(false); // other scaling lists not present
    }
    b.write_ue(0); // log2_max_frame_num_minus4
    b.write_ue(0); // pic_order_cnt_type
    b.write_ue(0);
    b.write_ue(1);
    b.write_bit(false);
    b.write_ue(0);
    b.write_ue(0);
    b.write_bit(true);
    b.write_bit(true);
    b.write_bit(false);
    b.write_bit(false);
    let rbsp = b.build_rbsp();

    let result = catch_unwind(AssertUnwindSafe(|| {
        Sps::parse(&rbsp)
    }));

    assert!(result.is_ok(), "Sps::parse must not panic on scaling list delta_scale overflow");
    assert!(result.unwrap().is_ok());
}

#[test]
fn test_pps_scaling_matrix_delta_scale_overflow() {
    // PPS with transform_8x8_mode_flag = 1, pic_scaling_matrix_present_flag = 1
    // and delta_scale = i32::MAX
    let mut b = TestBitstreamBuilder::new();
    b.write_ue(0); // pic_parameter_set_id
    b.write_ue(0); // seq_parameter_set_id
    b.write_bit(false); // entropy_coding_mode_flag
    b.write_bit(false); // bottom_field_pic_order_in_frame_present_flag
    b.write_ue(0); // num_slice_groups_minus1
    b.write_ue(0); // num_ref_idx_l0_default_active_minus1
    b.write_ue(0); // num_ref_idx_l1_default_active_minus1
    b.write_bit(false); // weighted_pred_flag
    b.write_bits(0, 2); // weighted_bipred_idc
    b.write_se(0); // pic_init_qp_minus26
    b.write_se(0); // pic_init_qs_minus26
    b.write_se(0); // chroma_qp_index_offset
    b.write_bit(false); // deblocking_filter_control_present_flag
    b.write_bit(false); // constrained_intra_pred_flag
    b.write_bit(false); // redundant_pic_cnt_present_flag

    // Extensions present in more_rbsp_data
    b.write_bit(true); // transform_8x8_mode_flag
    b.write_bit(true); // pic_scaling_matrix_present_flag
    b.write_bit(true); // first list present
    b.write_se(i32::MAX); // delta_scale = i32::MAX -> Must safely handle in i64!
    for _ in 0..15 {
        b.write_se(0);
    }
    for _ in 1..8 {
        b.write_bit(false);
    }
    b.write_se(0); // second_chroma_qp_index_offset
    let rbsp = b.build_rbsp();

    let result = catch_unwind(AssertUnwindSafe(|| {
        Pps::parse(&rbsp)
    }));

    assert!(result.is_ok(), "Pps::parse must not panic on scaling list delta_scale overflow");
    assert!(result.unwrap().is_ok());
}

#[test]
fn test_sps_parse_rejects_huge_macroblock_dimensions() {
    let rbsp_huge_w = build_test_sps(0, 1025, 10);
    let res = Sps::parse(&rbsp_huge_w);
    assert!(
        matches!(res, Err(CodecError::InvalidSps(ref msg)) if msg.contains("less than 1024")),
        "Expected InvalidSps for pic_width_in_mbs_minus1 >= 1024, got {:?}",
        res
    );

    let rbsp_huge_h = build_test_sps(0, 10, 1025);
    let res_h = Sps::parse(&rbsp_huge_h);
    assert!(
        matches!(res_h, Err(CodecError::InvalidSps(ref msg)) if msg.contains("less than 1024")),
        "Expected InvalidSps for pic_height_in_map_units_minus1 >= 1024, got {:?}",
        res_h
    );
}




