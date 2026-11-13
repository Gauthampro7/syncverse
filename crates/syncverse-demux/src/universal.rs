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
    /// Per-stream detected frame rates indexed by stream index.
    frame_rates: Vec<Option<f64>>,
    /// Buffered packet retained after a keyframe seek to determine landed PTS.
    peeked_packet: Option<DemuxPacket>,
    /// Container presentation start time offset in microseconds (AV_TIME_BASE units).
    start_time_us: i64,
    /// Preferred stream index for keyframe-targeted seeking (first active video, or audio fallback).
    preferred_seek_stream_index: Option<usize>,
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
            frame_rates: Vec::new(),
            peeked_packet: None,
            start_time_us: 0,
            preferred_seek_stream_index: None,
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
        mut ictx: ffmpeg::format::context::Input,
    ) -> Result<ContainerInfo, DemuxError> {
        let mut tracks = Vec::new();
        let mut time_bases = Vec::new();
        let mut frame_rates = Vec::new();
        let mut discard_indices = Vec::new();

        for s in ictx.streams() {
            let s_idx = s.index();
            let tb = s.time_base();
            time_bases.push(tb);

            let r = s.avg_frame_rate();
            let fps = if r.denominator() > 0 && r.numerator() > 0 {
                Some(r.numerator() as f64 / r.denominator() as f64)
            } else {
                let r2 = s.rate();
                if r2.denominator() > 0 && r2.numerator() > 0 {
                    Some(r2.numerator() as f64 / r2.denominator() as f64)
                } else {
                    None
                }
            };
            frame_rates.push(fps);

            let disp = s.disposition();
            let is_attached_pic = disp.contains(ffmpeg::format::stream::Disposition::ATTACHED_PIC);
            if is_attached_pic {
                discard_indices.push(s_idx);
                continue;
            }

            let is_default = disp.contains(ffmpeg::format::stream::Disposition::DEFAULT);
            let is_forced = disp.contains(ffmpeg::format::stream::Disposition::FORCED);

            let params = s.parameters();
            let track_type = match params.medium() {
                ffmpeg::media::Type::Video => TrackType::Video,
                ffmpeg::media::Type::Audio => TrackType::Audio,
                ffmpeg::media::Type::Subtitle => TrackType::Subtitle,
                _ => {
                    discard_indices.push(s_idx);
                    continue;
                }
            };

            let codec_id = params.id().name().to_string();

            // SAFETY: `params.as_ptr()` points to a valid `AVCodecParameters` owned by the FFmpeg
            // stream context for the duration of `s`. Reading scalar fields (`width`, `height`,
            // `sample_rate`, `ch_layout.nb_channels`, `extradata`, `extradata_size`) is valid,
            // and `extradata` is only sliced when non-null and `extradata_size > 0`.
            let (width, height, sample_rate, channels, codec_private) = unsafe {
                let raw = params.as_ptr();
                let w = if track_type == TrackType::Video && (*raw).width > 0 {
                    Some((*raw).width as u32)
                } else {
                    None
                };
                let h = if track_type == TrackType::Video && (*raw).height > 0 {
                    Some((*raw).height as u32)
                } else {
                    None
                };
                let sr = if track_type == TrackType::Audio && (*raw).sample_rate > 0 {
                    Some((*raw).sample_rate as f64)
                } else {
                    None
                };
                let ch = if track_type == TrackType::Audio && (*raw).ch_layout.nb_channels > 0 {
                    Some((*raw).ch_layout.nb_channels as u32)
                } else {
                    None
                };
                let extra = if !(*raw).extradata.is_null() && (*raw).extradata_size > 0 {
                    let slice = std::slice::from_raw_parts(
                        (*raw).extradata,
                        (*raw).extradata_size as usize,
                    );
                    Some(slice.to_vec())
                } else {
                    None
                };
                (w, h, sr, ch, extra)
            };

            let mut language = None;
            let mut title = None;
            let metadata = s.metadata();
            if let Some(lang) = metadata.get("language").or_else(|| metadata.get("lang")) {
                let trimmed = lang.trim();
                if !trimmed.is_empty() {
                    language = Some(trimmed.to_string());
                }
            }
            if let Some(t) = metadata.get("title").or_else(|| metadata.get("handler_name")) {
                let trimmed = t.trim();
                if !trimmed.is_empty() {
                    title = Some(trimmed.to_string());
                }
            }

            tracks.push(TrackInfo {
                track_id: s_idx as u64,
                track_type,
                codec_id,
                language,
                title,
                width,
                height,
                sample_rate,
                channels,
                codec_private,
                is_default,
                is_forced,
            });
        }

        if !discard_indices.is_empty() {
            // SAFETY: `ictx.as_mut_ptr()` returns a valid, non-null `AVFormatContext` pointer.
            // Every index in `discard_indices` came from `ictx.streams()`, so it is strictly less
            // than `(*raw_ctx).nb_streams as usize` and points to a valid `AVStream`.
            unsafe {
                let raw_ctx = ictx.as_mut_ptr();
                let nb_streams = (*raw_ctx).nb_streams as usize;
                for idx in discard_indices {
                    if idx < nb_streams {
                        let stream_ptr = *(*raw_ctx).streams.add(idx);
                        if !stream_ptr.is_null() {
                            (*stream_ptr).discard = ffmpeg::ffi::AVDiscard::AVDISCARD_ALL;
                        }
                    }
                }
            }
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

        let mut chapters = Vec::new();
        for ch in ictx.chapters() {
            let id = ch.id();
            let tb = ch.time_base();
            let num = tb.numerator() as i128;
            let den = if tb.denominator() != 0 {
                tb.denominator() as i128
            } else {
                1
            };
            let start_ms = ((ch.start() as i128 * num * 1000) / den).clamp(0, u64::MAX as i128) as u64;
            let end_ms = ((ch.end() as i128 * num * 1000) / den).clamp(0, u64::MAX as i128) as u64;
            let title = ch
                .metadata()
                .get("title")
                .map(String::from)
                .unwrap_or_else(|| format!("Chapter {}", id));
            chapters.push(crate::traits::MediaChapter {
                id,
                title,
                start_ms,
                end_ms,
            });
        }

        let preferred_seek_stream_index = tracks
            .iter()
            .find(|t| t.track_type == TrackType::Video)
            .or_else(|| tracks.iter().find(|t| t.track_type == TrackType::Audio))
            .map(|t| t.track_id as usize);

        let info = ContainerInfo {
            duration_ms,
            timescale: 1_000_000,
            tracks,
            chapters,
        };

        self.time_bases = time_bases;
        self.frame_rates = frame_rates;
        self.info = Some(info.clone());
        self.ictx = Some(ictx);
        self.peeked_packet = None;
        self.start_time_us = start_time_us;
        self.preferred_seek_stream_index = preferred_seek_stream_index;

        Ok(info)
    }

    /// Returns the presentation start time offset in microseconds (AV_TIME_BASE units).
    pub fn start_time_us(&self) -> i64 {
        self.start_time_us
    }

    /// Configures which streams remain active during demuxing and discards all others at the
    /// container demuxer level (`AVDISCARD_ALL`).
    ///
    /// - Attached-picture, data, attachment, and unknown streams are always discarded.
    /// - Video streams are kept only if their stream index equals `video_id`.
    /// - Audio streams are kept only if their stream index equals `audio_id`.
    /// - Subtitle streams are kept only if `keep_subtitles` is `true`.
    ///
    /// Any buffered post-seek packet belonging to a newly discarded stream is dropped, and
    /// `preferred_seek_stream_index` is updated to `video_id.or(audio_id)`.
    pub fn set_active_streams(
        &mut self,
        video_id: Option<u64>,
        audio_id: Option<u64>,
        keep_subtitles: bool,
    ) {
        let Some(ictx) = self.ictx.as_mut() else {
            return;
        };

        let mut discarded_tracks = Vec::new();

        // SAFETY: `ictx.as_mut_ptr()` returns a valid, non-null `AVFormatContext` owned by `self.ictx`.
        // `nb_streams` bounds the `streams` pointer array, and each non-null entry points to a valid
        // `AVStream` owned by `ictx`.
        unsafe {
            let raw_ctx = ictx.as_mut_ptr();
            let nb_streams = (*raw_ctx).nb_streams as usize;
            for i in 0..nb_streams {
                let stream_ptr = *(*raw_ctx).streams.add(i);
                if stream_ptr.is_null() {
                    continue;
                }
                let stream_id = (*stream_ptr).index as u64;
                let is_attached_pic =
                    ((*stream_ptr).disposition & ffmpeg::ffi::AV_DISPOSITION_ATTACHED_PIC) != 0;
                let codecpar = (*stream_ptr).codecpar;
                let media_type = if !codecpar.is_null() {
                    ffmpeg::media::Type::from((*codecpar).codec_type)
                } else {
                    ffmpeg::media::Type::Unknown
                };

                let discard = if is_attached_pic {
                    ffmpeg::ffi::AVDiscard::AVDISCARD_ALL
                } else {
                    match media_type {
                        ffmpeg::media::Type::Video => {
                            if Some(stream_id) == video_id {
                                ffmpeg::ffi::AVDiscard::AVDISCARD_DEFAULT
                            } else {
                                ffmpeg::ffi::AVDiscard::AVDISCARD_ALL
                            }
                        }
                        ffmpeg::media::Type::Audio => {
                            if Some(stream_id) == audio_id {
                                ffmpeg::ffi::AVDiscard::AVDISCARD_DEFAULT
                            } else {
                                ffmpeg::ffi::AVDiscard::AVDISCARD_ALL
                            }
                        }
                        ffmpeg::media::Type::Subtitle => {
                            if keep_subtitles {
                                ffmpeg::ffi::AVDiscard::AVDISCARD_DEFAULT
                            } else {
                                ffmpeg::ffi::AVDiscard::AVDISCARD_ALL
                            }
                        }
                        _ => ffmpeg::ffi::AVDiscard::AVDISCARD_ALL,
                    }
                };

                (*stream_ptr).discard = discard;
                if discard == ffmpeg::ffi::AVDiscard::AVDISCARD_ALL {
                    discarded_tracks.push(stream_id);
                }
            }
        }

        if let Some(ref pkt) = self.peeked_packet {
            if discarded_tracks.contains(&pkt.track_id) {
                self.peeked_packet = None;
            }
        }

        self.preferred_seek_stream_index = video_id.or(audio_id).map(|id| id as usize);
    }

    /// Returns `true` if the given stream index currently has `AVDISCARD_ALL` set.
    fn is_stream_discarded(&self, stream_idx: usize) -> bool {
        let Some(ictx) = self.ictx.as_ref() else {
            return false;
        };
        // SAFETY: `ictx.as_ptr()` returns a valid, non-null `AVFormatContext` owned by `self.ictx`.
        // We bounds-check `stream_idx < (*raw).nb_streams as usize` before dereferencing `streams[stream_idx]`.
        unsafe {
            let raw = ictx.as_ptr();
            if stream_idx >= (*raw).nb_streams as usize {
                return false;
            }
            let stream_ptr = *(*raw).streams.add(stream_idx);
            if stream_ptr.is_null() {
                return false;
            }
            (*stream_ptr).discard == ffmpeg::ffi::AVDiscard::AVDISCARD_ALL
        }
    }

    /// Normalizes raw packet timestamps against container start time to yield 0-based timestamps.
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
        let raw_pts_us = (raw_pts as i128 * num * 1_000_000) / den;
        let norm_pts_us = (raw_pts_us - self.start_time_us as i128).max(0);
        let pts_us = norm_pts_us.min(i64::MAX as i128) as i64;
        let pts_ms = (norm_pts_us as u64) / 1000;

        let dts_ms = packet.dts().map(|dts| {
            let dts_us = (dts as i128 * num * 1_000_000) / den;
            let norm_dts_us = (dts_us - self.start_time_us as i128).max(0);
            (norm_dts_us / 1000) as u64
        });

        let duration_ms = if packet.duration() > 0 {
            let dur_us = (packet.duration() as i128 * num * 1_000_000) / den;
            Some((dur_us / 1000) as u64)
        } else {
            None
        };

        let is_keyframe = packet.is_key();
        let data = packet.data().unwrap_or(&[]).to_vec();

        DemuxPacket {
            track_id: s_idx as u64,
            pts_ms,
            pts_us,
            dts_ms,
            duration_ms,
            is_keyframe,
            data,
        }
    }

    /// Returns a reference to the active container metadata, if initialized.
    pub fn container_info(&self) -> Option<&ContainerInfo> {
        self.info.as_ref()
    }

    /// Returns the slice of chapters declared in this media container.
    pub fn chapters(&self) -> &[crate::traits::MediaChapter] {
        self.info
            .as_ref()
            .map(|i| i.chapters.as_slice())
            .unwrap_or(&[])
    }

    /// Returns the FFmpeg codec parameters and rational time base for a given track ID, if available.
    pub fn stream_params(
        &self,
        track_id: u64,
    ) -> Option<(ffmpeg::codec::Parameters, ffmpeg::Rational)> {
        self.ictx.as_ref().and_then(|ctx| {
            ctx.streams()
                .find(|s| s.index() as u64 == track_id)
                .map(|s| (s.parameters(), s.time_base()))
        })
    }

    /// Returns the FFmpeg codec parameters and rational time base for a subtitle track ID, if available.
    pub fn subtitle_stream_params(
        &self,
        track_id: u64,
    ) -> Option<(ffmpeg::codec::Parameters, ffmpeg::Rational)> {
        self.ictx.as_ref().and_then(|ctx| {
            ctx.streams()
                .find(|s| {
                    s.index() as u64 == track_id
                        && s.parameters().medium() == ffmpeg::media::Type::Subtitle
                })
                .map(|s| (s.parameters(), s.time_base()))
        })
    }

    /// Returns the FFmpeg codec parameters for a given stream index, if available.
    pub fn stream_parameters(&self, stream_index: usize) -> Option<ffmpeg::codec::Parameters> {
        self.ictx.as_ref().and_then(|ctx| {
            ctx.streams().find(|s| s.index() == stream_index).map(|s| s.parameters())
        })
    }

    /// Returns the rational time base (numerator, denominator) for a given stream index, if available.
    pub fn stream_time_base(&self, stream_index: usize) -> Option<(i32, i32)> {
        self.time_bases.get(stream_index).map(|tb| (tb.numerator(), tb.denominator()))
    }

    /// Returns the detected frame rate (FPS) for a given stream index, if available.
    pub fn stream_frame_rate(&self, stream_index: usize) -> Option<f64> {
        self.frame_rates.get(stream_index).copied().flatten()
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

        if self.ictx.is_none() {
            return Err(DemuxError::InvalidHeader(
                "UniversalDemuxer not initialized".into(),
            ));
        }

        let mut packet = ffmpeg::Packet::empty();
        loop {
            let read_res = {
                let ictx = self.ictx.as_mut().expect("ictx checked above");
                packet.read(ictx)
            };
            match read_res {
                Ok(()) => {
                    if self.is_stream_discarded(packet.stream()) {
                        continue;
                    }
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

        let target_us = self
            .start_time_us
            .saturating_add((target_pts_ms as i64).saturating_mul(1000));

        let preferred_stream = self
            .preferred_seek_stream_index
            .and_then(|idx| self.time_bases.get(idx).copied().map(|tb| (idx, tb)))
            .filter(|(_, tb)| tb.numerator() > 0 && tb.denominator() > 0);

        {
            let ictx = match self.ictx.as_mut() {
                Some(ctx) => ctx,
                None => {
                    return Err(DemuxError::InvalidHeader(
                        "UniversalDemuxer not initialized".into(),
                    ));
                }
            };
            ictx.clear_interrupt();

            let av_time_base_q = ffmpeg::ffi::AVRational {
                num: 1,
                den: ffmpeg::ffi::AV_TIME_BASE,
            };

            let (stream_index, stream_ts) = if let Some((idx, tb)) = preferred_stream {
                // SAFETY: `av_rescale_q` is a pure integer rational scaling function with no pointer arguments.
                let ts = unsafe { ffmpeg::ffi::av_rescale_q(target_us, av_time_base_q, tb.into()) };
                (idx as std::ffi::c_int, ts)
            } else {
                (-1 as std::ffi::c_int, target_us)
            };

            // SAFETY: `ictx.as_mut_ptr()` returns a valid, non-null `AVFormatContext` pointer owned by `self.ictx`.
            // `stream_index` is either `-1` (`AV_TIME_BASE`) or a validated stream index from `self.time_bases`.
            let seek_ret = unsafe {
                let raw_ctx = ictx.as_mut_ptr();
                let mut ret = ffmpeg::ffi::avformat_seek_file(
                    raw_ctx,
                    stream_index,
                    i64::MIN,
                    stream_ts,
                    stream_ts,
                    ffmpeg::ffi::AVSEEK_FLAG_BACKWARD as std::ffi::c_int,
                );
                if ret < 0 {
                    // Fallback: target may precede first keyframe (e.g. priming delay or target == 0).
                    ret = ffmpeg::ffi::avformat_seek_file(
                        raw_ctx,
                        stream_index,
                        stream_ts,
                        stream_ts,
                        i64::MAX,
                        0,
                    );
                }
                if ret < 0 && stream_index != -1 {
                    ret = ffmpeg::ffi::avformat_seek_file(
                        raw_ctx,
                        -1,
                        i64::MIN,
                        target_us,
                        target_us,
                        ffmpeg::ffi::AVSEEK_FLAG_BACKWARD as std::ffi::c_int,
                    );
                    if ret < 0 {
                        ret = ffmpeg::ffi::avformat_seek_file(
                            raw_ctx,
                            -1,
                            target_us,
                            target_us,
                            i64::MAX,
                            0,
                        );
                    }
                }
                if ret < 0 {
                    ret = ffmpeg::ffi::avformat_seek_file(
                        raw_ctx,
                        -1,
                        self.start_time_us,
                        self.start_time_us,
                        i64::MAX,
                        0,
                    );
                }
                if ret < 0 {
                    ret = ffmpeg::ffi::avformat_seek_file(raw_ctx, -1, 0, 0, i64::MAX, 0);
                }
                if ret < 0 {
                    ret = ffmpeg::ffi::avformat_seek_file(
                        raw_ctx,
                        -1,
                        i64::MIN,
                        target_us,
                        i64::MAX,
                        0,
                    );
                }
                ret
            };

            if seek_ret < 0 {
                return Err(DemuxError::Io(format!(
                    "FFmpeg seek failure: {}",
                    ffmpeg::Error::from(seek_ret)
                )));
            }
        }

        // Read the first active packet after seek to establish the exact landed PTS and buffer it
        // in `self.peeked_packet` so the post-seek keyframe is never dropped.
        let mut packet = ffmpeg::Packet::empty();
        loop {
            let read_res = {
                let ictx = self.ictx.as_mut().expect("ictx checked above");
                packet.read(ictx)
            };
            match read_res {
                Ok(()) => {
                    if self.is_stream_discarded(packet.stream()) {
                        continue;
                    }
                    let demux_packet = self.normalize_packet(&packet);
                    let landed_pts = demux_packet.pts_ms;
                    self.peeked_packet = Some(demux_packet);
                    return Ok(landed_pts);
                }
                Err(ffmpeg::Error::InvalidData) => continue,
                Err(ffmpeg::Error::Eof) => return Ok(target_pts_ms),
                Err(e) => return Err(DemuxError::Io(format!("FFmpeg read post-seek error: {e}"))),
            }
        }
    }
}
