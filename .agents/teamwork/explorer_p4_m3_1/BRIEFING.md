# BRIEFING — 2026-10-25T23:55:00Z

## Mission
Investigate and design Native Desktop Windowing (eframe/egui/wgpu on Wayland/X11 & Windows, lifecycle, fullscreen F11/double-click) and GPU-Accelerated Video Rendering Viewport (aspect-ratio preservation letterbox/pillarbox, egui texture updates from VideoFrameRGBA, vsync pacing) for Milestone 3.

## 🔒 My Identity
- Archetype: explorer
- Roles: investigation, architectural design, synthesis
- Working directory: /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m3_1
- Original parent: ee88cac4-07d4-4dad-bc7e-4c3f606ef03f
- Milestone: Phase 4 Milestone 3 (Desktop Graphical UI - Windowing & Viewport)

## 🔒 Key Constraints
- Read-only investigation — do NOT implement code changes directly in source tree.
- Communicate proposals, architectural designs, and prototype code via handoff.md.
- Follow AGENTS.md rules and project invariants: no external monolithic players (mpv, vlc), no external database engines, strictly idiomatic Rust, zero warnings, memory safety.
- Strictly adhere to Fedora Linux (Wayland/X11) and Windows cross-platform desktop standards.

## Current Parent
- Conversation ID: ee88cac4-07d4-4dad-bc7e-4c3f606ef03f
- Updated: 2026-10-25T23:55:00Z

## Investigation State
- **Explored paths**:
  - `crates/syncplay-ui/` (`lib.rs`, `snapshot.rs`, `render.rs`, `command.rs`, `scrubber.rs`, `overlay.rs`, `terminal.rs`)
  - `crates/syncplay-codecs/src/frame.rs` (`VideoFrameRGBA`, row stride alignment)
  - `crates/syncplay-clock/src/master_clock.rs` (`evaluate_presentation_timing`, `SeqLock`)
  - `crates/syncplay-core/src/coordinator.rs` (`SpscRingBuffer<VideoFrameRGBA, 32>`, presentation loops)
  - `tests/empirical_challenge/test_ui_renderer.py` (Test 9 aspect ratio calculation, letterboxing/pillarboxing)
  - `openspec/specs/ui-renderer/spec.md` (Wayland/X11, VSync, 2.5s auto-hide)
  - Host environment (Fedora Wayland Intel Iris/Arc GPU, Mesa 26.2.5, Vulkan 1.4, rustc 1.99.1)
- **Key findings**:
  - `eframe` v0.37 has native Wayland, X11, and WGPU enabled by default.
  - VideoFrameRGBA uses 64-byte row alignment (`stride = (((width * 4) + 63) & !63)`). Standard 1080p/720p/4K has 0 padding (exact zero-copy slice borrow). Non-standard resolutions require row-by-row packing into reusable buffer.
  - Aspect ratio preservation uses exact mathematical formulas verified across 10 empirical cases.
  - Fullscreen toggle via F11 / double-click with 2.5s cursor & controls auto-hide state machine.
  - Texture upload optimization: check frame PTS; call `handle.set()` in-place; trigger repaint only on new frame arrival or presentation deadline.
- **Unexplored areas**: Milestone 3 scope completed for Windowing & Viewport.

## Key Decisions Made
- Architecture split: `window.rs` (NativeOptions, FullscreenState), `viewport.rs` (ViewportGeometry, VideoTextureBuffer, VideoViewport), and `app.rs` (`SyncplayApp` eframe runner).
- Safe memory handling: Avoid unsafe blocks in syncplay-ui; use slice borrowing for standard resolutions and reusable `Vec<u8>` for padded strides.
- Synchronization: Driven by `MasterClock::evaluate_presentation_timing` returning `Wait`, `PresentNow`, `PresentNowLate`, `DropLateFrame`.

## Artifact Index
- /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m3_1/DISPATCH.md — Dispatch instructions
- /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m3_1/BRIEFING.md — Persistent context & identity
- /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m3_1/progress.md — Liveness heartbeat
- /home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m3_1/handoff.md — 5-component architectural handoff report
