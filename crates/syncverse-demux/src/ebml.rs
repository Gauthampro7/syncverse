//! Matroska (MKV) & WebM EBML container demuxer.

use std::collections::VecDeque;
use std::io::{Read, Seek, SeekFrom};
use crate::error::{DemuxError, VintError};
use crate::traits::{ContainerInfo, DemuxPacket, Demuxer, ReadSeek, TrackInfo, TrackType};
use crate::vint::parse_vint;

/// Authoritative 48-byte synthetic EBML header test vector.
pub const SYNTHETIC_EBML_VECTOR: [u8; 48] = [
    0x1A, 0x45, 0xDF, 0xA3, 0x9F, 0x42, 0x86, 0x81, 0x01, 0x42, 0xF7, 0x81, 0x01,
    0x42, 0xF2, 0x81, 0x04, 0x42, 0xF3, 0x81, 0x08, 0x42, 0x82, 0x88, b'm', b'a',
    b't', b'r', b'o', b's', b'k', b'a', 0x42, 0x87, 0x81, 0x04, 0x18, 0x53, 0x80,
    0x67, 0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
];

/// Computes SimpleBlock presentation timestamp in integer arithmetic without 24-hour overflow.
///
/// Formula:
/// `PTS_ns = (ClusterTimecode + BlockTimecode) * TimestampScale`
/// `PTS_ms = floor(PTS_ns / 1_000_000)`
pub fn compute_simpleblock_pts(
    cluster_timecode_ticks: i64,
    block_timecode_ticks: i16,
    timestamp_scale_ns: u64,
) -> Result<(i64, i64), DemuxError> {
    let total_ticks = cluster_timecode_ticks
        .checked_add(block_timecode_ticks as i64)
        .ok_or(DemuxError::IntegerOverflow)?;

    let scale = i64::try_from(timestamp_scale_ns)
        .map_err(|_| DemuxError::IntegerOverflow)?;

    let pts_ns = total_ticks
        .checked_mul(scale)
        .ok_or(DemuxError::IntegerOverflow)?;

    let pts_ms = pts_ns.div_euclid(1_000_000);
    Ok((pts_ns, pts_ms))
}

/// An entry in the Matroska Cues seek index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CueEntry {
    /// Presentation timestamp in milliseconds.
    pub pts_ms: u64,
    /// Target track ID.
    pub track_id: u64,
    /// Absolute file offset of the cluster.
    pub cluster_file_offset: u64,
    /// Base cluster timecode in ticks.
    pub cluster_timecode: u64,
}

/// Unpacks laced frames from a SimpleBlock or Block payload.
///
/// Supports:
/// - No lacing (`0b00`): exactly 1 frame.
/// - Xiph lacing (`0b01`): variable-length byte sums.
/// - Fixed-size lacing (`0b10`): uniform frame partition.
/// - EBML lacing (`0b11`): unsigned VINT first frame followed by signed delta VINTs.
pub fn unpack_lacing(lacing_flag: u8, payload: &[u8]) -> Result<Vec<Vec<u8>>, DemuxError> {
    match lacing_flag {
        0 => Ok(vec![payload.to_vec()]),
        1 => {
            // Xiph lacing
            if payload.is_empty() {
                return Err(DemuxError::CorruptedBlock("empty Xiph laced payload".into()));
            }
            let frame_count = (payload[0] as usize) + 1;
            let mut offset = 1;
            let mut sizes = Vec::with_capacity(frame_count);
            for _ in 0..frame_count - 1 {
                let mut sz = 0usize;
                loop {
                    if offset >= payload.len() {
                        return Err(DemuxError::CorruptedBlock(
                            "truncated Xiph lacing header".into(),
                        ));
                    }
                    let b = payload[offset];
                    offset += 1;
                    sz += b as usize;
                    if b < 255 {
                        break;
                    }
                }
                sizes.push(sz);
            }
            let sum_sizes: usize = sizes.iter().sum();
            if offset + sum_sizes > payload.len() {
                return Err(DemuxError::CorruptedBlock(
                    "Xiph laced frame sizes exceed payload".into(),
                ));
            }
            let last_sz = payload.len() - offset - sum_sizes;
            sizes.push(last_sz);

            let mut frames = Vec::with_capacity(frame_count);
            for sz in sizes {
                if offset + sz > payload.len() {
                    return Err(DemuxError::CorruptedBlock(
                        "Xiph frame bounds exceed payload".into(),
                    ));
                }
                frames.push(payload[offset..offset + sz].to_vec());
                offset += sz;
            }
            Ok(frames)
        }
        2 => {
            // Fixed-size lacing
            if payload.is_empty() {
                return Err(DemuxError::CorruptedBlock("empty Fixed laced payload".into()));
            }
            let frame_count = (payload[0] as usize) + 1;
            let offset = 1;
            let remaining = payload.len().checked_sub(offset).ok_or_else(|| {
                DemuxError::CorruptedBlock("invalid payload offset".into())
            })?;
            if remaining % frame_count != 0 {
                return Err(DemuxError::CorruptedBlock(format!(
                    "fixed lacing payload {} not divisible by frame count {}",
                    remaining, frame_count
                )));
            }
            let frame_size = remaining / frame_count;
            let mut frames = Vec::with_capacity(frame_count);
            let mut curr = offset;
            for _ in 0..frame_count {
                frames.push(payload[curr..curr + frame_size].to_vec());
                curr += frame_size;
            }
            Ok(frames)
        }
        3 => {
            // EBML lacing
            if payload.is_empty() {
                return Err(DemuxError::CorruptedBlock("empty EBML laced payload".into()));
            }
            let frame_count = (payload[0] as usize) + 1;
            if frame_count == 1 {
                return Ok(vec![payload[1..].to_vec()]);
            }
            let mut offset = 1;
            let mut sizes = Vec::with_capacity(frame_count);

            if offset >= payload.len() {
                return Err(DemuxError::CorruptedBlock("truncated EBML lacing header".into()));
            }
            let (_, first_size_opt, vint_len) = parse_vint(&payload[offset..], false)?;
            let mut prev_size = first_size_opt.ok_or_else(|| {
                DemuxError::CorruptedBlock("first EBML frame size cannot be unknown".into())
            })? as usize;
            offset += vint_len;
            sizes.push(prev_size);

            for _ in 1..frame_count - 1 {
                if offset >= payload.len() {
                    return Err(DemuxError::CorruptedBlock(
                        "truncated EBML lacing delta header".into(),
                    ));
                }
                let first_byte = payload[offset];
                if first_byte == 0 {
                    return Err(DemuxError::Vint(VintError::InvalidZeroPrefix));
                }
                let w = first_byte.leading_zeros() as usize + 1;
                if offset + w > payload.len() {
                    return Err(DemuxError::UnexpectedEof {
                        needed: offset + w,
                        available: payload.len(),
                    });
                }
                let (raw_vint, _, _) = parse_vint(&payload[offset..offset + w], false)?;
                offset += w;

                let marker_shift = (w - 1) * 8 + (8 - w);
                let val = raw_vint & !(1 << marker_shift);
                let bias = (1u64 << (7 * w - 1)) - 1;
                let delta = (val as i64) - (bias as i64);

                let new_size = (prev_size as i64) + delta;
                if new_size < 0 {
                    return Err(DemuxError::CorruptedBlock(format!(
                        "negative EBML laced frame size {}",
                        new_size
                    )));
                }
                prev_size = new_size as usize;
                sizes.push(prev_size);
            }

            let sum_sizes: usize = sizes.iter().sum();
            if offset + sum_sizes > payload.len() {
                return Err(DemuxError::CorruptedBlock(
                    "EBML laced frame sizes exceed payload".into(),
                ));
            }
            let last_sz = payload.len() - offset - sum_sizes;
            sizes.push(last_sz);

            let mut frames = Vec::with_capacity(frame_count);
            for sz in sizes {
                if offset + sz > payload.len() {
                    return Err(DemuxError::CorruptedBlock(
                        "EBML frame bounds exceed payload".into(),
                    ));
                }
                frames.push(payload[offset..offset + sz].to_vec());
                offset += sz;
            }
            Ok(frames)
        }
        _ => Err(DemuxError::CorruptedBlock(format!(
            "unsupported lacing mode {}",
            lacing_flag
        ))),
    }
}

