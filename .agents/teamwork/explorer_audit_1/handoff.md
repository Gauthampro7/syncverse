# Comprehensive Desktop Video Player Feature Audit & Architectural Specification

**Author**: `explorer_audit_1` (`teamwork_explorer`)  
**Date**: 2026-10-27T19:05:00Z  
**Workspace**: `/home/gautham/syncverse`  
**Target Milestone**: Desktop Video Player Standard Features (Subtitles, Audio/Video Configuration, Track Cycling, OSD HUD)

---

## 1. Observation

### 1.1 Existing Subsystems & Codebase Architecture
Direct inspection of all crates and files in the workspace revealed the following:

1. **`crates/syncverse-demux`**:
   - `crates/syncverse-demux/src/traits.rs:18`: `TrackType::Subtitle` already exists in `TrackType`, alongside `Video` and `Audio`.
   - `crates/syncverse-demux/src/traits.rs:22-40`: `TrackInfo` currently contains:
     ```rust
     pub struct TrackInfo {
         pub track_id: u64,
         pub track_type: TrackType,
         pub codec_id: String,
         pub width: Option<u32>,
         pub height: Option<u32>,
         pub sample_rate: Option<f64>,
         pub channels: Option<u32>,
         pub codec_private: Option<Vec<u8>>,
     }
     ```
     *Missing*: `TrackInfo` lacks stream metadata tags (`language: Option<String>`, `title: Option<String>`).
   - `crates/syncverse-demux/src/traits.rs:54-66`: `DemuxPacket` has:
     ```rust
     pub struct DemuxPacket {
         pub track_id: u64,
         pub pts_ms: u64,
         pub dts_ms: Option<u64>,
         pub is_keyframe: bool,
         pub data: Vec<u8>,
     }
     ```
     *Missing*: Does not expose `duration_ms: Option<u64>`, which is critical for subtitle cue display duration.
   - `crates/syncverse-demux/src/universal.rs:83`: `ffmpeg::media::Type::Subtitle => TrackType::Subtitle` is parsed during `init_from_context`, but subtitle packets are not collected or decoded in the playback loop.
   - *Missing*: No subtitle parsers exist for SubRip (`.srt`), WebVTT (`.vtt`), or SubStation Alpha (`.ass`/`.ssa`).
   - *Missing*: No external subtitle file detection scanner in media directory matching media base name.

2. **`crates/syncverse-core`**:
   - `crates/syncverse-core/src/playback_runtime.rs:92-105`:
     ```rust
     let vt = info.tracks.iter().find(|t| t.track_type == TrackType::Video).cloned();
     let at = info.tracks.iter().find(|t| t.track_type == TrackType::Audio).cloned();
     let video_track_id = vt.as_ref().map(|t| t.track_id);
     let audio_track_id = at.as_ref().map(|t| t.track_id);
     ```
     *Observation*: `PlaybackSession` hardcodes selection of the first audio track and completely ignores subtitles or secondary audio streams.
   - `crates/syncverse-core/src/playback_runtime.rs:853-884`: Audio packet decoding only checks `Some(packet.track_id) == sess.audio_track_id`. When switching audio tracks, `sess.audio_track_id` and `sess.audio_decoder` must be updated, and the audio sink must be reset.
   - *Missing*: No audio delay / sync offset adjustment parameter in `PlaybackSession` or commands.

3. **`crates/syncverse-clock`**:
   - `crates/syncverse-clock/src/audio_pacer.rs:164-182`: `evaluate_timing` evaluates `(frame_pts_ms as f64) - master_pts`.
   - *Missing*: `AudioPacer` does not have an atomic `audio_delay_ms: AtomicI64` field to offset presentation timing for user-configurable audio sync.

