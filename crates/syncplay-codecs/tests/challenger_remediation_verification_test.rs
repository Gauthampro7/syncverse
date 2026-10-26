//! Empirical Verification Suite for Milestone 1 Remediations in syncplay-codecs.
//!
//! Independently verifies:
//! 1. Dynamic resolution transitions with `first_mb_in_slice > 0` in `H264Decoder`.
//! 2. Arithmetic overflows in `Sps::parse` scaling lists and `visible_dimensions()`.
//! 3. Scaling lists in `Pps::parse` with boundary `delta_scale` values (`i32::MAX`, `i32::MIN`, negative).
//! 4. Consecutive dynamic resolution hops, multi-slice dimension switches, and 8K bounds enforcement.

#![deny(warnings)]

use std::panic::{catch_unwind, AssertUnwindSafe};
use syncplay_codecs::error::CodecError;
use syncplay_codecs::h264::{H264Decoder, Pps, Sps};

/// Helper bitstream builder for synthetic H.264 NAL units.
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
            ((- (val as i64)) as u64) * 2
        } else {
            (val as u64) * 2 - 1
        };
        assert!(k <= u32::MAX as u64, "k must fit in Exp-Golomb 32 bits");
        self.write_ue(k as u32);
    }

    fn build_rbsp(&mut self) -> Vec<u8> {
        self.write_bit(true); // rbsp_stop_one_bit
        while !self.bits.len().is_multiple_of(8) {
            self.write_bit(false); // rbsp_alignment_zero_bit
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

fn build_sps_rbsp(sps_id: u32, width_mbs: u32, height_mbs: u32) -> Vec<u8> {
    let mut b = BitstreamBuilder::new();
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
    b.write_bit(true); // frame_mbs_only_flag
    b.write_bit(true); // direct_8x8_inference_flag
    b.write_bit(false); // frame_cropping_flag
    b.write_bit(false); // vui_parameters_present_flag
    b.build_rbsp()
}

fn build_pps_rbsp(sps_id: u32, pps_id: u32) -> Vec<u8> {
    let mut b = BitstreamBuilder::new();
    b.write_ue(pps_id);
    b.write_ue(sps_id);
    b.write_bit(false); // entropy_coding_mode_flag
    b.write_bit(false); // bottom_field
    b.write_ue(0); // num_slice_groups_minus1
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

fn build_slice_rbsp(pps_id: u32, first_mb: u32, frame_num: u64) -> Vec<u8> {
    let mut b = BitstreamBuilder::new();
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

fn make_packet(sps_rbsp: &[u8], pps_rbsp: &[u8], slice_rbsp: &[u8]) -> Vec<u8> {
    let mut pkt = Vec::new();
    pkt.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x67]);
    pkt.extend_from_slice(sps_rbsp);
    pkt.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x68]);
    pkt.extend_from_slice(pps_rbsp);
    pkt.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x65]);
    pkt.extend_from_slice(slice_rbsp);
    pkt
}

// ============================================================================
// 1. DYNAMIC RESOLUTION TRANSITIONS WITH first_mb_in_slice > 0
// ============================================================================

#[test]
fn test_resolution_expansion_various_first_mb_offsets() {
    let mut decoder = H264Decoder::new();

    // Initial 16x16 frame (1x1 MB)
    let sps0 = build_sps_rbsp(0, 1, 1);
    let pps0 = build_pps_rbsp(0, 0);
    let slc0 = build_slice_rbsp(0, 0, 0);
    let pkt0 = make_packet(&sps0, &pps0, &slc0);
    let f0 = decoder.decode_packet(&pkt0, 0).unwrap().unwrap();
    assert_eq!(f0.width, 16);
    assert_eq!(f0.height, 16);

    // Expand to 128x128 (8x8 = 64 MBs) with multiple first_mb offsets
    for (idx, &first_mb) in [1, 2, 8, 15, 32, 63].iter().enumerate() {
        let sps = build_sps_rbsp((idx + 1) as u32, 8, 8);
        let pps = build_pps_rbsp((idx + 1) as u32, (idx + 1) as u32);
        let slc = build_slice_rbsp((idx + 1) as u32, first_mb, (idx + 1) as u64);
        let pkt = make_packet(&sps, &pps, &slc);

        let res = catch_unwind(AssertUnwindSafe(|| {
            decoder.decode_packet(&pkt, (idx as u64 + 1) * 33)
        }));
        assert!(res.is_ok(), "Panicked on first_mb = {first_mb}");
        let frame = res.unwrap().unwrap().unwrap();
        assert_eq!(frame.width, 128);
        assert_eq!(frame.height, 128);
    }
}

