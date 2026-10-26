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
        Self {
            ictx: None,
            info: None,
            time_bases: Vec::new(),
            peeked_packet: None,
        }
    }

    /// Opens a media file directly from the filesystem by file path.
    pub fn open_file<P: AsRef<Path>>(path: P) -> Result<Self, DemuxError> {
        ffmpeg::init().map_err(|e| DemuxError::Io(format!("FFmpeg init failure: {}", e)))?;
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

            if let Ok(ctx) = ffmpeg::codec::context::Context::from_parameters(params) {
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

            tracks.push(TrackInfo {
                track_id: s_idx as u64,
                track_type,
                codec_id,
                width,
                height,
                sample_rate,
                channels,
                codec_private: None,
            });
        }

        // Duration in AV_TIME_BASE units (microseconds).
        let duration_ms = if ictx.duration() > 0 {
            Some((ictx.duration() / 1000) as u64)
        } else {
            None
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

        Ok(info)
    }

    /// Returns a reference to the active container metadata, if initialized.
    pub fn container_info(&self) -> Option<&ContainerInfo> {
        self.info.as_ref()
    }
}

impl Demuxer for UniversalDemuxer {
    fn probe(reader: &mut (dyn ReadSeek + Send)) -> Result<bool, DemuxError> {
        let start_pos = reader
            .stream_position()
            .map_err(|e| DemuxError::Io(e.to_string()))?;
        let mut buf = vec![0u8; 4096];
        let n = reader
            .read(&mut buf)
            .map_err(|e| DemuxError::Io(e.to_string()))?;
        reader
            .seek(SeekFrom::Start(start_pos))
            .map_err(|e| DemuxError::Io(e.to_string()))?;

        if n < 4 {
            return Ok(false);
        }

        // 1. FFmpeg C probe via av_probe_input_format
        let probe_data = ffmpeg::ffi::AVProbeData {
            filename: std::ptr::null(),
            buf: buf.as_mut_ptr(),
            buf_size: n as std::ffi::c_int,
            mime_type: std::ptr::null(),
        };

        // SAFETY: `buf` is a valid heap-allocated byte buffer of length `n`, and `buf_size`
        // accurately specifies the number of available bytes. `av_probe_input_format`
        // only inspects the first `buf_size` bytes to determine the container format
        // and does not write to or retain the pointer.
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
                    let pts_ms = if raw_pts <= 0 {
                        0u64
                    } else {
                        let scaled = (raw_pts as i128 * num * 1000) / den;
                        if scaled < 0 { 0 } else { scaled as u64 }
                    };

                    let dts_ms = packet.dts().map(|dts| {
                        if dts <= 0 {
                            0u64
                        } else {
                            let scaled = (dts as i128 * num * 1000) / den;
                            if scaled < 0 { 0 } else { scaled as u64 }
                        }
                    });

                    let is_keyframe = packet.is_key();
                    let data = packet.data().unwrap_or(&[]).to_vec();

                    return Ok(Some(DemuxPacket {
                        track_id: s_idx as u64,
                        pts_ms,
                        dts_ms,
                        is_keyframe,
                        data,
                    }));
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

        // Convert target_pts_ms to microseconds (AV_TIME_BASE = 1_000_000)
        let target_us = (target_pts_ms as i64) * 1000;

        // Seek backwards to the nearest keyframe preceding or at target_us
        ictx.seek(target_us, ..target_us)
            .map_err(|e| DemuxError::Io(format!("FFmpeg seek failure: {}", e)))?;

        // Read the first packet after seek to establish the exact landed PTS and buffer it
        let mut packet = ffmpeg::Packet::empty();
        match packet.read(ictx) {
            Ok(()) => {
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
                let pts_ms = if raw_pts <= 0 {
                    0u64
                } else {
                    let scaled = (raw_pts as i128 * num * 1000) / den;
                    if scaled < 0 { 0 } else { scaled as u64 }
                };

                let dts_ms = packet.dts().map(|dts| {
                    if dts <= 0 {
                        0u64
                    } else {
                        let scaled = (dts as i128 * num * 1000) / den;
                        if scaled < 0 { 0 } else { scaled as u64 }
                    }
                });

                let is_keyframe = packet.is_key();
                let data = packet.data().unwrap_or(&[]).to_vec();

                let landed_pts = pts_ms;
                self.peeked_packet = Some(DemuxPacket {
                    track_id: s_idx as u64,
                    pts_ms,
                    dts_ms,
                    is_keyframe,
                    data,
                });

                Ok(landed_pts)
            }
            Err(ffmpeg::Error::Eof) => Ok(target_pts_ms),
            Err(e) => Err(DemuxError::Io(format!("FFmpeg read post-seek error: {}", e))),
        }
    }
}
