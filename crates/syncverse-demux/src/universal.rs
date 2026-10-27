//! Universal container demuxer powered by FFmpeg bindings (`ffmpeg-next`).
//!
//! Provides container probing, metadata extraction, sequential packet demuxing,
//! and random-access keyframe seeking across all standard multimedia containers:
//! - Matroska (`.mkv`, `.webm`)
//! - ISOBMFF / QuickTime (`.mp4`, `.m4v`, `.mov`)
//! - AVI (`.avi`)
//! - MPEG Transport Stream (`.ts`)
//! - Flash Video (`.flv`)
//! - Ogg (`.ogg`, `.ogv`)

use std::io::SeekFrom;
use std::path::Path;
use ffmpeg_next as ffmpeg;

use crate::error::DemuxError;
use crate::traits::{ContainerInfo, DemuxPacket, Demuxer, ReadSeek, TrackInfo, TrackType};

/// Universal container demuxer delegating container demuxing and packet parsing
/// to system FFmpeg shared libraries via `ffmpeg-next`.
pub struct UniversalDemuxer {
    /// Underlying FFmpeg format input context.
    ictx: Option<ffmpeg::format::context::Input>,
    /// Container-level metadata and track inventory.
    info: Option<ContainerInfo>,
    /// Per-stream rational time bases indexed by stream index.
    time_bases: Vec<ffmpeg::Rational>,
    /// Buffered packet retained after a keyframe seek to determine landed PTS.
    peeked_packet: Option<DemuxPacket>,
    /// Container presentation start time offset in microseconds (AV_TIME_BASE units).
    start_time_us: i64,
}

impl Default for UniversalDemuxer {
    fn default() -> Self {
        Self::new()
    }
}

impl UniversalDemuxer {
    /// Constructs a new, uninitialized universal demuxer.
    pub fn new() -> Self {
        // Initialize FFmpeg library components once.
        let _ = ffmpeg::init();
        ffmpeg::util::log::set_level(ffmpeg::util::log::level::Level::Error);
        Self {
            ictx: None,
            info: None,
            time_bases: Vec::new(),
            peeked_packet: None,
            start_time_us: 0,
        }
    }

    /// Opens a media file directly from the filesystem by file path.
    pub fn open_file<P: AsRef<Path>>(path: P) -> Result<Self, DemuxError> {
        ffmpeg::init().map_err(|e| DemuxError::Io(format!("FFmpeg init failure: {}", e)))?;
        ffmpeg::util::log::set_level(ffmpeg::util::log::level::Level::Error);
        let ictx = ffmpeg::format::input(&path)
            .map_err(|e| DemuxError::Io(format!("FFmpeg input open failure: {}", e)))?;
        let mut demuxer = Self::new();
        demuxer.init_from_context(ictx)?;
        Ok(demuxer)
    }

    /// Internal helper initializing track state and time bases from an open `Input` context.
    fn init_from_context(
        &mut self,
        ictx: ffmpeg::format::context::Input,
    ) -> Result<ContainerInfo, DemuxError> {
        let mut tracks = Vec::new();
        let mut time_bases = Vec::new();

        for s in ictx.streams() {
            let s_idx = s.index();
            let tb = s.time_base();
            time_bases.push(tb);

            let params = s.parameters();
            let track_type = match params.medium() {
                ffmpeg::media::Type::Video => TrackType::Video,
                ffmpeg::media::Type::Audio => TrackType::Audio,
                ffmpeg::media::Type::Subtitle => TrackType::Subtitle,
                _ => continue,
            };

            let codec_id = params.id().name().to_string();
            let mut width = None;
            let mut height = None;
            let mut sample_rate = None;
            let mut channels = None;

            if let Ok(ctx) = ffmpeg::codec::context::Context::from_parameters(params.clone()) {
                if track_type == TrackType::Video {
                    if let Ok(video) = ctx.decoder().video() {
                        width = Some(video.width());
                        height = Some(video.height());
                    }
                } else if track_type == TrackType::Audio {
                    if let Ok(audio) = ctx.decoder().audio() {
                        sample_rate = Some(audio.rate() as f64);
                        channels = Some(audio.channels() as u32);
                    }
                }
            }

            // SAFETY: `params.as_ptr()` points to a valid AVCodecParameters owned by FFmpeg stream context.
            // If extradata is non-null and extradata_size > 0, the pointer addresses a valid byte slice of length extradata_size.
            let codec_private = unsafe {
                let raw = params.as_ptr();
                if !(*raw).extradata.is_null() && (*raw).extradata_size > 0 {
                    let slice = std::slice::from_raw_parts((*raw).extradata, (*raw).extradata_size as usize);
                    Some(slice.to_vec())
                } else {
                    None
                }
            };



            tracks.push(TrackInfo {
                track_id: s_idx as u64,
                track_type,
                codec_id,
                width,
                height,
                sample_rate,
                channels,
                codec_private,
            });
        }


        // Duration in AV_TIME_BASE units (microseconds).
        let duration_ms = if ictx.duration() > 0 {
            Some((ictx.duration() / 1000) as u64)
        } else {
            None
        };

        // SAFETY: `ictx.as_ptr()` returns a non-null pointer to an open AVFormatContext.
        // We read `start_time` and ensure it is positive and not AV_NOPTS_VALUE.
        let start_time_us = unsafe {
            let raw = ictx.as_ptr();
            let st = (*raw).start_time;
            if st != ffmpeg::ffi::AV_NOPTS_VALUE && st > 0 {
                st
            } else {
                let mut min_st = i64::MAX;
                for s in ictx.streams() {
                    let s_st = s.start_time();
                    if s_st != ffmpeg::ffi::AV_NOPTS_VALUE && s_st > 0 {
                        let tb = s.time_base();
                        let den = if tb.denominator() != 0 { tb.denominator() as i128 } else { 1 };
                        let us = (s_st as i128 * tb.numerator() as i128 * 1_000_000) / den;
                        if us > 0 && (us as i64) < min_st {
                            min_st = us as i64;
                        }
                    }
                }
                if min_st != i64::MAX {
                    min_st
                } else {
                    0
                }
            }
        };

        let info = ContainerInfo {
            duration_ms,
            timescale: 1_000_000,
            tracks,
        };

        self.time_bases = time_bases;
        self.info = Some(info.clone());
        self.ictx = Some(ictx);
        self.peeked_packet = None;
        self.start_time_us = start_time_us;

        Ok(info)
    }

