# Independent Quality & Adversarial Review Report: Milestone 4 Remediation
**Requirement R1: Fedora Desktop Application & Interactive Terminal/OSD Player Launcher**

- **Reviewer Agent**: `reviewer_p3_m4_fix` (Roles: `reviewer`, `critic`)
- **Target Work Product**: Milestone 4 Remediation by `worker_p3_m4_fix`
- **Target Crate / Scope**: `crates/syncplay-common/src/path.rs`, `crates/syncplay-core/src/main.rs`, `crates/syncplay-core/src/coordinator.rs`, `crates/syncplay-core/src/interactive.rs`, `tests/empirical_challenge/test_desktop_packaging.py`
- **Authoritative Invariants**: `AGENTS.md`, `PROJECT.md`, `ORIGINAL_REQUEST.md` (## 2026-10-25T15:21:44Z)
- **Review Verdict**: **APPROVE**
- **Risk Assessment**: **LOW**

---

## 1. Observation

Direct forensic inspection of the codebase, git workspace, and runtime execution revealed the following exact observations:

### 1.1 URI Path Normalization (`crates/syncplay-common/src/path.rs`)
- `from_hex_digit(b: u8) -> Option<u8>`: Lines 8–15 implement pure-Rust ASCII hex decoding for `0..=9`, `a..=f`, and `A..=F` with `#[inline]`.
- `percent_decode(raw: &str) -> String`: Lines 22–41 implement single-pass, byte-level percent decoding. It allocates `Vec<u8>::with_capacity(bytes.len())`, detects `%` followed by two valid hex digits within bounds (`i + 2 < bytes.len()`), pushes decoded bytes `(h1 << 4) | h2`, advances `i += 3`, and falls back to verbatim byte emission on incomplete or non-hex patterns. In lines 39–40, UTF-8 conversion is guarded:
  ```rust
  String::from_utf8(decoded)
      .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
  ```
  preventing any panic on invalid UTF-8 byte sequences.
- `normalize_target_path(target: &str) -> String`: Lines 51–65 strip prefixes in order:
  1. `target.strip_prefix("file://")`: checks for `localhost` and strips if present (`local`), otherwise returns `stripped`.
  2. `target.strip_prefix("file:")`: strips `file:`.
  3. Non-`file:` targets (e.g. raw filesystem paths `/home/...`, relative paths, or synthetic tokens `"synthetic-mkv"`): immediately returned via `target.to_string()`, ensuring zero regressions for non-URI targets.
  4. Returns `percent_decode(raw_path)`.
- Zero external dependencies: `crates/syncplay-common/src/path.rs` imports only from `std`. Zero third-party URI crates (`url`, `percent-encoding`, etc.) were introduced.

### 1.2 Subsystem Integration Across Core Entrypoints
- `crates/syncplay-common/src/lib.rs:15`: `pub use path::normalize_target_path;` re-exports the function.
- `crates/syncplay-core/src/lib.rs:18`: Re-exports `pub use syncplay_common::normalize_target_path;`.
- `crates/syncplay-core/src/main.rs`:
  * Line 49: `inspect_media(&normalize_target_path(&args[2]));`
  * Line 63: `target = normalize_target_path(&args[i]);` in `play` argument parser.
  * Line 316: `file = Some(normalize_target_path(&args[idx]));` in `party` `--file` parser.
- `crates/syncplay-core/src/coordinator.rs`:
  * Line 280: `let normalized = normalize_target_path(target);` in `PlayerCoordinator::inspect`.
  * Line 314: `let normalized = normalize_target_path(target);` in `PlayerCoordinator::run_presentation_pipeline`.
- `crates/syncplay-core/src/interactive.rs`:
  * Line 66: `let normalized = normalize_target_path(target);` in `InteractivePlayer::new`.

### 1.3 Safe Unicode Title Truncation (`crates/syncplay-core/src/interactive.rs:488–505`)
- Pre-remediation byte-slice `&raw_title[..inner_width]` was replaced with character iterator truncation:
  ```rust
  let display_title: String = if raw_title.chars().count() > inner_width {
      raw_title.chars().take(inner_width).collect()
  } else {
      raw_title
  };
  let display_chars = display_title.chars().count();
  let pad_spaces = inner_width.saturating_sub(display_chars);
  ```
- Because `.chars().take(inner_width)` yields Unicode scalar values, slicing across UTF-8 continuation bytes is structurally impossible. Padding uses `inner_width.saturating_sub(display_chars)` with character counting.

### 1.4 Chat Dwell Timer Reference (`crates/syncplay-core/src/interactive.rs`)
- `InteractivePlayer` contains `pub start_time: Instant` (line 60), initialized at construction (line 94) and re-anchored at `run_presentation_loop` entry (line 383: `self.start_time = Instant::now()`).
- Live chat overlay in line 568 computes:
  ```rust
  let wall_secs = self.start_time.elapsed().as_secs_f64();
  ```
- Command bar user input in line 325 computes:
  ```rust
  let wall_now = self.start_time.elapsed().as_secs_f64();
  self.chat_overlay.on_message(line, wall_now);
  ```
- Both producer and consumer utilize the same monotonic baseline `self.start_time`, activating the 4.0s dwell and 1.0s linear fadeout lifecycle (`calculate_alpha_dwell_fade`).
- Line 347 guards terminal raw mode with `self.max_ticks.is_none() && stdin().is_terminal() && stdout().is_terminal()`, ensuring automated test suites never hang on stdin.

### 1.5 Verification Suite Execution Results
Verbatim tool executions yielded:
- `./scripts/audit_dependencies.sh`: Clean (19 banned patterns checked, 0 violations).
- `cargo check --workspace --all-targets`: Clean in 0.01s (0 warnings).
- `cargo clippy --workspace --all-targets -- -D warnings`: Clean in 0.03s (0 warnings).
- `cargo test --workspace`: 100% pass across all unit and integration test targets.
- `python3 tests/empirical_challenge/test_desktop_packaging.py`: Clean pass across all 5 verification phases:
  * Phase 1: SVG vector asset structure (`viewBox="0 0 512 512"`) -> PASS.
  * Phase 2: `desktop-file-validate` on `assets/syncplay-alt.desktop` -> 0 errors.
  * Phase 3: Installed desktop entry and binary (`~/.local/bin/syncplay-alt`) -> PASS.
  * Phase 4: GTK4 / GIO icon theme discovery -> PASS.
  * Phase 5: `file://` and percent-encoded URI execution (`inspect` and `play`) -> PASS.
- `python3 scripts/intent_harness.py`: All 8 intents passed with 100.0% Compliance Score (0 drift).
- `python3 tests/empirical_challenge/test_milestone4_interactive_stress.py`: All 16 Rust empirical stress tests passed cleanly in 0.13s, with Python mathematical oracles for scrubber/HUD and desktop packaging passing.

---

## 2. Logic Chain

1. **Root Cause Resolution**:
   - `challenger_p3_m4_2` discovered that passing `file:///...` or percent-encoded URIs caused `Io(NotFound)` because the literal URI string was handed directly to filesystem `open()`.
   - `worker_p3_m4_fix` created `normalize_target_path` and `percent_decode` in `crates/syncplay-common/src/path.rs`.
   - Calling `normalize_target_path` at every external input boundary (`main.rs`, `coordinator.rs`, `interactive.rs`) ensures all representations (`file:///path`, `file://localhost/path`, `file:/path`, `file:///path%20with%20spaces`) resolve to the exact local filesystem path before any file I/O occurs.
2. **Crash & Panic Elimination**:
   - `challenger_p3_m4_1` discovered that multibyte UTF-8 characters caused `&raw_title[..inner_width]` to panic with `byte index is not a char boundary`.
   - By switching to Unicode `.chars().take(inner_width).collect::<String>()` and `.chars().count()`, the string is sliced strictly at valid codepoint boundaries. This completely eliminates byte-slicing panics.
3. **Chat Lifecycle Restoration**:
   - Previously `now.elapsed()` was evaluated against an `Instant::now()` created inside the tick loop, yielding `dt ~ 0` every tick and preventing chat message aging.
   - Anchoring `self.start_time` at loop entry and computing `self.start_time.elapsed().as_secs_f64()` allows `wall_secs` to advance monotonically, enabling accurate 4.0s dwell and 1.0s linear alpha fadeout.
4. **Architectural & Integrity Adherence**:
   - No external libraries were added; zero dependency rules (`AGENTS.md`) remain strictly preserved.
   - No hardcoded test responses or facade bypasses were found. Real hex parsing and URI stripping is executed.

---

## 3. Adversarial Challenges & Stress Testing

### 3.1 Challenge: Malformed & Path-Traversal Percent-Encoding
- **Assumption Challenged**: `percent_decode` must not decode malformed percent sequences or enable path traversal vulnerabilities.
- **Attack Scenario**:
  1. Trailing `%` (`file:///tmp/movie%`)
  2. Single hex digit (`file:///tmp/movie%2`)
  3. Non-hex characters (`file:///tmp/movie%ZZ`)
  4. Null-byte injection (`file:///tmp/movie%00.mkv`)
  5. Multi-byte invalid UTF-8 (`file:///tmp/movie%FF%FE.mkv`)
  6. Double-decoding traversal (`file:///tmp/test%252Fescape.mkv`)
- **Stress Test Verification**:
  * Trailing `%`, single hex digit, and non-hex characters are emitted verbatim without index out of bounds or panics.
  * `%00` decodes safely into a null byte; subsequent `std::fs::File::open` safely errors with `InvalidInput` without crashing.
  * `%FF%FE` fails `String::from_utf8` and losslessly replaces invalid sequences with `\u{FFFD}` via `String::from_utf8_lossy`.
  * `%252F` decodes once to `%2F`, preventing double-decoding directory traversal exploits.
- **Result**: **PASS**.

### 3.2 Challenge: Literal Percent in Filename vs URI Normalization
- **Assumption Challenged**: A file containing a literal `%` on disk (e.g. `/tmp/test 50% discount.mkv`) must be accessible via both raw CLI path and properly encoded `file://` URI (`%25`).
- **Attack Scenario**: Create `/tmp/test 50% discount.mkv` and invoke:
  1. `syncplay-alt inspect "/tmp/test 50% discount.mkv"`
  2. `syncplay-alt inspect "file:///tmp/test%2050%25%20discount.mkv"`
- **Stress Test Verification**:
  * Raw CLI path does not start with `file:`, so `normalize_target_path` passes it through untouched. Inspection succeeds.
  * URI encodes `%` as `%25`, which decodes to `%`. Inspection succeeds.
- **Result**: **PASS**.

### 3.3 Challenge: Multilingual CJK and Accented Character Titles in Interactive Player
- **Assumption Challenged**: Running the interactive player on files with CJK and accented characters must never crash, regardless of terminal column width.
- **Attack Scenario**: Target `"file:///tmp/%E6%9D%B1%E4%BA%AC_%C3%A9l%C3%A9phant.mkv"` (`/tmp/東京_éléphant.mkv`) invoked with `with_max_ticks(Some(2))` across narrow widths.
- **Stress Test Verification**:
  * Unit tests and independent CLI executions verified that `InteractivePlayer` decodes the URI, probes the file, formats the title line, and completes the presentation loop without panicking.
- **Result**: **PASS**.

---

## 4. Caveats

1. **CJK Fullwidth Monospace Column Alignment**:
   `raw_title.chars().count()` counts Unicode scalar values rather than terminal monospace column cells (wcwidth). In terminals with monospace fonts, fullwidth CJK characters occupy 2 column cells, which may visually shift the right border `│` by 1 cell per CJK character in narrow terminal views. This does not crash or panic the engine, and preserving zero external dependencies (avoiding `unicode-width` crate) is the correct architectural trade-off.
2. **Network Streaming URIs**:
   `normalize_target_path` is explicitly scoped to local `file:` schemes. Non-file URIs (`http://`, `rtsp://`) are passed through untouched as intended.

---

## 5. Conclusion

**Verdict: APPROVE**

The remediation implemented by `worker_p3_m4_fix` is complete, robust, and mathematically sound:
1. Pure-Rust `normalize_target_path` and `percent_decode` correctly resolve `file://localhost`, `file://`, `file:`, and percent-encoded paths without external dependencies.
2. All entry points (`main.rs`, `coordinator.rs`, `interactive.rs`) are properly integrated.
3. Multi-byte UTF-8 string slicing panics are eliminated by character iterator truncation.
4. Chat bubble dwell fading operates reliably against the player's monotonic start time.
5. 100% of lint, audit, unit, integration, intent, and packaging tests pass.

---

## 6. Verification Method

To independently reproduce this verification:

```bash
export PATH="$HOME/.cargo/bin:$PATH"

# 1. Dependency Audit
./scripts/audit_dependencies.sh

# 2. Strict Warning & Clippy Check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings

# 3. Unit and Integration Test Suite
cargo test --workspace

# 4. Desktop Packaging & URI Execution Test Suite
python3 tests/empirical_challenge/test_desktop_packaging.py

# 5. Interactive Player Adversarial Stress Suite
python3 tests/empirical_challenge/test_milestone4_interactive_stress.py

# 6. Intent Verification Harness
python3 scripts/intent_harness.py

# 7. Independent Adversarial URI Verification
python3 -c '
import subprocess, os
from pathlib import Path

bin_path = os.path.expanduser("~/.local/bin/syncplay-alt")
SYNTHETIC_EBML_VECTOR = bytes([
    0x1A, 0x45, 0xDF, 0xA3, 0x9F, 0x42, 0x86, 0x81, 0x01, 0x42, 0xF7, 0x81, 0x01,
    0x42, 0xF2, 0x81, 0x04, 0x42, 0xF3, 0x81, 0x08, 0x42, 0x82, 0x88, ord("m"), ord("a"),
    ord("t"), ord("r"), ord("o"), ord("s"), ord("k"), ord("a"), 0x42, 0x87, 0x81, 0x04,
    0x18, 0x53, 0x80, 0x67, 0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF
])

test_files = [
    ("/tmp/test_plain.mkv", "file:///tmp/test_plain.mkv"),
    ("/tmp/test with spaces.mkv", "file:///tmp/test%20with%20spaces.mkv"),
    ("/tmp/test_東京_café.mkv", "file:///tmp/test_%E6%9D%B1%E4%BA%AC_caf%C3%A9.mkv"),
    ("/tmp/test_local.mkv", "file://localhost/tmp/test_local.mkv"),
    ("/tmp/test_singleslash.mkv", "file:/tmp/test_singleslash.mkv"),
]

for filepath, uri in test_files:
    p = Path(filepath)
    p.write_bytes(SYNTHETIC_EBML_VECTOR)
    try:
        r1 = subprocess.run([bin_path, "inspect", uri], capture_output=True, text=True)
        assert r1.returncode == 0, f"inspect failed for {uri}: {r1.stderr}"
        assert "EBML/MKV probe match:" in r1.stdout and "YES" in r1.stdout
        
        r2 = subprocess.run([bin_path, "play", uri, "--ticks", "2"], capture_output=True, text=True)
        assert r2.returncode == 0, f"play failed for {uri}: {r2.stderr}"
        assert "Tick [1]" in r2.stdout
        
        r3 = subprocess.run([bin_path, "inspect", filepath], capture_output=True, text=True)
        assert r3.returncode == 0, f"inspect failed for {filepath}: {r3.stderr}"
        assert "EBML/MKV probe match:" in r3.stdout and "YES" in r3.stdout
        print(f"PASSED: {filepath} <-> {uri}")
    finally:
        if p.exists():
            p.unlink()
print("All adversarial URI and UTF-8 path variants PASSED!")
'
```
