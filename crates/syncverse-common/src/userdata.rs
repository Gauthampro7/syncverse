//! Persistent per-user data store (settings, resume positions) with atomic writes.
//!
//! Everything here is a tiny in-tree text format; there is no external dependency.
//!
//! * [`UserDirs`] resolves the config (`$XDG_CONFIG_HOME`, `%APPDATA%`) and state
//!   (`$XDG_STATE_HOME`) directories.
//! * [`atomic_write`] replaces a file crash-safely: write `<file>.tmp`, `fsync`,
//!   `rename`, then `fsync` the directory. A crash at any point leaves either the
//!   old or the new content, and a leftover `.tmp` is ignored by every reader.
//! * [`Settings`] is a versioned `key=value` file. Unknown keys are preserved on
//!   rewrite; corrupt lines, a UTF-8 BOM and CRLF endings are tolerated.
//! * [`FileKey`] identifies a media file by `size + CRC32(first 64 KiB) +
//!   CRC32(last 64 KiB)`: at most 128 KiB is read, so multi-GB files key instantly
//!   and the key survives rename/move/copy.
//! * [`ResumeStore`] is an LRU (500) map from [`FileKey`] to [`ResumeEntry`].

use crate::crc::Crc32;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Current settings file format version, written as `settings_version=1`.
pub const SETTINGS_VERSION: u32 = 1;
/// Maximum number of per-file resume entries retained (LRU).
pub const RESUME_CAPACITY: usize = 500;
/// Number of bytes hashed at each end of a file by [`FileKey`].
pub const FILE_KEY_SAMPLE: u64 = 64 * 1024;

/// Errors raised by the user-data layer.
#[derive(Error, Debug)]
pub enum UserDataError {
    /// No suitable configuration/state directory could be resolved.
    #[error("no user data directory could be resolved from the environment")]
    NoDirectory,
    /// An I/O operation failed.
    #[error("{op} `{path}` failed: {source}")]
    Io {
        /// Short operation name (`read`, `write`, `rename`, ...).
        op: &'static str,
        /// Path involved.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
}

fn io_err<'a>(
    op: &'static str,
    path: &'a Path,
) -> impl FnOnce(std::io::Error) -> UserDataError + 'a {
    move |source| UserDataError::Io {
        op,
        path: path.to_path_buf(),
        source,
    }
}

// ───────────────────────────── paths ─────────────────────────────

/// Resolved per-user directories (each already includes the `syncverse` leaf).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserDirs {
    /// Configuration directory (settings, recent files).
    pub config: PathBuf,
    /// State directory (resume store, queue).
    pub state: PathBuf,
}

impl UserDirs {
    /// Resolves directories from the process environment.
    ///
    /// Config: `XDG_CONFIG_HOME` → `APPDATA` → `USERPROFILE\AppData\Roaming` → `~/.config`.
    /// State: `XDG_STATE_HOME` → `LOCALAPPDATA` → `APPDATA` → `~/.local/state`.
    pub fn from_env() -> Result<Self, UserDataError> {
        Self::from_lookup(|k| std::env::var_os(k).filter(|v| !v.is_empty()))
    }

    /// Resolves directories using a custom variable lookup (used by tests).
    pub fn from_lookup(
        get: impl Fn(&str) -> Option<std::ffi::OsString>,
    ) -> Result<Self, UserDataError> {
        let p = |k: &str| get(k).map(PathBuf::from);
        let home = p("HOME");
        let config = p("XDG_CONFIG_HOME")
            .or_else(|| p("APPDATA"))
            .or_else(|| p("USERPROFILE").map(|u| u.join("AppData").join("Roaming")))
            .or_else(|| home.clone().map(|h| h.join(".config")))
            .ok_or(UserDataError::NoDirectory)?;
        let state = p("XDG_STATE_HOME")
            .or_else(|| p("LOCALAPPDATA"))
            .or_else(|| p("APPDATA"))
            .or_else(|| p("USERPROFILE").map(|u| u.join("AppData").join("Local")))
            .or_else(|| home.map(|h| h.join(".local").join("state")))
            .ok_or(UserDataError::NoDirectory)?;
        Ok(Self {
            config: config.join("syncverse"),
            state: state.join("syncverse"),
        })
    }

    /// Directories rooted under one base directory (`base/config`, `base/state`).
    pub fn under(base: impl AsRef<Path>) -> Self {
        Self {
            config: base.as_ref().join("config").join("syncverse"),
            state: base.as_ref().join("state").join("syncverse"),
        }
    }

    /// `settings.conf` path.
    pub fn settings_path(&self) -> PathBuf {
        self.config.join("settings.conf")
    }

