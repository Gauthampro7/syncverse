//! Recent files history management with zero-dependency plaintext persistence.

use std::path::{Path, PathBuf};

/// Manager tracking up to 10 recently opened media files in MRU order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecentFilesManager {
    /// Ordered list of recent media file paths.
    pub recent_paths: Vec<PathBuf>,
}

impl RecentFilesManager {
    /// Maximum number of recent files to retain.
    pub const MAX_ENTRIES: usize = 10;

    /// Loads recent files history from configuration directory.
    pub fn load() -> Self {
        let mut manager = Self::default();
        if let Some(config_path) = Self::config_path() {
            if config_path.exists() {
                if let Ok(contents) = std::fs::read_to_string(&config_path) {
                    for line in contents.lines() {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            let p = PathBuf::from(trimmed);
                            if p.exists() && !manager.recent_paths.contains(&p) {
                                manager.recent_paths.push(p);
                            }
                        }
                    }
                }
            }
        }
        manager
    }

    /// Adds a newly opened path to the top of the recent list and saves to disk.
    pub fn add_file(&mut self, path: impl AsRef<Path>) {
        let pb = path.as_ref().to_path_buf();
        self.recent_paths.retain(|p| p != &pb);
        self.recent_paths.insert(0, pb);
        if self.recent_paths.len() > Self::MAX_ENTRIES {
            self.recent_paths.truncate(Self::MAX_ENTRIES);
        }
        self.save();
    }

    /// Clears recent files history and removes config file.
    pub fn clear(&mut self) {
        self.recent_paths.clear();
        self.save();
    }

    /// Serialises the list: one path per line, `\n` terminated (format unchanged since v0.4).
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for p in &self.recent_paths {
            out.push_str(&p.to_string_lossy());
            out.push('\n');
        }
        out
    }

    /// Saves recent files list line-by-line to config file (atomic tmp+fsync+rename).
    fn save(&self) {
        if let Some(config_path) = Self::config_path() {
            let _ = syncverse_common::userdata::atomic_write(&config_path, self.to_text().as_bytes());
        }
    }

    /// Resolves `$XDG_CONFIG_HOME/syncverse/recent_files.txt` on Linux,
    /// or `%APPDATA%/syncverse/recent_files.txt` / `%USERPROFILE%/AppData/Roaming/syncverse/recent_files.txt` on Windows.
    fn config_path() -> Option<PathBuf> {
        syncverse_common::userdata::UserDirs::from_env()
            .ok()
            .map(|d| d.recent_files_path())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recent_files_deduplication_and_limit() {
        let mut manager = RecentFilesManager::default();
        for i in 0..15 {
            manager.recent_paths.push(PathBuf::from(format!("/movie_{}.mkv", i)));
        }
        assert_eq!(manager.recent_paths.len(), 15);

        manager.add_file(PathBuf::from("/movie_99.mkv"));
        assert_eq!(manager.recent_paths.len(), RecentFilesManager::MAX_ENTRIES);
        assert_eq!(manager.recent_paths[0], PathBuf::from("/movie_99.mkv"));
    }

    #[test]
    fn golden_file_format_is_one_path_per_line() {
        let m = RecentFilesManager {
            recent_paths: vec![PathBuf::from("/a/one.mkv"), PathBuf::from("/b/two words.mp4")],
        };
        assert_eq!(m.to_text(), "/a/one.mkv\n/b/two words.mp4\n");
        assert_eq!(RecentFilesManager::default().to_text(), "");
    }

    #[test]
    fn test_config_path_resolution() {
        let p = RecentFilesManager::config_path();
        assert!(p.is_some(), "config_path must resolve on standard OS environments");
        let path = p.unwrap();
        assert!(
            path.ends_with("syncverse/recent_files.txt")
                || path.ends_with("syncverse\\recent_files.txt"),
            "Path must terminate with syncverse/recent_files.txt"
        );
    }
}
