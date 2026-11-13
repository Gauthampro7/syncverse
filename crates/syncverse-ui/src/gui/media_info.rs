//! Technical media container info, video/audio codec statistics, and clock synchronization diagnostics dialog.

use std::path::PathBuf;
use egui::{Align2, Color32, Context, RichText, Vec2};
use crate::gui::aspect_ratio::{AspectRatioOverride, ZoomMode};
use crate::gui::dialogs::{AudioTrackItem, VideoStreamMetadata};
use crate::snapshot::PlayerUiSnapshot;

/// Formats a byte size into human-readable representation (B, KB, MB, GB).
pub fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB ({} bytes)", bytes as f64 / GB as f64, bytes)
    } else if bytes >= MB {
        format!("{:.2} MB ({} bytes)", bytes as f64 / MB as f64, bytes)
    } else if bytes >= KB {
        format!("{:.1} KB ({} bytes)", bytes as f64 / KB as f64, bytes)
    } else {
        format!("{} bytes", bytes)
    }
}

/// Contextual playback, audio, video, and sync telemetry state passed to `MediaInfoDialog`.
#[derive(Debug)]
pub struct MediaInfoContext<'a> {
    /// Currently loaded media file path, if any.
    pub current_file: Option<&'a PathBuf>,
    /// Video stream technical metadata (codec, resolution, fps).
    pub video_meta: &'a VideoStreamMetadata,
    /// Container audio stream tracks.
    pub audio_tracks: &'a [AudioTrackItem],
    /// 0-indexed selected audio track, if available.
    pub active_audio_track: Option<usize>,
    /// Dynamic audio synchronization delay in milliseconds.
    pub audio_delay_ms: i64,
    /// Dynamic subtitle synchronization delay in milliseconds.
    pub subtitle_delay_ms: i64,
    /// Active aspect ratio override preset.
    pub aspect_override: AspectRatioOverride,
    /// Active video zoom mode.
    pub zoom_mode: ZoomMode,
    /// Effective audio volume [0.0, 2.0].
    pub volume: f32,
    /// Latest atomic UI telemetry snapshot.
    pub snapshot: &'a PlayerUiSnapshot,
}

/// Modal dialog displaying technical container metadata, video/audio codec properties,
/// and live clock synchronization telemetry diagnostics.
#[derive(Debug, Clone, Default)]
pub struct MediaInfoDialog {
    /// Whether the dialog window is open.
    pub is_open: bool,
    /// Transient status message (e.g. "Diagnostics copied to clipboard!").
    pub status_message: Option<String>,
    /// Cached path for avoiding per-frame `std::fs::metadata` syscalls.
    cached_file_path: Option<PathBuf>,
    /// Cached file size in bytes corresponding to `cached_file_path`.
    cached_file_size: Option<u64>,
}

impl MediaInfoDialog {
    /// Constructs a new `MediaInfoDialog`.
    pub fn new() -> Self {
        Self {
            is_open: false,
            status_message: None,
            cached_file_path: None,
            cached_file_size: None,
        }
    }

    /// Opens the media info dialog window.
    pub fn open(&mut self) {
        self.is_open = true;
        self.status_message = None;
    }

    /// Closes the media info dialog window.
    pub fn close(&mut self) {
        self.is_open = false;
    }

    /// Toggles open/close state.
    pub fn toggle(&mut self) {
        self.is_open = !self.is_open;
    }