    /// `recent_files.txt` path (format unchanged since v0.4).
    pub fn recent_files_path(&self) -> PathBuf {
        self.config.join("recent_files.txt")
    }

    /// `resume.tsv` path.
    pub fn resume_path(&self) -> PathBuf {
        self.state.join("resume.tsv")
    }

    /// `queue.m3u` path (playlist autosave).
    pub fn queue_path(&self) -> PathBuf {
        self.state.join("queue.m3u")
    }
}

// ─────────────────────────── atomic write ───────────────────────────

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".tmp");
    path.with_file_name(name)
}

/// First half of [`atomic_write`]: writes and `fsync`s `<path>.tmp` without touching `path`.
///
/// Exposed so tests can simulate a crash between staging and commit.
pub fn stage_tmp(path: &Path, bytes: &[u8]) -> Result<PathBuf, UserDataError> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(io_err("create_dir", parent))?;
    }
    let tmp = tmp_path(path);
    let mut f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&tmp)
        .map_err(io_err("create", &tmp))?;
    f.write_all(bytes).map_err(io_err("write", &tmp))?;
    f.sync_all().map_err(io_err("fsync", &tmp))?;
    Ok(tmp)
}

/// Second half of [`atomic_write`]: renames the staged `.tmp` over `path`, then syncs the directory.
pub fn commit_tmp(path: &Path) -> Result<(), UserDataError> {
    let tmp = tmp_path(path);
    fs::rename(&tmp, path).map_err(io_err("rename", &tmp))?;
    #[cfg(unix)]
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        if let Ok(dir) = File::open(parent) {
            // Best effort: some filesystems refuse directory fsync.
            let _ = dir.sync_all();
        }
    }
    Ok(())
}

/// Atomically replaces `path` with `bytes` (tmp + fsync + rename).
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), UserDataError> {
    stage_tmp(path, bytes)?;
    commit_tmp(path)
}

/// Reads a text file, returning `None` if absent/unreadable; strips a UTF-8 BOM, lossy-decodes.
fn read_text(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let s = String::from_utf8_lossy(&bytes).into_owned();
    Some(s.strip_prefix('\u{feff}').map(str::to_owned).unwrap_or(s))
}

// ───────────────────────────── settings ─────────────────────────────

/// Well-known settings keys (all other keys are preserved untouched).
pub mod keys {
    /// Playback volume, `0.0..=1.0`.
    pub const VOLUME: &str = "volume";
    /// Muted flag (`true`/`false`).
    pub const MUTED: &str = "muted";
    /// Username in watch-parties.
    pub const USERNAME: &str = "username";
    /// Last host port.
    pub const LAST_HOST_PORT: &str = "last_host_port";
    /// Sidebar visible flag.
    pub const SIDEBAR_OPEN: &str = "sidebar_open";
    /// Hardware decode mode name.
    pub const HWDEC_MODE: &str = "hwdec_mode";
    /// Resume policy: `ask`, `always`, `never`.
    pub const RESUME_POLICY: &str = "resume_policy";
    /// Remember audio/subtitle choices etc. per file (`true`/`false`).
    pub const REMEMBER_PER_FILE: &str = "remember_per_file";
    /// Remember global settings (`true`/`false`).
    pub const REMEMBER_SETTINGS: &str = "remember_settings";
    /// Subtitle style blob (opaque string).
    pub const SUBTITLE_STYLE: &str = "subtitle_style";
    /// Equalizer blob (opaque string).
    pub const EQ: &str = "eq";
}

/// Versioned `key=value` settings with unknown-key preservation.
///
/// Keys keep their first-seen order; later duplicates overwrite earlier ones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    version: u32,
    entries: Vec<(String, String)>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            entries: Vec::new(),
        }
    }
}

impl Settings {
    /// Parses settings text. Never fails: blank lines, `#` comments, lines without `=`
    /// or with an empty/garbage key are skipped; CRLF and a leading BOM are accepted.
    pub fn parse(text: &str) -> Self {
        let mut s = Self::default();
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        for line in text.lines() {
            let line = line.trim_end_matches('\r').trim_start();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let k = k.trim();
            if k.is_empty() || k.contains('\u{fffd}') || k.chars().any(char::is_control) {
                continue;
            }
            if k == "settings_version" {
                if let Ok(v) = v.trim().parse() {
                    s.version = v;
                }
                continue;
            }
            s.set(k, v.trim());
        }
        s
    }

    /// Serialises to text: `settings_version=N` first, then entries in order.
    pub fn to_text(&self) -> String {
        let mut out = format!("settings_version={}\n", self.version.max(SETTINGS_VERSION));
        for (k, v) in &self.entries {
            out.push_str(k);
            out.push('=');
            // Values cannot contain line breaks in this format.
            out.push_str(&v.replace(['\n', '\r'], " "));
            out.push('\n');
        }
        out
    }

