# DISPATCH — explorer_p4_m3_1

## Mission
You are `explorer_p4_m3_1`, exploring the Desktop Windowing & Video Viewport architecture for Milestone 3 (Flawless Desktop Graphical Player UI).
Your working directory is `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m3_1`.

You must read:
1. `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` (specifically `## 2026-10-25T20:12:42Z` R1 requirements)
2. `/home/gautham/syncplay-alt/.agents/teamwork/orchestrator_4/PROJECT.md`
3. Codebase files:
   - `crates/syncplay-ui/Cargo.toml`
   - `crates/syncplay-ui/src/lib.rs`
   - `crates/syncplay-ui/src/gui/` (existing files if any)
   - `crates/syncplay-ui/src/renderer.rs`
   - `crates/syncplay-core/src/coordinator.rs`

## Investigation Scope
1. **Windowing & Application Lifecycle**:
   - `eframe` setup and `NativeOptions`:
     * Window title: `"Syncplay-Alt - Desktop Media Player"`.
     * Initial size: 1280x720, resizable, min size 640x360.
     * Wayland and X11 compatibility on Fedora Linux.
     * Fullscreen toggle (`F11` key and double-clicking the video viewport).
     * Clean window close handling.
2. **GPU-Accelerated Video Viewport**:
   - Aspect-ratio letterboxing / pillarboxing:
     * Calculate display rectangle centered in available viewport while strictly preserving `video_width / video_height`.
     * Black background filling outside the video rect.
   - Frame texture updates:
     * Receiving `VideoFrameRGBA` from `SpscRingBuffer<VideoFrameRGBA, 32>` (or via `PlayerUiSnapshot` / coordinator channel).
     * Updating `egui::ColorImage` and `egui::TextureHandle`.
     * Minimizing texture upload overhead (upload only when a new video frame PTS is presented).
   - Vsync pacing & repaint requests: `ctx.request_repaint()` vs `request_repaint_after()`.

## Output Requirements
Document your architectural design, data structures, and draft code in `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m3_1/handoff.md`.
Send a completion message to the orchestrator.


## 2026-10-25T23:51:30Z
Sender: ee88cac4-07d4-4dad-bc7e-4c3f606ef03f
Content: You are `explorer_p4_m3_1`. Read `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m3_1/DISPATCH.md`.
Also read `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` (specifically R1 requirements).
Investigate and design:
1. Native Desktop Windowing (`eframe`/`egui`/`wgpu` on Wayland and X11 for Fedora Linux and Windows), window lifecycle, resizing, and fullscreen toggling (F11 and double-click).
2. GPU-Accelerated Video Rendering Viewport: automatic letterboxing/pillarboxing maintaining aspect ratio, egui texture updates from VideoFrameRGBA, and vsync pacing.
Write your design and prototype code to `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m3_1/handoff.md` and send a message to the orchestrator.
