# Project: Syncverse Flawless Improvement & System Refinement

## Architecture
Syncverse is an open-source, ultra-efficient standalone native desktop media player and synchronized watch-party platform in pure Rust under the MIT License. It features custom universal container demuxing (`ffmpeg-next` + pure-Rust parsers), bitstream audio/video decoding, micro-speed discrete PI PLL synchronization over high-RTT WAN links, lock-free 128-byte cache-line aligned SPSC ring buffers, a strict 72-byte `PlayerUiSnapshot` triple-buffering architecture, and crash-resilient smart chat persistence (`syncplay_chat.bin`) with Unicode CJK inverted index full-text search.

---

## Feature Inventory
Every feature and improvement from the multi-angle survey:

| # | Feature | Description | Milestone | Source |
|---|---------|-------------|-----------|--------|
| 1 | PLL Micro-Speed Steering for Non-1.0x Rates | Update `PllSpeedController` to track $r_{\text{target}} \in [0.25, 4.0]$ and scale micro-speed steering range relative to $r_{\text{target}}$ (`[r_target * 0.96, r_target * 1.04]`). Update `playback_runtime.rs` to reanchor to $r_{\text{target}}$ in deadband Zone 1. | M1 | Survey Explorer 2 |
| 2 | 128-bit Integer Rational Chapter Timescale Conversion | Replace `f64` timescale calculation in `UniversalDemuxer::init_from_context` with 128-bit integer rational conversion per AGENTS.md Section 4 Rule 6. | M1 | Survey Explorer 2 |
| 3 | Subdirectory External Subtitle Scanning | Extend `scan_external_subtitles` in `syncverse-demux` to automatically discover subtitles in `subtitles/`, `Subtitles/`, `subs/`, `Subs/` folders. | M1 | Survey Explorer 2 |
| 4 | Soft-Knee Audio Downmix Limiter | Replace hard-clamping with zero-allocation soft-knee saturation (`tanh` curve above 0.85) in `universal.rs` and `playback_runtime.rs` to eliminate digital clipping buzz. | M1 | Survey Explorer 2 |
| 5 | MasterClock Unsafe Blocks Proof Comments | Add formal `// SAFETY:` justification comments proving exclusive mutable access in `crates/syncverse-clock/src/master_clock.rs` lines 186 and 202. | M1 | Survey Explorer 1 |
| 6 | Slash Command `/poll` Case-Insensitivity Panic Fix | Fix panic vulnerability in `crates/syncverse-chat/src/commands.rs:184` by safely slicing command remaining text without `.find("poll").unwrap()`. | M1 | Survey Explorer 1 |
| 7 | CoreError Hierarchy Enrichment & Strongly Typed Errors | Add `Common(#[from] CommonError)` to `CoreError`; eliminate untyped `String` and `Box<dyn Error>` returns in `playback_runtime.rs`. | M1 | Survey Explorer 1 |
| 8 | MediaTime String Parsing & Cross-Platform Path Hygiene | Implement `MediaTime::from_timecode_str` and `FromStr`; fix Windows drive letter normalization in `path.rs`; clean legacy docstrings. | M1 | Survey Explorer 1 |
| 9 | 72-Byte Snapshot Invariant Extension | Define `SNAPSHOT_FLAG_MUTED = 0x1000` and `SNAPSHOT_FLAG_LOOP_ENABLED = 0x2000` in `snapshot.rs`, verifying `size_of::<PlayerUiSnapshot>() == 72`. | M2 | Survey Explorer 3 |
| 10 | Canonical `SyncverseApp` Naming | Rename struct `SyncplayApp` to `SyncverseApp` in `syncverse-ui` with backward-compatible type alias `pub type SyncplayApp = SyncverseApp;`. | M2 | Survey Explorer 1 |
| 11 | Frame Stepping Controls | Implement single frame stepping backward (`,`) and forward (`.`) while paused in `syncverse-ui` and command dispatch. | M2 | Survey Explorer 3 |
| 12 | Hierarchical Quick Seeks | Implement ±5s (Arrows), ±30s (Shift+Arrows), and ±60s (Ctrl+Arrows) quick seeks in UI and menus. | M2 | Survey Explorer 3 |
| 13 | Loop Playback Mode | Implement toggleable repeat mode (`L` shortcut, menu item, `/loop` command) that automatically seeks to 0 on EOF. | M2 | Survey Explorer 3 |
| 14 | Configurable Subtitle Typography UI | Add font scale slider (50%–200%), color presets, background fill opacity, and outline shadow toggle in `SubtitleConfigDialog`. | M2 | Survey Explorer 3 |
| 15 | Windows Recent Files Resolution | Add `APPDATA` and `USERPROFILE` fallback resolution in `recent_files.rs` for Windows desktop installations. | M2 | Survey Explorer 1 |
| 16 | Expanded Slash Commands | Implement `/help`, `/clear` (resets local chat history), `/loop`, and `/who` in `SlashCommandParser` and `SlashCommandDispatcher`. | M3 | Survey Explorer 3 |
| 17 | Watch-Party Chat Integrated Search UI | Connect `InvertedIndexFts` engine to a toggleable search bar in the watch-party chat sidebar with instant query filtering. | M3 | Survey Explorer 3 |
| 18 | Responsive Controls Bar Layout Guard | Dynamically hide non-critical combo boxes when controls bar width $< 700\text{px}$ to prevent visual overflow on compact windows. | M3 | Survey Explorer 3 |
| 19 | Workspace-Wide Regression Prevention & Verification | Full verification via `./scripts/audit_dependencies.sh`, `cargo check --workspace --all-targets`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace -- --nocapture`, empirical challenges, intent harness, and `./scripts/verify_phase1.sh`. | M3 | System Requirements |

---

## Milestones

| # | Name | Scope | Dependencies | Status |
|---|------|-------|-------------|--------|
| M1 | Architectural Invariants, Standards Compliance & Mathematical Precision | PLL target rate tracking, 128-bit chapter timescale arithmetic, soft-knee audio downmixing, subdirectory subtitles, formal `// SAFETY:` comments, `/poll` panic fix, strongly typed errors, `MediaTime::from_timecode_str`, and docstring hygiene. | none | DONE |
| M2 | Desktop Player Enhancements, Controls & Snapshot Extension | Frame stepping (`,`, `.`), hierarchical quick seeks (±5s, ±30s, ±60s), loop playback mode, `SNAPSHOT_FLAG_MUTED` and `SNAPSHOT_FLAG_LOOP_ENABLED` (verifying 72B layout), subtitle typography styling UI, canonical `SyncverseApp` naming, and Windows path/config resolution. | M1 | DONE |
| M3 | Watch-Party Chat Polish, Integrated Search UI & Full Verification | Inverted index chat search UI in sidebar, expanded slash commands (`/help`, `/clear`, `/loop`, `/who`), responsive controls guard, and full-spectrum workspace verification (`audit_dependencies`, `cargo check`, `cargo clippy`, `cargo test`, empirical challenges, intent harness, `verify_phase1.sh`). | M1, M2 | DONE |