#[test]
fn test_resolution_shrinking_with_first_mb_offsets() {
    let mut decoder = H264Decoder::new();

    // Initial 128x128 frame (8x8 = 64 MBs)
    let sps0 = build_sps_rbsp(0, 8, 8);
    let pps0 = build_pps_rbsp(0, 0);
    let slc0 = build_slice_rbsp(0, 0, 0);
    let pkt0 = make_packet(&sps0, &pps0, &slc0);
    let f0 = decoder.decode_packet(&pkt0, 0).unwrap().unwrap();
    assert_eq!(f0.width, 128);
    assert_eq!(f0.height, 128);

    // Shrink to 16x16 (1x1 = 1 MB) with first_mb = 0, 1, 5, 100
    for (idx, &first_mb) in [0, 1, 5, 100].iter().enumerate() {
        let sps = build_sps_rbsp((idx + 1) as u32, 1, 1);
        let pps = build_pps_rbsp((idx + 1) as u32, (idx + 1) as u32);
        let slc = build_slice_rbsp((idx + 1) as u32, first_mb, (idx + 1) as u64);
        let pkt = make_packet(&sps, &pps, &slc);

        let res = catch_unwind(AssertUnwindSafe(|| {
            decoder.decode_packet(&pkt, (idx as u64 + 1) * 33)
        }));
        assert!(res.is_ok(), "Panicked on shrink with first_mb = {first_mb}");
        let frame = res.unwrap().unwrap().unwrap();
        assert_eq!(frame.width, 16);
        assert_eq!(frame.height, 16);
    }
}

#[test]
fn test_multi_slice_packet_with_dimension_change() {
    let mut decoder = H264Decoder::new();

    // Packet 1: 32x32 frame (2x2 MBs)
    let sps1 = build_sps_rbsp(0, 2, 2);
    let pps1 = build_pps_rbsp(0, 0);
    let slc1 = build_slice_rbsp(0, 0, 0);
    let pkt1 = make_packet(&sps1, &pps1, &slc1);
    let _ = decoder.decode_packet(&pkt1, 0).unwrap().unwrap();

    // Packet 2: 64x64 frame (4x4 MBs = 16 MBs) split into 2 slices:
    // Slice A: MBs 0..7 (first_mb = 0)
    // Slice B: MBs 8..15 (first_mb = 8)
    let sps2 = build_sps_rbsp(1, 4, 4);
    let pps2 = build_pps_rbsp(1, 1);
    let slc2a = build_slice_rbsp(1, 0, 1);
    let slc2b = build_slice_rbsp(1, 8, 1);

    let mut pkt2 = Vec::new();
    pkt2.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x67]);
    pkt2.extend_from_slice(&sps2);
    pkt2.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x68]);
    pkt2.extend_from_slice(&pps2);
    pkt2.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x65]);
    pkt2.extend_from_slice(&slc2a);
    pkt2.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x65]);
    pkt2.extend_from_slice(&slc2b);

    let res = catch_unwind(AssertUnwindSafe(|| {
        decoder.decode_packet(&pkt2, 33)
    }));
    assert!(res.is_ok(), "Multi-slice with dimension change must not panic");
    let frame2 = res.unwrap().unwrap().unwrap();
    assert_eq!(frame2.width, 64);
    assert_eq!(frame2.height, 64);
}

