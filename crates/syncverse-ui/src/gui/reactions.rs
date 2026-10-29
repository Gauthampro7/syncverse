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
        // Pseudo-random distribution across horizontal width (15% to 85%)
        let norm_x = 0.15 + (((seed % 1000) as f32) / 1000.0) * 0.70;
        let start_x = rect.left() + rect.width() * norm_x;
        let start_y = rect.bottom() - 40.0;

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

/// Particle simulation engine managing active floating reactions over the video viewport.
#[derive(Debug, Default)]
pub struct ReactionSystem {
    particles: Vec<ReactionParticle>,
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
        self.particles.push(ReactionParticle::new(emoji, username, viewport_rect, seed));
    }

    /// Updates particles, advancing positions and pruning expired reactions.
    pub fn update(&mut self, dt_secs: f32) {
        self.particles.retain_mut(|p| {
            if p.is_expired() {
                return false;
            }
            p.pos.y -= p.velocity_y * dt_secs;
            true
        });
    }

    /// Returns true if there are active particles being animated.
    pub fn has_active_particles(&self) -> bool {
        !self.particles.is_empty()
    }

    /// Renders active reaction particles onto the UI painter.
    pub fn render(&self, ui: &mut Ui, _viewport_rect: Rect) {
        let painter = ui.painter();

        for p in &self.particles {
            let alpha = p.alpha();
            let scale = p.scale();

            let elapsed = p.spawn_time.elapsed().as_secs_f32();
            let wobble_x = (elapsed * p.wobble_freq + p.phase).sin() * p.wobble_amplitude;
            let current_pos = Pos2::new(p.pos.x + wobble_x, p.pos.y);

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
