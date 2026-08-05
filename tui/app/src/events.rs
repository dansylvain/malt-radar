//! Keyboard dispatch.
//!
//! Dispatch is **by mode first, key second**. That ordering is not
//! cosmetic: text-entry modes must swallow every `Char`, including `q`,
//! `r` and `a`. When this was one flat `match key.code` with `if mode ==`
//! guards, the only thing keeping the alias overlay from triggering a
//! refresh on the letter "r" was that its `Char(c)` arm happened to be
//! declared above the `Char('r')` arm — a property no compiler checks and
//! no reader can see. Per-mode functions make the swallowing structural.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc;

use crate::app::{App, AuditPanel, Focus, Mode};
use crate::tasks::{AliasCommand, FicheCommand};

/// The channels a key handler is allowed to write to. Bundled so handlers
/// take one argument and can be driven from tests with throwaway channels.
pub struct Commands<'a> {
    pub refresh_tx: &'a mpsc::UnboundedSender<()>,
    pub alias_cmd_tx: &'a mpsc::UnboundedSender<AliasCommand>,
    pub fiche_cmd_tx: &'a mpsc::UnboundedSender<FicheCommand>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Quit,
}

const PAGE_JUMP: usize = 10;

pub fn handle_key(app: &mut App, key: KeyEvent, cmds: &Commands) -> Flow {
    // Checked before mode dispatch so it works even inside a text-entry
    // mode: in raw mode the terminal doesn't handle Ctrl+C for us, and
    // without this arm the muscle-memory kill simply does nothing.
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Flow::Quit;
    }

    match app.mode {
        Mode::Dashboard => handle_dashboard_key(app, key, cmds),
        Mode::SkillAudit => handle_skill_audit_key(app, key, cmds),
        Mode::AliasEdit => handle_alias_edit_key(app, key, cmds),
        Mode::FeedFilter => handle_feed_filter_key(app, key),
        Mode::FicheDetail => handle_fiche_detail_key(app, key, cmds),
        Mode::Help => handle_help_key(app, key),
        Mode::DigestFull => handle_digest_full_key(app, key),
    }
}

fn request_refresh(app: &mut App, cmds: &Commands) {
    app.loading = true;
    app.status_line = "refresh...".to_string();
    let _ = cmds.refresh_tx.send(());
}

fn handle_dashboard_key(app: &mut App, key: KeyEvent, cmds: &Commands) -> Flow {
    match key.code {
        KeyCode::Char('q') => return Flow::Quit,
        // `Esc` backs out of an active filter before it quits — a filtered
        // dashboard is a modified view, and quitting on the first `Esc`
        // would throw it away with no way back short of `r`.
        KeyCode::Esc => {
            if app.has_active_feed_filter() {
                app.clear_feed_filter();
            } else {
                return Flow::Quit;
            }
        }
        KeyCode::Char('r') => request_refresh(app, cmds),
        KeyCode::Char('a') => app.toggle_skill_audit(),
        KeyCode::Char('/') => app.enter_feed_filter(),
        KeyCode::Char('?') => app.toggle_help(),
        KeyCode::Tab | KeyCode::BackTab => app.cycle_focus(),
        KeyCode::Enter => match app.focus {
            Focus::LiveFeed => app.open_fiche_detail(),
            Focus::TopSkills => app.apply_selected_skill_filter(),
            Focus::Digest => app.open_digest_full(),
        },
        KeyCode::Down | KeyCode::Char('j') => match app.focus {
            Focus::LiveFeed => app.feed_move(1),
            Focus::TopSkills => app.top_skills_move(1),
            Focus::Digest => app.digest_scroll_by(1),
        },
        KeyCode::Up | KeyCode::Char('k') => match app.focus {
            Focus::LiveFeed => app.feed_move(-1),
            Focus::TopSkills => app.top_skills_move(-1),
            Focus::Digest => app.digest_scroll_by(-1),
        },
        KeyCode::PageDown => match app.focus {
            Focus::LiveFeed => app.feed_move(PAGE_JUMP as isize),
            Focus::TopSkills => app.top_skills_move(PAGE_JUMP as isize),
            Focus::Digest => app.digest_scroll_by(PAGE_JUMP as i32),
        },
        KeyCode::PageUp => match app.focus {
            Focus::LiveFeed => app.feed_move(-(PAGE_JUMP as isize)),
            Focus::TopSkills => app.top_skills_move(-(PAGE_JUMP as isize)),
            Focus::Digest => app.digest_scroll_by(-(PAGE_JUMP as i32)),
        },
        KeyCode::Home => match app.focus {
            Focus::LiveFeed => app.feed_select_first(),
            Focus::TopSkills => app.top_skills_select_first(),
            Focus::Digest => app.digest_scroll = 0,
        },
        // Digest has no `End` behavior: the true last line depends on the
        // wrapped line count, which needs the render width — not known
        // here (same limitation `digest_scroll_by`'s doc comment notes).
        KeyCode::End => match app.focus {
            Focus::LiveFeed => app.feed_select_last(),
            Focus::TopSkills => app.top_skills_select_last(),
            Focus::Digest => {}
        },
        _ => {}
    }
    Flow::Continue
}

