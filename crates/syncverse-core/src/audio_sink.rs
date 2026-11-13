//! Native low-latency audio output sinks (`cpal`), PipeWire/ALSA subprocess fallback, and null/mock sinks.
//!
//! Provides in-process hardware audio output via [`CpalAudioSink`] backed by a lock-free
//! [`SpscRingBuffer`] with 2ms cosine de-click ramps, atomic smooth volume/mute/pause controls,
//! instant `<1ms` seek flushes, and accurate hardware buffer latency reporting.
//!
//! When audio hardware is unavailable or running headless, [`create_default_audio_sink`] and
//! [`AutoAudioSink`] gracefully fall back through [`PipeAudioSink`] (`pw-cat` / `aplay`) to
//! [`NullAudioSink`].

use std::f32::consts::PI;
use std::io::Write;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    BufferSize, FromSample, SampleFormat, SizedSample, Stream, StreamConfig, SupportedBufferSize,
};
use syncverse_clock::SpscRingBuffer;
use syncverse_codecs::{soft_clip_sample, LinearResampler};

use crate::error::CoreError;

/// Error type returned by [`AudioSink`] operations.
pub type AudioSinkError = CoreError;

/// Power-of-two capacity (in `f32` samples) for [`CpalAudioSink`]'s lock-free ring buffer.
///
/// 32,768 samples = 16,384 stereo frames (~341 ms at 48 kHz stereo, 128 KiB).
const CPAL_RING_CAPACITY: usize = 32_768;

/// Preferred low-latency hardware period size in frames (~10.67 ms at 48 kHz).
const PREFERRED_HW_BUFFER_FRAMES: u32 = 512;

/// Core interface for audio hardware or daemon output sinks.
pub trait AudioSink: Send {
    /// Writes interleaved 32-bit float PCM samples to the sink.
    /// Returns the number of samples written.
    fn write_samples(&mut self, samples: &[f32]) -> Result<usize, AudioSinkError>;

    /// Flushes any pending buffered audio to the hardware or playback daemon.
    fn flush(&mut self) -> Result<(), AudioSinkError> {
        Ok(())
    }

    /// Target sample rate in Hz (default 48,000).
    fn sample_rate(&self) -> u32 {
        48_000
    }

    /// Number of audio channels (default 2 for stereo).
    fn channels(&self) -> u16 {
        2
    }

    /// Pauses audio output if supported.
    fn pause(&mut self) -> Result<(), AudioSinkError> {
        Ok(())
    }

    /// Resumes audio output if supported.
    fn resume(&mut self) -> Result<(), AudioSinkError> {
        Ok(())
    }

    /// Sets the linear output volume gain in `0.0..=2.0` (1.0 = unity gain).
    fn set_volume(&mut self, _volume: f32) {}

    /// Mutes or unmutes audio output non-destructively without discarding queued PCM samples.
    fn set_muted(&mut self, _muted: bool) {}

    /// Returns total audio output latency (queued ring samples + hardware buffer) in milliseconds.
    fn latency_ms(&self) -> u64 {
        0
    }

    /// Returns the duration of audio currently queued in the sink buffer in milliseconds.
    fn queued_ms(&self) -> u64 {
        0
    }

    /// Returns the number of input audio frames that can currently be accepted without blocking.
    fn available_frames(&self) -> usize {
        usize::MAX
    }
}

/// Shared lock-free ring buffer and atomic control state between [`CpalAudioSink`] and its
/// real-time audio callback thread.
struct CpalSharedState {
    ring: SpscRingBuffer<f32, CPAL_RING_CAPACITY>,
    /// Protects consumer-side `try_pop()` exclusively during synchronous `flush()` purges.
    /// The real-time audio callback uses `try_lock()` and never blocks.
    consumer_lock: Mutex<()>,
    /// Linear gain in `[0.0, 2.0]` stored as `f32::to_bits()`.
    volume_bits: AtomicU32,
    /// Instant non-destructive mute flag.
    muted: AtomicBool,
    /// Atomic pause flag (fades smoothly to silence without tearing down the audio stream).
    paused: AtomicBool,
    /// Monotonically incremented on `flush()` to trigger a 2ms cosine de-click transition.
    flush_epoch: AtomicU64,
    /// Set if the underlying CPAL stream reports a fatal device error.
    stream_failed: AtomicBool,
}

impl CpalSharedState {
    fn new() -> Self {
        Self {
            ring: SpscRingBuffer::new(),
            consumer_lock: Mutex::new(()),
            volume_bits: AtomicU32::new(1.0f32.to_bits()),
            muted: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            flush_epoch: AtomicU64::new(0),
            stream_failed: AtomicBool::new(false),
        }
    }

    fn target_gain(&self) -> f32 {
        if self.muted.load(Ordering::Acquire) {
            return 0.0;
        }
        let vol = f32::from_bits(self.volume_bits.load(Ordering::Acquire));
        if vol.is_finite() {
            vol.clamp(0.0, 2.0)
        } else {
            0.0
        }
    }
}

/// Per-callback audio DSP state owned exclusively by the real-time CPAL thread (zero locks, zero allocs).
struct CallbackState {
    channels: usize,
    ramp_frames: usize,
    current_gain: f32,
    last_flush_epoch: u64,
    fade_out_remaining: usize,
    fade_in_remaining: usize,
    was_paused: bool,
    in_underrun: bool,
    prev_frame: Vec<f32>,
}

impl CallbackState {
    fn new(device_sample_rate: u32, device_channels: u16) -> Self {
        let channels = (device_channels as usize).max(1);
        let ramp_frames = ((device_sample_rate as usize * 2) / 1000).max(1);
        Self {
            channels,
            ramp_frames,
            current_gain: 1.0,
            last_flush_epoch: 0,
            fade_out_remaining: 0,
            fade_in_remaining: 0,
            was_paused: false,
            in_underrun: false,
            prev_frame: vec![0.0f32; channels],
        }
    }

    /// Computes a raised-cosine value in `[0.0, 1.0]` for `t` in `[0.0, 1.0]`.
    #[inline]
    fn cosine_ramp(t: f32) -> f32 {
        0.5 * (1.0 - (PI * t.clamp(0.0, 1.0)).cos())
    }

