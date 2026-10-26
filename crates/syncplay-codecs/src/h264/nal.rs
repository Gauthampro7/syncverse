//! H.264 / AVC NAL unit parsing, start code scanning, and RBSP un-escaping.

use crate::error::CodecError;

/// H.264 NAL Unit Type as specified in ITU-T H.264 Table 7-1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NalUnitType {
    /// Unspecified NAL unit.
    Unspecified,
    /// Coded slice of a non-IDR picture.
    SliceNonIdr,
    /// Coded slice data partition A.
    SliceDataPartitionA,
    /// Coded slice data partition B.
    SliceDataPartitionB,
    /// Coded slice data partition C.
    SliceDataPartitionC,
    /// Coded slice of an IDR picture.
    SliceIdr,
    /// Supplemental enhancement information (SEI).
    Sei,
    /// Sequence parameter set (SPS).
    Sps,
    /// Picture parameter set (PPS).
    Pps,
    /// Access unit delimiter (AUD).
    Aud,
    /// End of sequence.
    EndOfSequence,
    /// End of stream.
    EndOfStream,
    /// Filler data.
    FillerData,
    /// Sequence parameter set extension.
    SpsExtension,
    /// Prefix NAL unit.
    PrefixNalUnit,
    /// Subset sequence parameter set.
    SubsetSps,
    /// Reserved or other NAL unit type.
    Other(u8),
}

impl NalUnitType {
    /// Returns the raw 5-bit NAL unit type ID.
    pub fn type_id(&self) -> u8 {
        match self {
            Self::Unspecified => 0,
            Self::SliceNonIdr => 1,
            Self::SliceDataPartitionA => 2,
            Self::SliceDataPartitionB => 3,
            Self::SliceDataPartitionC => 4,
            Self::SliceIdr => 5,
            Self::Sei => 6,
            Self::Sps => 7,
            Self::Pps => 8,
            Self::Aud => 9,
            Self::EndOfSequence => 10,
            Self::EndOfStream => 11,
            Self::FillerData => 12,
            Self::SpsExtension => 13,
            Self::PrefixNalUnit => 14,
            Self::SubsetSps => 15,
            Self::Other(id) => *id,
        }
    }
}

impl From<u8> for NalUnitType {
    fn from(val: u8) -> Self {
        match val & 0x1F {
            0 => Self::Unspecified,
            1 => Self::SliceNonIdr,
            2 => Self::SliceDataPartitionA,
            3 => Self::SliceDataPartitionB,
            4 => Self::SliceDataPartitionC,
            5 => Self::SliceIdr,
            6 => Self::Sei,
            7 => Self::Sps,
            8 => Self::Pps,
            9 => Self::Aud,
            10 => Self::EndOfSequence,
            11 => Self::EndOfStream,
            12 => Self::FillerData,
            13 => Self::SpsExtension,
            14 => Self::PrefixNalUnit,
            15 => Self::SubsetSps,
            other => Self::Other(other),
        }
    }
}

/// NAL unit header (1 byte).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NalHeader {
    /// Forbidden zero bit (bit 7). MUST be false in valid streams.
    pub forbidden_zero_bit: bool,
    /// Reference indicator (bits 5-6). 0 = non-reference, >0 = reference.
    pub nal_ref_idc: u8,
    /// NAL unit type (bits 0-4).
    pub unit_type: NalUnitType,
}

impl NalHeader {
    /// Parses a 1-byte NAL header.
    pub fn parse(byte: u8) -> Result<Self, CodecError> {
        let forbidden = (byte & 0x80) != 0;
        if forbidden {
            return Err(CodecError::InvalidNal(
                "forbidden_zero_bit must be 0".into(),
            ));
        }
        let ref_idc = (byte >> 5) & 0x03;
        let u_type = NalUnitType::from(byte & 0x1F);
        Ok(Self {
            forbidden_zero_bit: forbidden,
            nal_ref_idc: ref_idc,
            unit_type: u_type,
        })
    }
}

/// Strips H.264 emulation prevention bytes (`0x000003`) from a NAL unit payload,
/// yielding raw byte sequence payload (RBSP).
///
/// Per ITU-T H.264 Section 7.3.1:
/// When `0x000003` occurs in the payload, the `0x03` byte is discarded if followed by
/// `0x00`, `0x01`, `0x02`, or `0x03`.
pub fn unescape_rbsp(nal_payload: &[u8]) -> Vec<u8> {
    let mut rbsp = Vec::with_capacity(nal_payload.len());
    let mut i = 0;
    while i < nal_payload.len() {
        if i + 2 < nal_payload.len()
            && nal_payload[i] == 0x00
            && nal_payload[i + 1] == 0x00
            && nal_payload[i + 2] == 0x03
        {
            rbsp.push(0x00);
            rbsp.push(0x00);
            // Emulation prevention byte 0x03 is discarded
            i += 3;
        } else {
            rbsp.push(nal_payload[i]);
            i += 1;
        }
    }
    rbsp
}

