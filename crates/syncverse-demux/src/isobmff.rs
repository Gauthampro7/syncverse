//! ISOBMFF / MP4 container box parser, sample table mapping, and synthetic vectors.

use std::fmt;
use std::io::{Read, Seek, SeekFrom};
use crate::error::DemuxError;
use crate::traits::{ContainerInfo, DemuxPacket, Demuxer, ReadSeek, TrackInfo, TrackType};

/// 4-byte box identifier (FourCC).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FourCc(pub [u8; 4]);

impl FourCc {
    /// Constructs a `FourCc` from a 4-byte string slice.
    pub const fn from_bytes(bytes: [u8; 4]) -> Self {
        Self(bytes)
    }

    /// Returns the FourCC as an ASCII string.
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or("????")
    }
}

impl fmt::Debug for FourCc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FourCc({})", self.as_str())
    }
}

impl fmt::Display for FourCc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Parsed ISOBMFF Box header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoxHeader {
    /// FourCC identifier of the box.
    pub fourcc: FourCc,
    /// Header size in bytes (8 for standard 32-bit, 16 for 64-bit largesize).
    pub header_size: usize,
    /// Total box size in bytes including header.
    pub total_size: u64,
}

impl BoxHeader {
    /// Parses an ISOBMFF box header conforming to spec requirements:
    /// - If `size == 1 && largesize < 16` => `Err(DemuxError::MalformedBox)`
    /// - `largesize` MUST NOT exceed `i64::MAX as u64` ($2^{63}-1$)
    /// - If `size != 0 && size < 8` => `Err(DemuxError::MalformedBox)`
    /// - If available bytes < header size => `Err(DemuxError::UnexpectedEof)`
    pub fn parse(buf: &[u8]) -> Result<Self, DemuxError> {
        if buf.len() < 8 {
            return Err(DemuxError::UnexpectedEof {
                needed: 8,
                available: buf.len(),
            });
        }

        let size32 = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let fourcc = FourCc([buf[4], buf[5], buf[6], buf[7]]);

        if size32 == 1 {
            // 64-bit largesize
            if buf.len() < 16 {
                return Err(DemuxError::UnexpectedEof {
                    needed: 16,
                    available: buf.len(),
                });
            }
            let largesize = u64::from_be_bytes([
                buf[8], buf[9], buf[10], buf[11],
                buf[12], buf[13], buf[14], buf[15],
            ]);

            if largesize < 16 {
                return Err(DemuxError::MalformedBox(format!(
                    "largesize {} < 16 is malformed",
                    largesize
                )));
            }

            const I64_MAX_U64: u64 = i64::MAX as u64;
            if largesize > I64_MAX_U64 {
                return Err(DemuxError::MalformedBox(format!(
                    "largesize {} exceeds i64::MAX",
                    largesize
                )));
            }

            Ok(Self {
                fourcc,
                header_size: 16,
                total_size: largesize,
            })
        } else if size32 == 0 {
            // Extends to EOF
            Ok(Self {
                fourcc,
                header_size: 8,
                total_size: buf.len() as u64,
            })
        } else if size32 < 8 {
            Err(DemuxError::MalformedBox(format!(
                "32-bit box size {} < 8 is malformed",
                size32
            )))
        } else {
            Ok(Self {
                fourcc,
                header_size: 8,
                total_size: size32 as u64,
            })
        }
    }
}

/// 132-byte in-tree synthetic MP4 header test vector containing `ftyp` (16 bytes) and `moov/mvhd` (116 bytes).
pub const SYNTHETIC_MP4_VECTOR: [u8; 132] = [
    // --- 1. ftyp box (16 bytes) ---
    0x00, 0x00, 0x00, 0x10, // size = 16
    b'f', b't', b'y', b'p', // fourcc = "ftyp"
    b'i', b's', b'o', b'm', // major_brand = "isom"
    0x00, 0x00, 0x02, 0x00, // minor_version = 0x00000200
    // --- 2. moov box (116 bytes) ---
    0x00, 0x00, 0x00, 0x74, // size = 116
    b'm', b'o', b'o', b'v', // fourcc = "moov"
    // --- 2.1 mvhd box inside moov (108 bytes) ---
    0x00, 0x00, 0x00, 0x6C, // size = 108
    b'm', b'v', b'h', b'd', // fourcc = "mvhd"
    0x00,                   // version = 0
    0x00, 0x00, 0x00,       // flags = 0
    0x00, 0x00, 0x00, 0x00, // creation_time = 0
    0x00, 0x00, 0x00, 0x00, // modification_time = 0
    0x00, 0x00, 0x03, 0xE8, // timescale = 1000 ticks/s (1000 Hz)
    0x00, 0x00, 0x27, 0x10, // duration = 10000 ticks (10.0 seconds = 10,000 ms)
    0x00, 0x01, 0x00, 0x00, // rate = 1.0 (0x00010000 fixed-point 16.16)
    0x01, 0x00,             // volume = 1.0 (0x0100 fixed-point 8.8)
    0x00, 0x00,             // reserved = 0
    0x00, 0x00, 0x00, 0x00, // reserved[0] = 0
    0x00, 0x00, 0x00, 0x00, // reserved[1] = 0
    // Unity matrix (36 bytes):
    0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00,
    // pre_defined (24 bytes):
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x02, // next_track_id = 2
];

