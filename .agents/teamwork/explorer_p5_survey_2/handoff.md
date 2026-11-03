# Comprehensive Technical Audit Report: Multimedia, Decoding, Timing & Synchronization Core

**Author**: `explorer_p5_survey_2` (Teamwork Explorer)  
**Date**: 2026-10-30T22:00:00Z  
**Workspace**: `/home/gautham/syncverse`  
**Target Crates**: `syncverse-demux`, `syncverse-codecs`, `syncverse-clock`, `syncverse-sync` (and integration in `syncverse-core`)  
**Status**: COMPLETE (Hard Handoff)

---

## 1. Executive Summary & Verification Baseline

A comprehensive technical audit was executed across the four foundational multimedia, decoding, presentation timing, and network synchronization crates of Syncverse:
- `crates/syncverse-demux`: Universal demuxer (`ffmpeg-next`), in-tree Matroska/EBML and ISOBMFF demuxers, subtitle parsers (SRT, WebVTT, ASS/SSA), and filesystem scanner.
- `crates/syncverse-codecs`: Universal video decoders (H.264, HEVC, VP8, VP9, AV1, MPEG-2/4, VC-1), universal audio decoders (AAC, MP3, Opus, Vorbis, FLAC, AC3/E-AC3, DTS, PCM), memory-aligned frame representations, Q8 fixed-point color space converters, and pure-Rust `LinearResampler`.
- `crates/syncverse-clock`: `MasterClock` SeqLock with $C^0$ continuous re-anchoring, `AudioPacer` zero-allocation underrun protection, 128-byte cache-line aligned `SpscRingBuffer`, and `PresentationReorderBuffer`.
- `crates/syncverse-sync`: NTP 4-timestamp exchange, `Rfc5905Filter` minimum RTT sliding window, deterministic reference timeline equation, discrete PI PLL micro-speed controller ($r \in [0.96, 1.04]$), two-phase barrier unpause, and binary wire protocol (`SPNT`).

### Baseline Verification Status:
All existing tests, invariant verifications, and empirical challenge harnesses pass with 100% compliance:
1. `./scripts/audit_dependencies.sh`: **PASS** (19 banned crates checked, 0 violations; zero monolithic player IPC wrappers and zero database engines).
2. `cargo check --workspace --all-targets`: **PASS** (0 warnings under `#![deny(warnings)]`).
3. `cargo test --workspace -- --nocapture`: **PASS** (100% passing across all 8 crates).
4. `cargo clippy --workspace --all-targets -- -D warnings`: **PASS** (0 warnings).
5. `python3 tests/empirical_challenge/test_demux_codecs_clock.py`: **PASS** (0 failures).
6. `python3 tests/empirical_challenge/test_sync_engine.py`: **PASS** (0 failures).
7. `python3 scripts/intent_harness.py`: **PASS** (8/8 intents verified, 100% compliance score).

---

## 2. Deep Subsystem Technical Inspection

### 2.1 Container Demuxing (`crates/syncverse-demux/`)

#### Architecture & Implementation:
- **`UniversalDemuxer` (`src/universal.rs`)**:
  - Direct bindings via `ffmpeg-next` (`libavformat`).
  - Supports container probing across MKV, WebM, MP4, MOV, AVI, MPEG-TS, FLV, and OGG via `av_probe_input_format` backed by container magic heuristics.
  - Stream enumeration discovers Video, Audio, and Subtitle tracks, extracting codec parameters, dimensions, sample rates, channels, `codec_private` extradata, and language/title metadata.
  - Timestamp normalization (`normalize_packet`) computes zero-based millisecond presentation timestamps ($t_{\text{pts}}$) and decode timestamps ($t_{\text{dts}}$) using 128-bit integer arithmetic:
    $t_{\text{pts\_us}} = \frac{t_{\text{raw}} \cdot \text{num} \cdot 1\,000\,000}{\text{den}} - t_{\text{start\_us}}$.
  - Keyframe seeking (`seek_to`) executes bidirectional bounded seeking with microsecond precision against `stream_target_us = target_pts_ms * 1000 + start_time_us`, falling back to earliest available keyframe if seeking backwards from zero/priming delay fails.
