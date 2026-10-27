//! Dual-temporal chat record wire models, serialization, and CRC32 framing.

use syncverse_common::Crc32;
use syncverse_sync::ChatMessagePayload;
use crate::error::ChatError;

/// 4-byte magic signature at the start of `syncplay_chat.bin`.
pub const HEADER_MAGIC: [u8; 4] = *b"SPCH";

/// Current binary log header version.
pub const HEADER_VERSION: u16 = 1;

/// 2-byte record synchronization word.
pub const SYNC_WORD: [u8; 2] = [0xAA, 0x55];

/// Sentinel value for media timestamp indicating an unanchored lobby message.
pub const UNANCHORED_MEDIA_PTS: u64 = u64::MAX;

/// Dual-temporal chat record stored in `syncplay_chat.bin`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatRecord {
    /// Record type: 1 = Text, 2 = Reaction, 3 = Bookmark, 4 = Command.
    pub record_type: u8,
    /// Record flags (bit 0 = tombstone / deleted).
    pub flags: u8,
    /// Physical UTC epoch timestamp in milliseconds.
    pub wall_timestamp_utc_ms: u64,
    /// Playback media timestamp in milliseconds (`u64::MAX` if unanchored).
    pub media_timestamp_ms: u64,
    /// 16-byte UUID of the message sender.
    pub sender_id: [u8; 16],
    /// Display name of the sender (up to 64 bytes UTF-8).
    pub sender_name: String,
    /// Raw payload bytes (e.g. UTF-8 text or binary command data).
    pub payload: Vec<u8>,
}

impl ChatRecord {
    /// Constructs a new text chat message.
    pub fn new_text(
        wall_ms: u64,
        media_ms: u64,
        sender_id: [u8; 16],
        sender_name: String,
        text: &str,
    ) -> Self {
        Self {
            record_type: 1,
            flags: 0,
            wall_timestamp_utc_ms: wall_ms,
            media_timestamp_ms: media_ms,
            sender_id,
            sender_name,
            payload: text.as_bytes().to_vec(),
        }
    }

    /// Serializes the record into framed binary wire bytes with `0xAA55` and CRC32.
    ///
    /// Total record size = `2 (sync) + 4 (len) + len(body) + 4 (crc) = len(body) + 10 >= 48 bytes`.
    pub fn serialize_record(&self) -> Vec<u8> {
        let name_bytes = self.sender_name.as_bytes();
        let name_len = name_bytes.len().min(64) as u8;

        // Construct record body
        let mut body = Vec::with_capacity(35 + (name_len as usize) + 4 + self.payload.len());
        body.push(self.record_type);
        body.push(self.flags);
        body.extend_from_slice(&self.wall_timestamp_utc_ms.to_le_bytes());
        body.extend_from_slice(&self.media_timestamp_ms.to_le_bytes());
        body.extend_from_slice(&self.sender_id);
        body.push(name_len);
        body.extend_from_slice(&name_bytes[..name_len as usize]);
        body.extend_from_slice(&(self.payload.len() as u32).to_le_bytes());
        body.extend_from_slice(&self.payload);

        let body_crc = Crc32::digest(&body);
        let total_len = (2 + 4 + body.len() + 4) as u32;

        let mut record_bytes = Vec::with_capacity(total_len as usize);
        record_bytes.extend_from_slice(&SYNC_WORD);
        record_bytes.extend_from_slice(&total_len.to_le_bytes());
        record_bytes.extend_from_slice(&body);
        record_bytes.extend_from_slice(&body_crc.to_le_bytes());

        record_bytes
    }

    /// Deserializes a single framed binary record (`0xAA55` sync word, length, body, CRC32).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ChatError> {
        if bytes.len() < 48 {
            return Err(ChatError::MalformedRecord(format!(
                "record too short: {} bytes (minimum 48)",
                bytes.len()
            )));
        }

        if bytes[0..2] != SYNC_WORD {
            return Err(ChatError::MalformedRecord(
                "missing 0xAA55 sync word framing".to_string(),
            ));
        }

