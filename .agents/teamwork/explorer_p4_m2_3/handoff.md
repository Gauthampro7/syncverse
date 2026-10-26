# Handoff Report: Universal Audio Decoding & Clock Pacing Architecture

**Agent**: `explorer_p4_m2_3`  
**Date**: 2026-10-25T21:23:00Z  
**Context**: Syncplay-Alt Phase 4 Milestone 2 — Universal Video & Audio Compatibility Engine  
**Destination**: `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_3/handoff.md`  

---

## 1. Observation

### 1.1 Audio Codec Coverage & FFmpeg Decoder Availability
We probed Gautham's Fedora Linux 44 system environment and the cached `ffmpeg-next-9.0.0` crate linking against system `ffmpeg-libs-8.1.3` (libavcodec 62.28.103, libavformat 62.12.103, libswresample 6.3.103).
Testing `ffmpeg_next::codec::decoder::find(codec_id)` for all target audio codecs specified in `ORIGINAL_REQUEST.md` (R2) yielded:
```text
Codec AAC         : Decoder=Some("aac"), Encoder=Some("aac")
Codec MP3         : Decoder=Some("mp3float"), Encoder=Some("libmp3lame")
Codec Opus        : Decoder=Some("opus"), Encoder=Some("libopus")
Codec Vorbis      : Decoder=Some("vorbis"), Encoder=Some("libvorbis")
Codec FLAC        : Decoder=Some("flac"), Encoder=Some("flac")
Codec AC3         : Decoder=Some("ac3"), Encoder=Some("ac3")
Codec EAC3        : Decoder=Some("eac3"), Encoder=Some("eac3")
Codec DTS         : Decoder=Some("dca"), Encoder=Some("dca")
Codec PCM_S16LE   : Decoder=Some("pcm_s16le"), Encoder=Some("pcm_s16le")
Codec PCM_F32LE   : Decoder=Some("pcm_f32le"), Encoder=Some("pcm_f32le")
```
Every single requested codec has a high-performance native decoder pre-installed on the host.

### 1.2 Empirical Verification Across Real Audio Test Streams
We synthesized standalone test streams for all 9 codecs using system tools and decoded them through `UniversalAudioDecoder` with real-time resampling to 48,000 Hz, stereo, interleaved `f32`:
- `/tmp/test_aac.m4a` (AAC in MP4): Decoded 46 frames, 100,310 samples (~1.04s), 100% in `[-1.0, 1.0]`.
- `/tmp/test_mp3.mp3` (MP3): Decoded 41 frames, 100,310 samples (~1.04s), 100% in `[-1.0, 1.0]`.
- `/tmp/test_opus.opus` (Opus): Decoded 51 frames, 97,296 samples (~1.01s), 100% in `[-1.0, 1.0]`.
- `/tmp/test_vorbis.ogg` (Vorbis): Decoded 45 frames, 97,106 samples (~1.01s), 100% in `[-1.0, 1.0]`.
- `/tmp/test_flac.flac` (FLAC): Decoded 11 frames, 96,000 samples (~1.00s), 100% in `[-1.0, 1.0]`.
- `/tmp/test_ac3.ac3` (AC3): Decoded 30 frames, 96,966 samples (~1.01s), 100% in `[-1.0, 1.0]`.
- `/tmp/test_eac3.eac3` (E-AC3): Decoded 30 frames, 96,966 samples (~1.01s), 100% in `[-1.0, 1.0]`.
- `/tmp/test_pcm.wav` (PCM S16LE): Decoded 12 frames, 96,000 samples (~1.00s), 100% in `[-1.0, 1.0]`.
- `/tmp/test_dts.dts` (DTS): Decoded 88 frames, 96,966 samples (~1.01s), 100% in `[-1.0, 1.0]`.

All 9 target codecs passed decoding and resampling with zero errors and zero clipping violations.

### 1.3 Key Edge Cases Discovered & Solved
1. **Empty Channel Layout in WAV / Demuxed Streams**:
   - In `PCM (WAV)` decoding, `raw_frame.channel_layout()` reported `is_empty: true` (mask 0), which caused `swr_convert_frame` to fail with `ffmpeg::Error(1668179713: Input changed)`.
   - **Resolution**: Defined `effective_layout(&raw_frame)`:
     ```rust
     fn effective_layout(frame: &AudioFrame) -> ChannelLayout {
         let layout = frame.channel_layout();
         if layout.is_empty() || layout.channels() == 0 {
             ChannelLayout::default(frame.channels() as i32)
         } else {
             layout
         }
     }
     ```
     If the frame layout mask is empty, we automatically assign `ChannelLayout::default(channels)`, resolving the error immediately.