/// Scanner for Annex B formatted byte streams delimited by `0x000001` or `0x00000001` start codes.
#[derive(Debug, Clone)]
pub struct AnnexBScanner<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> AnnexBScanner<'a> {
    /// Constructs a new Annex B scanner over the provided byte stream slice.
    pub const fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Finds the next start code prefix (3-byte `0x000001` or 4-byte `0x00000001`).
    /// Returns the (start_index, prefix_len) if found.
    fn find_start_code(&self, from: usize) -> Option<(usize, usize)> {
        let len = self.data.len();
        if from + 3 > len {
            return None;
        }
        let mut i = from;
        while i + 2 < len {
            if self.data[i] == 0x00 && self.data[i + 1] == 0x00 {
                if self.data[i + 2] == 0x01 {
                    // Check if there was an additional leading zero
                    if i > from && self.data[i - 1] == 0x00 {
                        return Some((i - 1, 4));
                    }
                    return Some((i, 3));
                } else if i + 3 < len && self.data[i + 2] == 0x00 && self.data[i + 3] == 0x01 {
                    return Some((i, 4));
                }
            }
            i += 1;
        }
        None
    }

    /// Scans and returns the next NAL unit slice (without start code prefix).
    pub fn next_nal(&mut self) -> Option<&'a [u8]> {
        if self.pos >= self.data.len() {
            return None;
        }

        // Find initial start code
        let (start_idx, prefix_len) = self.find_start_code(self.pos)?;
        let nal_start = start_idx + prefix_len;

        // Find next start code to determine NAL unit boundary
        if let Some((next_start, _)) = self.find_start_code(nal_start) {
            let mut nal_end = next_start;
            // Trim any trailing zeros preceding the next start code
            while nal_end > nal_start && self.data[nal_end - 1] == 0x00 {
                nal_end -= 1;
            }
            self.pos = next_start;
            if nal_start <= nal_end {
                Some(&self.data[nal_start..nal_end])
            } else {
                self.next_nal()
            }
        } else {
            // Reached last NAL in the buffer
            let mut nal_end = self.data.len();
            while nal_end > nal_start && self.data[nal_end - 1] == 0x00 {
                nal_end -= 1;
            }
            self.pos = self.data.len();
            if nal_start <= nal_end {
                Some(&self.data[nal_start..nal_end])
            } else {
                None
            }
        }
    }
}

/// Parses an Annex B stream into a list of NAL unit slices.
pub fn parse_annex_b(data: &[u8]) -> Vec<&[u8]> {
    let mut scanner = AnnexBScanner::new(data);
    let mut nals = Vec::new();
    while let Some(nal) = scanner.next_nal() {
        if !nal.is_empty() {
            nals.push(nal);
        }
    }
    nals
}

/// Parses AVCC length-prefixed NAL units (typically 4-byte length prefix).
pub fn parse_avcc(data: &[u8], length_size: usize) -> Result<Vec<&[u8]>, CodecError> {
    if length_size != 1 && length_size != 2 && length_size != 4 {
        return Err(CodecError::InvalidNal(format!(
            "unsupported AVCC length size: {}",
            length_size
        )));
    }

    let mut nals = Vec::new();
    let mut offset = 0;

    while offset + length_size <= data.len() {
        let nal_len = match length_size {
            1 => data[offset] as usize,
            2 => u16::from_be_bytes([data[offset], data[offset + 1]]) as usize,
            4 => u32::from_be_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]) as usize,
            _ => unreachable!(),
        };
        offset += length_size;

        if offset + nal_len > data.len() {
            return Err(CodecError::InvalidNal(format!(
                "AVCC NAL length {} exceeds remaining packet bytes {}",
                nal_len,
                data.len() - offset
            )));
        }

        if nal_len > 0 {
            nals.push(&data[offset..offset + nal_len]);
        }
        offset += nal_len;
    }

    Ok(nals)
}

/// Automatically extracts NAL units from a packet, detecting Annex B or AVCC format.
pub fn extract_nal_units(data: &[u8]) -> Vec<&[u8]> {
    if data.len() >= 3 && (data.starts_with(&[0, 0, 1]) || data.starts_with(&[0, 0, 0, 1])) {
        return parse_annex_b(data);
    }
    // Try AVCC 4-byte prefix
    if data.len() >= 4 {
        if let Ok(nals) = parse_avcc(data, 4) {
            if !nals.is_empty() {
                return nals;
            }
        }
    }
    // Fall back to Annex B scan
    parse_annex_b(data)
}
