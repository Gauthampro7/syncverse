//! Desktop graphical user interface modules (eframe/egui).

pub mod app;
pub mod aspect_ratio;
pub mod controls;
pub mod dialogs;
pub mod drag_drop;
pub mod file_dialog;
pub mod inhibit;
pub mod mpris;
pub mod reactions;
pub mod recent_files;
pub mod sidebar;
pub mod subtitle_overlay;
pub mod updater;
pub mod viewport;

pub use app::{create_native_options, run_desktop_gui, run_desktop_player, SyncplayApp};
pub use aspect_ratio::{AspectRatioOverride, IntoDimension};
pub use controls::{
    style as controls_style, ControlsBarAction, ControlsBarState, PlaybackControlsBar,
    VolumeControlState, SUPPORTED_SPEEDS,
};
pub use dialogs::{
    AboutDialog, AudioConfigAction, AudioConfigDialog, AudioTrackItem, ConnectPartyDialog,
    HostPartyDialog, ShortcutsDialog, SubtitleConfigAction, SubtitleConfigDialog,
    SubtitleTrackItem, VideoConfigAction, VideoConfigDialog, VideoStreamMetadata,
};
pub use drag_drop::{is_supported_media, DragDropHandler, SUPPORTED_EXTENSIONS};
pub use file_dialog::FileDialogManager;
pub use inhibit::IdleInhibitor;
pub use mpris::MprisManager;
pub use reactions::{ReactionParticle, ReactionSystem, REACTION_PRESETS};
pub use recent_files::RecentFilesManager;
pub use sidebar::{
    evaluate_file_match, evaluate_file_match_with_hash, format_duration_hms, FileMatchStatus,
    RoomMember, SidebarState,
};
pub use subtitle_overlay::{
    compute_effective_pts, find_active_cue, find_active_cue_text, find_active_cues,
    ActiveSubtitleTrack, SubtitleCue, SubtitleOverlay, SubtitleOverlayConfig, SubtitleTrack,
};
pub use updater::{
    check_for_updates, open_browser_url, spawn_update_check, UpdateError, UpdateInfo,
};
pub use viewport::{
    VideoTextureBuffer, VideoViewport, ViewportAction, ViewportDisplayMode, ViewportGeometry,
};