    /// Loads settings from `path`; a missing or garbage file yields defaults.
    pub fn load(path: &Path) -> Self {
        read_text(path).map(|t| Self::parse(&t)).unwrap_or_default()
    }

    /// Saves atomically to `path`.
    pub fn save(&self, path: &Path) -> Result<(), UserDataError> {
        atomic_write(path, self.to_text().as_bytes())
    }

    /// File format version found when parsing (may exceed [`SETTINGS_VERSION`]).
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Raw string value.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Sets a value (insert or overwrite in place).
    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        let value = value.into();
        match self.entries.iter_mut().find(|(k, _)| k == key) {
            Some(e) => e.1 = value,
            None => self.entries.push((key.to_owned(), value)),
        }
    }

    /// Removes a key, returning whether it existed.
    pub fn remove(&mut self, key: &str) -> bool {
        let n = self.entries.len();
        self.entries.retain(|(k, _)| k != key);
        n != self.entries.len()
    }

    /// Boolean value (`true/false/1/0/yes/no/on/off`), `None` if absent or malformed.
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        match self.get(key)?.to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => Some(true),
            "false" | "0" | "no" | "off" => Some(false),
            _ => None,
        }
    }

    /// Sets a boolean.
    pub fn set_bool(&mut self, key: &str, v: bool) {
        self.set(key, if v { "true" } else { "false" });
    }

    /// Unsigned integer value.
    pub fn get_u64(&self, key: &str) -> Option<u64> {
        self.get(key)?.parse().ok()
    }

    /// Finite float value.
    pub fn get_f32(&self, key: &str) -> Option<f32> {
        self.get(key)?.parse().ok().filter(|f: &f32| f.is_finite())
    }

    /// Iterates all `(key, value)` pairs in order (including unknown keys).
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

// ───────────────────────────── file key ─────────────────────────────

/// Content-based file identity: size + CRC32 of first and last 64 KiB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileKey {
    /// File length in bytes.
    pub size: u64,
    /// CRC32 of the first `min(size, 64 KiB)` bytes.
    pub head_crc: u32,
    /// CRC32 of the last `min(size, 64 KiB)` bytes.
    pub tail_crc: u32,
}

impl FileKey {
    /// Computes the key of a file, reading at most 128 KiB.
    pub fn from_path(path: &Path) -> Result<Self, UserDataError> {
        let mut f = File::open(path).map_err(io_err("open", path))?;
        let size = f.metadata().map_err(io_err("stat", path))?.len();
        let n = size.min(FILE_KEY_SAMPLE) as usize;
        let mut buf = vec![0u8; n];
        f.read_exact(&mut buf).map_err(io_err("read", path))?;
        let head_crc = Crc32::digest(&buf);
        f.seek(SeekFrom::Start(size - n as u64))
            .map_err(io_err("seek", path))?;
        f.read_exact(&mut buf).map_err(io_err("read", path))?;
        let tail_crc = Crc32::digest(&buf);
        Ok(Self {
            size,
            head_crc,
            tail_crc,
        })
    }

    /// Canonical text form `size-head-tail` (decimal size, 8 hex digits each).
    pub fn to_text(&self) -> String {
        format!("{}-{:08x}-{:08x}", self.size, self.head_crc, self.tail_crc)
    }

    /// Parses [`FileKey::to_text`] output.
    pub fn parse(s: &str) -> Option<Self> {
        let mut it = s.split('-');
        let size = it.next()?.parse().ok()?;
        let head_crc = u32::from_str_radix(it.next()?, 16).ok()?;
        let tail_crc = u32::from_str_radix(it.next()?, 16).ok()?;
        if it.next().is_some() {
            return None;
        }
        Some(Self {
            size,
            head_crc,
            tail_crc,
        })
    }
}

// ──────────────────────────── resume store ────────────────────────────

/// Per-file remembered state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeEntry {
    /// File identity.
    pub key: FileKey,
    /// Last playback position (ms); `0` when finished/cleared.
    pub pos_ms: u64,
    /// Media duration (ms), `0` if unknown.
    pub dur_ms: u64,
    /// Selected audio track index, `-1` = default/unset.
    pub audio: i32,
    /// Selected subtitle track index, `-1` = default/unset, `-2` = off.
    pub sub: i32,
    /// Audio delay (ms, signed).
    pub audio_delay_ms: i64,
    /// Subtitle delay (ms, signed).
    pub sub_delay_ms: i64,
    /// Aspect override label (`auto`, `16:9`, ...); whitespace is replaced by `_` on save.
    pub aspect: String,
    /// Unix seconds of last update.
    pub updated: u64,
}

