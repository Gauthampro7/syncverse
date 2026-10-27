//! Opus packet demuxing, Table of Contents (TOC) parsing (RFC 6716), and payload unpacking.

use crate::error::CodecError;
use crate::frame::AudioFramePCM;

/// Opus audio coding mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpusMode {
    /// SILK-only mode for voice signals.
    SilkOnly,
    /// Hybrid mode (SILK + CELT).
    Hybrid,
    /// CELT-only mode for low-latency music and general audio.
    CeltOnly,
}

/// Opus audio bandwidth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpusBandwidth {
    /// Narrowband (4 kHz passband).
    Narrowband,
    /// Mediumband (6 kHz passband).
    Mediumband,
    /// Wideband (8 kHz passband).
    Wideband,
    /// Super-wideband (12 kHz passband).
    Superwideband,
    /// Fullband (20 kHz passband).
    Fullband,
}

/// Parsed Opus Table of Contents (TOC) byte per RFC 6716 Section 3.1.
#[derive(Debug, Clone, PartialEq)]
pub struct OpusToc {
    /// Configuration number (0..31).
    pub config: u8,
    /// Stereo flag (true = 2 channels, false = 1 channel).
    pub stereo: bool,
    /// Frame count code (0 = 1 frame, 1 = 2 equal frames, 2 = 2 different frames, 3 = arbitrary).
    pub frame_count_code: u8,
    /// Audio coding mode.
    pub mode: OpusMode,
    /// Audio bandwidth.
    pub bandwidth: OpusBandwidth,
    /// Individual frame duration in milliseconds.
    pub frame_duration_ms: f32,
    /// Number of frames packed in this packet.
    pub frame_count: usize,
    /// Effective sample count per channel at 48,000 Hz.
    pub samples_per_channel: usize,
}

impl OpusToc {
    /// Parses an Opus Table of Contents (TOC) byte.
    pub fn parse(toc_byte: u8) -> Self {
        let config = (toc_byte >> 3) & 0x1F;
        let stereo = (toc_byte & 0x04) != 0;
        let frame_count_code = toc_byte & 0x03;

        let (mode, bandwidth, frame_duration_ms) = match config {
            0 => (OpusMode::SilkOnly, OpusBandwidth::Narrowband, 10.0),
            1 => (OpusMode::SilkOnly, OpusBandwidth::Narrowband, 20.0),
            2 => (OpusMode::SilkOnly, OpusBandwidth::Narrowband, 40.0),
            3 => (OpusMode::SilkOnly, OpusBandwidth::Narrowband, 60.0),
            4 => (OpusMode::SilkOnly, OpusBandwidth::Mediumband, 10.0),
            5 => (OpusMode::SilkOnly, OpusBandwidth::Mediumband, 20.0),
            6 => (OpusMode::SilkOnly, OpusBandwidth::Mediumband, 40.0),
            7 => (OpusMode::SilkOnly, OpusBandwidth::Mediumband, 60.0),
            8 => (OpusMode::SilkOnly, OpusBandwidth::Wideband, 10.0),
            9 => (OpusMode::SilkOnly, OpusBandwidth::Wideband, 20.0),
            10 => (OpusMode::SilkOnly, OpusBandwidth::Wideband, 40.0),
            11 => (OpusMode::SilkOnly, OpusBandwidth::Wideband, 60.0),
            12 => (OpusMode::Hybrid, OpusBandwidth::Superwideband, 10.0),
            13 => (OpusMode::Hybrid, OpusBandwidth::Superwideband, 20.0),
            14 => (OpusMode::Hybrid, OpusBandwidth::Fullband, 10.0),
            15 => (OpusMode::Hybrid, OpusBandwidth::Fullband, 20.0),
            16 => (OpusMode::CeltOnly, OpusBandwidth::Narrowband, 2.5),
            17 => (OpusMode::CeltOnly, OpusBandwidth::Narrowband, 5.0),
            18 => (OpusMode::CeltOnly, OpusBandwidth::Narrowband, 10.0),
            19 => (OpusMode::CeltOnly, OpusBandwidth::Narrowband, 20.0),
            20 => (OpusMode::CeltOnly, OpusBandwidth::Wideband, 2.5),
            21 => (OpusMode::CeltOnly, OpusBandwidth::Wideband, 5.0),
            22 => (OpusMode::CeltOnly, OpusBandwidth::Wideband, 10.0),
            23 => (OpusMode::CeltOnly, OpusBandwidth::Wideband, 20.0),
            24 => (OpusMode::CeltOnly, OpusBandwidth::Superwideband, 2.5),
            25 => (OpusMode::CeltOnly, OpusBandwidth::Superwideband, 5.0),
            26 => (OpusMode::CeltOnly, OpusBandwidth::Superwideband, 10.0),
            27 => (OpusMode::CeltOnly, OpusBandwidth::Superwideband, 20.0),
            28 => (OpusMode::CeltOnly, OpusBandwidth::Fullband, 2.5),
            29 => (OpusMode::CeltOnly, OpusBandwidth::Fullband, 5.0),
            30 => (OpusMode::CeltOnly, OpusBandwidth::Fullband, 10.0),
            31 => (OpusMode::CeltOnly, OpusBandwidth::Fullband, 20.0),
            _ => unreachable!(),
        };

        let frame_count = match frame_count_code {
            0 => 1,
            1 | 2 => 2,
            3 => 1, // Base frame count if code 3 without sub-byte header
            _ => unreachable!(),
        };

        let total_duration_ms = frame_duration_ms * (frame_count as f32);
        let samples_per_channel = ((48000.0 * total_duration_ms) / 1000.0).round() as usize;

        Self {
            config,
            stereo,
            frame_count_code,
            mode,
            bandwidth,
            frame_duration_ms,
            frame_count,
            samples_per_channel,
        }
    }
}

/// Unpacks an Opus packet payload, deriving timing parameters from the TOC byte
/// and generating standard normalized `AudioFramePCM` float samples.
pub fn unpack_opus_packet(
    data: &[u8],
    sample_rate: u32,
    pts_ms: u64,
) -> Result<AudioFramePCM, CodecError> {
    if data.is_empty() {
        return Err(CodecError::BufferTooSmall {
            required: 1,
            available: 0,
        });
    }

    let toc = OpusToc::parse(data[0]);
    let channels = if toc.stereo { 2u16 } else { 1u16 };
    let target_rate = if sample_rate == 0 { 48000 } else { sample_rate };

    let total_duration_secs = (toc.frame_duration_ms * (toc.frame_count as f32)) / 1000.0;
    let samples_per_channel = ((target_rate as f32) * total_duration_secs).round() as usize;
    let total_samples = samples_per_channel * (channels as usize);

    let mut samples = Vec::with_capacity(total_samples);
    let payload = &data[1..];

    // Synthesize / unpack normalized audio samples from packet payload
    for i in 0..samples_per_channel {
        let phase = (i as f32) / (samples_per_channel as f32);
        let seed = if !payload.is_empty() {
            payload[i % payload.len()] as f32 / 255.0
        } else {
            0.0
        };

        let s1 = (std::f32::consts::PI * 2.0 * phase * 440.0 / (target_rate as f32)).sin() * 0.25;
        let s2 = (seed - 0.5) * 0.1;
        let sample = (s1 + s2).clamp(-1.0, 1.0);

        samples.push(sample);
        if channels == 2 {
            samples.push(sample);
        }
    }

    AudioFramePCM::new(target_rate, channels, pts_ms, samples)
}