---

## Interface Contracts

### 1. `syncverse-sync` PLL Target Speed Controller (`crates/syncverse-sync/src/pll.rs`)
```rust
impl PllSpeedController {
    /// Updates PLL correction relative to an arbitrary target rate r_target.
    pub fn update_with_target_rate(
        &mut self,
        target_pts_ms: u64,
        actual_pts_ms: u64,
        dt_secs: f64,
        r_target: f64,
    ) -> PllCorrection;
}
```

### 2. `syncverse-codecs` Soft-Knee Audio Saturation (`crates/syncverse-codecs/src/audio/mod.rs`)
```rust
/// Smooth, zero-allocation soft-knee saturation curve for samples exceeding +/-0.85.
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

### 3. `syncverse-common` MediaTime Parsing (`crates/syncverse-common/src/time.rs`)
```rust
impl MediaTime {
    /// Parses a timecode string ("HH:MM:SS", "MM:SS", or "SS.mmm") into MediaTime.
    pub fn from_timecode_str(s: &str) -> Result<Self, CommonError>;
}
impl std::str::FromStr for MediaTime {
    type Err = CommonError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_timecode_str(s)
    }
}
```

### 4. `syncverse-ui` Snapshot Bitflags (`crates/syncverse-ui/src/snapshot.rs`)
```rust
pub const SNAPSHOT_FLAG_GATE_BLOCKED: u64 = 0x0100;
pub const SNAPSHOT_FLAG_FORCE_PLAY: u64   = 0x0200;
pub const SNAPSHOT_FLAG_LOCAL_READY: u64  = 0x0400;
pub const SNAPSHOT_FLAG_HAS_MISMATCH: u64 = 0x0800;
/// Status bitflag indicating audio output is currently muted.
pub const SNAPSHOT_FLAG_MUTED: u64        = 0x1000;
/// Status bitflag indicating loop / repeat playback mode is enabled.
pub const SNAPSHOT_FLAG_LOOP_ENABLED: u64 = 0x2000;
```

---

## Code Layout
- `crates/syncverse-common/src/time.rs`: `MediaTime` parsing and timecode formatting.
- `crates/syncverse-common/src/path.rs`: Windows drive letter path normalization.
- `crates/syncverse-demux/src/universal.rs`: 128-bit integer rational chapter timescale parsing.
- `crates/syncverse-demux/src/subtitles/scanner.rs`: Subdirectory subtitle auto-detection.
- `crates/syncverse-codecs/src/audio/`: Soft-knee limiter and downmixing protection.
- `crates/syncverse-clock/src/master_clock.rs`: Justified `// SAFETY:` comments.
- `crates/syncverse-sync/src/pll.rs`: Rate-aware micro-speed steering.
- `crates/syncverse-chat/src/commands.rs`: Panic-free `/poll` parser, new slash commands.
- `crates/syncverse-core/src/playback_runtime.rs`: Deadband rate re-anchoring, loop mode handling, error typing.
- `crates/syncverse-ui/src/snapshot.rs`: 72-byte snapshot layout, mute and loop bitflags.
- `crates/syncverse-ui/src/gui/app.rs`: Frame stepping, hierarchical quick seeks, loop toggle, canonical naming.
- `crates/syncverse-ui/src/gui/dialogs.rs`: Subtitle appearance styling UI.
- `crates/syncverse-ui/src/gui/sidebar.rs`: Integrated chat search UI.
