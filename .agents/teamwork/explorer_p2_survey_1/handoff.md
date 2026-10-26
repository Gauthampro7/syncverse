# Handoff Report: R1 Container Demuxing Technical Survey

**Agent**: `explorer_p2_survey_1`  
**Milestone**: Phase 2 Technical Survey (R1: Full Container Demuxing & Packet Processing)  
**Deliverable Report**: `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p2_survey_1/survey_report.md`  
**Date**: 2026-10-25T09:19:00Z  

---

## 1. Observation

1. **`EbmlDemuxer` Implementation (`crates/syncplay-demux/src/ebml.rs`)**:
   - Lines 168–180: `EbmlDemuxer::init` returns hardcoded dummy container metadata:
     ```rust
     let info = ContainerInfo {
         duration_ms: None,
         timescale: self.timestamp_scale_ns,
         tracks: vec![TrackInfo {
             track_id: 1,
             track_type: TrackType::Video,
             codec_id: "V_VP9".into(),
             width: Some(1920),
             height: Some(1080),
             sample_rate: None,
             channels: None,
         }],
     };
     ```
     Neither the `Segment/Info` master element (`TimestampScale`, `Duration`) nor the `Segment/Tracks` master element (`TrackEntry`) is traversed.
   - Lines 260–293: `read_packet` only reads `SimpleBlock` (`0xA3`). It does not handle `BlockGroup` (`0xA0`) / `Block` (`0xA1`), nor does it handle any frame lacing (Xiph, Fixed-size, or EBML lacing defined in flags bits 1–2).
   - Lines 306–309: Seeking is stubbed as a no-op returning `Ok(target_pts_ms)`:
     ```rust
     fn seek_to(&mut self, target_pts_ms: u64) -> Result<u64, DemuxError> {
         // Linear scan or cues-based seek
         Ok(target_pts_ms)
     }
     ```
     `Cues` (`0x1C53BB6B`) / `CuePoint` (`0xBB`) are not parsed, and the underlying reader is not repositioned.

2. **`IsoBmffDemuxer` Implementation (`crates/syncplay-demux/src/isobmff.rs`)**:
   - Lines 307–353: `parse_headers_from_slice` only scans for `moov/mvhd` (timescale and duration); child `trak` boxes are completely skipped.
   - Lines 377–389: `IsoBmffDemuxer::init` returns a hardcoded single track:
     ```rust
     let info = ContainerInfo {
         duration_ms: Some(duration_ms),
         timescale,
         tracks: vec![TrackInfo {
             track_id: 1,
             track_type: TrackType::Video,
             codec_id: "avc1".into(),
             width: Some(1920),
             height: Some(1080),
             sample_rate: None,
             channels: None,
         }],
     };
     ```
   - Lines 165–283: `SampleTableResolver` is fully implemented and tested for `stsc`, `stsz`, `stco`, and `stss` mapping, but is an isolated struct never instantiated or called by `IsoBmffDemuxer`.
   - Lines 396–398: `read_packet` is an empty stub:
     ```rust
     fn read_packet(&mut self) -> Result<Option<DemuxPacket>, DemuxError> {
         Ok(None)
     }
     ```
     No samples are extracted from `mdat`.
   - Lines 400–402: `seek_to` is an empty stub returning `Ok(target_pts_ms)`.

3. **In-Tree Test Vectors & Intent Invariants (`crates/syncplay-demux/src/ebml.rs`, `src/isobmff.rs`, `.pi/harness/intent_matrix.json`)**:
   - `SYNTHETIC_EBML_VECTOR`: Exactly 48 bytes (EBML Header 36B + Segment Header 12B).
   - `SYNTHETIC_MP4_VECTOR`: Exactly 132 bytes (`ftyp` 16B + `moov/mvhd` 116B).
   - `INTENT-07` in `intent_matrix.json` asserts that these exact vectors parse with zero errors. Therefore, they must remain untouched, while new full multi-track test vectors (`SYNTHETIC_FULL_MKV` and `SYNTHETIC_FULL_MP4`) must be added alongside them.

4. **Environment & Tooling Validation**:
   - `cargo check --workspace --all-targets` exits 0 with zero compiler warnings (when `$HOME/.cargo/bin` is in PATH).
   - `cargo test -p syncplay-demux` passes all 7 unit tests cleanly in 0.00s.
   - `./scripts/audit_dependencies.sh` exits 0 with zero banned dependencies.
   - `python3 scripts/intent_harness.py` passes all 8 core intents with 100.0% compliance.

---

## 2. Logic Chain

