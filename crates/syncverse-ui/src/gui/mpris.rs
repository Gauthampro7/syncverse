//! MPRIS v2 D-Bus interface integration (`org.mpris.MediaPlayer2`).
//!
//! Enables native desktop integration on Linux (Fedora Wayland/X11), allowing GNOME Shell
//! quick-settings media controls, lock screen widgets, and multimedia keyboard keys
//! to natively display and control Syncverse playback.

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use crate::command::PlayerCommand;

/// Shared playback metadata and state tracked by the MPRIS D-Bus service.
#[derive(Debug, Clone, Default)]
pub struct MprisPlaybackState {
    /// Active media title.
    pub title: String,
    /// Playback status: "Playing", "Paused", or "Stopped".
    pub status: String,
    /// Current playback position in microseconds.
    pub position_us: i64,
    /// Media total duration in microseconds.
    pub duration_us: i64,
    /// Volume ratio [0.0, 1.0].
    pub volume: f64,
}

#[cfg(target_os = "linux")]
mod linux_mpris {
    use super::*;
    use std::collections::HashMap;
    use zbus::zvariant::Value;

    pub struct MediaPlayer2Root;

    #[zbus::interface(name = "org.mpris.MediaPlayer2")]
    impl MediaPlayer2Root {
        fn raise(&self) {}
        fn quit(&self) {}

        #[zbus(property)]
        fn can_quit(&self) -> bool {
            false
        }

        #[zbus(property)]
        fn can_raise(&self) -> bool {
            false
        }

        #[zbus(property)]
        fn has_track_list(&self) -> bool {
            false
        }

        #[zbus(property)]
        fn identity(&self) -> &str {
            "Syncverse"
        }

        #[zbus(property)]
        fn supported_uri_schemes(&self) -> Vec<&str> {
            vec!["file"]
        }

        #[zbus(property)]
        fn supported_mime_types(&self) -> Vec<&str> {
            vec!["video/mp4", "video/x-matroska", "video/webm"]
        }
    }

    pub struct MediaPlayer2Player {
        command_tx: Sender<PlayerCommand>,
        state: Arc<Mutex<MprisPlaybackState>>,
    }

    impl MediaPlayer2Player {
        pub fn new(command_tx: Sender<PlayerCommand>, state: Arc<Mutex<MprisPlaybackState>>) -> Self {
            Self { command_tx, state }
        }
    }

    #[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
    impl MediaPlayer2Player {
        fn next(&self) {
            let _ = self.command_tx.send(PlayerCommand::NextChapter);
        }

        fn previous(&self) {
            let _ = self.command_tx.send(PlayerCommand::PreviousChapter);
        }

        fn pause(&self) {
            let is_playing = self.state.lock().map(|s| s.status == "Playing").unwrap_or(false);
            if is_playing {
                let _ = self.command_tx.send(PlayerCommand::TogglePause);
            }
        }

        fn play_pause(&self) {
            let _ = self.command_tx.send(PlayerCommand::TogglePause);
        }

        fn stop(&self) {
            let _ = self.command_tx.send(PlayerCommand::Stop);
        }

        fn play(&self) {
            let is_paused = self.state.lock().map(|s| s.status != "Playing").unwrap_or(true);
            if is_paused {
                let _ = self.command_tx.send(PlayerCommand::TogglePause);
            }
        }

        fn seek(&self, offset_us: i64) {
            let current_ms = self.state.lock().map(|s| s.position_us / 1000).unwrap_or(0);
            let offset_ms = offset_us / 1000;
            let target_ms = (current_ms + offset_ms).max(0) as u64;
            let _ = self.command_tx.send(PlayerCommand::Seek(target_ms));
        }

        fn set_position(&self, _track_id: zbus::zvariant::ObjectPath<'_>, position_us: i64) {
            let target_ms = (position_us / 1000).max(0) as u64;
            let _ = self.command_tx.send(PlayerCommand::Seek(target_ms));
        }

        #[zbus(property)]
        fn playback_status(&self) -> String {
            self.state.lock().map(|s| s.status.clone()).unwrap_or_else(|_| "Stopped".to_string())
        }

