//! Centred popups drawn on top of whatever view is active.

use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::ui::theme::{base_style, bold, centered_rect, panel_title, BG, TEAL, VIOLET};

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
            bold().fg(VIOLET)
        } else {
            Style::default()
        };
        lines.push(Line::styled(format!("  {suggestion}"), style));
    }

    // Violet: this popup only ever opens on top of the Skill Audit view,
    // so it carries the same identity colour rather than the brand teal.
    let block = Block::default()
        .borders(Borders::ALL)
        .title(panel_title("Nouvel alias", VIOLET))
        .border_style(Style::default().fg(VIOLET))
        .style(Style::default().bg(BG));
    frame.render_widget(Paragraph::new(lines).style(base_style()).block(block), popup);
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
        Line::styled(title, bold().fg(TEAL))
    }
    let lines = vec![
        section("Dashboard"),
        Line::from("  q          quitter"),
        Line::from("  Esc        efface le filtre actif, sinon quitte"),
        Line::from("  r          refresh"),
        Line::from("  a          audit des compétences"),
        Line::from("  /          filtrer le Live Feed"),
        Line::from("  Tab        change le panneau actif (Top Skills/Live Feed/Digest)"),
        Line::from("  Entrée     détail de la fiche / digest complet selon le panneau actif"),
        Line::from("  ↑/↓ j/k    naviguer (ou défiler le Digest si actif)"),
        Line::from("  PgUp/PgDn  défiler par page  ·  Home/End  début/fin"),
        Line::from(""),
        section("Filtre (/)"),
        Line::from("  Entrée valider  ·  Esc annuler  ·  Ctrl+U vider"),
        Line::from(""),
        section("Détail de fiche (Entrée sur Live Feed)"),
        Line::from("  Esc/Entrée fermer  ·  o ouvrir l'URL  ·  ↑/↓ défiler"),
        Line::from("  d supprimer la fiche"),
        Line::from(""),
        section("Digest complet (Entrée sur le panneau Digest)"),
        Line::from("  Esc/Entrée fermer  ·  ↑/↓ j/k défiler  ·  PgUp/PgDn  ·  Home/End"),
        Line::from(""),
        section("Audit des compétences (a)"),
        Line::from("  ←/→ colonne  ·  ↑/↓ j/k naviguer"),
        Line::from("  Entrée fusionner/renommer  ·  d défusionner"),
        Line::from(""),
        Line::from("Esc remonte d'un niveau ; q quitte (désactivé en saisie)."),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .title(panel_title("Aide", TEAL))
        .border_style(Style::default().fg(TEAL))
        .style(Style::default().bg(BG));
    frame.render_widget(Paragraph::new(lines).style(base_style()).block(block), popup);
}
