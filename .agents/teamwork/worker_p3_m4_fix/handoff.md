# Technical Remediation Report: Milestone 4 (Requirement R1: Fedora Desktop Application & Interactive Terminal/OSD Player Launcher)

**Author**: `worker_p3_m4_fix`  
**Role**: Remediation Worker (implementer, qa, specialist)  
**Target Milestone**: Milestone 4 Remediation (Fedora Desktop Application & Interactive Terminal/OSD Player Launcher)  
**Recipient**: `orchestrator_3_gen2` (`a21e166c-91fa-462c-b129-2d84da07e92f`)  
**Working Directory**: `/home/gautham/syncplay-alt/.agents/teamwork/worker_p3_m4_fix`  
**Status**: **COMPLETE** (All critical defects and advisory recommendations remediated, verified, and regression-tested)

---

## 1. Observation

### 1.1 Root Cause Reproductions (Pre-Remediation)
1. **Unhandled URI Argument Decoding (`challenger_p3_m4_2/handoff.md:80-108`)**:
   - `assets/syncplay-alt.desktop` line 7 binds `Exec=syncplay-alt play %U`.
   - Before remediation, invoking `~/.local/bin/syncplay-alt inspect file:///tmp/test_synthetic.mkv` failed with:
     ```
     [INSPECT] Probing container: file:///tmp/test_synthetic.mkv
     [ERROR] Inspection failed: Io(Os { code: 2, kind: NotFound, message: "No such file or directory" })
     ```
   - Invoking percent-encoded target `~/.local/bin/syncplay-alt inspect "file:///tmp/test%20video.mkv"` failed identically with `Io(NotFound)`.
   - Inspection of `crates/syncplay-core/src/main.rs:47, 61, 314`, `crates/syncplay-core/src/coordinator.rs:296-299, 320-323`, and `crates/syncplay-core/src/interactive.rs:63` confirmed zero URI scheme stripping or percent-decoding logic existed prior to remediation.
2. **UTF-8 Multi-Byte String Slicing Panic (`challenger_p3_m4_1/handoff.md:91-106`)**:
   - In `crates/syncplay-core/src/interactive.rs:484-488`:
     ```rust
     let raw_title = format!(" SYNCPLAY-ALT — Target: {} ({})", self.target, container_desc);
     let display_title = if raw_title.len() > inner_width {
         &raw_title[..inner_width]
     } else {
         &raw_title
     };
     ```
   - When multibyte UTF-8 characters (e.g. CJK `'東'`, accented `'é'`, em-dash `'—'`) landed across `inner_width`, slicing `&raw_title[..inner_width]` panicked with `end byte index is not a char boundary`.
3. **Chat Bubble Dwell Time Reference (`challenger_p3_m4_1/handoff.md:107-113`)**:
   - In `crates/syncplay-core/src/interactive.rs:564`:
     ```rust
     let wall_secs = now.elapsed().as_secs_f64();
     ```
   - `now` was instantiated at line 385 on each tick iteration (`let now = Instant::now();`). Consequently, `now.elapsed()` returned sub-millisecond values instead of wall time elapsed since playback began, preventing the 4.0s dwell / 1.0s fade lifecycle from operating.

---

## 2. Logic Chain

1. **Pure-Rust Zero-Dependency URI Normalization (`normalize_target_path`)**:
   - Added module `crates/syncplay-common/src/path.rs` containing `normalize_target_path(target: &str) -> String` and `percent_decode(raw: &str) -> String`:
     * Strips `file://localhost`, `file://`, and `file:` prefixes cleanly.
     * Implements byte-level percent decoding: converts `%XX` hex digit pairs into raw bytes `(h1 << 4) | h2`, collects into `Vec<u8>`, and converts via `String::from_utf8` (with lossy UTF-8 fallback for malformed sequences).
     * If `target` is not a `file:` URI (e.g. raw filesystem paths, or synthetic tokens `"synthetic-mkv"`), it returns `target.to_string()` completely untouched, guaranteeing zero regressions on existing CLI and coordinator workflows.
   - Re-exported `normalize_target_path` from `syncplay_common` and `syncplay_core`.
2. **Subsystem Integration Across CLI, Coordinator, and Interactive Player**:
   - `crates/syncplay-core/src/main.rs`:
     * `inspect`: wrapped `args[2]` with `normalize_target_path(&args[2])`.
     * `play`: wrapped `args[i]` with `normalize_target_path(&args[i])`.
     * `party`: wrapped `--file` argument with `normalize_target_path(&args[idx])`.
   - `crates/syncplay-core/src/coordinator.rs`:
     * `PlayerCoordinator::inspect`: normalized `target` at entry prior to dispatching to synthetic vectors or filesystem probing.
     * `run_presentation_pipeline`: normalized `target` before file opening.
   - `crates/syncplay-core/src/interactive.rs`:
     * `InteractivePlayer::new`: normalized `target` at initialization before probing `media_info`.
