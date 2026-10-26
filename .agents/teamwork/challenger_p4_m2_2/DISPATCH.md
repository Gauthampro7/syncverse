# DISPATCH — challenger_p4_m2_2

## Mission
You are `challenger_p4_m2_2`, an empirical challenger and stress tester for Milestone 2 (Universal Codecs & Memory Alignment).
Your working directory is `/home/gautham/syncplay-alt/.agents/teamwork/challenger_p4_m2_2`.

You must read:
1. `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` (specifically `## 2026-10-25T20:12:42Z`)
2. `/home/gautham/syncplay-alt/.agents/teamwork/orchestrator_4/PROJECT.md`
3. Target code files:
   - `crates/syncplay-codecs/src/universal_video.rs`
   - `crates/syncplay-codecs/src/audio/universal.rs`
   - `crates/syncplay-codecs/src/frame.rs`
   - `crates/syncplay-codecs/src/aligned_buffer.rs`
   - `crates/syncplay-clock/src/audio_pacer.rs`

## Empirical Challenge Scope
1. Empirically challenge `UniversalVideoDecoder`:
   - Test decoding across video codecs: H.264, H.265, VP8, VP9, AV1, MPEG-2, MPEG-4, and VC-1.
   - Verify strict memory alignment invariants:
     * Planar buffers (`y_plane`, `u_plane`, `v_plane`) aligned to 64-byte memory boundaries (`buf.as_ptr() as usize % 64 == 0`).
     * Row strides strictly 16-byte aligned (`stride == (width + 15) & !15`).
     * +16B SIMD tail padding present and zeroed for safe vector loads.
   - Test non-standard resolutions (odd width/height, e.g. 191x107, 720x480, 1280x720, 1920x1080) to ensure zero out-of-bounds reads.
2. Empirically challenge `UniversalAudioDecoder`:
   - Test decoding and resampling across audio codecs: AAC, MP3, Opus, Vorbis, FLAC, AC3, E-AC3, DTS, and PCM.
   - Verify normalized output format: 48,000 Hz, 2-channel stereo, `f32` in `[-1.0, 1.0]`.
   - Test handling of unusual channel configurations (mono, 5.1 surround, 7.1) and sample rates (8kHz, 22.05kHz, 44.1kHz, 96kHz).
3. Run verification checks:
   - `cargo test --workspace`
   - `python3 tests/empirical_challenge/test_demux_codecs_clock.py`
4. Render a clear verdict: `APPROVE` or `REQUEST_CHANGES`.

## Output Requirements
Document all challenge tests, data points, and verdict in `/home/gautham/syncplay-alt/.agents/teamwork/challenger_p4_m2_2/handoff.md`.
Send a completion message with your verdict to the orchestrator.


## 2026-10-25T21:45:15Z
From: ee88cac4-07d4-4dad-bc7e-4c3f606ef03f
You are `challenger_p4_m2_2`. Read your instructions at `/home/gautham/syncplay-alt/.agents/teamwork/challenger_p4_m2_2/DISPATCH.md`.
Also read `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md`.
Empirically stress test UniversalVideoDecoder and UniversalAudioDecoder:
- Check all 8 video codecs (H.264, H.265, VP8, VP9, AV1, MPEG-2, MPEG-4, VC-1).
- Check strict 64-byte alignment, 16-byte row strides, and +16B SIMD tail padding zeroing.
- Check all 9 audio codecs and 48kHz stereo f32 resampling.
Write your report to `/home/gautham/syncplay-alt/.agents/teamwork/challenger_p4_m2_2/handoff.md` and send a message with your verdict (APPROVE or REQUEST_CHANGES) to the orchestrator.


## 2026-10-25T21:55:54Z
From: ee88cac4-07d4-4dad-bc7e-4c3f606ef03f
**Context**: Milestone 2 Gate Status
**Content**: Reviewer 1, Reviewer 2, Auditor, and Challenger 1 have delivered their handoffs. Please report your current status on the codec empirical stress tests and memory alignment checks.
**Action**: Send your findings and handoff report when ready.
