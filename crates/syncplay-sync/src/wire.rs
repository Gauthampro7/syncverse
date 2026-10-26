//! Binary wire protocol serialization, deserialization, and CRC32 framing.
//!
//! Provides the core network packet definitions for the SyncPlay Network Transport (SPNT).

use syncplay_common::Crc32;

use crate::error::SyncError;

/// 4-byte magic signature for SyncPlay Network Transport.
pub const WIRE_MAGIC: [u8; 4] = *b"SPNT";

/// Length of the fixed packet header in bytes:
/// 4 (magic) + 1 (packet_type) + 1 (flags) + 2 (payload_len) + 4 (crc32) = 12 bytes.
pub const HEADER_LEN: usize = 12;

/// Default flag value (no special options).
pub const FLAG_NONE: u8 = 0x00;

/// Flag indicating a barrier commit notification.
pub const FLAG_BARRIER_COMMIT: u8 = 0x01;

/// Binary wire packet types for network synchronization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PacketType {
    /// 0x01: NTP Probe Request (`probe_id: u32`, `t1_client_send_ns: u64`)
    NtpProbeRequest = 0x01,
    /// 0x02: NTP Probe Response (`probe_id: u32`, `t1_client_send_ns: u64`, `t2_server_recv_ns: u64`, `t3_server_send_ns: u64`)
    NtpProbeResponse = 0x02,
    /// 0x03: Room State Broadcast (`sequence: u64`, `ref_media_pts_ms: u64`, `target_speed_q16: u32`, `ref_server_wall_ms: u64`, `paused: u8`)
    RoomStateBroadcast = 0x03,
    /// 0x04: Barrier Prepare / Schedule (`session_id: u32`, `target_unpause_server_ms: u64`, `preroll_budget_ms: u32`)
    BarrierPrepare = 0x04,
    /// 0x05: Barrier Ready / Commit Ack (`session_id: u32`, `client_id: u32`, `preroll_completed_ms: u32`)
    BarrierReady = 0x05,
    /// 0x06: Seek / Command Event (`event_id: u32`, `target_pts_ms: u64`)
    SeekCommand = 0x06,
    /// 0x08: In-band Chat Message (`sequence: u32`, `wall_timestamp_ms: u64`, `media_timestamp_ms: u64`, `payload: Vec<u8>`)
    ChatMessage = 0x08,
}

impl TryFrom<u8> for PacketType {
    type Error = SyncError;

    fn try_from(val: u8) -> Result<Self, Self::Error> {
        match val {
            0x01 => Ok(Self::NtpProbeRequest),
            0x02 => Ok(Self::NtpProbeResponse),
            0x03 => Ok(Self::RoomStateBroadcast),
            0x04 => Ok(Self::BarrierPrepare),
            0x05 => Ok(Self::BarrierReady),
            0x06 => Ok(Self::SeekCommand),
            0x08 => Ok(Self::ChatMessage),
            other => Err(SyncError::UnknownPacketType(other)),
        }
    }
}

/// Payload for `0x01` NTP Probe Request (12 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NtpProbeRequestPayload {
    /// Monotonic probe sequence ID.
    pub probe_id: u32,
    /// Client transmit timestamp in nanoseconds ($T_1$).
    pub t1_client_send_ns: u64,
}

/// Payload for `0x02` NTP Probe Response (28 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NtpProbeResponsePayload {
    /// Monotonic probe sequence ID matching the request.
    pub probe_id: u32,
    /// Client transmit timestamp in nanoseconds ($T_1$).
    pub t1_client_send_ns: u64,
    /// Server receive timestamp in nanoseconds ($T_2$).
    pub t2_server_recv_ns: u64,
    /// Server reply transmit timestamp in nanoseconds ($T_3$).
    pub t3_server_send_ns: u64,
}

/// Payload for `0x03` Room State Broadcast (29 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoomStatePayload {
    /// Monotonic update sequence number for out-of-order rejection.
    pub sequence: u64,
    /// Reference media presentation timestamp in milliseconds.
    pub ref_media_pts_ms: u64,
    /// Commanded playback speed in Q16 fixed-point format ($1.0 = 65536$).
    pub target_speed_q16: u32,
    /// Reference server wall-clock timestamp in milliseconds.
    pub ref_server_wall_ms: u64,
    /// Whether playback is currently paused (0 = playing, 1 = paused).
    pub paused: u8,
}

