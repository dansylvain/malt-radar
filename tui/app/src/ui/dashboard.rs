//! The four-zone dashboard: metrics and skills on top, feed and digest
//! below, status bar at the bottom.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use malt_radar_core::{iqr_gauge_positions, truncate_label, Digest, Fiche, MarketMetrics, TjmTrend, TREND_WINDOW_DAYS};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, Focus, Mode};
use crate::ui::theme::{
    bold, border_style, dim, focus_style, trend_style, ACCENT_COLOR, SKILL_BAR_WIDTH, SKILL_NAME_WIDTH,
};

const IQR_GAUGE_WIDTH: usize = 20;
/// Small enough to leave most of the screen to Live Feed — the digest is
/// long-form (~40-60 lines of markdown) and meant to be scrolled (`j`/`k`,
/// PageUp/PageDown — see `App::digest_scroll_by`), not fully visible at
/// once. An earlier version sized this panel to fit a "meaningful chunk"
/// of the digest (18 rows), but that ate too much of the Live Feed for a
/// panel that's scrollable anyway.
const DIGEST_PANEL_HEIGHT: u16 = 8;

pub fn draw_dashboard(frame: &mut Frame, app: &mut App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(11),
            Constraint::Fill(1),
            Constraint::Length(DIGEST_PANEL_HEIGHT),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(root[0]);

    let filter_label = app.feed_filter_label();
    draw_market_metrics(frame, top[0], &app.metrics, &app.trend);
    draw_top_skills(
        frame,
        top[1],
        &app.metrics,
        &mut app.top_skills_state,
        app.focus == Focus::TopSkills,
    );
    draw_live_feed(
        frame,
        root[1],
        &app.resolved_fiches,
        &app.feed.visible,
        &app.feed.fresh,
        &mut app.feed.list_state,
        filter_label.as_deref(),
        app.focus == Focus::LiveFeed,
    );
    draw_digest(frame, root[2], app.digest.as_ref(), app.digest_scroll, app.focus == Focus::Digest);
    draw_footer(frame, root[3], app);
}

/// Trend arrow + delta text for the hero line, coloured via `trend_style`.
/// `None` reads the same as a near-zero delta (both mean "nothing here yet")
/// except it uses `—` instead of `→`, so a genuinely flat market isn't
/// mistaken for "not enough data".
fn format_trend(delta: Option<f64>) -> (String, Style) {
    let style = trend_style(delta);
    let text = match delta {
        Some(d) if d > 1.0 => format!("↑ +{d:.0} ({TREND_WINDOW_DAYS}j)"),
        Some(d) if d < -1.0 => format!("↓ {d:.0} ({TREND_WINDOW_DAYS}j)"),
        Some(_) => format!("→ ({TREND_WINDOW_DAYS}j)"),
        None => format!("— ({TREND_WINDOW_DAYS}j)"),
    };
    (text, style)
}

/// Renders the IQR gauge as a single styled line: `─` outside the
/// interquartile range, `█` inside it, `┃` at the median — see
/// `iqr_gauge_positions` for the underlying arithmetic.
fn iqr_gauge_line(metrics: &MarketMetrics) -> Line<'static> {
    let Some((p25_pos, median_pos, p75_pos)) = iqr_gauge_positions(&metrics.tjm, IQR_GAUGE_WIDTH) else {
        return Line::styled("TJM insuffisamment renseigné", dim());
    };
    let bar_style = Style::default().fg(ACCENT_COLOR);
    let mut spans: Vec<Span<'static>> = (0..IQR_GAUGE_WIDTH)
        .map(|i| {
            if i == median_pos {
                Span::styled("┃", bold().fg(ACCENT_COLOR))
            } else if i >= p25_pos && i <= p75_pos {
                Span::styled("█", bar_style)
            } else {
                Span::styled("─", dim())
            }
        })
        .collect();
    let (p25, p75) = (metrics.tjm.p25.unwrap_or_default(), metrics.tjm.p75.unwrap_or_default());
    spans.push(Span::raw(format!("  p25 {p25:.0} · p75 {p75:.0}")));
    Line::from(spans)
}