4. **`crates/syncverse-ui`**:
   - `crates/syncverse-ui/src/snapshot.rs:12-37`: `PlayerUiSnapshot` has `#[repr(C)]` and a fixed memory footprint of exactly 72 bytes (`assert_eq!(size, 72)`).
     *Critical Invariant*: Under AGENTS.md § 1.3 and unit test `test_struct_memory_size`, `PlayerUiSnapshot` must remain exactly 72 bytes. Rich events (track inventories, active track names, subtitle cue updates) must be communicated via `UiEvent` over the existing MPSC channel.
   - `crates/syncverse-ui/src/command.rs:6-52`: `PlayerCommand` defines 15 variants, but lacks:
     - `SelectAudioTrack(u64)`
     - `CycleAudioTrack`
     - `SetAudioDelay(i64)`
     - `AdjustAudioDelay(i64)`
     - `SelectSubtitleTrack(Option<usize>)`
     - `CycleSubtitleTrack`
     - `SetSubtitleDelay(i64)`
     - `AdjustSubtitleDelay(i64)`
     - `LoadSubtitleFile(PathBuf)`
     - `SetAspectRatio(AspectRatioOverride)`
   - `crates/syncverse-ui/src/event.rs:7-41`: `UiEvent` lacks events for track discovery, track change notifications, and HUD telemetry banners.
   - `crates/syncverse-ui/src/gui/app.rs:221-288`: Keyboard handling implements `F11`, `Space`, `Left/Right`, `Up/Down`, `M`, `S`, `[`/`]`, `Backspace`.
     *Missing*: Lacks cycling hotkeys (`B` for audio cycling, `V`/`C` for subtitle cycling, `J`/`K` for audio delay, `Z`/`X` for subtitle delay, `A` for aspect ratio).
   - `crates/syncverse-ui/src/gui/viewport.rs:36-95`: `ViewportGeometry::compute` only computes aspect ratio preservation based on raw video width/height.
     *Missing*: Does not support aspect ratio overrides (16:9, 4:3, 21:9, 1:1, Fill/Stretch).
     *Missing*: Does not render synchronized subtitle cues inside `video_rect`.
     *Missing*: Does not render OSD HUD status banners for track switching.
   - `crates/syncverse-ui/src/gui/controls.rs:94-161`: Playback controls bar has volume slider and speed selector, but lacks audio delay adjustment, audio track selection menu, and subtitle track selection menu.

### 1.2 Baseline Verification Tooling & Test Status
- `cargo check --workspace --all-targets`: Clean (0 warnings).
- `./scripts/audit_dependencies.sh`: Clean (0 banned player or DB dependencies).
- `cargo test --workspace`: Clean (all 126 unit/integration tests passed).
- Empirical test suites & `python3 scripts/intent_harness.py`: 8/8 intents passed (100% compliance).

---

## 2. Logic Chain

### 2.1 Standard Desktop Video Player Feature Inventory
To bring Syncverse to the standard of modern standalone desktop video players (VLC, mpv, IINA) while maintaining strict architectural invariants, the required feature matrix is:

| Category | Feature | Status in Codebase | Proposed Implementation |
|---|---|---|---|
| **Subtitles** | Embedded container track detection (MKV, MP4) | Metadata parsed, ignored in session | Extract language/title in `TrackInfo`, emit in `TracksDiscovered` |
| **Subtitles** | External subtitle auto-detection (.srt, .vtt, .ass) | Not implemented | Scan directory of opened media for matching stem + extensions |
| **Subtitles** | Pure-Rust SubRip (.srt) parser | Not implemented | Parse timestamps `HH:MM:SS,mmm`, multiline text, strip basic tags |
| **Subtitles** | Pure-Rust WebVTT (.vtt) parser | Not implemented | Parse `WEBVTT`, timestamps `HH:MM:SS.mmm`, cue settings, tags |
| **Subtitles** | Pure-Rust ASS/SSA (.ass/.ssa) parser | Not implemented | Parse `[Events]` `Dialogue:`, centiseconds `H:MM:SS.cs`, strip overrides |
| **Subtitles** | Subtitle track selection & 'Off' state | Not implemented | `SelectSubtitleTrack(Option<usize>)`, track list in UI |
| **Subtitles** | Viewport synchronized subtitle overlay | Not implemented | Render in `VideoViewport` anchored to `video_rect` bottom |
| **Subtitles** | Subtitle sync / delay adjustment | Not implemented | `subtitle_delay_ms: i64`, hotkeys `Z`/`X`, UI slider |
| **Audio** | Multi-audio track discovery & listing | First track only | Discover all audio tracks, extract codec/lang/title/channels |
| **Audio** | Audio track switching during playback | Not implemented | Re-instantiate `UniversalAudioDecoder` + reset audio sink |
| **Audio** | Audio track cycling hotkey (`B`) | Not implemented | Cycle sequentially through available audio tracks + OSD HUD |
| **Audio** | Audio delay / sync offset adjustment | Not implemented | Atomic offset in `AudioPacer`, hotkeys `J`/`K`, UI slider |
| **Audio** | Volume slider [0-100%] & Mute toggle (`M`) | Partially implemented in UI | Full integration with Audio menu + HUD feedback |
| **Video** | Aspect ratio override (Original, 16:9, 4:3, 21:9, Fill) | Only Original | `AspectRatioOverride` in `ViewportGeometry`, hotkey `A` |
| **Video** | Playback speed control (presets & hotkeys) | Basic speed slider exists | Presets (0.5x, 0.75x, 1x, 1.25x, 1.5x, 2x), HUD feedback |
| **Video** | Video Configuration modal / menu | Not implemented | Video modal with aspect ratio, resolution, format details |
| **Hotkeys & HUD** | Dedicated Audio & Subtitle cycle hotkeys | Not implemented | `B` (Audio), `V`/`C` (Subtitles), OSD fade-out banner |
| **Hotkeys & HUD** | On-Screen Display (OSD) feedback banner | Basic chat bubble only | Floating translucent banner in viewport with 2.0s fade |
| **Hotkeys & HUD** | Keyboard Shortcuts Dialog (F1) | Existing, lacks new keys | Updated documentation for all new player shortcuts |