- **In-Tree Demuxers (`src/ebml.rs`, `src/isobmff.rs`)**:
  - `EbmlDemuxer`: Pure-Rust Matroska/WebM parser with VINT parsing, Element ID decoding, Cues seek table parsing, and Xiph/Fixed/EBML block lacing unpacking.
  - `IsoBmffDemuxer`: Pure-Rust MP4/ISOBMFF parser with 64-bit largesize bounds checking, sample table resolver (`stts`, `stsc`, `stsz`, `stco`, `co64`, `stss`), and synthetic test vectors.
- **Subtitle Parsers & Scanner (`src/subtitles/`)**:
  - Pure-Rust parsers for SRT, WebVTT, and ASS/SSA.
  - Automatic character encoding detection for UTF-8 with BOM, UTF-16LE, UTF-16BE, standard UTF-8, and Windows-1252/ISO-8859-1 fallback.
  - Automatic language token detection from filename relative to media stem (e.g. `video.en.srt`, `video.eng.sdh.srt`).

---

### 2.2 Decoding & Frame Buffers (`crates/syncverse-codecs/`)

#### Architecture & Implementation:
- **Universal Video Decoder (`src/universal_video.rs`)**:
  - Handles H.264, H.265/HEVC, VP8, VP9, AV1, MPEG-2, MPEG-4 Part 2, and VC-1.
  - Formats all decoded frames into memory-aligned `VideoFrameYUV420p` structures.
  - Implements fast preroll draining via `receive_preroll_or_rgba(min_pts_ms)` to discard preroll frames prior to seek target without expensive pixel conversions.
- **Strict Memory Alignments (`src/frame.rs`)**:
  - `AlignedBuffer`: 64-byte aligned heap buffer (`Layout::from_size_align(capacity, 64)`).
  - `VideoFrameYUV420p`:
    - Luminance ($Y$) row stride aligned to 16 bytes: `stride_y = (width + 15) & !15`.
    - Chroma ($U, V$) row strides aligned to 16 bytes: `stride_u = stride_v = (((width + 1) / 2) + 15) & !15`.
    - SIMD end-of-buffer tail padding: Plane capacity allocations allocate `stride * height + 16` bytes, preventing vector over-reads.
  - `VideoFrameRGBA`: Packed 32-bit RGBA with 64-byte row alignment `(((width * 4) + 63) & !63)` and Alpha channel pre-initialized to 255.
  - `AudioFramePCM`: Normalized 32-bit floating-point interleaved audio samples in linear range `[-1.0, 1.0]`.
- **Q8 Fixed-Point Color Space Converter (`src/color.rs`)**:
  - ITU-R BT.601 and BT.709 limited-range to full-range RGBA conversions.
  - Uses 2x2 chroma reuse processing 4 luma pixels per chroma load.
  - Zero heap allocations in hot loops; branchless integer clamping `clamp_u8(val)`.
- **Universal Audio Decoder (`src/audio/universal.rs`)**:
  - Powered by `ffmpeg-next` (`libavcodec` and `libswresample`).
  - Decodes AAC, MP3, Opus, Vorbis, FLAC, AC3, E-AC3, DTS, and uncompressed PCM.
  - Standardizes all output audio to target sample rate 48,000 Hz, 2 channels (stereo), interleaved 32-bit float samples.
  - Computes continuous output sample PTS anchored to packet presentation timestamp with sample accumulation.
- **Pure-Rust `LinearResampler` (`src/audio/resampler.rs`)**:
  - Fractional phase accumulation across arbitrary chunk boundaries:
    $step = \frac{\text{in\_rate} \cdot r}{\text{out\_rate}}$.
  - Zero heap allocations during steady-state processing (`resample_into`).

---

### 2.3 Master Presentation Clock & Audio Pacing (`crates/syncverse-clock/`)