    /// Renders the Media Information & Codec Statistics window if open.
    pub fn show(
        &mut self,
        ctx: &Context,
        info: &MediaInfoContext<'_>,
    ) {
        if !self.is_open {
            return;
        }

        let current_file = info.current_file;
        let video_meta = info.video_meta;
        let audio_tracks = info.audio_tracks;
        let active_audio_track = info.active_audio_track;
        let audio_delay_ms = info.audio_delay_ms;
        let subtitle_delay_ms = info.subtitle_delay_ms;
        let aspect_override = info.aspect_override;
        let zoom_mode = info.zoom_mode;
        let volume = info.volume;
        let snapshot = info.snapshot;

        // Refresh cached file size only when `current_file` changes
        let path_changed = match (current_file, self.cached_file_path.as_ref()) {
            (Some(p), Some(cached)) => p != cached,
            (None, None) => false,
            _ => true,
        };
        if path_changed {
            self.cached_file_path = current_file.cloned();
            self.cached_file_size = current_file
                .and_then(|p| std::fs::metadata(p).ok())
                .map(|m| m.len());
        }

        let mut is_open = self.is_open;
        let screen = ctx.content_rect();
        let max_size = (screen.size() - Vec2::new(24.0, 24.0)).max(Vec2::new(240.0, 160.0));
        let default_size = Vec2::new(540.0, 480.0).min(max_size);

        egui::Window::new("ℹ️ Media Information & Codec Statistics")
            .open(&mut is_open)
            .collapsible(false)
            .resizable(true)
            .vscroll(true)
            .default_size(default_size)
            .max_size(max_size)
            .constrain_to(screen)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new("Comprehensive technical stream properties and synchronization metrics.")
                            .italics()
                            .color(Color32::from_rgb(156, 163, 175)),
                    );

