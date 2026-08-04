//! Rendering. Every function here only *reads* application state — the
//! single exception is the `&mut ListState` that ratatui's stateful
//! widgets need to track their own scroll offset.

pub mod dashboard;
pub mod detail;
pub mod overlay;
pub mod skill_audit;
pub mod theme;

use ratatui::Frame;

use crate::app::{App, Mode};

pub fn draw(frame: &mut Frame, app: &mut App) {
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
    }
}