### 2.2 Subtitle Architecture & Pipeline Design
1. **Models in `syncverse-common`**:
   - `SubtitleCue`: `start: MediaTime`, `end: MediaTime`, `text: String`.
   - `SubtitleSource`: `Embedded` vs `External`.
   - `SubtitleTrack`: `id: usize`, `source: SubtitleSource`, `format: String`, `language: Option<String>`, `title: String`, `cues: Vec<SubtitleCue>`.
2. **Parsers in `syncverse-demux`**:
   - Pure-Rust implementations in `syncverse-demux::subtitles`:
     - `srt.rs`: SubRip parser.
     - `vtt.rs`: WebVTT parser.
     - `ass.rs`: Advanced SubStation Alpha parser.
     - `scanner.rs`: Directory scanner matching `media_stem.*` with `.srt`, `.vtt`, `.ass`, `.ssa`, parsing language tags (e.g., `movie.en.srt` -> language `"en"`, title `"English"`).
3. **Synchronization & Viewport Rendering in `syncverse-ui`**:
   - Active subtitle cues are queried in $O(\log N)$ binary search against `current_pts_ms = snapshot.media_position_pts / 1_000_000 + subtitle_delay_ms`.
   - Viewport painter renders active cue text at the bottom center of `video_rect` (calculated by `ViewportGeometry`), wrapped in high-contrast text styling (shadow/outline or semi-transparent dark pill background).

### 2.3 Audio Multi-Track & Delay Architecture
1. **Stream Discovery**:
   - `UniversalDemuxer` inspects all stream metadata tags (`AVDictionary` in FFmpeg C-FFI) to populate `TrackInfo.language` and `TrackInfo.title`.
2. **Dynamic Decoder Switching**:
   - Upon receiving `PlayerCommand::SelectAudioTrack(track_id)` or `PlayerCommand::CycleAudioTrack`:
     - `sess.audio_track_id = Some(new_id)`.
     - Reconstruct `UniversalAudioDecoder` from stream parameters.
     - Send `AudioWorkerCommand::Reset` to flush stale soundcard PCM buffers.
     - Emit `UiEvent::AudioTrackChanged` with track name and OSD notification.
3. **Audio Delay Offset**:
   - `AudioPacer` stores `audio_delay_ms: AtomicI64`.
   - In `evaluate_timing`, `effective_pts = (frame_pts_ms as i64 + delay_ms) as f64`.
   - Adjustable via `PlayerCommand::SetAudioDelay(ms)` and `PlayerCommand::AdjustAudioDelay(delta_ms)` (hotkeys `J` [-50ms], `K` [+50ms]).

### 2.4 Video Aspect Ratio Override Architecture
1. **Aspect Ratio Override Enum**:
   ```rust
   #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
   pub enum AspectRatioOverride {
       #[default]
       Original,
       Ratio16_9,
       Ratio4_3,
       Ratio21_9,
       Ratio1_1,
       FillStretch,
   }
   ```