                    if ui.button("📋 Copy Diagnostics").clicked() {
                        let text = Self::build_diagnostics_report(info);
                        ctx.copy_text(text);
                        self.status_message = Some("Copied to clipboard!".to_string());
                    }
                    if ui.button("🔄 Refresh").clicked() {
                        self.cached_file_path = current_file.cloned();
                        self.cached_file_size = current_file
                            .and_then(|p| std::fs::metadata(p).ok())
                            .map(|m| m.len());
                        self.status_message = Some("Refreshed statistics".to_string());
                    }
                });

                if let Some(ref msg) = self.status_message {
                    ui.label(RichText::new(msg).color(Color32::from_rgb(74, 222, 128)).strong());
                }

                ui.separator();

                // 1. General Container & File Info
                ui.heading("General Container Information");
                egui::Grid::new("media_info_general_grid")
                    .num_columns(2)
                    .spacing([16.0, 6.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label(RichText::new("File Path:").strong());
                        let path_str = current_file
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_else(|| "No media loaded".to_string());
                        ui.add(egui::Label::new(&path_str).truncate())
                            .on_hover_text(&path_str);
                        ui.end_row();

                        if let Some(file_size) = self.cached_file_size {
                            ui.label(RichText::new("File Size:").strong());
                            ui.label(format_file_size(file_size));
                            ui.end_row();
                        }

                        let dur_ms = snapshot.media_duration_pts / 1_000_000;
                        ui.label(RichText::new("Duration:").strong());
                        ui.label(crate::gui::controls::ControlsBarState::format_timestamp(dur_ms));
                        ui.end_row();

                        let pos_ms = snapshot.media_position_pts / 1_000_000;
                        ui.label(RichText::new("Current PTS:").strong());
                        ui.label(format!("{} ({} ms)", crate::gui::controls::ControlsBarState::format_timestamp(pos_ms), pos_ms));
                        ui.end_row();
                    });

                ui.add_space(8.0);
                ui.separator();

                // 2. Video Stream Details
                ui.heading("Video Stream Details");
                egui::Grid::new("media_info_video_grid")
                    .num_columns(2)
                    .spacing([16.0, 6.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label(RichText::new("Codec & Format:").strong());
                        ui.label(video_meta.codec_str());
                        ui.end_row();

                        if let Some(ref pf) = video_meta.pixel_format {
                            ui.label(RichText::new("Pixel Format:").strong());
                            ui.label(pf);
                            ui.end_row();
                        }

                        if let Some(bd) = video_meta.bit_depth {
                            ui.label(RichText::new("Bit Depth:").strong());
                            ui.label(format!("{}-bit", bd));
                            ui.end_row();
                        }

                        ui.label(RichText::new("Resolution:").strong());
                        ui.label(video_meta.resolution_str());
                        ui.end_row();

                        ui.label(RichText::new("Aspect Ratio:").strong());
                        ui.label(format!("{} (Override: {})", video_meta.aspect_ratio_str(), aspect_override.display_name()));
                        ui.end_row();

                        ui.label(RichText::new("Zoom Mode:").strong());
                        ui.label(zoom_mode.display_name());
                        ui.end_row();

                        ui.label(RichText::new("Frame Rate:").strong());
                        ui.label(video_meta.frame_rate_str());
                        ui.end_row();
                    });

                ui.add_space(8.0);
                ui.separator();

                // 3. Audio Stream Details
                ui.heading("Audio Stream Details");
                egui::Grid::new("media_info_audio_grid")
                    .num_columns(2)
                    .spacing([16.0, 6.0])
                    .striped(true)
                    .show(ui, |ui| {
                        let track_desc = if let Some(idx) = active_audio_track {
                            if idx < audio_tracks.len() {
                                let t = &audio_tracks[idx];
                                format!("Track {}/{} — {} ({})", idx + 1, audio_tracks.len(), t.title, t.codec)
                            } else {
                                "Default".to_string()
                            }
                        } else {
                            "None".to_string()
                        };

                        ui.label(RichText::new("Active Track:").strong());
                        ui.add(egui::Label::new(&track_desc).truncate())
                            .on_hover_text(&track_desc);
                        ui.end_row();

                        if let Some(idx) = active_audio_track {
                            if idx < audio_tracks.len() {
                                let t = &audio_tracks[idx];
                                if let Some(sr) = t.sample_rate {
                                    ui.label(RichText::new("Sample Rate:").strong());
                                    ui.label(format!("{:.0} Hz", sr));
                                    ui.end_row();
                                }
                                if let Some(ch) = t.channels {
                                    ui.label(RichText::new("Channels:").strong());
                                    let ch_str = match ch {
                                        1 => "1.0 Mono".to_string(),
                                        2 => "2.0 Stereo".to_string(),
                                        6 => "5.1 Surround".to_string(),
                                        8 => "7.1 Surround".to_string(),
                                        n => format!("{} channels", n),
                                    };
                                    ui.label(ch_str);
                                    ui.end_row();
                                }
                            }
                        }

                        ui.label(RichText::new("Audio Delay:").strong());
                        ui.label(format!("{:+} ms", audio_delay_ms));
                        ui.end_row();

                        ui.label(RichText::new("Subtitle Delay:").strong());
                        ui.label(format!("{:+} ms", subtitle_delay_ms));
                        ui.end_row();

                        ui.label(RichText::new("Volume Gain:").strong());
                        let vol_pct = (volume * 100.0).round() as u32;
                        let boost_str = if volume > 1.0 { " [Over-Amplification Boost]" } else { "" };
                        ui.label(format!("{}%{}", vol_pct, boost_str));
                        ui.end_row();
                    });

                ui.add_space(8.0);
                ui.separator();

                // 4. Live Synchronization & Telemetry Diagnostics
                ui.heading("Synchronizer & Network Telemetry");
                egui::Grid::new("media_info_sync_grid")
                    .num_columns(2)
                    .spacing([16.0, 6.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label(RichText::new("Network RTT:").strong());
                        let rtt_ms = snapshot.network_rtt_nanos as f64 / 1_000_000.0;
                        ui.label(format!("{:.2} ms", rtt_ms));
                        ui.end_row();

                        ui.label(RichText::new("Clock Offset (θ):").strong());
                        let offset_ms = snapshot.clock_offset_nanos as f64 / 1_000_000.0;
                        ui.label(format!("{:+.2} ms", offset_ms));
                        ui.end_row();

                        ui.label(RichText::new("PLL Phase Error:").strong());
                        let phase_ms = snapshot.phase_error_nanos as f64 / 1_000_000.0;
                        ui.label(format!("{:+.2} ms", phase_ms));
                        ui.end_row();

                        ui.label(RichText::new("Commanded Speed:").strong());
                        let speed = snapshot.pll_speed_ratio_q16 as f64 / 65536.0;
                        ui.label(format!("{:.4}x", speed));
                        ui.end_row();

                        ui.label(RichText::new("Room Members:").strong());
                        ui.label(format!("{} peer(s) ({} ready)", snapshot.peer_count, snapshot.peers_ready));
                        ui.end_row();
                    });
            });

        self.is_open = is_open;
    }

    /// Builds a comprehensive markdown formatted diagnostics report for clipboard export.
    pub fn build_diagnostics_report(info: &MediaInfoContext<'_>) -> String {
        let current_file = info.current_file;
        let video_meta = info.video_meta;
        let audio_tracks = info.audio_tracks;
        let active_audio_track = info.active_audio_track;
        let audio_delay_ms = info.audio_delay_ms;
        let subtitle_delay_ms = info.subtitle_delay_ms;
        let aspect_override = info.aspect_override;
        let zoom_mode = info.zoom_mode;
        let volume = info.volume;
        let snapshot = info.snapshot;

        let mut report = String::new();
        report.push_str("### Syncverse Media & Diagnostics Report\n\n");

        report.push_str("**General Information:**\n");
        let path_str = current_file
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "None".to_string());
        report.push_str(&format!("- **File:** {}\n", path_str));
        if let Some(p) = current_file {
            if let Ok(meta) = std::fs::metadata(p) {
                report.push_str(&format!("- **File Size:** {}\n", format_file_size(meta.len())));
            }
        }
        let dur_ms = snapshot.media_duration_pts / 1_000_000;
        let pos_ms = snapshot.media_position_pts / 1_000_000;
        report.push_str(&format!("- **Position:** {}\n", crate::overlay::format_timecode_hud(pos_ms, dur_ms)));

        report.push_str("\n**Video Stream:**\n");
        report.push_str(&format!("- **Codec:** {}\n", video_meta.codec_str()));
        if let Some(ref pf) = video_meta.pixel_format {
            report.push_str(&format!("- **Pixel Format:** {}\n", pf));
        }
        if let Some(bd) = video_meta.bit_depth {
            report.push_str(&format!("- **Bit Depth:** {}-bit\n", bd));
        }
        report.push_str(&format!("- **Resolution:** {}\n", video_meta.resolution_str()));
        report.push_str(&format!("- **Aspect Ratio:** {} (Override: {})\n", video_meta.aspect_ratio_str(), aspect_override.display_name()));
        report.push_str(&format!("- **Zoom Mode:** {}\n", zoom_mode.display_name()));
        report.push_str(&format!("- **Frame Rate:** {}\n", video_meta.frame_rate_str()));

        report.push_str("\n**Audio Stream:**\n");
        let track_str = if let Some(idx) = active_audio_track {
            if idx < audio_tracks.len() {
                let t = &audio_tracks[idx];
                format!("{} ({})", t.title, t.codec)
            } else {
                "Default".to_string()
            }
        } else {
            "None".to_string()
        };
        report.push_str(&format!("- **Active Track:** {}\n", track_str));
        if let Some(idx) = active_audio_track {
            if idx < audio_tracks.len() {
                let t = &audio_tracks[idx];
                if let Some(sr) = t.sample_rate {
                    report.push_str(&format!("- **Sample Rate:** {:.0} Hz\n", sr));
                }
                if let Some(ch) = t.channels {
                    report.push_str(&format!("- **Channels:** {}\n", ch));
                }
            }
        }
        report.push_str(&format!("- **Audio Delay:** {:+} ms\n", audio_delay_ms));
        report.push_str(&format!("- **Subtitle Delay:** {:+} ms\n", subtitle_delay_ms));
        let vol_pct = (volume * 100.0).round() as u32;
        let boost_str = if volume > 1.0 { " [Boost]" } else { "" };
        report.push_str(&format!("- **Volume:** {}%{}\n", vol_pct, boost_str));

        report.push_str("\n**Synchronization Diagnostics:**\n");
        report.push_str(&format!("- **RTT:** {:.2} ms\n", snapshot.network_rtt_nanos as f64 / 1_000_000.0));
        report.push_str(&format!("- **Clock Offset:** {:+.2} ms\n", snapshot.clock_offset_nanos as f64 / 1_000_000.0));
        report.push_str(&format!("- **PLL Phase Error:** {:+.2} ms\n", snapshot.phase_error_nanos as f64 / 1_000_000.0));
        report.push_str(&format!("- **PLL Speed:** {:.4}x\n", snapshot.pll_speed_ratio_q16 as f64 / 65536.0));
        report.push_str(&format!("- **Peers:** {} (Ready: {})\n", snapshot.peer_count, snapshot.peers_ready));

        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_file_size() {
        assert_eq!(format_file_size(500), "500 bytes");
        assert_eq!(format_file_size(2048), "2.0 KB (2048 bytes)");
        assert_eq!(format_file_size(10 * 1024 * 1024), "10.00 MB (10485760 bytes)");
        assert_eq!(format_file_size(2 * 1024 * 1024 * 1024), "2.00 GB (2147483648 bytes)");
    }

    #[test]
    fn test_media_info_dialog_state() {
        let mut dlg = MediaInfoDialog::new();
        assert!(!dlg.is_open);
        dlg.open();
        assert!(dlg.is_open);
        dlg.close();
        assert!(!dlg.is_open);
        dlg.toggle();
        assert!(dlg.is_open);

        dlg.status_message = Some("Refreshed statistics".to_string());
        assert_eq!(dlg.status_message.as_deref(), Some("Refreshed statistics"));
        dlg.open();
        assert_eq!(dlg.status_message, None);
    }

    #[test]
    fn test_media_info_dialog_caches_metadata_and_clamps_viewport() {
        let dir = std::env::temp_dir().join(format!("syncverse_mediainfo_cache_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let test_file = dir.join("very_deep_nested_long_filename_for_media_info_clamping_test.mkv");
        std::fs::write(&test_file, vec![0u8; 4096]).unwrap();

        let meta = VideoStreamMetadata::default();
        let snap = PlayerUiSnapshot::default();
        let info_ctx = MediaInfoContext {
            current_file: Some(&test_file),
            video_meta: &meta,
            audio_tracks: &[],
            active_audio_track: None,
            audio_delay_ms: 0,
            subtitle_delay_ms: 0,
            aspect_override: AspectRatioOverride::Original,
            zoom_mode: ZoomMode::Fit,
            volume: 1.0,
            snapshot: &snap,
        };

        let mut dlg = MediaInfoDialog::new();
        dlg.open();

        let ctx = Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(480.0, 320.0));
        let raw = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };

        // First render populates cache
        let mut out = ctx.run_ui(raw.clone(), |ctx| {
            dlg.show(ctx, &info_ctx);
        });
        out.textures_delta.clear();
        assert_eq!(dlg.cached_file_path.as_ref(), Some(&test_file));
        assert_eq!(dlg.cached_file_size, Some(4096));

        // Modify underlying file size on disk: second render must reuse cache without stat syscall
        std::fs::write(&test_file, vec![0u8; 8192]).unwrap();
        let mut out = ctx.run_ui(raw, |ctx| {
            dlg.show(ctx, &info_ctx);
        });
        out.textures_delta.clear();
        assert_eq!(dlg.cached_file_size, Some(4096));

        let rect = ctx
            .memory(|m| {
                m.area_rect(egui::Id::unique(Some(
                    "ℹ️ Media Information & Codec Statistics",
                )))
            })
            .expect("MediaInfoDialog area_rect must exist");
        assert!(
            screen.contains_rect(rect),
            "MediaInfoDialog {:?} escaped 480x320 {:?}",
            rect,
            screen
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
