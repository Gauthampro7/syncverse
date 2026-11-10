//! Unified Audio & Video Effects and Adjustments dialog (Ctrl+E) conforming to VLC extended settings.

use egui::{Align2, Color32, ComboBox, Context, CornerRadius, RichText, Slider, Stroke, Vec2, Window};
use syncverse_codecs::audio::{AudioEqualizerConfig, EqualizerPreset, BAND_FREQUENCIES};
use crate::gui::video_effects::VideoColorAdjustments;

/// Active tab within the Effects & Filters dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EffectsTab {
    /// Audio equalizer, dynamic compressor (night mode), and stereo pan.
    #[default]
    Audio,
    /// Video picture adjustments (brightness, contrast, saturation, gamma).
    Video,
}

/// Action emitted when a user modifies settings in the Effects & Filters dialog.
#[derive(Debug, Clone, PartialEq)]
pub enum EffectsAction {
    /// Equalizer configuration updated.
    AudioEqualizerChanged(AudioEqualizerConfig),
    /// Audio compressor / night mode settings updated.
    AudioCompressorChanged {
        /// Whether compression is enabled.
        enabled: bool,
        /// Threshold in dBFS.
        threshold_db: f32,
        /// Compression ratio.
        ratio: f32,
        /// Makeup gain in dBFS.
        makeup_gain_db: f32,
    },
    /// Audio stereo panning updated [-1.0, 1.0].
    AudioStereoPanChanged(f32),
    /// Video picture color adjustments updated.
    VideoAdjustmentsChanged(VideoColorAdjustments),
}

/// Modal dialog for Audio Equalizer and Video Picture Adjustments.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectsDialog {
    /// Whether the dialog window is open.
    pub is_open: bool,
    /// Currently active tab (Audio vs Video).
    pub active_tab: EffectsTab,
    /// Active equalizer configuration.
    pub equalizer_config: AudioEqualizerConfig,
    /// Currently selected equalizer preset.
    pub selected_preset: EqualizerPreset,
    /// Whether dynamic range compressor (night mode) is enabled.
    pub compressor_enabled: bool,
    /// Compressor threshold in dBFS [-40.0, 0.0].
    pub compressor_threshold_db: f32,
    /// Compressor ratio [1.0, 8.0].
    pub compressor_ratio: f32,
    /// Compressor makeup gain in dBFS [0.0, 12.0].
    pub compressor_makeup_gain_db: f32,
    /// Stereo pan position [-1.0, 1.0].
    pub stereo_pan: f32,
    /// Active video picture adjustments.
    pub video_adjustments: VideoColorAdjustments,
}

impl Default for EffectsDialog {
    fn default() -> Self {
        Self {
            is_open: false,
            active_tab: EffectsTab::Audio,
            equalizer_config: AudioEqualizerConfig::flat(),
            selected_preset: EqualizerPreset::Flat,
            compressor_enabled: false,
            compressor_threshold_db: -16.0,
            compressor_ratio: 3.5,
            compressor_makeup_gain_db: 2.0,
            stereo_pan: 0.0,
            video_adjustments: VideoColorAdjustments::default(),
        }
    }
}

impl EffectsDialog {
    /// Constructs a new `EffectsDialog`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens the dialog to the specified tab.
    pub fn open(&mut self, tab: EffectsTab) {
        self.is_open = true;
        self.active_tab = tab;
    }

    /// Closes the dialog.
    pub fn close(&mut self) {
        self.is_open = false;
    }

    /// Renders the Effects & Filters window dialog and collects emitted actions.
    pub fn show(&mut self, ctx: &Context) -> Vec<EffectsAction> {
        if !self.is_open {
            return Vec::new();
        }

        let mut actions = Vec::new();
        let mut close_dialog = false;

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            close_dialog = true;
        }

