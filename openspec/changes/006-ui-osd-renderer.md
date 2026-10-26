# Change Proposal: 006-ui-osd-renderer

**Change ID**: `006-ui-osd-renderer`  
**Title**: Native Player UI, OSD Overlay Architecture & Lock-Free Atomic State Snapshots  
**Status**: Ready for Implementation  
**Target Milestone**: Phase 1 Foundational Primitives  

---

## 1. Context & Motivation

To achieve "VLC-grade responsiveness" (< 16ms input latency) and avoid UI hitching or dropped audio frames, the interface cannot share mutex locks with decode or sync threads. This change implements a decoupled UI architecture powered by lock-free triple-buffered atomic state snapshots and composited OSD overlay elements.

---

## 2. Dependency Graph

```
[003-clock-pipeline] ────────┐
                             │
[004-network-sync-protocol] ─┼─► [006-ui-osd-renderer] (syncplay-ui, syncplay-core)
                             │
[005-smart-chat-storage] ────┘
```

- **Dependencies**: `003-clock-pipeline`, `004-network-sync-protocol`, `005-smart-chat-storage`.
- **Blocks**: None (Terminal phase of Phase 1 workspace).

---

## 3. Affected Crates & Components

- **`crates/syncplay-ui`**:
  - `src/snapshot/model.rs`: `PlayerUiSnapshot` struct and SPSC triple-buffer atomic publisher constrained to a single Telemetry Aggregator.
  - `src/command/queue.rs`: Lock-free SPSC `UICommandQueue` for user actions.
  - `src/osd/hud.rs`: Transport state glyphs, monospace timecode HUD.
  - `src/osd/sync_badge.rs`: Real-time sync health badge with green/amber/red status dot.
  - `src/osd/chat_overlay.rs`: Floating semi-transparent chat bubbles with exponential decay.
  - `src/osd/scrub_bar.rs`: Interactive timeline slider with 10 Hz debounced seek dispatching (5 intermediate + 1 release = 6 commands).
- **`crates/syncplay-core`**:
  - `src/coordinator.rs`: Runtime loop integrating demuxer, decoder, clock, sync, chat, and UI with central Telemetry Aggregator.

---

## 4. Deliverables & Implementation Plan

1. **Triple-Buffered Atomic Snapshot**:
   - `PlayerUiSnapshot` struct encapsulating time, PTS, duration, RTT, drift, PLL speed, and peer counts.
   - Reader acquires latest snapshot via single atomic pointer load without blocking over an SPSC topology published exclusively by the central Telemetry Aggregator.
2. **Lock-Free Command Queue**:
   - UI pushes user commands (`TogglePause`, `SeekRelative`, `SendChatMessage`) into SPSC ring buffer.
   - Core engine drains commands on each tick.
3. **OSD Overlay Pipeline**:
   - Transport glyphs with 600ms exponential fade ($\alpha(t) = 0.8 \cdot e^{-5t}$).
   - Sync health badge color coding:
     - Green ($|e| < 15\text{ ms}$): In-sync deadband.
     - Amber ($15\text{ ms} \le |e| \le 1500\text{ ms}$): Active PLL micro-speed slew.
     - Red ($|e| > 1500\text{ ms}$): Out of sync / hard seek.
   - Floating chat bubbles stacking up to 5 items with 4-second dwell and 1-second fade.
   - Scrub-bar with $10\text{ Hz}$ seek debouncing (5 intermediate + 1 release) while maintaining 60–144 Hz visual feedback.
4. **Performance & Memory Compliance**:
   - Pre-allocated vertex buffers and single-channel Alpha8 (`R8_UNORM`) font glyph atlas texture (4 MB at 2048x2048) ensuring total UI memory footprint remains strictly under 15 MB.
   - CPU overhead under $1.2\%$; zero heap allocation in render loop.

---

## 5. Verification Commands

Execute the following commands from the repository root:

```bash
# 1. Type and borrow check affected crates
cargo check -p syncplay-ui -p syncplay-core

# 2. Run unit tests verifying snapshot atomicity and scrub-bar debouncing
cargo test -p syncplay-ui -- --nocapture

# 3. Verify zero banned GUI toolkits or external media dependencies
cargo tree -p syncplay-ui --edges no-dev
```

Expected output: Snapshot tests verify concurrent reading and writing without data races or lock contention; debouncer unit tests verify rate-limiting to $10\text{ Hz}$.
