//! Floating emoji particle reactions for Watch-Party 2.0.
//!
//! Provides a lightweight, GPU-efficient floating particle system rendered directly on top
//! of the video viewport, visualizing real-time emotional reactions (❤️, 😂, 🔥, 👏, 🍿, 🎉)
//! synchronized across room peers.

use std::time::Instant;
use egui::{Color32, FontId, Pos2, Rect, Ui};

/// Standard watch-party reaction emoji options.
pub const REACTION_PRESETS: &[(&str, &str)] = &[
    ("❤️", "Love"),
    ("😂", "Laugh"),
    ("🔥", "Fire"),
    ("👏", "Clap"),
    ("🍿", "Popcorn"),
    ("🎉", "Celebrate"),
];

/// A single floating emoji reaction particle.
#[derive(Debug, Clone)]
pub struct ReactionParticle {
    /// Emoji string (e.g. "❤️", "🔥", "😂").
    pub emoji: String,
    /// Sender username, if provided.
    pub username: Option<String>,
    /// Screen-space position.
    pub pos: Pos2,
    /// Upward velocity in pixels/second.
    pub velocity_y: f32,
    /// Horizontal oscillation amplitude in pixels.
    pub wobble_amplitude: f32,
    /// Horizontal oscillation frequency.
    pub wobble_freq: f32,
    /// Spawn instant.
    pub spawn_time: Instant,
    /// Total lifetime in seconds (e.g. 2.5s).
    pub max_lifetime_secs: f32,
    /// Random phase offset for horizontal wobble.
    pub phase: f32,
}

impl ReactionParticle {
    /// Creates a new reaction particle spawned near the bottom of `rect`.
    pub fn new(emoji: impl Into<String>, username: Option<String>, rect: Rect, seed: u64) -> Self {
        let norm_x = ((seed % 1000) as f32) / 1000.0;
        let (start_x, start_y) = if rect.is_finite() && rect.width() > 1.0 && rect.height() > 1.0 {
            let pad = (rect.width() * 0.08)
                .clamp(12.0, 32.0)
                .min(rect.width() * 0.45);
            let usable_w = (rect.width() - 2.0 * pad).max(0.0);
            let x = rect.left() + pad + usable_w * norm_x;
            let y = (rect.bottom() - 16.0).max(rect.top());
            (x, y)
        } else {
            (rect.center().x, rect.center().y)
        };

        let velocity_y = 120.0 + (((seed / 1000) % 50) as f32); // 120 to 170 px/s
        let wobble_amplitude = 15.0 + (((seed / 50000) % 20) as f32);
        let wobble_freq = 2.0 + (((seed / 100000) % 15) as f32) * 0.1;
        let phase = ((seed % 628) as f32) / 100.0;

        Self {
            emoji: emoji.into(),
            username,
            pos: Pos2::new(start_x, start_y),
            velocity_y,
            wobble_amplitude,
            wobble_freq,
            spawn_time: Instant::now(),
            max_lifetime_secs: 2.8,
            phase,
        }
    }

    /// Evaluates current progress in range [0.0, 1.0].
    pub fn progress(&self) -> f32 {
        let elapsed = self.spawn_time.elapsed().as_secs_f32();
        (elapsed / self.max_lifetime_secs).clamp(0.0, 1.0)
    }

    /// Returns true if the particle has completed its animation lifetime.
    pub fn is_expired(&self) -> bool {
        self.spawn_time.elapsed().as_secs_f32() >= self.max_lifetime_secs
    }

    /// Calculates current alpha opacity in range [0.0, 1.0].
    pub fn alpha(&self) -> f32 {
        let p = self.progress();
        if p < 0.15 {
            // Rapid pop-in / fade-in
            p / 0.15
        } else if p > 0.65 {
            // Smooth fade-out
            1.0 - ((p - 0.65) / 0.35)
        } else {
            1.0
        }
    }

