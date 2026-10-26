# Phase 5 Technical Survey: Network Wire Protocol, Peer State & Unpause Gate

## 1. Executive Summary & Problem Scope

This survey investigates the network wire protocol and peer state tracking in Syncplay-Alt across `crates/syncplay-sync`, `crates/syncplay-common`, `crates/syncplay-core`, and `crates/syncplay-ui`. It establishes the concrete engineering design for:
1. **Peer Media Metadata Exchange**: Transmitting filename basename, duration in milliseconds, and file size in bytes over UDP wire packets.
2. **Peer Readiness State**: Exchanging persistent user readiness (`Ready` vs `Not Ready`), updating peer badges in the sidebar.
3. **File Match Diagnostics**: Comparing local media against peer media to display `[Match ✔]`, `[⚠️ Mismatch]`, or `[⏳ No file loaded]`.
4. **Strict Playback Unpause Gate**: Halting unpause attempts when peers are unready, alerting the room, supporting Host "Force Play" override, and hooking into the two-phase barrier commit protocol.
5. **Automated Room Announcements**: Broadcasting synchronized chat notices for media loading, ready toggling, pausing, unpausing, and seeking.

---

## 2. Current Architecture & Codebase Baseline

### 2.1 Packet Header & Wire Framing (`crates/syncplay-sync/src/wire.rs`)
The SyncPlay Network Transport (SPNT) utilizes a 12-byte fixed header preceding a variable-length payload:

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|       'S'     |      'P'      |      'N'      |      'T'      | Magic (4B)
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|  packet_type  |     flags     |          payload_len          | Type (1B), Flags (1B), Len (2B LE)
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                        crc32 (IEEE 802.3)                     | CRC32 of payload (4B LE)
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                                                               |
+                        payload_bytes                          + Payload (payload_len bytes)
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

- **Magic**: `b"SPNT"` (`WIRE_MAGIC`, 4 bytes).
- **Header Length**: `HEADER_LEN = 12` bytes.
- **Flags**: `0x00` (`FLAG_NONE`), `0x01` (`FLAG_BARRIER_COMMIT`).
- **Integrity**: Table-driven IEEE 802.3 CRC32 computed over `payload_bytes` with polynomial `0xEDB88320`.
- **Deserialization**: Strictly validates `HEADER_LEN`, magic bytes, valid `PacketType`, payload length bounds, CRC32 digest match, and individual payload length requirements.

### 2.2 Existing Packet Types (`wire.rs:22-57`)
| Opcode | Name | Size (Bytes) | Fields |
|---|---|---|---|
| `0x01` | `NtpProbeRequest` | 12 | `probe_id: u32`, `t1_client_send_ns: u64` |
| `0x02` | `NtpProbeResponse` | 28 | `probe_id: u32`, `t1: u64`, `t2: u64`, `t3: u64` |
| `0x03` | `RoomStateBroadcast` | 29 | `sequence: u64`, `ref_media_pts_ms: u64`, `target_speed_q16: u32`, `ref_server_wall_ms: u64`, `paused: u8` |
| `0x04` | `BarrierPrepare` | 16 | `session_id: u32`, `target_unpause_server_ms: u64`, `preroll_budget_ms: u32` |
| `0x05` | `BarrierReady` | 12 | `session_id: u32`, `client_id: u32`, `preroll_completed_ms: u32` |
| `0x06` | `SeekCommand` | 12 | `event_id: u32`, `target_pts_ms: u64` |
| `0x07` | *Unused* | - | Reserved / previously negative test fixture |
| `0x08` | `ChatMessage` | 20 + N | `sequence: u32`, `wall_timestamp_ms: u64`, `media_timestamp_ms: u64`, `payload: Vec<u8>` |

### 2.3 Existing Peer State Models
1. **`HostServer` (`crates/syncplay-sync/src/transport.rs:70-101`)**:
   Tracks clients in `clients: HashMap<SocketAddr, ClientSession>`:
   ```rust
   pub struct ClientSession {
       pub client_id: u32,
       pub addr: SocketAddr,
       pub rtt_ms: u64,
       pub last_seen: Instant,
       pub is_ready: bool, // Note: currently used strictly for barrier pre-roll
   }
   ```