/// Payload for `0x04` Barrier Prepare / Schedule (16 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarrierPreparePayload {
    /// Unique unpause session ID.
    pub session_id: u32,
    /// Authoritative future server timestamp when presentation resumes.
    pub target_unpause_server_ms: u64,
    /// Allowed pre-roll decode budget in milliseconds (typically 200ms).
    pub preroll_budget_ms: u32,
}

/// Payload for `0x05` Barrier Ready / Commit Ack (12 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarrierReadyPayload {
    /// Unique unpause session ID matching the prepare broadcast.
    pub session_id: u32,
    /// Client identifier reporting readiness.
    pub client_id: u32,
    /// Actual duration taken to complete pre-roll decode in milliseconds.
    pub preroll_completed_ms: u32,
}

/// Payload for `0x06` Seek / Command Event (12 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeekCommandPayload {
    /// Monotonic event ID.
    pub event_id: u32,
    /// Commanded target media presentation timestamp in milliseconds.
    pub target_pts_ms: u64,
}

/// Payload for `0x08` Chat Message (minimum 20 bytes: 4 seq + 8 wall_ms + 8 media_ms + N payload bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessagePayload {
    /// Monotonic chat sequence number.
    pub sequence: u32,
    /// Physical wall-clock timestamp in milliseconds.
    pub wall_timestamp_ms: u64,
    /// Media presentation timestamp in milliseconds (or `u64::MAX` if unanchored).
    pub media_timestamp_ms: u64,
    /// Raw chat message payload bytes (e.g. framed ChatRecord or UTF-8 text).
    pub payload: Vec<u8>,
}

impl ChatMessagePayload {
    /// Constructs a new `ChatMessagePayload`.
    pub fn new(
        sequence: u32,
        wall_timestamp_ms: u64,
        media_timestamp_ms: u64,
        payload: Vec<u8>,
    ) -> Self {
        Self {
            sequence,
            wall_timestamp_ms,
            media_timestamp_ms,
            payload,
        }
    }
}

/// Typed representation of wire packet payloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PacketPayload {
    /// NTP Probe Request
    NtpProbeRequest(NtpProbeRequestPayload),
    /// NTP Probe Response
    NtpProbeResponse(NtpProbeResponsePayload),
    /// Room State Broadcast
    RoomStateBroadcast(RoomStatePayload),
    /// Barrier Prepare / Schedule
    BarrierPrepare(BarrierPreparePayload),
    /// Barrier Ready / Commit Ack
    BarrierReady(BarrierReadyPayload),
    /// Seek Command
    SeekCommand(SeekCommandPayload),
    /// In-band Chat Message
    ChatMessage(ChatMessagePayload),
}

impl PacketPayload {
    /// Serializes the typed payload into raw binary bytes.
    pub fn serialize(&self) -> Vec<u8> {
        match self {
            Self::NtpProbeRequest(p) => {
                let mut buf = Vec::with_capacity(12);
                buf.extend_from_slice(&p.probe_id.to_le_bytes());
                buf.extend_from_slice(&p.t1_client_send_ns.to_le_bytes());
                buf
            }
            Self::NtpProbeResponse(p) => {
                let mut buf = Vec::with_capacity(28);
                buf.extend_from_slice(&p.probe_id.to_le_bytes());
                buf.extend_from_slice(&p.t1_client_send_ns.to_le_bytes());
                buf.extend_from_slice(&p.t2_server_recv_ns.to_le_bytes());
                buf.extend_from_slice(&p.t3_server_send_ns.to_le_bytes());
                buf
            }
            Self::RoomStateBroadcast(p) => {
                let mut buf = Vec::with_capacity(29);
                buf.extend_from_slice(&p.sequence.to_le_bytes());
                buf.extend_from_slice(&p.ref_media_pts_ms.to_le_bytes());
                buf.extend_from_slice(&p.target_speed_q16.to_le_bytes());
                buf.extend_from_slice(&p.ref_server_wall_ms.to_le_bytes());
                buf.push(p.paused);
                buf
            }
            Self::BarrierPrepare(p) => {
                let mut buf = Vec::with_capacity(16);
                buf.extend_from_slice(&p.session_id.to_le_bytes());
                buf.extend_from_slice(&p.target_unpause_server_ms.to_le_bytes());
                buf.extend_from_slice(&p.preroll_budget_ms.to_le_bytes());
                buf
            }
            Self::BarrierReady(p) => {
                let mut buf = Vec::with_capacity(12);
                buf.extend_from_slice(&p.session_id.to_le_bytes());
                buf.extend_from_slice(&p.client_id.to_le_bytes());
                buf.extend_from_slice(&p.preroll_completed_ms.to_le_bytes());
                buf
            }
            Self::SeekCommand(p) => {
                let mut buf = Vec::with_capacity(12);
                buf.extend_from_slice(&p.event_id.to_le_bytes());
                buf.extend_from_slice(&p.target_pts_ms.to_le_bytes());
                buf
            }
            Self::ChatMessage(p) => {
                let mut buf = Vec::with_capacity(20 + p.payload.len());
                buf.extend_from_slice(&p.sequence.to_le_bytes());
                buf.extend_from_slice(&p.wall_timestamp_ms.to_le_bytes());
                buf.extend_from_slice(&p.media_timestamp_ms.to_le_bytes());
                buf.extend_from_slice(&p.payload);
                buf
            }
        }
    }