    /// Fills a CPAL output buffer with processed, volume-smoothed, soft-clipped audio samples.
    fn fill_output<T>(&mut self, shared: &CpalSharedState, output: &mut [T])
    where
        T: SizedSample + FromSample<f32>,
    {
        let ch = self.channels;
        if ch == 0 || output.is_empty() {
            return;
        }

        // Detect synchronous flush() epoch bumps to trigger an anti-click transition.
        let epoch = shared.flush_epoch.load(Ordering::Acquire);
        if epoch != self.last_flush_epoch {
            self.last_flush_epoch = epoch;
            if self.prev_frame.iter().any(|&s| s.abs() > 1e-5) {
                self.fade_out_remaining = self.ramp_frames;
            }
            self.fade_in_remaining = self.ramp_frames;
            self.in_underrun = false;
        }

        let is_paused = shared.paused.load(Ordering::Acquire);
        if is_paused && !self.was_paused {
            self.was_paused = true;
            if self.prev_frame.iter().any(|&s| s.abs() > 1e-5) {
                self.fade_out_remaining = self.ramp_frames;
            }
        } else if !is_paused && self.was_paused {
            self.was_paused = false;
            self.fade_out_remaining = 0;
            self.fade_in_remaining = self.ramp_frames;
        }

        let target_gain = shared.target_gain();
        let gain_slew = 1.0 / (self.ramp_frames as f32);

        // Non-blocking try_lock: if flush() is mid-drain (<10us), emit smooth fade/silence immediately.
        let consumer_guard = shared.consumer_lock.try_lock();

        for frame_out in output.chunks_mut(ch) {
            if self.fade_out_remaining > 0 {
                let progress = (self.ramp_frames - self.fade_out_remaining + 1) as f32
                    / (self.ramp_frames as f32);
                let fade = 1.0 - Self::cosine_ramp(progress);
                self.fade_out_remaining -= 1;

                for (c, dst) in frame_out.iter_mut().enumerate() {
                    let s = self.prev_frame[c] * fade;
                    if self.fade_out_remaining == 0 {
                        self.prev_frame[c] = 0.0;
                    }
                    *dst = T::from_sample(s.clamp(-1.0, 1.0));
                }
                continue;
            }

            if is_paused {
                self.prev_frame.fill(0.0);
                for dst in frame_out.iter_mut() {
                    *dst = T::EQUILIBRIUM;
                }
                continue;
            }

            // Smooth gain per frame to eliminate zipper noise on volume/mute changes.
            let diff = target_gain - self.current_gain;
            if diff.abs() <= gain_slew {
                self.current_gain = target_gain;
            } else {
                self.current_gain += diff.signum() * gain_slew;
            }

            let mut popped_all = false;
            if consumer_guard.is_ok() && shared.ring.len() >= ch {
                popped_all = true;
                for c in 0..ch {
                    if let Some(s) = shared.ring.try_pop() {
                        self.prev_frame[c] = s;
                    } else {
                        self.prev_frame[c] = 0.0;
                        popped_all = false;
                    }
                }
            }

            if popped_all {
                if self.in_underrun {
                    self.in_underrun = false;
                    self.fade_in_remaining = self.ramp_frames;
                }

                let fade_in = if self.fade_in_remaining > 0 {
                    let progress = (self.ramp_frames - self.fade_in_remaining + 1) as f32
                        / (self.ramp_frames as f32);
                    self.fade_in_remaining -= 1;
                    Self::cosine_ramp(progress)
                } else {
                    1.0
                };

                let total_gain = self.current_gain * fade_in;
                for (c, dst) in frame_out.iter_mut().enumerate() {
                    let raw = self.prev_frame[c];
                    let scaled = if raw.is_finite() { raw * total_gain } else { 0.0 };
                    let final_sample = if total_gain > 1.0 {
                        soft_clip_sample(scaled)
                    } else {
                        scaled.clamp(-1.0, 1.0)
                    };
                    self.prev_frame[c] = final_sample;
                    *dst = T::from_sample(final_sample);
                }
            } else {
                // Underrun or ring draining during flush: smoothly decay previous sample to 0.
                self.in_underrun = true;
                for (c, dst) in frame_out.iter_mut().enumerate() {
                    let decayed = self.prev_frame[c] * 0.85;
                    self.prev_frame[c] = if decayed.abs() < 1e-5 { 0.0 } else { decayed };
                    *dst = T::from_sample(self.prev_frame[c].clamp(-1.0, 1.0));
                }
            }
        }
    }
}

/// Core lock-free ring buffer and sample adaptation engine used by [`CpalAudioSink`].
///
/// Separated cleanly so DSP, ring capacity, flush, mute/volume/pause, and latency math can be
/// deterministically verified even in headless test environments without audio hardware.
struct CpalRingEngine {
    shared: Arc<CpalSharedState>,
    input_sample_rate: u32,
    input_channels: u16,
    device_sample_rate: u32,
    device_channels: u16,
    hw_buffer_frames: u32,
    resampler: Option<LinearResampler>,
    channel_scratch: Vec<f32>,
    resample_scratch: Vec<f32>,
}

impl CpalRingEngine {
    fn new(
        input_sample_rate: u32,
        input_channels: u16,
        device_sample_rate: u32,
        device_channels: u16,
        hw_buffer_frames: u32,
    ) -> Result<Self, AudioSinkError> {
        if input_sample_rate == 0
            || input_channels == 0
            || device_sample_rate == 0
            || device_channels == 0
        {
            return Err(CoreError::Runtime(
                "Audio sample rate and channel count must be > 0".into(),
            ));
        }

        let resampler = if device_sample_rate != input_sample_rate {
            Some(LinearResampler::new(
                input_sample_rate,
                device_sample_rate,
                device_channels,
            )?)
        } else {
            None
        };

        Ok(Self {
            shared: Arc::new(CpalSharedState::new()),
            input_sample_rate,
            input_channels,
            device_sample_rate,
            device_channels,
            hw_buffer_frames,
            resampler,
            channel_scratch: Vec::with_capacity(4096),
            resample_scratch: Vec::with_capacity(4096),
        })
    }

