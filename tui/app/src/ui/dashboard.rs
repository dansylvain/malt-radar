//! The four-zone dashboard: metrics and skills on top, feed and digest
//! below, status bar at the bottom.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use malt_radar_core::{
    is_sensitive_skill, iqr_gauge_positions, truncate_label, Digest, Fiche, MarketMetrics, TjmTrend,
};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, Focus, Mode};
use crate::ui::theme::{
    base_style, bold, border_style, dim, focus_style, panel_title, skill_bar_gradient, status_style,
    trend_style, AMBER, BORDER_NEUTRAL, RED, SKILL_BAR_WIDTH, SKILL_NAME_WIDTH, TEAL, VIOLET,
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
            Constraint::Length(9),
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
    draw_digest(frame, root[2], app.digest.as_ref(), &mut app.digest_scroll, app.focus == Focus::Digest);
    draw_footer(frame, root[3], app);
}

/// Trend arrow + delta text for the hero line, coloured via `trend_style`.
/// `None` reads the same as a near-zero delta (both mean "nothing here yet")
/// except it uses `—` instead of `→`, so a genuinely flat market isn't
/// mistaken for "not enough data".
fn format_trend(delta: Option<f64>) -> (String, Style) {
    let style = trend_style(delta);
    let text = match delta {
        Some(d) if d > 1.0 => format!("↑ +{d:.0}"),
        Some(d) if d < -1.0 => format!("↓ {d:.0}"),
        Some(_) => "→".to_string(),
        None => "—".to_string(),
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
    let bar_style = Style::default().fg(AMBER);
    let spans: Vec<Span<'static>> = (0..IQR_GAUGE_WIDTH)
        .map(|i| {
            if i == median_pos {
                Span::styled("┃", bold().fg(AMBER))
            } else if i >= p25_pos && i <= p75_pos {
                Span::styled("█", bar_style)
            } else {
                Span::styled("─", dim())
            }
        })
        .collect();
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
                Span::styled(fmt_eur(median), bold().fg(AMBER)),
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

    let range_line = match (tjm.min, tjm.max) {
        (Some(min), Some(max)) => Line::styled(format!("  min {min:.0} · max {max:.0}"), dim()),
        _ => Line::from(""),
    };

    let coverage_pct = (tjm.coverage_ratio() * 100.0).round() as i64;
    let volume_line = Line::styled(
        format!("  {} fiches · TJM connu {}/{} ({coverage_pct}%)", metrics.fiche_count, tjm.sample, tjm.total),
        dim(),
    );

    let avg = tjm.avg.map(fmt_eur).unwrap_or_else(|| "n/a".to_string());
    let top_skill = metrics
        .top_skills
        .first()
        .map(|s| format!("{} ({}%)", s.name, s.percent_rounded))
        .unwrap_or_else(|| "n/a".to_string());
    let secondary_line = Line::styled(format!("  moyen {avg} · dominant {top_skill}"), dim());

    let lines = vec![Line::from(""), hero_line, gauge_line, range_line, volume_line, secondary_line];
    let block = Block::default()
        .borders(Borders::ALL)
        .title(panel_title("MARKET METRICS", TEAL))
        .border_style(Style::default().fg(TEAL));
    let paragraph = Paragraph::new(lines).style(base_style()).block(block);
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
            let name = truncate_label(&skill.name, SKILL_NAME_WIDTH);
            let pct = if skill.percent_rounded == 0 {
                "<1%".to_string()
            } else {
                format!("{}%", skill.percent_rounded)
            };
            // Teal → near-bg gradient rather than a flat fill, one span per
            // cell — the palette's explicit call-out for these bars, kept
            // separate from the panel's own violet identity (border/title).
            let mut spans = vec![
                Span::raw(format!("{name:<SKILL_NAME_WIDTH$}")),
                Span::raw("["),
            ];
            for i in 0..SKILL_BAR_WIDTH {
                let ch = if i < filled { "█" } else { " " };
                spans.push(Span::styled(ch, Style::default().fg(skill_bar_gradient(i, SKILL_BAR_WIDTH))));
            }
            spans.push(Span::raw(format!("] {:>3} {pct:>4}", skill.count)));
            ListItem::new(Line::from(spans))
        })
        .collect();

    let (hl_style, hl_symbol) = focus_style(focused, VIOLET);
    let list = List::new(items)
        .style(base_style())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(panel_title("TOP SKILLS", VIOLET))
                .border_style(border_style(focused, VIOLET)),
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
/// prompt always produces (`## <Section>`, `---` separators, `**bold**`
/// emphasis) scannable in a terminal: `#`/`##` headers go bold+accent
/// (prefix stripped, any `**` markers inside dropped since the whole line
/// is already bold), separators go dim, everything else goes through
/// `parse_inline_bold` so `**text**` spans render bold with the markers
/// stripped. `accent` is the digest's identity colour (`VIOLET`), passed
/// through rather than hardcoded so this stays reusable if a caller ever
/// needs a different emphasis colour.
fn digest_content_lines(content: &str, accent: Color) -> Vec<Line<'static>> {
    content
        .lines()
        .map(|line| {
            if let Some(header) = line.strip_prefix("## ") {
                Line::styled(header.replace("**", ""), bold().fg(accent))
            } else if let Some(header) = line.strip_prefix("# ") {
                Line::styled(header.replace("**", ""), bold().fg(accent))
            } else if line.trim() == "---" {
                Line::styled(line.to_string(), dim())
            } else {
                Line::from(parse_inline_bold(line, accent))
            }
        })
        .collect()
}