#[test]
fn test_rapid_resolution_oscillation() {
    let mut decoder = H264Decoder::new();

    let resolutions = [
        (1, 1),   // 16x16
        (4, 4),   // 64x64
        (2, 3),   // 32x48
        (10, 6),  // 160x96
        (1, 1),   // 16x16
        (8, 2),   // 128x32
        (3, 5),   // 48x80
        (1, 1),   // 16x16
    ];

    for (i, &(wm, hm)) in resolutions.iter().enumerate() {
        let sps = build_sps_rbsp(i as u32, wm, hm);
        let pps = build_pps_rbsp(i as u32, i as u32);
        let first_mb = (i as u32 * 3) % (wm * hm);
        let slc = build_slice_rbsp(i as u32, first_mb, i as u64);
        let pkt = make_packet(&sps, &pps, &slc);

        let res = catch_unwind(AssertUnwindSafe(|| {
            decoder.decode_packet(&pkt, i as u64 * 33)
        }));
        assert!(res.is_ok(), "Oscillation step {i} failed");
        let frame = res.unwrap().unwrap().unwrap();
        assert_eq!(frame.width, wm * 16);
        assert_eq!(frame.height, hm * 16);
    }
}

// ============================================================================
// 2. ARITHMETIC OVERFLOWS IN SPS SCALING LISTS AND VISIBLE DIMENSIONS
// ============================================================================

#[test]
fn test_sps_scaling_lists_extreme_delta_scales() {
    let test_values = [
        i32::MAX,
        i32::MIN + 1,
        -1,
        -256,
        256,
        -257,
        257,
        i32::MAX - 1,
        -2147483640,
    ];

    for &delta in &test_values {
        let mut b = BitstreamBuilder::new();
        b.write_bits(100, 8); // High Profile
        b.write_bits(0, 8);
        b.write_bits(31, 8);
        b.write_ue(0);
        b.write_ue(1); // 4:2:0
        b.write_ue(0);
        b.write_ue(0);
        b.write_bit(false);
        b.write_bit(true); // seq_scaling_matrix_present_flag
        // List 0 present
        b.write_bit(true);
        let mut ls = 8i64;
        let mut ns = 8i64;
        for _ in 0..16 {
            if ns != 0 {
                b.write_se(delta);
                ns = (ls + delta as i64).rem_euclid(256);
            }
            ls = if ns == 0 { ls } else { ns };
        }
        for _ in 1..8 {
            b.write_bit(false);
        }
        b.write_ue(0);
        b.write_ue(0);
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

        let res = catch_unwind(AssertUnwindSafe(|| Sps::parse(&rbsp)));
        let parse_res = res.unwrap();
        assert!(parse_res.is_ok(), "Sps::parse returned Err({:?}) on delta = {delta}", parse_res.unwrap_err());
    }
}