2. **`playback_runtime.rs` (`crates/syncplay-core/src/playback_runtime.rs:158-178`)**:
   `WatchPartyRole::Host` maintains `client_names: HashMap<SocketAddr, String>`, learned lazily when a peer sends a chat message. If no chat message has been sent, the username defaults to `"Peer-<port>"`.
3. **`syncplay-ui` (`crates/syncplay-ui/src/gui/sidebar.rs:16-28`)**:
   `RoomMember` currently contains:
   ```rust
   pub struct RoomMember {
       pub username: String,
       pub is_local: bool,
       pub is_host: bool,
       pub rtt_ms: f64,
   }
   ```
   It has no fields for readiness state or loaded media status.

---

## 3. Wire Protocol Extensions

### 3.1 Opcode Allocation & Negative Test Collision Defense
In `crates/syncplay-sync/tests/watch_party_transport.rs` (line 483) and `crates/syncplay-sync/tests/chat_transport_adversarial.rs` (line 303), negative test fixtures test invalid opcodes:
- `watch_party_transport.rs:483`: `for &bad_type in &[0x00u8, 0x07, 0x42, 0xAA, 0xFF]`
- `chat_transport_adversarial.rs:303`: `for &bad_op in &[0x00u8, 0x07, 0x09, 0x0A, 0x33, 0x7F, 0xFE, 0xFF]`

**Critical Implementation Invariant**:
When allocating opcodes for Phase 5:
- If allocating `0x09` and `0x0A`:
  The negative test fixtures in `tests/chat_transport_adversarial.rs:303` MUST be updated to remove `0x09, 0x0A` from the bad opcodes array (e.g. replaced with `0x0B, 0x0C` or `0x7E`).
- If allocating `0x07`:
  Both `watch_party_transport.rs:483` and `chat_transport_adversarial.rs:303` test fixtures must remove `0x07`.

### 3.2 Protocol Design: Separate Packets vs Unified State Packet

#### Option A: Dedicated Separate Packets
- `0x09`: `PeerMedia` (`client_id: u32`, `duration_ms: u64`, `file_size_bytes: u64`, `username: String`, `filename: String`)
- `0x0A`: `PeerReadiness` (`client_id: u32`, `is_ready: bool`, `username: String`)

*Pros*: Minimal packet payload size (17 bytes for readiness).
*Cons*: Differential state over UDP. If `PeerMedia` is dropped, peers observe updated readiness with stale/unloaded media, causing state divergence.

#### Option B: Unified Idempotent State Packet (`PeerStatus`)
- `0x09`: `PeerStatus` (`client_id: u32`, `duration_ms: u64`, `file_size_bytes: u64`, `is_ready: bool`, `has_media: bool`, `username: String`, `filename: String`)

*Pros*:
1. **Idempotence & Self-Containment**: Every datagram provides the full, authoritative snapshot of the peer's state.
2. **Robustness over Lossy WAN UDP**: Lost packets do not create out-of-order partial state (e.g., ready without media).
3. **Heartbeat Healing**: Host and clients can broadcast their `PeerStatus` periodically (e.g. 1 Hz) to immediately heal state after packet loss or upon dynamic joins.
4. **Compactness**: Total payload is only 30–70 bytes, trivial for UDP.

#### Recommended Architecture: Unified `PeerStatus` (Opcode `0x09`)
We recommend `PacketType::PeerStatus = 0x09` as the primary state synchronization datagram, with optional support for `PacketType::PeerReadiness = 0x0A` if standalone readiness nudges are desired.

