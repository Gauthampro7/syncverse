# Project: Syncplay-Alt Phase 3 Implementation

## Architecture
Transform the Syncplay-Alt engine into a VLC-grade standalone interactive media player and synchronized watch-party desktop application for Fedora Linux in pure, idiomatic Rust with zero external multimedia, codec, or database frameworks, governed by the OpenSpec suite and Pi Framework Intent Verification Harness.

## Feature Inventory
| # | Feature | Description | Milestone | Source |
|---|---------|-------------|-----------|--------|
| 1 | H.264 / AVC Bitstream Parser & Reconstruction | Annex B NAL parsing, SPS/PPS decoding, slice headers, YUV420p reconstruction | M1 | R3 |
| 2 | Audio Packet Demuxing & Unpacking | PCM, Vorbis/Opus packet unpacking yielding float samples in `AudioFramePCM` | M1 | R3 |
| 3 | RGBA & Terminal TrueColor Converter | High-perf Q8 fixed-point integer color space converter to RGBA / ANSI 24-bit TrueColor | M1 | R3 |
| 4 | Linux Audio Sink & Underrun Protection | Zero-external-dependency audio sink (ALSA PCM / raw pipe), zero-allocation underrun protection | M2 | R2 |
| 5 | Audio Resampling & Clock Pacing | Linear sample rate conversion (44.1 <-> 48 kHz), clock pacing synced with `MasterClock` | M2 | R2 |
| 6 | Interactive Host Server (`party host`) | UDP host broadcasting room state, answering NTP probes, orchestrating barrier unpause | M3 | R4 |
| 7 | Client Node & Micro-Speed Steering (`party connect`) | Connects to host, computes $\hat{\theta}$ and RTT, drives PI PLL steering, barrier unpause | M3 | R4 |
| 8 | Live In-Band Smart Chat & Slash Commands | UDP framed chat messages, local binary log persistence, FTS, floating on-screen HUD | M3 | R4 |
| 9 | Fedora Desktop Application Integration | `~/.local/share/applications/syncplay-alt.desktop` with Exec, MIME types, modern icon | M4 | R1 |
| 10 | Interactive 60fps Terminal/OSD Presentation Loop | `play <FILE>` with full keyboard navigation (Space, seek, speed, vol, HUDs, slash, exit) | M4 | R1 |
| 11 | Sleek Terminal Aesthetic OSD | 24-bit TrueColor / box-drawing / half-block video rendering, scrubber, HUDs, comments | M4 | R1 |
| 12 | Intent Verification & Zero-Drift Defense | 100.0% Intent compliance score, zero warnings, zero banned crates, automated tests | M5 & M6 | R5 |

## Milestones
| # | Name | Scope | Dependencies | Status |
|---|------|-------|-------------|--------|
| Survey | Deep Technical Survey | 3 Explorers mapping R1–R5 codebase, interfaces, vectors | none | COMPLETED |
| M1 | Bitstream Decoders (Video & Audio) | `crates/syncplay-demux`, `crates/syncplay-codecs` | Survey | DONE (GATE PASSED) |
| M2 | Audio Output & Resampling Engine | `crates/syncplay-clock`, `crates/syncplay-codecs`, `crates/syncplay-core` | M1 | DONE (GATE PASSED) |
| M3 | Watch-Party Networking & In-Band Chat | `crates/syncplay-sync`, `crates/syncplay-chat`, `crates/syncplay-core` | M2 | IN_PROGRESS |
| M4 | Fedora Desktop & Interactive Terminal/OSD | `crates/syncplay-ui`, `crates/syncplay-core`, `~/.local/share/applications/` | M1, M2, M3 | PLANNED |
| M5 | System Integration & Intent Verification | Workspace-wide integration, intent harness, challenge suites | M1, M2, M3, M4 | PLANNED |
| M6 | Final Victory Audit & Attestation | Independent forensic verification, 100% tests, clean audit | M5 | PLANNED |

## Interface Contracts
- `VideoDecoder`: `fn decode_nal(&mut self, nal_data: &[u8]) -> Result<Option<VideoFrameYUV420p>, CodecError>;`
- `AudioDecoder`: `fn decode_packet(&mut self, packet_data: &[u8]) -> Result<AudioFramePCM, CodecError>;`
- `AudioSink`: `fn write_samples(&mut self, samples: &[f32]) -> Result<usize, AudioError>; fn pause(&mut self); fn resume(&mut self);`
- `AudioResampler`: `fn resample(&mut self, input: &[f32], output: &mut Vec<f32>, in_rate: u32, out_rate: u32) -> Result<(), AudioError>;`
- `WatchPartyHost`: `fn bind(addr: &str, media_file: &Path) -> Result<Self, SyncError>; fn poll_events(&mut self) -> Result<Vec<PartyEvent>, SyncError>;`
- `WatchPartyClient`: `fn connect(host_addr: &str, media_file: &Path) -> Result<Self, SyncError>; fn update(&mut self, wall_now: WallTime, media_pts: MediaTime) -> Result<PllAction, SyncError>;`
- `InteractivePlayer`: `fn run_presentation_loop(&mut self) -> Result<(), CoreError>;`

## Code Layout
```
crates/
├── syncplay-common/     # Shared types, timestamps, CRC32, bitstream readers, errors
├── syncplay-demux/      # EBML/MKV & ISOBMFF/MP4 demuxers + H.264 / audio packet extractors
├── syncplay-codecs/     # Video/Audio frames, H.264 bitstream parser, Q8 color converter, audio resampler
├── syncplay-clock/      # Master monotonic clock, 128B SPSC ring buffer, audio pacing
├── syncplay-sync/       # RFC 5905 NTP, discrete PI PLL, barrier unpause, interactive watch-party networking
├── syncplay-chat/       # Binary log syncplay_chat.bin, crash recovery, FTS, slash commands, network framing
├── syncplay-ui/         # Atomic triple-buffered snapshots, scrubber debouncer, ANSI TrueColor OSD renderer
└── syncplay-core/       # Coordinator, interactive terminal presentation loop, audio sink, CLI commands
```