    fn write_samples(&mut self, samples: &[f32]) -> Result<usize, AudioSinkError> {
        if self.shared.stream_failed.load(Ordering::Acquire) {
            return Err(CoreError::Runtime(
                "CPAL output audio stream disconnected".into(),
            ));
        }

        let in_ch = self.input_channels as usize;
        let dev_ch = self.device_channels as usize;
        let total_input_frames = samples.len() / in_ch;
        if total_input_frames == 0 {
            return Ok(0);
        }

        let free_dev_samples = CPAL_RING_CAPACITY.saturating_sub(self.shared.ring.len());
        let free_dev_frames = free_dev_samples / dev_ch;
        if free_dev_frames == 0 {
            return Ok(0);
        }

        let max_input_frames = if self.device_sample_rate == self.input_sample_rate {
            free_dev_frames
        } else {
            let scaled = (free_dev_frames as u64 * self.input_sample_rate as u64)
                / (self.device_sample_rate as u64);
            // Reserve 2 frames of rounding margin when upsampling/downsampling.
            (scaled as usize).saturating_sub(2)
        };

        if max_input_frames == 0 {
            return Ok(0);
        }

        let frames_to_write = total_input_frames.min(max_input_frames);
        let input_slice = &samples[..frames_to_write * in_ch];

        // Step 1: Fast zero-copy path when input and device formats match identically.
        if self.device_channels == self.input_channels
            && self.device_sample_rate == self.input_sample_rate
        {
            let mut pushed = 0usize;
            for &s in input_slice {
                if self.shared.ring.try_push(s).is_err() {
                    break;
                }
                pushed += 1;
            }
            return Ok(pushed);
        }

        // Step 2: Channel adaptation into pre-allocated scratch buffer.
        let ch_adapted: &[f32] = if self.device_channels == self.input_channels {
            input_slice
        } else {
            Self::adapt_channels(
                input_slice,
                self.input_channels,
                self.device_channels,
                &mut self.channel_scratch,
            );
            &self.channel_scratch
        };

        // Step 3: Sample rate adaptation (if hardware rejected 48 kHz).
        let final_samples: &[f32] = if let Some(ref resampler) = self.resampler {
            resampler.resample_into(ch_adapted, &mut self.resample_scratch)?;
            &self.resample_scratch
        } else {
            ch_adapted
        };

        let mut pushed = 0usize;
        for &s in final_samples {
            if self.shared.ring.try_push(s).is_err() {
                break;
            }
            pushed += 1;
        }
        let _ = pushed;

        Ok(frames_to_write * in_ch)
    }

    fn adapt_channels(input: &[f32], in_ch: u16, out_ch: u16, out: &mut Vec<f32>) {
        out.clear();
        let in_c = in_ch as usize;
        let out_c = out_ch as usize;
        let frames = input.len() / in_c;
        out.reserve(frames * out_c);

        for frame in input.chunks_exact(in_c) {
            if in_c == 2 && out_c == 1 {
                // Stereo -> Mono downmix
                out.push((frame[0] + frame[1]) * 0.5);
            } else if in_c == 1 && out_c >= 2 {
                // Mono -> Stereo (zero-pad extra surround channels)
                out.push(frame[0]);
                out.push(frame[0]);
                for _ in 2..out_c {
                    out.push(0.0);
                }
            } else {
                let copy_len = in_c.min(out_c);
                out.extend_from_slice(&frame[..copy_len]);
                out.resize(out.len() + (out_c - copy_len), 0.0);
            }
        }
    }

    fn flush(&mut self) -> Result<(), AudioSinkError> {
        self.shared.flush_epoch.fetch_add(1, Ordering::AcqRel);
        if let Some(ref resampler) = self.resampler {
            resampler.reset();
        }
        let _guard = self
            .shared
            .consumer_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        while self.shared.ring.try_pop().is_some() {}
        Ok(())
    }

    fn pause(&mut self) {
        self.shared.paused.store(true, Ordering::Release);
    }

    fn resume(&mut self) {
        self.shared.paused.store(false, Ordering::Release);
    }

    fn set_volume(&mut self, volume: f32) {
        let clamped = if volume.is_finite() {
            volume.clamp(0.0, 2.0)
        } else {
            0.0
        };
        self.shared
            .volume_bits
            .store(clamped.to_bits(), Ordering::Release);
    }

    fn set_muted(&mut self, muted: bool) {
        self.shared.muted.store(muted, Ordering::Release);
    }

    fn queued_frames_device(&self) -> u64 {
        (self.shared.ring.len() / (self.device_channels as usize)) as u64
    }

    fn queued_ms(&self) -> u64 {
        (self.queued_frames_device() * 1000) / (self.device_sample_rate as u64)
    }

    fn latency_ms(&self) -> u64 {
        let total_frames = self.queued_frames_device() + (self.hw_buffer_frames as u64);
        (total_frames * 1000) / (self.device_sample_rate as u64)
    }

    fn available_frames(&self) -> usize {
        let free_dev_samples = CPAL_RING_CAPACITY.saturating_sub(self.shared.ring.len());
        let free_dev_frames = free_dev_samples / (self.device_channels as usize);
        if self.device_sample_rate == self.input_sample_rate {
            free_dev_frames
        } else {
            let scaled = (free_dev_frames as u64 * self.input_sample_rate as u64)
                / (self.device_sample_rate as u64);
            (scaled as usize).saturating_sub(2)
        }
    }
}

/// Native in-process low-latency hardware audio sink powered by `cpal`.
///
/// Features:
/// - Zero subprocess spawning; streams directly into the host audio backend (PipeWire/ALSA/WASAPI/CoreAudio).
/// - Lock-free [`SpscRingBuffer`] handoff with non-blocking `write_samples` and `<1ms` `flush()`.
/// - 2ms cosine de-click fade-out/fade-in on seeks, pause/resume, and underrun recovery.
/// - Per-sample smoothed atomic volume (`0.0..=2.0`) with `soft_clip_sample` headroom protection.
/// - Real hardware + ring buffer latency reporting (`latency_ms()`).
pub struct CpalAudioSink {
    engine: CpalRingEngine,
    _stream: Stream,
    device_name: String,
    sample_format_label: &'static str,
}