### 3.3 Binary Payload Byte Layout for `PeerStatus` (`0x09`)
```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                           client_id                           | Client ID (4B LE)
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                                                               |
+                          duration_ms                          + Media Duration (8B LE)
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                                                               |
+                        file_size_bytes                        + File Size (8B LE)
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|   is_ready    |   has_media   |         username_len          | Ready (1B), Media (1B), UserLen (2B LE)
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                        username_bytes                         |
|                   (length = username_len)                     |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|         filename_len          |        filename_bytes         | FileLen (2B LE), File bytes
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+                               |
|                   (length = filename_len)                     |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

#### Field Specifications:
- `client_id` (`u32`, 4 bytes LE): Identifier of reporting client (0 for host).
- `duration_ms` (`u64`, 8 bytes LE): Container duration in milliseconds (0 if none).
- `file_size_bytes` (`u64`, 8 bytes LE): File size on disk in bytes (0 if none).
- `is_ready` (`u8`, 1 byte): `0x01` if Ready to Watch, `0x00` if Not Ready.
- `has_media` (`u8`, 1 byte): `0x01` if media loaded, `0x00` if unloaded.
- `username_len` (`u16`, 2 bytes LE): Byte length of username (max 256).
- `username_bytes`: Valid UTF-8 bytes of username.
- `filename_len` (`u16`, 2 bytes LE): Byte length of filename basename (max 1024).
- `filename_bytes`: Valid UTF-8 bytes of filename basename.

**Security / Privacy Invariant**: The filename MUST be strictly the basename (e.g. `sintel_trailer.mp4`), stripped of all local parent directory paths (`/home/user/...` or `C:\Users\...`).

---

## 4. Peer State Storage & Synchronization

### 4.1 Separation of User Readiness vs Barrier Pre-roll Ready
In `crates/syncplay-sync/src/transport.rs:70-84`, `ClientSession.is_ready` is currently used during barrier unpause:
```rust
// Current code in transport.rs
pub struct ClientSession {
    pub client_id: u32,
    pub addr: SocketAddr,
    pub rtt_ms: u64,
    pub last_seen: Instant,
    pub is_ready: bool, // WARNING: overloaded during barrier session
}
```

**Architectural Fix**:
We must separate these two orthogonal states:
1. `is_user_ready: bool`: Persistent social readiness toggle ("Ready to Watch" checkbox). Persists across play/pause/seek events until toggled by the user or invalidated by unloading media.
2. `barrier_preroll_ready: bool`: Ephemeral hardware decode pre-roll ack for an active barrier unpause session (`session_id`). Reset on `prepare_barrier_unpause()`.

### 4.2 Extended `ClientSession`
```rust
pub struct ClientSession {
    pub client_id: u32,
    pub addr: SocketAddr,
    pub rtt_ms: u64,
    pub last_seen: Instant,
    pub barrier_preroll_ready: bool,
    pub is_user_ready: bool,
    pub has_media: bool,
    pub username: String,
    pub filename: String,
    pub duration_ms: u64,
    pub file_size_bytes: u64,
}
```

### 4.3 Star-Topology Peer Roster Relay
In Syncplay-Alt watch parties, peers connect to the Host in a star topology.
1. When a client sends a `PeerStatus` packet to the Host:
   - Host updates its local `clients` entry for that `SocketAddr`.
   - Host relays the `PeerStatus` packet to all other registered clients.
2. Host periodically broadcasts its own `PeerStatus` and relays current member statuses.
3. Every client maintains a peer status map: `HashMap<u32, PeerStatus>` or receives `UiEvent::RoomMembersUpdated` from the runtime.

### 4.4 File Match Diagnostic Logic
In `crates/syncplay-ui/src/gui/sidebar.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileMatchStatus {
    NoFileLoaded,
    Match,
    Mismatch,
}

pub fn evaluate_file_match(
    local_has_media: bool,
    local_duration_ms: u64,
    local_filename: &str,
    peer_has_media: bool,
    peer_duration_ms: u64,
    peer_filename: &str,
) -> FileMatchStatus {
    if !peer_has_media || peer_filename.is_empty() {
        return FileMatchStatus::NoFileLoaded;
    }
    if !local_has_media {
        return FileMatchStatus::NoFileLoaded;
    }

    let duration_diff_ms = (local_duration_ms as i64 - peer_duration_ms as i64).abs();
    let duration_matches = duration_diff_ms <= 2000; // ±2 seconds
    let filename_matches = local_filename.eq_ignore_ascii_case(peer_filename);

    if duration_matches || filename_matches {
        FileMatchStatus::Match
    } else {
        FileMatchStatus::Mismatch
    }
}
```

---

## 5. Strict Playback Unpause Gate & Barrier Integration

### 5.1 Call Chain & Gating Sequence

```
User Action: Spacebar / Play Button / /play command
     │
     ▼
UI Thread: sends `PlayerCommand::TogglePause`
     │
     ▼
