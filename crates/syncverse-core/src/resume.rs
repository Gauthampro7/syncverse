//! Resume-position tracking policy (offer thresholds, throttled saves, per-file track memory).
//!
//! [`ResumeTracker`] is pure logic over a [`ResumeStore`]; time is injected as a monotonic
//! millisecond counter (`now_ms`) so behaviour is deterministic and testable.
//!
//! Rules:
//! * A resume is offered only if `30 s <= pos <= duration - 60 s`.
//! * Reaching EOF (or passing the end threshold) clears the saved position.
//! * Never offered/applied inside a watch-party: the room timeline wins.
//! * Policy [`ResumePolicy`]: `Ask` (banner), `Always` (seek silently), `Never`.
//! * Position saves are throttled to one per [`SAVE_INTERVAL_MS`] while playing; pause,
//!   stop, open-other and quit force a save.
//! * Per-file audio/subtitle track, delays and aspect are remembered and restored
//!   regardless of policy (and also inside a party, they are local presentation state).

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use syncverse_common::userdata::{FileKey, ResumeEntry, ResumeStore, UserDataError};

/// Minimum saved position (ms) for a resume to be offered.
pub const MIN_RESUME_POS_MS: u64 = 30_000;
/// Distance from the end (ms) inside which a position is considered "finished".
pub const END_MARGIN_MS: u64 = 60_000;
/// Minimum interval between throttled position saves (ms).
pub const SAVE_INTERVAL_MS: u64 = 5_000;

/// What to do when a file with a saved position is opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResumePolicy {
    /// Show the Resume / Start over banner.
    #[default]
    Ask,
    /// Seek to the saved position without asking.
    Always,
    /// Never resume.
    Never,
}

impl ResumePolicy {
    /// Settings-file spelling (`ask`, `always`, `never`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Always => "always",
            Self::Never => "never",
        }
    }

    /// Parses the settings spelling (case-insensitive); unknown ⇒ `None`.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "ask" => Some(Self::Ask),
            "always" => Some(Self::Always),
            "never" => Some(Self::Never),
            _ => None,
        }
    }
}

/// Outcome of the position part of [`ResumeTracker::on_open`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeDecision {
    /// Nothing to resume (no entry, out of thresholds, policy `Never`, or party mode).
    None,
    /// Emit `UiEvent::ResumeAvailable { pos_ms, duration_ms }` and wait for accept/decline.
    Offer {
        /// Saved position (ms).
        pos_ms: u64,
    },
    /// Seek to `pos_ms` immediately (policy `Always`).
    AutoResume {
        /// Saved position (ms).
        pos_ms: u64,
    },
}

/// Per-file presentation state remembered between sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMemory {
    /// Audio track index (`-1` = default).
    pub audio: i32,
    /// Subtitle track index (`-1` = default, `-2` = off).
    pub sub: i32,
    /// Audio delay (ms).
    pub audio_delay_ms: i64,
    /// Subtitle delay (ms).
    pub sub_delay_ms: i64,
    /// Aspect label (`auto`, `16:9`, ...).
    pub aspect: String,
}

impl Default for FileMemory {
    fn default() -> Self {
        Self {
            audio: -1,
            sub: -1,
            audio_delay_ms: 0,
            sub_delay_ms: 0,
            aspect: "auto".into(),
        }
    }
}

/// Result of [`ResumeTracker::on_open`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenOutcome {
    /// Position decision.
    pub decision: ResumeDecision,
    /// Remembered per-file state to apply (`None` if the file was never seen).
    pub memory: Option<FileMemory>,
}

/// Tracks the current file and persists its resume state.
#[derive(Debug)]
pub struct ResumeTracker {
    store: ResumeStore,
    store_path: Option<PathBuf>,
    policy: ResumePolicy,
    remember_per_file: bool,
    current: Option<Current>,
    last_save_ms: Option<u64>,
    writes: u64,
    last_error: Option<UserDataError>,
}