/// Splits a line on `**bold**` markers, stripping the markers and returning
/// the bracketed text as a bold span among plain-text spans either side. An
/// unterminated `**` (no closing pair) is left as literal text rather than
/// swallowed — a truncated digest shouldn't silently eat the marker.
///
/// Paired with `accent`, not just `Modifier::BOLD` alone: confirmed against
/// a real render that some terminals don't visibly distinguish bold text
/// without an accompanying colour change (same reason the `##` headers
/// above pair `bold()` with `.fg(accent)` rather than using bold on its
/// own).
fn parse_inline_bold(line: &str, accent: Color) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find("**") {
        if start > 0 {
            spans.push(Span::raw(rest[..start].to_string()));
        }
        let after_marker = &rest[start + 2..];
        match after_marker.find("**") {
            Some(end) => {
                spans.push(Span::styled(after_marker[..end].to_string(), bold().fg(accent)));
                rest = &after_marker[end + 2..];
            }
            None => {
                spans.push(Span::raw(rest[start..].to_string()));
                rest = "";
                break;
            }
        }
    }
    if !rest.is_empty() {
        spans.push(Span::raw(rest.to_string()));
    }
    spans
}

/// Scrollable (see `App::digest_scroll_by`) and focusable like Top
/// Skills/Live Feed, despite being read-only content — focus is what
/// routes `↑`/`↓` here instead of to the feed/skills selection (see
/// `handle_dashboard_key`). Shows `summary_short` only — the panel is a
/// compact ~6-line strip, and the full 5-section Markdown lives one
/// `Enter` away in `Mode::DigestFull` (`draw_digest_full`).
///
/// `scroll` is taken by `&mut` and clamped in place against the true
/// wrapped-line count (`Paragraph::line_count`, needs the render width,
/// which only exists here) — this is what stops `digest_scroll_by` from
/// scrolling past the last line into blank space, and it's why the clamp
/// can't live in `App` itself.
fn draw_digest(frame: &mut Frame, area: Rect, digest: Option<&Digest>, scroll: &mut u16, focused: bool) {
    let title = match digest {
        Some(d) => format!(
            "DAILY AI STRATEGY DIGEST — {} fiches · généré {} · [Entrée] pour le digest complet",
            d.fiche_count,
            format_digest_age(&d.created_at)
        ),
        None => "DAILY AI STRATEGY DIGEST".to_string(),
    };

    let lines = match digest {
        Some(d) => digest_content_lines(&d.summary_short, VIOLET),
        None => vec![Line::styled(
            "Aucun digest généré pour l'instant — le job quotidien n'a pas encore tourné.",
            dim(),
        )],
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(panel_title(title, VIOLET))
        .border_style(border_style(focused, VIOLET));
    let inner = block.inner(area);

    let content = Paragraph::new(lines).style(base_style()).wrap(Wrap { trim: false });
    let total_lines = content.line_count(inner.width.max(1)) as u16;
    *scroll = (*scroll).min(total_lines.saturating_sub(inner.height));

    let paragraph = content.scroll((*scroll, 0)).block(block);
    frame.render_widget(paragraph, area);
}

/// Full-screen view of the current digest's complete Markdown
/// (`digest.content`) — entered with `Enter` while `Focus::Digest` (see
/// `App::open_digest_full`). Same "full-screen content + one-line footer"
/// shape as `skill_audit::draw_skill_audit`, rather than an overlay on top
/// of the dashboard: the digest is long-form content meant to be read, not
/// a popup glanced at and dismissed.
///
/// `scroll` is `&mut`, clamped in place — same reasoning as `draw_digest`.
pub fn draw_digest_full(frame: &mut Frame, digest: Option<&Digest>, scroll: &mut u16) {
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
        Some(d) => digest_content_lines(&d.content, VIOLET),
        None => vec![Line::styled(
            "Aucun digest généré pour l'instant — le job quotidien n'a pas encore tourné.",
            dim(),
        )],
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(panel_title(title, VIOLET))
        .border_style(Style::default().fg(VIOLET));
    let inner = block.inner(root[0]);

    let content = Paragraph::new(lines).style(base_style()).wrap(Wrap { trim: false });
    let total_lines = content.line_count(inner.width.max(1)) as u16;
    *scroll = (*scroll).min(total_lines.saturating_sub(inner.height));

    let paragraph = content.scroll((*scroll, 0)).block(block);
    frame.render_widget(paragraph, root[0]);

    let footer = Line::from(vec![
        Span::styled("[↑/↓ ou j/k]", bold()),
        Span::raw(" défiler  "),
        Span::styled("[PgUp/PgDn]", bold()),
        Span::raw(" page  "),
        Span::styled("[Home]", bold()),
        Span::raw(" début  "),
        Span::styled("[End]", bold()),
        Span::raw(" fin  "),
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

            // Fraîche depuis le dernier refresh : marqueur `•` + accent en
            // façade, jamais un fond coloré (les aplats de fond sortent mal
            // sur ce terminal). Sur deux lignes : la ligne stack débordait
            // souvent la largeur du panneau (List ne wrap pas comme
            // Paragraph), d'où la coupure sans retour à la ligne observée.
            let is_fresh = fresh.contains(&fiche.filename);
            let marker = if is_fresh { "• " } else { "  " };
            let row_style = if is_fresh { Style::default().fg(TEAL) } else { Style::default() };

            let head_line = Line::from(vec![
                Span::raw(format!("{marker}{title} — {company} — ")),
                Span::styled(tjm, Style::default().fg(AMBER)),
            ]);

            // Stack tags get their own spans (not one joined string) so a
            // security-sensitive tag (EDR/SIEM/XDR/Cybersécurité — see
            // `is_sensitive_skill`) can be picked out in red among the rest,
            // per the palette's "repérer d'un coup d'œil les fiches
            // sécurité" recommendation.
            let mut stack_spans = vec![Span::raw("    ")];
            if fiche.stack.is_empty() {
                stack_spans.push(Span::raw("(stack non extraite)"));
            } else {
                for (i, skill) in fiche.stack.iter().enumerate() {
                    if i > 0 {
                        stack_spans.push(Span::raw(", "));
                    }
                    let style = if is_sensitive_skill(skill) { Style::default().fg(RED) } else { Style::default() };
                    stack_spans.push(Span::styled(skill.clone(), style));
                }
            }

            let text = Text::from(vec![head_line, Line::from(stack_spans)]);
            ListItem::new(text).style(row_style)
        })
        .collect();

    let title = match filter_label {
        Some(label) => format!("LIVE FEED ({}/{} — {label})", visible.len(), resolved_fiches.len()),
        None => format!("LIVE FEED ({})", resolved_fiches.len()),
    };

    // Live Feed carries no panel identity of its own — border stays neutral
    // whether focused or not, so it doesn't compete with the amber/red data
    // it lists; the `>` cursor and row highlight (in brand teal) are the
    // focus cue instead.
    let (hl_style, hl_symbol) = focus_style(focused, TEAL);
    let list = List::new(items)
        .style(base_style())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(panel_title(title, TEAL))
                .border_style(Style::default().fg(BORDER_NEUTRAL)),
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
            Span::styled("▏", Style::default().fg(TEAL)),
            Span::raw("   "),
            Span::styled("[Entrée]", bold()),
            Span::raw(" valider  "),
            Span::styled("[Esc]", bold()),
            Span::raw(" annuler  "),
            Span::styled("[Ctrl+U]", bold()),
            Span::raw(" vider"),
        ]);
        frame.render_widget(Paragraph::new(line).style(base_style()), area);
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
        Span::styled(&app.status_line, status_style(&app.status_line)),
        Span::raw(format!("   refresh {age}{loading_marker}")),
    ]);
    frame.render_widget(Paragraph::new(line).style(base_style()), area);
}