        #[zbus(property)]
        fn loop_status(&self) -> &str {
            "None"
        }

        #[zbus(property)]
        fn rate(&self) -> f64 {
            1.0
        }

        #[zbus(property)]
        fn shuffle(&self) -> bool {
            false
        }

        #[zbus(property)]
        fn metadata(&self) -> HashMap<String, Value<'static>> {
            let mut map = HashMap::new();
            if let Ok(st) = self.state.lock() {
                map.insert("xesam:title".to_string(), Value::from(st.title.clone()));
                map.insert("mpris:length".to_string(), Value::from(st.duration_us));
                map.insert("mpris:trackid".to_string(), Value::from("/org/syncverse/current_track"));
            }
            map
        }

        #[zbus(property)]
        fn volume(&self) -> f64 {
            self.state.lock().map(|s| s.volume).unwrap_or(1.0)
        }

        #[zbus(property)]
        fn set_volume(&self, volume: f64) {
            let _ = self.command_tx.send(PlayerCommand::SetVolume(volume as f32));
        }

        #[zbus(property)]
        fn position(&self) -> i64 {
            self.state.lock().map(|s| s.position_us).unwrap_or(0)
        }

        #[zbus(property)]
        fn minimum_rate(&self) -> f64 {
            0.25
        }

        #[zbus(property)]
        fn maximum_rate(&self) -> f64 {
            3.0
        }

        #[zbus(property)]
        fn can_go_next(&self) -> bool {
            true
        }

        #[zbus(property)]
        fn can_go_previous(&self) -> bool {
            true
        }

        #[zbus(property)]
        fn can_play(&self) -> bool {
            true
        }

        #[zbus(property)]
        fn can_pause(&self) -> bool {
            true
        }

        #[zbus(property)]
        fn can_seek(&self) -> bool {
            true
        }

        #[zbus(property)]
        fn can_control(&self) -> bool {
            true
        }
    }
}

/// Manages the MPRIS v2 D-Bus background connection and telemetry updates.
pub struct MprisManager {
    state: Arc<Mutex<MprisPlaybackState>>,
    _handle: Option<std::thread::JoinHandle<()>>,
}

impl MprisManager {
    /// Attempts to register and start the MPRIS v2 D-Bus service on the session bus.
    pub fn start(command_tx: Sender<PlayerCommand>) -> Self {
        let state = Arc::new(Mutex::new(MprisPlaybackState {
            title: "Syncverse".to_string(),
            status: "Stopped".to_string(),
            position_us: 0,
            duration_us: 0,
            volume: 1.0,
        }));

        #[cfg(target_os = "linux")]
        {
            let s_clone = state.clone();
            let handle = std::thread::Builder::new()
                .name("syncverse-mpris".to_string())
                .spawn(move || {
                    let root = linux_mpris::MediaPlayer2Root;
                    let player = linux_mpris::MediaPlayer2Player::new(command_tx, s_clone);

                    let conn_res = zbus::blocking::connection::Builder::session()
                        .and_then(|b| b.name("org.mpris.MediaPlayer2.syncverse"))
                        .and_then(|b| b.serve_at("/org/mpris/MediaPlayer2", root))
                        .and_then(|b| b.serve_at("/org/mpris/MediaPlayer2", player))
                        .and_then(|b| b.build());

                    if let Ok(conn) = conn_res {
                        conn.closed();
                    }
                })
                .ok();

            Self {
                state,
                _handle: handle,
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            let _ = command_tx;
            Self {
                state,
                _handle: None,
            }
        }
    }

    /// Updates current media and playback telemetry exposed on D-Bus.
    pub fn update(
        &self,
        title: &str,
        is_playing: bool,
        position_ms: u64,
        duration_ms: u64,
        volume: f32,
    ) {
        if let Ok(mut st) = self.state.lock() {
            st.title = title.to_string();
            st.status = if is_playing {
                "Playing".to_string()
            } else if position_ms > 0 {
                "Paused".to_string()
            } else {
                "Stopped".to_string()
            };
            st.position_us = (position_ms as i64) * 1000;
            st.duration_us = (duration_ms as i64) * 1000;
            st.volume = volume as f64;
        }
    }
}