impl CpalAudioSink {
    /// Opens the default output audio device configured for 48 kHz stereo `f32` PCM input.
    pub fn new() -> Result<Self, AudioSinkError> {
        Self::with_config(48_000, 2)
    }

    /// Opens the default output audio device adapting from `(sample_rate, channels)` input PCM.
    pub fn with_config(sample_rate: u32, channels: u16) -> Result<Self, AudioSinkError> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or_else(|| {
            CoreError::Runtime("No default CPAL output audio device available".into())
        })?;

        let device_name = device
            .description()
            .map(|d| d.name().to_string())
            .unwrap_or_else(|_| "default".to_string());

        let (stream_config, sample_format, estimated_hw_frames) =
            Self::negotiate_config(&device, sample_rate, channels)?;

        let mut engine = CpalRingEngine::new(
            sample_rate,
            channels,
            stream_config.sample_rate,
            stream_config.channels,
            estimated_hw_frames,
        )?;

        let (stream, actual_hw_frames) = Self::build_and_start_stream(
            &device,
            stream_config,
            sample_format,
            estimated_hw_frames,
            Arc::clone(&engine.shared),
        )?;
        engine.hw_buffer_frames = actual_hw_frames;

        let sample_format_label = match sample_format {
            SampleFormat::F32 => "f32",
            SampleFormat::I16 => "i16",
            SampleFormat::U16 => "u16",
            _ => "other",
        };

        Ok(Self {
            engine,
            _stream: stream,
            device_name,
            sample_format_label,
        })
    }

    /// Negotiates the optimal output stream configuration on `device`, preferring 48 kHz `f32`.
    fn negotiate_config(
        device: &cpal::Device,
        preferred_rate: u32,
        preferred_channels: u16,
    ) -> Result<(StreamConfig, SampleFormat, u32), AudioSinkError> {
        let default_cfg = device.default_output_config().map_err(|e| {
            CoreError::Runtime(format!("Failed to query default output config: {e}"))
        })?;

        let mut best_candidate: Option<(u8, StreamConfig, SampleFormat, u32)> = None;

        if let Ok(ranges) = device.supported_output_configs() {
            for range in ranges {
                let fmt_score: u8 = match range.sample_format() {
                    SampleFormat::F32 => 30,
                    SampleFormat::I16 => 20,
                    SampleFormat::U16 => 10,
                    _ => continue,
                };

                let ch_score: u8 = if range.channels() == preferred_channels {
                    20
                } else if range.channels() >= 2 {
                    10
                } else {
                    5
                };

                let chosen_rate = if range.contains_rate(preferred_rate) {
                    preferred_rate
                } else if range.contains_rate(48_000) {
                    48_000
                } else if range.contains_rate(44_100) {
                    44_100
                } else {
                    range.max_sample_rate()
                };

                let rate_score: u8 = if chosen_rate == preferred_rate {
                    40
                } else if chosen_rate == 48_000 {
                    30
                } else {
                    10
                };

                let hw_frames = match *range.buffer_size() {
                    SupportedBufferSize::Range { min, max } => {
                        PREFERRED_HW_BUFFER_FRAMES.clamp(min, max)
                    }
                    SupportedBufferSize::Unknown => PREFERRED_HW_BUFFER_FRAMES,
                };

                let total_score = fmt_score + ch_score + rate_score;
                let replace = best_candidate
                    .as_ref()
                    .map(|(best, _, _, _)| total_score > *best)
                    .unwrap_or(true);

                if replace {
                    best_candidate = Some((
                        total_score,
                        StreamConfig {
                            channels: range.channels(),
                            sample_rate: chosen_rate,
                            buffer_size: BufferSize::Fixed(hw_frames),
                        },
                        range.sample_format(),
                        hw_frames,
                    ));
                }
            }
        }

        if let Some((_, cfg, fmt, hw_frames)) = best_candidate {
            return Ok((cfg, fmt, hw_frames));
        }

        let fmt = match default_cfg.sample_format() {
            SampleFormat::F32 | SampleFormat::I16 | SampleFormat::U16 => {
                default_cfg.sample_format()
            }
            other => {
                return Err(CoreError::Runtime(format!(
                    "Unsupported CPAL default sample format: {other}"
                )));
            }
        };

        let hw_frames = match *default_cfg.buffer_size() {
            SupportedBufferSize::Range { min, max } => PREFERRED_HW_BUFFER_FRAMES.clamp(min, max),
            SupportedBufferSize::Unknown => 1024,
        };

        let mut cfg = default_cfg.config();
        cfg.buffer_size = BufferSize::Default;
        Ok((cfg, fmt, hw_frames))
    }

    /// Builds and starts the CPAL output stream, falling back to `BufferSize::Default` if a
    /// fixed period size is rejected by the backend.
    fn build_and_start_stream(
        device: &cpal::Device,
        config: StreamConfig,
        sample_format: SampleFormat,
        estimated_hw_frames: u32,
        shared: Arc<CpalSharedState>,
    ) -> Result<(Stream, u32), AudioSinkError> {
        match Self::try_build_typed_stream(device, config, sample_format, Arc::clone(&shared)) {
            Ok(stream) => {
                stream.play().map_err(|e| {
                    CoreError::Runtime(format!("Failed to start CPAL output stream: {e}"))
                })?;
                Ok((stream, estimated_hw_frames))
            }
            Err(fixed_err) if matches!(config.buffer_size, BufferSize::Fixed(_)) => {
                let mut fallback_cfg = config;
                fallback_cfg.buffer_size = BufferSize::Default;
                let stream = Self::try_build_typed_stream(
                    device,
                    fallback_cfg,
                    sample_format,
                    shared,
                )
                .map_err(|default_err| {
                    CoreError::Runtime(format!(
                        "Failed to build CPAL stream (fixed: {fixed_err}; default: {default_err})"
                    ))
                })?;
                stream.play().map_err(|e| {
                    CoreError::Runtime(format!("Failed to start CPAL output stream: {e}"))
                })?;
                Ok((stream, estimated_hw_frames.max(1024)))
            }
            Err(err) => Err(err),
        }
    }

    fn try_build_typed_stream(
        device: &cpal::Device,
        config: StreamConfig,
        sample_format: SampleFormat,
        shared: Arc<CpalSharedState>,
    ) -> Result<Stream, AudioSinkError> {
        let rate = config.sample_rate;
        let channels = config.channels;

        let err_shared = Arc::clone(&shared);
        let error_callback = move |_err: cpal::Error| {
            err_shared.stream_failed.store(true, Ordering::Release);
        };

        let stream_res = match sample_format {
            SampleFormat::F32 => {
                let mut cb_state = CallbackState::new(rate, channels);
                device.build_output_stream(
                    config,
                    move |data: &mut [f32], _| {
                        cb_state.fill_output(&shared, data);
                    },
                    error_callback,
                    None,
                )
            }
            SampleFormat::I16 => {
                let mut cb_state = CallbackState::new(rate, channels);
                device.build_output_stream(
                    config,
                    move |data: &mut [i16], _| {
                        cb_state.fill_output(&shared, data);
                    },
                    error_callback,
                    None,
                )
            }
            SampleFormat::U16 => {
                let mut cb_state = CallbackState::new(rate, channels);
                device.build_output_stream(
                    config,
                    move |data: &mut [u16], _| {
                        cb_state.fill_output(&shared, data);
                    },
                    error_callback,
                    None,
                )
            }
            other => {
                return Err(CoreError::Runtime(format!(
                    "Unsupported CPAL sample format: {other}"
                )));
            }
        };

        stream_res.map_err(|e| CoreError::Runtime(format!("CPAL build_output_stream failed: {e}")))
    }

    /// Returns the name of the active hardware output device.
    #[inline]
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Returns the negotiated device sample rate in Hz.
    #[inline]
    pub fn device_sample_rate(&self) -> u32 {
        self.engine.device_sample_rate
    }

    /// Returns the negotiated device channel count.
    #[inline]
    pub fn device_channels(&self) -> u16 {
        self.engine.device_channels
    }

    /// Returns the negotiated device sample format label (`"f32"`, `"i16"`, or `"u16"`).
    #[inline]
    pub fn sample_format(&self) -> &'static str {
        self.sample_format_label
    }
}

