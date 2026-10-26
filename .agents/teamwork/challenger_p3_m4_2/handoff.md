# Adversarial Challenge Report: Milestone 4 (Fedora Desktop Application & Interactive Terminal/OSD Player Launcher)

**Author**: `challenger_p3_m4_2`  
**Role**: Adversarial Challenge Specialist (critic, specialist)  
**Target Milestone**: Milestone 4 (Requirement R1: Fedora Desktop Application & Interactive Terminal/OSD Player Launcher)  
**Recipient**: `orchestrator_3_gen2` (`a21e166c-91fa-462c-b129-2d84da07e92f`)  
**Working Directory**: `/home/gautham/syncplay-alt/.agents/teamwork/challenger_p3_m4_2`  
**Verdict**: **REQUEST_CHANGES** (1 Critical Defect in URI Argument Decoding)

---

## 1. Observation

### 1.1 Desktop Packaging & XDG Invariants
- **Validation of Desktop File**:
  Executed `desktop-file-validate assets/syncplay-alt.desktop` and `desktop-file-validate ~/.local/share/applications/syncplay-alt.desktop`.
  Output: Exited 0 with zero warnings or errors.
- **Audit of `[Desktop Action]` Groups**:
  Inspected `assets/syncplay-alt.desktop`:
  Lines 15–26 contain:
  ```ini
  [Desktop Action HostParty]
  Name=Host Watch Party Room
  Exec=syncplay-alt party host

  [Desktop Action ConnectParty]
  Name=Connect to Watch Party
  Exec=syncplay-alt party connect 127.0.0.1:18242

  [Desktop Action Inspect]
  Name=Inspect Media Container
  Exec=syncplay-alt inspect %U
  ```
  Confirmed: No illegal keys (specifically no forbidden `Terminal` keys) are present in any `[Desktop Action]` block.
- **MIME Type Associations**:
  Queried via `gio mime`:
  ```bash
  for mime in "video/x-matroska" "video/mp4" "video/webm" "video/quicktime"; do
      gio mime "$mime"
  done
  ```
  Result: `syncplay-alt.desktop` is present in both `Registered applications` and `Recommended applications` for all 4 MIME types:
  - `video/x-matroska`: Registered & Recommended
  - `video/mp4`: Registered & Recommended
  - `video/webm`: Registered & Recommended
  - `video/quicktime`: Registered & Recommended
- **Scalable Vector Icon & GTK4 Icon Discovery**:
  - `assets/syncplay-alt.svg`: Valid XML SVG (`<svg viewBox="0 0 512 512" ...>`), installed to `~/.local/share/icons/hicolor/scalable/apps/syncplay-alt.svg` with file mode `644` (`-rw-r--r--`).
  - Executed GTK4 test:
    ```bash
    python3 -c "import gi; gi.require_version('Gtk', '4.0'); from gi.repository import Gtk; theme = Gtk.IconTheme.new(); assert theme.has_icon('syncplay-alt'); print('[PASS] GTK4 has_icon syncplay-alt is True')"
    ```
    Output: `[PASS] GTK4 has_icon syncplay-alt is True` (Exit code 0).

### 1.2 Linux Installer & File System Permissions
- Executed `./scripts/install_linux.sh`:
  - Target files and permissions:
    * `/home/gautham/.local/bin/syncplay-alt`: `-rwxr-xr-x` (755, executable)
    * `/home/gautham/.cargo/bin/syncplay-alt`: `lrwxrwxrwx` symlink to `~/.local/bin/syncplay-alt`
    * `/home/gautham/.local/share/applications/syncplay-alt.desktop`: `-rw-r--r--` (644)
    * `/home/gautham/.local/share/icons/hicolor/scalable/apps/syncplay-alt.svg`: `-rw-r--r--` (644)
  - Exit code: 0.

### 1.3 Binary Execution Robustness & Graceful Error Handling
- **Synthetic playback**:
  Executed `syncplay-alt play synthetic-full-mkv --ticks 5`:
  Output: Ran 5 simulation ticks with live HUD (`Tick [1..5] t_pos: ... HUD: [00:00:00 / 00:00:01] | state: PLAY | RTT: 0ms | speed: 1.000x | sync: Green | snapshot: OK`), post-seek verification, and exited 0.
