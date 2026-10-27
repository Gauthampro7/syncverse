//! Master A/V monotonic presentation clock, SeqLock continuous re-anchoring, and lock-free SPSC ring buffer.

#![deny(warnings)]
#![deny(missing_docs)]

pub mod audio_pacer;
pub mod error;
pub mod master_clock;
pub mod reorder;
pub mod spsc;

pub use audio_pacer::{AudioPacer, AudioPacerStats, AudioTimingAction};
pub use error::{BufferFullError, ClockError};
pub use master_clock::{MasterClock, PlaybackState, PresentationAction};
pub use reorder::PresentationReorderBuffer;
pub use spsc::{CacheAligned, SpscRingBuffer};