    /// Returns the associated `PacketType` for this payload.
    pub fn packet_type(&self) -> PacketType {
        match self {
            Self::NtpProbeRequest(_) => PacketType::NtpProbeRequest,
            Self::NtpProbeResponse(_) => PacketType::NtpProbeResponse,
            Self::RoomStateBroadcast(_) => PacketType::RoomStateBroadcast,
            Self::BarrierPrepare(_) => PacketType::BarrierPrepare,
            Self::BarrierReady(_) => PacketType::BarrierReady,
            Self::SeekCommand(_) => PacketType::SeekCommand,
            Self::ChatMessage(_) => PacketType::ChatMessage,
        }
    }
}

/// Structured binary wire packet for SyncPlay Network Transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WirePacket {
    /// Packet type.
    pub packet_type: PacketType,
    /// Protocol flags.
    pub flags: u8,
    /// Typed payload.
    pub payload: PacketPayload,
}

impl WirePacket {
    /// Creates a new `WirePacket` with the specified packet type, flags, and payload.
    pub fn new(packet_type: PacketType, flags: u8, payload: PacketPayload) -> Self {
        Self {
            packet_type,
            flags,
            payload,
        }
    }

    /// Constructs an NTP Probe Request packet (`0x01`).
    pub fn ntp_request(probe_id: u32, t1_client_send_ns: u64) -> Self {
        Self {
            packet_type: PacketType::NtpProbeRequest,
            flags: FLAG_NONE,
            payload: PacketPayload::NtpProbeRequest(NtpProbeRequestPayload {
                probe_id,
                t1_client_send_ns,
            }),
        }
    }

    /// Constructs an NTP Probe Response packet (`0x02`).
    pub fn ntp_response(
        probe_id: u32,
        t1_client_send_ns: u64,
        t2_server_recv_ns: u64,
        t3_server_send_ns: u64,
    ) -> Self {
        Self {
            packet_type: PacketType::NtpProbeResponse,
            flags: FLAG_NONE,
            payload: PacketPayload::NtpProbeResponse(NtpProbeResponsePayload {
                probe_id,
                t1_client_send_ns,
                t2_server_recv_ns,
                t3_server_send_ns,
            }),
        }
    }

    /// Constructs a Room State Broadcast packet (`0x03`).
    pub fn room_state(
        sequence: u64,
        ref_media_pts_ms: u64,
        target_speed_q16: u32,
        ref_server_wall_ms: u64,
        paused: bool,
    ) -> Self {
        Self {
            packet_type: PacketType::RoomStateBroadcast,
            flags: FLAG_NONE,
            payload: PacketPayload::RoomStateBroadcast(RoomStatePayload {
                sequence,
                ref_media_pts_ms,
                target_speed_q16,
                ref_server_wall_ms,
                paused: if paused { 1 } else { 0 },
            }),
        }
    }

    /// Constructs a Barrier Prepare / Schedule packet (`0x04`).
    pub fn barrier_prepare(
        session_id: u32,
        target_unpause_server_ms: u64,
        preroll_budget_ms: u32,
        is_commit: bool,
    ) -> Self {
        Self {
            packet_type: PacketType::BarrierPrepare,
            flags: if is_commit {
                FLAG_BARRIER_COMMIT
            } else {
                FLAG_NONE
            },
            payload: PacketPayload::BarrierPrepare(BarrierPreparePayload {
                session_id,
                target_unpause_server_ms,
                preroll_budget_ms,
            }),
        }
    }