impl ResumeEntry {
    /// New entry with defaults (no tracks chosen, `auto` aspect, position 0).
    pub fn new(key: FileKey) -> Self {
        Self {
            key,
            pos_ms: 0,
            dur_ms: 0,
            audio: -1,
            sub: -1,
            audio_delay_ms: 0,
            sub_delay_ms: 0,
            aspect: "auto".into(),
            updated: 0,
        }
    }

    fn to_line(&self) -> String {
        let aspect: String = self
            .aspect
            .chars()
            .map(|c| {
                if c.is_whitespace() || c.is_control() {
                    '_'
                } else {
                    c
                }
            })
            .collect();
        let aspect = if aspect.is_empty() {
            "auto".into()
        } else {
            aspect
        };
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            self.key.to_text(),
            self.pos_ms,
            self.dur_ms,
            self.audio,
            self.sub,
            self.audio_delay_ms,
            self.sub_delay_ms,
            aspect,
            self.updated
        )
    }

    fn from_line(line: &str) -> Option<Self> {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 9 {
            return None;
        }
        Some(Self {
            key: FileKey::parse(f[0])?,
            pos_ms: f[1].parse().ok()?,
            dur_ms: f[2].parse().ok()?,
            audio: f[3].parse().ok()?,
            sub: f[4].parse().ok()?,
            audio_delay_ms: f[5].parse().ok()?,
            sub_delay_ms: f[6].parse().ok()?,
            aspect: f[7].to_owned(),
            updated: f[8].parse().ok()?,
        })
    }
}

/// LRU store of [`ResumeEntry`] (most recently used first, at most [`RESUME_CAPACITY`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResumeStore {
    entries: Vec<ResumeEntry>,
}

impl ResumeStore {
    /// Empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses store text (`resume.tsv`); malformed lines, BOM and CRLF are tolerated.
    /// Duplicate keys keep the first (most recent) occurrence.
    pub fn parse(text: &str) -> Self {
        let mut s = Self::new();
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        for line in text.lines() {
            let line = line.trim_end_matches('\r');
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(e) = ResumeEntry::from_line(line) {
                if !s.entries.iter().any(|x| x.key == e.key) && s.entries.len() < RESUME_CAPACITY {
                    s.entries.push(e);
                }
            }
        }
        s
    }

    /// Serialises, most recent first.
    pub fn to_text(&self) -> String {
        let mut out = String::from("# syncverse resume v1\n");
        for e in &self.entries {
            out.push_str(&e.to_line());
            out.push('\n');
        }
        out
    }

    /// Loads from disk; missing/garbage ⇒ empty.
    pub fn load(path: &Path) -> Self {
        read_text(path).map(|t| Self::parse(&t)).unwrap_or_default()
    }

    /// Saves atomically.
    pub fn save(&self, path: &Path) -> Result<(), UserDataError> {
        atomic_write(path, self.to_text().as_bytes())
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Looks up an entry without changing recency.
    pub fn get(&self, key: &FileKey) -> Option<&ResumeEntry> {
        self.entries.iter().find(|e| &e.key == key)
    }

    /// Inserts/replaces an entry as most recent; evicts the least recent beyond capacity.
    pub fn put(&mut self, entry: ResumeEntry) {
        self.entries.retain(|e| e.key != entry.key);
        self.entries.insert(0, entry);
        self.entries.truncate(RESUME_CAPACITY);
    }

    /// Removes an entry, returning it.
    pub fn remove(&mut self, key: &FileKey) -> Option<ResumeEntry> {
        let i = self.entries.iter().position(|e| &e.key == key)?;
        Some(self.entries.remove(i))
    }

    /// Iterates entries, most recent first.
    pub fn iter(&self) -> impl Iterator<Item = &ResumeEntry> {
        self.entries.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirs_prefer_xdg() {
        let d = UserDirs::from_lookup(|k| match k {
            "XDG_CONFIG_HOME" => Some("/c".into()),
            "XDG_STATE_HOME" => Some("/s".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(
            d.settings_path(),
            PathBuf::from("/c/syncverse/settings.conf")
        );
        assert_eq!(d.resume_path(), PathBuf::from("/s/syncverse/resume.tsv"));
    }

    #[test]
    fn dirs_fallback_home() {
        let d = UserDirs::from_lookup(|k| (k == "HOME").then(|| "/h".into())).unwrap();
        assert_eq!(d.config, PathBuf::from("/h/.config/syncverse"));
        assert_eq!(d.state, PathBuf::from("/h/.local/state/syncverse"));
        assert!(UserDirs::from_lookup(|_| None).is_err());
    }
}