Background Playback Worker (`playback_runtime.rs`):
     │
     ├─► If currently PLAYING:
     │     - Set clock state: PAUSED
     │     - Send AudioWorkerCommand::Pause
     │     - Broadcast RoomState(paused = true)
     │     - Broadcast Chat: "⏸ {user} paused playback"
     │
     └─► If currently PAUSED (Attempting to UNPAUSE):
           │
           ▼
     [ UNPAUSE GATE EVALUATION ]
           │
           ├─► Check: Is local user ready and media loaded?
           ├─► Check: For all connected room peers:
           │     - Does peer have media loaded?
           │     - Is peer `is_user_ready == true`?
           │
           ├──► [ GATE BLOCKED: Unready Peers Exist ]
           │     │
           │     ├─► Check: Is Host "Force Play" override enabled?
           │     │     │
           │     │     ├─► YES (Force Play): Proceed to Unpause (Bypass Gate)
           │     │     │     Broadcast Chat: "▶ {user} forced playback resumption"
           │     │     │
           │     │     └─► NO:
           │     │           - Keep clock PAUSED!
           │     │           - Collect unready usernames: e.g. ["Bob", "Carol"]
           │     │           - Format alert: "⏸ Paused: Waiting for Bob, Carol to load media and get ready..."
           │     │           - Send `UiEvent::UnpauseGateBlocked(msg)` for top banner / OSD
           │     │           - Broadcast Chat announcement to room
           │     │           - RETURN WITHOUT UNPAUSING!
           │     │
           └──► [ GATE CLEARED: All Peers Ready ]
                 │
                 ├─► If in Watch Party (Host):
                 │     - Initiate two-phase barrier unpause:
                 │         `target_server_ms = host.prepare_barrier_unpause(200)`
                 │     - Broadcast `BarrierPrepare(target_server_ms)`
                 │     - When all peers ack or deadline reached:
                 │         `host.commit_barrier_unpause(target_server_ms)`
                 │     - At scheduled instant: set clock Playing, resume audio
                 │     - Broadcast Chat: "▶ {user} resumed playback"
                 │
                 └─► If in Watch Party (Client):
                 │     - Client notifies host of unpause request or slash command
                 │
                 └─► If Solo / Local:
                       - Set clock Playing, resume audio
```

---

## 6. Automated Room Announcements (R4)

All room announcements are generated as synchronized in-band chat records (`ChatRecord`) using the existing `PacketType::ChatMessage` (`0x08`) transport:

| Event | Message Template |
|---|---|
| Media Loaded | `🎬 {user} loaded '{filename}' ({HH:MM:SS})` |
| Ready Toggled (Ready) | `✓ {user} is ready to watch` |
| Ready Toggled (Unready) | `⏳ {user} is not ready` |
| Playback Paused | `⏸ {user} paused playback` |
| Playback Resumed | `▶ {user} resumed playback` |
| Seeking | `⏩ {user} seeked to {timecode}` |
| Gate Blocked | `⏸ Paused: Waiting for {users} to load media and get ready...` |
| Force Play | `▶ {user} forced playback resumption` |

---

## 7. Extended Types Across Crates

### `crates/syncplay-ui/src/gui/sidebar.rs`
```rust
pub struct RoomMember {
    pub username: String,
    pub is_local: bool,
    pub is_host: bool,
    pub rtt_ms: f64,
    pub is_ready: bool,
    pub has_media: bool,
    pub media_filename: Option<String>,
    pub media_duration_ms: Option<u64>,
    pub media_file_size: Option<u64>,
}
```

### `crates/syncplay-ui/src/command.rs`
```rust
pub enum PlayerCommand {
    // Existing commands...
    /// Toggle or set persistent user readiness state.
    SetReadiness(bool),
    /// Host override to bypass unready peers and force playback unpause.
    ForcePlay,
}
```

### `crates/syncplay-ui/src/event.rs`
```rust
pub enum UiEvent {
    NewChatRecord(ChatRecord),
    RoomMembersUpdated(Vec<RoomMember>),
    /// Alert triggered when unpause gate halts playback.
    UnpauseGateBlocked {
        unready_users: Vec<String>,
        message: String,
    },
}
```
