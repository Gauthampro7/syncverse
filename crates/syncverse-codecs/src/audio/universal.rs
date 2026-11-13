//! Universal Audio Decoder for `syncverse-codecs`.
//!
//! Powered by `ffmpeg-next` (`libavcodec` and `libswresample`).
//! Decodes all standard audio codecs: AAC, MP3, Opus, Vorbis, FLAC, AC3, E-AC3, DTS, and PCM.
//! Normalizes all output to 48,000 Hz, 2 channels (stereo), interleaved 32-bit float samples in `[-1.0, 1.0]`.

use ffmpeg_next::codec::{decoder, Context as CodecContext, Id, Parameters};
use ffmpeg_next::format::sample::Type;
use ffmpeg_next::format::Sample;
use ffmpeg_next::frame::Audio as AudioFrame;
use ffmpeg_next::software::resampling::Context as ResamplerContext;
use ffmpeg_next::{ChannelLayout, Error as FfmpegError, Packet, Rational};

use super::soft_clip_sample;
use crate::error::CodecError;
use crate::frame::AudioFramePCM;

/// Target audio sampling rate across the entire Syncverse presentation pipeline (48 kHz).
pub const TARGET_AUDIO_SAMPLE_RATE: u32 = 48_000;

/// Target audio channel count (Stereo = 2).
pub const TARGET_AUDIO_CHANNELS: u16 = 2;