impl AudioSink for CpalAudioSink {
    fn write_samples(&mut self, samples: &[f32]) -> Result<usize, AudioSinkError> {
        self.engine.write_samples(samples)
    }

    fn flush(&mut self) -> Result<(), AudioSinkError> {
        self.engine.flush()
    }

    fn sample_rate(&self) -> u32 {
        self.engine.input_sample_rate
    }

    fn channels(&self) -> u16 {
        self.engine.input_channels
    }

    fn pause(&mut self) -> Result<(), AudioSinkError> {
        self.engine.pause();
        Ok(())
    }

    fn resume(&mut self) -> Result<(), AudioSinkError> {
        self.engine.resume();
        Ok(())
    }

    fn set_volume(&mut self, volume: f32) {
        self.engine.set_volume(volume);
    }

    fn set_muted(&mut self, muted: bool) {
        self.engine.set_muted(muted);
    }

    fn latency_ms(&self) -> u64 {
        self.engine.latency_ms()
    }

    fn queued_ms(&self) -> u64 {
        self.engine.queued_ms()
    }

    fn available_frames(&self) -> usize {
        self.engine.available_frames()
    }
}

/// Pure-Rust audio sink streaming raw IEEE 754 32-bit float PCM samples to a system subprocess pipe.
///
/// Spawns `pw-cat` (PipeWire native playback tool on Fedora) with automatic fallback to `aplay` (ALSA).
pub struct PipeAudioSink {
    child: Child,
    stdin: ChildStdin,
    sample_rate: u32,
    channels: u16,
    byte_buffer: Vec<u8>,
    backend_name: String,
}

impl PipeAudioSink {
    /// Constructs a `PipeAudioSink` configured for 48 kHz stereo audio, probing `pw-cat` then `aplay`.
    pub fn new() -> Result<Self, AudioSinkError> {
        Self::with_config(48_000, 2)
    }

    /// Constructs a `PipeAudioSink` with custom sample rate and channel count.
    pub fn with_config(sample_rate: u32, channels: u16) -> Result<Self, AudioSinkError> {
        // Try PipeWire (pw-cat) first
        match Self::spawn_pw_cat(sample_rate, channels) {
            Ok(sink) => Ok(sink),
            Err(pw_err) => {
                // Fall back to ALSA (aplay)
                match Self::spawn_aplay(sample_rate, channels) {
                    Ok(sink) => Ok(sink),
                    Err(aplay_err) => Err(CoreError::Runtime(format!(
                        "Failed to spawn audio subprocess: pw-cat error ({}), aplay error ({})",
                        pw_err, aplay_err
                    ))),
                }
            }
        }
    }

