//! Shared visual constants and style helpers.
//!
//! Accent colours are applied to the *foreground* only, never as a
//! background fill: background patches read as harsh blocks on a
//! light-themed terminal, which is what this dashboard is used on.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

pub const ACCENT_COLOR: Color = Color::Cyan;
pub const SKILL_BAR_WIDTH: usize = 24;
pub const SKILL_NAME_WIDTH: usize = 16;

/// Highlight style + cursor symbol for a list, depending on whether its
/// panel currently holds focus. Keeping the two together is deliberate: an
/// unfocused list must lose *both* the colour and the `>` marker, otherwise
/// two panels look simultaneously active.
pub fn focus_style(focused: bool) -> (Style, &'static str) {
    if focused {
        (Style::default().fg(ACCENT_COLOR).add_modifier(Modifier::BOLD), "> ")
    } else {
        (Style::default(), "  ")
    }
}

/// Border style for a panel that participates in dashboard focus.
pub fn border_style(focused: bool) -> Style {
    if focused {
        Style::default().fg(ACCENT_COLOR)
    } else {
        Style::default()
    }
}

pub fn bold() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

/// Secondary-tier text (e.g. the mean/dominant-skill line under Market
/// Metrics' hero number). `Color::DarkGray` rather than `Modifier::DIM`:
/// many terminals ignore the dim modifier outright, so it wouldn't reliably
/// read as de-emphasised.
pub fn dim() -> Style {
    Style::default().fg(Color::DarkGray)
}

/// Colours a TJM trend delta: green rising, red falling, dim for a flat/
/// unavailable reading. `None` (not enough dated samples yet) gets the same
/// dim treatment as a near-zero delta — both mean "nothing to read here".
pub fn trend_style(delta: Option<f64>) -> Style {
    match delta {
        Some(d) if d > 1.0 => Style::default().fg(Color::Green),
        Some(d) if d < -1.0 => Style::default().fg(Color::Red),
        _ => dim(),
    }
}

/// A `width` × `height` rectangle centred in `area`, clamped so it always
/// fits — every overlay goes through this rather than doing its own `Rect`
/// arithmetic, which is how a narrow terminal ends up with a popup drawn
/// partly off-screen.
pub fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}