#[cfg(test)]
mod tests {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use malt_radar_core::{Digest, MarketMetrics, SkillStat, TjmStats, TjmTrend};

    use super::{draw_digest, draw_digest_full, draw_market_metrics};

    /// Flattens a `TestBackend`'s buffer into one string per row, trimmed of
    /// trailing padding — lets tests assert on rendered text by substring
    /// instead of poking at cells directly.
    fn buffer_rows(terminal: &Terminal<TestBackend>) -> Vec<String> {
        let buffer = terminal.backend().buffer();
        let area = buffer.area;
        (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    /// The bug this guards: the panel used to show only median/trend, the
    /// IQR gauge and an average+dominant-skill line — no fiche count and no
    /// min/max range. Confirms both were actually wired into the render, not
    /// just into the data model. Also locks in that the trend arrow and the
    /// gauge stay bare (no "(7j, n=X→Y)" / "p25 · p75" legend text) — Dan
    /// found that annotation confusing in practice and asked for it back out.
    #[test]
    fn draw_market_metrics_shows_volume_and_range_without_cluttering_trend_or_gauge() {
        let backend = TestBackend::new(60, 11);
        let mut terminal = Terminal::new(backend).unwrap();
        let metrics = MarketMetrics {
            fiche_count: 342,
            tjm: TjmStats {
                sample: 210,
                total: 342,
                avg: Some(495.0),
                median: Some(480.0),
                p25: Some(400.0),
                p75: Some(605.0),
                min: Some(250.0),
                max: Some(900.0),
            },
            top_skills: vec![SkillStat {
                name: "Python".to_string(),
                count: 67,
                percent: 32.0,
                percent_rounded: 32,
            }],
        };
        let trend = TjmTrend {
            current: Some(495.0),
            previous: Some(480.0),
            delta: Some(15.0),
            current_n: 32,
            previous_n: 28,
            undated: 0,
        };

        terminal
            .draw(|frame| {
                let area = frame.area();
                draw_market_metrics(frame, area, &metrics, &trend);
            })
            .unwrap();

        let rows = buffer_rows(&terminal).join("\n");
        assert!(rows.contains("342 fiches"), "expected fiche count, got:\n{rows}");
        assert!(rows.contains("TJM connu 210/342"), "expected TJM coverage, got:\n{rows}");
        assert!(rows.contains("min 250"), "expected min TJM, got:\n{rows}");
        assert!(rows.contains("max 900"), "expected max TJM, got:\n{rows}");
        assert!(!rows.contains("n="), "trend sample sizes should no longer render, got:\n{rows}");
        assert!(!rows.contains("p25"), "gauge legend should no longer render, got:\n{rows}");
        assert!(!rows.contains("p75"), "gauge legend should no longer render, got:\n{rows}");
    }

    fn digest(summary_short: &str, content: &str) -> Digest {
        Digest {
            summary_short: summary_short.to_string(),
            content: content.to_string(),
            ..Default::default()
        }
    }

    /// The bug this guards: `digest_scroll_by` has no upper bound of its
    /// own (see its doc comment) — before this fix, scrolling past the end
    /// of a short digest just rendered blank space forever. The clamp now
    /// lives here, where the render width/height needed to know the true
    /// last line is actually available.
    #[test]
    fn draw_digest_clamps_scroll_to_the_wrapped_line_count() {
        let backend = TestBackend::new(40, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        let d = digest("one\ntwo\nthree", "");
        let mut scroll = u16::MAX;

        terminal
            .draw(|frame| {
                let area = frame.area();
                draw_digest(frame, area, Some(&d), &mut scroll, false);
            })
            .unwrap();

        // 3 content lines can never need more than a handful of rows of
        // scroll offset, however large the input was.
        assert!(scroll < 10, "expected scroll to be clamped near the content, got {scroll}");
    }

    #[test]
    fn draw_digest_full_clamps_scroll_to_the_wrapped_line_count() {
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let d = digest("", "## Résumé\nune ligne\nune autre ligne");
        let mut scroll = u16::MAX;

        terminal
            .draw(|frame| {
                draw_digest_full(frame, Some(&d), &mut scroll);
            })
            .unwrap();

        assert!(scroll < 10, "expected scroll to be clamped near the content, got {scroll}");
    }

    /// A digest longer than the viewport must still let scrolling reach a
    /// non-zero offset — the clamp must bound scroll from above, not pin it
    /// to 0 regardless of content length.
    #[test]
    fn draw_digest_full_allows_scrolling_content_taller_than_the_viewport() {
        let backend = TestBackend::new(40, 6);
        let mut terminal = Terminal::new(backend).unwrap();
        let long_content = (0..50).map(|i| format!("ligne {i}")).collect::<Vec<_>>().join("\n");
        let d = digest("", &long_content);
        let mut scroll = u16::MAX;

        terminal
            .draw(|frame| {
                draw_digest_full(frame, Some(&d), &mut scroll);
            })
            .unwrap();

        assert!(scroll > 0, "expected a tall digest to allow a positive scroll offset");
    }
}
