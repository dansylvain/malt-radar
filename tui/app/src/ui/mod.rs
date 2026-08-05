//! Rendering. Every function here only *reads* application state, with two
//! exceptions: the `&mut ListState` that ratatui's stateful widgets need to
//! track their own scroll offset, and the scroll fields (`&mut u16`) for
//! digest/detail content, clamped in place against the true wrapped-line
//! count once the render width is known (see
//! `dashboard::draw_digest`/`draw_digest_full`/`detail::draw_fiche_detail`).

pub mod dashboard;
pub mod detail;
pub mod overlay;
pub mod skill_audit;
pub mod theme;

use ratatui::style::Style;
use ratatui::widgets::Block;
use ratatui::Frame;

use crate::app::{App, Mode};
use crate::ui::theme::BG;

pub fn draw(frame: &mut Frame, app: &mut App) {
    // The app's own canvas, painted once behind everything else — see
    // `theme`'s module doc for why this (and only this) is a background
    // fill. Every panel/overlay below either leaves it showing through
    // (Block borders with no `bg` of their own) or repaints it explicitly
    // via `theme::base_style()`.
    frame.render_widget(Block::default().style(Style::default().bg(BG)), frame.area());

    // Stateful (`&mut ListState`) rendering is why `app` is borrowed
    // mutably here, unlike everything downstream.
    match app.mode {
        Mode::SkillAudit | Mode::AliasEdit => {
            skill_audit::draw_skill_audit(frame, app);
            if app.mode == Mode::AliasEdit {
                overlay::draw_alias_edit_overlay(frame, app);
            }
        }
        // `FeedFilter` is rendered as part of the ordinary dashboard — only
        // the footer changes to show the input line — so the Live Feed
        // stays visible and keeps filtering live as the user types.
        Mode::Dashboard | Mode::FeedFilter => dashboard::draw_dashboard(frame, app),
        Mode::FicheDetail => {
            dashboard::draw_dashboard(frame, app);
            detail::draw_fiche_detail(frame, app);
        }
        Mode::Help => {
            dashboard::draw_dashboard(frame, app);
            overlay::draw_help_overlay(frame);
        }
        Mode::DigestFull => {
            dashboard::draw_digest_full(frame, app.digest.as_ref(), &mut app.digest_full_scroll)
        }
    }
}
