//! Terminal screen rendering, box-drawing scrubber bars, and ANSI half-block video canvas.

use std::fmt::Write;

/// Visual rendering style for the scrubber progress bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrubberStyle {
    /// Classic heavy line with glowing bullet seekhead: `[━━━━●───────]`
    Classic,
    /// Detailed box-drawing style utilizing `━`, `╺`, `─`, and `●`
    BoxDrawing,
    /// High-contrast block bar using `█`, `●`, and `─`
    Block,
}

/// Renders a visual scrubber progress bar with custom box-drawing characters.
///
/// Supports characters:
/// - `━` (`U+2501`): Heavy horizontal line
/// - `╺` (`U+257A`): Heavy left terminal segment
/// - `─` (`U+2500`): Light horizontal line
/// - `█` (`U+2588`): Full solid block
/// - `●` (`U+25CF`): Black circle seekhead
pub fn render_styled_scrubber_bar(
    current_ms: u64,
    duration_ms: u64,
    bar_width: usize,
    style: ScrubberStyle,
) -> String {
    let bar_width = bar_width.max(10);
    let inner_width = bar_width.saturating_sub(2);

    let progress = if duration_ms > 0 {
        (current_ms as f64 / duration_ms as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let head_pos = ((progress * (inner_width.saturating_sub(1) as f64)).round() as usize)
        .min(inner_width.saturating_sub(1));

    let mut out = String::with_capacity(bar_width * 8);
    out.push('[');

    match style {
        ScrubberStyle::Classic => {
            for _ in 0..head_pos {
                out.push_str("\x1b[1;36m━\x1b[0m");
            }
            out.push_str("\x1b[1;37m●\x1b[0m");
            for _ in (head_pos + 1)..inner_width {
                out.push_str("\x1b[90m─\x1b[0m");
            }
        }
        ScrubberStyle::BoxDrawing => {
            for i in 0..head_pos {
                if i == 0 {
                    out.push_str("\x1b[1;36m╺\x1b[0m");
                } else {
                    out.push_str("\x1b[1;36m━\x1b[0m");
                }
            }
            out.push_str("\x1b[1;37m●\x1b[0m");
            for _ in (head_pos + 1)..inner_width {
                out.push_str("\x1b[90m─\x1b[0m");
            }
        }
        ScrubberStyle::Block => {
            for _ in 0..head_pos {
                out.push_str("\x1b[1;36m█\x1b[0m");
            }
            out.push_str("\x1b[1;37m●\x1b[0m");
            for _ in (head_pos + 1)..inner_width {
                out.push_str("\x1b[90m─\x1b[0m");
            }
        }
    }

    out.push(']');
    out
}

/// Renders a 2D RGB video canvas to ANSI 24-bit TrueColor using Unicode Upper Half Block (`▀`, `U+2580`).
///
/// Each terminal cell displays two vertically stacked sub-pixels:
/// - Top sub-pixel: Foreground color (`\x1b[38;2;R;G;Bm`)
/// - Bottom sub-pixel: Background color (`\x1b[48;2;R;G;Bm`)
///
/// Employs run-length color deduplication to suppress redundant escape sequences,
/// drastically reducing bandwidth and eliminating visual terminal flicker.
pub fn render_half_block_canvas(
    cols: usize,
    rows: usize,
    top_colors: &[(u8, u8, u8)],
    bot_colors: &[(u8, u8, u8)],
    out: &mut String,
) {
    out.clear();
    let total_cells = cols * rows;
    if top_colors.len() < total_cells || bot_colors.len() < total_cells {
        return;
    }

    let mut current_fg: Option<(u8, u8, u8)> = None;
    let mut current_bg: Option<(u8, u8, u8)> = None;

    for r in 0..rows {
        let row_offset = r * cols;
        for c in 0..cols {
            let idx = row_offset + c;
            let fg = top_colors[idx];
            let bg = bot_colors[idx];

            if current_fg != Some(fg) {
                let _ = write!(out, "\x1b[38;2;{};{};{}m", fg.0, fg.1, fg.2);
                current_fg = Some(fg);
            }
            if current_bg != Some(bg) {
                let _ = write!(out, "\x1b[48;2;{};{};{}m", bg.0, bg.1, bg.2);
                current_bg = Some(bg);
            }
            out.push('▀');
        }
        out.push_str("\x1b[0m\n");
        current_fg = None;
        current_bg = None;
    }
}

/// Renders a single row of ANSI half-block characters into `out`.
pub fn render_half_block_row(
    cols: usize,
    top_colors: &[(u8, u8, u8)],
    bot_colors: &[(u8, u8, u8)],
    out: &mut String,
) {
    let mut current_fg: Option<(u8, u8, u8)> = None;
    let mut current_bg: Option<(u8, u8, u8)> = None;

    let len = cols.min(top_colors.len()).min(bot_colors.len());
    for i in 0..len {
        let fg = top_colors[i];
        let bg = bot_colors[i];

        if current_fg != Some(fg) {
            let _ = write!(out, "\x1b[38;2;{};{};{}m", fg.0, fg.1, fg.2);
            current_fg = Some(fg);
        }
        if current_bg != Some(bg) {
            let _ = write!(out, "\x1b[48;2;{};{};{}m", bg.0, bg.1, bg.2);
            current_bg = Some(bg);
        }
        out.push('▀');
    }
    out.push_str("\x1b[0m");
}

/// Renders a formatted sync diagnostics box with border characters.
pub fn render_sync_diagnostics_panel(
    rtt_ms: u64,
    dispersion_ms: f64,
    offset_ms: f64,
    phase_error_ms: f64,
    pll_speed: f64,
    unpause_phase: &str,
) -> String {
    let mut out = String::with_capacity(512);
    out.push_str("┌── [SYNC DIAGNOSTICS & PLL STATE] ──────────────────────────────────────────┐\n");
    let _ = writeln!(
        out,
        "│  WAN RTT: {:>4} ms  │  Dispersion: {:>5.1} ms  │  Offset: {:>+6.1} ms         │",
        rtt_ms, dispersion_ms, offset_ms
    );
    let _ = writeln!(
        out,
        "│  Phase Err: {:>+5.1} ms│  PLL Slew: {:>6.3}x       │  Unpause: {:<14}      │",
        phase_error_ms, pll_speed, unpause_phase
    );
    out.push_str("└────────────────────────────────────────────────────────────────────────────┘\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_styled_scrubber_bar() {
        let classic = render_styled_scrubber_bar(500, 1000, 20, ScrubberStyle::Classic);
        assert!(classic.contains('━'));
        assert!(classic.contains('●'));
        assert!(classic.contains('─'));

        let box_draw = render_styled_scrubber_bar(500, 1000, 20, ScrubberStyle::BoxDrawing);
        assert!(box_draw.contains('╺'));
        assert!(box_draw.contains('●'));

        let block = render_styled_scrubber_bar(500, 1000, 20, ScrubberStyle::Block);
        assert!(block.contains('█'));
        assert!(block.contains('●'));
    }

    #[test]
    fn test_render_half_block_canvas() {
        let cols = 4;
        let rows = 2;
        let top = vec![(255, 0, 0); cols * rows];
        let bot = vec![(0, 0, 255); cols * rows];

        let mut out = String::new();
        render_half_block_canvas(cols, rows, &top, &bot, &mut out);

        assert!(out.contains("▀"));
        assert!(out.contains("38;2;255;0;0"));
        assert!(out.contains("48;2;0;0;255"));
    }

    #[test]
    fn test_render_sync_diagnostics_panel() {
        let panel = render_sync_diagnostics_panel(42, 6.5, -2.1, 1.4, 1.002, "Ready/Commit");
        assert!(panel.contains("SYNC DIAGNOSTICS"));
        assert!(panel.contains("WAN RTT:   42 ms"));
        assert!(panel.contains("Ready/Commit"));
    }
}
