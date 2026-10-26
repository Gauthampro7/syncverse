# DISPATCH — explorer_p4_m2_fix_1

## Mission
You are `explorer_p4_m2_fix_1`, investigating fixes for Milestone 2 issues discovered by `challenger_p4_m2_1` and `challenger_p4_m2_2`.
Your working directory is `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_fix_1`.

You must read:
1. Challenger 1 report: `/home/gautham/syncplay-alt/.agents/teamwork/challenger_p4_m2_1/handoff.md`
2. Challenger 2 report: `/home/gautham/syncplay-alt/.agents/teamwork/challenger_p4_m2_2/handoff.md`
3. Target files:
   - `crates/syncplay-demux/src/universal.rs`
   - `crates/syncplay-demux/tests/universal_adversarial_challenge.rs`
   - `crates/syncplay-codecs/src/universal_video.rs`
   - `crates/syncplay-codecs/src/audio/universal.rs`

## Investigation Scope
1. **FLV `seek_to(0)` Fix**:
   - Analyze how `ictx.seek` / `avformat_seek_file` behaves when `target_us == 0` or before first keyframe.
   - Formulate a robust fix: e.g., if `target_us <= start_time_us`, or if `seek(target_us, ..target_us)` returns an error, fallback to `ictx.seek(target_us, ..)` or seek with `AVSEEK_FLAG_BACKWARD | AVSEEK_FLAG_ANY` or range `..=target_us`.
   - Verify that seeking to 0 on `multi_flv.flv` lands on packet 0 without `EPERM`.
2. **Stream Start-Time Offset Normalization**:
   - `ictx.start_time()`: determine how to handle `AV_NOPTS_VALUE` vs valid microseconds.
   - Normalize packet timestamps in `read_packet()`: `pts_ms = raw_pts_ms.saturating_sub(start_time_ms)`.
   - Normalize target PTS in `seek_to()`: `stream_target_us = (target_pts_ms + start_time_ms) * 1000`.
   - Verify timeline navigation on `timescale_90k.ts`.
3. **Probe Buffer Padding**:
   - Ensure buffer passed to `av_probe_input_format` allocates `4096 + 64` bytes and reads up to 4096, guaranteeing trailing zero bytes for `AVPROBE_PADDING_SIZE`.
4. **Codec Hardening**:
   - Check `UniversalVideoDecoder` `swscale` cache reset if frame format or dimensions change.
   - Check DTS codec aliases (`dca`, `dts`) in `UniversalAudioDecoder::from_codec_name`.

## Output Requirements
Write your concrete fix strategy and proposed code diffs to `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_fix_1/handoff.md`.
Send a completion message to the orchestrator.

## 2026-10-25T21:57:24Z
[Message] timestamp=2026-10-25T21:57:24Z sender=ee88cac4-07d4-4dad-bc7e-4c3f606ef03f priority=MESSAGE_PRIORITY_HIGH content=You are `explorer_p4_m2_fix_1`. Read `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_fix_1/DISPATCH.md`.
Also read `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md`.
Investigate the exact fix strategy for:
1. FLV seek(0) EPERM error in UniversalDemuxer::seek_to
2. Stream start-time offset normalization (MPEG-TS PCR offset) in UniversalDemuxer
3. AVProbeData buffer zero-padding
4. Video swscale cache invalidation on format/dimension change and audio DTS alias mapping
Produce a detailed fix strategy and draft code diffs in `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p4_m2_fix_1/handoff.md`.
Send a completion message to the orchestrator.

## 2026-10-25T22:02:23Z
[Message] timestamp=2026-10-25T22:02:23Z sender=ee88cac4-07d4-4dad-bc7e-4c3f606ef03f priority=MESSAGE_PRIORITY_HIGH content=**Context**: Milestone 2 Remediation Analysis
**Content**: Checking in on your progress regarding the fix strategy for FLV seek(0), MPEG-TS start-time normalization, AVProbeData buffer padding, and codec hardening.
**Action**: Please provide a quick status update or submit your handoff.md if complete.