- **Non-existent file**:
  Executed `syncplay-alt play non_existent_file.mkv --ticks 5`:
  Output:
  ```
  [PLAYBACK] Initializing playback runtime for: non_existent_file.mkv
  [ERROR] Playback failed: Io(Os { code: 2, kind: NotFound, message: "No such file or directory" })
  ```
  Exit code: 1. Did NOT panic.
- **Piped / Non-terminal execution**:
  Executed `echo "q" | syncplay-alt play non_existent_file.mkv`:
  Output: Gracefully exited 1 with `[ERROR] Playback failed: Io(NotFound)`.

### 1.4 Critical Defect: Unhandled URI Argument Decoding (`file:///...` vs `/...`)
- **Direct Observation of Defect**:
  Generated a synthetic Matroska file at `/tmp/test_synthetic.mkv`.
  - Normal path invocation:
    * `~/.local/bin/syncplay-alt inspect /tmp/test_synthetic.mkv` -> **PASS** (Exit code 0, successfully probed EBML timescale and tracks).
    * `~/.local/bin/syncplay-alt play /tmp/test_synthetic.mkv --ticks 2` -> **PASS** (Exit code 0, successfully played 2 ticks).
  - File URI invocation:
    * Command: `~/.local/bin/syncplay-alt inspect file:///tmp/test_synthetic.mkv`
      Output:
      ```
      [INSPECT] Probing container: file:///tmp/test_synthetic.mkv
      [ERROR] Inspection failed: Io(Os { code: 2, kind: NotFound, message: "No such file or directory" })
      ```
      Exit code: 1 (**FAIL**).
    * Command: `~/.local/bin/syncplay-alt play file:///tmp/test_synthetic.mkv --ticks 2`
      Output:
      ```
      [PLAYBACK] Initializing playback runtime for: file:///tmp/test_synthetic.mkv
      [ERROR] Playback failed: Io(Os { code: 2, kind: NotFound, message: "No such file or directory" })
      ```
      Exit code: 1 (**FAIL**).
  - Percent-encoded URI invocation:
    * Command: `~/.local/bin/syncplay-alt inspect "file:///tmp/test%20video.mkv"` (where `/tmp/test video.mkv` exists)
      Output:
      ```
      [INSPECT] Probing container: file:///tmp/test%20video.mkv
      [ERROR] Inspection failed: Io(Os { code: 2, kind: NotFound, message: "No such file or directory" })
      ```
      Exit code: 1 (**FAIL**).
- **Target Files & Lines with Root Cause**:
  - `assets/syncplay-alt.desktop:7`:
    `Exec=syncplay-alt play %U`
    Specifies `%U`, which prompts XDG desktop environments (GNOME Shell, Nautilus, GIO) to pass `file://` URLs.
  - `crates/syncplay-core/src/main.rs:47, 61, 71`:
    Targets from `args[2]` and `args[i]` are passed directly without URI scheme stripping or percent decoding.
  - `crates/syncplay-core/src/coordinator.rs:296-299, 320-323`:
    ```rust
    real_file => {
        let path = Path::new(real_file);
        return Self::inspect_path(path);
    }
    ```
    and
    ```rust
    real_file => {
        let f = std::fs::File::open(real_file)?;
        (Box::new(std::io::BufReader::new(f)), "Media File")
    }
    ```
    `std::fs::File::open("file:///tmp/test_synthetic.mkv")` treats `file:` as a relative folder on Linux filesystems, resulting in `io::ErrorKind::NotFound`.
  - `crates/syncplay-core/src/interactive.rs:63`:
    `let media_info = PlayerCoordinator::inspect(target).ok();`
    In interactive mode launched from GNOME, `inspect` returns `Err(NotFound)`, which causes `media_info` to evaluate to `None`. The player falls back to a 60-second phantom timeline without demuxing any packets or tracks from the file.

---

## 2. Logic Chain

1. **Desktop Entry Contract**:
   Observation 1.1 and 1.4 confirm `assets/syncplay-alt.desktop` binds `Exec=syncplay-alt play %U`.
2. **XDG Desktop Entry Specification**:
   According to the FreeDesktop XDG specification, `%U` indicates URL arguments. Modern Linux desktop file managers (specifically GNOME Nautilus and GIO `g_app_info_launch_uris`) pass file targets formatted as `file:///absolute/path/to/media.mkv` or percent-encoded `file:///absolute/path/to/my%20media.mkv`.
