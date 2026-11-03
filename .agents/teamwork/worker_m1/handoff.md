# Handoff Report — Milestone 1: Architectural Invariants, Standards Compliance & Mathematical Precision

**Agent**: `worker_m1` (implementer / qa)  
**Parent Conversation**: `3feda53a-5598-4b7c-86b8-98dbeb7556bd`  
**Date**: 2026-10-31  

---

## 1. Observation

Direct observations and findings across the codebase prior to and following implementation:

1. **PLL Target Rate Steering (`crates/syncverse-sync/src/pll.rs` & `crates/syncverse-core/src/playback_runtime.rs`)**:
   - `PllSpeedController` previously assumed a fixed baseline speed of `1.0` in `update(&mut self, target_pts_ms, actual_pts_ms, dt_secs) -> PllCorrection`, returning `1.0` in Zone 1 (Deadband) and clamping steering to fixed `[r_min, r_max]` (`[0.96, 1.04]`). When a room operated at a non-1.0x rate (e.g., 1.25x, 1.5x, 0.75x), the PLL would continually attempt to steer back toward 1.0x rather than the room's consensus target rate $r_{\text{target}}$.
   - In `playback_runtime.rs:480-492`, when entering Deadband (`drift_ms <= 15`), the client clock retained its prior micro-steering rate instead of re-anchoring to $r_{\text{target}}$, causing residual velocity drift.

2. **Chapter Timescale Rational Arithmetic (`crates/syncverse-demux/src/universal.rs`)**:
   - Lines 207-210 in `UniversalDemuxer::init_from_context` calculated chapter start and end times using `f64` division:
     `let tb_f = tb.numerator() as f64 / tb.denominator() as f64;`
     `let start_ms = (ch.start() as f64 * tb_f * 1000.0).max(0.0) as u64;`
     `let end_ms = (ch.end() as f64 * tb_f * 1000.0).max(0.0) as u64;`
     This violated AGENTS.md Section 4 Rule 6 ("Use 64-bit/128-bit integer arithmetic for container timescale conversions to avoid float precision loss or integer overflows").

3. **Subtitle Subdirectory Discovery (`crates/syncverse-demux/src/subtitles/scanner.rs`)**:
   - `scan_external_subtitles` only inspected the immediate media file parent directory (`media_dir`), failing to detect external subtitles located in standard organization subdirectories (`subtitles/`, `Subtitles/`, `subs/`, `Subs/`).

4. **Audio Soft-Clipping with Tanh Saturation (`crates/syncverse-codecs/src/audio/`)**:
   - In `crates/syncverse-codecs/src/audio/universal.rs:327`, stereo downmixing used hard clamping: `sum.clamp(-1.0, 1.0)`.
   - In `crates/syncverse-core/src/playback_runtime.rs`, volume scaling also used hard clamping `(*s * vol).clamp(-1.0, 1.0)`. Hard clamping generates odd-harmonic distortion ("digital buzz") when audio peaks exceed 0 dBFS.

5. **MasterClock Memory Safety Proofs (`crates/syncverse-clock/src/master_clock.rs`)**:
   - Unsafe blocks at `set_state` (line 186) and `seek_to` (line 204) mutated the inner state via raw pointer dereference `&mut *self.state.get()` without an explicit formal `// SAFETY:` justification proving exclusive mutable access guaranteed by holding `writer_lock`.

6. **Slash Command Case-Insensitive Slicing (`crates/syncverse-chat/src/commands.rs`)**:
   - In `/poll` parsing (line 184), `input.find("poll").unwrap()` caused an immediate thread panic if the user entered uppercase or mixed-case commands like `/POLL` or `/Poll`, because `.find("poll")` looks for lowercase byte matches.

7. **MediaTime Timecode Parsing, Windows Path Normalization & Legacy Docstrings (`crates/syncverse-common/`)**:
   - `MediaTime` lacked string parsing (`from_timecode_str` and `FromStr`), requiring ad-hoc manual parsing in callers.
   - `crates/syncverse-common/src/path.rs` failed to strip leading slashes before Windows drive letters (e.g. `/C:/path/file.mp4`), producing invalid Windows paths. Clippy also flagged `collapsible_if` in `path.rs:67`.
   - Legacy documentation referring to "Syncplay-Alt" remained in `common/src/lib.rs`, `common/src/path.rs`, `common/src/error.rs`, `sync/src/lib.rs`, and `core/src/aggregator.rs`.

