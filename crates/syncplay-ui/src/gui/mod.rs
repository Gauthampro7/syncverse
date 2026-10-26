//! Desktop graphical user interface modules (eframe/egui).

pub mod app;
pub mod controls;
pub mod dialogs;
pub mod drag_drop;
pub mod file_dialog;
pub mod recent_files;
pub mod sidebar;
pub mod viewport;

pub use app::{create_native_options, run_desktop_gui, run_desktop_player, SyncplayApp};
pub use controls::{
    style as controls_style, ControlsBarAction, ControlsBarState, PlaybackControlsBar,
    VolumeControlState, SUPPORTED_SPEEDS,
};
pub use dialogs::{AboutDialog, ConnectPartyDialog, HostPartyDialog, ShortcutsDialog};
pub use drag_drop::{is_supported_media, DragDropHandler, SUPPORTED_EXTENSIONS};
pub use file_dialog::FileDialogManager;
pub use recent_files::RecentFilesManager;
pub use sidebar::{RoomMember, SidebarState};
pub use viewport::{
    VideoTextureBuffer, VideoViewport, ViewportAction, ViewportDisplayMode, ViewportGeometry,
};
