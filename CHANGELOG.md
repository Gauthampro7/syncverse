# Changelog

All notable changes to Syncverse are documented in this file.

## [0.5.0] - 2026-11-18

### Added
- **VA-API Hardware Decoding (Linux, Opt-in)**: Added `--hwdec=off|auto|vaapi`, `SYNCVERSE_HWDEC` / `SYNCVERSE_HWDEC_DEVICE` environment variables, and Preferences selection. Uses hardware decode with CPU readback and falls back cleanly to software decoding with an OSD notice if initialisation or profile support fails.
- **NAT Traversal for Watch Parties**:
  - Opt-in RFC 8489 STUN discovery ("Detect public address" button in Host/Connect dialogs) using `stun.l.google.com:19302` and `stun.cloudflare.com:3478` to report public `ip:port` and NAT type (`Open`, `EndpointIndependent`, `Symmetric`).
  - UDP hole punching over the shared room socket via `NatMux` and `DatagramInterceptor`.
  - NAT-PMP support alongside hardened UPnP IGD port mapping.
- **Room Resilience**: Added `Hello` / `HelloAck` handshake, periodic heartbeats, host-loss detection, automatic reconnect, clean `Bye` on exit, and a live connection state badge in the sidebar (`Connecting`, `Connected`, `Reconnecting`, `Failed`, `Closed`).
- **Persistence & Resume**:
  - Settings stored atomically at `$XDG_CONFIG_HOME/syncverse/settings.conf` (volume, mute, username, last host, sidebar state, hwdec mode, resume policy, per-file memory toggle, subtitle style, equalizer).
  - Resume-last-position (`Ask` / `Always` / `Never` with an 8-second resume banner, suppressed in watch-party rooms) and per-file track, delay, and aspect-ratio memory at `$XDG_STATE_HOME/syncverse/resume.tsv`.
- **Queue & Frame-Stepping Improvements**:
  - Automatic playlist queue persistence at `$XDG_STATE_HOME/syncverse/queue.m3u` with drag-reorder in the queue dialog, multi-file drop queuing, and **Add to queue** / **Play next** actions.
  - True frame-stepping (`.` / `,`) using presented-PTS history and exact-target paused seeks (solo and host only; disabled for watch-party clients).
  - A-B repeat loop gated to solo and host only.
- **Footprint & Documentation**: Added `scripts/bench_footprint.sh`, `scripts/footprint_baseline_v0.4.0.txt`, `tests/e2e_phase5/README.md`, and updated `README.md` and `PROJECT.md`.

### Fixed
- Corrected `README.md` audio backend description (`cpal` primary with subprocess/null fallback), verification script reference (`verify_phase1.sh`), and full keyboard/mouse shortcut table.
- Undecodable UDP datagrams in watch-party transport are now skipped and counted instead of aborting `poll_and_process`.