    /// Constructs a Barrier Ready / Commit Ack packet (`0x05`).
    pub fn barrier_ready(session_id: u32, client_id: u32, preroll_completed_ms: u32) -> Self {
        Self {
            packet_type: PacketType::BarrierReady,
            flags: FLAG_NONE,
            payload: PacketPayload::BarrierReady(BarrierReadyPayload {
                session_id,
                client_id,
                preroll_completed_ms,
            }),
        }
    }

    /// Constructs a Seek / Command Event packet (`0x06`).
    pub fn seek_command(event_id: u32, target_pts_ms: u64) -> Self {
        Self {
            packet_type: PacketType::SeekCommand,
            flags: FLAG_NONE,
            payload: PacketPayload::SeekCommand(SeekCommandPayload {
                event_id,
                target_pts_ms,
            }),
        }
    }

    /// Constructs a Chat Message packet (`0x08`).
    pub fn chat_message(
        sequence: u32,
        wall_timestamp_ms: u64,
        media_timestamp_ms: u64,
        payload: Vec<u8>,
    ) -> Self {
        Self {
            packet_type: PacketType::ChatMessage,
            flags: FLAG_NONE,
            payload: PacketPayload::ChatMessage(ChatMessagePayload {
                sequence,
                wall_timestamp_ms,
                media_timestamp_ms,
                payload,
            }),
        }
    }

    /// Serializes this packet into a binary byte vector with header framing and CRC32.
    pub fn to_bytes(&self) -> Vec<u8> {
        let payload_bytes = self.payload.serialize();
        let payload_len = payload_bytes.len() as u16;
        let crc = Crc32::digest(&payload_bytes);

        let mut buf = Vec::with_capacity(HEADER_LEN + payload_bytes.len());
        buf.extend_from_slice(&WIRE_MAGIC);
        buf.push(self.packet_type as u8);
        buf.push(self.flags);
        buf.extend_from_slice(&payload_len.to_le_bytes());
        buf.extend_from_slice(&crc.to_le_bytes());
        buf.extend_from_slice(&payload_bytes);

        buf
    }

    /// Deserializes a binary byte buffer into a validated `WirePacket`.
    pub fn from_bytes(buf: &[u8]) -> Result<Self, SyncError> {
        if buf.len() < HEADER_LEN {
            return Err(SyncError::PacketTooShort(buf.len()));
        }

        let magic = [buf[0], buf[1], buf[2], buf[3]];
        if magic != WIRE_MAGIC {
            return Err(SyncError::InvalidMagic(magic));
        }

        let packet_type_raw = buf[4];
        let flags = buf[5];
        let packet_type = PacketType::try_from(packet_type_raw)?;

        let payload_len = u16::from_le_bytes([buf[6], buf[7]]) as usize;
        let expected_crc = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);

        if buf.len() < HEADER_LEN + payload_len {
            return Err(SyncError::PayloadTruncated {
                expected: payload_len,
                actual: buf.len() - HEADER_LEN,
            });
        }

        let payload_slice = &buf[HEADER_LEN..HEADER_LEN + payload_len];
        let computed_crc = Crc32::digest(payload_slice);
        if computed_crc != expected_crc {
            return Err(SyncError::CrcMismatch {
                expected: expected_crc,
                actual: computed_crc,
            });
        }