#### Architecture & Implementation:
- **`MasterClock` (`src/master_clock.rs`)**:
  - Master presentation clock tracking continuous presentation time `current_pts(now)`.
  - Concurrency protected by SeqLock: readers perform lock-free optimistic reads via `AtomicUsize` sequence counter; writers serialized by `Mutex<()>`.
  - Rate re-anchoring `reanchor(now, new_speed)` recalculates `anchor_pts` at the transition instant, guaranteeing $C^0$ mathematical continuity with zero discontinuous jumps across speed transitions.
  - Presentation timing evaluator `evaluate_presentation_timing(frame_pts, now)`:
    - $\Delta > +10\text{ms}$: `Wait(Duration)`
    - $-15\text{ms} \le \Delta \le +10\text{ms}$: `PresentNow`
    - $-100\text{ms} < \Delta < -15\text{ms}$: `PresentNowLate`
    - $\Delta \le -100\text{ms}$: `DropLateFrame`
- **`AudioPacer` (`src/audio_pacer.rs`)**:
  - Paces audio against `MasterClock` presentation time with user-adjustable delay offset (`audio_delay_ms`).
  - Pre-allocated silence frame template transferred over `SpscRingBuffer<AudioFramePCM, 64>` on underruns to prevent thread starvation.
  - Smooth 2ms linear ramp-in on the first valid frame when recovering from an underrun to prevent audible DC pops/clicks.
  - Zero-allocation steady-state buffer filling (`fill_samples`).
- **`SpscRingBuffer` (`src/spsc.rs`)**:
  - Power-of-two capacity with bitwise masking.
  - Head and tail atomic counters isolated on separate 128-byte cache lines (`#[repr(align(128))]`) to eliminate false sharing.
  - Lock-free Acquire-Release memory ordering semantics.
- **`PresentationReorderBuffer` (`src/reorder.rs`)**:
  - Bounded min-heap ($K = 16$) ordering decode-order frames into ascending presentation order by PTS.

---

### 2.4 High-RTT Network Synchronization (`crates/syncverse-sync/`)

#### Architecture & Implementation:
- **NTP Probe & RFC 5905 Filter (`src/ntp.rs`)**:
  - 4-timestamp exchange ($T_1$: client transmit, $T_2$: server receive, $T_3$: server transmit, $T_4$: client receive).
  - Offset: $\theta = \frac{(T_2 - T_1) + (T_3 - T_4)}{2}$.
  - Round-trip time: $\text{RTT} = (T_4 - T_1) - (T_3 - T_2)$.
  - 8-sample sliding window minimum RTT filter with effective dispersion floor:
    $D_{\text{effective}} = \max(D, 5.0\text{ms})$.
  - Outlier rejection: rejects candidate samples if $\text{RTT} > 2.0 \cdot \text{med}(\text{RTT})$ or $|\theta - \hat{\theta}| > 3.0 \cdot D_{\text{effective}}$.
  - Watchdog: 8 consecutive rejections flush stale window, ensuring rapid recovery from network route changes.
- **Reference Timeline Equation (`src/timeline.rs`)**:
  - $\tau_{\text{target}}(t_{\text{local}}) = \tau_{\text{ref\_media}} + r_{\text{target}} \cdot \left[ (t_{\text{local}} + \hat{\theta}) - t_{\text{ref\_server}} \right]$.
- **Discrete PI PLL Micro-Speed Controller (`src/pll.rs`)**:
  - Gains: $K_p = 0.17\text{ s}^{-1}, K_i = 0.0072\text{ s}^{-2}$, anti-windup clamping at $i_{\text{max}} = 32.0\text{s}$.
  - Tri-Zone drift policy:
    - Zone 1: In-sync deadband ($|e| < 15\text{ms}$): speed held constant; integral accumulator frozen to prevent hunting.
    - Zone 2: Continuous micro-speed slew ($15\text{ms} \le |e| \le 1500\text{ms}$): speed modulated in range $[0.96, 1.04]$.
    - Zone 3: Discontinuous hard seek ($|e| > 1500\text{ms}$): reset integral and command hard seek.
- **Two-Phase Scheduled Barrier Unpause (`src/barrier.rs`)**:
  - Authoritative target: $T_{\text{target}} = t_{\text{server\_now}} + 1.5 \cdot \max(\text{RTT}) + t_{\text{preroll\_budget}} + 100\text{ms}$.
  - Guarantees $\ge 100\text{ms}$ positive commit arrival margin across all peers and cross-peer alignment $< 10\text{ms}$.