3. **Safe Unicode UTF-8 Boundary Truncation**:
   - In `crates/syncplay-core/src/interactive.rs:484-500`:
     * Replaced byte slicing `&raw_title[..inner_width]` with Unicode character iterator truncation:
       ```rust
       let display_title: String = if raw_title.chars().count() > inner_width {
           raw_title.chars().take(inner_width).collect()
       } else {
           raw_title
       };
       let display_chars = display_title.chars().count();
       let pad_spaces = inner_width.saturating_sub(display_chars);
       ```
     * Prevents slicing across continuation bytes, eliminating non-character boundary panics.
4. **Player Monotonic Start Time for Chat Dwell Fading**:
   - Added `pub start_time: Instant` to `InteractivePlayer` struct.
   - Initialized `start_time: Instant::now()` in `InteractivePlayer::new`.
   - Reset `self.start_time = Instant::now()` at entry to `run_presentation_loop`.
   - Calculated `let wall_secs = self.start_time.elapsed().as_secs_f64()` at line 567 in the live chat overlay and `let wall_now = self.start_time.elapsed().as_secs_f64()` in `handle_command_bar_input`, restoring proper 4.0s dwell and 1.0s linear fadeout dynamics.
   - Guarded `is_interactive` with `self.max_ticks.is_none() && stdin().is_terminal() && stdout().is_terminal()`, ensuring bounded simulation test runs never hijack terminal raw mode or block on stdin.

---

## 3. Caveats

1. **Non-File URIs**:
   - `normalize_target_path` is explicitly scoped to `file:` URIs (`file://localhost`, `file://`, `file:`). Network streaming URIs (`http://`, `rtsp://`) are not local filesystem targets and are passed through untouched.
2. **Terminal Emulator TrueColor Rendering**:
   - The ANSI TrueColor HUD requires terminal emulators supporting 24-bit color (GNOME Terminal, Ptyxis, Foot, Kitty). On dumb or monochrome terminals, ANSI escape codes are stripped or ignored.

---

## 4. Conclusion

**Verdict: REMEDIATION_COMPLETE**

All requested fixes and adversarial findings have been resolved:
- Pure-Rust `normalize_target_path` implemented and integrated across CLI, coordinator, and interactive player.
- Multi-byte UTF-8 character boundary slicing in interactive HUD safely truncated using Unicode character iterators and padded via character count.
- Chat bubble dwell timer references the monotonic player start time.
- Linux packaging scripts and desktop application verified (`desktop-file-validate` 0 errors, GTK4 IconTheme discovery confirmed, installed binary executable).
- 100% of unit, integration, and empirical tests pass cleanly with zero compiler warnings and zero clippy warnings.

---

## 5. Verification Method

To independently verify all changes, run:

```bash
export PATH="$HOME/.cargo/bin:$PATH"

# 1. Dependency Blacklist Audit (Confirms zero banned monolithic players or database engines)
./scripts/audit_dependencies.sh

# 2. Workspace Zero-Warning Checks
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings

# 3. Unit and Integration Tests (100% PASS)
cargo test --workspace

# 4. Desktop Packaging & URI Execution Test Suite
python3 tests/empirical_challenge/test_desktop_packaging.py
python3 tests/empirical_challenge/test_ui_renderer.py

# 5. Pi Framework Intent Verification Harness (100.0% Compliance)
python3 scripts/intent_harness.py

# 6. Linux Installer Script Execution & Verification
./scripts/install_linux.sh

# 7. Direct CLI URI Reproduction Test
python3 -c '
SYNTHETIC_EBML_VECTOR = bytes([
    0x1A, 0x45, 0xDF, 0xA3, 0x9F, 0x42, 0x86, 0x81, 0x01, 0x42, 0xF7, 0x81, 0x01,
    0x42, 0xF2, 0x81, 0x04, 0x42, 0xF3, 0x81, 0x08, 0x42, 0x82, 0x88, ord("m"), ord("a"),
    ord("t"), ord("r"), ord("o"), ord("s"), ord("k"), ord("a"), 0x42, 0x87, 0x81, 0x04,
    0x18, 0x53, 0x80, 0x67, 0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF
])
with open("/tmp/test_synthetic.mkv", "wb") as f:
    f.write(SYNTHETIC_EBML_VECTOR)
'
~/.local/bin/syncplay-alt inspect file:///tmp/test_synthetic.mkv
~/.local/bin/syncplay-alt play file:///tmp/test_synthetic.mkv --ticks 2

# Verify percent-encoded spaces:
cp /tmp/test_synthetic.mkv "/tmp/test space video.mkv"
~/.local/bin/syncplay-alt inspect "file:///tmp/test%20space%20video.mkv"
~/.local/bin/syncplay-alt play "file:///tmp/test%20space%20video.mkv" --ticks 2
rm -f "/tmp/test space video.mkv" /tmp/test_synthetic.mkv
```

**Invalidation Conditions**:
- Any `Io(NotFound)` error when passing `file:///...` or percent-encoded URIs.
- Any panic when launching media with multibyte UTF-8 characters in filename or path.
- Any compiler warning under `#![deny(warnings)]` or clippy violation under `-D warnings`.
