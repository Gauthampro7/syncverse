//! H.264 Sequence Parameter Set (SPS) decoding and picture dimension calculation.

use syncplay_common::reader::BitReader;
use crate::error::CodecError;

/// Decoded H.264 Sequence Parameter Set (SPS) per ITU-T H.264 Section 7.3.2.1.1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sps {
    /// Profile indicator (e.g. 66 = Baseline, 77 = Main, 100 = High).
    pub profile_idc: u8,
    /// Constraint set flags.
    pub constraint_set_flags: u8,
    /// Level indicator (e.g. 31 = Level 3.1).
    pub level_idc: u8,
    /// Sequence parameter set ID (0..31).
    pub seq_parameter_set_id: u32,
    /// Chroma format indicator (0 = monochrome, 1 = 4:2:0, 2 = 4:2:2, 3 = 4:4:4).
    pub chroma_format_idc: u32,
    /// Separate colour plane flag (when chroma_format_idc == 3).
    pub separate_colour_plane_flag: bool,
    /// Bit depth luma minus 8.
    pub bit_depth_luma_minus8: u32,
    /// Bit depth chroma minus 8.
    pub bit_depth_chroma_minus8: u32,
    /// QpPrime Y zero transform bypass flag.
    pub qpprime_y_zero_transform_bypass_flag: bool,
    /// Sequence scaling matrix present flag.
    pub seq_scaling_matrix_present_flag: bool,
    /// Log2 max frame num minus 4.
    pub log2_max_frame_num_minus4: u32,
    /// Picture order count type (0, 1, or 2).
    pub pic_order_cnt_type: u32,
    /// Log2 max pic order count lsb minus 4 (when pic_order_cnt_type == 0).
    pub log2_max_pic_order_cnt_lsb_minus4: u32,
    /// Delta pic order always zero flag (when pic_order_cnt_type == 1).
    pub delta_pic_order_always_zero_flag: bool,
    /// Offset for non-ref pic (when pic_order_cnt_type == 1).
    pub offset_for_non_ref_pic: i32,
    /// Offset for top to bottom field (when pic_order_cnt_type == 1).
    pub offset_for_top_to_bottom_field: i32,
    /// Number of reference frames in pic order cnt cycle.
    pub num_ref_frames_in_pic_order_cnt_cycle: u32,
    /// Offsets for ref frame in cycle.
    pub offset_for_ref_frame: Vec<i32>,
    /// Maximum number of reference frames.
    pub max_num_ref_frames: u32,
    /// Gaps in frame num value allowed flag.
    pub gaps_in_frame_num_value_allowed_flag: bool,
    /// Picture width in macroblocks minus 1.
    pub pic_width_in_mbs_minus1: u32,
    /// Picture height in slice map units minus 1.
    pub pic_height_in_map_units_minus1: u32,
    /// Frame macroblocks only flag (true for progressive frames).
    pub frame_mbs_only_flag: bool,
    /// Macroblock adaptive frame/field flag.
    pub mb_adaptive_frame_field_flag: bool,
    /// Direct 8x8 inference flag.
    pub direct_8x8_inference_flag: bool,
    /// Frame cropping flag.
    pub frame_cropping_flag: bool,
    /// Left crop offset in crop units.
    pub frame_crop_left_offset: u32,
    /// Right crop offset in crop units.
    pub frame_crop_right_offset: u32,
    /// Top crop offset in crop units.
    pub frame_crop_top_offset: u32,
    /// Bottom crop offset in crop units.
    pub frame_crop_bottom_offset: u32,
    /// VUI parameters present flag.
    pub vui_parameters_present_flag: bool,
}