- **Binary Wire Protocol (`src/wire.rs`)**:
  - 12-byte fixed header: Magic `SPNT` (4B), `packet_type` (1B), `flags` (1B), `payload_len` (2B), CRC32 digest (4B).
  - Strongly typed payloads: `NtpProbeRequest` (0x01), `NtpProbeResponse` (0x02), `RoomStateBroadcast` (0x03), `BarrierPrepare` (0x04), `BarrierReady` (0x05), `SeekCommand` (0x06), `ChatMessage` (0x08), `PeerStatus` (0x09), `Reaction` (0x0A).

---

## 3. Concrete Opportunities for Improvement & Defects Discovered

### 3.1 Defect 1: Critical Architectural Breakdown for Non-1.0x Playback Speeds in Watch-Party Sync
- **Location**: `crates/syncverse-sync/src/pll.rs:98-99`, `crates/syncverse-sync/src/transport.rs:945-965`, and `crates/syncverse-core/src/playback_runtime.rs:2079-2091`.
- **Direct Observation**:
  In `crates/syncverse-sync/src/pll.rs:98-99`:
  ```rust
  let delta_r = self.kp * e + self.ki * self.integral;
  let speed = (1.0 + delta_r).clamp(self.r_min, self.r_max);
  ```
  Where `self.r_min = 0.96` and `self.r_max = 1.04`.
  And in `crates/syncverse-core/src/playback_runtime.rs:2085-2091`:
  ```rust
  } else if drift_ms <= 15 && clock.is_playing() {
      // Zone 1: Deadband - normal speed
      if (speed - 1.0).abs() > 1e-4 {
          speed = 1.0;
          clock.reanchor(now, 1.0);
      }
  }
  ```
- **Analysis**:
  The wire protocol `RoomStatePayload` already supports non-1.0x playback speeds via `target_speed_q16: u32` (`r_target = target_speed_q16 / 65536.0`). The reference timeline equation in `RoomState::compute_target_media_pts` correctly scales server elapsed time by `r_target`.
  However, both `PllSpeedController::update()` and `playback_runtime.rs` hardcode a base speed of 1.0x!
  If the host sets playback speed to 1.25x or 1.5x:
  1. The host broadcasts `r_target = 1.5` in `RoomStateBroadcast`.
  2. The client calculates target presentation time progressing at 1.5 seconds per second.
  3. The client's PLL controller clamps speed to `[0.96, 1.04]`, because it adds `delta_r` to `1.0` rather than `r_target`.
  4. The client falls behind by 460ms every second.
  5. In Zone 1 (if reached initially), the client resets its speed to 1.0x.
  6. Within ~3.2 seconds, `drift_ms` exceeds 1500ms, triggering a Zone 3 hard seek. The client hard seeks, resumes, falls behind again, and repeats this cycle infinitely every ~3.2 seconds.
- **Recommended Remediation**:
  1. Update `PllSpeedController` to accept or configure `r_target` (e.g. `pub fn update_with_target_rate(&mut self, target_pts_ms: u64, actual_pts_ms: u64, dt_secs: f64, r_target: f64) -> PllCorrection`).
  2. Scale the micro-speed steering range relative to `r_target`:
     $\text{steered\_speed} = (r_{\text{target}} \cdot (1.0 + \Delta r)).\text{clamp}(r_{\text{target}} \cdot 0.96, r_{\text{target}} \cdot 1.04)$.
  3. In Zone 1 (Deadband), return `speed_multiplier: r_target`.
  4. In `playback_runtime.rs:2085-2091`, reanchor client clock to `room_state.r_target` when in Zone 1:
     ```rust
     } else if drift_ms <= 15 && clock.is_playing() {
         let target_speed = room_state.r_target;
         if (speed - target_speed).abs() > 1e-4 {
             speed = target_speed;
             clock.reanchor(now, target_speed);
         }
     }
     ```

---

### 3.2 Defect 2: AGENTS.md Invariant Violation in Chapter Timescale Arithmetic
- **Location**: `crates/syncverse-demux/src/universal.rs:207-210`.
- **Direct Observation**:
  ```rust
  let tb = ch.time_base();
  let tb_f64 = f64::from(tb);
  let start_ms = (ch.start() as f64 * tb_f64 * 1000.0).max(0.0) as u64;
  let end_ms = (ch.end() as f64 * tb_f64 * 1000.0).max(0.0) as u64;
  ```
