//! Full detail of a single fiche, as an overlay on top of the dashboard.
//!
//! Deliberately frontmatter-only: the fiche's markdown body is just a
//! `# {title}` heading (see `worker/fiche.py::render_fiche`) — there's no
//! extra prose to go fetch. The URL is rendered on its own line, in full,
//! never truncated: terminals like Windows Terminal detect and make it
//! ctrl-clickable on their own, and `o` opens it directly (see
//! `crate::browser`).

use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::App;
use crate::ui::theme::{bold, centered_rect, ACCENT_COLOR};

pub fn draw_fiche_detail(frame: &mut Frame, app: &App) {
    let Some(fiche) = app.detail_fiche() else { return };

    let area = frame.area();
    let popup = centered_rect(area, 70, 18);
    frame.render_widget(Clear, popup);

    let title = fiche.title.as_deref().unwrap_or("(sans titre)");
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
    let stack = if fiche.stack.is_empty() {
        "(stack non extraite)".to_string()
    } else {
        fiche.stack.join(" · ")
    };
    let source_line = match (&fiche.source, &fiche.published_at) {
        (Some(source), Some(published)) => format!("{source} · publiée le {published}"),
        (Some(source), None) => source.clone(),
        (None, Some(published)) => format!("publiée le {published}"),
        (None, None) => "source inconnue".to_string(),
    };
    let separator = "─".repeat(popup.width.saturating_sub(2) as usize);

    let mut lines = vec![
        Line::styled(title, bold()),
        Line::from(separator.clone()),
        Line::from(format!("Société     {}", fiche.company.as_deref().unwrap_or("n/a"))),
        Line::from(format!("Lieu        {}", fiche.location.as_deref().unwrap_or("n/a"))),
        Line::from(format!("TJM         {tjm}")),
        Line::from(format!("Durée       {duree}")),
        Line::from(format!("Niveau      {}", fiche.niveau.as_deref().unwrap_or("n/a"))),
//        Line::from(format!("Catégorie   {}", fiche.categorie.as_deref().unwrap_or("n/a"))),
        Line::from(format!("Stack       {stack}")),
        Line::from(format!("Source      {source_line}")),
        Line::from(separator),
    ];
    match &fiche.url {
        Some(url) => lines.push(Line::styled(url.as_str(), Style::default().fg(ACCENT_COLOR))),
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

    let block = Block::default()
        .borders(Borders::ALL)
        .title("Détail de la fiche")
        .border_style(Style::default().fg(ACCENT_COLOR));
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.detail_scroll, 0));
    frame.render_widget(paragraph, popup);
}
