//! Top-level runtime coordinator binding all subsystems.

#![deny(warnings)]
#![deny(missing_docs)]
#![deny(unsafe_code)]

pub mod aggregator;
pub mod audio_sink;
pub mod coordinator;
pub mod error;
pub mod interactive;
pub mod playback_runtime;

pub use aggregator::{SubsystemMetrics, TelemetryAggregator};
pub use audio_sink::{AudioSink, AutoAudioSink, MockAudioSink, NullAudioSink, PipeAudioSink};
pub use coordinator::{MediaInfo, PartyRole, PlayerCoordinator};
pub use error::CoreError;
pub use interactive::{InteractivePlayer, PlayerInputMode};
pub use playback_runtime::{launch_desktop_player_app, run_playback_worker, AudioWorkerCommand};
pub use syncplay_common::normalize_target_path;

