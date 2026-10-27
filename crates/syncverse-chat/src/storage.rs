//! Append-only binary log storage engine and crash-resilient recovery scanner.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use syncverse_common::Crc32;

use crate::error::ChatError;
use crate::record::{make_file_header, ChatRecord, HEADER_MAGIC, SYNC_WORD};

/// Statistics collected during log file crash recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RecoveryStats {
    /// Number of valid records recovered from the log.
    pub valid_records: usize,
    /// Number of mid-stream corrupted or false-sync candidates skipped.
    pub corrupted_chunks: usize,
    /// Number of trailing torn writes detected at EOF (0 or 1).
    pub torn_writes: usize,
}

/// Forward-scanning recovery scanner for `syncplay_chat.bin`.
#[derive(Debug)]
pub struct ChatRecoveryScanner<'a> {
    data: &'a [u8],
}

impl<'a> ChatRecoveryScanner<'a> {
    /// Constructs a scanner over raw log file bytes.
    pub fn new(data: &'a [u8]) -> Self {
        Self { data }
    }

    /// Convenience helper to recover valid records directly from a byte slice.
    pub fn recover_from_slice(data: &[u8]) -> Result<(Vec<ChatRecord>, RecoveryStats), ChatError> {
        let scanner = ChatRecoveryScanner { data };
        scanner.recover()
    }

    /// Convenience helper to recover valid records directly from a file path.
    pub fn recover_from_file(path: impl AsRef<Path>) -> Result<(Vec<ChatRecord>, RecoveryStats), ChatError> {
        let mut f = File::open(path).map_err(|e| ChatError::Io(e.to_string()))?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).map_err(|e| ChatError::Io(e.to_string()))?;
        Self::recover_from_slice(&buf)
    }

    /// Recovers all valid records from the byte slice, skipping corrupted chunks and false sync words.
    pub fn recover(&self) -> Result<(Vec<ChatRecord>, RecoveryStats), ChatError> {
        let file_len = self.data.len();
        if file_len < 16 {
            return Ok((Vec::new(), RecoveryStats::default()));
        }

        // Validate 16-byte fixed file header
        let magic = &self.data[0..4];
        if magic != HEADER_MAGIC {
            return Err(ChatError::InvalidHeaderMagic);
        }

        let stored_hdr_crc = u32::from_le_bytes([
            self.data[12],
            self.data[13],
            self.data[14],
            self.data[15],
        ]);
        let calc_hdr_crc = Crc32::digest(&self.data[0..12]);
        if stored_hdr_crc != calc_hdr_crc {
            return Err(ChatError::HeaderCrcMismatch);
        }

        let mut offset = 16usize;
        let mut last_valid_end = 16usize;
        let mut valid_records = Vec::new();
        let mut corrupted_chunks = 0usize;

        while offset < file_len {
            // Find SYNC_WORD [0xAA, 0x55]
            if offset + 2 > file_len {
                break;
            }

            if self.data[offset..offset + 2] != SYNC_WORD {
                // Search forward for next sync word
                let mut found = None;
                for i in offset..file_len.saturating_sub(1) {
                    if self.data[i..i + 2] == SYNC_WORD {
                        found = Some(i);
                        break;
                    }
                }
                match found {
                    Some(next_sync) => {
                        corrupted_chunks += 1;
                        offset = next_sync;
                        continue;
                    }
                    None => break,
                }
            }

            // Check if candidate header (sync + length = 6 bytes) is within file bounds
            if offset + 6 > file_len {
                break;
            }

            let total_len = u32::from_le_bytes([
                self.data[offset + 2],
                self.data[offset + 3],
                self.data[offset + 4],
                self.data[offset + 5],
            ]) as usize;

            // False-sync and boundary check:
            // total_len must be >= 48 and fit within the remaining file bytes.
            if total_len < 48 || offset + total_len > file_len {
                corrupted_chunks += 1;
                offset += 1; // Scan forward byte-by-byte
                continue;
            }

            // Verify body CRC32
            let body_end = offset + total_len - 4;
            let body = &self.data[offset + 6..body_end];
            let stored_crc = u32::from_le_bytes([
                self.data[body_end],
                self.data[body_end + 1],
                self.data[body_end + 2],
                self.data[body_end + 3],
            ]);
            let calc_crc = Crc32::digest(body);

            if calc_crc != stored_crc {
                // CRC mismatch: corrupted candidate
                corrupted_chunks += 1;
                offset += 1; // Advance byte-by-byte
                continue;
            }

            // Valid record! Parse fields
            let rec_type = body[0];
            let flags = body[1];
            let wall_ms = u64::from_le_bytes(body[2..10].try_into().unwrap());
            let media_ms = u64::from_le_bytes(body[10..18].try_into().unwrap());
            let sender_id: [u8; 16] = body[18..34].try_into().unwrap();
            let name_len = body[34] as usize;

            if 35 + name_len + 4 > body.len() {
                corrupted_chunks += 1;
                offset += 1;
                continue;
            }

            let sender_name = String::from_utf8_lossy(&body[35..35 + name_len]).to_string();
            let payload_len = u32::from_le_bytes(
                body[35 + name_len..39 + name_len].try_into().unwrap(),
            ) as usize;

            if 39 + name_len + payload_len > body.len() {
                corrupted_chunks += 1;
                offset += 1;
                continue;
            }

            let payload = body[39 + name_len..39 + name_len + payload_len].to_vec();

            valid_records.push(ChatRecord {
                record_type: rec_type,
                flags,
                wall_timestamp_utc_ms: wall_ms,
                media_timestamp_ms: media_ms,
                sender_id,
                sender_name,
                payload,
            });

            offset += total_len;
            last_valid_end = offset;
        }

        let torn_writes = if last_valid_end < file_len { 1 } else { 0 };

        let stats = RecoveryStats {
            valid_records: valid_records.len(),
            corrupted_chunks,
            torn_writes,
        };

        Ok((valid_records, stats))
    }
}