fn draw_market_metrics(frame: &mut Frame, area: Rect, metrics: &MarketMetrics, trend: &TjmTrend) {
    let tjm = &metrics.tjm;
    let fmt_eur = |v: f64| format!("{v:.0} EUR/j");

    let hero_line = match tjm.median {
        Some(median) => {
            let (trend_text, trend_style) = format_trend(trend.delta);
            Line::from(vec![
                Span::raw("  "),
                Span::styled(fmt_eur(median), bold().fg(ACCENT_COLOR)),
                Span::raw("    "),
                Span::styled(trend_text, trend_style),
            ])
        }
        None => Line::styled("  TJM non renseigné", dim()),
    };

    let gauge_line = {
        let mut line = iqr_gauge_line(metrics);
        line.spans.insert(0, Span::raw("  "));
        line
    };

    let avg = tjm.avg.map(fmt_eur).unwrap_or_else(|| "n/a".to_string());
    let top_skill = metrics
        .top_skills
        .first()
        .map(|s| format!("{} ({}%)", s.name, s.percent_rounded))
        .unwrap_or_else(|| "n/a".to_string());
    let secondary_line = Line::styled(format!("  moyen {avg} · dominant {top_skill}"), dim());

    let lines = vec![Line::from(""), hero_line, gauge_line, secondary_line];
    let paragraph = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Market Metrics"));
    frame.render_widget(paragraph, area);
}

/// Every distinct skill now shows (`ALL_SKILLS` — no more top-5 cap), so
/// unlike Round 1 this panel scrolls: a real `List`/`ListState` rather than
/// a fixed `Paragraph`, mirroring the Live Feed's stateful rendering.
///
/// Bars are relative to the *top* skill's count, not an absolute 100% share
/// — the old absolute scale leaned on "Autres" always soaking up whatever
/// was left; without a cap there's no such bucket, and an absolute scale
/// would render every bar unreadably short (a top skill at 6% of all
/// mentions would fill one character out of 24). The percentage shown
/// alongside each bar is still the real share of mentions.
fn draw_top_skills(
    frame: &mut Frame,
    area: Rect,
    metrics: &MarketMetrics,
    list_state: &mut ListState,
    focused: bool,
) {
    let max_count = metrics.top_skills.iter().map(|s| s.count).max().unwrap_or(0);
    let items: Vec<ListItem> = metrics
        .top_skills
        .iter()
        .map(|skill| {
            let filled = if max_count == 0 {
                0
            } else {
                ((skill.count as f64 / max_count as f64) * SKILL_BAR_WIDTH as f64).round() as usize
            };
            let bar = format!("{}{}", "█".repeat(filled), " ".repeat(SKILL_BAR_WIDTH - filled));
            let name = truncate_label(&skill.name, SKILL_NAME_WIDTH);
            let pct = if skill.percent_rounded == 0 {
                "<1%".to_string()
            } else {
                format!("{}%", skill.percent_rounded)
            };
            let line = Line::from(vec![
                Span::raw(format!("{name:<SKILL_NAME_WIDTH$}")),
                Span::raw("["),
                Span::styled(bar, Style::default().fg(ACCENT_COLOR)),
                Span::raw(format!("] {:>3} {pct:>4}", skill.count)),
            ]);
            ListItem::new(line)
        })
        .collect();

    let (hl_style, hl_symbol) = focus_style(focused);
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Top Skills")
                .border_style(border_style(focused)),
        )
        .highlight_style(hl_style)
        .highlight_symbol(hl_symbol);
    frame.render_stateful_widget(list, area, list_state);
}

/// `created_at` (RFC 3339, e.g. `2026-08-05T08:00:12+00:00`) as a
/// human-relative French label — same rough shape as the footer's
/// `{}s`/refresh-age readout, just coarser (minutes/hours/days) since a
/// digest is generated once a day, not once a minute. Falls back to the
/// raw string on a parse failure rather than hiding a malformed timestamp.
fn format_digest_age(created_at: &str) -> String {
    let Ok(parsed) = DateTime::parse_from_rfc3339(created_at) else {
        return created_at.to_string();
    };
    let elapsed = Utc::now().signed_duration_since(parsed.with_timezone(&Utc));
    if elapsed.num_days() >= 1 {
        format!("il y a {}j", elapsed.num_days())
    } else if elapsed.num_hours() >= 1 {
        format!("il y a {}h", elapsed.num_hours())
    } else if elapsed.num_minutes() >= 1 {
        format!("il y a {}min", elapsed.num_minutes())
    } else {
        "à l'instant".to_string()
    }
}