impl Sps {
    /// Parses an SPS from an RBSP byte slice.
    pub fn parse(rbsp: &[u8]) -> Result<Self, CodecError> {
        let mut reader = BitReader::new(rbsp);

        let profile_idc = reader.read_bits(8)? as u8;
        let constraint_set_flags = reader.read_bits(8)? as u8;
        let level_idc = reader.read_bits(8)? as u8;
        let seq_parameter_set_id = reader.read_ue()?;

        let mut chroma_format_idc = 1; // Default 4:2:0
        let mut separate_colour_plane_flag = false;
        let mut bit_depth_luma_minus8 = 0;
        let mut bit_depth_chroma_minus8 = 0;
        let mut qpprime_y_zero_transform_bypass_flag = false;
        let mut seq_scaling_matrix_present_flag = false;

        // High profile and professional extensions
        if matches!(
            profile_idc,
            100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135
        ) {
            chroma_format_idc = reader.read_ue()?;
            if chroma_format_idc == 3 {
                separate_colour_plane_flag = reader.read_bit()?;
            }
            bit_depth_luma_minus8 = reader.read_ue()?;
            bit_depth_chroma_minus8 = reader.read_ue()?;
            qpprime_y_zero_transform_bypass_flag = reader.read_bit()?;
            seq_scaling_matrix_present_flag = reader.read_bit()?;

            if seq_scaling_matrix_present_flag {
                let num_lists = if chroma_format_idc != 3 { 8 } else { 12 };
                for i in 0..num_lists {
                    let seq_scaling_list_present_flag = reader.read_bit()?;
                    if seq_scaling_list_present_flag {
                        let size = if i < 6 { 16 } else { 64 };
                        let mut last_scale = 8i32;
                        let mut next_scale = 8i32;
                        for _ in 0..size {
                            if next_scale != 0 {
                                let delta_scale = reader.read_se()?;
                                next_scale = ((last_scale as i64 + delta_scale as i64).rem_euclid(256)) as i32;
                            }
                            last_scale = if next_scale == 0 { last_scale } else { next_scale };
                        }
                    }
                }
            }
        }

        let log2_max_frame_num_minus4 = reader.read_ue()?;
        let pic_order_cnt_type = reader.read_ue()?;

        let mut log2_max_pic_order_cnt_lsb_minus4 = 0;
        let mut delta_pic_order_always_zero_flag = false;
        let mut offset_for_non_ref_pic = 0;
        let mut offset_for_top_to_bottom_field = 0;
        let mut num_ref_frames_in_pic_order_cnt_cycle = 0;
        let mut offset_for_ref_frame = Vec::new();

        if pic_order_cnt_type == 0 {
            log2_max_pic_order_cnt_lsb_minus4 = reader.read_ue()?;
        } else if pic_order_cnt_type == 1 {
            delta_pic_order_always_zero_flag = reader.read_bit()?;
            offset_for_non_ref_pic = reader.read_se()?;
            offset_for_top_to_bottom_field = reader.read_se()?;
            num_ref_frames_in_pic_order_cnt_cycle = reader.read_ue()?;
            for _ in 0..num_ref_frames_in_pic_order_cnt_cycle {
                offset_for_ref_frame.push(reader.read_se()?);
            }
        }

        let max_num_ref_frames = reader.read_ue()?;
        let gaps_in_frame_num_value_allowed_flag = reader.read_bit()?;
        let pic_width_in_mbs_minus1 = reader.read_ue()?;
        let pic_height_in_map_units_minus1 = reader.read_ue()?;

        if pic_width_in_mbs_minus1 >= 1024 || pic_height_in_map_units_minus1 >= 1024 {
            return Err(CodecError::InvalidSps(
                "pic_width_in_mbs_minus1 and pic_height_in_map_units_minus1 must be less than 1024".into(),
            ));
        }
        let frame_mbs_only_flag = reader.read_bit()?;

        let mut mb_adaptive_frame_field_flag = false;
        if !frame_mbs_only_flag {
            mb_adaptive_frame_field_flag = reader.read_bit()?;
        }

        let direct_8x8_inference_flag = reader.read_bit()?;
        let frame_cropping_flag = reader.read_bit()?;

        let mut frame_crop_left_offset = 0;
        let mut frame_crop_right_offset = 0;
        let mut frame_crop_top_offset = 0;
        let mut frame_crop_bottom_offset = 0;

        if frame_cropping_flag {
            frame_crop_left_offset = reader.read_ue()?;
            frame_crop_right_offset = reader.read_ue()?;
            frame_crop_top_offset = reader.read_ue()?;
            frame_crop_bottom_offset = reader.read_ue()?;
        }

        let vui_parameters_present_flag = reader.read_bit()?;

        Ok(Self {
            profile_idc,
            constraint_set_flags,
            level_idc,
            seq_parameter_set_id,
            chroma_format_idc,
            separate_colour_plane_flag,
            bit_depth_luma_minus8,
            bit_depth_chroma_minus8,
            qpprime_y_zero_transform_bypass_flag,
            seq_scaling_matrix_present_flag,
            log2_max_frame_num_minus4,
            pic_order_cnt_type,
            log2_max_pic_order_cnt_lsb_minus4,
            delta_pic_order_always_zero_flag,
            offset_for_non_ref_pic,
            offset_for_top_to_bottom_field,
            num_ref_frames_in_pic_order_cnt_cycle,
            offset_for_ref_frame,
            max_num_ref_frames,
            gaps_in_frame_num_value_allowed_flag,
            pic_width_in_mbs_minus1,
            pic_height_in_map_units_minus1,
            frame_mbs_only_flag,
            mb_adaptive_frame_field_flag,
            direct_8x8_inference_flag,
            frame_cropping_flag,
            frame_crop_left_offset,
            frame_crop_right_offset,
            frame_crop_top_offset,
            frame_crop_bottom_offset,
            vui_parameters_present_flag,
        })
    }

    /// Computes the visible picture width and height in pixels after taking
    /// macroblock geometry and frame cropping offsets into account.
    pub fn visible_dimensions(&self) -> (u32, u32) {
        let width_mbs_minus1 = (self.pic_width_in_mbs_minus1 as u64).min(1023);
        let height_mbs_minus1 = (self.pic_height_in_map_units_minus1 as u64).min(1023);
        let sub_height_factor = if self.frame_mbs_only_flag { 1u64 } else { 2u64 };

        let raw_width = (width_mbs_minus1.saturating_add(1)).saturating_mul(16);
        let raw_height = (height_mbs_minus1.saturating_add(1))
            .saturating_mul(16)
            .saturating_mul(sub_height_factor);

        if !self.frame_cropping_flag {
            return (raw_width as u32, raw_height as u32);
        }

        let crop_unit_x: u64 = match self.chroma_format_idc {
            1 | 2 => 2,
            3 => 1,
            _ => 1,
        };
        let crop_unit_y: u64 = sub_height_factor
            * match self.chroma_format_idc {
                1 => 2,
                2 => 1,
                3 => 1,
                _ => 1,
            };

        let crop_x = (self.frame_crop_left_offset as u64)
            .saturating_add(self.frame_crop_right_offset as u64)
            .saturating_mul(crop_unit_x);
        let crop_y = (self.frame_crop_top_offset as u64)
            .saturating_add(self.frame_crop_bottom_offset as u64)
            .saturating_mul(crop_unit_y);

        let visible_width = raw_width.saturating_sub(crop_x);
        let visible_height = raw_height.saturating_sub(crop_y);

        (visible_width as u32, visible_height as u32)
    }
}
