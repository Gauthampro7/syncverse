# BRIEFING — 2026-10-25T21:49:00Z

## Mission
Empirically challenge UniversalDemuxer across containers (MKV, WebM, MP4, MOV, AVI, MPEG-TS, FLV, OGG), seeking accuracy, timescale arithmetic, and backward compatibility.

## 🔒 My Identity
- Archetype: empirical challenger
- Roles: critic, specialist
- Working directory: /home/gautham/syncplay-alt/.agents/teamwork/challenger_p4_m2_1
- Original parent: ee88cac4-07d4-4dad-bc7e-4c3f606ef03f
- Milestone: Milestone 2 (Phase 4)
- Instance: 1 of 1

## 🔒 Key Constraints
- Review-only — do NOT modify implementation code
- Report failures as findings — do not fix them yourself
- Run verification code empirically — do not trust unverified claims
- All metadata in /home/gautham/syncplay-alt/.agents/teamwork/challenger_p4_m2_1/

## Current Parent
- Conversation ID: ee88cac4-07d4-4dad-bc7e-4c3f606ef03f
- Updated: not yet

## Review Scope
- **Files to review**: `crates/syncplay-demux/src/universal.rs`, `crates/syncplay-demux/tests/universal_demux_tests.rs`
- **Interface contracts**: `Demuxer` trait, `ContainerInfo`, `DemuxPacket`, `TrackInfo`
- **Review criteria**: UniversalDemuxer format coverage (MKV, WebM, MP4, MOV, AVI, MPEG-TS, FLV, OGG), seeking accuracy, landed PTS consistency, boundary packets, timescale conversions (1/90000, 1/1000, 1/12800), and backward compatibility of EbmlDemuxer/IsoBmffDemuxer

## Key Decisions Made
- Executed comprehensive empirical adversarial test suite across 8 container formats with synthetic and encoded media vectors.
- Verified probing, metadata, and timescale conversions across all 8 formats.
- Discovered 3 empirical failure modes in UniversalDemuxer:
  1. `seek_to(0)` fails with `Operation not permitted` (EPERM) on FLV files due to `ictx.seek(target_us, ..target_us)` requiring keyframe <= 0 when stream PTS starts at 25ms.
  2. MPEG-TS stream start offset (`start_time = 1480ms`) is not normalized or compensated, causing seeks < 1480ms to clamp and making the second half of media unaddressable.
  3. `UniversalDemuxer::probe` allocates `vec![0u8; 4096]` without the `AVPROBE_PADDING_SIZE` (32 bytes) zero-padding required by FFmpeg C API.
- Rendered verdict: `REQUEST_CHANGES`.

## Artifact Index
- `.agents/teamwork/challenger_p4_m2_1/BRIEFING.md` — Persistent agent briefing
- `.agents/teamwork/challenger_p4_m2_1/progress.md` — Liveness heartbeat and progress tracker
- `.agents/teamwork/challenger_p4_m2_1/handoff.md` — 5-component handoff report
- `crates/syncplay-demux/tests/universal_adversarial_challenge.rs` — Empirical adversarial challenge test harness

## Attack Surface
- **Hypotheses tested**:
  * Timescale arithmetic under 1/90000, 1/12800, 1/1000, 1/25 (CONFIRMED ACCURATE, exactly ~40ms delta at 25fps).
  * Keyframe seeking accuracy across 8 containers (FAILED on FLV with EPERM, DEGRADED on MPEG-TS).
  * Landed PTS consistency with buffered peek packet (CONFIRMED MATCHING).
  * In-memory `Cursor<Vec<u8>>` streaming demuxing (CONFIRMED WORKING).
  * EOF boundary packets and post-EOF seek recovery (CONFIRMED WORKING, except FLV seek(0)).
  * Adversarial probe noise/truncation rejection (CONFIRMED REJECTED).
  * EbmlDemuxer and IsoBmffDemuxer backward compatibility (CONFIRMED PASSING).
- **Vulnerabilities found**:
  * FLV `seek_to(0)` returns `Io("FFmpeg seek failure: Operation not permitted")`.
  * MPEG-TS PCR offset causes seek clamping and unreachability of media past `duration - start_time`.
  * `AVProbeData` buffer lacks `AVPROBE_PADDING_SIZE` zero-padding.
- **Untested angles**:
  * Live UDP multicast streaming demuxing without file bounds.

## Loaded Skills
- Source: In-tree Pi framework skills (.pi/skills/bitstream-vector-testing)
- Local copy: None
- Core methodology: Empirical boundary stress testing, property verification, adversarial test synthesis
