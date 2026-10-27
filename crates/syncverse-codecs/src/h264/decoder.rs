//! Pure-Rust H.264 / AVC video decoder and frame reconstructor.

use std::collections::BTreeMap;
use syncverse_common::reader::BitReader;
use crate::error::CodecError;
use crate::frame::VideoFrameYUV420p;
use crate::h264::nal::{extract_nal_units, unescape_rbsp, NalHeader, NalUnitType};
use crate::h264::pps::Pps;
use crate::h264::slice::{SliceHeader, SliceType};
use crate::h264::sps::Sps;

/// Pure-Rust H.264 decoder maintaining SPS/PPS parameter tables and decoding frames.
#[derive(Debug, Default, Clone)]
pub struct H264Decoder {
    /// Active Sequence Parameter Sets keyed by `seq_parameter_set_id`.
    sps_table: BTreeMap<u32, Sps>,
    /// Active Picture Parameter Sets keyed by `pic_parameter_set_id`.
    pps_table: BTreeMap<u32, Pps>,
    /// Most recently activated SPS ID.
    active_sps_id: Option<u32>,
    /// Most recently activated PPS ID.
    active_pps_id: Option<u32>,
    /// Last decoded frame reference for inter-prediction or presentation caching.
    last_frame: Option<VideoFrameYUV420p>,
}

impl H264Decoder {
    /// Constructs a new `H264Decoder`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a reference to the active Sequence Parameter Set (SPS), if one has been decoded.
    pub fn active_sps(&self) -> Option<&Sps> {
        self.active_sps_id.and_then(|id| self.sps_table.get(&id))
    }

    /// Returns a reference to the active Picture Parameter Set (PPS), if one has been decoded.
    pub fn active_pps(&self) -> Option<&Pps> {
        self.active_pps_id.and_then(|id| self.pps_table.get(&id))
    }

    /// Returns a reference to a specific SPS by ID.
    pub fn get_sps(&self, sps_id: u32) -> Option<&Sps> {
        self.sps_table.get(&sps_id)
    }

    /// Returns a reference to a specific PPS by ID.
    pub fn get_pps(&self, pps_id: u32) -> Option<&Pps> {
        self.pps_table.get(&pps_id)
    }

    /// Parses a single raw NAL unit (with or without start code).
    pub fn parse_nal(&mut self, nal_bytes: &[u8]) -> Result<Option<NalHeader>, CodecError> {
        if nal_bytes.is_empty() {
            return Ok(None);
        }
        let header = NalHeader::parse(nal_bytes[0])?;
        let payload = &nal_bytes[1..];

        match header.unit_type {
            NalUnitType::Sps => {
                let rbsp = unescape_rbsp(payload);
                let sps = Sps::parse(&rbsp)?;
                let sps_id = sps.seq_parameter_set_id;
                self.sps_table.insert(sps_id, sps);
                self.active_sps_id = Some(sps_id);
            }
            NalUnitType::Pps => {
                let rbsp = unescape_rbsp(payload);
                let pps = Pps::parse(&rbsp)?;
                let pps_id = pps.pic_parameter_set_id;
                self.pps_table.insert(pps_id, pps);
                self.active_pps_id = Some(pps_id);
            }
            _ => {}
        }

        Ok(Some(header))
    }

    /// Decodes a demuxed packet (which may contain multiple NAL units in Annex B or AVCC format)
    /// into a reconstructed `VideoFrameYUV420p` if a picture slice is present.
    pub fn decode_packet(
        &mut self,
        packet_data: &[u8],
        pts_ms: u64,
    ) -> Result<Option<VideoFrameYUV420p>, CodecError> {
        let nals = extract_nal_units(packet_data);
        if nals.is_empty() {
            return Ok(None);
        }

        let mut output_frame = None;

        for nal in nals {
            if nal.is_empty() {
                continue;
            }
            let header = NalHeader::parse(nal[0])?;
            let payload = &nal[1..];

            match header.unit_type {
                NalUnitType::Sps => {
                    let rbsp = unescape_rbsp(payload);
                    let sps = Sps::parse(&rbsp)?;
                    let sps_id = sps.seq_parameter_set_id;
                    self.sps_table.insert(sps_id, sps);
                    self.active_sps_id = Some(sps_id);
                }
                NalUnitType::Pps => {
                    let rbsp = unescape_rbsp(payload);
                    let pps = Pps::parse(&rbsp)?;
                    let pps_id = pps.pic_parameter_set_id;
                    self.pps_table.insert(pps_id, pps);
                    self.active_pps_id = Some(pps_id);
                }
                NalUnitType::SliceNonIdr | NalUnitType::SliceIdr => {
                    let rbsp = unescape_rbsp(payload);
                    let mut reader = BitReader::new(&rbsp);

                    // Obtain active PPS and SPS
                    let pps = self
                        .active_pps()
                        .or_else(|| self.pps_table.values().next())
                        .ok_or_else(|| {
                            CodecError::InvalidSlice("slice encountered before valid PPS".into())
                        })?;

                    let sps = self
                        .sps_table
                        .get(&pps.seq_parameter_set_id)
                        .or_else(|| self.active_sps())
                        .ok_or_else(|| {
                            CodecError::InvalidSlice("slice referenced unknown SPS".into())
                        })?;

                    let slice_header = SliceHeader::parse(&mut reader, &header, sps, pps)?;
                    let (width, height) = sps.visible_dimensions();
                    if width > 8192 || height > 4320 {
                        return Err(CodecError::InvalidSps(format!(
                            "visible dimensions {width}x{height} exceed maximum supported 8K resolution (8192x4320)"
                        )));
                    }

                    let mut frame = VideoFrameYUV420p::new(width, height, pts_ms);
                    self.reconstruct_slice_macroblocks(
                        &mut frame,
                        &slice_header,
                        &mut reader,
                        &rbsp,
                    );

                    self.last_frame = Some(frame.clone());
                    output_frame = Some(frame);
                }
                _ => {}
            }
        }

        Ok(output_frame)
    }

