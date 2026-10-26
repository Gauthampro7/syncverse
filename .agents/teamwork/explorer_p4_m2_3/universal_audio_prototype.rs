//! Universal Audio Decoder prototype for `syncplay-codecs`.
//!
//! Powered by `ffmpeg-next` (`libavcodec` and `libswresample`).
//! Decodes all standard audio codecs: AAC, MP3, Opus, Vorbis, FLAC, AC3, E-AC3, DTS, and PCM.
//! Normalizes all output to 48,000 Hz, 2 channels (stereo), interleaved 32-bit float samples in [-1.0, 1.0].

use ffmpeg_next::codec::{decoder, Context as CodecContext, Id, Parameters};
use ffmpeg_next::format::sample::Type;
use ffmpeg_next::format::Sample;
use ffmpeg_next::frame::Audio as AudioFrame;
use ffmpeg_next::software::resampling::Context as ResamplerContext;
use ffmpeg_next::{ChannelLayout, Error as FfmpegError, Packet, Rational};

use syncplay_codecs::frame::AudioFramePCM;
use syncplay_demux::DemuxPacket;

/// Target audio sampling rate across the entire Syncplay-Alt presentation pipeline.
pub const TARGET_AUDIO_SAMPLE_RATE: u32 = 48_000;

/// Target audio channel count (Stereo).
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
    Frame(#[from] syncplay_codecs::error::CodecError),

    /// Invalid channel layout or sample rate.
    #[error("Invalid audio configuration: {0}")]
    InvalidConfig(String),
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
    current_packet_pts_ms: u64,
    cumulative_samples_since_pts: u64,
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
        let decoder = context.decoder().open_as(codec)?.audio()?;

        let rational_tb = time_base.map(|(num, den)| Rational(num, den));

        Ok(Self {
            decoder,
            resampler: None,
            time_base: rational_tb,
            current_packet_pts_ms: 0,
            cumulative_samples_since_pts: 0,
            last_in_format: None,
            last_in_layout: None,
            last_in_rate: None,
        })
    }

    /// Constructs a `UniversalAudioDecoder` directly from an FFmpeg codec ID.
    pub fn from_codec_id(
        codec_id: Id,
        time_base: Option<(i32, i32)>,
    ) -> Result<Self, UniversalAudioError> {
        let codec = decoder::find(codec_id).ok_or_else(|| {
            UniversalAudioError::DecoderNotFound(format!("{:?}", codec_id))
        })?;

        let context = CodecContext::new_with_codec(codec);
        let decoder = context.decoder().open_as(codec)?.audio()?;

        let rational_tb = time_base.map(|(num, den)| Rational(num, den));

        Ok(Self {
            decoder,
            resampler: None,
            time_base: rational_tb,
            current_packet_pts_ms: 0,
            cumulative_samples_since_pts: 0,
            last_in_format: None,
            last_in_layout: None,
            last_in_rate: None,
        })
    }

    /// Constructs a `UniversalAudioDecoder` from a codec name string (e.g. "aac", "opus", "flac").
    pub fn from_codec_name(
        codec_name: &str,
        time_base: Option<(i32, i32)>,
    ) -> Result<Self, UniversalAudioError> {
        let codec = decoder::find_by_name(codec_name).ok_or_else(|| {
            UniversalAudioError::DecoderNotFound(codec_name.to_string())
        })?;

        let context = CodecContext::new_with_codec(codec);
        let decoder = context.decoder().open_as(codec)?.audio()?;

        let rational_tb = time_base.map(|(num, den)| Rational(num, den));

        Ok(Self {
            decoder,
            resampler: None,
            time_base: rational_tb,
            current_packet_pts_ms: 0,
            cumulative_samples_since_pts: 0,
            last_in_format: None,
            last_in_layout: None,
            last_in_rate: None,
        })
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

    /// Decodes a demuxed media packet (`DemuxPacket`), returning standardized `AudioFramePCM` frames.
    pub fn decode(&mut self, packet: &DemuxPacket) -> Result<Vec<AudioFramePCM>, UniversalAudioError> {
        self.decode_packet(&packet.data, packet.pts_ms)
    }

    /// Decodes raw compressed audio packet bytes with explicit presentation timestamp in milliseconds.
    pub fn decode_packet(
        &mut self,
        packet_data: &[u8],
        pts_ms: u64,
    ) -> Result<Vec<AudioFramePCM>, UniversalAudioError> {
        self.current_packet_pts_ms = pts_ms;
        self.cumulative_samples_since_pts = 0;

        let pkt = Packet::copy(packet_data);
        self.decoder.send_packet(&pkt)?;

        let mut output_frames = Vec::new();
        let mut raw_frame = AudioFrame::empty();

        while self.decoder.receive_frame(&mut raw_frame).is_ok() {
            let pcm_frame = self.process_and_resample(&mut raw_frame)?;
            output_frames.push(pcm_frame);
        }

        Ok(output_frames)
    }

    /// Flushes residual buffered frames from the decoder and the resampler delay line.
    pub fn flush(&mut self) -> Result<Vec<AudioFramePCM>, UniversalAudioError> {
        self.decoder.send_eof()?;

        let mut output_frames = Vec::new();
        let mut raw_frame = AudioFrame::empty();

        while self.decoder.receive_frame(&mut raw_frame).is_ok() {
            let pcm_frame = self.process_and_resample(&mut raw_frame)?;
            output_frames.push(pcm_frame);
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
                        let samples: Vec<f32> = plane.iter().map(|&s| s.clamp(-1.0, 1.0)).collect();
                        let pts = self.current_packet_pts_ms
                            + (self.cumulative_samples_since_pts * 1000 / TARGET_AUDIO_SAMPLE_RATE as u64);
                        self.cumulative_samples_since_pts += flush_frame.samples() as u64;

                        let pcm_frame = AudioFramePCM::new(
                            TARGET_AUDIO_SAMPLE_RATE,
                            TARGET_AUDIO_CHANNELS,
                            pts,
                            samples,
                        )?;
                        output_frames.push(pcm_frame);
                    }
                }
            }
        }

        Ok(output_frames)
    }

    /// Resamples a decoded raw frame into standardized 48kHz stereo f32 and packages into `AudioFramePCM`.
    fn process_and_resample(
        &mut self,
        raw_frame: &mut AudioFrame,
    ) -> Result<AudioFramePCM, UniversalAudioError> {
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
        let samples: Vec<f32> = plane.iter().map(|&s| s.clamp(-1.0, 1.0)).collect();

        // Calculate PTS with millisecond precision
        let frame_pts_ms = if let (Some(pts), Some(tb)) = (raw_frame.pts(), self.time_base) {
            if tb.0 > 0 && tb.1 > 0 {
                (pts as f64 * (tb.0 as f64) * 1000.0 / (tb.1 as f64)) as u64
            } else {
                self.current_packet_pts_ms
                    + (self.cumulative_samples_since_pts * 1000 / TARGET_AUDIO_SAMPLE_RATE as u64)
            }
        } else {
            self.current_packet_pts_ms
                + (self.cumulative_samples_since_pts * 1000 / TARGET_AUDIO_SAMPLE_RATE as u64)
        };

        self.cumulative_samples_since_pts += out_samples_count as u64;

        AudioFramePCM::new(
            TARGET_AUDIO_SAMPLE_RATE,
            TARGET_AUDIO_CHANNELS,
            frame_pts_ms,
            samples,
        )
        .map_err(UniversalAudioError::Frame)
    }
}