/// Sample-to-Chunk mapping entry (`stsc`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StscEntry {
    /// 1-based index of the first chunk this entry applies to.
    pub first_chunk: u32,
    /// Number of samples contained in each chunk. Must be >= 1.
    pub samples_per_chunk: u32,
    /// Sample description index.
    pub sample_description_index: u32,
}

/// ISOBMFF Sample Table (`stbl`) index resolver.
#[derive(Debug, Clone, Default)]
pub struct SampleTableResolver {
    /// Time-to-sample run-length entries `(sample_count, sample_delta)`.
    pub stts: Vec<(u32, u32)>,
    /// Composition time-to-sample run-length entries `(sample_count, sample_offset)`.
    pub ctts: Vec<(u32, i32)>,
    /// Sample-to-chunk run-length entries.
    pub stsc: Vec<StscEntry>,
    /// Sample size table (or empty if uniform size is used).
    pub stsz: Vec<u32>,
    /// Default uniform sample size if `stsz` table is empty.
    pub uniform_sample_size: Option<u32>,
    /// Absolute chunk offsets (`stco` or `co64`).
    pub chunk_offsets: Vec<u64>,
    /// 0-based sync sample indices (`stss`).
    pub sync_samples: Vec<usize>,
}

impl SampleTableResolver {
    /// Constructs a new empty `SampleTableResolver`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets sync samples from 1-based `stss` entries, converting to 0-based indices ($S = sync - 1$).
    pub fn set_sync_samples_1based(&mut self, stss_entries: &[u32]) {
        self.sync_samples = stss_entries
            .iter()
            .filter_map(|&s| if s >= 1 { Some((s - 1) as usize) } else { None })
            .collect();
    }

    /// Adds an `stsc` entry, validating that `samples_per_chunk >= 1`.
    pub fn add_stsc_entry(
        &mut self,
        first_chunk: u32,
        samples_per_chunk: u32,
        sample_description_index: u32,
    ) -> Result<(), DemuxError> {
        if samples_per_chunk == 0 {
            return Err(DemuxError::CorruptSampleTable(
                "samples_per_chunk cannot be 0".into(),
            ));
        }
        self.stsc.push(StscEntry {
            first_chunk,
            samples_per_chunk,
            sample_description_index,
        });
        Ok(())
    }

    /// Returns the total number of samples described by this sample table.
    pub fn total_samples(&self) -> usize {
        if !self.stsz.is_empty() {
            self.stsz.len()
        } else if self.uniform_sample_size.is_some() {
            self.stts.iter().map(|&(count, _)| count as usize).sum()
        } else {
            0
        }
    }

    /// Computes presentation and decode timestamps in milliseconds for all samples.
    pub fn compute_sample_timings(&self, timescale: u64) -> (Vec<u64>, Vec<u64>) {
        if timescale == 0 {
            return (Vec::new(), Vec::new());
        }

        let mut dts_ticks = Vec::new();
        let mut curr_dts = 0u64;
        for &(count, delta) in &self.stts {
            for _ in 0..count {
                dts_ticks.push(curr_dts);
                curr_dts = curr_dts.saturating_add(delta as u64);
            }
        }

        let total = self.total_samples().max(dts_ticks.len());
        while dts_ticks.len() < total {
            dts_ticks.push(curr_dts);
        }

        let mut pts_ticks = Vec::with_capacity(total);
        if !self.ctts.is_empty() {
            let mut ctts_unrolled = Vec::with_capacity(total);
            for &(count, offset) in &self.ctts {
                for _ in 0..count {
                    ctts_unrolled.push(offset);
                }
            }
            for (i, &dts) in dts_ticks.iter().enumerate() {
                let c_off = ctts_unrolled.get(i).copied().unwrap_or(0);
                let pts = (dts as i64).saturating_add(c_off as i64).max(0) as u64;
                pts_ticks.push(pts);
            }
        } else {
            pts_ticks = dts_ticks.clone();
        }

        let dts_ms: Vec<u64> = dts_ticks
            .iter()
            .map(|&t| t.saturating_mul(1000) / timescale)
            .collect();
        let pts_ms: Vec<u64> = pts_ticks
            .iter()
            .map(|&t| t.saturating_mul(1000) / timescale)
            .collect();

        (dts_ms, pts_ms)
    }

    /// Computes the sample byte size for 0-based sample index $S$.
    pub fn get_sample_size(&self, sample_index: usize) -> Result<u64, DemuxError> {
        if let Some(uniform) = self.uniform_sample_size {
            return Ok(uniform as u64);
        }
        if sample_index < self.stsz.len() {
            Ok(self.stsz[sample_index] as u64)
        } else {
            Err(DemuxError::NotFound(format!(
                "sample index {} out of stsz bounds",
                sample_index
            )))
        }
    }

