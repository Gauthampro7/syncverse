//! Deterministic conversation history export in JSON and Markdown formats.

use syncverse_common::MediaTime;
use crate::record::ChatRecord;

/// Supported transcript export formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptFormat {
    /// Machine-readable structured JSON array.
    Json,
    /// Human-readable chronological Markdown transcript.
    Markdown,
}

/// Exports a collection of chat records in Markdown transcript format.
pub fn export_transcript_markdown(records: &[ChatRecord]) -> String {
    let mut out = String::from("# Syncverse Room Chat Transcript\n\n");
    for rec in records {
        if rec.flags & 1 != 0 {
            continue; // Skip deleted/tombstone records
        }
        let timecode = if rec.media_timestamp_ms == u64::MAX {
            "--:--:--.---".to_string()
        } else {
            MediaTime::from_millis(rec.media_timestamp_ms).to_timecode_string()
        };
        let text = String::from_utf8_lossy(&rec.payload);
        out.push_str(&format!("[{}] <{}>: {}\n", timecode, rec.sender_name, text));
    }
    out
}

/// Exports a collection of chat records in standard JSON array format.
pub fn export_history_json(records: &[ChatRecord]) -> String {
    let mut items = Vec::new();
    for rec in records {
        if rec.flags & 1 != 0 {
            continue;
        }
        let timecode = if rec.media_timestamp_ms == u64::MAX {
            "unanchored".to_string()
        } else {
            MediaTime::from_millis(rec.media_timestamp_ms).to_timecode_string()
        };
        let text = String::from_utf8_lossy(&rec.payload);
        let sender_uuid_hex = rec
            .sender_id
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>();

        let escaped_text = text.replace('\\', "\\\\").replace('"', "\\\"");
        let escaped_name = rec.sender_name.replace('\\', "\\\\").replace('"', "\\\"");

        items.push(format!(
            "  {{\n    \"sender_id\": \"{}\",\n    \"sender_name\": \"{}\",\n    \"wall_timestamp_utc_ms\": {},\n    \"media_timecode\": \"{}\",\n    \"text\": \"{}\"\n  }}",
            sender_uuid_hex, escaped_name, rec.wall_timestamp_utc_ms, timecode, escaped_text
        ));
    }

    format!("[\n{}\n]", items.join(",\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_export_markdown_transcript() {
        let sid = [0u8; 16];
        let records = vec![
            ChatRecord::new_text(1000, 14320, sid, "Alice".into(), "Hello!"),
            ChatRecord::new_text(1020, 18500, sid, "Bob".into(), "Welcome!"),
        ];

        let md = export_transcript_markdown(&records);
        assert!(md.contains("# Syncverse Room Chat Transcript"));
        assert!(md.contains("[00:00:14.320] <Alice>: Hello!"));
        assert!(md.contains("[00:00:18.500] <Bob>: Welcome!"));

        let json = export_history_json(&records);
        assert!(json.contains("\"sender_name\": \"Alice\""));
        assert!(json.contains("\"media_timecode\": \"00:00:14.320\""));
    }
}