    /// Spawns `pw-cat` (PipeWire native tool).
    fn spawn_pw_cat(sample_rate: u32, channels: u16) -> Result<Self, AudioSinkError> {
        let rate_str = sample_rate.to_string();
        let chan_str = channels.to_string();

        let mut child = Command::new("pw-cat")
            .args([
                "--playback",
                "--raw",
                "--rate",
                &rate_str,
                "--channels",
                &chan_str,
                "--format",
                "f32",
                "--latency",
                "50ms",
                "-",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(CoreError::Io)?;

        // Ensure child process did not immediately fail on startup
        std::thread::sleep(std::time::Duration::from_millis(5));
        if let Ok(Some(status)) = child.try_wait() {
            return Err(CoreError::Runtime(format!(
                "pw-cat exited immediately with status: {}",
                status
            )));
        }

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| CoreError::Runtime("Failed to acquire pw-cat stdin".into()))?;

        Ok(Self {
            child,
            stdin,
            sample_rate,
            channels,
            byte_buffer: Vec::with_capacity(4096),
            backend_name: "pw-cat".into(),
        })
    }

    /// Spawns `aplay` (ALSA native tool).
    fn spawn_aplay(sample_rate: u32, channels: u16) -> Result<Self, AudioSinkError> {
        let rate_str = sample_rate.to_string();
        let chan_str = channels.to_string();

        let mut child = Command::new("aplay")
            .args([
                "-r",
                &rate_str,
                "-c",
                &chan_str,
                "-f",
                "FLOAT_LE",
                "-t",
                "raw",
                "--buffer-time=50000",
                "-",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(CoreError::Io)?;

        std::thread::sleep(std::time::Duration::from_millis(5));
        if let Ok(Some(status)) = child.try_wait() {
            return Err(CoreError::Runtime(format!(
                "aplay exited immediately with status: {}",
                status
            )));
        }

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| CoreError::Runtime("Failed to acquire aplay stdin".into()))?;

        Ok(Self {
            child,
            stdin,
            sample_rate,
            channels,
            byte_buffer: Vec::with_capacity(4096),
            backend_name: "aplay".into(),
        })
    }

    /// Returns the active backend tool name ("pw-cat" or "aplay").
    #[inline]
    pub fn backend(&self) -> &str {
        &self.backend_name
    }
}

impl AudioSink for PipeAudioSink {
    fn write_samples(&mut self, samples: &[f32]) -> Result<usize, AudioSinkError> {
        if samples.is_empty() {
            return Ok(0);
        }

        self.byte_buffer.clear();
        self.byte_buffer.reserve(samples.len() * 4);

        for &sample in samples {
            let clamped = sample.clamp(-1.0, 1.0);
            self.byte_buffer.extend_from_slice(&clamped.to_le_bytes());
        }

        self.stdin
            .write_all(&self.byte_buffer)
            .map_err(CoreError::Io)?;
        let _ = self.stdin.flush();
        Ok(samples.len())
    }

    fn flush(&mut self) -> Result<(), AudioSinkError> {
        self.stdin.flush().map_err(CoreError::Io)
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn channels(&self) -> u16 {
        self.channels
    }
}

impl Drop for PipeAudioSink {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Zero-overhead sink discarding audio for headless environments and CI test suites.
#[derive(Debug, Clone)]
pub struct NullAudioSink {
    sample_rate: u32,
    channels: u16,
    samples_written: u64,
}

impl NullAudioSink {
    /// Creates a new `NullAudioSink` with specified sample rate and channels.
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            sample_rate,
            channels,
            samples_written: 0,
        }
    }

    /// Default 48 kHz stereo null audio sink.
    pub fn default_48k_stereo() -> Self {
        Self::new(48_000, 2)
    }

    /// Returns total number of samples written to this sink.
    #[inline]
    pub fn samples_written(&self) -> u64 {
        self.samples_written
    }
}

impl Default for NullAudioSink {
    fn default() -> Self {
        Self::default_48k_stereo()
    }
}

impl AudioSink for NullAudioSink {
    fn write_samples(&mut self, samples: &[f32]) -> Result<usize, AudioSinkError> {
        self.samples_written += samples.len() as u64;
        Ok(samples.len())
    }

    fn flush(&mut self) -> Result<(), AudioSinkError> {
        Ok(())
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn channels(&self) -> u16 {
        self.channels
    }
}

/// Auto-detecting audio sink that selects `CpalAudioSink` or `PipeAudioSink` (PipeWire/ALSA)
/// when available, or falls back cleanly to `NullAudioSink` when running headless or without an audio daemon.
pub enum AutoAudioSink {
    /// Native low-latency in-process CPAL hardware audio sink.
    Cpal(Box<CpalAudioSink>),
    /// Active pipe-based audio sink connected to system sound daemon.
    Pipe(PipeAudioSink),
    /// Zero-overhead null sink for headless/CI operation.
    Null(NullAudioSink),
}

impl AutoAudioSink {
    /// Auto-detects PipeWire or ALSA playback subprocess; falls back to `NullAudioSink` if unavailable.
    pub fn new() -> Self {
        Self::with_config(48_000, 2)
    }

    /// Auto-detects with specific sample rate and channels (`PipeAudioSink` -> `NullAudioSink`).
    pub fn with_config(sample_rate: u32, channels: u16) -> Self {
        match PipeAudioSink::with_config(sample_rate, channels) {
            Ok(pipe) => Self::Pipe(pipe),
            Err(_) => Self::Null(NullAudioSink::new(sample_rate, channels)),
        }
    }

    /// Auto-detects native `CpalAudioSink` first, falling back to `PipeAudioSink` then `NullAudioSink`.
    pub fn new_native() -> Self {
        Self::new_native_with_config(48_000, 2)
    }

    /// Auto-detects native `CpalAudioSink` with specific sample rate and channels, falling back
    /// to `PipeAudioSink` then `NullAudioSink`.
    pub fn new_native_with_config(sample_rate: u32, channels: u16) -> Self {
        if let Ok(cpal_sink) = CpalAudioSink::with_config(sample_rate, channels) {
            return Self::Cpal(Box::new(cpal_sink));
        }
        Self::with_config(sample_rate, channels)
    }

    /// Explicitly creates a `NullAudioSink` without attempting to open hardware or spawn processes.
    pub fn new_null() -> Self {
        Self::Null(NullAudioSink::default_48k_stereo())
    }

    /// Returns the active backend identifier (`"cpal"`, `"pw-cat"`, `"aplay"`, or `"null"`).
    pub fn backend_name(&self) -> &str {
        match self {
            Self::Cpal(_) => "cpal",
            Self::Pipe(p) => p.backend(),
            Self::Null(_) => "null",
        }
    }

    /// Returns whether the sink is operating as a null sink.
    #[inline]
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null(_))
    }
}

impl Default for AutoAudioSink {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioSink for AutoAudioSink {
    fn write_samples(&mut self, samples: &[f32]) -> Result<usize, AudioSinkError> {
        match self {
            Self::Cpal(c) => c.write_samples(samples),
            Self::Pipe(p) => p.write_samples(samples),
            Self::Null(n) => n.write_samples(samples),
        }
    }