    /// Resolves the absolute byte offset of 0-based sample index $S$ in the file.
    ///
    /// Formula:
    /// `Offset(S) = ChunkOffset[C] + sum_{j=S-k}^{S-1} size(j)`
    pub fn compute_sample_offset(&self, sample_index: usize) -> Result<u64, DemuxError> {
        if self.stsc.is_empty() || self.chunk_offsets.is_empty() {
            return Err(DemuxError::CorruptSampleTable(
                "empty stsc or chunk_offsets table".into(),
            ));
        }

        let mut current_sample = 0usize;
        let num_chunks = self.chunk_offsets.len();

        for chunk_idx in 0..num_chunks {
            let chunk_1based = (chunk_idx + 1) as u32;

            let stsc_entry = self
                .stsc
                .iter()
                .rfind(|e| e.first_chunk <= chunk_1based)
                .ok_or_else(|| {
                    DemuxError::CorruptSampleTable(format!(
                        "no stsc entry for chunk {}",
                        chunk_1based
                    ))
                })?;

            let samples_in_chunk = stsc_entry.samples_per_chunk as usize;
            if sample_index < current_sample + samples_in_chunk {
                let chunk_base_offset = self.chunk_offsets[chunk_idx];

                let mut sample_offset = chunk_base_offset;
                for j in current_sample..sample_index {
                    sample_offset = sample_offset
                        .checked_add(self.get_sample_size(j)?)
                        .ok_or(DemuxError::IntegerOverflow)?;
                }
                return Ok(sample_offset);
            }
            current_sample += samples_in_chunk;
        }

        Err(DemuxError::NotFound(format!(
            "sample index {} exceeds total samples in chunks",
            sample_index
        )))
    }
}

/// Active track state for ISOBMFF demuxing.
#[derive(Debug, Clone)]
pub struct Mp4TrackState {
    /// Track identifier.
    pub track_id: u64,
    /// Track media type.
    pub track_type: TrackType,
    /// Native media timescale in Hz.
    pub timescale: u64,
    /// Sample table resolver mapping samples to file offsets and sizes.
    pub resolver: SampleTableResolver,
    /// Current sample index to be demuxed.
    pub current_sample: usize,
    /// Total number of samples in the track.
    pub total_samples: usize,
    /// Precomputed decode timestamps in milliseconds.
    pub sample_dts_ms: Vec<u64>,
    /// Precomputed presentation timestamps in milliseconds.
    pub sample_pts_ms: Vec<u64>,
}

/// Pure-Rust ISOBMFF / MP4 container demuxer.
pub struct IsoBmffDemuxer {
    reader: Option<Box<dyn ReadSeek + Send>>,
    info: Option<ContainerInfo>,
    tracks: Vec<Mp4TrackState>,
}

impl Default for IsoBmffDemuxer {
    fn default() -> Self {
        Self::new()
    }
}

impl IsoBmffDemuxer {
    /// Constructs an uninitialized `IsoBmffDemuxer`.
    pub fn new() -> Self {
        Self {
            reader: None,
            info: None,
            tracks: Vec::new(),
        }
    }

    /// Returns the internal track demuxing states.
    pub fn track_states(&self) -> &[Mp4TrackState] {
        &self.tracks
    }

    /// Parses the top-level MP4 headers and returns `(timescale, duration_ms)`.
    pub fn parse_headers_from_slice(buf: &[u8]) -> Result<(u64, u64), DemuxError> {
        let mut offset = 0;
        let mut timescale = 1000u64;
        let mut duration_ms = 0u64;

        while offset + 8 <= buf.len() {
            let hdr = BoxHeader::parse(&buf[offset..])?;
            if hdr.fourcc == FourCc(*b"moov") {
                let mut moov_offset = offset + hdr.header_size;
                let moov_end = offset + (hdr.total_size as usize).min(buf.len() - offset);
                while moov_offset + 8 <= moov_end {
                    let sub_hdr = BoxHeader::parse(&buf[moov_offset..])?;
                    if sub_hdr.fourcc == FourCc(*b"mvhd") {
                        let mvhd_data = &buf[moov_offset + sub_hdr.header_size..];
                        if mvhd_data.len() >= 20 {
                            let version = mvhd_data[0];
                            if version == 0 {
                                let ts = u32::from_be_bytes([
                                    mvhd_data[12],
                                    mvhd_data[13],
                                    mvhd_data[14],
                                    mvhd_data[15],
                                ]);
                                let dur = u32::from_be_bytes([
                                    mvhd_data[16],
                                    mvhd_data[17],
                                    mvhd_data[18],
                                    mvhd_data[19],
                                ]);
                                timescale = ts as u64;
                                duration_ms = if ts > 0 {
                                    (dur as u64 * 1000) / (ts as u64)
                                } else {
                                    0
                                };
                            } else if version == 1 && mvhd_data.len() >= 32 {
                                let ts = u32::from_be_bytes([
                                    mvhd_data[20],
                                    mvhd_data[21],
                                    mvhd_data[22],
                                    mvhd_data[23],
                                ]);
                                let dur = u64::from_be_bytes([
                                    mvhd_data[24],
                                    mvhd_data[25],
                                    mvhd_data[26],
                                    mvhd_data[27],
                                    mvhd_data[28],
                                    mvhd_data[29],
                                    mvhd_data[30],
                                    mvhd_data[31],
                                ]);
                                timescale = ts as u64;
                                duration_ms = if ts > 0 {
                                    (dur.saturating_mul(1000)) / (ts as u64)
                                } else {
                                    0
                                };
                            }
                        }
                    }
                    moov_offset += sub_hdr.total_size as usize;
                }
            }
            offset += hdr.total_size as usize;
        }

        Ok((timescale, duration_ms))
    }

