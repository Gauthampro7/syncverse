## 2026-10-27T19:03:34Z
You are worker_audio_mgmt, a teamwork_worker.
Your working directory is:
/home/gautham/syncverse/.agents/teamwork/worker_audio_mgmt/

MANDATORY FIRST STEP:
Read the authoritative request file at:
/home/gautham/syncverse/.agents/teamwork/ORIGINAL_REQUEST.md

Also read:
/home/gautham/syncverse/AGENTS.md
/home/gautham/syncverse/.agents/teamwork/explorer_audit_1/handoff.md

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

SCOPE & EXCLUSIVE FILE CARD OWNERSHIP:
You own Multi-Audio Track Switching, Audio Delay Pacing, Volume/Mute in clock & core:
- In `crates/syncverse-clock/src/audio_pacer.rs`:
  - Add atomic audio delay support: `audio_delay_ms: AtomicI64` to `AudioPacer`.
  - Add methods `pub fn set_audio_delay_ms(&self, delay_ms: i64)`, `pub fn audio_delay_ms(&self) -> i64`, and `pub fn adjust_audio_delay_ms(&self, delta_ms: i64) -> i64`.
  - In `evaluate_timing`, factor in `delay_ms`: `effective_pts = (frame_pts_ms as i64 + delay_ms) as f64`.
  - Ensure zero heap allocation on audio pacing hot path.
- In `crates/syncverse-core/src/playback_runtime.rs`:
  - Track all available audio tracks in `PlaybackSession` (store `available_audio_tracks: Vec<TrackInfo>`).
  - Implement dynamic audio track switching:
    - Handle `PlayerCommand::SelectAudioTrack(track_id: u64)`: update `audio_track_id`, construct a new `UniversalAudioDecoder` for the selected stream, reset the audio worker sink to flush stale buffers.
    - Handle `PlayerCommand::CycleAudioTrack`: sequentially advance to the next discovered audio track in `available_audio_tracks` (cycling back to index 0), and emit a notification / event.
    - Handle `PlayerCommand::SetAudioDelay(delay_ms: i64)` and `PlayerCommand::AdjustAudioDelay(delta_ms: i64)`.
  - Ensure volume scaling [0.0 - 1.0] and mute toggle are cleanly handled without audio clipping or crackle.
- Add unit tests verifying `AudioPacer` delay calculation and audio track selection logic.
- Verify `cargo test -p syncverse-clock -p syncverse-core` passes with zero warnings under `#![deny(warnings)]`.

Deliver your report to `/home/gautham/syncverse/.agents/teamwork/worker_audio_mgmt/handoff.md` and send a message when done.