        let total_len = u32::from_le_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]) as usize;
        if total_len < 48 || total_len > bytes.len() {
            return Err(ChatError::MalformedRecord(format!(
                "invalid total length {}: buffer size is {}",
                total_len,
                bytes.len()
            )));
        }

        let body_end = total_len - 4;
        let body = &bytes[6..body_end];
        let stored_crc = u32::from_le_bytes([
            bytes[body_end],
            bytes[body_end + 1],
            bytes[body_end + 2],
            bytes[body_end + 3],
        ]);
        let calc_crc = Crc32::digest(body);
        if stored_crc != calc_crc {
            return Err(ChatError::CrcMismatch);
        }

        let record_type = body[0];
        let flags = body[1];
        let wall_ms = u64::from_le_bytes(body[2..10].try_into().map_err(|_| {
            ChatError::MalformedRecord("invalid wall timestamp bytes".into())
        })?);
        let media_ms = u64::from_le_bytes(body[10..18].try_into().map_err(|_| {
            ChatError::MalformedRecord("invalid media timestamp bytes".into())
        })?);
        let sender_id: [u8; 16] = body[18..34].try_into().map_err(|_| {
            ChatError::MalformedRecord("invalid sender UUID bytes".into())
        })?;
        let name_len = body[34] as usize;

        if 35 + name_len + 4 > body.len() {
            return Err(ChatError::MalformedRecord(
                "name length exceeds body boundaries".into(),
            ));
        }

        let sender_name = String::from_utf8_lossy(&body[35..35 + name_len]).to_string();
        let payload_len = u32::from_le_bytes(
            body[35 + name_len..39 + name_len]
                .try_into()
                .map_err(|_| ChatError::MalformedRecord("invalid payload length bytes".into()))?,
        ) as usize;

        if 39 + name_len + payload_len > body.len() {
            return Err(ChatError::MalformedRecord(
                "payload length exceeds body boundaries".into(),
            ));
        }

        let payload = body[39 + name_len..39 + name_len + payload_len].to_vec();

        Ok(Self {
            record_type,
            flags,
            wall_timestamp_utc_ms: wall_ms,
            media_timestamp_ms: media_ms,
            sender_id,
            sender_name,
            payload,
        })
    }

    /// Serializes this record into network bytes for UDP watch-party transmission.
    pub fn to_network_bytes(&self) -> Vec<u8> {
        self.serialize_record()
    }

    /// Deserializes a record received over the network.
    pub fn from_network_bytes(bytes: &[u8]) -> Result<Self, ChatError> {
        Self::from_bytes(bytes)
    }

    /// Constructs a `ChatRecord` from an in-band `ChatMessagePayload`.
    ///
    /// If the packet payload contains framed `0xAA55` bytes, it is deserialized
    /// directly. Otherwise, it is converted into a text chat record using the packet's
    /// timestamps.
    pub fn from_chat_message(msg: &ChatMessagePayload) -> Self {
        if msg.payload.len() >= 48 && msg.payload[0..2] == SYNC_WORD {
            if let Ok(rec) = Self::from_bytes(&msg.payload) {
                return rec;
            }
        }
        if msg.payload.len() >= 52 && msg.payload[4..6] == SYNC_WORD {
            if let Ok(pkt) = ChatNetworkPacket::from_bytes(&msg.payload) {
                return pkt.record;
            }
        }
        let text = String::from_utf8_lossy(&msg.payload);
        Self::new_text(
            msg.wall_timestamp_ms,
            msg.media_timestamp_ms,
            [0u8; 16],
            "Peer".to_string(),
            &text,
        )
    }

    /// Converts this record into a `ChatMessagePayload` with the specified sequence number.
    pub fn to_chat_message(&self, sequence: u32) -> ChatMessagePayload {
        ChatMessagePayload {
            sequence,
            wall_timestamp_ms: self.wall_timestamp_utc_ms,
            media_timestamp_ms: self.media_timestamp_ms,
            payload: self.serialize_record(),
        }
    }
}

/// UDP Network Packet for transmitting chat messages across peers in a watch party.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatNetworkPacket {
    /// Monotonic sequence number for ordering and deduplication.
    pub sequence: u32,
    /// The embedded chat record.
    pub record: ChatRecord,
}

impl ChatNetworkPacket {
    /// Constructs a new `ChatNetworkPacket`.
    pub fn new(sequence: u32, record: ChatRecord) -> Self {
        Self { sequence, record }
    }

    /// Serializes the packet into binary bytes:
    /// `4 bytes (sequence LE)` + `framed ChatRecord bytes`.
    pub fn to_bytes(&self) -> Vec<u8> {
        let record_bytes = self.record.to_network_bytes();
        let mut buf = Vec::with_capacity(4 + record_bytes.len());
        buf.extend_from_slice(&self.sequence.to_le_bytes());
        buf.extend_from_slice(&record_bytes);
        buf
    }

