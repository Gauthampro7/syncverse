# Dispatch for worker_p2_presentation

## Task
Implement Milestone 2: Video & Audio Presentation Engine (`002-codecs-presentation`, `003-clock-pipeline`, `006-ui-osd-renderer`) across `crates/syncplay-codecs`, `crates/syncplay-clock`, and `crates/syncplay-ui`.

## Exclusive Write Ownership
You have exclusive write ownership of:
- `crates/syncplay-codecs/`
- `crates/syncplay-clock/`
- `crates/syncplay-ui/`

## Inputs & Specifications
- `/home/gautham/syncplay-alt/.agents/teamwork/ORIGINAL_REQUEST.md`
- `/home/gautham/syncplay-alt/AGENTS.md`
- `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p2_survey_2/survey_report.md`
- `/home/gautham/syncplay-alt/.agents/teamwork/explorer_p2_survey_2/handoff.md`
- `openspec/specs/codecs/spec.md`
- `openspec/specs/av-pipeline/spec.md`
- `openspec/specs/ui-renderer/spec.md`
- `openspec/changes/002-codecs-presentation.md`
- `openspec/changes/003-clock-pipeline.md`
- `openspec/changes/006-ui-osd-renderer.md`

## Detailed Implementation Requirements
1. **`crates/syncplay-codecs` (Color Space Conversion & Frame Geometry)**:
   - Ensure `VideoFrameYUV420p` memory geometry: 64-byte aligned buffers, 16-byte row stride alignment (`(width + 15) & !15`), and $+16$B SIMD tail padding.
   - In `crates/syncplay-codecs/src/color.rs`: Implement high-throughput Q8 fixed-point integer color space conversion ($Y'CbCr \to R'G'B'$) for BT.601 and BT.709 with unrolled $2 \times 2$ pixel chroma reuse (computing chroma terms once per pair of pixels across 4:2:0 blocks) and branchless saturation clamping to `[0, 255]`.
   - Provide a formal trait `ColorSpaceConverter` and concrete implementations `Bt601Converter` and `Bt709Converter`.
   - Add unit tests validating mathematical equivalence between unrolled and scalar conversion, stride handling, and SIMD tail safety.
2. **`crates/syncplay-clock` (Master Clock & SPSC Ring Buffer)**:
   - Ensure `SpscRingBuffer` head and tail are isolated on separate 128-byte cache lines (`#[repr(align(128))]`) to eliminate false sharing.
   - Master monotonic clock (`master_clock.rs`): optimistic `SeqLock` protection, continuous timeline re-anchoring with mathematical $C^0$ continuity on speed shifts ($r \in [0.96, 1.04]$).
   - Presentation decision window: `evaluate_presentation_timing` classifying into `Wait(Duration)`, `PresentNow`, `PresentNowLate`, `DropLateFrame`.
   - Bounded 16-frame reorder buffer.
3. **`crates/syncplay-ui` (UI Snapshot, Debouncer & OSD Bounding)**:
   - Lock-free atomic triple-buffered snapshot: `PlayerUiSnapshot` (exactly 72 bytes) published via `TripleBufferSnapshotExchange`.
   - 10Hz scrubber debouncer: rate-limits scrub drag events to 10 Hz (emitting 5 intermediate seeks for 500ms drag at 120Hz) and immediately emits authoritative seek on drag release (total 6 commands).
   - Memory bounding: `BoundedChatOverlay` capped at 5 bubbles with 4.0s dwell and 1.0s fade. Alpha8 font atlas (4.0 MB at 2048x2048). Total UI RAM $\le 15.0\text{MB}$.
4. **Coding Standards & Invariants**:
   - `#![deny(warnings)]`, `#![deny(missing_docs)]`.
   - Safe Rust outside justified SIMD/SPSC primitives with `// SAFETY:` proofs.
   - Zero banned external dependencies (`./scripts/audit_dependencies.sh`).
   - 100% Intent Compliance (`python3 scripts/intent_harness.py`).
   - All tests pass (`cargo test -p syncplay-codecs -p syncplay-clock -p syncplay-ui`).

## Deliverables
- Implementation in `crates/syncplay-codecs/`, `crates/syncplay-clock/`, `crates/syncplay-ui/`
- Progress updates in `/home/gautham/syncplay-alt/.agents/teamwork/worker_p2_presentation/progress.md`
- Formal handoff report at `/home/gautham/syncplay-alt/.agents/teamwork/worker_p2_presentation/handoff.md`