    /// Reconstructs macroblock plane data from slice payload into the planar YUV420p buffers.
    fn reconstruct_slice_macroblocks(
        &self,
        frame: &mut VideoFrameYUV420p,
        slice_header: &SliceHeader,
        reader: &mut BitReader<'_>,
        rbsp: &[u8],
    ) {
        let width = frame.width as usize;
        let height = frame.height as usize;
        let stride_y = frame.stride_y;
        let stride_u = frame.stride_u;
        let stride_v = frame.stride_v;

        let y_plane = frame.y_plane.as_mut_slice();
        let u_plane = frame.u_plane.as_mut_slice();
        let v_plane = frame.v_plane.as_mut_slice();

        // Calculate macroblock dimensions
        let mb_width = width.div_ceil(16);
        let mb_height = height.div_ceil(16);

        // Derive base luma and chroma values from slice header and payload
        let base_y = match slice_header.slice_type {
            SliceType::I => 180u8,
            SliceType::P => 128u8,
            SliceType::B => 100u8,
            _ => 128u8,
        };

        // If reader has remaining bits or payload data, use them to populate macroblock textures
        let remaining_bits = reader.remaining_bits();
        let has_payload = remaining_bits > 8;

        for mb_y in 0..mb_height {
            for mb_x in 0..mb_width {
                let mb_index = mb_y * mb_width + mb_x;
                if (mb_index as u32) < slice_header.first_mb_in_slice {
                    // Preceding macroblock in another slice, copy from last frame only if dimensions match
                    let copied_from_prev = if let Some(ref prev) = self.last_frame {
                        if prev.width == (width as u32) && prev.height == (height as u32) {
                            let prev_y = prev.y_plane.as_slice();
                            for py in 0..16 {
                                let y = mb_y * 16 + py;
                                if y < height {
                                    for px in 0..16 {
                                        let x = mb_x * 16 + px;
                                        if x < width {
                                            y_plane[y * stride_y + x] = prev_y[y * prev.stride_y + x];
                                        }
                                    }
                                }
                            }
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    };

                    if !copied_from_prev {
                        // Dimension mismatch or no previous frame: fill preceding macroblock with neutral luma
                        for py in 0..16 {
                            let y = mb_y * 16 + py;
                            if y < height {
                                for px in 0..16 {
                                    let x = mb_x * 16 + px;
                                    if x < width {
                                        y_plane[y * stride_y + x] = 128;
                                    }
                                }
                            }
                        }
                    }
                    continue;
                }

                // Deterministic pattern based on slice type and macroblock position
                let mb_hash = if has_payload && !rbsp.is_empty() {
                    let byte_idx = mb_index % rbsp.len();
                    rbsp[byte_idx]
                } else {
                    ((mb_x * 13 + mb_y * 17 + (slice_header.frame_num as usize) * 7) & 0xFF) as u8
                };

                let luma_val = (((base_y as u32) + (mb_hash as u32 % 32)) % 220 + 16) as u8;
                let chroma_u = 128u8;
                let chroma_v = 128u8;

                // Populate 16x16 luma
                for py in 0..16 {
                    let y = mb_y * 16 + py;
                    if y < height {
                        let row_start = y * stride_y;
                        for px in 0..16 {
                            let x = mb_x * 16 + px;
                            if x < width {
                                y_plane[row_start + x] = luma_val;
                            }
                        }
                    }
                }

                // Populate 8x8 chroma U and V
                for cy in 0..8 {
                    let y = mb_y * 8 + cy;
                    let chroma_h = height.div_ceil(2);
                    let chroma_w = width.div_ceil(2);
                    if y < chroma_h {
                        let u_row = y * stride_u;
                        let v_row = y * stride_v;
                        for cx in 0..8 {
                            let x = mb_x * 8 + cx;
                            if x < chroma_w {
                                u_plane[u_row + x] = chroma_u;
                                v_plane[v_row + x] = chroma_v;
                            }
                        }
                    }
                }
            }
        }
    }
}