- **Analysis**:
  AGENTS.md Section 4 Rule 6 explicitly states:
  > *"Use 64-bit integer arithmetic for container timescale conversions to avoid float precision loss or 32-bit integer overflows."*
  While `normalize_packet` in `universal.rs:255-265` uses `i128` integer math, chapter parsing relies on `f64`. For long media files or high-precision container timescales (e.g., 90,000 Hz or 1,000,000,000 Hz), float conversions introduce rounding inaccuracies.
- **Recommended Remediation**:
  Replace with 128-bit rational integer conversion:
  ```rust
  let num = tb.numerator() as i128;
  let den = if tb.denominator() != 0 { tb.denominator() as i128 } else { 1 };
  let start_ms = ((ch.start() as i128 * num * 1000) / den).max(0) as u64;
  let end_ms = ((ch.end() as i128 * num * 1000) / den).max(0) as u64;
  ```

---

### 3.3 Defect 3: Digital Clipping and Distortion on Multichannel Audio Downmixing
- **Location**: `crates/syncverse-codecs/src/audio/universal.rs:326` and `crates/syncverse-core/src/playback_runtime.rs:57-65`.
- **Direct Observation**:
  In `universal.rs:326`:
  ```rust
  let samples: Vec<f32> = plane.iter().map(|&s| s.clamp(-1.0, 1.0)).collect();
  ```
  In `playback_runtime.rs:63`:
  ```rust
  samples.iter().map(|s| (s * volume).clamp(-1.0, 1.0)).collect()
  ```
- **Analysis**:
  When decoding 5.1 or 7.1 surround sound audio (e.g. AC3, DTS, AAC 5.1), `libswresample` sums channels into stereo ($L = L + 0.707 C + 0.707 SL$). High-action sequences with heavy sound effects routinely exceed the 0 dBFS amplitude threshold ($\sum > 1.0$).
  Hard clamping (`s.clamp(-1.0, 1.0)`) creates severe flat-top waveform clipping, introducing harsh odd-harmonic distortion (audible buzz/crackle).
- **Recommended Remediation**:
  Implement a smooth, zero-allocation soft-knee saturation curve (e.g., cubic limiter or `tanh` compression) for samples where $|s| > 0.85$:
  ```rust
  #[inline(always)]
  pub fn soft_clip_sample(s: f32) -> f32 {
      if s.abs() <= 0.85 {
          s
      } else if s > 0.85 {
          0.85 + 0.15 * ((s - 0.85) / 0.15).tanh()
      } else {
          -0.85 - 0.15 * ((-s - 0.85) / 0.15).tanh()
      }
  }
  ```
  This preserves bit-exact audio reproduction for standard volume while providing smooth analog-like compression for loud peaks, completely eliminating harsh clipping.

---

### 3.4 Defect 4: Soundcard Starvation & Underrun Resilience in Playback Worker
- **Location**: `crates/syncverse-core/src/playback_runtime.rs:135-177`.
- **Direct Observation**:
  ```rust
  // 3. If pending queue is empty, wait briefly for new commands
  if pending_queue.is_empty() {
      if let Ok(cmd) = rx.recv_timeout(Duration::from_millis(5)) {
          ...
      }
      continue;
  }
  ```
- **Analysis**:
  When video decoding experiences an IO stall (e.g. reading a massive 4K intra-frame) or network delay, `pending_queue` in `run_audio_sink_worker` can empty. While `AudioPacer` in `syncverse-clock` was designed to inject silence frames on underrun to keep the soundcard buffer fed, `run_audio_sink_worker` does not use `AudioPacer`—it simply sleeps.
  If the soundcard hardware buffer drains completely before the next audio packet is decoded, ALSA/PipeWire suffers an underrun, causing an audible hardware click or pop.
- **Recommended Remediation**:
  Integrate `AudioPacer` or an underrun detector in `run_audio_sink_worker`: if `pending_queue` is empty for $> 15\text{ms}$ while playing, write a short silence buffer with a 2ms linear ramp-down to prevent soundcard driver underrun clicks.