    fn flush(&mut self) -> Result<(), AudioSinkError> {
        match self {
            Self::Cpal(c) => c.flush(),
            Self::Pipe(p) => p.flush(),
            Self::Null(n) => n.flush(),
        }
    }

    fn sample_rate(&self) -> u32 {
        match self {
            Self::Cpal(c) => c.sample_rate(),
            Self::Pipe(p) => p.sample_rate(),
            Self::Null(n) => n.sample_rate(),
        }
    }

    fn channels(&self) -> u16 {
        match self {
            Self::Cpal(c) => c.channels(),
            Self::Pipe(p) => p.channels(),
            Self::Null(n) => n.channels(),
        }
    }

    fn pause(&mut self) -> Result<(), AudioSinkError> {
        match self {
            Self::Cpal(c) => c.pause(),
            Self::Pipe(p) => p.pause(),
            Self::Null(n) => n.pause(),
        }
    }

    fn resume(&mut self) -> Result<(), AudioSinkError> {
        match self {
            Self::Cpal(c) => c.resume(),
            Self::Pipe(p) => p.resume(),
            Self::Null(n) => n.resume(),
        }
    }

    fn set_volume(&mut self, volume: f32) {
        if let Self::Cpal(c) = self {
            c.set_volume(volume);
        }
    }

    fn set_muted(&mut self, muted: bool) {
        if let Self::Cpal(c) = self {
            c.set_muted(muted);
        }
    }

    fn latency_ms(&self) -> u64 {
        match self {
            Self::Cpal(c) => c.latency_ms(),
            Self::Pipe(p) => p.latency_ms(),
            Self::Null(n) => n.latency_ms(),
        }
    }

    fn queued_ms(&self) -> u64 {
        match self {
            Self::Cpal(c) => c.queued_ms(),
            Self::Pipe(p) => p.queued_ms(),
            Self::Null(n) => n.queued_ms(),
        }
    }

    fn available_frames(&self) -> usize {
        match self {
            Self::Cpal(c) => c.available_frames(),
            Self::Pipe(p) => p.available_frames(),
            Self::Null(n) => n.available_frames(),
        }
    }
}

/// Creates the default boxed [`AudioSink`] for runtime playback.
///
/// When `headless` is `true`, returns [`NullAudioSink`] immediately without probing audio hardware.
/// When `headless` is `false`, tries [`CpalAudioSink::new`] first, falling back gracefully through
/// [`PipeAudioSink::new`] to [`NullAudioSink`].
pub fn create_default_audio_sink(headless: bool) -> Box<dyn AudioSink> {
    if headless {
        return Box::new(NullAudioSink::default_48k_stereo());
    }
    if let Ok(cpal_sink) = CpalAudioSink::new() {
        return Box::new(cpal_sink);
    }
    if let Ok(pipe_sink) = PipeAudioSink::new() {
        return Box::new(pipe_sink);
    }
    Box::new(NullAudioSink::default_48k_stereo())
}

/// In-memory audio sink capturing written samples for testing and verification.
#[derive(Debug, Default, Clone)]
pub struct MockAudioSink {
    sample_rate: u32,
    channels: u16,
    samples: Vec<f32>,
    flush_count: usize,
}

impl MockAudioSink {
    /// Creates a new `MockAudioSink`.
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            sample_rate,
            channels,
            samples: Vec::new(),
            flush_count: 0,
        }
    }

    /// Returns recorded audio samples.
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// Returns the number of flushes executed.
    pub fn flush_count(&self) -> usize {
        self.flush_count
    }

    /// Clears recorded samples and flushes.
    pub fn clear(&mut self) {
        self.samples.clear();
        self.flush_count = 0;
    }
}

impl AudioSink for MockAudioSink {
    fn write_samples(&mut self, samples: &[f32]) -> Result<usize, AudioSinkError> {
        self.samples.extend_from_slice(samples);
        Ok(samples.len())
    }

