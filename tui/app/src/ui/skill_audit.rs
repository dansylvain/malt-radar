//! The full-screen skill audit view.

use malt_radar_core::alias_keys_targeting;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::Frame;

use crate::app::{App, AuditPanel};
use crate::ui::theme::{bold, focus_style};

/// Two columns, `←`/`→` to switch focus between them — orphans (raw,
/// unmerged skill names) on the left, consolidated groups (the target of
/// one or more aliases) on the right. See `AuditPanel`'s doc comment for
/// why this split exists (it's not just cosmetic — it's what makes an
/// already-merged group reliably selectable for `d`).
pub fn draw_skill_audit(frame: &mut Frame, app: &mut App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Fill(1), Constraint::Length(1)])
        .split(frame.area());

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(root[0]);

    let orphan_items: Vec<ListItem> = app
        .audit_orphans
        .iter()
        .map(|skill| ListItem::new(format!("{:>5}  {}", skill.count, skill.name)))
        .collect();
    let consolidated_items: Vec<ListItem> = app
        .audit_consolidated
        .iter()
        .map(|skill| {
            let variants = alias_keys_targeting(&skill.name, &app.aliases).len();
            let plural = if variants > 1 { "s" } else { "" };
            ListItem::new(format!("{:>5}  {} ({variants} variante{plural})", skill.count, skill.name))
        })
        .collect();

    let (orphan_hl_style, orphan_hl_symbol) = focus_style(app.audit_panel == AuditPanel::Orphans);
    let orphan_list = List::new(orphan_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Compétences seules — {}", app.audit_orphans.len())),
        )
        .highlight_style(orphan_hl_style)
        .highlight_symbol(orphan_hl_symbol);
    frame.render_stateful_widget(orphan_list, columns[0], &mut app.orphans_list_state);

    let (consolidated_hl_style, consolidated_hl_symbol) = focus_style(app.audit_panel == AuditPanel::Consolidated);
    let consolidated_list = List::new(consolidated_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Groupes fusionnés — {}", app.audit_consolidated.len())),
        )
        .highlight_style(consolidated_hl_style)
        .highlight_symbol(consolidated_hl_symbol);
    frame.render_stateful_widget(consolidated_list, columns[1], &mut app.consolidated_list_state);

    let footer = Line::from(vec![
        Span::styled("[←/→]", bold()),
        Span::raw(" colonne  "),
        Span::styled("[↑/↓ ou j/k]", bold()),
        Span::raw(" naviguer  "),
        Span::styled("[Entrée]", bold()),
        Span::raw(" fusionner/renommer  "),
        Span::styled("[d]", bold()),
        Span::raw(" défusionner  "),
        Span::styled("[a/Esc]", bold()),
        Span::raw(" retour  "),
        Span::styled("[q]", bold()),
        Span::raw(" quitter"),
    ]);
    frame.render_widget(Paragraph::new(footer), root[1]);
}