        Window::new("🎛 Effects and Adjustments")
            .collapsible(false)
            .resizable(false)
            .pivot(Align2::CENTER_CENTER)
            .fixed_pos(ctx.content_rect().center())
            .fixed_size(Vec2::new(560.0, 420.0))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);

                // Tab Header
                ui.horizontal(|ui| {
                    if ui.selectable_label(self.active_tab == EffectsTab::Audio, "🎵 Audio Effects").clicked() {
                        self.active_tab = EffectsTab::Audio;
                    }
                    if ui.selectable_label(self.active_tab == EffectsTab::Video, "🎬 Video Adjustments").clicked() {
                        self.active_tab = EffectsTab::Video;
                    }
                });

                ui.separator();

                match self.active_tab {
                    EffectsTab::Audio => {
                        // 1. Equalizer Section
                        let mut eq_changed = false;
                        ui.horizontal(|ui| {
                            if ui.checkbox(&mut self.equalizer_config.enabled, "Enable 10-Band Equalizer").changed() {
                                eq_changed = true;
                            }

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("↺ Reset to Flat").clicked() {
                                    self.equalizer_config = AudioEqualizerConfig::flat();
                                    self.selected_preset = EqualizerPreset::Flat;
                                    eq_changed = true;
                                }

                                // Presets Dropdown
                                ComboBox::from_id_salt("eq_presets_dropdown")
                                    .selected_text(self.selected_preset.name())
                                    .show_ui(ui, |ui| {
                                        for preset in EqualizerPreset::ALL {
                                            if ui.selectable_label(self.selected_preset == *preset, preset.name()).clicked() {
                                                self.selected_preset = *preset;
                                                self.equalizer_config = AudioEqualizerConfig::from_preset(*preset);
                                                eq_changed = true;
                                            }
                                        }
                                    });
                                ui.label("Preset:");
                            });
                        });

                        ui.add_space(4.0);

                        // Pre-amp slider
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Pre-amp:").small().color(Color32::from_rgb(180, 180, 195)));
                            let pre_resp = ui.add(
                                Slider::new(&mut self.equalizer_config.pre_amp_db, -20.0..=20.0)
                                    .suffix(" dB")
                                    .text_color(Color32::WHITE),
                            );
                            if pre_resp.changed() {
                                eq_changed = true;
                            }
                        });

                        ui.add_space(6.0);

                        // 10 Vertical Band Sliders
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                for (i, &freq) in BAND_FREQUENCIES.iter().enumerate() {
                                    ui.vertical(|ui| {
                                        let label = if freq >= 1000.0 {
                                            format!("{:.0}k", freq / 1000.0)
                                        } else {
                                            format!("{:.0}", freq)
                                        };
                                        ui.label(RichText::new(label).small().color(Color32::from_rgb(150, 150, 165)));

                                        let slider = ui.add_sized(
                                            [36.0, 130.0],
                                            Slider::new(&mut self.equalizer_config.bands[i], -20.0..=20.0)
                                                .vertical()
                                                .show_value(false),
                                        );
                                        if slider.changed() {
                                            eq_changed = true;
                                        }

                                        let gain_val = self.equalizer_config.bands[i];
                                        let gain_text = format!("{:+.0}", gain_val);
                                        let text_color = if gain_val.abs() > 0.05 {
                                            Color32::from_rgb(56, 189, 248)
                                        } else {
                                            Color32::from_rgb(120, 120, 135)
                                        };
                                        ui.label(RichText::new(gain_text).size(9.0).color(text_color));
                                    });
                                }
                            });
                        });

                        if eq_changed {
                            actions.push(EffectsAction::AudioEqualizerChanged(self.equalizer_config));
                        }

                        ui.add_space(8.0);
                        ui.separator();

                        // 2. Dynamics Compressor (Night Mode) & Stereo Pan
                        ui.horizontal(|ui| {
                            // Compressor
                            ui.vertical(|ui| {
                                let mut comp_changed = false;
                                if ui.checkbox(&mut self.compressor_enabled, "Night Mode (Compressor)").changed() {
                                    comp_changed = true;
                                }
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Threshold:").small());
                                    if ui.add(Slider::new(&mut self.compressor_threshold_db, -40.0..=0.0).suffix(" dB")).changed() {
                                        comp_changed = true;
                                    }
                                });
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Ratio:").small());
                                    if ui.add(Slider::new(&mut self.compressor_ratio, 1.0..=8.0).suffix(":1")).changed() {
                                        comp_changed = true;
                                    }
                                });
                                if comp_changed {
                                    actions.push(EffectsAction::AudioCompressorChanged {
                                        enabled: self.compressor_enabled,
                                        threshold_db: self.compressor_threshold_db,
                                        ratio: self.compressor_ratio,
                                        makeup_gain_db: self.compressor_makeup_gain_db,
                                    });
                                }
                            });

                            ui.separator();

                            // Stereo Balance Pan
                            ui.vertical(|ui| {
                                ui.label(RichText::new("Stereo Balance").strong());
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("L").small().color(Color32::from_rgb(150, 150, 165)));
                                    if ui.add(Slider::new(&mut self.stereo_pan, -1.0..=1.0).show_value(false)).changed() {
                                        actions.push(EffectsAction::AudioStereoPanChanged(self.stereo_pan));
                                    }
                                    ui.label(RichText::new("R").small().color(Color32::from_rgb(150, 150, 165)));
                                });
                                let pan_label = if self.stereo_pan < -0.05 {
                                    format!("Left {:.0}%", -self.stereo_pan * 100.0)
                                } else if self.stereo_pan > 0.05 {
                                    format!("Right {:.0}%", self.stereo_pan * 100.0)
                                } else {
                                    "Center".to_string()
                                };
                                ui.label(RichText::new(pan_label).small().color(Color32::from_rgb(180, 180, 195)));
                            });
                        });
                    }

                    EffectsTab::Video => {
                        let mut vid_changed = false;
                        ui.horizontal(|ui| {
                            if ui.checkbox(&mut self.video_adjustments.enabled, "Enable Video Adjustments").changed() {
                                vid_changed = true;
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("↺ Reset to Defaults").clicked() {
                                    self.video_adjustments.reset();
                                    vid_changed = true;
                                }
                            });
                        });

                        ui.add_space(8.0);

                        ui.group(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(8.0, 12.0);

                            // Brightness slider
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Brightness:").strong().color(Color32::WHITE));
                                let resp = ui.add(
                                    Slider::new(&mut self.video_adjustments.brightness, -1.0..=1.0)
                                        .step_by(0.01)
                                        .text_color(Color32::WHITE),
                                );
                                if resp.changed() {
                                    vid_changed = true;
                                }
                            });

                            // Contrast slider
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Contrast:").strong().color(Color32::WHITE));
                                let resp = ui.add(
                                    Slider::new(&mut self.video_adjustments.contrast, 0.0..=2.0)
                                        .step_by(0.01)
                                        .text_color(Color32::WHITE),
                                );
                                if resp.changed() {
                                    vid_changed = true;
                                }
                            });

                            // Saturation slider
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Saturation:").strong().color(Color32::WHITE));
                                let resp = ui.add(
                                    Slider::new(&mut self.video_adjustments.saturation, 0.0..=3.0)
                                        .step_by(0.01)
                                        .text_color(Color32::WHITE),
                                );
                                if resp.changed() {
                                    vid_changed = true;
                                }
                            });

                            // Gamma slider
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Gamma:").strong().color(Color32::WHITE));
                                let resp = ui.add(
                                    Slider::new(&mut self.video_adjustments.gamma, 0.2..=2.5)
                                        .step_by(0.01)
                                        .text_color(Color32::WHITE),
                                );
                                if resp.changed() {
                                    vid_changed = true;
                                }
                            });
                        });

                        if vid_changed {
                            actions.push(EffectsAction::VideoAdjustmentsChanged(self.video_adjustments));
                        }
                    }
                }

                ui.add_space(8.0);
                ui.separator();

                // Close Button
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(
                        egui::Button::new(RichText::new("Close").color(Color32::WHITE))
                            .stroke(Stroke::new(1.0, Color32::from_rgb(75, 85, 99)))
                            .corner_radius(CornerRadius::same(6)),
                    ).clicked() {
                        close_dialog = true;
                    }
                });
            });

        if close_dialog {
            self.close();
        }

        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_effects_dialog_lifecycle_and_reset() {
        let mut dlg = EffectsDialog::new();
        assert!(!dlg.is_open);
        assert_eq!(dlg.active_tab, EffectsTab::Audio);

        dlg.open(EffectsTab::Video);
        assert!(dlg.is_open);
        assert_eq!(dlg.active_tab, EffectsTab::Video);

        dlg.video_adjustments.enabled = true;
        dlg.video_adjustments.brightness = 0.5;
        assert!(!dlg.video_adjustments.is_identity());

        dlg.video_adjustments.reset();
        assert!(dlg.video_adjustments.is_identity());

        dlg.close();
        assert!(!dlg.is_open);
    }

    #[test]
    fn test_effects_dialog_equalizer_presets() {
        let mut dlg = EffectsDialog::new();
        dlg.selected_preset = EqualizerPreset::Rock;
        dlg.equalizer_config = AudioEqualizerConfig::from_preset(EqualizerPreset::Rock);

        assert!(dlg.equalizer_config.enabled);
        assert_eq!(dlg.equalizer_config.bands, EqualizerPreset::Rock.band_gains());
    }
}