#[derive(Debug)]
struct Current {
    key: FileKey,
    dur_ms: u64,
    in_party: bool,
    /// Last position reported (ms), flushed on stop/open-other.
    pos_ms: u64,
    memory: FileMemory,
}

fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Whether `pos_ms` of a `dur_ms`-long file is worth offering (`30 s <= pos <= dur-60 s`).
/// Unknown duration (`0`) never qualifies.
pub fn is_resumable(pos_ms: u64, dur_ms: u64) -> bool {
    dur_ms >= END_MARGIN_MS + MIN_RESUME_POS_MS
        && pos_ms >= MIN_RESUME_POS_MS
        && pos_ms <= dur_ms - END_MARGIN_MS
}

impl ResumeTracker {
    /// Loads the store from `store_path` (`None` ⇒ in-memory only).
    pub fn load(store_path: Option<&Path>, policy: ResumePolicy) -> Self {
        let store = store_path.map(ResumeStore::load).unwrap_or_default();
        Self::with_store(store, store_path.map(Path::to_path_buf), policy)
    }

    /// Builds a tracker around an existing store.
    pub fn with_store(
        store: ResumeStore,
        store_path: Option<PathBuf>,
        policy: ResumePolicy,
    ) -> Self {
        Self {
            store,
            store_path,
            policy,
            remember_per_file: true,
            current: None,
            last_save_ms: None,
            writes: 0,
            last_error: None,
        }
    }

    /// Changes the policy (Preferences).
    pub fn set_policy(&mut self, policy: ResumePolicy) {
        self.policy = policy;
    }

    /// Current policy.
    pub fn policy(&self) -> ResumePolicy {
        self.policy
    }

    /// Enables/disables per-file track/delay/aspect restore (and saving of it).
    pub fn set_remember_per_file(&mut self, on: bool) {
        self.remember_per_file = on;
    }

    /// Read access to the underlying store.
    pub fn store(&self) -> &ResumeStore {
        &self.store
    }

    /// Number of disk writes performed so far (for throttling tests/diagnostics).
    pub fn writes(&self) -> u64 {
        self.writes
    }

    /// Takes the last persistence error, if any (saves never fail playback).
    pub fn take_error(&mut self) -> Option<UserDataError> {
        self.last_error.take()
    }

    /// Declares whether the session is inside a watch-party (host or client).
    /// Call when joining/leaving a room; affects the *current* and later opens.
    pub fn set_in_party(&mut self, in_party: bool) {
        if let Some(c) = &mut self.current {
            c.in_party = in_party;
        }
    }

    /// Called when a file has finished loading and its duration is known.
    ///
    /// Flushes the previously open file, then returns the resume decision and the
    /// remembered per-file state of the new file.
    pub fn on_open(
        &mut self,
        key: FileKey,
        dur_ms: u64,
        in_party: bool,
        now_ms: u64,
    ) -> OpenOutcome {
        self.flush_current(now_ms, false);
        let saved = self.store.get(&key).cloned();
        let decision = match &saved {
            Some(e) if !in_party && is_resumable(e.pos_ms, dur_ms) => match self.policy {
                ResumePolicy::Ask => ResumeDecision::Offer { pos_ms: e.pos_ms },
                ResumePolicy::Always => ResumeDecision::AutoResume { pos_ms: e.pos_ms },
                ResumePolicy::Never => ResumeDecision::None,
            },
            _ => ResumeDecision::None,
        };
        let memory = saved
            .filter(|_| self.remember_per_file)
            .map(|e| FileMemory {
                audio: e.audio,
                sub: e.sub,
                audio_delay_ms: e.audio_delay_ms,
                sub_delay_ms: e.sub_delay_ms,
                aspect: e.aspect,
            });
        self.current = Some(Current {
            key,
            dur_ms,
            in_party,
            pos_ms: 0,
            memory: memory.clone().unwrap_or_default(),
        });
        self.last_save_ms = Some(now_ms);
        OpenOutcome { decision, memory }
    }

