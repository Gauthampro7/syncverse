## 2026-10-27T19:03:47Z
You are worker_video_controls, a teamwork_worker.
Your working directory is:
/home/gautham/syncverse/.agents/teamwork/worker_video_controls/

MANDATORY FIRST STEP:
Read the authoritative request file at:
/home/gautham/syncverse/.agents/teamwork/ORIGINAL_REQUEST.md

Also read:
/home/gautham/syncverse/AGENTS.md
/home/gautham/syncverse/.agents/teamwork/explorer_audit_1/handoff.md

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

SCOPE & EXCLUSIVE FILE OWNERSHIP:
You own Aspect Ratio Selection/Override, Playback Speed Control, and Video Adjustments in `syncverse-ui`:
- In `crates/syncverse-ui/src/gui/aspect_ratio.rs` (or `crates/syncverse-ui/src/gui/viewport.rs`):
  - Define `AspectRatioOverride` enum:
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
  - Implement methods on `AspectRatioOverride`:
    - `pub fn display_name(&self) -> &'static str`
    - `pub fn cycle_next(&self) -> Self`
    - `pub fn target_ratio(&self, natural_width: u32, natural_height: u32) -> Option<f32>`
  - Implement `ViewportGeometry::compute_with_aspect(available_w: f32, available_h: f32, video_w: u32, video_h: u32, aspect_override: AspectRatioOverride) -> ViewportGeometry`.
  - PRESERVE existing `ViewportGeometry::compute(available_w: f32, available_h: f32, video_w: u32, video_h: u32) -> ViewportGeometry` unchanged (calling `compute_with_aspect(..., AspectRatioOverride::Original)`) so all existing empirical tests pass without alteration.
  - Wire aspect ratio state and cycling into `VideoViewport`.
- In `crates/syncverse-ui/src/gui/controls.rs`:
  - Enhance speed selector controls (supporting 0.25x, 0.5x, 0.75x, 1.0x, 1.25x, 1.5x, 1.75x, 2.0x).
  - Add aspect ratio selector button/menu in controls bar.
- Add unit tests for `compute_with_aspect` across 16:9, 4:3, 21:9, 1:1, FillStretch, and Original aspect ratios.
- Verify `cargo test -p syncverse-ui` passes with zero warnings under `#![deny(warnings)]`.

Deliver your report to `/home/gautham/syncverse/.agents/teamwork/worker_video_controls/handoff.md` and send a message when done.