2. **Preserving Backward Compatibility in `ViewportGeometry`**:
   - Maintain `ViewportGeometry::compute(available_w, available_h, video_w, video_h)` exactly as-is so existing empirical tests (`test_ui_renderer.py`, `viewport_debouncer_empirical.rs`) pass without modification.
   - Add `ViewportGeometry::compute_with_aspect(available_w, available_h, video_w, video_h, aspect_override)`.

### 2.5 Preserving Architectural Invariants & Memory Layouts
- **Zero Banned Crates**: No `mpv`, `vlc`, `sqlite`, `rocksdb`.
- **72-Byte `PlayerUiSnapshot` Invariant**: `PlayerUiSnapshot` remains exactly 72 bytes. All dynamic track metadata and notifications use `UiEvent` over the existing MPSC channel.

---

## 3. Caveats

1. **Embedded Bitmap Subtitles**:
   - Formats like PGS (Blu-ray `.sup`) or VobSub (DVD `.sub`/`.idx`) are raster bitmaps, not timed text. Initial desktop player scope focuses on timed text formats (SRT, WebVTT, ASS, MOV text), which represent >95% of digital media subtitles. Bitmap subtitles can be flagged as unsupported or passed as text placeholders.
2. **Audio Resampling on Track Switch**:
   - Different audio tracks may have different sample rates (e.g. 44.1 kHz vs 48 kHz). Syncverse's `UniversalAudioDecoder` already normalizes output to 48 kHz stereo `AudioFramePCM`, ensuring seamless soundcard output without renegotiating ALSA/PipeWire stream parameters.
3. **Seek Flushing**:
   - Switching audio tracks mid-stream requires an immediate audio sink reset to avoid playing lingering audio frames from the previous language.

---

## 4. Conclusion & Precise Work Breakdown (10 Parallel Subagents)

To execute the implementation and verification cleanly and generously in parallel across 10 subagents, the work breakdown is structured as follows:

```
┌────────────────────────────────────────────────────────────────────────┐
│                   PHASE 2: 10 PARALLEL SUBAGENTS                       │
├────────────────────────────────┬───────────────────────────────────────┤
│ Backend Implementation:        │ Frontend & UX Implementation:         │
│  - Worker 1: worker_subtitles  │  - Worker 4: worker_ui_controls       │
│  - Worker 2: worker_audio_mgmt │  - Worker 5: worker_hud_hotkeys       │
│  - Worker 3: worker_video_ctrl │  - Worker 6: worker_subtitle_render   │
├────────────────────────────────┼───────────────────────────────────────┤
│ Verification & Quality:        │ Forensic Audit & Resilience:          │
│  - Reviewer 1: reviewer_core   │  - Challenger 1: challenger_empirical │
│  - Reviewer 2: reviewer_ui_hud │  - Auditor 1: auditor_integrity       │
└────────────────────────────────┴───────────────────────────────────────┘
```

### Detailed Agent Assignments & File Mapping:

#### 1. Worker 1 (`worker_subtitles`)
- **Scope**: Subtitle Data Models, Parsers, Container Demuxing & External Scanner.
- **Files to Modify / Create**:
  - `crates/syncverse-common/src/subtitles.rs` (new module): Define `SubtitleCue`, `SubtitleTrack`, `SubtitleSource`. Re-export in `crates/syncverse-common/src/lib.rs`.
  - `crates/syncverse-demux/src/traits.rs`: Add `language: Option<String>` and `title: Option<String>` to `TrackInfo`. Add `duration_ms: Option<u64>` to `DemuxPacket`.
  - `crates/syncverse-demux/src/universal.rs`: Populate stream language and title metadata from FFmpeg tags.
  - `crates/syncverse-demux/src/subtitles/mod.rs` (new module):
    - `srt.rs`: Pure-Rust parser for SubRip (.srt).
    - `vtt.rs`: Pure-Rust parser for WebVTT (.vtt).
    - `ass.rs`: Pure-Rust parser for Advanced SubStation Alpha (.ass/.ssa).
    - `scanner.rs`: Automatic external subtitle scanner for media directory matching base filename.
- **Unit Tests**: Full test suite for parsing valid/corrupt SRT, VTT, and ASS files, and directory scanning with language code extraction.