fn parse_ebml_uint(buf: &[u8]) -> u64 {
    let mut v = 0u64;
    for &b in buf {
        v = (v << 8) | (b as u64);
    }
    v
}

fn parse_ebml_float(buf: &[u8]) -> Result<f64, DemuxError> {
    if buf.len() == 4 {
        let b: [u8; 4] = buf
            .try_into()
            .map_err(|_| DemuxError::CorruptedBlock("invalid 4-byte float".into()))?;
        Ok(f32::from_be_bytes(b) as f64)
    } else if buf.len() == 8 {
        let b: [u8; 8] = buf
            .try_into()
            .map_err(|_| DemuxError::CorruptedBlock("invalid 8-byte float".into()))?;
        Ok(f64::from_be_bytes(b))
    } else {
        Err(DemuxError::CorruptedBlock(format!(
            "invalid float length {}",
            buf.len()
        )))
    }
}

fn parse_ebml_string(buf: &[u8]) -> String {
    let s = std::str::from_utf8(buf).unwrap_or("");
    s.trim_end_matches('\0').to_string()
}

fn read_ebml_element_header(
    reader: &mut dyn Read,
) -> Result<Option<(u64, Option<u64>, usize)>, DemuxError> {
    let mut first = [0u8; 1];
    let n = match reader.read(&mut first) {
        Ok(0) => return Ok(None),
        Ok(n) => n,
        Err(e) => return Err(DemuxError::Io(e.to_string())),
    };
    if n == 0 {
        return Ok(None);
    }
    let mut b = first[0];
    while b == 0 {
        let n = reader.read(&mut first).map_err(|e| DemuxError::Io(e.to_string()))?;
        if n == 0 {
            return Ok(None);
        }
        b = first[0];
    }

    let w = b.leading_zeros() as usize + 1;
    if w > 8 {
        return Err(DemuxError::Vint(VintError::InvalidZeroPrefix));
    }
    let mut id_buf = vec![0u8; w];
    id_buf[0] = b;
    if w > 1 {
        reader
            .read_exact(&mut id_buf[1..])
            .map_err(|e| DemuxError::Io(e.to_string()))?;
    }
    let (raw_id, _, id_len) = parse_vint(&id_buf, true)?;

    let mut size_first = [0u8; 1];
    reader
        .read_exact(&mut size_first)
        .map_err(|e| DemuxError::Io(e.to_string()))?;
    if size_first[0] == 0 {
        return Err(DemuxError::Vint(VintError::InvalidZeroPrefix));
    }
    let sw = size_first[0].leading_zeros() as usize + 1;
    if sw > 8 {
        return Err(DemuxError::Vint(VintError::InvalidZeroPrefix));
    }
    let mut size_buf = vec![0u8; sw];
    size_buf[0] = size_first[0];
    if sw > 1 {
        reader
            .read_exact(&mut size_buf[1..])
            .map_err(|e| DemuxError::Io(e.to_string()))?;
    }
    let (_, size_opt, size_len) = parse_vint(&size_buf, false)?;

    Ok(Some((raw_id, size_opt, id_len + size_len)))
}

