# DISPATCH — auditor_p4_m2

## Mission
You are `auditor_p4_m2`, the Forensic Auditor for Milestone 2 (Universal Video & Audio Compatibility Engine).
Your working directory is `/home/gautham/syncplay-alt/.agents/teamwork/auditor_p4_m2`.

You must read:
1. `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md` (specifically `## 2026-10-25T20:12:42Z`)
2. `/home/gautham/syncplay-alt/.agents/teamwork/orchestrator_4/PROJECT.md`
3. Worker report: `/home/gautham/syncplay-alt/.agents/teamwork/worker_p4_m2/handoff.md`
4. Target code files:
   - `Cargo.toml`
   - `crates/syncplay-demux/Cargo.toml`
   - `crates/syncplay-demux/src/universal.rs`
   - `crates/syncplay-demux/src/lib.rs`
   - `crates/syncplay-codecs/Cargo.toml`
   - `crates/syncplay-codecs/src/universal_video.rs`
   - `crates/syncplay-codecs/src/audio/universal.rs`
   - `crates/syncplay-codecs/src/lib.rs`
   - `crates/syncplay-core/src/coordinator.rs`

## Forensic Audit Scope
1. **Architectural Invariants & Prohibition Check**:
   - Zero banned crates: verify absence of `mpv`, `libmpv`, `vlc`, `libvlc`, `sqlite`, `sqlite3`, `rusqlite`, `sqlx`, `diesel`, `rocksdb`, `sled`, `heed`, `tantivy`.
   - Run `./scripts/audit_dependencies.sh` to confirm zero violations across the entire workspace.
2. **Authenticity & Non-Facade Verification**:
   - Verify that `UniversalDemuxer`, `UniversalVideoDecoder`, and `UniversalAudioDecoder` are genuine implementations directly using FFmpeg bindings and native mathematical layouts, not mocks, stubs, or facades.
   - Verify real bitstream demuxing, decoding, color conversion, and audio resampling.
3. **Safety & Invariant Standards**:
   - Inspect every `unsafe` block for a formal `// SAFETY:` comment proving memory safety, bounds validity, alignment, and lack of data races.
   - Verify 64-byte alignment, 16-byte row strides, and SIMD tail padding zeroing in `UniversalVideoDecoder`.
4. **Harness & Verification Scripts**:
   - Run `python3 scripts/intent_harness.py` to ensure 100.0% compliance score.
   - Run `./scripts/verify_phase4.sh` to confirm all 8 verification stages pass.
5. **Verdict**:
   - Render a binary verdict: `CLEAN` or `INTEGRITY VIOLATION`.

## Output Requirements
Document all forensic findings, dependency graphs, source traces, and verdict in `/home/gautham/syncplay-alt/.agents/teamwork/auditor_p4_m2/handoff.md`.
Send a message with your verdict to the orchestrator.


## 2026-10-25T21:45:23Z
You are `auditor_p4_m2`. Read your instructions at `/home/gautham/syncplay-alt/.agents/teamwork/auditor_p4_m2/DISPATCH.md`.
Also read `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md`.
Perform the Forensic Integrity Audit for Milestone 2:
- Architectural Invariants: zero banned crates (mpv, vlc, sqlite, diesel, rocksdb). Run scripts/audit_dependencies.sh.
- Authenticity: verify that UniversalDemuxer, UniversalVideoDecoder, and UniversalAudioDecoder are genuine native implementations, not facades or mock bypasses.
- Safety: inspect every unsafe block for formal // SAFETY: comments, prove memory alignment and bounds checking.
- Harness: run python3 scripts/intent_harness.py and ./scripts/verify_phase4.sh.
Render your binary verdict: CLEAN or INTEGRITY VIOLATION.
Write your forensic report to `/home/gautham/syncplay-alt/.agents/teamwork/auditor_p4_m2/handoff.md` and send a message with your verdict to the orchestrator.