/// Line-by-line "markdown-lite" styling — not a real Markdown renderer,
/// just enough to make the fixed 5-section structure `digest.py`'s system
/// prompt always produces (`## <Section>`, `---` separators) scannable in
/// a terminal: `##` headers go bold+accent (prefix stripped), separators
/// go dim, everything else renders as plain wrapped text.
fn digest_content_lines(content: &str) -> Vec<Line<'static>> {
    content
        .lines()
        .map(|line| {
            if let Some(header) = line.strip_prefix("## ") {
                Line::styled(header.to_string(), bold().fg(ACCENT_COLOR))
            } else if line.trim() == "---" {
                Line::styled(line.to_string(), dim())
            } else {
                Line::raw(line.to_string())
            }
        })
        .collect()
}

/// Scrollable (see `App::digest_scroll_by`) and focusable like Top
/// Skills/Live Feed, despite being read-only content — focus is what
/// routes `↑`/`↓` here instead of to the feed/skills selection (see
/// `handle_dashboard_key`). Shows `summary_short` only — the panel is a
/// compact ~6-line strip, and the full 5-section Markdown lives one
/// `Enter` away in `Mode::DigestFull` (`draw_digest_full`).
fn draw_digest(frame: &mut Frame, area: Rect, digest: Option<&Digest>, scroll: u16, focused: bool) {
    let title = match digest {
        Some(d) => format!(
            "DAILY AI STRATEGY DIGEST — {} fiches · généré {} · [Entrée] pour le digest complet",
            d.fiche_count,
            format_digest_age(&d.created_at)
        ),
        None => "DAILY AI STRATEGY DIGEST".to_string(),
    };

    let lines = match digest {
        Some(d) => digest_content_lines(&d.summary_short),
        None => vec![Line::styled(
            "Aucun digest généré pour l'instant — le job quotidien n'a pas encore tourné.",
            dim(),
        )],
    };

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false }).scroll((scroll, 0)).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(border_style(focused)),
    );
    frame.render_widget(paragraph, area);
}

/// Full-screen view of the current digest's complete Markdown
/// (`digest.content`) — entered with `Enter` while `Focus::Digest` (see
/// `App::open_digest_full`). Same "full-screen content + one-line footer"
/// shape as `skill_audit::draw_skill_audit`, rather than an overlay on top
/// of the dashboard: the digest is long-form content meant to be read, not
/// a popup glanced at and dismissed.
pub fn draw_digest_full(frame: &mut Frame, digest: Option<&Digest>, scroll: u16) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Fill(1), Constraint::Length(1)])
        .split(frame.area());

    let title = match digest {
        Some(d) => format!(
            "DAILY AI STRATEGY DIGEST — {} fiches · généré {}",
            d.fiche_count,
            format_digest_age(&d.created_at)
        ),
        None => "DAILY AI STRATEGY DIGEST".to_string(),
    };

    let lines = match digest {
        Some(d) => digest_content_lines(&d.content),
        None => vec![Line::styled(
            "Aucun digest généré pour l'instant — le job quotidien n'a pas encore tourné.",
            dim(),
        )],
    };

    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0))
        .block(Block::default().borders(Borders::ALL).title(title));
    frame.render_widget(paragraph, root[0]);

    let footer = Line::from(vec![
        Span::styled("[↑/↓ ou j/k]", bold()),
        Span::raw(" défiler  "),
        Span::styled("[PgUp/PgDn]", bold()),
        Span::raw(" page  "),
        Span::styled("[Home]", bold()),
        Span::raw(" début  "),
        Span::styled("[Esc/Entrée]", bold()),
        Span::raw(" retour  "),
        Span::styled("[q]", bold()),
        Span::raw(" quitter"),
    ]);
    frame.render_widget(Paragraph::new(footer), root[1]);
}

