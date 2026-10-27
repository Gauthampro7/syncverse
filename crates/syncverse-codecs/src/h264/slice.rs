//! H.264 slice header parsing per ITU-T H.264 Section 7.3.3.

use syncverse_common::reader::BitReader;
use crate::error::CodecError;
use crate::h264::nal::{NalHeader, NalUnitType};
use crate::h264::pps::Pps;
use crate::h264::sps::Sps;

/// H.264 Slice Type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliceType {
    /// P slice (predicted).
    P = 0,
    /// B slice (bi-directionally predicted).
    B = 1,
    /// I slice (intra-coded).
    I = 2,
    /// SP slice (switching P).
    Sp = 3,
    /// SI slice (switching I).
    Si = 4,
}

impl SliceType {
    /// Returns true if the slice type is intra-coded (I or SI).
    #[inline]
    pub fn is_intra(&self) -> bool {
        matches!(self, Self::I | Self::Si)
    }
}

/// Parsed H.264 Slice Header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SliceHeader {
    /// Address of first macroblock in slice.
    pub first_mb_in_slice: u32,
    /// Slice type (P, B, I, SP, SI).
    pub slice_type: SliceType,
    /// Picture parameter set ID.
    pub pic_parameter_set_id: u32,
    /// Colour plane ID (when separate_colour_plane_flag == true).
    pub colour_plane_id: Option<u8>,
    /// Frame number.
    pub frame_num: u64,
    /// Field picture flag.
    pub field_pic_flag: bool,
    /// Bottom field flag.
    pub bottom_field_flag: bool,
    /// IDR picture ID (when slice is IDR).
    pub idr_pic_id: Option<u32>,
    /// Picture order count LSB (when pic_order_cnt_type == 0).
    pub pic_order_cnt_lsb: Option<u32>,
    /// Delta pic order count bottom (when pic_order_cnt_type == 0).
    pub delta_pic_order_cnt_bottom: Option<i32>,
    /// Delta pic order count (when pic_order_cnt_type == 1).
    pub delta_pic_order_cnt: [i32; 2],
    /// Redundant picture count.
    pub redundant_pic_cnt: u32,
    /// Direct spatial motion vector prediction flag (for B slices).
    pub direct_spatial_mv_pred_flag: bool,
    /// Slice QP delta.
    pub slice_qp_delta: i32,
}

