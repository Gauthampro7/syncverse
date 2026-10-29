//! Deterministic media file fingerprinting and desynchronization detection for Watch-Party 2.0.
//!
//! Provides ultra-fast cryptographic media fingerprinting across gigabyte-scale video files
//! by hashing key file chunks (header, midpoint, and trailer) along with container duration
//! and file size using SHA-256, enabling instant detection of mismatched movie cuts.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use ring::digest::{Context, SHA256};

/// Maximum chunk size sampled from each region of the file (64 KB).
const CHUNK_SIZE: usize = 65536;

/// Deterministic fingerprint uniquely identifying a media file and its container properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MediaFingerprint {
    /// SHA-256 hash digest of sampled container chunks and metadata.
    pub hash: [u8; 32],
    /// Media container duration in milliseconds.
    pub duration_ms: u64,
    /// Total file size on disk in bytes.
    pub file_size_bytes: u64,
}

/// Comparison status between local peer's media and the room host's media.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaMatchStatus {
    /// Both peers have identical media files (exact SHA-256 fingerprint match).
    ExactMatch,
    /// Similar container duration (within 2s), but different hash (e.g. different encode/remux).
    Variation,
    /// Container duration differs significantly (> 2s), indicating different cuts or episodes.
    DesyncAlert {
        /// Duration delta in milliseconds: `abs(local - host)`.
        delta_ms: u64,
    },
    /// One or both peers have no media loaded.
    NoMedia,
}

impl MediaFingerprint {
    /// Computes a fast deterministic fingerprint for the media file at `path`.
    ///
    /// Reads at most 192 KB total (up to 64 KB from head, middle, and tail) to guarantee
    /// sub-millisecond execution regardless of file size (e.g. 50GB 4K remuxes).
    pub fn compute<P: AsRef<Path>>(path: P, duration_ms: u64) -> std::io::Result<Self> {
        let mut file = File::open(path)?;
        let file_size = file.metadata()?.len();

        let mut context = Context::new(&SHA256);
        context.update(b"Syncverse-MediaFingerprint-v1:");
        context.update(&duration_ms.to_le_bytes());
        context.update(&file_size.to_le_bytes());

        let mut buf = vec![0u8; CHUNK_SIZE];

        // 1. Read head chunk
        file.seek(SeekFrom::Start(0))?;
        let n_head = file.read(&mut buf)?;
        context.update(&buf[..n_head]);

        // 2. Read middle chunk if file is sufficiently large
        if file_size > (CHUNK_SIZE as u64 * 2) {
            let mid_offset = (file_size / 2).saturating_sub((CHUNK_SIZE / 2) as u64);
            file.seek(SeekFrom::Start(mid_offset))?;
            let n_mid = file.read(&mut buf)?;
            context.update(&buf[..n_mid]);
        }

        // 3. Read tail chunk if file is larger than one chunk
        if file_size > CHUNK_SIZE as u64 {
            let tail_offset = file_size.saturating_sub(CHUNK_SIZE as u64);
            file.seek(SeekFrom::Start(tail_offset))?;
            let n_tail = file.read(&mut buf)?;
            context.update(&buf[..n_tail]);
        }

        let digest = context.finish();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(digest.as_ref());

        Ok(Self {
            hash,
            duration_ms,
            file_size_bytes: file_size,
        })
    }

    /// Evaluates synchronization compatibility against another peer's media fingerprint.
    pub fn compare(&self, other: &Self) -> MediaMatchStatus {
        if self.file_size_bytes == 0 || other.file_size_bytes == 0 {
            return MediaMatchStatus::NoMedia;
        }

        if self.hash == other.hash && self.duration_ms == other.duration_ms {
            return MediaMatchStatus::ExactMatch;
        }

        let delta_ms = (self.duration_ms as i64 - other.duration_ms as i64).unsigned_abs();
        if delta_ms > 2000 {
            MediaMatchStatus::DesyncAlert { delta_ms }
        } else {
            MediaMatchStatus::Variation
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(1);

    fn create_test_file(prefix: &str, content: &[u8]) -> std::path::PathBuf {
        let count = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("syncverse_fp_test_{}_{}.bin", prefix, count));
        let mut f = File::create(&path).unwrap();
        f.write_all(content).unwrap();
        f.flush().unwrap();
        path
    }

    #[test]
    fn test_fingerprint_identical_files() {
        let content = vec![0x42u8; 100_000];
        let path1 = create_test_file("ident1", &content);
        let path2 = create_test_file("ident2", &content);

        let fp1 = MediaFingerprint::compute(&path1, 120_000).unwrap();
        let fp2 = MediaFingerprint::compute(&path2, 120_000).unwrap();

        let _ = std::fs::remove_file(path1);
        let _ = std::fs::remove_file(path2);

        assert_eq!(fp1, fp2);
        assert_eq!(fp1.compare(&fp2), MediaMatchStatus::ExactMatch);
    }

    #[test]
    fn test_fingerprint_different_duration_desync() {
        let path = create_test_file("cut", &[1, 2, 3, 4]);

        let fp_theatrical = MediaFingerprint::compute(&path, 7_200_000).unwrap();
        let fp_extended = MediaFingerprint::compute(&path, 7_500_000).unwrap();

        let _ = std::fs::remove_file(path);

        match fp_theatrical.compare(&fp_extended) {
            MediaMatchStatus::DesyncAlert { delta_ms } => assert_eq!(delta_ms, 300_000),
            other => panic!("Expected DesyncAlert, got {:?}", other),
        }
    }

    #[test]
    fn test_fingerprint_same_duration_different_content_variation() {
        let path1 = create_test_file("var1", b"1080p release content");
        let path2 = create_test_file("var2", b"720p release content!");

        let fp1 = MediaFingerprint::compute(&path1, 50_000).unwrap();
        let fp2 = MediaFingerprint::compute(&path2, 50_500).unwrap();

        let _ = std::fs::remove_file(path1);
        let _ = std::fs::remove_file(path2);

        assert_eq!(fp1.compare(&fp2), MediaMatchStatus::Variation);
    }
}