    /// Calculates current scale multiplier (e.g. pop-in from 0.6 to 1.2 then settle to 1.0).
    pub fn scale(&self) -> f32 {
        let p = self.progress();
        if p < 0.12 {
            0.6 + (p / 0.12) * 0.6 // 0.6 -> 1.2
        } else if p < 0.25 {
            1.2 - ((p - 0.12) / 0.13) * 0.2 // 1.2 -> 1.0
        } else {
            1.0
        }
    }
}

/// Internal particle wrapper tracking normalized horizontal anchor and upward travel distance
/// so particles always stay mapped within the active `viewport_rect` across window resizes.
#[derive(Debug, Clone)]
struct ActiveParticle {
    particle: ReactionParticle,
    norm_x: f32,
    y_traveled: f32,
}

/// Particle simulation engine managing active floating reactions over the video viewport.
#[derive(Debug, Default)]
pub struct ReactionSystem {
    particles: Vec<ActiveParticle>,
    seed_counter: u64,
}

impl ReactionSystem {
    /// Creates a new `ReactionSystem`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Spawns a new floating reaction particle within the viewport rect.
    pub fn spawn(&mut self, emoji: impl Into<String>, username: Option<String>, viewport_rect: Rect) {
        self.seed_counter = self.seed_counter.wrapping_add(1337);
        let time_salt = Instant::now().elapsed().as_nanos() as u64;
        let seed = self.seed_counter ^ time_salt;
        let norm_x = ((seed % 1000) as f32) / 1000.0;
        let particle = ReactionParticle::new(emoji, username, viewport_rect, seed);
        self.particles.push(ActiveParticle {
            particle,
            norm_x,
            y_traveled: 0.0,
        });
    }

    /// Updates particles, advancing positions and pruning expired reactions.
    pub fn update(&mut self, dt_secs: f32) {
        let dt = dt_secs.max(0.0);
        self.particles.retain_mut(|entry| {
            if entry.particle.is_expired() {
                return false;
            }
            let dy = entry.particle.velocity_y * dt;
            entry.particle.pos.y -= dy;
            entry.y_traveled += dy;
            true
        });
    }

    /// Returns true if there are active particles being animated.
    pub fn has_active_particles(&self) -> bool {
        !self.particles.is_empty()
    }

    /// Renders active reaction particles clipped strictly to `viewport_rect`.
    pub fn render(&self, ui: &mut Ui, viewport_rect: Rect) {
        if !viewport_rect.is_finite()
            || viewport_rect.width() <= 1.0
            || viewport_rect.height() <= 1.0
        {
            return;
        }

        let painter = ui.painter_at(viewport_rect);
        let pad_x = (viewport_rect.width() * 0.08)
            .clamp(12.0, 32.0)
            .min(viewport_rect.width() * 0.45);
        let min_x = viewport_rect.left() + pad_x;
        let max_x = (viewport_rect.right() - pad_x).max(min_x);
        let start_y = (viewport_rect.bottom() - 16.0).max(viewport_rect.top());

        for entry in &self.particles {
            let p = &entry.particle;
            let alpha = p.alpha();
            let scale = p.scale();

            let elapsed = p.spawn_time.elapsed().as_secs_f32();
            let wobble_x = (elapsed * p.wobble_freq + p.phase).sin() * p.wobble_amplitude;
            let base_x = min_x + (max_x - min_x) * entry.norm_x;
            let current_x = (base_x + wobble_x).clamp(min_x, max_x);
            let current_y = start_y - entry.y_traveled;

            if current_y < viewport_rect.top() - 48.0 {
                continue;
            }

            let current_pos = Pos2::new(current_x, current_y);
            let font_size = 28.0 * scale;
            let alpha_u8 = (alpha * 255.0) as u8;

            // Render subtle shadow for visual pop
            let shadow_color = Color32::from_rgba_unmultiplied(0, 0, 0, alpha_u8 / 2);
            painter.text(
                Pos2::new(current_pos.x + 1.5, current_pos.y + 1.5),
                egui::Align2::CENTER_CENTER,
                &p.emoji,
                FontId::proportional(font_size),
                shadow_color,
            );

            // Render primary emoji
            let emoji_color = Color32::from_rgba_unmultiplied(255, 255, 255, alpha_u8);
            painter.text(
                current_pos,
                egui::Align2::CENTER_CENTER,
                &p.emoji,
                FontId::proportional(font_size),
                emoji_color,
            );

            // If username is attached, render compact name badge above the emoji
            if let Some(ref uname) = p.username {
                let badge_alpha_u8 = (alpha * 200.0) as u8;
                let badge_color = Color32::from_rgba_unmultiplied(220, 220, 240, badge_alpha_u8);
                painter.text(
                    Pos2::new(current_pos.x, current_pos.y - font_size * 0.7),
                    egui::Align2::CENTER_CENTER,
                    uname,
                    FontId::proportional(11.0),
                    badge_color,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2};

    fn run_reaction_frame(
        system: &ReactionSystem,
        screen_size: egui::Vec2,
        viewport_rect: Rect,
    ) -> egui::FullOutput {
        let ctx = egui::Context::default();
        let raw_input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, screen_size)),
            ..Default::default()
        };
        let mut output = ctx.run_ui(raw_input, |ui| {
            system.render(ui, viewport_rect);
        });
        output.textures_delta.clear();
        output
    }