impl SliceHeader {
    /// Parses a slice header from an RBSP reader given the active NAL header, SPS, and PPS.
    pub fn parse(
        reader: &mut BitReader<'_>,
        nal_header: &NalHeader,
        sps: &Sps,
        pps: &Pps,
    ) -> Result<Self, CodecError> {
        let first_mb_in_slice = reader.read_ue()?;
        let raw_slice_type = reader.read_ue()?;
        let slice_type = match raw_slice_type % 5 {
            0 => SliceType::P,
            1 => SliceType::B,
            2 => SliceType::I,
            3 => SliceType::Sp,
            4 => SliceType::Si,
            _ => unreachable!(),
        };

        let pic_parameter_set_id = reader.read_ue()?;

        let mut colour_plane_id = None;
        if sps.separate_colour_plane_flag {
            colour_plane_id = Some(reader.read_bits(2)? as u8);
        }

        let frame_num_bits = sps.log2_max_frame_num_minus4 as usize + 4;
        let frame_num = reader.read_bits(frame_num_bits)?;

        let mut field_pic_flag = false;
        let mut bottom_field_flag = false;
        if !sps.frame_mbs_only_flag {
            field_pic_flag = reader.read_bit()?;
            if field_pic_flag {
                bottom_field_flag = reader.read_bit()?;
            }
        }

        let mut idr_pic_id = None;
        if nal_header.unit_type == NalUnitType::SliceIdr {
            idr_pic_id = Some(reader.read_ue()?);
        }

        let mut pic_order_cnt_lsb = None;
        let mut delta_pic_order_cnt_bottom = None;
        let mut delta_pic_order_cnt = [0i32; 2];

        if sps.pic_order_cnt_type == 0 {
            let poc_bits = sps.log2_max_pic_order_cnt_lsb_minus4 as usize + 4;
            pic_order_cnt_lsb = Some(reader.read_bits(poc_bits)? as u32);
            if pps.bottom_field_pic_order_in_frame_present_flag && !field_pic_flag {
                delta_pic_order_cnt_bottom = Some(reader.read_se()?);
            }
        } else if sps.pic_order_cnt_type == 1 && !sps.delta_pic_order_always_zero_flag {
            delta_pic_order_cnt[0] = reader.read_se()?;
            if pps.bottom_field_pic_order_in_frame_present_flag && !field_pic_flag {
                delta_pic_order_cnt[1] = reader.read_se()?;
            }
        }

        let mut redundant_pic_cnt = 0;
        if pps.redundant_pic_cnt_present_flag {
            redundant_pic_cnt = reader.read_ue()?;
        }

        let mut direct_spatial_mv_pred_flag = false;
        if slice_type == SliceType::B {
            direct_spatial_mv_pred_flag = reader.read_bit()?;
        }

        if slice_type == SliceType::P
            || slice_type == SliceType::B
            || slice_type == SliceType::Sp
        {
            let num_ref_idx_active_override_flag = reader.read_bit()?;
            if num_ref_idx_active_override_flag {
                let _num_ref_idx_l0_active = reader.read_ue()?;
                if slice_type == SliceType::B {
                    let _num_ref_idx_l1_active = reader.read_ue()?;
                }
            }
        }

        // ref_pic_list_modification
        if slice_type != SliceType::I && slice_type != SliceType::Si {
            let ref_pic_list_modification_flag_l0 = reader.read_bit()?;
            if ref_pic_list_modification_flag_l0 {
                loop {
                    let mod_idc = reader.read_ue()?;
                    if mod_idc == 3 {
                        break;
                    }
                    let _ = reader.read_ue()?;
                }
            }
            if slice_type == SliceType::B {
                let ref_pic_list_modification_flag_l1 = reader.read_bit()?;
                if ref_pic_list_modification_flag_l1 {
                    loop {
                        let mod_idc = reader.read_ue()?;
                        if mod_idc == 3 {
                            break;
                        }
                        let _ = reader.read_ue()?;
                    }
                }
            }
        }

        // dec_ref_pic_marking
        if nal_header.unit_type == NalUnitType::SliceIdr {
            let _no_output_of_prior_pics_flag = reader.read_bit()?;
            let _long_term_reference_flag = reader.read_bit()?;
        } else {
            let adaptive_ref_pic_marking_mode_flag = reader.read_bit()?;
            if adaptive_ref_pic_marking_mode_flag {
                loop {
                    let mmco = reader.read_ue()?;
                    if mmco == 0 {
                        break;
                    }
                    match mmco {
                        1 | 3 => {
                            let _ = reader.read_ue()?;
                        }
                        2 => {
                            let _ = reader.read_ue()?;
                        }
                        4 => {
                            let _ = reader.read_ue()?;
                        }
                        5 => {}
                        6 => {
                            let _ = reader.read_ue()?;
                        }
                        _ => break,
                    }
                }
            }
        }

        if pps.entropy_coding_mode_flag && slice_type != SliceType::I && slice_type != SliceType::Si {
            let _cabac_init_idc = reader.read_ue()?;
        }

        let slice_qp_delta = reader.read_se()?;

        Ok(Self {
            first_mb_in_slice,
            slice_type,
            pic_parameter_set_id,
            colour_plane_id,
            frame_num,
            field_pic_flag,
            bottom_field_flag,
            idr_pic_id,
            pic_order_cnt_lsb,
            delta_pic_order_cnt_bottom,
            delta_pic_order_cnt,
            redundant_pic_cnt,
            direct_spatial_mv_pred_flag,
            slice_qp_delta,
        })
    }
}