#[test]
fn test_sps_parse_bounds_and_8k_guard() {
    // 1. pic_width_in_mbs_minus1 == 1024 MUST be rejected by Sps::parse
    let mut b1 = BitstreamBuilder::new();
    b1.write_bits(66, 8);
    b1.write_bits(0, 8);
    b1.write_bits(31, 8);
    b1.write_ue(0);
    b1.write_ue(0);
    b1.write_ue(0);
    b1.write_ue(0);
    b1.write_ue(1);
    b1.write_bit(false);
    b1.write_ue(1024); // >= 1024
    b1.write_ue(10);
    b1.write_bit(true);
    b1.write_bit(true);
    b1.write_bit(false);
    b1.write_bit(false);
    let rbsp1 = b1.build_rbsp();
    let res1 = Sps::parse(&rbsp1);
    assert!(
        matches!(res1, Err(CodecError::InvalidSps(ref msg)) if msg.contains("less than 1024")),
        "Expected InvalidSps, got {:?}",
        res1
    );

    // 2. pic_width_in_mbs_minus1 == 1023 (16384 pixels) parses in Sps::parse,
    // but decode_packet MUST reject it as exceeding 8K limit (8192x4320).
    let sps_16k = build_sps_rbsp(0, 1024, 10);
    let pps_16k = build_pps_rbsp(0, 0);
    let slc_16k = build_slice_rbsp(0, 0, 0);
    let pkt_16k = make_packet(&sps_16k, &pps_16k, &slc_16k);

    let mut decoder = H264Decoder::new();
    let res_decode = decoder.decode_packet(&pkt_16k, 0);
    assert!(
        matches!(res_decode, Err(CodecError::InvalidSps(ref msg)) if msg.contains("exceed maximum supported 8K resolution")),
        "Expected 8K guard rejection, got {:?}",
        res_decode
    );
}

// ============================================================================
// 3. SCALING LISTS IN PPS::PARSE
// ============================================================================

#[test]
fn test_pps_scaling_lists_extreme_delta_scales() {
    let test_values = [
        i32::MAX,
        i32::MIN + 1,
        -1,
        256,
        -256,
        i32::MAX - 1,
        -2147483640,
    ];

    for &delta in &test_values {
        let mut b = BitstreamBuilder::new();
        b.write_ue(0); // pic_parameter_set_id
        b.write_ue(0); // seq_parameter_set_id
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

        // Extensions
        b.write_bit(true); // transform_8x8_mode_flag
        b.write_bit(true); // pic_scaling_matrix_present_flag
        // 8 lists total (6 of size 16, 2 of size 64)
        for i in 0..8 {
            b.write_bit(true); // list present
            let size = if i < 6 { 16 } else { 64 };
            let mut ls = 8i64;
            let mut ns = 8i64;
            for _ in 0..size {
                if ns != 0 {
                    b.write_se(delta);
                    ns = (ls + delta as i64).rem_euclid(256);
                }
                ls = if ns == 0 { ls } else { ns };
            }
        }
        b.write_se(0);
        let rbsp = b.build_rbsp();

        let res = catch_unwind(AssertUnwindSafe(|| Pps::parse(&rbsp)));
        assert!(res.is_ok(), "Pps::parse panicked on delta = {delta}");
        assert!(res.unwrap().is_ok());
    }
}

#[test]
fn test_first_mb_exceeding_total_macroblocks_without_panic() {
    let mut decoder = H264Decoder::new();

    // Frame 1: 32x32 (4 macroblocks total: 0..3)
    let sps1 = build_sps_rbsp(0, 2, 2);
    let pps1 = build_pps_rbsp(0, 0);
    let slc1 = build_slice_rbsp(0, 0, 0);
    let pkt1 = make_packet(&sps1, &pps1, &slc1);
    let _ = decoder.decode_packet(&pkt1, 0).unwrap().unwrap();

    // Frame 2: 32x32, but first_mb_in_slice = 100 (> 4 total macroblocks)
    // All macroblocks 0..3 are preceding macroblocks! Must copy from previous frame safely.
    let slc2 = build_slice_rbsp(0, 100, 1);
    let pkt2 = make_packet(&sps1, &pps1, &slc2);

    let res = catch_unwind(AssertUnwindSafe(|| {
        decoder.decode_packet(&pkt2, 33)
    }));
    assert!(res.is_ok(), "first_mb > total_mbs panicked");
    let frame2 = res.unwrap().unwrap().unwrap();
    assert_eq!(frame2.width, 32);
    assert_eq!(frame2.height, 32);

    // Frame 3: Dimension change to 16x16 with first_mb_in_slice = 9999
    // Must fill neutral luma safely without accessing out-of-bounds previous frame.
    let sps3 = build_sps_rbsp(1, 1, 1);
    let pps3 = build_pps_rbsp(1, 1);
    let slc3 = build_slice_rbsp(1, 9999, 2);
    let pkt3 = make_packet(&sps3, &pps3, &slc3);

    let res3 = catch_unwind(AssertUnwindSafe(|| {
        decoder.decode_packet(&pkt3, 66)
    }));
    assert!(res3.is_ok(), "first_mb > total_mbs with dimension change panicked");
    let frame3 = res3.unwrap().unwrap().unwrap();
    assert_eq!(frame3.width, 16);
    assert_eq!(frame3.height, 16);
}