/// Overlay on top of the dashboard: `Esc`/`Enter` close it, `o` opens the
/// fiche's URL, arrows/`j`/`k` scroll — the content can overflow a small
/// terminal (a long stack, in particular) — and `d` deletes the fiche
/// (e.g. a bad extraction, like an out-of-bounds TJM slipping through).
fn handle_fiche_detail_key(app: &mut App, key: KeyEvent, cmds: &Commands) -> Flow {
    match key.code {
        KeyCode::Char('q') => return Flow::Quit,
        KeyCode::Esc | KeyCode::Enter => app.close_fiche_detail(),
        KeyCode::Char('o') => app.open_detail_url(),
        KeyCode::Char('d') => app.delete_detail_fiche(cmds.fiche_cmd_tx),
        KeyCode::Down | KeyCode::Char('j') => app.detail_scroll_down(),
        KeyCode::Up | KeyCode::Char('k') => app.detail_scroll_up(),
        _ => {}
    }
    Flow::Continue
}

/// Full-screen view of the current digest's full Markdown — same
/// `Esc`/`Enter`-both-close and `j`/`k`-scroll shape as
/// `handle_fiche_detail_key`, minus the fiche-specific actions (`o`, `d`).
fn handle_digest_full_key(app: &mut App, key: KeyEvent) -> Flow {
    match key.code {
        KeyCode::Char('q') => return Flow::Quit,
        KeyCode::Esc | KeyCode::Enter => app.close_digest_full(),
        KeyCode::Down | KeyCode::Char('j') => app.digest_full_scroll_by(1),
        KeyCode::Up | KeyCode::Char('k') => app.digest_full_scroll_by(-1),
        KeyCode::PageDown => app.digest_full_scroll_by(PAGE_JUMP as i32),
        KeyCode::PageUp => app.digest_full_scroll_by(-(PAGE_JUMP as i32)),
        KeyCode::Home => app.digest_full_scroll = 0,
        _ => {}
    }
    Flow::Continue
}

fn handle_help_key(app: &mut App, key: KeyEvent) -> Flow {
    match key.code {
        KeyCode::Char('q') => return Flow::Quit,
        KeyCode::Esc | KeyCode::Char('?') => app.toggle_help(),
        _ => {}
    }
    Flow::Continue
}

fn handle_skill_audit_key(app: &mut App, key: KeyEvent, cmds: &Commands) -> Flow {
    match key.code {
        KeyCode::Char('q') => return Flow::Quit,
        KeyCode::Esc | KeyCode::Char('a') => app.toggle_skill_audit(),
        KeyCode::Char('r') => request_refresh(app, cmds),
        KeyCode::Enter => app.enter_alias_edit(),
        KeyCode::Char('d') => app.defuse_selected_skill(cmds.alias_cmd_tx),
        KeyCode::Left => app.audit_panel = AuditPanel::Orphans,
        KeyCode::Right => app.audit_panel = AuditPanel::Consolidated,
        KeyCode::Down | KeyCode::Char('j') => app.focused_list_state_mut().select_next(),
        KeyCode::Up | KeyCode::Char('k') => app.focused_list_state_mut().select_previous(),
        KeyCode::PageDown => {
            for _ in 0..PAGE_JUMP {
                app.focused_list_state_mut().select_next();
            }
        }
        KeyCode::PageUp => {
            for _ in 0..PAGE_JUMP {
                app.focused_list_state_mut().select_previous();
            }
        }
        KeyCode::Home => app.focused_list_state_mut().select_first(),
        KeyCode::End => app.focused_list_state_mut().select_last(),
        _ => {}
    }
    Flow::Continue
}

