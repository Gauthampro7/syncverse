//! H.264 Picture Parameter Set (PPS) decoding.

use syncverse_common::reader::BitReader;
use crate::error::CodecError;

/// Decoded H.264 Picture Parameter Set (PPS) per ITU-T H.264 Section 7.3.2.2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pps {
    /// Picture parameter set ID (0..255).
    pub pic_parameter_set_id: u32,
    /// Reference sequence parameter set ID (0..31).
    pub seq_parameter_set_id: u32,
    /// Entropy coding mode flag: false = CAVLC, true = CABAC.
    pub entropy_coding_mode_flag: bool,
    /// Bottom field pic order in frame present flag.
    pub bottom_field_pic_order_in_frame_present_flag: bool,
    /// Number of slice groups minus 1.
    pub num_slice_groups_minus1: u32,
    /// Number of reference index list 0 default active minus 1.
    pub num_ref_idx_l0_default_active_minus1: u32,
    /// Number of reference index list 1 default active minus 1.
    pub num_ref_idx_l1_default_active_minus1: u32,
    /// Weighted prediction flag.
    pub weighted_pred_flag: bool,
    /// Weighted biprediction indicator (0 = default, 1 = explicit, 2 = implicit).
    pub weighted_bipred_idc: u8,
    /// Initial slice QP minus 26.
    pub pic_init_qp_minus26: i32,
    /// Initial slice QS minus 26.
    pub pic_init_qs_minus26: i32,
    /// Chroma QP index offset.
    pub chroma_qp_index_offset: i32,
    /// Deblocking filter control present flag.
    pub deblocking_filter_control_present_flag: bool,
    /// Constrained intra prediction flag.
    pub constrained_intra_pred_flag: bool,
    /// Redundant pic cnt present flag.
    pub redundant_pic_cnt_present_flag: bool,
    /// Transform 8x8 mode flag (if more RBSP data exists).
    pub transform_8x8_mode_flag: bool,
    /// Pic scaling matrix present flag (if more RBSP data exists).
    pub pic_scaling_matrix_present_flag: bool,
    /// Second chroma QP index offset (if more RBSP data exists).
    pub second_chroma_qp_index_offset: i32,
}

impl Pps {
    /// Parses a PPS from an RBSP byte slice.
    pub fn parse(rbsp: &[u8]) -> Result<Self, CodecError> {
        let mut reader = BitReader::new(rbsp);

        let pic_parameter_set_id = reader.read_ue()?;
        let seq_parameter_set_id = reader.read_ue()?;
        let entropy_coding_mode_flag = reader.read_bit()?;
        let bottom_field_pic_order_in_frame_present_flag = reader.read_bit()?;
        let num_slice_groups_minus1 = reader.read_ue()?;

        if num_slice_groups_minus1 > 0 {
            let slice_group_map_type = reader.read_ue()?;
            match slice_group_map_type {
                0 => {
                    for _ in 0..=num_slice_groups_minus1 {
                        let _run_length = reader.read_ue()?;
                    }
                }
                2 => {
                    for _ in 0..num_slice_groups_minus1 {
                        let _top_left = reader.read_ue()?;
                        let _bottom_right = reader.read_ue()?;
                    }
                }
                3..=5 => {
                    let _change_dir = reader.read_bit()?;
                    let _change_rate = reader.read_ue()?;
                }
                6 => {
                    let pic_size_in_map_units = reader.read_ue()?;
                    let bits = ((num_slice_groups_minus1 + 1) as f32).log2().ceil() as usize;
                    for _ in 0..=pic_size_in_map_units {
                        let _group_id = reader.read_bits(bits)?;
                    }
                }
                _ => {}
            }
        }

        let num_ref_idx_l0_default_active_minus1 = reader.read_ue()?;
        let num_ref_idx_l1_default_active_minus1 = reader.read_ue()?;
        let weighted_pred_flag = reader.read_bit()?;
        let weighted_bipred_idc = reader.read_bits(2)? as u8;
        let pic_init_qp_minus26 = reader.read_se()?;
        let pic_init_qs_minus26 = reader.read_se()?;
        let chroma_qp_index_offset = reader.read_se()?;
        let deblocking_filter_control_present_flag = reader.read_bit()?;
        let constrained_intra_pred_flag = reader.read_bit()?;
        let redundant_pic_cnt_present_flag = reader.read_bit()?;

        let mut transform_8x8_mode_flag = false;
        let mut pic_scaling_matrix_present_flag = false;
        let mut second_chroma_qp_index_offset = chroma_qp_index_offset;

        // Check if more RBSP data exists (extensions)
        if reader.has_more_rbsp_data() {
            transform_8x8_mode_flag = reader.read_bit()?;
            pic_scaling_matrix_present_flag = reader.read_bit()?;
            if pic_scaling_matrix_present_flag {
                let num_lists = 6 + (if transform_8x8_mode_flag { 2 } else { 0 });
                for i in 0..num_lists {
                    let pic_scaling_list_present_flag = reader.read_bit()?;
                    if pic_scaling_list_present_flag {
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
            if reader.has_more_rbsp_data() {
                second_chroma_qp_index_offset = reader.read_se()?;
            }
        }

        Ok(Self {
            pic_parameter_set_id,
            seq_parameter_set_id,
            entropy_coding_mode_flag,
            bottom_field_pic_order_in_frame_present_flag,
            num_slice_groups_minus1,
            num_ref_idx_l0_default_active_minus1,
            num_ref_idx_l1_default_active_minus1,
            weighted_pred_flag,
            weighted_bipred_idc,
            pic_init_qp_minus26,
            pic_init_qs_minus26,
            chroma_qp_index_offset,
            deblocking_filter_control_present_flag,
            constrained_intra_pred_flag,
            redundant_pic_cnt_present_flag,
            transform_8x8_mode_flag,
            pic_scaling_matrix_present_flag,
            second_chroma_qp_index_offset,
        })
    }
}