2. **Resampler Flush Buffer Requirement**:
   - Calling `resampler.flush(&mut flush_frame)` on an unallocated `AudioFrame::empty()` caused a panic: `ffmpeg::Error(1668179714: Output changed)` because `swr_convert_frame` requires pre-allocated buffers when flushing without an input frame.
   - **Resolution**: Check `resampler.delay()`, and if `delay.output > 0`, allocate `AudioFrame::new(dst_format, delay.output as usize, dst_layout)` prior to calling `resampler.flush(&mut flush_frame)`.

### 1.4 Clock Pacing & Ring Buffer Transfer
In `crates/syncplay-clock/src/audio_pacer.rs`:
- Line 47: `AudioPacer` tracks presentation against `MasterClock`, classifying frames into `PlayNow` ($|\Delta| \le 20\text{ms}$), `Wait(\Delta - 20\text{ms})`, or `Late(-\Delta)`.
- Line 191: Inter-thread frame delivery is implemented via `SpscRingBuffer<AudioFramePCM, 64>` (128-byte cache-line aligned).
- Line 204: When the ring buffer underruns, `AudioPacer` automatically injects a pre-allocated silence scratch frame anchored to `master_clock.current_pts(now)`.
- Line 198: When valid frames resume after an underrun, `AudioPacer` applies a smooth 2ms linear ramp-in to eliminate pops/clicks.
- In our empirical integration test: 51 frames were decoded, pushed to `SpscRingBuffer`, pulled by `AudioPacer`, and underrun protection was triggered and verified with output:
  `SUCCESS! Pushed 51 frames, popped 51 frames via 128B SPSC and AudioPacer. Underruns handled cleanly: AudioPacerStats { valid_frames: 51, underruns: 1, silence_frames_injected: 1 }`.

### 1.5 Audio Sink Availability
In `crates/syncplay-core/src/audio_sink.rs`:
- Lines 42-125: `PipeAudioSink` streams raw IEEE 754 32-bit float PCM (`f32`, 48kHz stereo) to system tools:
  - First attempts `/usr/bin/pw-cat --playback --raw --rate 48000 --channels 2 --format f32 -` (PipeWire native).
  - Falls back cleanly to `/usr/bin/aplay -r 48000 -c 2 -f FLOAT_LE -t raw -` (ALSA).
- Line 220: `NullAudioSink` provides a zero-overhead sink for headless automated testing and CI.
- `AutoAudioSink` dynamically selects `PipeAudioSink` when available and falls back to `NullAudioSink`.

---

## 2. Logic Chain

### 2.1 Universal Compatibility Architecture
1. **Observation 1.1 & 1.2**: All 9 codecs required by `ORIGINAL_REQUEST.md` (R2) are supported by `ffmpeg-next` and system `libavcodec`. No other pure-Rust crate supports AC3, E-AC3, DTS, and full container variety without C bindings.
2. **Standardization Pipeline**:
   $$\text{DemuxPacket} \xrightarrow{\text{libavcodec}} \text{AudioFrame (raw)} \xrightarrow{\text{libswresample}} \text{AudioFrame (48kHz stereo f32)} \xrightarrow{} \text{AudioFramePCM}$$
   - Any source sampling rate ($8\text{kHz} \dots 192\text{kHz}$) is converted to $48,000\text{ Hz}$.
   - Any source channel configuration (mono, stereo, 5.1, 7.1) is mixed down or up to 2-channel stereo.
   - Any sample format (U8, S16, S24, S32, FLT, DBL, planar or packed) is converted to packed interleaved 32-bit float (`f32`), clamped to $[-1.0, 1.0]$.
3. **Queue Sizing & Buffer Capacity**:
   - SPSC ring buffer depth is 64 frames.
   - At 1024 samples per frame (48kHz stereo), 1 frame $\approx 21.3\text{ms}$.
   - Total buffer capacity $= 64 \times 21.3\text{ms} \approx 1.36\text{ seconds}$.
   - Memory footprint: $64 \times 1024 \times 2 \times 4\text{ bytes} \approx 524\text{ KB}$, well within the memory budget.
4. **Synchronization Continuity**:
   - In watch-party mode, `PllSpeedController` shifts playback speed $r \in [0.96, 1.04]$.
   - `MasterClock` updates presentation speed atomically via SeqLock without phase discontinuities ($C^0$ continuity).
   - In-tree `LinearResampler` (`crates/syncplay-codecs/src/audio/resampler.rs`) or `swresample` can perform micro-speed phase steering if required, or audio pacer paces presentation against `MasterClock`.