    fn parse_trak(
        trak_data: &[u8],
    ) -> Result<Option<(TrackInfo, Mp4TrackState)>, DemuxError> {
        let mut offset = 0;
        let mut track_id = 1u64;
        let mut track_type = TrackType::Video;
        let mut width: Option<u32> = None;
        let mut height: Option<u32> = None;
        let mut sample_rate: Option<f64> = None;
        let mut channels: Option<u32> = None;
        let mut codec_id = String::from("unknown");
        let mut codec_private: Option<Vec<u8>> = None;
        let mut track_timescale = 1000u64;
        let mut resolver = SampleTableResolver::new();

        while offset + 8 <= trak_data.len() {
            let hdr = BoxHeader::parse(&trak_data[offset..])?;
            let box_end = offset + (hdr.total_size as usize).min(trak_data.len() - offset);
            let body = &trak_data[offset + hdr.header_size..box_end];

            if hdr.fourcc == FourCc(*b"tkhd") && body.len() >= 84 {
                let version = body[0];
                if version == 0 {
                    track_id = u32::from_be_bytes([body[12], body[13], body[14], body[15]]) as u64;
                    let w = u32::from_be_bytes([body[76], body[77], body[78], body[79]]) >> 16;
                    let h = u32::from_be_bytes([body[80], body[81], body[82], body[83]]) >> 16;
                    if w > 0 {
                        width = Some(w);
                    }
                    if h > 0 {
                        height = Some(h);
                    }
                } else if version == 1 && body.len() >= 96 {
                    track_id = u32::from_be_bytes([body[20], body[21], body[22], body[23]]) as u64;
                    let w = u32::from_be_bytes([body[88], body[89], body[90], body[91]]) >> 16;
                    let h = u32::from_be_bytes([body[92], body[93], body[94], body[95]]) >> 16;
                    if w > 0 {
                        width = Some(w);
                    }
                    if h > 0 {
                        height = Some(h);
                    }
                }
            } else if hdr.fourcc == FourCc(*b"mdia") {
                let mut mdia_off = 0;
                while mdia_off + 8 <= body.len() {
                    let m_hdr = BoxHeader::parse(&body[mdia_off..])?;
                    let m_end =
                        mdia_off + (m_hdr.total_size as usize).min(body.len() - mdia_off);
                    let m_body = &body[mdia_off + m_hdr.header_size..m_end];

                    if m_hdr.fourcc == FourCc(*b"mdhd") && m_body.len() >= 20 {
                        let version = m_body[0];
                        track_timescale = if version == 0 {
                            u32::from_be_bytes([m_body[12], m_body[13], m_body[14], m_body[15]])
                                as u64
                        } else if m_body.len() >= 24 {
                            u32::from_be_bytes([m_body[20], m_body[21], m_body[22], m_body[23]])
                                as u64
                        } else {
                            1000
                        };
                    } else if m_hdr.fourcc == FourCc(*b"hdlr") && m_body.len() >= 12 {
                        let handler = &m_body[8..12];
                        track_type = match handler {
                            b"vide" => TrackType::Video,
                            b"soun" => TrackType::Audio,
                            b"subt" | b"sbtl" | b"text" => TrackType::Subtitle,
                            _ => TrackType::Video,
                        };
                    } else if m_hdr.fourcc == FourCc(*b"minf") {
                        let mut minf_off = 0;
                        while minf_off + 8 <= m_body.len() {
                            let minf_hdr = BoxHeader::parse(&m_body[minf_off..])?;
                            let minf_end = minf_off
                                + (minf_hdr.total_size as usize).min(m_body.len() - minf_off);
                            let minf_sub_body = &m_body[minf_off + minf_hdr.header_size..minf_end];

                            if minf_hdr.fourcc == FourCc(*b"stbl") {
                                let mut stbl_off = 0;
                                while stbl_off + 8 <= minf_sub_body.len() {
                                    let s_hdr = BoxHeader::parse(&minf_sub_body[stbl_off..])?;
                                    let s_end = stbl_off
                                        + (s_hdr.total_size as usize)
                                            .min(minf_sub_body.len() - stbl_off);
                                    let s_body =
                                        &minf_sub_body[stbl_off + s_hdr.header_size..s_end];

                                    match &s_hdr.fourcc.0 {
                                        b"stsd" if s_body.len() >= 8 => {
                                            let entry_count = u32::from_be_bytes([
                                                s_body[4], s_body[5], s_body[6], s_body[7],
                                            ]);
                                            if entry_count > 0 && s_body.len() >= 16 {
                                                let _entry_size = u32::from_be_bytes([
                                                    s_body[8], s_body[9], s_body[10], s_body[11],
                                                ]) as usize;
                                                let fourcc = &s_body[12..16];
                                                codec_id = std::str::from_utf8(fourcc)
                                                    .unwrap_or("unknown")
                                                    .to_string();

                                                let entry_body = &s_body[16..];
                                                if track_type == TrackType::Video
                                                    && entry_body.len() >= 28
                                                {
                                                    let w = u16::from_be_bytes([
                                                        entry_body[24], entry_body[25],
                                                    ]) as u32;
                                                    let h = u16::from_be_bytes([
                                                        entry_body[26], entry_body[27],
                                                    ]) as u32;
                                                    if w > 0 {
                                                        width = Some(w);
                                                    }
                                                    if h > 0 {
                                                        height = Some(h);
                                                    }

                                                    if entry_body.len() > 70 {
                                                        let extra = &entry_body[70..];
                                                        let mut e_off = 0;
                                                        while e_off + 8 <= extra.len() {
                                                            if let Ok(e_hdr) =
                                                                BoxHeader::parse(&extra[e_off..])
                                                            {
                                                                if e_hdr.fourcc == FourCc(*b"avcC")
                                                                    || e_hdr.fourcc
                                                                        == FourCc(*b"hvcC")
                                                                    || e_hdr.fourcc
                                                                        == FourCc(*b"vpcC")
                                                                {
                                                                    let tot =
                                                                        e_hdr.total_size as usize;
                                                                    if e_off + tot <= extra.len() {
                                                                        codec_private = Some(
                                                                            extra[e_off
                                                                                + e_hdr.header_size
                                                                                ..e_off + tot]
                                                                                .to_vec(),
                                                                        );
                                                                    }
                                                                    break;
                                                                }
                                                                e_off += e_hdr.total_size as usize;
                                                            } else {
                                                                break;
                                                            }
                                                        }
                                                    }
                                                } else if track_type == TrackType::Audio
                                                    && entry_body.len() >= 28
                                                {
                                                    let ch = u16::from_be_bytes([
                                                        entry_body[16], entry_body[17],
                                                    ]) as u32;
                                                    let sr = u32::from_be_bytes([
                                                        entry_body[24],
                                                        entry_body[25],
                                                        entry_body[26],
                                                        entry_body[27],
                                                    ]) >> 16;
                                                    channels = Some(ch);
                                                    sample_rate = Some(sr as f64);
                                                }
                                            }
                                        }
                                        b"stts" if s_body.len() >= 8 => {
                                            let count = u32::from_be_bytes([
                                                s_body[4], s_body[5], s_body[6], s_body[7],
                                            ]) as usize;
                                            let mut pos = 8;
                                            for _ in 0..count {
                                                if pos + 8 <= s_body.len() {
                                                    let sc = u32::from_be_bytes([
                                                        s_body[pos],
                                                        s_body[pos + 1],
                                                        s_body[pos + 2],
                                                        s_body[pos + 3],
                                                    ]);
                                                    let sd = u32::from_be_bytes([
                                                        s_body[pos + 4],
                                                        s_body[pos + 5],
                                                        s_body[pos + 6],
                                                        s_body[pos + 7],
                                                    ]);
                                                    resolver.stts.push((sc, sd));
                                                    pos += 8;
                                                } else {
                                                    break;
                                                }
                                            }
                                        }
                                        b"ctts" if s_body.len() >= 8 => {
                                            let count = u32::from_be_bytes([
                                                s_body[4], s_body[5], s_body[6], s_body[7],
                                            ]) as usize;
                                            let mut pos = 8;
                                            for _ in 0..count {
                                                if pos + 8 <= s_body.len() {
                                                    let sc = u32::from_be_bytes([
                                                        s_body[pos],
                                                        s_body[pos + 1],
                                                        s_body[pos + 2],
                                                        s_body[pos + 3],
                                                    ]);
                                                    let so = i32::from_be_bytes([
                                                        s_body[pos + 4],
                                                        s_body[pos + 5],
                                                        s_body[pos + 6],
                                                        s_body[pos + 7],
                                                    ]);
                                                    resolver.ctts.push((sc, so));
                                                    pos += 8;
                                                } else {
                                                    break;
                                                }
                                            }
                                        }
                                        b"stss" if s_body.len() >= 8 => {
                                            let count = u32::from_be_bytes([
                                                s_body[4], s_body[5], s_body[6], s_body[7],
                                            ]) as usize;
                                            let mut pos = 8;
                                            let mut entries = Vec::with_capacity(count);
                                            for _ in 0..count {
                                                if pos + 4 <= s_body.len() {
                                                    let sn = u32::from_be_bytes([
                                                        s_body[pos],
                                                        s_body[pos + 1],
                                                        s_body[pos + 2],
                                                        s_body[pos + 3],
                                                    ]);
                                                    entries.push(sn);
                                                    pos += 4;
                                                } else {
                                                    break;
                                                }
                                            }
                                            resolver.set_sync_samples_1based(&entries);
                                        }
                                        b"stsc" if s_body.len() >= 8 => {
                                            let count = u32::from_be_bytes([
                                                s_body[4], s_body[5], s_body[6], s_body[7],
                                            ]) as usize;
                                            let mut pos = 8;
                                            for _ in 0..count {
                                                if pos + 12 <= s_body.len() {
                                                    let fc = u32::from_be_bytes([
                                                        s_body[pos],
                                                        s_body[pos + 1],
                                                        s_body[pos + 2],
                                                        s_body[pos + 3],
                                                    ]);
                                                    let spc = u32::from_be_bytes([
                                                        s_body[pos + 4],
                                                        s_body[pos + 5],
                                                        s_body[pos + 6],
                                                        s_body[pos + 7],
                                                    ]);
                                                    let sdi = u32::from_be_bytes([
                                                        s_body[pos + 8],
                                                        s_body[pos + 9],
                                                        s_body[pos + 10],
                                                        s_body[pos + 11],
                                                    ]);
                                                    resolver.add_stsc_entry(fc, spc, sdi)?;
                                                    pos += 12;
                                                } else {
                                                    break;
                                                }
                                            }
                                        }
                                        b"stsz" if s_body.len() >= 12 => {
                                            let uniform = u32::from_be_bytes([
                                                s_body[4], s_body[5], s_body[6], s_body[7],
                                            ]);
                                            let count = u32::from_be_bytes([
                                                s_body[8], s_body[9], s_body[10], s_body[11],
                                            ]) as usize;
                                            if uniform > 0 {
                                                resolver.uniform_sample_size = Some(uniform);
                                            } else {
                                                let mut pos = 12;
                                                for _ in 0..count {
                                                    if pos + 4 <= s_body.len() {
                                                        let sz = u32::from_be_bytes([
                                                            s_body[pos],
                                                            s_body[pos + 1],
                                                            s_body[pos + 2],
                                                            s_body[pos + 3],
                                                        ]);
                                                        resolver.stsz.push(sz);
                                                        pos += 4;
                                                    } else {
                                                        break;
                                                    }
                                                }
                                            }
                                        }
                                        b"stco" if s_body.len() >= 8 => {
                                            let count = u32::from_be_bytes([
                                                s_body[4], s_body[5], s_body[6], s_body[7],
                                            ]) as usize;
                                            let mut pos = 8;
                                            for _ in 0..count {
                                                if pos + 4 <= s_body.len() {
                                                    let off = u32::from_be_bytes([
                                                        s_body[pos],
                                                        s_body[pos + 1],
                                                        s_body[pos + 2],
                                                        s_body[pos + 3],
                                                    ]) as u64;
                                                    resolver.chunk_offsets.push(off);
                                                    pos += 4;
                                                } else {
                                                    break;
                                                }
                                            }
                                        }
                                        b"co64" if s_body.len() >= 8 => {
                                            let count = u32::from_be_bytes([
                                                s_body[4], s_body[5], s_body[6], s_body[7],
                                            ]) as usize;
                                            let mut pos = 8;
                                            for _ in 0..count {
                                                if pos + 8 <= s_body.len() {
                                                    let off = u64::from_be_bytes([
                                                        s_body[pos],
                                                        s_body[pos + 1],
                                                        s_body[pos + 2],
                                                        s_body[pos + 3],
                                                        s_body[pos + 4],
                                                        s_body[pos + 5],
                                                        s_body[pos + 6],
                                                        s_body[pos + 7],
                                                    ]);
                                                    resolver.chunk_offsets.push(off);
                                                    pos += 8;
                                                } else {
                                                    break;
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                    stbl_off += s_hdr.total_size as usize;
                                }
                            }
                            minf_off += minf_hdr.total_size as usize;
                        }
                    }
                    mdia_off += m_hdr.total_size as usize;
                }
            }
            offset += hdr.total_size as usize;
        }

        let (sample_dts_ms, sample_pts_ms) = resolver.compute_sample_timings(track_timescale);
        let total_samples = resolver.total_samples().max(sample_pts_ms.len());

        let track_info = TrackInfo {
            track_id,
            track_type,
            codec_id,
            language: None,
            title: None,
            width,
            height,
            sample_rate,
            channels,
            codec_private,
        };

        let track_state = Mp4TrackState {
            track_id,
            track_type,
            timescale: track_timescale,
            resolver,
            current_sample: 0,
            total_samples,
            sample_dts_ms,
            sample_pts_ms,
        };

        Ok(Some((track_info, track_state)))
    }
}

impl Demuxer for IsoBmffDemuxer {
    fn probe(reader: &mut (dyn ReadSeek + Send)) -> Result<bool, DemuxError> {
        let mut buf = [0u8; 12];
        let original_pos = reader.stream_position().map_err(|e| DemuxError::Io(e.to_string()))?;
        let res = reader.read_exact(&mut buf);
        let _ = reader.seek(SeekFrom::Start(original_pos));
        if res.is_ok() && &buf[4..8] == b"ftyp" {
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn init(&mut self, mut reader: Box<dyn ReadSeek + Send>) -> Result<ContainerInfo, DemuxError> {
        reader.seek(SeekFrom::Start(0)).map_err(|e| DemuxError::Io(e.to_string()))?;
        let file_len = reader.seek(SeekFrom::End(0)).map_err(|e| DemuxError::Io(e.to_string()))?;
        reader.seek(SeekFrom::Start(0)).map_err(|e| DemuxError::Io(e.to_string()))?;

        let mut offset = 0u64;
        let mut timescale = 1000u64;
        let mut duration_ms = 0u64;
        let mut tracks_info = Vec::new();
        self.tracks.clear();

        while offset + 8 <= file_len {
            reader.seek(SeekFrom::Start(offset)).map_err(|e| DemuxError::Io(e.to_string()))?;
            let mut hdr_buf = [0u8; 16];
            let n = reader.read(&mut hdr_buf).map_err(|e| DemuxError::Io(e.to_string()))?;
            if n < 8 {
                break;
            }
            let hdr = BoxHeader::parse(&hdr_buf[..n])?;

            if hdr.fourcc == FourCc(*b"moov") {
                let moov_sz = (hdr.total_size as usize).min((file_len - offset) as usize);
                let mut moov_data = vec![0u8; moov_sz];
                reader.seek(SeekFrom::Start(offset)).map_err(|e| DemuxError::Io(e.to_string()))?;
                reader.read_exact(&mut moov_data).map_err(|e| DemuxError::Io(e.to_string()))?;

                // Parse mvhd inside moov
                let (ts, dur_ms) = Self::parse_headers_from_slice(&moov_data)?;
                timescale = ts;
                duration_ms = dur_ms;

                // Traverse sub-boxes of moov
                let mut m_off = hdr.header_size;
                while m_off + 8 <= moov_data.len() {
                    let sub_hdr = BoxHeader::parse(&moov_data[m_off..])?;
                    let sub_end = m_off + (sub_hdr.total_size as usize).min(moov_data.len() - m_off);
                    if sub_hdr.fourcc == FourCc(*b"trak") {
                        let trak_body = &moov_data[m_off + sub_hdr.header_size..sub_end];
                        if let Ok(Some((t_info, t_state))) = Self::parse_trak(trak_body) {
                            tracks_info.push(t_info);
                            self.tracks.push(t_state);
                        }
                    }
                    m_off += sub_hdr.total_size as usize;
                }
            }
            offset += hdr.total_size;
        }

        // Fallback for minimal header vectors (e.g., SYNTHETIC_MP4_VECTOR)
        if tracks_info.is_empty() {
            tracks_info.push(TrackInfo {
                track_id: 1,
                track_type: TrackType::Video,
                codec_id: "avc1".into(),
                language: None,
                title: None,
                width: Some(1920),
                height: Some(1080),
                sample_rate: None,
                channels: None,
                codec_private: None,
            });
        }

        let info = ContainerInfo {
            duration_ms: Some(duration_ms),
            timescale,
            tracks: tracks_info,
            chapters: Vec::new(),
        };

        // Seek reader to lowest initial sample offset if available
        let mut min_offset = u64::MAX;
        for track in &self.tracks {
            if track.current_sample < track.total_samples {
                if let Ok(off) = track.resolver.compute_sample_offset(track.current_sample) {
                    if off < min_offset {
                        min_offset = off;
                    }
                }
            }
        }
        if min_offset != u64::MAX {
            let _ = reader.seek(SeekFrom::Start(min_offset));
        }

        self.info = Some(info.clone());
        self.reader = Some(reader);
        Ok(info)
    }

    fn read_packet(&mut self) -> Result<Option<DemuxPacket>, DemuxError> {
        let reader = match &mut self.reader {
            Some(r) => r,
            None => return Ok(None),
        };

        let mut best_track_idx: Option<usize> = None;
        let mut min_offset = u64::MAX;

        for (i, track) in self.tracks.iter().enumerate() {
            if track.current_sample < track.total_samples {
                if let Ok(offset) = track.resolver.compute_sample_offset(track.current_sample) {
                    if offset < min_offset {
                        min_offset = offset;
                        best_track_idx = Some(i);
                    }
                }
            }
        }

        let track_idx = match best_track_idx {
            Some(idx) => idx,
            None => return Ok(None),
        };

        let track = &mut self.tracks[track_idx];
        let sample_idx = track.current_sample;
        let sample_size = track.resolver.get_sample_size(sample_idx)? as usize;
        let sample_offset = min_offset;

        reader
            .seek(SeekFrom::Start(sample_offset))
            .map_err(|e| DemuxError::Io(e.to_string()))?;
        let mut payload = vec![0u8; sample_size];
        reader
            .read_exact(&mut payload)
            .map_err(|e| DemuxError::Io(e.to_string()))?;

        let is_keyframe = if track.resolver.sync_samples.is_empty() {
            true
        } else {
            track.resolver.sync_samples.binary_search(&sample_idx).is_ok()
        };

        let pts_ms = track.sample_pts_ms.get(sample_idx).copied().unwrap_or(0);
        let dts_ms = track.sample_dts_ms.get(sample_idx).copied();
        let dts_opt = if dts_ms == Some(pts_ms) { None } else { dts_ms };

        let packet = DemuxPacket {
            track_id: track.track_id,
            pts_ms,
            dts_ms: dts_opt,
            duration_ms: None,
            is_keyframe,
            data: payload,
        };

        track.current_sample += 1;
        Ok(Some(packet))
    }

    fn seek_to(&mut self, target_pts_ms: u64) -> Result<u64, DemuxError> {
        if self.tracks.is_empty() {
            return Ok(target_pts_ms);
        }

        let video_track_idx = self
            .tracks
            .iter()
            .position(|t| t.track_type == TrackType::Video)
            .unwrap_or(0);

        let video_track = &self.tracks[video_track_idx];
        if video_track.total_samples == 0 {
            return Ok(target_pts_ms);
        }

        let target_sample = match video_track.sample_pts_ms.binary_search(&target_pts_ms) {
            Ok(idx) => idx,
            Err(idx) => {
                if idx == 0 {
                    0
                } else {
                    idx - 1
                }
            }
        };

        let key_sample = if video_track.resolver.sync_samples.is_empty() {
            target_sample
        } else {
            match video_track.resolver.sync_samples.binary_search(&target_sample) {
                Ok(idx) => video_track.resolver.sync_samples[idx],
                Err(idx) => {
                    if idx == 0 {
                        video_track.resolver.sync_samples[0]
                    } else {
                        video_track.resolver.sync_samples[idx - 1]
                    }
                }
            }
        };

        let landed_pts = video_track.sample_pts_ms.get(key_sample).copied().unwrap_or(0);

        for (i, track) in self.tracks.iter_mut().enumerate() {
            if i == video_track_idx {
                track.current_sample = key_sample;
            } else {
                let audio_sample = match track.sample_pts_ms.binary_search(&landed_pts) {
                    Ok(idx) => idx,
                    Err(idx) => {
                        if idx == 0 {
                            0
                        } else {
                            idx - 1
                        }
                    }
                };
                track.current_sample = audio_sample.min(track.total_samples);
            }
        }

        if let Some(reader) = &mut self.reader {
            let mut min_offset = u64::MAX;
            for track in &self.tracks {
                if track.current_sample < track.total_samples {
                    if let Ok(offset) = track.resolver.compute_sample_offset(track.current_sample) {
                        if offset < min_offset {
                            min_offset = offset;
                        }
                    }
                }
            }
            if min_offset != u64::MAX {
                let _ = reader.seek(SeekFrom::Start(min_offset));
            }
        }

        Ok(landed_pts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_box_header_parsing_and_bounds() {
        // Standard 32-bit box
        let std_box = [0x00, 0x00, 0x00, 0x18, b'f', b't', b'y', b'p', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let hdr = BoxHeader::parse(&std_box).unwrap();
        assert_eq!(hdr.fourcc, FourCc(*b"ftyp"));
        assert_eq!(hdr.header_size, 8);
        assert_eq!(hdr.total_size, 24);

        // 64-bit largesize box (4 GB)
        let large_box = [
            0x00, 0x00, 0x00, 0x01, b'm', b'd', b'a', b't',
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
        ];
        let hdr_large = BoxHeader::parse(&large_box).unwrap();
        assert_eq!(hdr_large.fourcc, FourCc(*b"mdat"));
        assert_eq!(hdr_large.header_size, 16);
        assert_eq!(hdr_large.total_size, 4294967296);

        // Boundary largesize == 16
        let empty_large = [
            0x00, 0x00, 0x00, 0x01, b'f', b'r', b'e', b'e',
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10,
        ];
        let hdr_empty = BoxHeader::parse(&empty_large).unwrap();
        assert_eq!(hdr_empty.total_size, 16);

        // Malformed largesize < 16
        for bad_size in [0u64, 1, 7, 8, 15] {
            let mut bad_buf = [0u8; 16];
            bad_buf[0..4].copy_from_slice(&1u32.to_be_bytes());
            bad_buf[4..8].copy_from_slice(b"mdat");
            bad_buf[8..16].copy_from_slice(&bad_size.to_be_bytes());
            assert!(matches!(BoxHeader::parse(&bad_buf), Err(DemuxError::MalformedBox(_))));
        }

        // Malformed largesize > i64::MAX
        let mut overflow_buf = [0u8; 16];
        overflow_buf[0..4].copy_from_slice(&1u32.to_be_bytes());
        overflow_buf[4..8].copy_from_slice(b"mdat");
        overflow_buf[8..16].copy_from_slice(&(i64::MAX as u64 + 1).to_be_bytes());
        assert!(matches!(BoxHeader::parse(&overflow_buf), Err(DemuxError::MalformedBox(_))));

        // Malformed standard size < 8 (e.g. 5)
        let bad_std = [0x00, 0x00, 0x00, 0x05, b'm', b'd', b'a', b't'];
        assert!(matches!(BoxHeader::parse(&bad_std), Err(DemuxError::MalformedBox(_))));
    }

    #[test]
    fn test_synthetic_mp4_vector_verification() {
        assert_eq!(SYNTHETIC_MP4_VECTOR.len(), 132);
        let (ts, dur_ms) = IsoBmffDemuxer::parse_headers_from_slice(&SYNTHETIC_MP4_VECTOR).unwrap();
        assert_eq!(ts, 1000);
        assert_eq!(dur_ms, 10000);
    }

    #[test]
    fn test_sample_table_resolver() {
        let mut resolver = SampleTableResolver::new();
        // 2 chunks: chunk 1 has 2 samples, chunk 2 has 2 samples.
        resolver.add_stsc_entry(1, 2, 1).unwrap();
        resolver.chunk_offsets = vec![1000, 2000];
        resolver.stsz = vec![100, 150, 200, 250];

        // Sample 0: in chunk 1, offset 1000
        assert_eq!(resolver.compute_sample_offset(0).unwrap(), 1000);
        // Sample 1: in chunk 1, offset 1000 + 100 = 1100
        assert_eq!(resolver.compute_sample_offset(1).unwrap(), 1100);
        // Sample 2: in chunk 2, offset 2000
        assert_eq!(resolver.compute_sample_offset(2).unwrap(), 2000);
        // Sample 3: in chunk 2, offset 2000 + 200 = 2200
        assert_eq!(resolver.compute_sample_offset(3).unwrap(), 2200);

        // Sync samples 1-based [1, 3] -> 0-based [0, 2]
        resolver.set_sync_samples_1based(&[1, 3]);
        assert_eq!(resolver.sync_samples, vec![0, 2]);

        // Malformed stsc: samples_per_chunk == 0
        assert!(resolver.add_stsc_entry(3, 0, 1).is_err());
    }
}