/// Append-only local binary chat log storage engine.
#[derive(Debug)]
pub struct ChatLogStorage {
    path: PathBuf,
    file: File,
}

impl ChatLogStorage {
    /// Opens or creates the `syncplay_chat.bin` log file at the given path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ChatError> {
        let path = path.as_ref().to_path_buf();
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| ChatError::Io(e.to_string()))?;

        // Initialize file with 16-byte header if empty
        let len = file.seek(SeekFrom::End(0)).map_err(|e| ChatError::Io(e.to_string()))?;
        if len == 0 {
            let hdr = make_file_header(0);
            file.write_all(&hdr).map_err(|e| ChatError::Io(e.to_string()))?;
            file.flush().map_err(|e| ChatError::Io(e.to_string()))?;
        }

        Ok(Self { path, file })
    }

    /// Appends a new chat record to the log.
    pub fn append(&mut self, record: &ChatRecord) -> Result<u64, ChatError> {
        let bytes = record.serialize_record();
        let pos = self
            .file
            .seek(SeekFrom::End(0))
            .map_err(|e| ChatError::Io(e.to_string()))?;
        self.file
            .write_all(&bytes)
            .map_err(|e| ChatError::Io(e.to_string()))?;
        self.file.flush().map_err(|e| ChatError::Io(e.to_string()))?;
        Ok(pos)
    }

    /// Recovers all valid records from the file.
    pub fn recover(&mut self) -> Result<(Vec<ChatRecord>, RecoveryStats), ChatError> {
        self.file.seek(SeekFrom::Start(0)).map_err(|e| ChatError::Io(e.to_string()))?;
        let mut buffer = Vec::new();
        self.file
            .read_to_end(&mut buffer)
            .map_err(|e| ChatError::Io(e.to_string()))?;
        let scanner = ChatRecoveryScanner::new(&buffer);
        scanner.recover()
    }

    /// Path to the log file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_clean_and_recovery() {
        let sender_id = [1u8; 16];
        let mut full_log = Vec::new();
        full_log.extend_from_slice(&make_file_header(0));

        for i in 0..10 {
            let rec = ChatRecord::new_text(
                1000 + i * 10,
                i * 5000,
                sender_id,
                format!("User{}", i),
                &format!("Message {}", i),
            );
            full_log.extend_from_slice(&rec.serialize_record());
        }

        // Clean log
        let scanner = ChatRecoveryScanner::new(&full_log);
        let (records, stats) = scanner.recover().unwrap();
        assert_eq!(records.len(), 10);
        assert_eq!(stats.torn_writes, 0);

        // Truncated at EOF (15 bytes truncated)
        let truncated = &full_log[..full_log.len() - 15];
        let scanner_trunc = ChatRecoveryScanner::new(truncated);
        let (records_trunc, stats_trunc) = scanner_trunc.recover().unwrap();
        assert_eq!(records_trunc.len(), 9);
        assert_eq!(stats_trunc.torn_writes, 1);

        // False sync resilience
        let bad_bytes = [0xAA, 0x55, 0xFF, 0xFF, 0xFF, 0x00];
        let mut attack_log = Vec::new();
        attack_log.extend_from_slice(&make_file_header(0));
        let rec0 = ChatRecord::new_text(1000, 0, sender_id, "User0".into(), "Msg 0");
        let rec1 = ChatRecord::new_text(1010, 5000, sender_id, "User1".into(), "Msg 1");
        attack_log.extend_from_slice(&rec0.serialize_record());
        attack_log.extend_from_slice(&rec1.serialize_record());
        // Insert false sync corrupted chunk
        attack_log.extend_from_slice(&bad_bytes);
        // Subsequent valid record
        let rec2 = ChatRecord::new_text(1020, 10000, sender_id, "User2".into(), "Msg 2");
        attack_log.extend_from_slice(&rec2.serialize_record());

        let scanner_attack = ChatRecoveryScanner::new(&attack_log);
        let (records_attack, stats_attack) = scanner_attack.recover().unwrap();
        assert_eq!(records_attack.len(), 3); // All 3 valid records recovered!
        assert!(stats_attack.corrupted_chunks >= 1);
    }
}