#### 2. Worker 2 (`worker_audio_mgmt`)
- **Scope**: Multi-Audio Track Switching, Audio Delay Pacing, Volume/Mute.
- **Files to Modify**:
  - `crates/syncverse-clock/src/audio_pacer.rs`: Add `audio_delay_ms: AtomicI64` and methods `set_audio_delay_ms(i64)`, `audio_delay_ms() -> i64`. Incorporate delay into `evaluate_timing`.
  - `crates/syncverse-core/src/playback_runtime.rs`:
    - Track all audio streams discovered in container (`Vec<TrackInfo>`).
    - Handle `PlayerCommand::SelectAudioTrack(u64)`: switch `sess.audio_track_id`, construct new `UniversalAudioDecoder`, reset audio sink.
    - Handle `PlayerCommand::CycleAudioTrack`: advance to next audio track index.
    - Handle `PlayerCommand::SetAudioDelay(i64)` and `PlayerCommand::AdjustAudioDelay(i64)`.
    - Emit `UiEvent::AudioTrackChanged` with track name and format.

#### 3. Worker 3 (`worker_video_controls`)
- **Scope**: Video Aspect Ratio Selection/Override, Playback Speed, Video Settings.
- **Files to Modify**:
  - `crates/syncverse-ui/src/gui/viewport.rs`:
    - Add `AspectRatioOverride` enum (`Original`, `Ratio16_9`, `Ratio4_3`, `Ratio21_9`, `Ratio1_1`, `FillStretch`).
    - Add `ViewportGeometry::compute_with_aspect(available_w, available_h, video_w, video_h, aspect_override)`.
    - Preserve existing `compute(...)` for backward compatibility.
    - Add aspect ratio state and setter to `VideoViewport`.
  - `crates/syncverse-ui/src/gui/controls.rs`:
    - Support speed preset shortcuts and aspect ratio cycling.

#### 4. Worker 4 (`worker_ui_controls`)
- **Scope**: Audio, Video, and Subtitle Configuration Menus/Modals & File Dialog.
- **Files to Modify**:
  - `crates/syncverse-ui/src/gui/dialogs.rs`:
    - Implement `AudioConfigDialog`: track dropdown, volume slider, audio sync delay slider (-5000ms to +5000ms), reset button.
    - Implement `VideoConfigDialog`: aspect ratio selector, playback speed selector, video stream resolution/codec info.
    - Implement `SubtitleConfigDialog`: list of embedded & external subtitle tracks, "Off" option, "Load External Subtitle..." file picker, subtitle sync delay slider.
  - `crates/syncverse-ui/src/gui/controls.rs`:
    - Add quick-access buttons in controls bar for Audio menu, Subtitle menu, Video settings.

#### 5. Worker 5 (`worker_hud_hotkeys`)
- **Scope**: Dedicated Cycling Hotkeys & Viewport OSD HUD Telemetry Feedback.
- **Files to Modify**:
  - `crates/syncverse-ui/src/command.rs`: Add commands for track cycling, delay adjustment, aspect ratio.
  - `crates/syncverse-ui/src/event.rs`: Add `UiEvent::HudNotification { text: String, timestamp: Instant }`.
  - `crates/syncverse-ui/src/gui/app.rs`:
    - Implement hotkeys:
      - `B`: Cycle audio tracks.
      - `V` / `C`: Cycle subtitle tracks (including 'Off').
      - `A`: Cycle aspect ratio.
      - `J` / `K`: Adjust audio delay (-50ms / +50ms).
      - `Z` / `X`: Adjust subtitle delay (-50ms / +50ms).
    - Trigger transient OSD HUD banner upon cycling.
    - Update `ShortcutsDialog` (F1) with complete shortcut documentation.
  - `crates/syncverse-ui/src/osd.rs` & `crates/syncverse-ui/src/gui/viewport.rs`:
    - Implement floating OSD HUD banner overlay in viewport with 2.0s dwell and linear alpha fade.