/// Text entry, like `handle_alias_edit_key`: every `Char` is input, never
/// a command. `Ctrl+U` clears the line — checked before the generic
/// `Char(c)` arm so a plain `u` (no modifier) still falls through to it
/// and gets typed normally.
fn handle_feed_filter_key(app: &mut App, key: KeyEvent) -> Flow {
    match key.code {
        KeyCode::Esc => app.cancel_feed_filter(),
        KeyCode::Enter => app.commit_feed_filter(),
        KeyCode::Backspace => app.feed_filter_backspace(),
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => app.feed_filter_clear_line(),
        KeyCode::Char(c) => app.feed_filter_push_char(c),
        _ => {}
    }
    Flow::Continue
}

/// Text entry: every `Char` is input, never a command.
fn handle_alias_edit_key(app: &mut App, key: KeyEvent, cmds: &Commands) -> Flow {
    match key.code {
        KeyCode::Esc => app.cancel_alias_edit(),
        KeyCode::Enter => app.submit_alias_edit(cmds.alias_cmd_tx),
        KeyCode::Backspace => app.alias_edit_backspace(),
        KeyCode::Up => app.alias_edit_move_suggestion(-1),
        KeyCode::Down => app.alias_edit_move_suggestion(1),
        KeyCode::Char(c) => app.alias_edit_push_char(c),
        _ => {}
    }
    Flow::Continue
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::{FetchOutcome, FICHES_LIMIT};
    use chrono::{Duration, Utc};
    use malt_radar_core::{Digest, Fiche, SkillFilter};

    fn digest(id: i64, content: &str) -> Digest {
        Digest {
            id,
            content: content.to_string(),
            ..Default::default()
        }
    }

    fn fiche(filename: &str) -> Fiche {
        Fiche {
            filename: filename.to_string(),
            title: Some(filename.to_string()),
            ..Default::default()
        }
    }

    fn fiche_with_skills(filename: &str, stack: &[&str]) -> Fiche {
        Fiche {
            filename: filename.to_string(),
            title: Some(filename.to_string()),
            stack: stack.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    fn fiche_with_published(filename: &str, tjm: f64, days_ago: i64) -> Fiche {
        Fiche {
            filename: filename.to_string(),
            title: Some(filename.to_string()),
            tjm_min: Some(tjm),
            tjm_max: Some(tjm),
            published_at: Some((Utc::now() - Duration::days(days_ago)).to_rfc3339()),
            ..Default::default()
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn feed_filter_swallows_q_r_and_a_as_plain_input() {
        let mut app = App::new();
        app.mode = Mode::FeedFilter;
        let (refresh_tx, mut refresh_rx) = mpsc::unbounded_channel::<()>();
        let (alias_tx, _alias_rx) = mpsc::unbounded_channel::<AliasCommand>();
        let (fiche_tx, _fiche_rx) = mpsc::unbounded_channel::<FicheCommand>();
        let cmds = Commands {
            refresh_tx: &refresh_tx,
            alias_cmd_tx: &alias_tx,
            fiche_cmd_tx: &fiche_tx,
        };

        for c in ['q', 'r', 'a'] {
            let flow = handle_key(&mut app, key(KeyCode::Char(c)), &cmds);
            assert_eq!(flow, Flow::Continue);
        }

        assert_eq!(app.mode, Mode::FeedFilter);
        assert_eq!(app.feed.filter_text, "qra");
        assert!(refresh_rx.try_recv().is_err(), "'r' must not have triggered a refresh");
    }

    #[test]
    fn esc_clears_an_active_feed_filter_before_it_quits() {
        let mut app = App::new();
        app.feed.skill_filter = SkillFilter::Named("RAG".to_string());
        let (refresh_tx, _refresh_rx) = mpsc::unbounded_channel::<()>();
        let (alias_tx, _alias_rx) = mpsc::unbounded_channel::<AliasCommand>();
        let (fiche_tx, _fiche_rx) = mpsc::unbounded_channel::<FicheCommand>();
        let cmds = Commands {
            refresh_tx: &refresh_tx,
            alias_cmd_tx: &alias_tx,
            fiche_cmd_tx: &fiche_tx,
        };

        let first = handle_key(&mut app, key(KeyCode::Esc), &cmds);
        assert_eq!(first, Flow::Continue);
        assert!(matches!(app.feed.skill_filter, SkillFilter::Any));

        let second = handle_key(&mut app, key(KeyCode::Esc), &cmds);
        assert_eq!(second, Flow::Quit);
    }

    #[test]
    fn feed_selection_survives_a_refresh_that_prepends_fiches() {
        let mut app = App::new();
        app.apply(FetchOutcome::Ok(vec![fiche("a.md"), fiche("b.md"), fiche("c.md")]));

        app.feed.list_state.select(Some(1));
        assert_eq!(app.resolved_fiches[app.feed.visible[1]].filename, "b.md");

        // Prepends "d.md" ahead of the three already shown — every plain
        // index into resolved_fiches shifts by one.
        app.apply(FetchOutcome::Ok(vec![
            fiche("d.md"),
            fiche("a.md"),
            fiche("b.md"),
            fiche("c.md"),
        ]));

        let selected = app.feed.list_state.selected().expect("selection should survive the refresh");
        assert_eq!(app.resolved_fiches[app.feed.visible[selected]].filename, "b.md");
    }

    #[test]
    fn d_in_fiche_detail_removes_the_fiche_and_closes_the_overlay() {
        let mut app = App::new();
        app.apply(FetchOutcome::Ok(vec![fiche("a.md"), fiche("b.md")]));
        app.feed.list_state.select(Some(0));
        app.open_fiche_detail();
        assert_eq!(app.mode, Mode::FicheDetail);

        let (refresh_tx, _refresh_rx) = mpsc::unbounded_channel::<()>();
        let (alias_tx, _alias_rx) = mpsc::unbounded_channel::<AliasCommand>();
        let (fiche_tx, mut fiche_rx) = mpsc::unbounded_channel::<FicheCommand>();
        let cmds = Commands {
            refresh_tx: &refresh_tx,
            alias_cmd_tx: &alias_tx,
            fiche_cmd_tx: &fiche_tx,
        };

        let flow = handle_key(&mut app, key(KeyCode::Char('d')), &cmds);

        assert_eq!(flow, Flow::Continue);
        assert_eq!(app.mode, Mode::Dashboard);
        assert!(app.fiches.iter().all(|f| f.filename != "a.md"));
        assert!(app.fiches.iter().any(|f| f.filename == "b.md"));
        match fiche_rx.try_recv() {
            Ok(FicheCommand::Delete { filename }) => assert_eq!(filename, "a.md"),
            other => panic!("expected a FicheCommand::Delete for a.md, got {other:?}"),
        }
    }

    #[test]
    fn tab_cycles_focus_through_top_skills_live_feed_and_digest() {
        let mut app = App::new();
        assert_eq!(app.focus, Focus::LiveFeed);
        let (refresh_tx, _refresh_rx) = mpsc::unbounded_channel::<()>();
        let (alias_tx, _alias_rx) = mpsc::unbounded_channel::<AliasCommand>();
        let (fiche_tx, _fiche_rx) = mpsc::unbounded_channel::<FicheCommand>();
        let cmds = Commands {
            refresh_tx: &refresh_tx,
            alias_cmd_tx: &alias_tx,
            fiche_cmd_tx: &fiche_tx,
        };

        handle_key(&mut app, key(KeyCode::Tab), &cmds);
        assert_eq!(app.focus, Focus::Digest);
        handle_key(&mut app, key(KeyCode::Tab), &cmds);
        assert_eq!(app.focus, Focus::TopSkills);
        handle_key(&mut app, key(KeyCode::Tab), &cmds);
        assert_eq!(app.focus, Focus::LiveFeed);
    }

    #[test]
    fn up_down_scroll_the_digest_panel_when_it_has_focus() {
        let mut app = App::new();
        app.focus = Focus::Digest;
        let (refresh_tx, _refresh_rx) = mpsc::unbounded_channel::<()>();
        let (alias_tx, _alias_rx) = mpsc::unbounded_channel::<AliasCommand>();
        let (fiche_tx, _fiche_rx) = mpsc::unbounded_channel::<FicheCommand>();
        let cmds = Commands {
            refresh_tx: &refresh_tx,
            alias_cmd_tx: &alias_tx,
            fiche_cmd_tx: &fiche_tx,
        };

        handle_key(&mut app, key(KeyCode::Down), &cmds);
        assert_eq!(app.digest_scroll, 1);
        handle_key(&mut app, key(KeyCode::Down), &cmds);
        assert_eq!(app.digest_scroll, 2);
        handle_key(&mut app, key(KeyCode::Up), &cmds);
        assert_eq!(app.digest_scroll, 1);
        // Clamped at 0, never wraps negative.
        handle_key(&mut app, key(KeyCode::Up), &cmds);
        handle_key(&mut app, key(KeyCode::Up), &cmds);
        assert_eq!(app.digest_scroll, 0);
    }

    #[test]
    fn enter_on_digest_focus_opens_digest_full_only_when_a_digest_exists() {
        let mut app = App::new();
        app.focus = Focus::Digest;
        let (refresh_tx, _refresh_rx) = mpsc::unbounded_channel::<()>();
        let (alias_tx, _alias_rx) = mpsc::unbounded_channel::<AliasCommand>();
        let (fiche_tx, _fiche_rx) = mpsc::unbounded_channel::<FicheCommand>();
        let cmds = Commands {
            refresh_tx: &refresh_tx,
            alias_cmd_tx: &alias_tx,
            fiche_cmd_tx: &fiche_tx,
        };

        // No digest yet: Enter is a no-op, mode stays Dashboard.
        handle_key(&mut app, key(KeyCode::Enter), &cmds);
        assert_eq!(app.mode, Mode::Dashboard);

        app.digest = Some(digest(1, "## Résumé exécutif\n..."));
        handle_key(&mut app, key(KeyCode::Enter), &cmds);
        assert_eq!(app.mode, Mode::DigestFull);

        // Esc and Enter both close it, back to Dashboard.
        handle_key(&mut app, key(KeyCode::Esc), &cmds);
        assert_eq!(app.mode, Mode::Dashboard);
    }

    #[test]
    fn up_down_scroll_digest_full_independently_of_the_panel_scroll() {
        let mut app = App::new();
        app.digest = Some(digest(1, "## Résumé exécutif\n..."));
        app.mode = Mode::DigestFull;
        app.digest_scroll = 3; // dashboard panel scroll, must stay untouched
        let (refresh_tx, _refresh_rx) = mpsc::unbounded_channel::<()>();
        let (alias_tx, _alias_rx) = mpsc::unbounded_channel::<AliasCommand>();
        let (fiche_tx, _fiche_rx) = mpsc::unbounded_channel::<FicheCommand>();
        let cmds = Commands {
            refresh_tx: &refresh_tx,
            alias_cmd_tx: &alias_tx,
            fiche_cmd_tx: &fiche_tx,
        };

        handle_key(&mut app, key(KeyCode::Down), &cmds);
        handle_key(&mut app, key(KeyCode::Down), &cmds);
        assert_eq!(app.digest_full_scroll, 2);
        handle_key(&mut app, key(KeyCode::Up), &cmds);
        assert_eq!(app.digest_full_scroll, 1);
        assert_eq!(app.digest_scroll, 3);
    }

    #[test]
    fn enter_on_top_skills_filters_the_feed_and_moves_focus_there() {
        let mut app = App::new();
        app.apply(FetchOutcome::Ok(vec![
            fiche_with_skills("a.md", &["RAG"]),
            fiche_with_skills("b.md", &["RAG"]),
            fiche_with_skills("c.md", &["Python"]),
        ]));
        app.focus = Focus::TopSkills;
        app.top_skills_state.select(Some(0)); // ranked by count: RAG (2) before Python (1)

        let (refresh_tx, _refresh_rx) = mpsc::unbounded_channel::<()>();
        let (alias_tx, _alias_rx) = mpsc::unbounded_channel::<AliasCommand>();
        let (fiche_tx, _fiche_rx) = mpsc::unbounded_channel::<FicheCommand>();
        let cmds = Commands {
            refresh_tx: &refresh_tx,
            alias_cmd_tx: &alias_tx,
            fiche_cmd_tx: &fiche_tx,
        };

        let flow = handle_key(&mut app, key(KeyCode::Enter), &cmds);

        assert_eq!(flow, Flow::Continue);
        assert_eq!(app.focus, Focus::LiveFeed);
        assert!(matches!(&app.feed.skill_filter, SkillFilter::Named(name) if name == "RAG"));
        assert_eq!(app.feed.visible.len(), 2);
    }

    #[test]
    fn top_skills_selection_survives_a_refresh_that_reranks_the_list() {
        let mut app = App::new();
        app.apply(FetchOutcome::Ok(vec![
            fiche_with_skills("a.md", &["RAG"]),
            fiche_with_skills("b.md", &["Python"]),
            fiche_with_skills("c.md", &["Python"]),
        ]));
        // Ranked by count: Python (2) before RAG (1).
        let python_idx = app.metrics.top_skills.iter().position(|s| s.name == "Python").unwrap();
        app.top_skills_state.select(Some(python_idx));

        // A refresh that makes RAG the new leader (3 mentions) re-ranks the
        // list ahead of Python — a plain index would now silently land on
        // an unrelated row.
        app.apply(FetchOutcome::Ok(vec![
            fiche_with_skills("a.md", &["RAG"]),
            fiche_with_skills("d.md", &["RAG"]),
            fiche_with_skills("e.md", &["RAG"]),
            fiche_with_skills("b.md", &["Python"]),
            fiche_with_skills("c.md", &["Python"]),
        ]));

        let selected = app.top_skills_state.selected().expect("selection should survive the refresh");
        assert_eq!(app.metrics.top_skills[selected].name, "Python");
    }

    #[test]
    fn digest_ok_sets_digest_and_resets_scroll_on_a_new_digest() {
        let mut app = App::new();
        app.digest_scroll = 5;
        app.digest_full_scroll = 9;

        app.apply(FetchOutcome::DigestOk(Some(digest(1, "## Résumé exécutif\n..."))));

        assert_eq!(app.digest.as_ref().map(|d| d.id), Some(1));
        assert_eq!(app.digest_scroll, 0);
        assert_eq!(app.digest_full_scroll, 0);
    }

    #[test]
    fn digest_ok_preserves_scroll_when_the_same_digest_refetches() {
        let mut app = App::new();
        app.apply(FetchOutcome::DigestOk(Some(digest(1, "content"))));
        app.digest_scroll = 7;
        app.digest_full_scroll = 4;

        app.apply(FetchOutcome::DigestOk(Some(digest(1, "content"))));

        assert_eq!(app.digest_scroll, 7);
        assert_eq!(app.digest_full_scroll, 4);
    }

    #[test]
    fn digest_ok_none_clears_digest_and_resets_scroll() {
        let mut app = App::new();
        app.apply(FetchOutcome::DigestOk(Some(digest(1, "content"))));
        app.digest_scroll = 3;
        app.digest_full_scroll = 6;

        app.apply(FetchOutcome::DigestOk(None));

        assert!(app.digest.is_none());
        assert_eq!(app.digest_scroll, 0);
        assert_eq!(app.digest_full_scroll, 0);
    }

    #[test]
    fn trend_delta_is_withheld_when_the_fetch_hits_the_fiches_limit() {
        // A fetch landing exactly on FICHES_LIMIT may mean the aggregator's
        // mtime-sorted, limit-capped `GET /fiches` truncated the older
        // (7-14 day) window — the delta would then be silently wrong but
        // plausible-looking, so `App::recompute` withholds it. Build enough
        // dated, priced fiches on both sides of the split that a real delta
        // WOULD be computed if the guard weren't there.
        let mut fiches = Vec::new();
        for i in 0..(FICHES_LIMIT as usize) {
            let (days_ago, tjm) = if i % 2 == 0 { (1, 500.0) } else { (9, 400.0) };
            fiches.push(fiche_with_published(&format!("f{i}.md"), tjm, days_ago));
        }

        let mut app = App::new();
        app.apply(FetchOutcome::Ok(fiches));

        assert!(app.trend.current.is_some());
        assert!(app.trend.previous.is_some());
        assert_eq!(app.trend.delta, None);
    }
}