1. **Premise 1**: The Phase 1 deliverable established verified wire parsers for VINT, EBML Header, BoxHeader, and basic timescale arithmetic, but purposely left `read_packet` and `seek_to` in stubbed states pending Phase 2 implementation (Observation 1 & 2).
2. **Premise 2**: In real media streams, audio streams (Opus/AAC) in Matroska containers rely heavily on frame lacing inside `SimpleBlock`s, and multi-frame clusters interleave video and audio frames. Furthermore, MP4 sample tables are required to calculate `mdat` byte offsets and map samples across multiple interleaved tracks (Observation 1 & 2).
3. **Premise 3**: Random-access seeking (`seek_to(pts)`) requires returning the exact presentation timestamp of the landed keyframe to allow the downstream video decoding pipeline (`002-codecs-presentation`) to initialize pre-roll decoding without glitching (Observation 1 & 2, and OpenSpec `openspec/specs/demuxer/spec.md`).
4. **Premise 4**: Intent 07 and Phase 1 test suites assert byte-exact properties of `SYNTHETIC_EBML_VECTOR` (48 bytes) and `SYNTHETIC_MP4_VECTOR` (132 bytes). Altering these vectors would trigger immediate test regressions in `test_demux_codecs_clock.py` and `adversarial_crates_1_to_4.rs` (Observation 3).
5. **Deduction**: Therefore, Phase 2 R1 implementation must:
   - Extend `EbmlDemuxer` to parse `Segment/Info`, `Segment/Tracks`, `Segment/Cues`, handle `SimpleBlock`/`BlockGroup` lacing with an internal packet queue (`VecDeque<DemuxPacket>`), and execute binary search keyframe seeking.
   - Extend `IsoBmffDemuxer` to parse `moov/trak` hierarchies, instantiate per-track `SampleTableResolver`s, read interleaved samples from `mdat` based on lowest physical file offset, and execute `stss` keyframe seeking.
   - Preserve existing synthetic vectors and introduce `SYNTHETIC_FULL_MKV` and `SYNTHETIC_FULL_MP4` for end-to-end multi-track test coverage.

---

## 3. Caveats

1. **Complex Codec Extradata**: Sub-box parsing inside `stsd` (`avcC`, `hvcC`, `esds`, `dOps`) and Matroska `CodecPrivate` (`0x63A2`) is passed through as raw byte vectors (`codec_private: Option<Vec<u8>>`) to the codec layer; full SPS/PPS parameter set parsing belongs in `syncplay-codecs`.
2. **Variable Frame Rate (VFR) Timestamps**: Some MKV/MP4 files use irregular sample durations. DTS and PTS must be strictly derived from run-length tables (`stts`, `ctts`, and block timecodes) rather than assumed fixed increments ($1/\text{fps}$).
3. **Unindexed / Live Streams**: Streams lacking `Cues` in MKV must gracefully fall back to linear cluster traversal rather than erroring out.

---

## 4. Conclusion

The technical requirements for Phase 2 R1 (`001-demuxer-bitstream`) are completely scoped and mapped:
1. `EbmlDemuxer` and `IsoBmffDemuxer` can be implemented independently by separate workers (`Worker R1-A` and `Worker R1-B`).
2. A third worker (`Worker R1-C`) can generate `SYNTHETIC_FULL_MKV` and `SYNTHETIC_FULL_MP4` test vectors, implement multi-track packet read and seeking integration tests, and ensure 100% Intent Compliance.
3. The exact algorithms, data structures, and edge case defenses are documented in full detail in `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p2_survey_1/survey_report.md`.

---

## 5. Verification Method

Independent verification of the survey findings and baseline state can be performed with the following commands:

```bash
# 1. Verify workspace compilation and zero compiler warnings
export PATH="$HOME/.cargo/bin:$PATH"
cargo check --workspace --all-targets

# 2. Verify all current demuxer unit tests pass
cargo test -p syncplay-demux -- --nocapture

# 3. Verify zero banned external media/codec dependencies
./scripts/audit_dependencies.sh

# 4. Verify Intent Harness achieves 100% compliance
python3 scripts/intent_harness.py

# 5. Inspect current CLI inspect output (confirming hardcoded single track baseline)
cargo run -p syncplay-core -- inspect synthetic-mkv
cargo run -p syncplay-core -- inspect synthetic-mp4
```

### Invalidation Conditions:
This survey report would be invalidated if:
- `Cargo.toml` or `Cargo.lock` introduces any external media crate (`ffmpeg`, `symphonia`, `nom-ebml`, etc.).
- Existing unit tests for `SYNTHETIC_EBML_VECTOR` (48B) or `SYNTHETIC_MP4_VECTOR` (132B) are broken.
- `EbmlDemuxer` or `IsoBmffDemuxer` fails to compile under `#![deny(warnings)]`, `#![deny(missing_docs)]`, or `#![deny(unsafe_code)]`.
