//! Pure-Rust Linux audio sink interface, PipeWire/ALSA subprocess pipe streaming, and null fallback.
//!
//! Provides zero-external-dependency audio playback on Fedora Linux by streaming raw IEEE 754 32-bit
//! float PCM (`f32`, 48 kHz stereo) directly to `/usr/bin/pw-cat` (PipeWire) with automatic fallback to
//! `/usr/bin/aplay` (ALSA) and `NullAudioSink` (for headless CI and containerized environments).

use std::io::Write;
use std::process::{Child, ChildStdin, Command, Stdio};

use crate::error::CoreError;

/// Core interface for audio hardware or daemon output sinks.
pub trait AudioSink: Send {
    /// Writes interleaved 32-bit float PCM samples to the sink.
    /// Returns the number of samples written.
    fn write_samples(&mut self, samples: &[f32]) -> Result<usize, CoreError>;

    /// Flushes any pending buffered audio to the hardware or playback daemon.
    fn flush(&mut self) -> Result<(), CoreError>;

    /// Target sample rate in Hz (default 48,000).
    fn sample_rate(&self) -> u32 {
        48_000
    }

    /// Number of audio channels (default 2 for stereo).
    fn channels(&self) -> u16 {
        2
    }

    /// Pauses audio output if supported.
    fn pause(&mut self) -> Result<(), CoreError> {
        Ok(())
    }

    /// Resumes audio output if supported.
    fn resume(&mut self) -> Result<(), CoreError> {
        Ok(())
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
    pub fn new() -> Result<Self, CoreError> {
        Self::with_config(48_000, 2)
    }

    /// Constructs a `PipeAudioSink` with custom sample rate and channel count.
    pub fn with_config(sample_rate: u32, channels: u16) -> Result<Self, CoreError> {
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
    fn spawn_pw_cat(sample_rate: u32, channels: u16) -> Result<Self, CoreError> {
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
    fn spawn_aplay(sample_rate: u32, channels: u16) -> Result<Self, CoreError> {
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
    fn write_samples(&mut self, samples: &[f32]) -> Result<usize, CoreError> {
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

    fn flush(&mut self) -> Result<(), CoreError> {
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
    fn write_samples(&mut self, samples: &[f32]) -> Result<usize, CoreError> {
        self.samples_written += samples.len() as u64;
        Ok(samples.len())
    }

    fn flush(&mut self) -> Result<(), CoreError> {
        Ok(())
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn channels(&self) -> u16 {
        self.channels
    }
}

/// Auto-detecting audio sink that selects `PipeAudioSink` (PipeWire/ALSA) when available,
/// or falls back cleanly to `NullAudioSink` when running headless or without audio daemon.
pub enum AutoAudioSink {
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

    /// Auto-detects with specific sample rate and channels.
    pub fn with_config(sample_rate: u32, channels: u16) -> Self {
        match PipeAudioSink::with_config(sample_rate, channels) {
            Ok(pipe) => Self::Pipe(pipe),
            Err(_) => Self::Null(NullAudioSink::new(sample_rate, channels)),
        }
    }

    /// Explicitly creates a `NullAudioSink` without attempting to spawn any processes.
    pub fn new_null() -> Self {
        Self::Null(NullAudioSink::default_48k_stereo())
    }

    /// Returns the active backend identifier ("pw-cat", "aplay", or "null").
    pub fn backend_name(&self) -> &str {
        match self {
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
    fn write_samples(&mut self, samples: &[f32]) -> Result<usize, CoreError> {
        match self {
            Self::Pipe(p) => p.write_samples(samples),
            Self::Null(n) => n.write_samples(samples),
        }
    }

    fn flush(&mut self) -> Result<(), CoreError> {
        match self {
            Self::Pipe(p) => p.flush(),
            Self::Null(n) => n.flush(),
        }
    }

    fn sample_rate(&self) -> u32 {
        match self {
            Self::Pipe(p) => p.sample_rate(),
            Self::Null(n) => n.sample_rate(),
        }
    }

    fn channels(&self) -> u16 {
        match self {
            Self::Pipe(p) => p.channels(),
            Self::Null(n) => n.channels(),
        }
    }

    fn pause(&mut self) -> Result<(), CoreError> {
        match self {
            Self::Pipe(p) => p.pause(),
            Self::Null(n) => n.pause(),
        }
    }

    fn resume(&mut self) -> Result<(), CoreError> {
        match self {
            Self::Pipe(p) => p.resume(),
            Self::Null(n) => n.resume(),
        }
    }
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
    fn write_samples(&mut self, samples: &[f32]) -> Result<usize, CoreError> {
        self.samples.extend_from_slice(samples);
        Ok(samples.len())
    }

    fn flush(&mut self) -> Result<(), CoreError> {
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
}
