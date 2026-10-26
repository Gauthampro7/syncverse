# Phase 5 Survey Handoff: Network Wire Protocol, Peer State & Unpause Gate

## 1. Observation

1. **Wire Framing and Packet Header Layout**:
   In `crates/syncplay-sync/src/wire.rs:9-15, 388-437`, the wire transport uses a 12-byte header:
   ```rust
   pub const WIRE_MAGIC: [u8; 4] = *b"SPNT";
   pub const HEADER_LEN: usize = 12;
   pub const FLAG_NONE: u8 = 0x00;
   pub const FLAG_BARRIER_COMMIT: u8 = 0x01;
   ```
   Payload bytes are verified with table-driven IEEE 802.3 CRC32 (`syncplay_common::Crc32::digest`).

2. **Existing Packet Opcode Allocations**:
   In `crates/syncplay-sync/src/wire.rs:25-57`:
   - `0x01`: `NtpProbeRequest`
   - `0x02`: `NtpProbeResponse`
   - `0x03`: `RoomStateBroadcast`
   - `0x04`: `BarrierPrepare`
   - `0x05`: `BarrierReady`
   - `0x06`: `SeekCommand`
   - `0x08`: `ChatMessage`
   Opcode `0x07` is currently unused in the enum.

3. **Opcode Negative Test Assertions**:
   In `crates/syncplay-sync/tests/watch_party_transport.rs:483`:
   ```rust
   for &bad_type in &[0x00u8, 0x07, 0x42, 0xAA, 0xFF] {
   ```
   In `crates/syncplay-sync/tests/chat_transport_adversarial.rs:303`:
   ```rust
   for &bad_op in &[0x00u8, 0x07, 0x09, 0x0A, 0x33, 0x7F, 0xFE, 0xFF] {
   ```
   Both test suites assert that opcodes `0x07`, `0x09`, and `0x0A` produce `SyncError::UnknownPacketType`. Allocating any of these opcodes for new wire packets requires updating these test fixture arrays.

4. **Peer Session State Overloading in `HostServer`**:
   In `crates/syncplay-sync/src/transport.rs:72-84`:
   ```rust
   pub struct ClientSession {
       pub client_id: u32,
       pub addr: SocketAddr,
       pub rtt_ms: u64,
       pub last_seen: Instant,
       pub is_ready: bool,
   }
   ```
   In `transport.rs:331-333`, `prepare_barrier_unpause()` resets `client.is_ready = false`. In `transport.rs:241`, `is_ready` is set to `true` upon receiving `PacketPayload::BarrierReady`. The field `is_ready` is currently overloaded strictly for barrier decode pre-roll readiness, rather than persistent user readiness ("Ready to Watch" checkbox).

5. **Play/Pause Bypass in Graphical Playback Runtime**:
   In `crates/syncplay-core/src/playback_runtime.rs:239-254`:
   ```rust
   PlayerCommand::TogglePause => {
       let now = Instant::now();
       let will_play = !clock.is_playing();
       if will_play {
           clock.set_state(now, PlaybackState::Playing);
           let _ = audio_tx.send(AudioWorkerCommand::Resume);
       } else {
           clock.set_state(now, PlaybackState::Paused);
           let _ = audio_tx.send(AudioWorkerCommand::Pause);
       }
       if let WatchPartyRole::Host { server, .. } = &mut party_role {
           let current_pts = clock.current_pts(now).max(0.0) as u64;
           server.set_media_state(current_pts, speed, !will_play);
           let _ = server.broadcast_room_state();
       }
   }
   ```
   When unpausing (`will_play == true`), the runtime directly switches to `Playing` without checking peer readiness, peer media presence, or invoking `HostServer::prepare_barrier_unpause()`.

6. **UI Room Member Representation**:
   In `crates/syncplay-ui/src/gui/sidebar.rs:18-27`:
   ```rust
   pub struct RoomMember {
       pub username: String,
       pub is_local: bool,
       pub is_host: bool,
       pub rtt_ms: f64,
   }
   ```
   `RoomMember` does not store peer media metadata (filename, duration, file size), file match diagnostic status, or user readiness.

7. **Snapshot Immutability Constraint**:
   In `crates/syncplay-ui/src/snapshot.rs:9-37` and line 174:
   `PlayerUiSnapshot` has a fixed memory size of exactly 72 bytes (`assert_eq!(size, 72)`). Per-peer variable metadata (usernames, filenames) cannot be added to `PlayerUiSnapshot` and must be transmitted via `UiEvent::RoomMembersUpdated(Vec<RoomMember>)`.

---

## 2. Logic Chain

1. **Wire Protocol Extension (From Observations 1, 2, 3)**:
   - To transmit media metadata (filename basename, duration in ms, file size in bytes) and user readiness state (`Ready` vs `Not Ready`), new packet types are required.
   - We analyzed two designs:
     * *Design A (Split)*: `PeerMedia` (0x09) + `PeerReadiness` (0x0A).
     * *Design B (Unified)*: `PeerStatus` (0x09) combining `client_id`, `duration_ms`, `file_size_bytes`, `is_ready`, `has_media`, `username`, and `filename`.
   - *Reasoning*: Design B (Unified `PeerStatus`) is strictly superior over UDP WAN links because it is idempotent and self-contained. If any packet is dropped, subsequent packets restore the complete peer state without partial out-of-order desynchronization (e.g., peer appearing ready before file metadata arrives).
   - *Negative Test Invariant*: When registering opcode `0x09` (or `0x09` and `0x0A`), the negative test fixture arrays in `tests/chat_transport_adversarial.rs:303` and `tests/watch_party_transport.rs:483` must be updated to remove `0x09` from the bad opcode lists.