    /// Deserializes a `ChatNetworkPacket` from binary bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ChatError> {
        if bytes.len() < 4 + 48 {
            return Err(ChatError::MalformedRecord(format!(
                "chat packet too short: {} bytes (minimum 52)",
                bytes.len()
            )));
        }
        let sequence = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let record = ChatRecord::from_network_bytes(&bytes[4..])?;
        Ok(Self { sequence, record })
    }

    /// Converts this packet to a wire `ChatMessagePayload`.
    pub fn to_chat_message(&self) -> ChatMessagePayload {
        ChatMessagePayload {
            sequence: self.sequence,
            wall_timestamp_ms: self.record.wall_timestamp_utc_ms,
            media_timestamp_ms: self.record.media_timestamp_ms,
            payload: self.record.serialize_record(),
        }
    }

    /// Constructs a `ChatNetworkPacket` from a wire `ChatMessagePayload`.
    pub fn from_chat_message(msg: &ChatMessagePayload) -> Self {
        Self {
            sequence: msg.sequence,
            record: ChatRecord::from_chat_message(msg),
        }
    }
}

impl From<&ChatRecord> for ChatMessagePayload {
    fn from(rec: &ChatRecord) -> Self {
        rec.to_chat_message(0)
    }
}

impl From<&ChatNetworkPacket> for ChatMessagePayload {
    fn from(pkt: &ChatNetworkPacket) -> Self {
        pkt.to_chat_message()
    }
}

impl From<ChatNetworkPacket> for ChatMessagePayload {
    fn from(pkt: ChatNetworkPacket) -> Self {
        pkt.to_chat_message()
    }
}

impl From<&ChatMessagePayload> for ChatRecord {
    fn from(msg: &ChatMessagePayload) -> Self {
        ChatRecord::from_chat_message(msg)
    }
}

impl From<&ChatMessagePayload> for ChatNetworkPacket {
    fn from(msg: &ChatMessagePayload) -> Self {
        ChatNetworkPacket::from_chat_message(msg)
    }
}

/// Creates a standard 16-byte fixed file header for `syncplay_chat.bin`.
pub fn make_file_header(flags: u16) -> [u8; 16] {
    let mut header = [0u8; 16];
    header[0..4].copy_from_slice(&HEADER_MAGIC);
    header[4..6].copy_from_slice(&HEADER_VERSION.to_le_bytes());
    header[6..8].copy_from_slice(&flags.to_le_bytes());
    header[8..12].copy_from_slice(&0u32.to_le_bytes()); // reserved = 0
    let crc = Crc32::digest(&header[0..12]);
    header[12..16].copy_from_slice(&crc.to_le_bytes());
    header
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_record_and_chat_message_conversion() {
        let rec = ChatRecord::new_text(
            1_700_000_000_000,
            12_345,
            [42; 16],
            "Alice".to_string(),
            "Syncplay in-band chat test!",
        );

        // Convert to ChatMessagePayload
        let wire_msg: ChatMessagePayload = (&rec).into();
        assert_eq!(wire_msg.wall_timestamp_ms, 1_700_000_000_000);
        assert_eq!(wire_msg.media_timestamp_ms, 12_345);
        assert!(wire_msg.payload.starts_with(&SYNC_WORD));

        // Convert back from ChatMessagePayload
        let rec_recovered = ChatRecord::from_chat_message(&wire_msg);
        assert_eq!(rec, rec_recovered);

        // Raw text conversion fallback
        let raw_wire = ChatMessagePayload {
            sequence: 5,
            wall_timestamp_ms: 1_700_000_001_000,
            media_timestamp_ms: 99_999,
            payload: b"Raw text without framing".to_vec(),
        };
        let fallback_rec = ChatRecord::from_chat_message(&raw_wire);
        assert_eq!(fallback_rec.wall_timestamp_utc_ms, 1_700_000_001_000);
        assert_eq!(fallback_rec.media_timestamp_ms, 99_999);
        assert_eq!(fallback_rec.sender_name, "Peer");
        assert_eq!(
            String::from_utf8_lossy(&fallback_rec.payload),
            "Raw text without framing"
        );
    }

    #[test]
    fn test_chat_network_packet_conversion() {
        let rec = ChatRecord::new_text(
            1_700_000_000_000,
            33_333,
            [7; 16],
            "Bob".to_string(),
            "Watch party packet test",
        );
        let pkt = ChatNetworkPacket::new(101, rec);

        let wire_msg: ChatMessagePayload = pkt.to_chat_message();
        assert_eq!(wire_msg.sequence, 101);

        let pkt_recovered = ChatNetworkPacket::from_chat_message(&wire_msg);
        assert_eq!(pkt.sequence, pkt_recovered.sequence);
        assert_eq!(pkt.record, pkt_recovered.record);
    }
}