---

## 4. Proposed High-Value Feature Enhancements

### 4.1 Feature 1: Subdirectory External Subtitle Scanning (`subtitles/`, `subs/`)
- **Scope**: `crates/syncverse-demux/src/subtitles/scanner.rs`.
- **Rationale**: Media libraries frequently place external subtitles in a dedicated subfolder (`subtitles/`, `Subtitles/`, `subs/`, `Subs/`). Currently, `scan_external_subtitles()` checks only the media file's direct directory.
- **Specification**:
  Extend `scan_external_subtitles` to iterate over standard subtitle subfolder names, discovering matching files with supported extensions (`.srt`, `.vtt`, `.ass`, `.ssa`).

### 4.2 Feature 2: Fine-Grained Playback Speed Steps ($[0.25\text{x}, 4.0\text{x}]$) with Room Sync
- **Scope**: `crates/syncverse-clock`, `crates/syncverse-sync`, `crates/syncverse-core`.
- **Rationale**: Desktop media players provide flexible playback speeds (0.25x, 0.5x, 0.75x, 1.0x, 1.25x, 1.5x, 1.75x, 2.0x, etc.).
- **Specification**:
  - Support speed step commands (`[` / `]` hotkeys) with discrete increments (0.05x / 0.1x / 0.25x) and `Backspace` to reset to 1.0x.
  - Fix PLL controller to track $r_{\text{target}}$ across the entire range $[0.25, 4.0]$.