#### 6. Worker 6 (`worker_subtitle_rendering`)
- **Scope**: Synchronized Subtitle Overlay in Video Viewport.
- **Files to Modify**:
  - `crates/syncverse-ui/src/gui/viewport.rs`:
    - Implement `render_subtitles(&mut self, ui: &mut Ui, video_rect: Rect, active_text: Option<&str>)`.
    - Anchor subtitles inside `video_rect` (above bottom letterbox margin).
    - Render text with high contrast: dark rounded pill background or outline stroke, centered multiline text, font size proportional to video height.
  - `crates/syncverse-ui/src/gui/app.rs`:
    - Store active subtitle cues in memory.
    - On each frame, query active cue for `current_pts_ms = (snapshot.media_position_pts / 1_000_000) + subtitle_delay_ms`.
    - Pass active cue text to viewport for presentation; clear when track is 'Off'.

#### 7. Reviewer 1 (`reviewer_core_codecs`)
- **Scope**: Review Backend Crates (`common`, `demux`, `codecs`, `clock`, `core`).
- **Responsibilities**:
  - Verify `#![deny(warnings)]` and `#![deny(missing_docs)]` compliance.
  - Verify safety comments on all `unsafe` FFI blocks.
  - Verify zero memory allocation on audio pacing hot paths.
  - Review unit test coverage for subtitle parsers, stream switching, and delay math.

#### 8. Reviewer 2 (`reviewer_ui_hud`)
- **Scope**: Review Frontend Crates (`ui`, `core` event bridge).
- **Responsibilities**:
  - Verify 72-byte memory layout invariant for `PlayerUiSnapshot`.
  - Check GUI responsiveness, text focus suppression during typing, and modal dialog state.
  - Verify subtitle positioning across different aspect ratios and resolutions.
  - Check OSD HUD banner alpha fading and contrast.

#### 9. Challenger 1 (`challenger_empirical`)
- **Scope**: Empirical Testing, Edge Case Stress Testing, Regression Prevention.
- **Responsibilities**:
  - Run existing test suites (`test_demux_codecs_clock.py`, `test_sync_engine.py`, `test_smart_chat.py`, `test_ui_renderer.py`, `scripts/intent_harness.py`).
  - Create adversarial subtitle test suite: malformed SRT/VTT/ASS files, UTF-8 BOM, overlapping cues, zero-duration cues.
  - Test audio track cycling under high playback rate.
  - Test aspect ratio calculations across anamorphic and ultrawide viewports.

#### 10. Auditor 1 (`auditor_integrity`)
- **Scope**: Invariant Auditing, Code Quality, Build Validation.
- **Responsibilities**:
  - Run `./scripts/audit_dependencies.sh` to ensure zero banned dependencies (`mpv`, `vlc`, `sqlite`, `rocksdb`).
  - Run `cargo clippy --workspace --all-targets -- -D warnings`.
  - Audit codebase for dummy stubs or mock facades.
  - Validate `./scripts/verify_phase1.sh`.

---

## 5. Verification Method

### 5.1 Independent Verification Commands
The complete implementation and verification panel must pass the following commands:

```bash
# 1. Dependency Blacklist Audit (Ensures zero banned monolithic players or database engines)
./scripts/audit_dependencies.sh

# 2. Workspace Compilation with Zero Warnings
cargo check --workspace --all-targets

# 3. Strict Clippy Linter Check
cargo clippy --workspace --all-targets -- -D warnings

# 4. Comprehensive Unit & Integration Test Suite
cargo test --workspace -- --nocapture

# 5. Empirical Challenge & Mathematical Invariant Test Suites
python3 tests/empirical_challenge/test_demux_codecs_clock.py
python3 tests/empirical_challenge/test_sync_engine.py
python3 tests/empirical_challenge/test_smart_chat.py
python3 tests/empirical_challenge/test_ui_renderer.py

# 6. Pi Framework Intent Verification Harness (Anti-Drift Guard)
python3 scripts/intent_harness.py

# 7. End-to-End Automated Verification Script
./scripts/verify_phase1.sh
```

### 5.2 Invalidation Conditions
The audit and subsequent implementation will be considered invalid if:
1. Any external monolithic player (`mpv`, `vlc`) or external database (`sqlite`, `rocksdb`) is introduced into `Cargo.toml` or build scripts.
2. The memory size of `PlayerUiSnapshot` deviates from exactly 72 bytes.
3. `cargo check` or `cargo clippy` emits any compiler warning under `#![deny(warnings)]`.
4. Any empirical challenge test fails.
5. Subtitle rendering renders outside the active video boundary during letterboxing/pillarboxing.
6. Audio track switching fails to reset stale audio buffers, causing audio overlap or crackle.
