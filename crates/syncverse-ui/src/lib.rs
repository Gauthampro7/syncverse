//! Native UI state, lock-free triple-buffered atomic snapshots, and desktop graphical player UI.

#![deny(warnings)]
#![deny(missing_docs)]

pub mod command;
pub mod debouncer;
pub mod error;
pub mod event;
pub mod gui;
pub mod osd;
pub mod overlay;
pub mod render;
pub mod scrubber;
pub mod snapshot;
pub mod terminal;

pub use command::PlayerCommand;
pub use debouncer::SeekDebouncer;
pub use error::UiError;
pub use event::UiEvent;
pub use gui::{
    controls_style, create_native_options, evaluate_file_match, evaluate_file_match_with_hash,
    format_duration_hms, is_supported_media, run_desktop_gui, run_desktop_player,
    ActiveSubtitleTrack, AspectRatioOverride, ControlsBarAction, ControlsBarState, DragDropHandler,
    FileDialogManager, FileMatchStatus, IntoDimension, PlaybackControlsBar,
    ReactionParticle, ReactionSystem, RecentFilesManager, RoomMember, SidebarState,
    SubtitleCue, SubtitleOverlay, SubtitleOverlayConfig, SubtitleTrack, SyncplayApp,
    VideoTextureBuffer, VideoViewport, ViewportAction, ViewportDisplayMode, ViewportGeometry,
    VolumeControlState, compute_effective_pts, find_active_cue, find_active_cue_text,
    find_active_cues, REACTION_PRESETS, SUPPORTED_EXTENSIONS, SUPPORTED_SPEEDS,
};
pub use osd::{
    calculate_alpha_dwell_fade, evaluate_tight_sync_badge, render_faded_chat_comment,
    OsdOverlay, TightSyncBadge,
};
pub use overlay::{
    evaluate_sync_health, format_timecode_hud, BoundedChatOverlay, ChatBubble, FontAtlasAlpha8,
    SyncBadgeColor, TransportGlyph, TransportGlyphKind,
};
pub use render::{
    render_half_block_canvas, render_half_block_row, render_styled_scrubber_bar,
    render_sync_diagnostics_panel, ScrubberStyle,
};
pub use scrubber::ScrubberDebouncer;
pub use snapshot::{
    PlayerUiSnapshot, TripleBufferSnapshotExchange, PLAYBACK_STATE_BUFFERING, PLAYBACK_STATE_PAUSED,
    PLAYBACK_STATE_PLAYING, PLAYBACK_STATE_SEEKING, SNAPSHOT_FLAG_FORCE_PLAY,
    SNAPSHOT_FLAG_GATE_BLOCKED, SNAPSHOT_FLAG_HAS_MISMATCH, SNAPSHOT_FLAG_LOCAL_READY,
};
pub use terminal::{
    get_terminal_size, parse_key_sequence, render_hotkeys_footer, render_osd_hud,
    render_scrubber_bar, KeyCommand, OsdHudState, TerminalGuard,
};