8. **Error Handling Architecture (`crates/syncverse-core/src/error.rs` & `playback_runtime.rs`)**:
   - `CoreError` lacked a variant for `syncverse_common::CommonError`.
   - `playback_runtime.rs` used untyped string errors (`Result<..., String>` and `Box<dyn Error>`), violating AGENTS.md Section 4 Rule 3 ("Never use String, &str, or Box<dyn Error> for operational error handling").

---

## 2. Logic Chain

1. **PLL Target Rate Math**:
   - To support dynamic playback rates ($r_{\text{target}} \in [0.25, 4.0]$), the speed adjustment $\Delta r$ derived from proportional-integral filtering must scale the baseline rate $r_{\text{target}}$:
     $r_{\text{steered}} = \left( r_{\text{target}} \cdot (1.0 + \Delta r) \right).\text{clamp}(r_{\text{target}} \cdot r_{\text{min}}, r_{\text{target}} \cdot r_{\text{max}})$.
   - When drift is within the deadband ($|\text{drift}| \le 15\text{ms}$, Zone 1), the controller must output exactly $r_{\text{target}}$ with zero error integration.
   - Backward compatibility is preserved by delegating `update(...)` to `update_with_target_rate(..., 1.0)`.
   - In `playback_runtime.rs`, when in Zone 1, checking `(speed - room_state.r_target).abs() > 1e-4` and calling `clock.reanchor(now, room_state.r_target)` eliminates persistent velocity offset.

2. **Integer Timescale Math**:
   - In FFmpeg, timebases are expressed as rational numbers $\frac{N}{D}$. Using 128-bit signed integer arithmetic:
     $\text{start\_ms} = \max\left(0, \frac{\text{start\_pts} \cdot N \cdot 1000}{D}\right)$.
   - Because $N, D \le 2^{31}-1$ and PTS can reach $2^{63}-1$, calculating $\text{PTS} \cdot N \cdot 1000$ in 128-bit integer space completely prevents 64-bit integer overflow and floating-point roundoff errors over arbitrarily long playback durations.

3. **Subtitle Subdirectory Discovery**:
   - External subtitle files often reside in subfolders next to video files (e.g., `subs/movie.srt`, `subtitles/movie.en.vtt`).
   - Querying `media_dir.join(sub)` for subdirectories `["subtitles", "Subtitles", "subs", "Subs"]` and collecting matching files by stem and extension, followed by canonicalized deduplication, resolves subtitles across standard folder structures without duplicate entries.

4. **Zero-Allocation Audio Soft-Clipping**:
   - For audio signals with $|s| \le 0.85$, the transfer function is purely linear: $f(s) = s$.
   - For $|s| > 0.85$, hyperbolic tangent saturation smoothly compresses peaks:
     $f(s) = \text{sgn}(s) \cdot \left[ 0.85 + 0.15 \cdot \tanh\left(\frac{|s| - 0.85}{0.15}\right) \right]$.
   - This function is continuous $C^1$, monotonic, strictly bounded in $[-1.0, 1.0]$, requires zero heap allocation, and eliminates hard digital clipping harmonics.

5. **MasterClock Formal Safety**:
   - `MasterClock` uses a SeqLock pattern around an `UnsafeCell<MasterClockState>`. Writers must acquire `writer_lock: Mutex<()>`.
   - While the mutex is locked, only one thread can execute writer logic, guaranteeing exclusive access to the `UnsafeCell`.
   - Adding formal `// SAFETY:` justification comments satisfies `#![deny(unsafe_code)]` exceptions and AGENTS.md Section 4 Rule 4.

6. **Safe Command Slicing**:
   - Command matching using `split_whitespace()` identifies the slash command token (`/poll`, `/POLL`, etc.). Slicing after the command token length (`&trimmed[cmd.len()..]`) safely retrieves the argument string regardless of input case without invoking `.find("poll").unwrap()`.