---

## 3. Caveats

1. **Host Build Environment Sysroot**:
   - System runtime libraries (`/usr/lib64/libavcodec.so.62`, `/usr/lib64/libswresample.so.6`) are present.
   - For compilation with `ffmpeg-next`, the environment variables configured by `scripts/setup_deps.sh` (`source .deps/env.sh`) must be active:
     ```bash
     source /home/gautham/syncplay-alt/.deps/env.sh
     ```
2. **CPAL Dependency**:
   - `cpal = "0.18.2"` requires `alsa.pc` from `alsa-lib-devel` if compiled as a native Cargo dependency.
   - For immediate Linux desktop playback, `AutoAudioSink` using `pw-cat` (PipeWire) and `aplay` (ALSA) streams raw PCM over OS pipes with zero compilation dependencies and zero linking overhead.
   - `cpal` can be added as an optional feature or once `alsa-lib-devel` is added to `.deps/`.

---

## 4. Conclusion

1. **Ready-to-Implement Prototype**:
   - We delivered the complete, tested `UniversalAudioDecoder` prototype in:
     `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_3/universal_audio_prototype.rs`.
2. **Universal Codec Coverage**:
   - 100% verified for AAC, MP3, Opus, Vorbis, FLAC, AC3, E-AC3, DTS, and PCM.
3. **Standardized Pacing Pipeline**:
   - Demuxed packets $\to$ `UniversalAudioDecoder` $\to$ `AudioFramePCM` (48kHz stereo f32) $\to$ `SpscRingBuffer<AudioFramePCM, 64>` $\to$ `AudioPacer` $\to$ `AudioSink` (`AutoAudioSink::Pipe` / `pw-cat`).
4. **Reliability & Edge Cases**:
   - Both empty channel layout normalization and resampler flush buffer pre-allocation are resolved and verified.

---

## 5. Verification Method

To independently verify this subsystem design:

1. **Verify Environment Setup**:
   ```bash
   source /home/gautham/syncplay-alt/.deps/env.sh
   ```

2. **Verify Codec Coverage & Resampling in Test Harness**:
   Run the empirical test program located in `/tmp/test_audio`:
   ```bash
   cd /tmp/test_audio
   cargo run
   ```
   **Expected Output**:
   ```text
   SUCCESS! Pushed 51 frames, popped 51 frames via 128B SPSC and AudioPacer. Underruns handled cleanly: AudioPacerStats { valid_frames: 51, underruns: 1, silence_frames_injected: 1 }
   ```

3. **Verify All 9 Codec Bitstream Decoding**:
   Execute the 9-codec test script:
   ```bash
   cat << 'EOF' > /tmp/test_audio/src/main.rs
   mod universal_audio;
   use universal_audio::UniversalAudioDecoder;
   use ffmpeg_next::format::input;
   use std::path::Path;

   fn main() {
       ffmpeg_next::init().unwrap();
       let test_files = [
           ("/tmp/test_aac.m4a", "AAC"),
           ("/tmp/test_mp3.mp3", "MP3"),
           ("/tmp/test_opus.opus", "Opus"),
           ("/tmp/test_vorbis.ogg", "Vorbis"),
           ("/tmp/test_flac.flac", "FLAC"),
           ("/tmp/test_ac3.ac3", "AC3"),
           ("/tmp/test_eac3.eac3", "E-AC3"),
           ("/tmp/test_pcm.wav", "PCM"),
           ("/tmp/test_dts.dts", "DTS"),
       ];
       for (file, label) in test_files {
           let mut ictx = input(&Path::new(file)).unwrap();
           let s = ictx.streams().best(ffmpeg_next::media::Type::Audio).unwrap();
           let tb = (s.time_base().0, s.time_base().1);
           let mut dec = UniversalAudioDecoder::from_parameters(s.parameters(), Some(tb)).unwrap();
           let s_idx = s.index();
           let mut count = 0;
           for (stream, pkt) in ictx.packets() {
               if stream.index() == s_idx {
                   let frames = dec.decode_packet(pkt.data().unwrap(), 0).unwrap();
                   count += frames.len();
               }
           }
           println!("Codec {}: {} frames decoded successfully", label, count);
           assert!(count > 0);
       }
   }
   EOF
   cargo run
   ```
   **Expected Output**:
   All 9 codecs print `... frames decoded successfully`.

4. **Verify Existing Workspace Tests**:
   ```bash
   cd /home/gautham/syncplay-alt
   cargo check --workspace --all-targets
   cargo test --workspace
   ```