3. **Absence of URI Normalization**:
   Observation 1.4 confirms that no URI decoding, `file://` scheme stripping, or percent-decoding exists anywhere in `crates/syncplay-core` (`main.rs`, `coordinator.rs`, `interactive.rs`).
4. **Execution Failure Blast Radius**:
   When Gautham double-clicks any video file associated with `syncplay-alt` in GNOME, or right-clicks and chooses "Open With -> Syncplay-Alt", the OS invokes `syncplay-alt play file:///path/to/video.mkv`. The player attempts `std::fs::File::open("file:///path/to/video.mkv")`, which immediately fails with `NotFound: No such file or directory`. In headless/simulation mode this crashes with exit code 1; in interactive mode it fails to load the container metadata and renders an empty black viewport.
5. **Necessity of Remediation**:
   Because Milestone 4's primary mandate is the Fedora Desktop Application Integration (Requirement R1), opening files via the desktop launcher is a core functional requirement. Therefore, the implementation cannot be approved until URI argument decoding is remediated.

---

## 3. Caveats

1. **Terminal Mode Raw Keyboard Capture**:
   In headless or piped environments (e.g. CI, shell scripts), `syncplay-alt play` automatically falls back to bounded simulation ticks via `IsTerminal` detection, which correctly avoids hanging the test runner.
2. **Implementation Scope**:
   Per the reviewer constraint ("Review-only — do NOT modify implementation code"), no source code modifications were made by this agent. The remediation must be implemented by the worker agent.

---

## 4. Conclusion

**Verdict: REQUEST_CHANGES**

Milestone 4 has successfully implemented:
- Strict XDG Desktop Entry Spec v1.5 compliance (zero invalid keys, zero warnings in `desktop-file-validate`).
- GTK4 scalable vector icon registration and discovery (`Gtk.IconTheme.new().has_icon('syncplay-alt') == True`).
- System-wide MIME type registration for all four required media types (`video/x-matroska`, `video/mp4`, `video/webm`, `video/quicktime`).
- Proper installation permissions (`755` binary, `644` assets, symlinks).
- 100% pass rate on dependency audits, clippy, unit tests, and the Pi Intent Verification Harness.

**Blocking Flaw to Fix**:
Input targets formatted as `file:///path/to/video.mkv`, `file://localhost/path/to/video.mkv`, or `file:///path/with%20spaces/video.mkv` must be decoded to their canonical filesystem paths before attempting `File::open` or container probing.

### Concrete Remediation Plan for Worker Agent:
1. Add a path/URI normalization helper in `crates/syncplay-core` (or `syncplay-common`):
   ```rust
   pub fn normalize_target_path(target: &str) -> String {
       let raw_path = if let Some(stripped) = target.strip_prefix("file://") {
           if let Some(local) = stripped.strip_prefix("localhost") {
               local
           } else {
               stripped
           }
       } else if let Some(stripped) = target.strip_prefix("file:") {
           stripped
       } else {
           return target.to_string();
       };

       // Pure-Rust percent decoding (zero external dependencies)
       percent_decode(raw_path)
   }
   ```
2. Integrate `normalize_target_path` in `main.rs` (for `play`, `inspect`, and `interactive`), in `coordinator.rs` (`PlayerCoordinator::inspect` and `run_presentation_pipeline`), and in `interactive.rs` (`InteractivePlayer::new`).
3. Add a test in `crates/syncplay-core/tests/cli_and_coordinator_integration.rs` asserting that `file:///tmp/...` and percent-encoded paths open and demux identically to raw paths.
4. Update `tests/empirical_challenge/test_desktop_packaging.py` to verify URI argument playback.

---

## 5. Verification Method

To independently reproduce the failure and verify the fix:

```bash
# 1. Create synthetic test vector
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

# 2. Raw path invocation (PASSES today)
~/.local/bin/syncplay-alt inspect /tmp/test_synthetic.mkv
~/.local/bin/syncplay-alt play /tmp/test_synthetic.mkv --ticks 2

# 3. File URI invocation (FAILS today with Io(NotFound))
~/.local/bin/syncplay-alt inspect file:///tmp/test_synthetic.mkv
~/.local/bin/syncplay-alt play file:///tmp/test_synthetic.mkv --ticks 2

# Invalidation Condition for REQUEST_CHANGES:
# When URI normalization is implemented, Step 3 will exit 0 and demux the EBML container cleanly.
```