        let payload = match packet_type {
            PacketType::NtpProbeRequest => {
                if payload_slice.len() != 12 {
                    return Err(SyncError::MalformedPayload(format!(
                        "NtpProbeRequest expected 12 bytes, got {}",
                        payload_slice.len()
                    )));
                }
                let probe_id = u32::from_le_bytes(payload_slice[0..4].try_into().unwrap());
                let t1_client_send_ns =
                    u64::from_le_bytes(payload_slice[4..12].try_into().unwrap());
                PacketPayload::NtpProbeRequest(NtpProbeRequestPayload {
                    probe_id,
                    t1_client_send_ns,
                })
            }
            PacketType::NtpProbeResponse => {
                if payload_slice.len() != 28 {
                    return Err(SyncError::MalformedPayload(format!(
                        "NtpProbeResponse expected 28 bytes, got {}",
                        payload_slice.len()
                    )));
                }
                let probe_id = u32::from_le_bytes(payload_slice[0..4].try_into().unwrap());
                let t1_client_send_ns =
                    u64::from_le_bytes(payload_slice[4..12].try_into().unwrap());
                let t2_server_recv_ns =
                    u64::from_le_bytes(payload_slice[12..20].try_into().unwrap());
                let t3_server_send_ns =
                    u64::from_le_bytes(payload_slice[20..28].try_into().unwrap());
                PacketPayload::NtpProbeResponse(NtpProbeResponsePayload {
                    probe_id,
                    t1_client_send_ns,
                    t2_server_recv_ns,
                    t3_server_send_ns,
                })
            }
            PacketType::RoomStateBroadcast => {
                if payload_slice.len() != 29 {
                    return Err(SyncError::MalformedPayload(format!(
                        "RoomStateBroadcast expected 29 bytes, got {}",
                        payload_slice.len()
                    )));
                }
                let sequence = u64::from_le_bytes(payload_slice[0..8].try_into().unwrap());
                let ref_media_pts_ms =
                    u64::from_le_bytes(payload_slice[8..16].try_into().unwrap());
                let target_speed_q16 =
                    u32::from_le_bytes(payload_slice[16..20].try_into().unwrap());
                let ref_server_wall_ms =
                    u64::from_le_bytes(payload_slice[20..28].try_into().unwrap());
                let paused = payload_slice[28];
                PacketPayload::RoomStateBroadcast(RoomStatePayload {
                    sequence,
                    ref_media_pts_ms,
                    target_speed_q16,
                    ref_server_wall_ms,
                    paused,
                })
            }
            PacketType::BarrierPrepare => {
                if payload_slice.len() != 16 {
                    return Err(SyncError::MalformedPayload(format!(
                        "BarrierPrepare expected 16 bytes, got {}",
                        payload_slice.len()
                    )));
                }
                let session_id = u32::from_le_bytes(payload_slice[0..4].try_into().unwrap());
                let target_unpause_server_ms =
                    u64::from_le_bytes(payload_slice[4..12].try_into().unwrap());
                let preroll_budget_ms =
                    u32::from_le_bytes(payload_slice[12..16].try_into().unwrap());
                PacketPayload::BarrierPrepare(BarrierPreparePayload {
                    session_id,
                    target_unpause_server_ms,
                    preroll_budget_ms,
                })
            }
            PacketType::BarrierReady => {
                if payload_slice.len() != 12 {
                    return Err(SyncError::MalformedPayload(format!(
                        "BarrierReady expected 12 bytes, got {}",
                        payload_slice.len()
                    )));
                }
                let session_id = u32::from_le_bytes(payload_slice[0..4].try_into().unwrap());
                let client_id = u32::from_le_bytes(payload_slice[4..8].try_into().unwrap());
                let preroll_completed_ms =
                    u32::from_le_bytes(payload_slice[8..12].try_into().unwrap());
                PacketPayload::BarrierReady(BarrierReadyPayload {
                    session_id,
                    client_id,
                    preroll_completed_ms,
                })
            }
            PacketType::SeekCommand => {
                if payload_slice.len() != 12 {
                    return Err(SyncError::MalformedPayload(format!(
                        "SeekCommand expected 12 bytes, got {}",
                        payload_slice.len()
                    )));
                }
                let event_id = u32::from_le_bytes(payload_slice[0..4].try_into().unwrap());
                let target_pts_ms =
                    u64::from_le_bytes(payload_slice[4..12].try_into().unwrap());
                PacketPayload::SeekCommand(SeekCommandPayload {
                    event_id,
                    target_pts_ms,
                })
            }
            PacketType::ChatMessage => {
                if payload_slice.len() < 20 {
                    return Err(SyncError::MalformedPayload(format!(
                        "ChatMessage expected at least 20 bytes, got {}",
                        payload_slice.len()
                    )));
                }
                let sequence = u32::from_le_bytes(payload_slice[0..4].try_into().unwrap());
                let wall_timestamp_ms =
                    u64::from_le_bytes(payload_slice[4..12].try_into().unwrap());
                let media_timestamp_ms =
                    u64::from_le_bytes(payload_slice[12..20].try_into().unwrap());
                let payload = payload_slice[20..].to_vec();
                PacketPayload::ChatMessage(ChatMessagePayload {
                    sequence,
                    wall_timestamp_ms,
                    media_timestamp_ms,
                    payload,
                })
            }
        };