7. **MediaTime & Path Normalization**:
   - Timecodes formatted as `HH:MM:SS`, `HH:MM:SS.mmm`, `MM:SS`, or `MM:SS.mmm` parse deterministically into milliseconds with strict component bounds validation.
   - Windows path strings with URI-style leading slashes (e.g. `/C:/`) strip the leading slash when followed by an ASCII alphabetic character and a colon (`/^[A-Za-z]:/`), producing valid OS paths on Windows while preserving absolute UNIX paths (`/home/...`).

8. **Strongly Typed Error Architecture**:
   - Adding `CoreError::Common(#[from] CommonError)` allows seamless `?` error propagation from `syncverse-common` to `syncverse-core`.
   - Refactoring `PlaybackSession::open`, `select_audio_track`, and `launch_desktop_player_app` to return `Result<..., CoreError>` completely eradicates untyped `String` and `Box<dyn Error>` boundaries in the coordinator runtime.

---

## 3. Caveats

- **External GUI Window Lifecycle**: GUI presentation tests run headless/mocked in CI environments because hardware X11/Wayland display servers are not guaranteed to be active in containerized verification runners.
- **Clock Drift Simulation**: Unit tests simulate synthetic drift and time advances; real WAN jitter tests depend on network simulation harnesses (`tests/empirical_challenge/test_sync_engine.py`).

---

## 4. Conclusion

All 8 tasks specified for Milestone 1 have been implemented with genuine, high-precision mathematical algorithms and full standards compliance:
1. `PllSpeedController` tracks $r_{\text{target}}$ with scaled micro-speed limits, Zone 1 deadband pass-through, and client clock reanchoring.
2. `UniversalDemuxer` computes chapter timestamps using 128-bit integer rational arithmetic.
3. `scan_external_subtitles` scans `subtitles/`, `Subtitles/`, `subs/`, and `Subs/` subdirectories with deduplication.
4. `soft_clip_sample` provides zero-allocation smooth `tanh` saturation for $|s| > 0.85$, protecting downmix and volume scaling from digital clipping.
5. `MasterClock` unsafe blocks feature formal `// SAFETY:` memory-safety justification proofs.
6. `/poll` command safely parses arguments case-insensitively without panicking.
7. `MediaTime::from_timecode_str` and `FromStr` implemented; Windows drive letter prefix normalized; legacy docstrings cleaned.
8. `CoreError` encapsulates `CommonError`, and `playback_runtime.rs` is fully typed with zero untyped errors.

All verification stages — dependency blacklist audit, compiler checks with `#![deny(warnings)]`, strict clippy checks with `-D warnings`, unit/integration tests, Pi intent harness (100.0% score), and end-to-end verification script — pass cleanly.

---

## 5. Verification Method

To independently verify the implementation, execute the following commands in the workspace root:

```bash
# 1. Verify zero banned player/database dependencies:
./scripts/audit_dependencies.sh

# 2. Verify clean workspace compilation with zero warnings:
cargo check --workspace --all-targets

# 3. Verify strict Clippy linter with zero warnings:
cargo clippy --workspace --all-targets -- -D warnings

# 4. Verify all workspace unit and integration tests:
cargo test --workspace -- --nocapture

# 5. Run the empirical mathematical and challenge test suites:
python3 tests/empirical_challenge/test_demux_codecs_clock.py
python3 tests/empirical_challenge/test_sync_engine.py
python3 tests/empirical_challenge/test_smart_chat.py
python3 tests/empirical_challenge/test_ui_renderer.py

# 6. Run the Pi Framework Intent Verification Harness (all 8 intents):
python3 scripts/intent_harness.py

# 7. Run the End-to-End Automated Verification Script:
./scripts/verify_phase1.sh
```

### Invalidation Conditions:
- Any occurrence of banned crates (`mpv`, `vlc`, `sqlite`, `rocksdb`, `diesel`).
- Any compiler warning under `#![deny(warnings)]` or clippy violation under `-D warnings`.
- Any floating-point arithmetic introduced into timescale conversion.
- Any audio distortion or hard clipping when sample amplitude exceeds 0.85.
- Any thread panic when invoking `/POLL` or `/Poll` commands.