    /// Periodic call (any rate) with the current position and presentation state.
    ///
    /// Returns `true` if a disk write happened. Writes only while `playing` and at most
    /// once per [`SAVE_INTERVAL_MS`]. Pause/stop should call [`Self::on_pause`]/[`Self::on_stop`].
    pub fn on_tick(
        &mut self,
        pos_ms: u64,
        playing: bool,
        memory: &FileMemory,
        now_ms: u64,
    ) -> bool {
        let Some(c) = &mut self.current else {
            return false;
        };
        c.pos_ms = pos_ms;
        c.memory = memory.clone();
        if !playing {
            return false;
        }
        if let Some(last) = self.last_save_ms {
            if now_ms.saturating_sub(last) < SAVE_INTERVAL_MS {
                return false;
            }
        }
        self.flush_current(now_ms, false)
    }

    /// Forced save on pause (position and state are current).
    pub fn on_pause(&mut self, pos_ms: u64, memory: &FileMemory, now_ms: u64) -> bool {
        if let Some(c) = &mut self.current {
            c.pos_ms = pos_ms;
            c.memory = memory.clone();
        }
        self.flush_current(now_ms, false)
    }

    /// Forced save on stop / quit. `eof == true` clears the saved position (file finished).
    /// The file stays "open" for further ticks until the next [`Self::on_open`].
    pub fn on_stop(&mut self, pos_ms: u64, memory: &FileMemory, eof: bool, now_ms: u64) -> bool {
        if let Some(c) = &mut self.current {
            c.pos_ms = pos_ms;
            c.memory = memory.clone();
        }
        self.flush_current(now_ms, eof)
    }

    /// Drops the saved *position* of `key` (user chose "Start over"), keeping track memory.
    pub fn decline(&mut self, key: &FileKey, now_ms: u64) {
        if let Some(mut e) = self.store.get(key).cloned() {
            e.pos_ms = 0;
            e.updated = unix_secs();
            self.store.put(e);
            self.persist(now_ms);
        }
    }

    /// Writes the current file's entry. Returns `true` if written.
    fn flush_current(&mut self, now_ms: u64, eof: bool) -> bool {
        let Some(c) = &self.current else {
            return false;
        };
        let mut e = self
            .store
            .get(&c.key)
            .cloned()
            .unwrap_or_else(|| ResumeEntry::new(c.key));
        e.dur_ms = c.dur_ms;
        // In a party the room timeline owns position: keep any older one untouched.
        if !c.in_party {
            e.pos_ms = if eof || c.pos_ms > c.dur_ms.saturating_sub(END_MARGIN_MS) {
                0
            } else {
                c.pos_ms
            };
        }
        if self.remember_per_file {
            e.audio = c.memory.audio;
            e.sub = c.memory.sub;
            e.audio_delay_ms = c.memory.audio_delay_ms;
            e.sub_delay_ms = c.memory.sub_delay_ms;
            e.aspect = c.memory.aspect.clone();
        }
        e.updated = unix_secs();
        // Nothing worth remembering: avoid polluting the store with default entries.
        let default_mem = e.audio == -1
            && e.sub == -1
            && e.audio_delay_ms == 0
            && e.sub_delay_ms == 0
            && e.aspect == "auto";
        if e.pos_ms == 0 && default_mem {
            if self.store.remove(&e.key).is_none() {
                return false;
            }
        } else {
            self.store.put(e);
        }
        self.persist(now_ms)
    }

    fn persist(&mut self, now_ms: u64) -> bool {
        self.last_save_ms = Some(now_ms);
        if let Some(p) = &self.store_path {
            if let Err(err) = self.store.save(p) {
                self.last_error = Some(err);
                return false;
            }
        }
        self.writes += 1;
        true
    }
}
