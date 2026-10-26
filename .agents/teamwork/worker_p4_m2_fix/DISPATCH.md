# DISPATCH — worker_p4_m2_fix

## Mandatory Integrity Warning
> DO NOT CHEAT. All implementations must be genuine. DO NOT
> hardcode test results, create dummy/facade implementations, or
> circumvent the intended task. A teamwork_preview_auditor will independently
> verify your work. Integrity violations WILL be detected and your
> work WILL be rejected.

## Mission
You are `worker_p4_m2_fix`, tasked with remediating container demuxing and codec issues discovered during Milestone 2 empirical challenge and review.
Your working directory is `/home/gautham/syncplay-alt/.agents/teamwork/worker_p4_m2_fix`.

You must read:
1. `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` (specifically `## 2026-10-25T20:12:42Z`)
2. Challenger 1 report: `/home/gautham/syncplay-alt/.agents/teamwork/challenger_p4_m2_1/handoff.md`
3. Challenger 2 report: `/home/gautham/syncplay-alt/.agents/teamwork/challenger_p4_m2_2/handoff.md`
4. Target files:
   - `crates/syncplay-demux/src/universal.rs`
   - `crates/syncplay-demux/tests/universal_adversarial_challenge.rs`
   - `crates/syncplay-codecs/src/universal_video.rs`
   - `crates/syncplay-codecs/src/audio/universal.rs`

## Implementation Tasks

### Task 1: Fix FLV `seek_to(0)` and Seeking Bounds in `UniversalDemuxer`
In `crates/syncplay-demux/src/universal.rs`:
- When seeking (`seek_to`), compute `stream_target_us = target_us + self.start_time_us`.
- Try seeking backwards: `self.ictx.seek(stream_target_us, ..stream_target_us)`.
- If seeking backwards returns an error (such as `EPERM` when `stream_target_us` is 0 or less than the first keyframe's PTS), fallback to seeking forward to the earliest available keyframe:
  `self.ictx.seek(stream_target_us, stream_target_us..)` (or `self.ictx.seek(0, 0..)`).
- Ensure `seek_to(0)` on FLV (and any media where first keyframe PTS > 0) succeeds and lands on packet 0.

### Task 2: Normalize Stream Start-Time in `UniversalDemuxer`
In `crates/syncplay-demux/src/universal.rs`:
- In `UniversalDemuxer`, store `start_time_us: i64` (default `0`).
- In `init_from_context`, check `ictx.start_time()`. If `start_time > 0`, store it.
- In `read_packet`, normalize the packet presentation timestamp:
  Calculate stream PTS in microseconds (or milliseconds), subtract `start_time_us` (saturating at 0), and emit normalized `pts_ms` relative to media start (0ms).
- In `seek_to(target_pts_ms)`, calculate `stream_target_us = (target_pts_ms as i64 * 1000) + self.start_time_us`.
- Ensure MPEG-TS (`timescale_90k.ts`) starts at 0ms and can be navigated across its full duration.

### Task 3: Zero-Pad Buffer in `UniversalDemuxer::probe`
In `crates/syncplay-demux/src/universal.rs`:
- Allocate buffer with `AVPROBE_PADDING_SIZE` (32 bytes) margin:
  `let mut buf = vec![0u8; 4096 + 64];`
- Read up to 4096 bytes:
  `let n = reader.read(&mut buf[..4096])...;`
- Pass `buf[..n].as_mut_ptr()` and `buf_size: n as std::ffi::c_int` to `AVProbeData`, ensuring that the remaining bytes after index `n` are guaranteed to be zero.

### Task 4: Codec Hardening
In `crates/syncplay-codecs`:
- In `universal_video.rs`: Ensure `swscale` scaler context is invalidated/recreated if the input frame's pixel format, width, or height changes from previous frames.
- In `audio/universal.rs`: Ensure `UniversalAudioDecoder::from_codec_name` recognizes `"dts"` and `"dca"` as `ffmpeg::codec::Id::DTS`.

### Task 5: Verification
Execute:
1. `cargo test -p syncplay-demux --test universal_adversarial_challenge -- --nocapture`
2. `cargo check --workspace --all-targets`
3. `cargo clippy --workspace --all-targets -- -D warnings`
4. `cargo test --workspace`
5. `./scripts/audit_dependencies.sh`
6. `python3 scripts/intent_harness.py`
7. `./scripts/verify_phase4.sh`

## Output Requirements
Document all code changes, test outputs, and commands in `/home/gautham/syncplay-alt/.agents/teamwork/worker_p4_m2_fix/handoff.md`.
Send a completion message to the orchestrator.

## 2026-10-25T23:30:40Z
[Message from ee88cac4-07d4-4dad-bc7e-4c3f606ef03f]
You are `worker_p4_m2_fix`. Read your task assignment at `/home/gautham/syncplay-alt/.agents/teamwork/worker_p4_m2_fix/DISPATCH.md`.
Also read `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md`.
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

Implement the remediations for:
1. FLV seek(0) EPERM and fallback seek in UniversalDemuxer
2. MPEG-TS stream start-time offset normalization
3. AVProbeData buffer zero-padding
4. Video swscale dynamic format/dimension invalidation and audio DTS/DCA alias mapping
5. Run all verification commands (cargo check, clippy -D warnings, cargo test, verify_phase4.sh, audit_dependencies.sh, intent_harness.py).

Write your report to `/home/gautham/syncplay-alt/.agents/teamwork/worker_p4_m2_fix/handoff.md` and send a completion message to the orchestrator.

## 2026-10-25T23:39:20Z
[Message from ee88cac4-07d4-4dad-bc7e-4c3f606ef03f]
**Context**: Milestone 2 Remediation Implementation
**Content**: Checking in on your progress regarding Tasks 1–5 (FLV seek(0), start-time normalization, probe zero-padding, video swscale invalidation, and audio DTS alias mapping).
**Action**: Please report status or submit handoff.md when ready.