        Ok(Self {
            packet_type,
            flags,
            payload,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ntp_probe_roundtrip() {
        let req = WirePacket::ntp_request(42, 1_000_000_000);
        let bytes = req.to_bytes();
        assert_eq!(&bytes[0..4], b"SPNT");
        assert_eq!(bytes[4], 0x01); // type
        assert_eq!(bytes.len(), 12 + 12);

        let parsed = WirePacket::from_bytes(&bytes).expect("deserialization should succeed");
        assert_eq!(req, parsed);

        let resp = WirePacket::ntp_response(42, 1_000_000_000, 1_150_000_000, 1_155_000_000);
        let resp_bytes = resp.to_bytes();
        assert_eq!(resp_bytes.len(), 12 + 28);
        let parsed_resp = WirePacket::from_bytes(&resp_bytes).expect("deserialization should succeed");
        assert_eq!(resp, parsed_resp);
    }

    #[test]
    fn test_room_state_roundtrip() {
        let pkt = WirePacket::room_state(1001, 45_000, 65536, 12_345, false);
        let bytes = pkt.to_bytes();
        assert_eq!(bytes.len(), 12 + 29);
        let parsed = WirePacket::from_bytes(&bytes).unwrap();
        assert_eq!(pkt, parsed);
    }

    #[test]
    fn test_barrier_roundtrip() {
        let prep = WirePacket::barrier_prepare(77, 900_000, 200, false);
        let prep_bytes = prep.to_bytes();
        assert_eq!(prep_bytes.len(), 12 + 16);
        let parsed_prep = WirePacket::from_bytes(&prep_bytes).unwrap();
        assert_eq!(prep, parsed_prep);

        let ready = WirePacket::barrier_ready(77, 10, 120);
        let ready_bytes = ready.to_bytes();
        assert_eq!(ready_bytes.len(), 12 + 12);
        let parsed_ready = WirePacket::from_bytes(&ready_bytes).unwrap();
        assert_eq!(ready, parsed_ready);
    }

    #[test]
    fn test_seek_command_roundtrip() {
        let seek = WirePacket::seek_command(5, 60_000);
        let bytes = seek.to_bytes();
        assert_eq!(bytes.len(), 12 + 12);
        let parsed = WirePacket::from_bytes(&bytes).unwrap();
        assert_eq!(seek, parsed);
    }

    #[test]
    fn test_invalid_magic() {
        let mut bytes = WirePacket::ntp_request(1, 100).to_bytes();
        bytes[0] = b'X';
        assert!(matches!(
            WirePacket::from_bytes(&bytes),
            Err(SyncError::InvalidMagic(_))
        ));
    }

    #[test]
    fn test_crc_mismatch() {
        let mut bytes = WirePacket::ntp_request(1, 100).to_bytes();
        // Corrupt payload byte
        let last_idx = bytes.len() - 1;
        bytes[last_idx] ^= 0xFF;
        assert!(matches!(
            WirePacket::from_bytes(&bytes),
            Err(SyncError::CrcMismatch { .. })
        ));
    }

    #[test]
    fn test_truncated_packet() {
        let bytes = WirePacket::ntp_request(1, 100).to_bytes();
        assert!(matches!(
            WirePacket::from_bytes(&bytes[..bytes.len() - 2]),
            Err(SyncError::PayloadTruncated { .. })
        ));
        assert!(matches!(
            WirePacket::from_bytes(&bytes[..6]),
            Err(SyncError::PacketTooShort(6))
        ));
    }

    #[test]
    fn test_unknown_packet_type() {
        let mut bytes = WirePacket::ntp_request(1, 100).to_bytes();
        bytes[4] = 0xFF; // invalid type
        assert!(matches!(
            WirePacket::from_bytes(&bytes),
            Err(SyncError::UnknownPacketType(0xFF))
        ));
    }

    #[test]
    fn test_chat_message_roundtrip() {
        let payload = b"Hello watch party!".to_vec();
        let pkt = WirePacket::chat_message(7, 1_700_000_000_123, 45_678, payload.clone());
        let bytes = pkt.to_bytes();

        assert_eq!(&bytes[0..4], b"SPNT");
        assert_eq!(bytes[4], 0x08); // type = ChatMessage

        let parsed = WirePacket::from_bytes(&bytes).expect("parse chat message");
        assert_eq!(pkt, parsed);

        if let PacketPayload::ChatMessage(chat) = parsed.payload {
            assert_eq!(chat.sequence, 7);
            assert_eq!(chat.wall_timestamp_ms, 1_700_000_000_123);
            assert_eq!(chat.media_timestamp_ms, 45_678);
            assert_eq!(chat.payload, payload);
        } else {
            panic!("expected ChatMessage payload");
        }
    }
}