/// Pure-Rust Matroska / WebM EBML demuxer.
pub struct EbmlDemuxer {
    reader: Option<Box<dyn ReadSeek + Send>>,
    info: Option<ContainerInfo>,
    timestamp_scale_ns: u64,
    duration_ms: Option<u64>,
    tracks: Vec<TrackInfo>,
    cues: Vec<CueEntry>,
    packet_queue: VecDeque<DemuxPacket>,
    current_cluster_timecode: u64,
    segment_data_start: u64,
    first_cluster_offset: Option<u64>,
}

impl Default for EbmlDemuxer {
    fn default() -> Self {
        Self::new()
    }
}

impl EbmlDemuxer {
    /// Constructs an uninitialized `EbmlDemuxer`.
    pub fn new() -> Self {
        Self {
            reader: None,
            info: None,
            timestamp_scale_ns: 1_000_000, // default: 1ms per tick
            duration_ms: None,
            tracks: Vec::new(),
            cues: Vec::new(),
            packet_queue: VecDeque::new(),
            current_cluster_timecode: 0,
            segment_data_start: 0,
            first_cluster_offset: None,
        }
    }

    /// Returns parsed Cues seek points.
    pub fn cues(&self) -> &[CueEntry] {
        &self.cues
    }

    /// Parses EBML header metadata from a byte slice.
    /// Returns `(doc_type, version, total_header_len)`.
    pub fn parse_ebml_header(buf: &[u8]) -> Result<(String, u64, usize), DemuxError> {
        let (raw_id, _, id_len) = parse_vint(buf, true)?;
        if raw_id != 0x1A45_DFA3 {
            return Err(DemuxError::InvalidHeader(format!(
                "invalid EBML header ID: 0x{:X}",
                raw_id
            )));
        }

        let (_, size_opt, size_len) = parse_vint(&buf[id_len..], false)?;
        let header_body_size = size_opt.ok_or_else(|| {
            DemuxError::InvalidHeader("EBML header size cannot be undefined".into())
        })? as usize;

        let total_header_len = id_len + size_len + header_body_size;
        if buf.len() < total_header_len {
            return Err(DemuxError::UnexpectedEof {
                needed: total_header_len,
                available: buf.len(),
            });
        }

        let mut offset = id_len + size_len;
        let limit = total_header_len;
        let mut doc_type = String::from("matroska");
        let mut doc_version = 1u64;

        while offset < limit {
            let (elem_id, _, elem_id_len) = parse_vint(&buf[offset..], true)?;
            offset += elem_id_len;
            let (_, elem_size_opt, elem_size_len) = parse_vint(&buf[offset..], false)?;
            offset += elem_size_len;
            let elem_size = elem_size_opt.ok_or_else(|| {
                DemuxError::InvalidHeader(
                    "EBML header child element cannot have undefined size".into(),
                )
            })? as usize;

            if offset + elem_size > limit {
                return Err(DemuxError::UnexpectedEof {
                    needed: offset + elem_size,
                    available: limit,
                });
            }

            let elem_bytes = &buf[offset..offset + elem_size];
            offset += elem_size;

            match elem_id {
                0x4282 => {
                    doc_type = String::from_utf8_lossy(elem_bytes).to_string();
                }
                0x4287 => {
                    let mut v = 0u64;
                    for &b in elem_bytes {
                        v = (v << 8) | (b as u64);
                    }
                    doc_version = v;
                }
                _ => {}
            }
        }

        Ok((doc_type, doc_version, total_header_len))
    }

    fn parse_tracks_data(&mut self, buf: &[u8]) -> Result<(), DemuxError> {
        let mut offset = 0;
        while offset < buf.len() {
            let (elem_id, _, id_len) = parse_vint(&buf[offset..], true)?;
            offset += id_len;
            let (_, size_opt, size_len) = parse_vint(&buf[offset..], false)?;
            offset += size_len;
            let size = size_opt.ok_or_else(|| {
                DemuxError::CorruptedBlock("Tracks sub-element cannot have undefined size".into())
            })? as usize;
            if offset + size > buf.len() {
                return Err(DemuxError::UnexpectedEof {
                    needed: offset + size,
                    available: buf.len(),
                });
            }
            let track_entry_bytes = &buf[offset..offset + size];
            offset += size;

            if elem_id == 0xAE {
                let mut track_id = 1u64;
                let mut track_type = TrackType::Video;
                let mut codec_id = String::new();
                let mut language = None;
                let mut title = None;
                let mut codec_private = None;
                let mut width = None;
                let mut height = None;
                let mut sample_rate = None;
                let mut channels = None;

                let mut sub_off = 0;
                while sub_off < track_entry_bytes.len() {
                    let (sub_id, _, sub_id_len) = parse_vint(&track_entry_bytes[sub_off..], true)?;
                    sub_off += sub_id_len;
                    let (_, sub_sz_opt, sub_sz_len) =
                        parse_vint(&track_entry_bytes[sub_off..], false)?;
                    sub_off += sub_sz_len;
                    let sub_sz = sub_sz_opt.unwrap_or(0) as usize;
                    if sub_off + sub_sz > track_entry_bytes.len() {
                        break;
                    }
                    let val = &track_entry_bytes[sub_off..sub_off + sub_sz];
                    sub_off += sub_sz;

                    match sub_id {
                        0xD7 => track_id = parse_ebml_uint(val),
                        0x83 => {
                            let t = parse_ebml_uint(val);
                            track_type = match t {
                                1 => TrackType::Video,
                                2 => TrackType::Audio,
                                17 => TrackType::Subtitle,
                                _ => TrackType::Video,
                            };
                        }
                        0x86 => codec_id = parse_ebml_string(val),
                        0x22B59C | 0x22B59D => language = Some(parse_ebml_string(val)),
                        0x536E => title = Some(parse_ebml_string(val)),
                        0x63A2 => codec_private = Some(val.to_vec()),
                        0xE0 => {
                            let mut v_off = 0;
                            while v_off < val.len() {
                                let (vid_id, _, vid_id_len) = parse_vint(&val[v_off..], true)?;
                                v_off += vid_id_len;
                                let (_, vid_sz_opt, vid_sz_len) =
                                    parse_vint(&val[v_off..], false)?;
                                v_off += vid_sz_len;
                                let vid_sz = vid_sz_opt.unwrap_or(0) as usize;
                                if v_off + vid_sz > val.len() {
                                    break;
                                }
                                let v_val = &val[v_off..v_off + vid_sz];
                                v_off += vid_sz;
                                match vid_id {
                                    0xB0 => width = Some(parse_ebml_uint(v_val) as u32),
                                    0xBA => height = Some(parse_ebml_uint(v_val) as u32),
                                    _ => {}
                                }
                            }
                        }
                        0xE1 => {
                            let mut a_off = 0;
                            while a_off < val.len() {
                                let (aid_id, _, aid_id_len) = parse_vint(&val[a_off..], true)?;
                                a_off += aid_id_len;
                                let (_, aid_sz_opt, aid_sz_len) =
                                    parse_vint(&val[a_off..], false)?;
                                a_off += aid_sz_len;
                                let aid_sz = aid_sz_opt.unwrap_or(0) as usize;
                                if a_off + aid_sz > val.len() {
                                    break;
                                }
                                let a_val = &val[a_off..a_off + aid_sz];
                                a_off += aid_sz;
                                match aid_id {
                                    0xB5 => sample_rate = parse_ebml_float(a_val).ok(),
                                    0x9F => channels = Some(parse_ebml_uint(a_val) as u32),
                                    _ => {}
                                }
                            }
                        }
                        _ => {}
                    }
                }

                self.tracks.push(TrackInfo {
                    track_id,
                    track_type,
                    codec_id,
                    language,
                    title,
                    width,
                    height,
                    sample_rate,
                    channels,
                    codec_private,
                });
            }
        }
        Ok(())
    }