    /// Returns the presentation start time offset in microseconds (AV_TIME_BASE units).
    pub fn start_time_us(&self) -> i64 {
        self.start_time_us
    }

    /// Normalizes raw packet timestamps against container start time to yield 0-based millisecond timestamps.
    fn normalize_packet(&self, packet: &ffmpeg::Packet) -> DemuxPacket {
        let s_idx = packet.stream();
        let tb = if s_idx < self.time_bases.len() {
            self.time_bases[s_idx]
        } else {
            ffmpeg::Rational(1, 1000)
        };

        let num = tb.numerator() as i128;
        let den = if tb.denominator() != 0 {
            tb.denominator() as i128
        } else {
            1
        };

        let raw_pts = packet.pts().or_else(|| packet.dts()).unwrap_or(0);
        let pts_us = (raw_pts as i128 * num * 1_000_000) / den;
        let norm_pts_us = (pts_us - self.start_time_us as i128).max(0);
        let pts_ms = (norm_pts_us / 1000) as u64;

        let dts_ms = packet.dts().map(|dts| {
            let dts_us = (dts as i128 * num * 1_000_000) / den;
            let norm_dts_us = (dts_us - self.start_time_us as i128).max(0);
            (norm_dts_us / 1000) as u64
        });

        let is_keyframe = packet.is_key();
        let data = packet.data().unwrap_or(&[]).to_vec();

        DemuxPacket {
            track_id: s_idx as u64,
            pts_ms,
            dts_ms,
            is_keyframe,
            data,
        }
    }

    /// Returns a reference to the active container metadata, if initialized.
    pub fn container_info(&self) -> Option<&ContainerInfo> {
        self.info.as_ref()
    }

    /// Returns the FFmpeg codec parameters for a given stream index, if available.
    pub fn stream_parameters(&self, stream_index: usize) -> Option<ffmpeg::codec::Parameters> {
        self.ictx.as_ref().and_then(|ctx| {
            ctx.streams().find(|s| s.index() == stream_index).map(|s| s.parameters())
        })
    }
}


