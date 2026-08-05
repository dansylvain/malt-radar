//! Shared visual constants and style helpers.
//!
//! Accent colours (panel identity, focus, semantic data colours) are
//! applied to the *foreground* only, never as a local background patch —
//! a patch on top of the terminal's own background reads as a harsh block.
//! The one exception is `BG` itself: a single uniform fill painted once
//! behind the whole app (see `ui::draw`) is the app's own canvas, not a
//! patch, and is what makes the rest of the palette (built for a dark
//! background) legible regardless of the terminal's own theme.
//!
//! Palette (see `Malt-radar palette de couleurs.pdf`): one colour per
//! role, not one colour everywhere. `TEAL` is the brand colour — Market
//! Metrics' identity, and also the generic "something is focused/active"
//! colour used where a panel has no identity of its own (Live Feed, Help).
//! `VIOLET` marks the two "analysis" panels (Top Skills, Digest) and their
//! full-screen views, plus Skill Audit which lives in the same family.
//! `AMBER`/`GREEN`/`RED` are semantic and travel with the *data*, not the
//! chrome: money, positive status, security-sensitive tags — applied the
//! same way no matter which panel they show up in.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

pub const BG: Color = Color::Rgb(0x0B, 0x0F, 0x14);
pub const TEAL: Color = Color::Rgb(0x45, 0xD9, 0xCF);
pub const TEAL_DARK: Color = Color::Rgb(0x12, 0x3A, 0x38);
pub const VIOLET: Color = Color::Rgb(0x8B, 0x93, 0xF5);
pub const AMBER: Color = Color::Rgb(0xF0, 0xB4, 0x29);
pub const GREEN: Color = Color::Rgb(0x5F, 0xD9, 0x8A);
pub const RED: Color = Color::Rgb(0xF0, 0x63, 0x7A);
pub const TEXT_PRIMARY: Color = Color::Rgb(0xE7, 0xEA, 0xEE);
pub const TEXT_MUTED: Color = Color::Rgb(0x57, 0x61, 0x6F);
pub const BORDER_NEUTRAL: Color = Color::Rgb(0x3A, 0x44, 0x52);

pub const SKILL_BAR_WIDTH: usize = 24;
pub const SKILL_NAME_WIDTH: usize = 16;

/// Default style for a widget's unstyled cells: `TEXT_PRIMARY` on `BG`.
/// Ratatui patches per-span styles over a widget's base style, so setting
/// this once via `Paragraph::style`/`List::style` is enough to move plain
/// text (and its background) off the terminal's own theme without
/// touching every call site — spans that set their own colour (amber TJM,
/// dim(), etc.) still win over this. The `bg(BG)` half matters most for
/// overlays: `Clear` blanks a popup's cells back to the terminal's own
/// background before the popup redraws over it, so its `Block`/`Paragraph`
/// need an explicit `BG` to stay opaque rather than showing a hole.
pub fn base_style() -> Style {
    Style::default().fg(TEXT_PRIMARY).bg(BG)
}

/// Highlight style + cursor symbol for a list, depending on whether its
/// panel currently holds focus, in `accent` — that panel's identity
/// colour. Keeping colour and symbol together is deliberate: an unfocused
/// list must lose *both*, otherwise two panels look simultaneously active.
pub fn focus_style(focused: bool, accent: Color) -> (Style, &'static str) {
    if focused {
        (Style::default().fg(accent).add_modifier(Modifier::BOLD), "> ")
    } else {
        (Style::default(), "  ")
    }
}

/// Border style for a panel that participates in dashboard focus:
/// `BORDER_NEUTRAL` at rest, `accent` (that panel's identity colour) once
/// focused.
pub fn border_style(focused: bool, accent: Color) -> Style {
    if focused {
        Style::default().fg(accent)
    } else {
        Style::default().fg(BORDER_NEUTRAL)
    }
}

pub fn bold() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

/// Panel/block title as a solid badge: `accent` as the background, bold
/// `TEXT_PRIMARY` text on top — the inverse of how `accent` is used
/// everywhere else (foreground-only). Deliberate exception to that rule,
/// at Dan's explicit request, scoped to titles only. Every
/// `Block::title(...)` in the TUI should go through this rather than a
/// bare `&str`/`String`, so panel titles stay visually consistent.
pub fn panel_title(text: impl Into<String>, accent: Color) -> Span<'static> {
    Span::styled(text.into(), bold().fg(accent))
}

/// Secondary-tier text (e.g. the mean/dominant-skill line under Market
/// Metrics' hero number). `TEXT_MUTED` rather than `Modifier::DIM`: many
/// terminals ignore the dim modifier outright, so it wouldn't reliably
/// read as de-emphasised.
pub fn dim() -> Style {
    Style::default().fg(TEXT_MUTED)
}

/// Colours a TJM trend delta: green rising, red falling, dim for a flat/
/// unavailable reading. `None` (not enough dated samples yet) gets the same
/// dim treatment as a near-zero delta — both mean "nothing to read here".
pub fn trend_style(delta: Option<f64>) -> Style {
    match delta {
        Some(d) if d > 1.0 => Style::default().fg(GREEN),
        Some(d) if d < -1.0 => Style::default().fg(RED),
        _ => dim(),
    }
}

/// Style for the footer's status line: red for an error message (the
/// convention used throughout `app.rs` is to prefix these with
/// `"erreur"`), green otherwise — "130 fiches chargées" and friends read
/// as a positive/neutral status, per the palette's "statuts positifs en
/// vert" rule.
pub fn status_style(text: &str) -> Style {
    if text.starts_with("erreur") {
        Style::default().fg(RED)
    } else {
        Style::default().fg(GREEN)
    }
}

/// Linear interpolation between two `Color::Rgb` values at `t` (0.0..=1.0).
fn lerp_rgb(from: (u8, u8, u8), to: (u8, u8, u8), t: f64) -> Color {
    let t = t.clamp(0.0, 1.0);
    let lerp = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * t).round() as u8;
    Color::Rgb(lerp(from.0, to.0), lerp(from.1, to.1), lerp(from.2, to.2))
}

/// Colour for one cell of a Top Skills bar at `index` out of `width`
/// cells: saturated `TEAL` at the start fading to `TEAL_DARK` (near-bg) at
/// the end, for a depth effect instead of a flat fill — the palette's
/// explicit recommendation for these bars specifically (unlike the rest of
/// the Top Skills panel, which carries the violet "analysis" identity).
pub fn skill_bar_gradient(index: usize, width: usize) -> Color {
    if width <= 1 {
        return TEAL;
    }
    let t = index as f64 / (width - 1) as f64;
    let (Color::Rgb(fr, fg, fb), Color::Rgb(tr, tg, tb)) = (TEAL, TEAL_DARK) else {
        unreachable!("TEAL/TEAL_DARK are always Color::Rgb")
    };
    lerp_rgb((fr, fg, fb), (tr, tg, tb), t)
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