/// Strongly typed operational errors for universal audio decoding.
#[derive(Debug, thiserror::Error)]
pub enum UniversalAudioError {
    /// Underlying FFmpeg library error.
    #[error("FFmpeg error: {0}")]
    Ffmpeg(#[from] FfmpegError),

    /// Decoder could not be located for the specified codec.
    #[error("Decoder not found for codec: {0}")]
    DecoderNotFound(String),

    /// Codec frame error or invalid parameters.
    #[error("Invalid audio frame: {0}")]
    Frame(#[from] CodecError),

    /// Invalid channel layout or sample rate.
    #[error("Invalid audio configuration: {0}")]
    InvalidConfig(String),
}

impl From<UniversalAudioError> for CodecError {
    fn from(err: UniversalAudioError) -> Self {
        CodecError::Decoder(err.to_string())
    }
}

/// Helper extracting or computing the effective channel layout from a frame.
#[inline]
fn effective_layout(frame: &AudioFrame) -> ChannelLayout {
    let layout = frame.channel_layout();
    if layout.is_empty() || layout.channels() == 0 {
        ChannelLayout::default(frame.channels() as i32)
    } else {
        layout
    }
}

/// Universal audio decoder supporting AAC, MP3, Opus, Vorbis, FLAC, AC3, E-AC3, DTS, and uncompressed PCM.
pub struct UniversalAudioDecoder {
    decoder: ffmpeg_next::decoder::Audio,
    resampler: Option<ResamplerContext>,
    time_base: Option<Rational>,
    anchored: bool,
    base_pts_us: i64,
    samples_since_anchor: u64,
    last_in_format: Option<Sample>,
    last_in_layout: Option<ChannelLayout>,
    last_in_rate: Option<u32>,
}

impl UniversalAudioDecoder {
    /// Constructs a `UniversalAudioDecoder` from demuxer track parameters.
    pub fn from_parameters(
        params: Parameters,
        time_base: Option<(i32, i32)>,
    ) -> Result<Self, UniversalAudioError> {
        let codec_id = params.id();
        let codec = decoder::find(codec_id).ok_or_else(|| {
            UniversalAudioError::DecoderNotFound(format!("{:?}", codec_id))
        })?;

        let context = CodecContext::from_parameters(params)?;
        let mut decoder = context.decoder().open_as(codec)?.audio()?;
        decoder.set_packet_time_base(Rational(1, 1_000_000));

        let rational_tb = time_base.map(|(num, den)| Rational(num, den));

        Ok(Self {
            decoder,
            resampler: None,
            time_base: rational_tb,
            anchored: false,
            base_pts_us: 0,
            samples_since_anchor: 0,
            last_in_format: None,
            last_in_layout: None,
            last_in_rate: None,
        })
    }

    /// Constructs a `UniversalAudioDecoder` directly from an FFmpeg codec ID and optional extradata.
    pub fn from_codec_id(
        codec_id: Id,
        time_base: Option<(i32, i32)>,
        extradata: Option<&[u8]>,
    ) -> Result<Self, UniversalAudioError> {
        let codec = decoder::find(codec_id).ok_or_else(|| {
            UniversalAudioError::DecoderNotFound(format!("{:?}", codec_id))
        })?;

        let mut context = CodecContext::new_with_codec(codec);

        // SAFETY: Context pointer is valid. Configure default rate/layout for raw decoders like PCM, and extradata if provided.
        unsafe {
            let raw = context.as_mut_ptr();
            if (*raw).sample_rate == 0 {
                (*raw).sample_rate = 48_000;
            }
            if (*raw).ch_layout.nb_channels == 0 {
                ffmpeg_next::ffi::av_channel_layout_default(&mut (*raw).ch_layout, 2);
            }
            if let Some(extra) = extradata {
                if !extra.is_empty() {
                    let padded_len = extra.len() + ffmpeg_next::ffi::AV_INPUT_BUFFER_PADDING_SIZE as usize;
                    let buf = ffmpeg_next::ffi::av_malloc(padded_len) as *mut u8;
                    if !buf.is_null() {
                        std::ptr::copy_nonoverlapping(extra.as_ptr(), buf, extra.len());
                        std::ptr::write_bytes(buf.add(extra.len()), 0, ffmpeg_next::ffi::AV_INPUT_BUFFER_PADDING_SIZE as usize);
                        (*raw).extradata = buf;
                        (*raw).extradata_size = extra.len() as std::os::raw::c_int;
                    }
                }
            }
        }

        let mut decoder = context.decoder().open_as(codec)?.audio()?;
        decoder.set_packet_time_base(Rational(1, 1_000_000));

        let rational_tb = time_base.map(|(num, den)| Rational(num, den));

        Ok(Self {
            decoder,
            resampler: None,
            time_base: rational_tb,
            anchored: false,
            base_pts_us: 0,
            samples_since_anchor: 0,
            last_in_format: None,
            last_in_layout: None,
            last_in_rate: None,
        })
    }

    /// Maps an audio codec name string or alias into an `ffmpeg_next::codec::Id`.
    pub fn map_codec_name(name: &str) -> Option<Id> {
        let lower = name.trim().to_ascii_lowercase();
        match lower.as_str() {
            "aac" | "mp4a" | "a_aac" => Some(Id::AAC),
            "mp3" | "mp3lame" | "lame" | "a_mpeg/l3" => Some(Id::MP3),
            "opus" | "libopus" | "a_opus" => Some(Id::OPUS),
            "vorbis" | "libvorbis" | "a_vorbis" => Some(Id::VORBIS),
            "flac" | "a_flac" => Some(Id::FLAC),
            "ac3" | "a_ac3" => Some(Id::AC3),
            "eac3" | "ec-3" | "a_eac3" => Some(Id::EAC3),
            "dts" | "dca" | "a_dts" => Some(Id::DTS),
            "pcm" | "pcm_s16le" | "a_pcm/int/lit" => Some(Id::PCM_S16LE),
            "pcm_f32le" => Some(Id::PCM_F32LE),
            _ => decoder::find_by_name(&lower).map(|d| d.id()),
        }
    }

    /// Constructs a `UniversalAudioDecoder` from a codec name string (e.g. "aac", "opus", "dts", "flac") and optional extradata.
    pub fn from_codec_name(
        codec_name: &str,
        time_base: Option<(i32, i32)>,
        extradata: Option<&[u8]>,
    ) -> Result<Self, UniversalAudioError> {
        let codec_id = Self::map_codec_name(codec_name).ok_or_else(|| {
            UniversalAudioError::DecoderNotFound(codec_name.to_string())
        })?;

        Self::from_codec_id(codec_id, time_base, extradata)
    }

    /// Returns the active FFmpeg codec ID.
    #[inline]
    pub fn codec_id(&self) -> Id {
        self.decoder.id()
    }

    /// Sets or updates the stream timescale (numerator, denominator).
    pub fn set_time_base(&mut self, num: i32, den: i32) {
        self.time_base = Some(Rational(num, den));
    }

    /// Maximum tolerated drift (in microseconds) before re-anchoring the continuous sample clock.
    const CONTINUITY_REANCHOR_THRESHOLD_US: i64 = 2_000;

    /// Computes the expected continuous sample clock timestamp in microseconds.
    #[inline]
    fn expected_pts_us(&self) -> i64 {
        let delta_us = ((self.samples_since_anchor as i128 * 1_000_000)
            / TARGET_AUDIO_SAMPLE_RATE as i128) as i64;
        self.base_pts_us.saturating_add(delta_us)
    }

    /// Decodes raw compressed audio packet bytes with explicit presentation timestamp in microseconds.
    ///
    /// Tracks continuous sample progression in exact integer samples (`base_pts_us + samples * 1_000_000 / 48_000`)
    /// and only re-anchors `base_pts_us` when the decoded frame PTS diverges from the expected
    /// continuous sample clock by more than `2_000 us` (2 ms).
    pub fn decode_packet_us(
        &mut self,
        data: &[u8],
        pts_us: i64,
    ) -> Result<Vec<AudioFramePCM>, UniversalAudioError> {
        let mut pkt = Packet::copy(data);
        pkt.set_pts(Some(pts_us));

        let mut output_frames = Vec::new();
        let mut raw_frame = AudioFrame::empty();

        if let Err(err) = self.decoder.send_packet(&pkt) {
            if matches!(
                err,
                FfmpegError::Other { errno }
                    if errno == ffmpeg_next::ffi::EAGAIN || errno == ffmpeg_next::ffi::EWOULDBLOCK
            ) {
                while self.decoder.receive_frame(&mut raw_frame).is_ok() {
                    let pcm_frame = self.process_and_resample(&mut raw_frame, pts_us)?;
                    output_frames.push(pcm_frame);
                }
                self.decoder.send_packet(&pkt)?;
            } else {
                return Err(err.into());
            }
        }

        while self.decoder.receive_frame(&mut raw_frame).is_ok() {
            let pcm_frame = self.process_and_resample(&mut raw_frame, pts_us)?;
            output_frames.push(pcm_frame);
        }

        Ok(output_frames)
    }

    /// Decodes raw compressed audio packet bytes with explicit presentation timestamp in milliseconds.
    pub fn decode_packet(
        &mut self,
        packet_data: &[u8],
        pts_ms: u64,
    ) -> Result<Vec<AudioFramePCM>, UniversalAudioError> {
        let pts_us = (pts_ms as i64).saturating_mul(1000);
        self.decode_packet_us(packet_data, pts_us)
    }

    /// Resets internal decoder buffers and resampler state (for random-access seek).
    pub fn flush_buffers(&mut self) {
        self.decoder.flush();
        self.resampler = None;
        self.last_in_format = None;
        self.last_in_layout = None;
        self.last_in_rate = None;
        self.anchored = false;
        self.base_pts_us = 0;
        self.samples_since_anchor = 0;
    }

    /// Flushes residual buffered frames from the decoder and the resampler delay line,
    /// and resets the decoder state (`avcodec_flush_buffers`, resampler, and sample counters)
    /// so subsequent post-seek packets decode cleanly.
    pub fn flush(&mut self) -> Result<Vec<AudioFramePCM>, UniversalAudioError> {
        let _ = self.decoder.send_eof();

        let mut output_frames = Vec::new();
        let mut raw_frame = AudioFrame::empty();

        while self.decoder.receive_frame(&mut raw_frame).is_ok() {
            let fallback_us = self.expected_pts_us();
            if let Ok(pcm_frame) = self.process_and_resample(&mut raw_frame, fallback_us) {
                output_frames.push(pcm_frame);
            }
        }

        // Flush resampler internal delay buffer if active
        if let Some(ref mut resampler) = self.resampler {
            if let Some(delay) = resampler.delay() {
                let delay_samples = delay.output as usize;
                if delay_samples > 0 {
                    let dst_format = Sample::F32(Type::Packed);
                    let dst_layout = ChannelLayout::STEREO;
                    let mut flush_frame = AudioFrame::new(dst_format, delay_samples, dst_layout);
                    flush_frame.set_rate(TARGET_AUDIO_SAMPLE_RATE);

                    if resampler.flush(&mut flush_frame).is_ok() && flush_frame.samples() > 0 {
                        let plane = flush_frame.plane::<f32>(0);
                        let samples: Vec<f32> = plane.iter().map(|&s| soft_clip_sample(s)).collect();
                        let pts_us = self.expected_pts_us();
                        let pts_ms = (pts_us.max(0) / 1000) as u64;
                        self.samples_since_anchor += flush_frame.samples() as u64;

                        if let Ok(pcm_frame) = AudioFramePCM::new(
                            TARGET_AUDIO_SAMPLE_RATE,
                            TARGET_AUDIO_CHANNELS,
                            pts_ms,
                            samples,
                        ) {
                            output_frames.push(pcm_frame);
                        }
                    }
                }
            }
        }

        // Reset the codec context buffers, resampler, and sample clock so future post-seek packets decode cleanly.
        self.flush_buffers();

        Ok(output_frames)
    }

    /// Resamples a decoded raw frame into standardized 48kHz stereo f32 and packages into `AudioFramePCM`.
    fn process_and_resample(
        &mut self,
        raw_frame: &mut AudioFrame,
        fallback_pts_us: i64,
    ) -> Result<AudioFramePCM, UniversalAudioError> {
        let frame_in_pts_us = raw_frame
            .timestamp()
            .or_else(|| raw_frame.pts())
            .unwrap_or(fallback_pts_us);

        let expected_us = self.expected_pts_us();
        if !self.anchored
            || frame_in_pts_us.abs_diff(expected_us) > Self::CONTINUITY_REANCHOR_THRESHOLD_US as u64
        {
            self.base_pts_us = frame_in_pts_us;
            self.samples_since_anchor = 0;
            self.anchored = true;
        }

        let in_format = raw_frame.format();
        let in_layout = effective_layout(raw_frame);
        let in_rate = raw_frame.rate();

        if raw_frame.channel_layout().is_empty() {
            raw_frame.set_channel_layout(in_layout);
        }

        let dst_format = Sample::F32(Type::Packed);
        let dst_layout = ChannelLayout::STEREO;
        let dst_rate = TARGET_AUDIO_SAMPLE_RATE;

        // Reconfigure resampler if input format, channel layout, or sampling rate changed
        let needs_new_resampler = match (
            &self.resampler,
            self.last_in_format,
            self.last_in_layout,
            self.last_in_rate,
        ) {
            (Some(_), Some(f), Some(l), Some(r)) => f != in_format || l != in_layout || r != in_rate,
            _ => true,
        };

        if needs_new_resampler {
            self.resampler = Some(ResamplerContext::get(
                in_format,
                in_layout,
                in_rate,
                dst_format,
                dst_layout,
                dst_rate,
            )?);
            self.last_in_format = Some(in_format);
            self.last_in_layout = Some(in_layout);
            self.last_in_rate = Some(in_rate);
        }

        let resampler = self.resampler.as_mut().expect("resampler initialized");
        let mut out_frame = AudioFrame::empty();
        let _ = resampler.run(raw_frame, &mut out_frame)?;

        let out_samples_count = out_frame.samples();
        let plane = out_frame.plane::<f32>(0);
        let samples: Vec<f32> = plane.iter().map(|&s| soft_clip_sample(s)).collect();

        let frame_pts_us = self.expected_pts_us();
        let frame_pts_ms = (frame_pts_us.max(0) / 1000) as u64;
        self.samples_since_anchor += out_samples_count as u64;

        AudioFramePCM::new(
            TARGET_AUDIO_SAMPLE_RATE,
            TARGET_AUDIO_CHANNELS,
            frame_pts_ms,
            samples,
        )
        .map_err(UniversalAudioError::Frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_universal_audio_decoder_instantiation() {
        let _ = ffmpeg_next::init();
        let test_codecs = [
            ("aac", Id::AAC),
            ("mp3", Id::MP3),
            ("opus", Id::OPUS),
            ("flac", Id::FLAC),
            ("ac3", Id::AC3),
            ("eac3", Id::EAC3),
            ("dts", Id::DTS),
            ("pcm_s16le", Id::PCM_S16LE),
        ];

        for (name, id) in test_codecs {
            let dec = UniversalAudioDecoder::from_codec_id(id, Some((1, 48000)), None);
            assert!(dec.is_ok(), "Failed to instantiate audio decoder for {name}: {:?}", dec.err());
            assert_eq!(dec.unwrap().codec_id(), id);
        }

        // Vorbis decoder availability in host system
        assert!(decoder::find(Id::VORBIS).is_some(), "Vorbis decoder must be available in system FFmpeg");

        // Verify alias mapping for DTS and DCA
        let dec_dts = UniversalAudioDecoder::from_codec_name("dts", Some((1, 48000)), None);
        assert!(dec_dts.is_ok(), "from_codec_name('dts') must succeed: {:?}", dec_dts.err());
        assert_eq!(dec_dts.unwrap().codec_id(), Id::DTS);

        let dec_dca = UniversalAudioDecoder::from_codec_name("dca", Some((1, 48000)), None);
        assert!(dec_dca.is_ok(), "from_codec_name('dca') must succeed: {:?}", dec_dca.err());
        assert_eq!(dec_dca.unwrap().codec_id(), Id::DTS);
    }

    fn ensure_48k_aac_fixture() -> std::path::PathBuf {
        let dir = std::path::PathBuf::from("/tmp/syncplay_codecs_challenge");
        std::fs::create_dir_all(&dir).expect("create fixture dir");
        let path = dir.join("aac_48k_subms.aac");
        if !path.exists() {
            let status = std::process::Command::new("ffmpeg")
                .args([
                    "-y",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:sample_rate=48000:duration=0.4",
                    "-ar",
                    "48000",
                    "-ac",
                    "2",
                    "-c:a",
                    "aac",
                    "-f",
                    "adts",
                ])
                .arg(&path)
                .status()
                .expect("run ffmpeg for aac fixture");
            assert!(status.success(), "ffmpeg must create aac_48k_subms.aac");
        }
        path
    }

    #[test]
    fn test_aac_sub_ms_continuous_pts_and_reanchor() {
        let _ = ffmpeg_next::init();
        let path = ensure_48k_aac_fixture();
        let mut ictx = ffmpeg_next::format::input(&path).expect("open aac");
        let stream = ictx
            .streams()
            .best(ffmpeg_next::media::Type::Audio)
            .expect("audio stream");
        let stream_index = stream.index();

        let mut dec = UniversalAudioDecoder::from_parameters(
            stream.parameters(),
            Some((stream.time_base().0, stream.time_base().1)),
        )
        .expect("aac decoder");

        let packets: Vec<Vec<u8>> = ictx
            .packets()
            .filter(|(s, _)| s.index() == stream_index)
            .map(|(_, p)| p.data().unwrap_or(&[]).to_vec())
            .collect();
        assert!(packets.len() >= 10, "expected >= 10 AAC packets");

        // Feed truncated millisecond timestamps (21.333ms per 1024-sample frame -> 0, 21, 42, 64, 85, ...)
        let mut emitted_pts = Vec::new();
        for (idx, pkt) in packets.iter().take(6).enumerate() {
            let truncated_ms = ((idx as u64) * 1024 * 1000) / 48_000;
            let frames = dec.decode_packet(pkt, truncated_ms).expect("decode_packet");
            for f in frames {
                emitted_pts.push((f.pts_ms, f.samples.len() / 2));
            }
        }
        assert!(emitted_pts.len() >= 4, "expected >= 4 decoded frames");

        // Verify continuous sample-accurate PTS progression (no re-anchor drift)
        let mut cumulative_samples = 0u64;
        for (pts_ms, sample_count) in &emitted_pts {
            let expected_ms = (cumulative_samples * 1000) / 48_000;
            assert_eq!(
                *pts_ms, expected_ms,
                "Expected sample-accurate PTS {}ms, got {}ms",
                expected_ms, pts_ms
            );
            cumulative_samples += *sample_count as u64;
        }

        // Now feed a discontinuous PTS jump (+500_000 us = +500 ms > 2_000 us threshold) via decode_packet_us
        let jump_us = ((cumulative_samples as i64) * 1_000_000 / 48_000) + 500_000;
        let jump_frames = dec
            .decode_packet_us(&packets[6], jump_us)
            .expect("decode_packet_us jump");
        assert!(!jump_frames.is_empty());
        assert_eq!(jump_frames[0].pts_ms, (jump_us / 1000) as u64);
    }

    #[test]
    fn test_audio_flush_resets_counters_and_resumes_cleanly() {
        let _ = ffmpeg_next::init();
        let path = ensure_48k_aac_fixture();
        let mut ictx = ffmpeg_next::format::input(&path).expect("open aac");
        let stream = ictx
            .streams()
            .best(ffmpeg_next::media::Type::Audio)
            .expect("audio stream");
        let stream_index = stream.index();

        let mut dec = UniversalAudioDecoder::from_parameters(
            stream.parameters(),
            Some((stream.time_base().0, stream.time_base().1)),
        )
        .expect("aac decoder");

        let packets: Vec<Vec<u8>> = ictx
            .packets()
            .filter(|(s, _)| s.index() == stream_index)
            .map(|(_, p)| p.data().unwrap_or(&[]).to_vec())
            .collect();

        let _ = dec.decode_packet_us(&packets[0], 0).expect("pkt 0");
        let _ = dec.decode_packet_us(&packets[1], 21_333).expect("pkt 1");

        let _ = dec.flush().expect("flush");
        assert!(!dec.anchored);
        assert_eq!(dec.base_pts_us, 0);
        assert_eq!(dec.samples_since_anchor, 0);
        assert!(dec.resampler.is_none());

        // Immediately decode post-seek packet at 120s
        let resumed = dec
            .decode_packet_us(&packets[2], 120_000_000)
            .expect("post-flush decode");
        assert!(!resumed.is_empty(), "post-seek packet must decode cleanly");
        assert_eq!(resumed[0].pts_ms, 120_000);
    }
}