#[test]
fn test_sps_visible_dimensions_with_u32_max_and_overflowing_crops() {
    let mut sps = Sps {
        profile_idc: 66,
        constraint_set_flags: 0,
        level_idc: 31,
        seq_parameter_set_id: 0,
        chroma_format_idc: 1, // 4:2:0
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
        pic_width_in_mbs_minus1: u32::MAX,
        pic_height_in_map_units_minus1: u32::MAX,
        frame_mbs_only_flag: false, // sub_height_factor = 2
        mb_adaptive_frame_field_flag: false,
        direct_8x8_inference_flag: true,
        frame_cropping_flag: true,
        frame_crop_left_offset: u32::MAX,
        frame_crop_right_offset: u32::MAX,
        frame_crop_top_offset: u32::MAX,
        frame_crop_bottom_offset: u32::MAX,
        vui_parameters_present_flag: false,
    };

    let res = catch_unwind(AssertUnwindSafe(|| sps.visible_dimensions()));
    assert!(res.is_ok(), "visible_dimensions() panicked with u32::MAX inputs");
    let (w, h) = res.unwrap();
    assert_eq!(w, 0, "Extreme crop must saturate visible width to 0");
    assert_eq!(h, 0, "Extreme crop must saturate visible height to 0");

    // Without cropping: clamped macroblocks
    sps.frame_cropping_flag = false;
    let (w_uncropped, h_uncropped) = sps.visible_dimensions();
    assert_eq!(w_uncropped, 16384);
    assert_eq!(h_uncropped, 32768);
}

#[test]
fn test_sps_scaling_lists_chroma_444_with_12_lists() {
    let mut b = BitstreamBuilder::new();
    b.write_bits(244, 8); // High 4:4:4 Profile
    b.write_bits(0, 8);
    b.write_bits(31, 8);
    b.write_ue(0);
    b.write_ue(3); // Chroma format 4:4:4 -> 12 scaling lists!
    b.write_bit(false); // separate_colour_plane_flag
    b.write_ue(0);
    b.write_ue(0);
    b.write_bit(false);
    b.write_bit(true); // seq_scaling_matrix_present_flag = true

    // 12 scaling lists total: lists 0..5 are 16 entries, lists 6..11 are 64 entries
    for i in 0..12 {
        b.write_bit(true); // list present
        let size = if i < 6 { 16 } else { 64 };
        let mut ls = 8i64;
        let mut ns = 8i64;
        for _ in 0..size {
            if ns != 0 {
                b.write_se(i32::MAX);
                ns = (ls + i32::MAX as i64).rem_euclid(256);
            }
            ls = if ns == 0 { ls } else { ns };
        }
    }

    b.write_ue(0);
    b.write_ue(0);
    b.write_ue(0);
    b.write_ue(1);
    b.write_bit(false);
    b.write_ue(1);
    b.write_ue(1);
    b.write_bit(true);
    b.write_bit(true);
    b.write_bit(false);
    b.write_bit(false);
    let rbsp = b.build_rbsp();

    let res = catch_unwind(AssertUnwindSafe(|| Sps::parse(&rbsp)));
    assert!(res.is_ok(), "Sps::parse 4:4:4 panicked");
    assert!(res.unwrap().is_ok());
}