2. **State Separation in `ClientSession` (From Observation 4)**:
   - Because `prepare_barrier_unpause()` clears `client.is_ready = false` to measure pre-roll completion, reusing `is_ready` for the user's "Ready to Watch" toggle would cause every unpause attempt to clear the user's checkbox.
   - *Reasoning*: `ClientSession` must maintain two separate fields: `barrier_preroll_ready: bool` (transient decode ack) and `is_user_ready: bool` (persistent watch readiness). In addition, it must store `has_media: bool`, `filename: String`, `duration_ms: u64`, and `file_size_bytes: u64`.

3. **Playback Unpause Gate & Barrier Commit (From Observations 4, 5)**:
   - When `PlayerCommand::TogglePause` is received while paused, the runtime must evaluate the unpause gate:
     `all_ready = local_ready && local_has_media && connected_clients.all(|c| c.is_user_ready && c.has_media)`.
   - If `!all_ready` and `!force_play`:
     * Playback remains `Paused`.
     * The unready usernames are collected.
     * An alert is dispatched to the UI (`UiEvent::UnpauseGateBlocked`) and a room announcement is broadcast: `"⏸ Paused: Waiting for [Users] to load media and get ready..."`.
   - If `all_ready` or `force_play`:
     * If in a watch party on Host: the host initiates two-phase barrier unpause (`prepare_barrier_unpause`), broadcasts `BarrierPrepare`, gathers pre-roll acks (`BarrierReady`), and commits (`commit_barrier_unpause`). Playback resumes at the synchronized target instant across all peers with $< 10\text{ms}$ alignment.
     * A chat announcement is broadcast: `"▶ {user} resumed playback"` (or `"▶ {user} forced playback resumption"`).

4. **File Matching Diagnostics (From Observations 6, 7)**:
   - In `sidebar.rs`, comparing local media against each peer's media evaluates:
     * If peer has no media loaded: `[⏳ No file loaded]`.
     * If duration matches within $\pm 2000\text{ms}$ OR filename matches (case-insensitive basename): `[Match ✔]` in green (`Color32::from_rgb(34, 197, 94)`).
     * Otherwise: `[⚠️ Mismatch]` in amber/red (`Color32::from_rgb(245, 158, 11)`).
   - This metadata flows from the playback runtime to the UI thread via `UiEvent::RoomMembersUpdated(Vec<RoomMember>)`, keeping `PlayerUiSnapshot` strictly 72 bytes.

---

## 3. Caveats

1. **UDP Packet Loss & Re-transmission**:
   UDP does not guarantee delivery. While `PeerStatus` is broadcast immediately on state change (file load, ready toggle), it should also be periodically broadcast at 1 Hz alongside NTP probes to guarantee eventual consistency across packet drops.
2. **Path Privacy Protection**:
   Filesystem paths from file open dialogs or drag-and-drop contain private usernames and directory trees. The runtime must strictly extract the basename (`Path::file_name`) before populating wire packets.
3. **Third-Party Client Compatibility**:
   The protocol changes use reserved opcodes `0x09` (and `0x0A`). Existing v0.1.1 clients receiving `0x09` will safely reject it with `SyncError::UnknownPacketType` due to strict enum validation, preventing memory corruption or crashes.

---

## 4. Conclusion

1. **Wire Protocol**: Extend `PacketType` with `PeerStatus = 0x09` (or `PeerMedia = 0x09`, `PeerReadiness = 0x0A`). Payload binary encoding uses little-endian numeric types and length-prefixed UTF-8 strings with table-driven CRC32 framing.
2. **Negative Test Fixtures**: Update `tests/chat_transport_adversarial.rs:303` to remove `0x09` from the bad opcodes list.
3. **Peer State Storage**: Update `ClientSession` on `HostServer` to separate `is_user_ready` from `barrier_preroll_ready`, and store `filename`, `duration_ms`, and `file_size_bytes`.
4. **Unpause Gate**: Hook gate evaluation into `playback_runtime.rs` on `PlayerCommand::TogglePause`. Halts unpause and alerts the room if unready peers exist; bypasses if `force_play` is set.
5. **UI & Diagnostics**: Extend `RoomMember` in `syncplay-ui` to display `[✓ Ready]` / `[⏳ Not Ready]`, peer media duration, and `[Match ✔]` / `[⚠️ Mismatch]` / `[⏳ No file loaded]` badges.

Detailed technical schemas, byte layouts, and algorithms are preserved in:
`/home/gautham/syncplay-alt/.agents/teamwork/explorer_survey_1/analysis.md`

---

## 5. Verification Method

To independently verify the baseline and prospective changes:

1. **Verify Current Workspace Compilation**:
   ```bash
   cargo check --workspace --all-targets
   ```
   *Expected*: Exits 0 with 0 compiler warnings.

2. **Verify Existing Sync Engine Test Suite**:
   ```bash
   cargo test -p syncplay-sync -- --nocapture
   ```
   *Expected*: All 33 unit and adversarial integration tests pass cleanly.

3. **Inspect Negative Test Assertions**:
   ```bash
   grep -n "bad_op" crates/syncplay-sync/tests/chat_transport_adversarial.rs
   grep -n "bad_type" crates/syncplay-sync/tests/watch_party_transport.rs
   ```
   *Expected*: Lines 303 and 483 contain the opcode arrays that must be updated when allocating `0x09`/`0x0A`.

4. **Verify Memory Layout of Snapshot**:
   ```bash
   cargo test -p syncplay-ui test_struct_memory_size
   ```
   *Expected*: Confirms `PlayerUiSnapshot` remains exactly 72 bytes.