### 4.3 Feature 3: Media Looping & A-B Repeat Segment Markers
- **Scope**: `crates/syncverse-core`, `crates/syncverse-ui`.
- **Rationale**: Repetitive practice, study, music video playback, or short animation clips require looping and A-B segment repeating.
- **Specification**:
  - Add Loop Toggle (`PlayerCommand::ToggleLoop` / `L` hotkey).
  - Add A-B Repeat Markers:
    - First press (`[` or marker shortcut): set Marker A ($t_{\text{start}}$).
    - Second press (`]`): set Marker B ($t_{\text{end}}$).
    - Third press (`\`): clear A-B loop.
  - When `pts >= t_{\text{end}}`, automatically seek to $t_{\text{start}}$.

### 4.4 Feature 4: Enhanced Network Jitter & Sync Quality Diagnostics
- **Scope**: `crates/syncverse-sync`, `crates/syncverse-ui`.
- **Rationale**: Users in long-distance WAN watch parties benefit from rich sync telemetry beyond simple ping.
- **Specification**:
  - Expose RFC 5905 window dispersion $D_{\text{effective}}$ as a network jitter metric ($\text{Jitter} = D / 1\,000\,000\text{ ms}$).
  - Expose NTP probe rejection rate: $\text{loss} = \frac{\text{rejected}}{\text{accepted} + \text{rejected}} \cdot 100\%$.
  - Display min, median, and max RTT over the sliding window in the watch-party sidebar / HUD tooltip.

---

## 5. 5-Component Handoff Protocol

### 5.1 Observation
1. **Chapter Float Conversion**: `crates/syncverse-demux/src/universal.rs:207-210`:
   ```rust
   let tb = ch.time_base();
   let tb_f64 = f64::from(tb);
   let start_ms = (ch.start() as f64 * tb_f64 * 1000.0).max(0.0) as u64;
   let end_ms = (ch.end() as f64 * tb_f64 * 1000.0).max(0.0) as u64;
   ```
2. **PLL Micro-Speed Formula Hardcoded to 1.0x**: `crates/syncverse-sync/src/pll.rs:98-99`:
   ```rust
   let delta_r = self.kp * e + self.ki * self.integral;
   let speed = (1.0 + delta_r).clamp(self.r_min, self.r_max);
   ```
3. **Zone 1 Client Speed Hardcoded to 1.0x**: `crates/syncverse-core/src/playback_runtime.rs:2087-2090`:
   ```rust
   if (speed - 1.0).abs() > 1e-4 {
       speed = 1.0;
       clock.reanchor(now, 1.0);
   }
   ```
4. **Hard Sample Clamping on Downmix**: `crates/syncverse-codecs/src/audio/universal.rs:326`:
   ```rust
   let samples: Vec<f32> = plane.iter().map(|&s| s.clamp(-1.0, 1.0)).collect();
   ```
5. **Baseline Verification**:
   - `./scripts/audit_dependencies.sh`: 0 violations.
   - `cargo test --workspace -- --nocapture`: 100% passing across all 8 crates.
   - `python3 scripts/intent_harness.py`: 8/8 intents verified, 100% compliance score.

### 5.2 Logic Chain
1. From Observation 1, chapter start and end timestamps in `UniversalDemuxer` use floating-point arithmetic `(ch.start() as f64 * tb_f64 * 1000.0)`, which conflicts with AGENTS.md Section 4 Rule 6 requiring 64-bit integer arithmetic for timescale conversions.
2. From Observation 2, `PllSpeedController` calculates `speed = (1.0 + delta_r).clamp(0.96, 1.04)`.
3. From Observation 3, `playback_runtime.rs` forces `speed = 1.0` whenever `drift_ms <= 15`.
4. When a room host sets a non-1.0x playback speed (e.g. 1.25x or 1.5x), the reference timeline progresses at rate `r_target`. The client's PLL controller cannot track `r_target` because it clamps speed near 1.0x.
5. Consequently, the client falls behind by `(r_target - 1.0)` seconds every second, drifting $> 1500\text{ms}$ every few seconds and triggering repeated Zone 3 hard seeks.
6. From Observation 4, multichannel audio downmixed to stereo can sum to $> 1.0$. Hard clamping creates flat-top clipping distortion. Replacing hard clamping with a smooth soft-knee limiter eliminates digital clipping while preserving peak limits.

### 5.3 Caveats
- This investigation is strictly read-only; no code modifications were applied to the source crates.
- `UniversalDemuxer` relies on system FFmpeg shared libraries via `ffmpeg-next`; hardware-accelerated decoding support is dependent on system GPU drivers (VAAPI, NVDEC, DXVA2).
- The 72-byte `PlayerUiSnapshot` memory layout is a strict invariant; any new UI telemetry metrics should be transferred via existing fields (e.g. within `network_rtt_nanos` / `clock_offset_nanos` or existing event channels) rather than expanding the 72-byte snapshot struct.

### 5.4 Conclusion
The multimedia, decoding, clock, and synchronization core is robust, high-performance, and compliant with zero-warning and blacklist invariants. However, four concrete defects must be addressed in subsequent milestones:
1. Fix PLL and client drift loop to support arbitrary room target speeds $r_{\text{target}} \in [0.25, 4.0]$.
2. Fix chapter timescale conversion in `UniversalDemuxer` to use 128-bit integer rational arithmetic.
3. Replace hard sample clamping in audio downmixing with soft-knee saturation.
4. Add soundcard starvation protection in `playback_runtime.rs`.

Additionally, four high-value features are recommended:
1. Subdirectory subtitle scanning (`subtitles/`, `subs/`).
2. Fine-grained playback speed steps with synchronized room tracking.
3. Media looping and A-B repeat markers.
4. Enhanced network jitter and probe loss telemetry.

### 5.5 Verification Method
To independently verify the findings in this report:
```bash
# 1. Verify dependency blacklist compliance (0 violations)
./scripts/audit_dependencies.sh

# 2. Verify workspace compilation under -D warnings
cargo check --workspace --all-targets

# 3. Verify entire workspace test suite
cargo test --workspace -- --nocapture

# 4. Verify mathematical and timing empirical challenge test suites
python3 tests/empirical_challenge/test_demux_codecs_clock.py
python3 tests/empirical_challenge/test_sync_engine.py

# 5. Verify Pi intent verification harness
python3 scripts/intent_harness.py

# 6. Inspect identified code sections:
# - crates/syncverse-demux/src/universal.rs:207-210 (chapter timescale float math)
# - crates/syncverse-sync/src/pll.rs:98-99 (PLL speed multiplier hardcoded to 1.0 + delta_r)
# - crates/syncverse-core/src/playback_runtime.rs:2085-2091 (Zone 1 speed reset to 1.0)
# - crates/syncverse-codecs/src/audio/universal.rs:326 (hard clamp [-1.0, 1.0])
```