/// `visible` indexes into `resolved_fiches` — the fiches passing the
/// current filter, in their original order. `list_state.selected()` is an
/// index into `visible` (the Nth *visible* row), not into `resolved_fiches`
/// directly, since a filter can drop items the selection previously sat on.
#[allow(clippy::too_many_arguments)]
fn draw_live_feed(
    frame: &mut Frame,
    area: Rect,
    resolved_fiches: &[Fiche],
    visible: &[usize],
    fresh: &HashSet<String>,
    list_state: &mut ListState,
    filter_label: Option<&str>,
    focused: bool,
) {
    let items: Vec<ListItem> = visible
        .iter()
        .filter_map(|&idx| resolved_fiches.get(idx))
        .map(|fiche| {
            let title = fiche.title.as_deref().unwrap_or("(sans titre)");
            let company = fiche.company.as_deref().unwrap_or("?");
            let tjm = fiche
                .tjm_mid()
                .map(|v| format!("{v:.0} EUR/j"))
                .unwrap_or_else(|| "TJM n/a".to_string());
            let stack = if fiche.stack.is_empty() {
                "(stack non extraite)".to_string()
            } else {
                fiche.stack.join(", ")
            };

            // Fraîche depuis le dernier refresh : marqueur `•` + accent en
            // façade, jamais un fond coloré (les aplats de fond sortent mal
            // sur ce terminal). Sur deux lignes : la ligne stack débordait
            // souvent la largeur du panneau (List ne wrap pas comme
            // Paragraph), d'où la coupure sans retour à la ligne observée.
            let is_fresh = fresh.contains(&fiche.filename);
            let marker = if is_fresh { "• " } else { "  " };
            let text = Text::from(vec![
                Line::from(format!("{marker}{title} — {company} — {tjm}")),
                Line::from(format!("    {stack}")),
            ]);
            let style = if is_fresh {
                Style::default().fg(ACCENT_COLOR)
            } else {
                Style::default()
            };

            ListItem::new(text).style(style)
        })
        .collect();

    let title = match filter_label {
        Some(label) => format!("Live Feed ({}/{} — {label})", visible.len(), resolved_fiches.len()),
        None => format!("Live Feed ({})", resolved_fiches.len()),
    };

    let (hl_style, hl_symbol) = focus_style(focused);
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(border_style(focused)),
        )
        .highlight_style(hl_style)
        .highlight_symbol(hl_symbol);
    frame.render_stateful_widget(list, area, list_state);
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    if app.mode == Mode::FeedFilter {
        let line = Line::from(vec![
            Span::raw("/"),
            Span::raw(app.feed.filter_text.as_str()),
            Span::styled("▏", Style::default().fg(Color::Yellow)),
            Span::raw("   "),
            Span::styled("[Entrée]", bold()),
            Span::raw(" valider  "),
            Span::styled("[Esc]", bold()),
            Span::raw(" annuler  "),
            Span::styled("[Ctrl+U]", bold()),
            Span::raw(" vider"),
        ]);
        frame.render_widget(Paragraph::new(line), area);
        return;
    }

    let age = app
        .last_refreshed
        .map(|t| format!("{}s", t.elapsed().as_secs()))
        .unwrap_or_else(|| "jamais".to_string());
    let loading_marker = if app.loading { " (chargement...)" } else { "" };
    // Data-quality readout, not a market signal — deliberately separate from
    // Market Metrics' hero number (see `draw_market_metrics`), and the
    // keymap shrinks to `?`/`q` now that Help carries the full reference.
    let tjm = &app.metrics.tjm;
    let coverage_pct = (tjm.coverage_ratio() * 100.0).round() as i64;
    let line = Line::from(vec![
        Span::styled("[?]", bold()),
        Span::raw(" aide · "),
        Span::styled("[q]", bold()),
        Span::raw(" quitter   "),
        Span::raw(format!(
            "{} fiches · TJM connu {}/{} ({coverage_pct}%)   ",
            app.metrics.fiche_count, tjm.sample, tjm.total
        )),
        Span::styled(&app.status_line, Style::default().fg(Color::Yellow)),
        Span::raw(format!("   refresh {age}{loading_marker}")),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}