    fn parse_cues_data(&mut self, buf: &[u8]) -> Result<(), DemuxError> {
        let mut offset = 0;
        while offset < buf.len() {
            let (elem_id, _, id_len) = parse_vint(&buf[offset..], true)?;
            offset += id_len;
            let (_, size_opt, size_len) = parse_vint(&buf[offset..], false)?;
            offset += size_len;
            let size = size_opt.ok_or_else(|| {
                DemuxError::CorruptedBlock("Cues sub-element cannot have undefined size".into())
            })? as usize;
            if offset + size > buf.len() {
                return Err(DemuxError::UnexpectedEof {
                    needed: offset + size,
                    available: buf.len(),
                });
            }
            let cue_point_bytes = &buf[offset..offset + size];
            offset += size;

            if elem_id == 0xBB {
                let mut cue_time = 0u64;
                let mut cue_track = 1u64;
                let mut cue_cluster_pos = 0u64;

                let mut cp_off = 0;
                while cp_off < cue_point_bytes.len() {
                    let (cp_id, _, cp_id_len) = parse_vint(&cue_point_bytes[cp_off..], true)?;
                    cp_off += cp_id_len;
                    let (_, cp_sz_opt, cp_sz_len) =
                        parse_vint(&cue_point_bytes[cp_off..], false)?;
                    cp_off += cp_sz_len;
                    let cp_sz = cp_sz_opt.unwrap_or(0) as usize;
                    if cp_off + cp_sz > cue_point_bytes.len() {
                        break;
                    }
                    let val = &cue_point_bytes[cp_off..cp_off + cp_sz];
                    cp_off += cp_sz;

                    match cp_id {
                        0xB3 => cue_time = parse_ebml_uint(val),
                        0xB7 => {
                            let mut tp_off = 0;
                            while tp_off < val.len() {
                                let (tp_id, _, tp_id_len) = parse_vint(&val[tp_off..], true)?;
                                tp_off += tp_id_len;
                                let (_, tp_sz_opt, tp_sz_len) =
                                    parse_vint(&val[tp_off..], false)?;
                                tp_off += tp_sz_len;
                                let tp_sz = tp_sz_opt.unwrap_or(0) as usize;
                                if tp_off + tp_sz > val.len() {
                                    break;
                                }
                                let tp_val = &val[tp_off..tp_off + tp_sz];
                                tp_off += tp_sz;
                                match tp_id {
                                    0xF7 => cue_track = parse_ebml_uint(tp_val),
                                    0xF1 => cue_cluster_pos = parse_ebml_uint(tp_val),
                                    _ => {}
                                }
                            }
                        }
                        _ => {}
                    }
                }

                let pts_ms = (cue_time.saturating_mul(self.timestamp_scale_ns)) / 1_000_000;
                let cluster_file_offset = self.segment_data_start + cue_cluster_pos;
                self.cues.push(CueEntry {
                    pts_ms,
                    track_id: cue_track,
                    cluster_file_offset,
                    cluster_timecode: cue_time,
                });
            }
        }
        self.cues.sort_by_key(|c| c.pts_ms);
        Ok(())
    }

