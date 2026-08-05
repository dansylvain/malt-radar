//! Full detail of a single fiche, as an overlay on top of the dashboard.
//!
//! Deliberately frontmatter-only: the fiche's markdown body is just a
//! `# {title}` heading (see `worker/fiche.py::render_fiche`) — there's no
//! extra prose to go fetch. The URL is rendered on its own line, in full,
//! never truncated: terminals like Windows Terminal detect and make it
//! ctrl-clickable on their own, and `o` opens it directly (see
//! `crate::browser`).

use malt_radar_core::is_sensitive_skill;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::App;
use crate::ui::theme::{base_style, bold, centered_rect, panel_title, AMBER, BG, RED, TEAL};

pub fn draw_fiche_detail(frame: &mut Frame, app: &mut App) {
    let Some(fiche) = app.detail_fiche() else { return };

    let area = frame.area();
    let popup = centered_rect(area, 70, 18);
    frame.render_widget(Clear, popup);

    let title = fiche.title.clone().unwrap_or_else(|| "(sans titre)".to_string());
    let tjm = match (fiche.tjm_min, fiche.tjm_max) {
        (Some(min), Some(max)) => format!("{min:.0} – {max:.0} EUR/j"),
        (Some(v), None) | (None, Some(v)) => format!("{v:.0} EUR/j"),
        (None, None) => "n/a".to_string(),
    };
    let duree = match (fiche.duree_valeur, fiche.duree_unite.as_deref()) {
        (Some(v), Some(unite)) => format!("{v} {unite}"),
        (Some(v), None) => v.to_string(),
        (None, _) => "n/a".to_string(),
    };
    let source_line = match (&fiche.source, &fiche.published_at) {
        (Some(source), Some(published)) => format!("{source} · publiée le {published}"),
        (Some(source), None) => source.clone(),
        (None, Some(published)) => format!("publiée le {published}"),
        (None, None) => "source inconnue".to_string(),
    };
    let separator = "─".repeat(popup.width.saturating_sub(2) as usize);

    // Stack tags as separate spans, not one joined string, so a
    // security-sensitive tag (EDR/SIEM/XDR/Cybersécurité) can be picked out
    // in red — same treatment as the Live Feed row this popup expands on.
    let mut stack_spans = vec![Span::raw("Stack       ")];
    if fiche.stack.is_empty() {
        stack_spans.push(Span::raw("(stack non extraite)"));
    } else {
        for (i, skill) in fiche.stack.iter().enumerate() {
            if i > 0 {
                stack_spans.push(Span::raw(" · "));
            }
            let style = if is_sensitive_skill(skill) { Style::default().fg(RED) } else { Style::default() };
            stack_spans.push(Span::styled(skill.clone(), style));
        }
    }

    let mut lines = vec![
        Line::styled(title, bold()),
        Line::from(separator.clone()),
        Line::from(format!("Société     {}", fiche.company.as_deref().unwrap_or("n/a"))),
        Line::from(format!("Lieu        {}", fiche.location.as_deref().unwrap_or("n/a"))),
        Line::from(vec![Span::raw("TJM         "), Span::styled(tjm, Style::default().fg(AMBER))]),
        Line::from(format!("Durée       {duree}")),
        Line::from(format!("Niveau      {}", fiche.niveau.as_deref().unwrap_or("n/a"))),
//        Line::from(format!("Catégorie   {}", fiche.categorie.as_deref().unwrap_or("n/a"))),
        Line::from(stack_spans),
        Line::from(format!("Source      {source_line}")),
        Line::from(separator),
    ];
    match fiche.url.clone() {
        Some(url) => lines.push(Line::styled(url, Style::default().fg(TEAL))),
        None => lines.push(Line::from("(pas d'URL)")),
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        ratatui::text::Span::styled("[Esc/Entrée]", bold()),
        ratatui::text::Span::raw(" fermer  "),
        ratatui::text::Span::styled("[o]", bold()),
        ratatui::text::Span::raw(" ouvrir  "),
        ratatui::text::Span::styled("[↑/↓]", bold()),
        ratatui::text::Span::raw(" défiler  "),
        ratatui::text::Span::styled("[d]", bold()),
        ratatui::text::Span::raw(" supprimer"),
    ]));

    // Teal: this overlay only opens from the Live Feed, so it borrows the
    // Live Feed's brand-teal interaction colour rather than a colour of its
    // own.
    let block = Block::default()
        .borders(Borders::ALL)
        .title(panel_title("Détail de la fiche", TEAL))
        .border_style(Style::default().fg(TEAL))
        .style(Style::default().bg(BG));
    let inner = block.inner(popup);

    let content = Paragraph::new(lines).style(base_style()).wrap(Wrap { trim: false });
    let total_lines = content.line_count(inner.width.max(1)) as u16;
    app.detail_scroll = app.detail_scroll.min(total_lines.saturating_sub(inner.height));

    let paragraph = content.scroll((app.detail_scroll, 0)).block(block);
    frame.render_widget(paragraph, popup);
}