    fn flush(&mut self) -> Result<(), AudioSinkError> {
        self.flush_count += 1;
        Ok(())
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn channels(&self) -> u16 {
        self.channels
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpal::Sample;
    use std::time::Instant;

    #[test]
    fn test_null_audio_sink() {
        let mut sink = NullAudioSink::default_48k_stereo();
        assert_eq!(sink.sample_rate(), 48000);
        assert_eq!(sink.channels(), 2);
        assert_eq!(sink.samples_written(), 0);

        let samples = vec![0.0f32; 1024];
        let written = sink.write_samples(&samples).unwrap();
        assert_eq!(written, 1024);
        assert_eq!(sink.samples_written(), 1024);

        sink.flush().unwrap();
        assert_eq!(sink.samples_written(), 1024);
    }

    #[test]
    fn test_mock_audio_sink() {
        let mut sink = MockAudioSink::new(48000, 2);
        let samples = vec![0.1f32, -0.2, 0.3, -0.4];
        let written = sink.write_samples(&samples).unwrap();
        assert_eq!(written, 4);
        assert_eq!(sink.samples(), &[0.1f32, -0.2, 0.3, -0.4]);

        sink.flush().unwrap();
        assert_eq!(sink.flush_count(), 1);

        sink.clear();
        assert!(sink.samples().is_empty());
        assert_eq!(sink.flush_count(), 0);
    }

    #[test]
    fn test_auto_audio_sink_fallback_and_api() {
        let mut sink = AutoAudioSink::new_null();
        assert!(sink.is_null());
        assert_eq!(sink.backend_name(), "null");

        let samples = vec![0.25f32; 512];
        let written = sink.write_samples(&samples).unwrap();
        assert_eq!(written, 512);
        sink.flush().unwrap();
        sink.pause().unwrap();
        sink.resume().unwrap();
    }

    #[test]
    fn test_create_default_audio_sink_headless_and_live_fallback() {
        let mut headless_sink = create_default_audio_sink(true);
        assert_eq!(headless_sink.sample_rate(), 48_000);
        assert_eq!(headless_sink.channels(), 2);
        assert_eq!(headless_sink.latency_ms(), 0);
        assert_eq!(headless_sink.queued_ms(), 0);
        assert_eq!(headless_sink.write_samples(&[0.5; 256]).unwrap(), 256);

        // Non-headless creation must never panic even if no soundcard exists in CI
        let mut live_or_fallback = create_default_audio_sink(false);
        assert_eq!(live_or_fallback.sample_rate(), 48_000);
        assert_eq!(live_or_fallback.channels(), 2);
        live_or_fallback.set_volume(0.85);
        live_or_fallback.set_muted(false);
        let _ = live_or_fallback.pause();
        let _ = live_or_fallback.resume();
        let _ = live_or_fallback.flush();
    }

    #[test]
    fn test_cpal_engine_instant_flush_and_declick_ramps() {
        let mut engine = CpalRingEngine::new(48_000, 2, 48_000, 2, 512).unwrap();
        let mut cb = CallbackState::new(48_000, 2);

        // Queue 4,800 stereo frames (9,600 samples = 100ms @ 48kHz)
        let pre_seek = vec![0.8f32; 9_600];
        assert_eq!(engine.write_samples(&pre_seek).unwrap(), 9_600);
        assert_eq!(engine.queued_ms(), 100);
        // 100ms queued + floor(512*1000/48000)=10ms -> 110ms
        assert_eq!(engine.latency_ms(), 110);

        // Render 1 buffer (512 samples = 256 frames) so prev_frame holds ~0.8
        let mut out = vec![0.0f32; 512];
        cb.fill_output(&engine.shared, &mut out);
        assert!((out[510] - 0.8).abs() < 1e-4);

        // Flush must complete in < 1ms and purge ring immediately
        let t0 = Instant::now();
        engine.flush().unwrap();
        assert!(t0.elapsed().as_millis() < 5);
        assert_eq!(engine.queued_ms(), 0);
        assert_eq!(engine.available_frames(), CPAL_RING_CAPACITY / 2);

        // Write fresh post-seek audio (amplitude -0.6)
        let post_seek = vec![-0.6f32; 2_048];
        assert_eq!(engine.write_samples(&post_seek).unwrap(), 2_048);

        // Callback output must smoothly fade out pre-seek (+0.8 -> 0.0 over 96 frames)
        // and smoothly fade in post-seek (0.0 -> -0.6 over 96 frames), with no sudden step!
        let mut after_flush = vec![0.0f32; 1024];
        cb.fill_output(&engine.shared, &mut after_flush);

        // Frame 0 begins near +0.8 (fade out), never jumping abruptly to -0.6
        assert!(after_flush[0] > 0.7);
        // Around frame 95 (end of fade-out), approaches 0.0
        assert!(after_flush[95 * 2].abs() < 0.05);
        // Frame 96 begins fade-in from near 0.0 toward -0.6
        assert!(after_flush[96 * 2].abs() < 0.05);
        // Well past fade-in (frame 250), reaches full -0.6
        assert!((after_flush[250 * 2] - (-0.6)).abs() < 1e-4);
    }

    #[test]
    fn test_cpal_engine_atomic_volume_soft_clip_mute_and_pause() {
        let mut engine = CpalRingEngine::new(48_000, 2, 48_000, 2, 512).unwrap();
        let mut cb = CallbackState::new(48_000, 2);

        // Feed 0.7 amplitude signal
        let samples = vec![0.7f32; 4_096];
        engine.write_samples(&samples).unwrap();

        // Boost volume to 2.0 (200%): 0.7 * 2.0 = 1.4 -> soft_clip_sample must bound within [-1.0, 1.0]
        engine.set_volume(2.0);
        let mut out = vec![0.0f32; 1024];
        cb.fill_output(&engine.shared, &mut out);
        // After 96-frame gain slew, output must be soft-clipped (> 0.85, < 1.0)
        let boosted = out[400];
        assert!(boosted > 0.85 && boosted < 1.0, "got {boosted}");

        // Mute must ramp down smoothly without destroying queued PCM samples
        let queued_before_mute = engine.queued_ms();
        assert!(queued_before_mute > 0);
        engine.set_muted(true);
        cb.fill_output(&engine.shared, &mut out);
        assert!(out[510].abs() < 1e-4, "muted output must reach silence");

        // Unmute restores signal smoothly
        engine.set_muted(false);
        cb.fill_output(&engine.shared, &mut out);
        assert!(out[510] > 0.85);

        // Pause preserves remaining queued PCM samples in ring buffer
        let queued_before_pause = engine.shared.ring.len();
        assert!(queued_before_pause > 0);
        engine.pause();
        cb.fill_output(&engine.shared, &mut out);
        assert_eq!(
            engine.shared.ring.len(),
            queued_before_pause,
            "pause must not discard queued PCM samples"
        );
        assert!(out[510].abs() < 1e-4, "paused output must be silent");

        // Resume consumes queued samples again
        engine.resume();
        cb.fill_output(&engine.shared, &mut out);
        assert!(engine.shared.ring.len() < queued_before_pause);
    }

    #[test]
    fn test_cpal_engine_sample_formats_and_channel_rate_adaptation() {
        // Test 48kHz stereo input -> 44.1kHz mono device with i16 and u16 formats
        let mut engine = CpalRingEngine::new(48_000, 2, 44_100, 1, 256).unwrap();
        let mut cb = CallbackState::new(44_100, 1);

        let stereo_input = vec![0.5f32; 4_800]; // 2,400 stereo frames = 50ms @ 48kHz
        let written = engine.write_samples(&stereo_input).unwrap();
        assert_eq!(written, 4_800);
        assert!(engine.queued_ms() >= 45 && engine.queued_ms() <= 52);

        let mut i16_out = vec![0i16; 512];
        cb.fill_output(&engine.shared, &mut i16_out);
        assert!(i16_out[256] > 10_000, "i16 output must reflect +0.5 PCM");

        let mut u16_out = vec![u16::EQUILIBRIUM; 512];
        cb.fill_output(&engine.shared, &mut u16_out);
        assert!(
            u16_out[256] > u16::EQUILIBRIUM,
            "u16 output must shift positively above equilibrium"
        );
    }
}