    #[test]
    fn test_reaction_system_clips_strictly_to_viewport_rect() {
        let mut system = ReactionSystem::new();
        let viewport_rect = Rect::from_min_size(pos2(200.0, 80.0), vec2(640.0, 360.0));

        for (emoji, _) in REACTION_PRESETS {
            system.spawn(*emoji, Some("Alice".to_string()), viewport_rect);
        }
        assert!(system.has_active_particles());

        // Advance simulation so particles have non-zero alpha and upward travel
        system.update(0.35);

        let output = run_reaction_frame(&system, vec2(1280.0, 720.0), viewport_rect);
        assert!(
            !output.shapes.is_empty(),
            "active particles inside valid viewport_rect should emit shapes"
        );

        for clipped in &output.shapes {
            assert!(
                viewport_rect.contains_rect(clipped.clip_rect),
                "shape clip_rect {:?} escaped viewport_rect {:?}",
                clipped.clip_rect,
                viewport_rect
            );
        }
    }

    #[test]
    fn test_reaction_system_handles_degenerate_viewport_rects() {
        let mut system = ReactionSystem::new();
        let valid_rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(640.0, 360.0));
        system.spawn("❤️", Some("Bob".to_string()), valid_rect);
        system.update(0.2);

        let degenerate_rects = [
            Rect::ZERO,
            Rect::from_min_size(pos2(10.0, 10.0), vec2(0.0, 0.0)),
            Rect::from_min_size(pos2(10.0, 10.0), vec2(1.0, 100.0)),
            Rect::from_min_size(pos2(10.0, 10.0), vec2(100.0, 0.5)),
            Rect::from_min_max(pos2(50.0, 50.0), pos2(10.0, 10.0)),
            Rect::NOTHING,
        ];

        for bad_rect in degenerate_rects {
            let output = run_reaction_frame(&system, vec2(1280.0, 720.0), bad_rect);
            assert!(
                output.shapes.is_empty(),
                "degenerate viewport_rect {:?} must emit zero shapes",
                bad_rect
            );
        }
    }

    #[test]
    fn test_reaction_system_remaps_particles_across_viewport_resize() {
        let mut system = ReactionSystem::new();
        let initial_rect = Rect::from_min_size(pos2(500.0, 300.0), vec2(600.0, 400.0));
        system.spawn("🔥", Some("Charlie".to_string()), initial_rect);
        system.update(0.3);

        // Render into a completely different / smaller viewport rect after resize
        let shrunk_rect = Rect::from_min_size(pos2(20.0, 40.0), vec2(220.0, 160.0));
        let output = run_reaction_frame(&system, vec2(480.0, 320.0), shrunk_rect);
        assert!(!output.shapes.is_empty());

        for clipped in &output.shapes {
            assert!(
                shrunk_rect.contains_rect(clipped.clip_rect),
                "remapped shape clip_rect {:?} escaped shrunk_rect {:?}",
                clipped.clip_rect,
                shrunk_rect
            );
        }
    }
}
