//! Centred popups drawn on top of whatever view is active.

use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::ui::theme::{bold, centered_rect, ACCENT_COLOR};

/// Small centered popup rendered on top of the Skill Audit view — lets the
/// user type the canonical name `alias_edit.target` should merge into,
/// with a live-filtered, alphabetically sorted list of existing canonical
/// names shown above the input as suggestions.
pub fn draw_alias_edit_overlay(frame: &mut Frame, app: &App) {
    let Some(state) = &app.alias_edit else { return };

    let area = frame.area();
    let popup = centered_rect(
        area,
        area.width.saturating_sub(10).clamp(20, 60),
        (state.suggestions.len() as u16 + 4).min(area.height.saturating_sub(4)),
    );

    frame.render_widget(Clear, popup);

    let mut lines = vec![
        Line::from(format!("Fusionner \"{}\" vers :", state.target)),
        Line::from(format!("> {}", state.input)),
        Line::from(""),
    ];
    for (i, suggestion) in state.suggestions.iter().enumerate() {
        let style = if state.suggestion_index == Some(i) {
            bold().fg(ACCENT_COLOR)
        } else {
            Style::default()
        };
        lines.push(Line::styled(format!("  {suggestion}"), style));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .title("Nouvel alias")
        .border_style(Style::default().fg(ACCENT_COLOR));
    frame.render_widget(Paragraph::new(lines).block(block), popup);
}

/// Static keymap reference — the footer only has room for a handful of
/// hints, and the keymap has grown past what that can carry.
pub fn draw_help_overlay(frame: &mut Frame) {
    let area = frame.area();
    let popup = centered_rect(area, 64, 24);
    frame.render_widget(Clear, popup);

    // A nested `fn`, not a closure: a closure here gets its input/output
    // lifetimes pinned to a single call site by inference, which then
    // fails to unify across the several differently-lived `&'static str`
    // literals below. A plain fn is properly generic over its lifetime.
    fn section(title: &str) -> Line<'_> {
        Line::styled(title, bold().fg(ACCENT_COLOR))
    }
    let lines = vec![
        section("Dashboard"),
        Line::from("  q          quitter"),
        Line::from("  Esc        efface le filtre actif, sinon quitte"),
        Line::from("  r          refresh"),
        Line::from("  a          audit des compétences"),
        Line::from("  /          filtrer le Live Feed"),
        Line::from("  Entrée     détail de la fiche sélectionnée"),
        Line::from("  ↑/↓ j/k    naviguer dans le Live Feed"),
        Line::from("  PgUp/PgDn  défiler par page  ·  Home/End  début/fin"),
        Line::from(""),
        section("Filtre (/)"),
        Line::from("  Entrée valider  ·  Esc annuler  ·  Ctrl+U vider"),
        Line::from(""),
        section("Détail de fiche (Entrée)"),
        Line::from("  Esc/Entrée fermer  ·  o ouvrir l'URL  ·  ↑/↓ défiler"),
        Line::from("  d supprimer la fiche"),
        Line::from(""),
        section("Audit des compétences (a)"),
        Line::from("  ←/→ colonne  ·  ↑/↓ j/k naviguer"),
        Line::from("  Entrée fusionner/renommer  ·  d défusionner"),
        Line::from(""),
        Line::from("Esc remonte d'un niveau ; q quitte (désactivé en saisie)."),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .title("Aide")
        .border_style(Style::default().fg(ACCENT_COLOR));
    frame.render_widget(Paragraph::new(lines).block(block), popup);
}