impl Demuxer for UniversalDemuxer {
    fn probe(reader: &mut (dyn ReadSeek + Send)) -> Result<bool, DemuxError> {
        let start_pos = reader
            .stream_position()
            .map_err(|e| DemuxError::Io(e.to_string()))?;
        let mut buf = vec![0u8; 4096 + 64];
        let n = reader
            .read(&mut buf[..4096])
            .map_err(|e| DemuxError::Io(e.to_string()))?;
        reader
            .seek(SeekFrom::Start(start_pos))
            .map_err(|e| DemuxError::Io(e.to_string()))?;

        if n < 4 {
            return Ok(false);
        }

        // Ensure buffer after `n` bytes contains at least AVPROBE_PADDING_SIZE (32 bytes) of zeroes
        buf[n..n + 64].fill(0);

        // 1. FFmpeg C probe via av_probe_input_format
        let probe_data = ffmpeg::ffi::AVProbeData {
            filename: std::ptr::null(),
            buf: buf.as_mut_ptr(),
            buf_size: n as std::ffi::c_int,
            mime_type: std::ptr::null(),
        };

        // SAFETY: `buf` is a valid heap-allocated byte buffer of length at least `n + 64`,
        // guaranteed to have at least `AVPROBE_PADDING_SIZE` (32 bytes) of zero-padding
        // following the `n` valid bytes indicated by `buf_size`. `av_probe_input_format`
        // inspects the buffer to determine format and does not retain or free the pointer.
        let fmt = unsafe { ffmpeg::ffi::av_probe_input_format(&probe_data, 1) };
        if !fmt.is_null() {
            return Ok(true);
        }

        // 2. Container magic fallback heuristic
        let b = &buf[..n];
        if b.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
            return Ok(true); // MKV/WebM
        }
        if b.len() >= 8
            && matches!(
                &b[4..8],
                b"ftyp" | b"moov" | b"mdat" | b"wide" | b"free" | b"skip" | b"qt  "
            )
        {
            return Ok(true); // MP4/MOV
        }
        if b.len() >= 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"AVI " {
            return Ok(true); // AVI
        }
        if b.len() >= 188 && b[0] == 0x47 {
            return Ok(true); // MPEG-TS
        }
        if b.len() >= 3 && &b[0..3] == b"FLV" {
            return Ok(true); // FLV
        }
        if b.len() >= 4 && &b[0..4] == b"OggS" {
            return Ok(true); // OGG
        }

        Ok(false)
    }

    fn init(&mut self, reader: Box<dyn ReadSeek + Send>) -> Result<ContainerInfo, DemuxError> {
        ffmpeg::init().map_err(|e| DemuxError::Io(format!("FFmpeg init failure: {}", e)))?;
        ffmpeg::util::log::set_level(ffmpeg::util::log::level::Level::Error);
        let stream_io = ffmpeg::format::context::StreamIo::from_read_seek(reader)
            .map_err(|e| DemuxError::Io(format!("StreamIo creation failure: {}", e)))?;
        let ictx = ffmpeg::format::input_from_stream(stream_io, None, None)
            .map_err(|e| DemuxError::Io(format!("FFmpeg input_from_stream failure: {}", e)))?;
        self.init_from_context(ictx)
    }

    fn read_packet(&mut self) -> Result<Option<DemuxPacket>, DemuxError> {
        if let Some(packet) = self.peeked_packet.take() {
            return Ok(Some(packet));
        }

        let ictx = match self.ictx.as_mut() {
            Some(ctx) => ctx,
            None => {
                return Err(DemuxError::InvalidHeader(
                    "UniversalDemuxer not initialized".into(),
                ));
            }
        };

        let mut packet = ffmpeg::Packet::empty();
        loop {
            match packet.read(ictx) {
                Ok(()) => {
                    return Ok(Some(self.normalize_packet(&packet)));
                }
                Err(ffmpeg::Error::Eof) => return Ok(None),
                Err(ffmpeg::Error::InvalidData) => {
                    // Skip corrupt packet and attempt to re-sync
                    continue;
                }
                Err(e) => {
                    return Err(DemuxError::Io(format!("FFmpeg demuxing error: {}", e)));
                }
            }
        }
    }

    fn seek_to(&mut self, target_pts_ms: u64) -> Result<u64, DemuxError> {
        self.peeked_packet = None;

        let ictx = match self.ictx.as_mut() {
            Some(ctx) => ctx,
            None => {
                return Err(DemuxError::InvalidHeader(
                    "UniversalDemuxer not initialized".into(),
                ));
            }
        };

        // Convert target_pts_ms to microseconds (AV_TIME_BASE = 1_000_000) and offset by start_time_us
        let stream_target_us = (target_pts_ms as i64 * 1000) + self.start_time_us;

        // 1. Attempt seeking backwards to nearest keyframe at or before target
        if ictx.seek(stream_target_us, ..stream_target_us).is_err() {
            // Fallback: if seeking backwards fails (e.g. target is before first keyframe,
            // such as FLV priming delay or target_pts_ms == 0), seek forward to earliest available keyframe.
            ictx.seek(stream_target_us, stream_target_us..)
                .or_else(|_| ictx.seek(self.start_time_us, self.start_time_us..))
                .or_else(|_| ictx.seek(0, 0..))
                .or_else(|_| ictx.seek(stream_target_us, ..))
                .map_err(|e| DemuxError::Io(format!("FFmpeg seek failure: {e}")))?;
        }

        // Read the first packet after seek to establish the exact landed PTS and buffer it
        let mut packet = ffmpeg::Packet::empty();
        match packet.read(ictx) {
            Ok(()) => {
                let demux_packet = self.normalize_packet(&packet);
                let landed_pts = demux_packet.pts_ms;
                self.peeked_packet = Some(demux_packet);
                Ok(landed_pts)
            }
            Err(ffmpeg::Error::Eof) => Ok(target_pts_ms),
            Err(e) => Err(DemuxError::Io(format!("FFmpeg read post-seek error: {e}"))),
        }
    }
}
