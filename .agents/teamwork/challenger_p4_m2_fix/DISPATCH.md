# DISPATCH — challenger_p4_m2_fix

## Mission
You are `challenger_p4_m2_fix`, the verification challenger for Milestone 2 fixes.
Your working directory is `/home/gautham/syncplay-alt/.agents/teamwork/challenger_p4_m2_fix`.

You must read:
1. `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` (specifically `## 2026-10-25T20:12:42Z`)
2. Challenger 1 report: `/home/gautham/syncplay-alt/.agents/teamwork/challenger_p4_m2_1/handoff.md`
3. Worker fix report: `/home/gautham/syncplay-alt/.agents/teamwork/worker_p4_m2_fix/handoff.md`
4. Target files:
   - `crates/syncplay-demux/src/universal.rs`
   - `crates/syncplay-demux/tests/universal_adversarial_challenge.rs`
   - `crates/syncplay-codecs/src/universal_video.rs`
   - `crates/syncplay-codecs/src/audio/universal.rs`

## Verification Scope
1. Empirically verify that FLV `seek_to(0)` succeeds cleanly on packet 0 without `EPERM`.
2. Empirically verify that MPEG-TS stream start offset is normalized to 0ms across demuxing and seeking, allowing the entire clip duration to be navigated.
3. Verify that `AVProbeData` buffer has the mandated zero-padding margin.
4. Verify that `universal_codecs_adversarial_stress` tests for swscale dynamic invalidation and DTS/DCA alias mapping pass.
5. Execute verification commands:
   - `cargo test -p syncplay-demux --test universal_adversarial_challenge -- --nocapture`
   - `cargo test -p syncplay-codecs --test universal_codecs_adversarial_stress -- --nocapture`
   - `cargo check --workspace --all-targets`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo test --workspace`
   - `./scripts/audit_dependencies.sh`
   - `python3 scripts/intent_harness.py`
   - `./scripts/verify_phase4.sh`
6. Render a clear verdict: `APPROVE` or `REQUEST_CHANGES`.

## Output Requirements
Document all test outputs and your verdict in `/home/gautham/syncplay-alt/.agents/teamwork/challenger_p4_m2_fix/handoff.md`.
Send a completion message with your verdict to the orchestrator.

## 2026-10-25T23:45:34Z
You are `challenger_p4_m2_fix`. Read your task assignment at `/home/gautham/syncplay-alt/.agents/teamwork/challenger_p4_m2_fix/DISPATCH.md`.
Also read `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md`.
Empirically verify that the fixes implemented by `worker_p4_m2_fix` resolve all previously discovered defects:
- FLV seek_to(0) EPERM
- MPEG-TS start-time offset normalization
- AVProbeData buffer zero padding
- ScalerContext dynamic invalidation and DTS/DCA alias mapping
Run the verification suites. Render your verdict (APPROVE or REQUEST_CHANGES) in `/home/gautham/syncplay-alt/.agents/teamwork/challenger_p4_m2_fix/handoff.md` and send a message to the orchestrator.