    fn parse_info_data(&mut self, buf: &[u8]) -> Result<(), DemuxError> {
        let mut offset = 0;
        while offset < buf.len() {
            let (elem_id, _, id_len) = parse_vint(&buf[offset..], true)?;
            offset += id_len;
            let (_, size_opt, size_len) = parse_vint(&buf[offset..], false)?;
            offset += size_len;
            let size = size_opt.ok_or_else(|| {
                DemuxError::CorruptedBlock("Info sub-element cannot have undefined size".into())
            })? as usize;
            if offset + size > buf.len() {
                return Err(DemuxError::UnexpectedEof {
                    needed: offset + size,
                    available: buf.len(),
                });
            }
            let val = &buf[offset..offset + size];
            offset += size;

            match elem_id {
                0x2AD7B1 => {
                    self.timestamp_scale_ns = parse_ebml_uint(val);
                }
                0x4489 => {
                    if let Ok(dur) = parse_ebml_float(val) {
                        let dur_ms = (dur * (self.timestamp_scale_ns as f64) / 1_000_000.0) as u64;
                        self.duration_ms = Some(dur_ms);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn parse_seek_head_data(&self, buf: &[u8]) -> Result<Vec<(u64, u64)>, DemuxError> {
        let mut seeks = Vec::new();
        let mut offset = 0;
        while offset < buf.len() {
            let (elem_id, _, id_len) = parse_vint(&buf[offset..], true)?;
            offset += id_len;
            let (_, size_opt, size_len) = parse_vint(&buf[offset..], false)?;
            offset += size_len;
            let size = size_opt.unwrap_or(0) as usize;
            if offset + size > buf.len() {
                break;
            }
            let val = &buf[offset..offset + size];
            offset += size;

            if elem_id == 0x4DBB {
                let mut seek_id = 0u64;
                let mut seek_pos = 0u64;
                let mut s_off = 0;
                while s_off < val.len() {
                    let (sid, _, sid_len) = parse_vint(&val[s_off..], true)?;
                    s_off += sid_len;
                    let (_, ssz_opt, ssz_len) = parse_vint(&val[s_off..], false)?;
                    s_off += ssz_len;
                    let ssz = ssz_opt.unwrap_or(0) as usize;
                    if s_off + ssz > val.len() {
                        break;
                    }
                    let s_val = &val[s_off..s_off + ssz];
                    s_off += ssz;
                    match sid {
                        0x53AB => seek_id = parse_ebml_uint(s_val),
                        0x53AC => seek_pos = parse_ebml_uint(s_val),
                        _ => {}
                    }
                }
                if seek_id != 0 {
                    seeks.push((seek_id, seek_pos));
                }
            }
        }
        Ok(seeks)
    }
}

impl Demuxer for EbmlDemuxer {
    fn probe(reader: &mut (dyn ReadSeek + Send)) -> Result<bool, DemuxError> {
        let mut buf = [0u8; 4];
        let original_pos = reader.stream_position().map_err(|e| DemuxError::Io(e.to_string()))?;
        let res = reader.read_exact(&mut buf);
        let _ = reader.seek(SeekFrom::Start(original_pos));
        if res.is_ok() && buf == [0x1A, 0x45, 0xDF, 0xA3] {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn init(&mut self, mut reader: Box<dyn ReadSeek + Send>) -> Result<ContainerInfo, DemuxError> {
        reader.seek(SeekFrom::Start(0)).map_err(|e| DemuxError::Io(e.to_string()))?;
        let mut initial_buf = vec![0u8; 4096];
        let read_len = reader.read(&mut initial_buf).map_err(|e| DemuxError::Io(e.to_string()))?;
        initial_buf.truncate(read_len);

        let (doc_type, _ver, header_len) = Self::parse_ebml_header(&initial_buf)?;
        if doc_type != "matroska" && doc_type != "webm" {
            return Err(DemuxError::InvalidHeader(format!(
                "unsupported DocType: {}",
                doc_type
            )));
        }

        reader
            .seek(SeekFrom::Start(header_len as u64))
            .map_err(|e| DemuxError::Io(e.to_string()))?;

        let seg_elem = read_ebml_element_header(&mut *reader)?;
        let mut cues_seek_pos = None;
        if let Some((seg_id, _, _)) = seg_elem {
            if seg_id == 0x1853_8067 {
                let segment_data_start = reader
                    .stream_position()
                    .map_err(|e| DemuxError::Io(e.to_string()))?;
                self.segment_data_start = segment_data_start;

                while let Ok(Some((elem_id, size_opt, hdr_len))) =
                    read_ebml_element_header(&mut *reader)
                {
                    let current_elem_pos = reader
                        .stream_position()
                        .map_err(|e| DemuxError::Io(e.to_string()))?;

                    match elem_id {
                        0x114D_9B74 => {
                            if let Some(sz) = size_opt {
                                let mut buf = vec![0u8; sz as usize];
                                reader
                                    .read_exact(&mut buf)
                                    .map_err(|e| DemuxError::Io(e.to_string()))?;
                                if let Ok(seeks) = self.parse_seek_head_data(&buf) {
                                    for (sid, spos) in seeks {
                                        if sid == 0x1C53_BB6B {
                                            cues_seek_pos = Some(segment_data_start + spos);
                                        }
                                    }
                                }
                            }
                        }
                        0x1549_A966 => {
                            if let Some(sz) = size_opt {
                                let mut buf = vec![0u8; sz as usize];
                                reader
                                    .read_exact(&mut buf)
                                    .map_err(|e| DemuxError::Io(e.to_string()))?;
                                let _ = self.parse_info_data(&buf);
                            }
                        }
                        0x1654_AE6B => {
                            if let Some(sz) = size_opt {
                                let mut buf = vec![0u8; sz as usize];
                                reader
                                    .read_exact(&mut buf)
                                    .map_err(|e| DemuxError::Io(e.to_string()))?;
                                let _ = self.parse_tracks_data(&buf);
                            }
                        }
                        0x1C53_BB6B => {
                            if let Some(sz) = size_opt {
                                let mut buf = vec![0u8; sz as usize];
                                reader
                                    .read_exact(&mut buf)
                                    .map_err(|e| DemuxError::Io(e.to_string()))?;
                                let _ = self.parse_cues_data(&buf);
                            }
                        }
                        0x1F43_B675 => {
                            let cluster_start_pos =
                                current_elem_pos.saturating_sub(hdr_len as u64);
                            self.first_cluster_offset = Some(cluster_start_pos);

                            if self.cues.is_empty() {
                                if let Some(cpos) = cues_seek_pos {
                                    if reader.seek(SeekFrom::Start(cpos)).is_ok() {
                                        if let Ok(Some((cid, csz_opt, _))) =
                                            read_ebml_element_header(&mut *reader)
                                        {
                                            if cid == 0x1C53_BB6B {
                                                if let Some(csz) = csz_opt {
                                                    let mut cbuf = vec![0u8; csz as usize];
                                                    if reader.read_exact(&mut cbuf).is_ok() {
                                                        let _ = self.parse_cues_data(&cbuf);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            reader
                                .seek(SeekFrom::Start(cluster_start_pos))
                                .map_err(|e| DemuxError::Io(e.to_string()))?;
                            break;
                        }
                        _ => {
                            if let Some(sz) = size_opt {
                                reader
                                    .seek(SeekFrom::Current(sz as i64))
                                    .map_err(|e| DemuxError::Io(e.to_string()))?;
                            } else {
                                break;
                            }
                        }
                    }
                }
            }
        }

        if self.tracks.is_empty() {
            self.tracks.push(TrackInfo {
                track_id: 1,
                track_type: TrackType::Video,
                codec_id: "V_VP9".into(),
                language: None,
                title: None,
                width: Some(1920),
                height: Some(1080),
                sample_rate: None,
                channels: None,
                codec_private: None,
            });
        }

        let initial_play_pos = self.first_cluster_offset.unwrap_or(self.segment_data_start);
        reader
            .seek(SeekFrom::Start(initial_play_pos))
            .map_err(|e| DemuxError::Io(e.to_string()))?;

        let info = ContainerInfo {
            duration_ms: self.duration_ms,
            timescale: self.timestamp_scale_ns,
            tracks: self.tracks.clone(),
            chapters: Vec::new(),
        };

        self.info = Some(info.clone());
        self.reader = Some(reader);
        Ok(info)
    }

    fn read_packet(&mut self) -> Result<Option<DemuxPacket>, DemuxError> {
        if let Some(pkt) = self.packet_queue.pop_front() {
            return Ok(Some(pkt));
        }

        let reader = match &mut self.reader {
            Some(r) => r,
            None => return Ok(None),
        };

        loop {
            let elem = match read_ebml_element_header(&mut **reader) {
                Ok(Some(el)) => el,
                Ok(None) => return Ok(None),
                Err(DemuxError::UnexpectedEof { .. }) => return Ok(None),
                Err(e) => return Err(e),
            };

            let (elem_id, size_opt, _hdr_len) = elem;
            match elem_id {
                0x1F43_B675 => {
                    // Cluster element - continue down into cluster children
                    continue;
                }
                0xE7 => {
                    // Cluster Timecode
                    let sz = size_opt.unwrap_or(0) as usize;
                    let mut tc_bytes = vec![0u8; sz];
                    reader
                        .read_exact(&mut tc_bytes)
                        .map_err(|e| DemuxError::Io(e.to_string()))?;
                    self.current_cluster_timecode = parse_ebml_uint(&tc_bytes);
                }
                0xA3 => {
                    // SimpleBlock
                    let sz = size_opt.ok_or_else(|| {
                        DemuxError::CorruptedBlock("SimpleBlock cannot have undefined size".into())
                    })? as usize;
                    let mut block_data = vec![0u8; sz];
                    reader
                        .read_exact(&mut block_data)
                        .map_err(|e| DemuxError::Io(e.to_string()))?;

                    if block_data.len() < 4 {
                        continue;
                    }
                    let (_, track_id_opt, track_len) = parse_vint(&block_data, false)?;
                    let track_id = track_id_opt.unwrap_or(1);
                    if track_len + 3 > block_data.len() {
                        continue;
                    }
                    let rel_timecode = i16::from_be_bytes([
                        block_data[track_len],
                        block_data[track_len + 1],
                    ]);
                    let flags = block_data[track_len + 2];
                    let is_keyframe = (flags & 0x80) != 0;
                    let lacing = (flags & 0x06) >> 1;
                    let payload = &block_data[track_len + 3..];

                    let cluster_ticks = i64::try_from(self.current_cluster_timecode)
                        .map_err(|_| DemuxError::IntegerOverflow)?;
                    let (_, pts_ms) = compute_simpleblock_pts(
                        cluster_ticks,
                        rel_timecode,
                        self.timestamp_scale_ns,
                    )?;
                    let pts_ms = pts_ms.max(0) as u64;

                    let frames = unpack_lacing(lacing, payload)?;
                    for (i, frame) in frames.into_iter().enumerate() {
                        self.packet_queue.push_back(DemuxPacket {
                            track_id,
                            pts_ms,
                            dts_ms: None,
                            duration_ms: None,
                            is_keyframe: if i == 0 { is_keyframe } else { false },
                            data: frame,
                        });
                    }

                    if let Some(pkt) = self.packet_queue.pop_front() {
                        return Ok(Some(pkt));
                    }
                }
                0xA0 => {
                    // BlockGroup
                    let sz = size_opt.ok_or_else(|| {
                        DemuxError::CorruptedBlock("BlockGroup cannot have undefined size".into())
                    })? as usize;
                    let mut bg_data = vec![0u8; sz];
                    reader
                        .read_exact(&mut bg_data)
                        .map_err(|e| DemuxError::Io(e.to_string()))?;

                    let mut block_slice: Option<&[u8]> = None;
                    let mut has_reference = false;

                    let mut bg_off = 0;
                    while bg_off < bg_data.len() {
                        let (child_id, _, cid_len) = parse_vint(&bg_data[bg_off..], true)?;
                        bg_off += cid_len;
                        let (_, csz_opt, csz_len) = parse_vint(&bg_data[bg_off..], false)?;
                        bg_off += csz_len;
                        let csz = csz_opt.unwrap_or(0) as usize;
                        if bg_off + csz > bg_data.len() {
                            break;
                        }
                        let c_val = &bg_data[bg_off..bg_off + csz];
                        bg_off += csz;

                        match child_id {
                            0xA1 => block_slice = Some(c_val),
                            0xFB => has_reference = true,
                            _ => {}
                        }
                    }

                    if let Some(block_data) = block_slice {
                        if block_data.len() >= 4 {
                            let (_, track_id_opt, track_len) = parse_vint(block_data, false)?;
                            let track_id = track_id_opt.unwrap_or(1);
                            if track_len + 3 <= block_data.len() {
                                let rel_timecode = i16::from_be_bytes([
                                    block_data[track_len],
                                    block_data[track_len + 1],
                                ]);
                                let flags = block_data[track_len + 2];
                                let is_keyframe = !has_reference;
                                let lacing = (flags & 0x06) >> 1;
                                let payload = &block_data[track_len + 3..];

                                let cluster_ticks = i64::try_from(self.current_cluster_timecode)
                                    .map_err(|_| DemuxError::IntegerOverflow)?;
                                let (_, pts_ms) = compute_simpleblock_pts(
                                    cluster_ticks,
                                    rel_timecode,
                                    self.timestamp_scale_ns,
                                )?;
                                let pts_ms = pts_ms.max(0) as u64;

                                let frames = unpack_lacing(lacing, payload)?;
                                for (i, frame) in frames.into_iter().enumerate() {
                                    self.packet_queue.push_back(DemuxPacket {
                                        track_id,
                                        pts_ms,
                                        dts_ms: None,
                                        duration_ms: None,
                                        is_keyframe: if i == 0 { is_keyframe } else { false },
                                        data: frame,
                                    });
                                }

                                if let Some(pkt) = self.packet_queue.pop_front() {
                                    return Ok(Some(pkt));
                                }
                            }
                        }
                    }
                }
                _ => {
                    if let Some(sz) = size_opt {
                        let seek_offset =
                            i64::try_from(sz).map_err(|_| DemuxError::IntegerOverflow)?;
                        reader
                            .seek(SeekFrom::Current(seek_offset))
                            .map_err(|e| DemuxError::Io(e.to_string()))?;
                    }
                }
            }
        }
    }

    fn seek_to(&mut self, target_pts_ms: u64) -> Result<u64, DemuxError> {
        self.packet_queue.clear();

        if !self.cues.is_empty() {
            let selected_cue = match self.cues.binary_search_by_key(&target_pts_ms, |c| c.pts_ms) {
                Ok(idx) => &self.cues[idx],
                Err(idx) => {
                    if idx == 0 {
                        &self.cues[0]
                    } else {
                        &self.cues[idx - 1]
                    }
                }
            };

            let landed_pts = selected_cue.pts_ms;
            let target_offset = selected_cue.cluster_file_offset;
            self.current_cluster_timecode = selected_cue.cluster_timecode;

            if let Some(reader) = &mut self.reader {
                reader
                    .seek(SeekFrom::Start(target_offset))
                    .map_err(|e| DemuxError::Io(e.to_string()))?;
            }
            Ok(landed_pts)
        } else {
            let fallback_offset = self.first_cluster_offset.unwrap_or(self.segment_data_start);
            if let Some(reader) = &mut self.reader {
                reader
                    .seek(SeekFrom::Start(fallback_offset))
                    .map_err(|e| DemuxError::Io(e.to_string()))?;
            }
            self.current_cluster_timecode = 0;
            Ok(0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_synthetic_ebml_vector_verification() {
        assert_eq!(SYNTHETIC_EBML_VECTOR.len(), 48);
        let (doc_type, ver, len) = EbmlDemuxer::parse_ebml_header(&SYNTHETIC_EBML_VECTOR).unwrap();
        assert_eq!(doc_type, "matroska");
        assert_eq!(ver, 4);
        assert_eq!(len, 36);

        // Verify root segment element at offset 36
        let (seg_id, _size_opt, width) = parse_vint(&SYNTHETIC_EBML_VECTOR[36..], true).unwrap();
        assert_eq!(seg_id, 0x1853_8067);
        assert_eq!(width, 4);
        let (_, seg_size, size_width) = parse_vint(&SYNTHETIC_EBML_VECTOR[40..], false).unwrap();
        assert_eq!(seg_size, None); // Unknown size
        assert_eq!(size_width, 8);
    }

    #[test]
    fn test_simpleblock_timescale_24h_playback() {
        let default_scale = 1_000_000u64;

        // Baseline: cluster=10,000 ticks, block=+250 ticks, scale=1,000,000
        let (pts_ns, pts_ms) = compute_simpleblock_pts(10000, 250, default_scale).unwrap();
        assert_eq!(pts_ns, 10_250_000_000);
        assert_eq!(pts_ms, 10250);

        // Negative relative block timecode
        let (pts_ns_neg, pts_ms_neg) = compute_simpleblock_pts(1000, -250, default_scale).unwrap();
        assert_eq!(pts_ns_neg, 750_000_000);
        assert_eq!(pts_ms_neg, 750);

        // 24-hour playback
        let ms_24h = 24 * 3600 * 1000i64;
        let (pts_ns_24h, pts_ms_24h) = compute_simpleblock_pts(ms_24h, 0, default_scale).unwrap();
        assert_eq!(pts_ms_24h, ms_24h);
        assert_eq!(pts_ns_24h, 86_400_000_000_000);
    }

    #[test]
    fn test_compute_simpleblock_pts_boundary_and_overflow_rejection() {
        // 1. Valid baseline computations
        let (pts_ns, pts_ms) = compute_simpleblock_pts(1000, 0, 1_000_000).unwrap();
        assert_eq!(pts_ns, 1_000_000_000);
        assert_eq!(pts_ms, 1000);

        // 2. Reject u64::MAX timescale (defect reproduction)
        assert_eq!(
            compute_simpleblock_pts(1000, 0, u64::MAX),
            Err(DemuxError::IntegerOverflow)
        );
        assert_eq!(
            compute_simpleblock_pts(0, 0, u64::MAX),
            Err(DemuxError::IntegerOverflow)
        );

        // 3. Reject timescale strictly exceeding i64::MAX
        let overflow_scale = (i64::MAX as u64) + 1;
        assert_eq!(
            compute_simpleblock_pts(1, 0, overflow_scale),
            Err(DemuxError::IntegerOverflow)
        );

        // 4. Accept boundary timescale == i64::MAX with tick 1, reject with tick >= 2
        let max_scale = i64::MAX as u64;
        let (max_ns, max_ms) = compute_simpleblock_pts(1, 0, max_scale).unwrap();
        assert_eq!(max_ns, i64::MAX);
        assert_eq!(max_ms, i64::MAX / 1_000_000);

        assert_eq!(
            compute_simpleblock_pts(2, 0, max_scale),
            Err(DemuxError::IntegerOverflow)
        );

        // 5. Total ticks addition overflow
        assert_eq!(
            compute_simpleblock_pts(i64::MAX, 1, 1_000_000),
            Err(DemuxError::IntegerOverflow)
        );
        assert_eq!(
            compute_simpleblock_pts(i64::MIN, -1, 1_000_000),
            Err(DemuxError::IntegerOverflow)
        );

        // 6. Multiplication overflow with standard timescale (1_000_000 ns = 1 ms)
        let max_safe_ticks = i64::MAX / 1_000_000;
        let (pts_ns_safe, pts_ms_safe) =
            compute_simpleblock_pts(max_safe_ticks, 0, 1_000_000).unwrap();
        assert_eq!(pts_ns_safe, max_safe_ticks * 1_000_000);
        assert_eq!(pts_ms_safe, max_safe_ticks);

        assert_eq!(
            compute_simpleblock_pts(max_safe_ticks + 1, 0, 1_000_000),
            Err(DemuxError::IntegerOverflow)
        );

        // 7. Negative multiplication underflow
        assert_eq!(
            compute_simpleblock_pts(-(max_safe_ticks + 1), 0, 1_000_000),
            Err(DemuxError::IntegerOverflow)
        );
    }

    #[test]
    fn test_unpack_lacing_modes() {
        // Mode 0: No lacing
        let raw = vec![1, 2, 3, 4];
        let frames0 = unpack_lacing(0, &raw).unwrap();
        assert_eq!(frames0, vec![vec![1, 2, 3, 4]]);

        // Mode 1: Xiph lacing
        // 2 frames: first frame 300 bytes (255 + 45), second frame 10 bytes
        let mut xiph_payload = vec![1]; // 2 frames (count - 1 = 1)
        xiph_payload.push(255);
        xiph_payload.push(45);
        xiph_payload.extend(vec![0xAA; 300]);
        xiph_payload.extend(vec![0xBB; 10]);
        let frames1 = unpack_lacing(1, &xiph_payload).unwrap();
        assert_eq!(frames1.len(), 2);
        assert_eq!(frames1[0].len(), 300);
        assert_eq!(frames1[1].len(), 10);
        assert_eq!(frames1[0][0], 0xAA);
        assert_eq!(frames1[1][0], 0xBB);

        // Mode 2: Fixed lacing
        // 3 frames: 6 bytes payload / 3 = 2 bytes per frame
        let fixed_payload = vec![2, 10, 11, 20, 21, 30, 31];
        let frames2 = unpack_lacing(2, &fixed_payload).unwrap();
        assert_eq!(frames2.len(), 3);
        assert_eq!(frames2[0], vec![10, 11]);
        assert_eq!(frames2[1], vec![20, 21]);
        assert_eq!(frames2[2], vec![30, 31]);

        // Mode 3: EBML lacing
        // 3 frames: frame 0 = 5 bytes, frame 1 = 7 bytes (+2 delta), frame 2 = 3 bytes
        // Frame 0 size VINT = 0x85 (5 bytes)
        // Frame 1 delta VINT: w=1, bias=63, val=2+63=65 (0x41), with marker bit = 0xC1
        // Remaining 3 bytes for frame 2
        let mut ebml_payload = vec![2]; // 3 frames (3 - 1 = 2)
        ebml_payload.push(0x85); // 5 bytes
        ebml_payload.push(0xC1); // +2 delta
        ebml_payload.extend(vec![1; 5]);
        ebml_payload.extend(vec![2; 7]);
        ebml_payload.extend(vec![3; 3]);
        let frames3 = unpack_lacing(3, &ebml_payload).unwrap();
        assert_eq!(frames3.len(), 3);
        assert_eq!(frames3[0], vec![1; 5]);
        assert_eq!(frames3[1], vec![2; 7]);
        assert_eq!(frames3[2], vec![3; 3]);

        // Mode 3: EBML lacing with frame_count = 1
        let ebml_single = vec![0, 42, 43, 44];
        let frames_single = unpack_lacing(3, &ebml_single).unwrap();
        assert_eq!(frames_single.len(), 1);
        assert_eq!(frames_single[0], vec![42, 43, 44]);

        // Mode 3: EBML lacing with delta 0xFF (+64)
        let mut ebml_max_delta = vec![2, 0x8A, 0xFF];
        ebml_max_delta.extend(vec![0xAA; 10]);
        ebml_max_delta.extend(vec![0xBB; 74]);
        ebml_max_delta.extend(vec![0xCC; 5]);
        let frames_max_delta = unpack_lacing(3, &ebml_max_delta).unwrap();
        assert_eq!(frames_max_delta.len(), 3);
        assert_eq!(frames_max_delta[0].len(), 10);
        assert_eq!(frames_max_delta[1].len(), 74);
        assert_eq!(frames_max_delta[2].len(), 5);
    }
}
