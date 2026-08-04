//! Malt-Radar TUI (Phase 4): reads the aggregator's `GET /fiches` over
//! Tailscale and renders it. No local state beyond what's fetched — the
//! client machine never touches the DB or the filesystem on the VPS/Pi5.
//!
//! Deliberately thin for now: `Market Metrics` and `Top Skills` are
//! computed client-side from the raw fiche list (`malt_radar_core::compute_metrics`)
//! rather than read from a weekly aggregation, because Phase 3 (the service
//! that would produce that aggregation) doesn't exist yet. The digest panel
//! is a placeholder for the same reason.

use std::collections::HashMap;
use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use malt_radar_client::{AggregatorClient, AggregatorConfig};
use malt_radar_core::{
    alias_keys_targeting, compute_metrics, resolve_fiche_skills, skill_audit, truncate_label, Fiche, MarketMetrics,
    SkillAlias, SkillCount,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::{Frame, Terminal};
use tokio::sync::mpsc;

const REFRESH_INTERVAL: Duration = Duration::from_secs(60);
const TOP_SKILLS: usize = 5;
const SKILL_BAR_WIDTH: usize = 24;
const SKILL_NAME_WIDTH: usize = 16;
const ACCENT_COLOR: Color = Color::Cyan;
const FICHES_LIMIT: u32 = 100;
const POLL_INTERVAL: Duration = Duration::from_millis(150);
const MAX_ALIAS_SUGGESTIONS: usize = 8;

enum FetchOutcome {
    Ok(Vec<Fiche>),
    Err(String),
    AliasesOk(Vec<SkillAlias>),
    AliasWriteErr(String),
}

/// Sent to `alias_write_loop`. Both variants use the *cascade* pattern:
/// a merge/rename or a "defuse" (undo) touches every alias row pointing
/// at the same canonical name, not just one — see `App::submit_alias_edit`
/// and `App::defuse_selected_skill`.
enum AliasCommand {
    Upsert { alias: String, canonical: String },
    Delete { alias: String },
}

/// `SkillAudit` is a full-screen scrollable dump of every distinct skill
/// name + mention count (see `malt_radar_core::skill_audit`) — the Top
/// Skills panel only ever shows the top 5 + "Autres", which hides the
/// long-tail name variants that need merging. `AliasEdit` is a small
/// overlay on top of `SkillAudit` (see `App::alias_edit`) for doing that
/// merge live, without leaving the TUI.
#[derive(PartialEq)]
enum Mode {
    Dashboard,
    SkillAudit,
    AliasEdit,
}

/// `SkillAudit` splits into two columns (`←`/`→` switches focus): skills
/// with no alias pointing at them ("orphelines" — raw, unmerged) on the
/// left, skills that are the target of one or more aliases ("groupes
/// fusionnés") on the right. Without this split, merged and unmerged rows
/// looked identical in one flat list, and — since `skill_audit` re-sorts
/// by count on every recompute — the selection *index* could silently
/// land on a different, unrelated row right after a merge, making it look
/// like merged groups couldn't be selected at all.
#[derive(PartialEq, Clone, Copy)]
enum AuditPanel {
    Orphans,
    Consolidated,
}

/// Live state for the `AliasEdit` overlay. Not part of `Mode` itself (which
/// stays a plain unit-variant enum, comparable with `==` everywhere it
/// already is) — this changes on every keystroke, so it lives in its own
/// `Option` field instead, `Some` iff `mode == Mode::AliasEdit`.
struct AliasEditState {
    /// The `skill_audit` row selected when Enter was pressed, cloned (not
    /// stored as an index) so it stays valid even if a background refresh
    /// reshuffles `skill_audit` while the overlay is open.
    target: String,
    input: String,
    suggestions: Vec<String>,
    suggestion_index: Option<usize>,
}

impl AliasEditState {
    fn new(target: String) -> Self {
        Self {
            target,
            input: String::new(),
            suggestions: Vec::new(),
            suggestion_index: None,
        }
    }
}

/// Re-selects by *name*, not index — a recompute can resort/reshuffle the
/// list (a merge moves the target to a higher count, a defuse splits one
/// row into several), and following the old numeric index would silently
/// land on an unrelated row. Falls back to a clamped index only when the
/// previous selection's name is gone entirely (or there wasn't one).
fn reselect(list_state: &mut ListState, items: &[SkillCount], previous_name: Option<&str>) {
    if items.is_empty() {
        list_state.select(None);
        return;
    }
    if let Some(name) = previous_name {
        if let Some(idx) = items.iter().position(|s| s.name == name) {
            list_state.select(Some(idx));
            return;
        }
    }
    let clamped = list_state.selected().unwrap_or(0).min(items.len() - 1);
    list_state.select(Some(clamped));
}

struct App {
    /// Exactly as fetched from the aggregator — never mutated. Resolution
    /// happens fresh into `resolved_fiches` on every fetch/alias change,
    /// never in place, so fiches themselves stay a faithful record of what
    /// the LLM actually extracted.
    fiches: Vec<Fiche>,
    aliases: HashMap<String, String>,
    resolved_fiches: Vec<Fiche>,
    metrics: MarketMetrics,
    /// Full uncapped distribution — kept around only to source the
    /// alias-edit overlay's suggestions; the Skill Audit view itself
    /// renders `audit_orphans`/`audit_consolidated` instead.
    skill_audit: Vec<SkillCount>,
    audit_orphans: Vec<SkillCount>,
    audit_consolidated: Vec<SkillCount>,
    orphans_list_state: ListState,
    consolidated_list_state: ListState,
    audit_panel: AuditPanel,
    mode: Mode,
    alias_edit: Option<AliasEditState>,
    status_line: String,
    loading: bool,
}

impl App {
    fn new() -> Self {
        Self {
            fiches: Vec::new(),
            aliases: HashMap::new(),
            resolved_fiches: Vec::new(),
            metrics: MarketMetrics::default(),
            skill_audit: Vec::new(),
            audit_orphans: Vec::new(),
            audit_consolidated: Vec::new(),
            orphans_list_state: ListState::default(),
            consolidated_list_state: ListState::default(),
            audit_panel: AuditPanel::Orphans,
            mode: Mode::Dashboard,
            alias_edit: None,
            status_line: "chargement des fiches...".to_string(),
            loading: true,
        }
    }

    fn focused_items(&self) -> &[SkillCount] {
        match self.audit_panel {
            AuditPanel::Orphans => &self.audit_orphans,
            AuditPanel::Consolidated => &self.audit_consolidated,
        }
    }

    fn focused_list_state_mut(&mut self) -> &mut ListState {
        match self.audit_panel {
            AuditPanel::Orphans => &mut self.orphans_list_state,
            AuditPanel::Consolidated => &mut self.consolidated_list_state,
        }
    }

    fn selected_skill_name(&self) -> Option<String> {
        let items = self.focused_items();
        let state = match self.audit_panel {
            AuditPanel::Orphans => &self.orphans_list_state,
            AuditPanel::Consolidated => &self.consolidated_list_state,
        };
        state.selected().and_then(|i| items.get(i)).map(|s| s.name.clone())
    }

    /// Re-derives everything downstream of `fiches`/`aliases`. Called after
    /// every fetch and after every alias write (no network round-trip
    /// needed for the recompute itself — it's pure, over already-fetched
    /// data).
    fn recompute(&mut self) {
        self.resolved_fiches = resolve_fiche_skills(&self.fiches, &self.aliases);
        self.metrics = compute_metrics(&self.resolved_fiches, TOP_SKILLS);
        self.skill_audit = skill_audit(&self.resolved_fiches);

        let previous_orphan = self
            .orphans_list_state
            .selected()
            .and_then(|i| self.audit_orphans.get(i))
            .map(|s| s.name.clone());
        let previous_consolidated = self
            .consolidated_list_state
            .selected()
            .and_then(|i| self.audit_consolidated.get(i))
            .map(|s| s.name.clone());

        let (orphans, consolidated): (Vec<SkillCount>, Vec<SkillCount>) = self
            .skill_audit
            .iter()
            .cloned()
            .partition(|s| alias_keys_targeting(&s.name, &self.aliases).is_empty());
        self.audit_orphans = orphans;
        self.audit_consolidated = consolidated;

        reselect(&mut self.orphans_list_state, &self.audit_orphans, previous_orphan.as_deref());
        reselect(
            &mut self.consolidated_list_state,
            &self.audit_consolidated,
            previous_consolidated.as_deref(),
        );
    }

    fn apply(&mut self, outcome: FetchOutcome) {
        match outcome {
            FetchOutcome::Ok(fiches) => {
                self.loading = false;
                self.fiches = fiches;
                self.status_line = format!("{} fiches chargées", self.fiches.len());
                self.recompute();
            }
            FetchOutcome::Err(message) => {
                self.loading = false;
                self.status_line = format!("erreur: {message}");
            }
            // Best-effort: aliases refresh independently of fiches (see
            // fetch_loop) so a missing/failed endpoint never blanks the
            // dashboard — it just leaves `aliases` stale until it works.
            FetchOutcome::AliasesOk(aliases) => {
                self.aliases = aliases.into_iter().map(|a| (a.alias_key, a.canonical)).collect();
                self.recompute();
            }
            FetchOutcome::AliasWriteErr(message) => {
                // The optimistic local edit already happened; this just
                // surfaces that the server-side write failed. It'll
                // self-heal on the next periodic aliases refetch.
                self.status_line = format!("erreur alias: {message}");
            }
        }
    }

    fn toggle_skill_audit(&mut self) {
        self.mode = match self.mode {
            Mode::Dashboard => {
                reselect(&mut self.orphans_list_state, &self.audit_orphans, None);
                reselect(&mut self.consolidated_list_state, &self.audit_consolidated, None);
                Mode::SkillAudit
            }
            Mode::SkillAudit | Mode::AliasEdit => {
                self.alias_edit = None;
                Mode::Dashboard
            }
        };
    }

    fn enter_alias_edit(&mut self) {
        let Some(name) = self.selected_skill_name() else {
            return;
        };
        self.alias_edit = Some(AliasEditState::new(name));
        self.mode = Mode::AliasEdit;
    }

    fn cancel_alias_edit(&mut self) {
        self.alias_edit = None;
        self.mode = Mode::SkillAudit;
    }

    fn alias_edit_push_char(&mut self, c: char) {
        if let Some(state) = &mut self.alias_edit {
            state.input.push(c);
            state.suggestion_index = None;
        }
        self.update_alias_suggestions();
    }

    fn alias_edit_backspace(&mut self) {
        if let Some(state) = &mut self.alias_edit {
            state.input.pop();
            state.suggestion_index = None;
        }
        self.update_alias_suggestions();
    }

    /// Suggestions come from `skill_audit`'s names — already deduped,
    /// already the current *terminal* display forms — not from
    /// `aliases.values()`, which can contain non-terminal canonical
    /// strings left behind by an earlier rename (see `submit_alias_edit`).
    fn update_alias_suggestions(&mut self) {
        let Some(state) = &mut self.alias_edit else { return };
        if state.input.is_empty() {
            state.suggestions.clear();
            return;
        }
        let needle = state.input.to_lowercase();
        let mut matches: Vec<String> = self
            .skill_audit
            .iter()
            .map(|s| s.name.clone())
            .filter(|name| name.to_lowercase().contains(&needle))
            .collect();
        matches.sort();
        matches.truncate(MAX_ALIAS_SUGGESTIONS);
        state.suggestions = matches;
    }

    /// Moves the highlighted suggestion by `delta` (clamped, not wrapping)
    /// and live-fills `input` with it — a combobox preview, never an
    /// auto-submit; only `Enter` submits.
    fn alias_edit_move_suggestion(&mut self, delta: isize) {
        let Some(state) = &mut self.alias_edit else { return };
        if state.suggestions.is_empty() {
            return;
        }
        let current = state.suggestion_index.map(|i| i as isize).unwrap_or(-1);
        let next = (current + delta).clamp(0, state.suggestions.len() as isize - 1) as usize;
        state.suggestion_index = Some(next);
        state.input = state.suggestions[next].clone();
    }

    /// The merge/rename operation. Deliberately the same code path whether
    /// this is a first-time merge or a rename of an already-merged group:
    /// every existing alias row pointing at `target` gets its canonical
    /// updated (cascade-on-write), plus a fresh row for `target` itself
    /// (so a future raw re-extraction of that literal string also folds
    /// into the new canonical, not just the variants merged so far).
    fn submit_alias_edit(&mut self, alias_cmd_tx: &mpsc::UnboundedSender<AliasCommand>) {
        let Some(state) = self.alias_edit.take() else { return };
        let new_canonical = state.input.trim().to_string();
        if new_canonical.is_empty() {
            // No-op: put the state back so the overlay stays open.
            self.alias_edit = Some(state);
            return;
        }

        let target_key = state.target.to_lowercase();
        let mut keys = alias_keys_targeting(&state.target, &self.aliases);
        keys.push(target_key);

        for key in keys {
            self.aliases.insert(key.clone(), new_canonical.clone());
            let _ = alias_cmd_tx.send(AliasCommand::Upsert {
                alias: key,
                canonical: new_canonical.clone(),
            });
        }
        self.recompute();
        self.mode = Mode::SkillAudit;
        // Follow the merge result into the Consolidated column — without
        // this the selection would stay wherever it was in Orphans, which
        // is exactly the "can't find what I just merged" confusion this
        // two-column split exists to avoid.
        self.audit_panel = AuditPanel::Consolidated;
        reselect(&mut self.consolidated_list_state, &self.audit_consolidated, Some(&new_canonical));
    }

    /// The "undo": deletes every alias row pointing at the selected row's
    /// name, scattering its raw variants back to resolving as themselves.
    fn defuse_selected_skill(&mut self, alias_cmd_tx: &mpsc::UnboundedSender<AliasCommand>) {
        let Some(target) = self.selected_skill_name() else {
            return;
        };
        let keys = alias_keys_targeting(&target, &self.aliases);
        if keys.is_empty() {
            return;
        }
        for key in keys {
            self.aliases.remove(&key);
            let _ = alias_cmd_tx.send(AliasCommand::Delete { alias: key });
        }
        self.recompute();
        // The defused group's own name reappears as a plain orphan (it
        // just lost its alias row); follow it there.
        self.audit_panel = AuditPanel::Orphans;
        reselect(&mut self.orphans_list_state, &self.audit_orphans, Some(&target));
    }
}

async fn fetch_loop(
    client: AggregatorClient,
    mut refresh_rx: mpsc::UnboundedReceiver<()>,
    outcome_tx: mpsc::UnboundedSender<FetchOutcome>,
) {
    let mut ticker = tokio::time::interval(REFRESH_INTERVAL);
    loop {
        tokio::select! {
            _ = ticker.tick() => {}
            received = refresh_rx.recv() => {
                if received.is_none() {
                    break;
                }
            }
        }

        let outcome = match client.list_fiches(FICHES_LIMIT).await {
            Ok(fiches) => FetchOutcome::Ok(fiches),
            Err(err) => FetchOutcome::Err(err.to_string()),
        };
        if outcome_tx.send(outcome).is_err() {
            break;
        }

        // Fetched independently and best-effort, deliberately not combined
        // with the fiches fetch above (e.g. via try_join!): the aggregator
        // might not have redeployed the /skill-aliases endpoint yet even
        // after this TUI binary is updated (deploys across the three
        // machines are manual), and a combined fetch would otherwise turn
        // a missing endpoint into a fully blank dashboard.
        if let Ok(aliases) = client.list_skill_aliases().await {
            if outcome_tx.send(FetchOutcome::AliasesOk(aliases)).is_err() {
                break;
            }
        }
    }
}

async fn alias_write_loop(
    client: AggregatorClient,
    mut cmd_rx: mpsc::UnboundedReceiver<AliasCommand>,
    outcome_tx: mpsc::UnboundedSender<FetchOutcome>,
) {
    while let Some(cmd) = cmd_rx.recv().await {
        let result = match cmd {
            AliasCommand::Upsert { alias, canonical } => {
                client.upsert_skill_alias(&alias, &canonical).await.map(|_| ())
            }
            AliasCommand::Delete { alias } => client.delete_skill_alias(&alias).await.map(|_| ()),
        };
        if let Err(err) = result {
            if outcome_tx.send(FetchOutcome::AliasWriteErr(err.to_string())).is_err() {
                break;
            }
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let config = AggregatorConfig::from_env().map_err(|err| {
        anyhow::anyhow!("{err}\n\nRenseigne AGGREGATOR_URL et AGGREGATOR_API_TOKEN dans tui/.env (voir tui/.env.example).")
    })?;
    let client = AggregatorClient::new(config);

    let (refresh_tx, refresh_rx) = mpsc::unbounded_channel::<()>();
    let (outcome_tx, mut outcome_rx) = mpsc::unbounded_channel::<FetchOutcome>();
    let (alias_cmd_tx, alias_cmd_rx) = mpsc::unbounded_channel::<AliasCommand>();
    let fetcher = tokio::spawn(fetch_loop(client.clone(), refresh_rx, outcome_tx.clone()));
    let alias_writer = tokio::spawn(alias_write_loop(client, alias_cmd_rx, outcome_tx));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let run_result = run(&mut terminal, &mut app, &refresh_tx, &alias_cmd_tx, &mut outcome_rx).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    drop(refresh_tx);
    drop(alias_cmd_tx);
    let _ = fetcher.await;
    let _ = alias_writer.await;

    run_result
}

async fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    refresh_tx: &mpsc::UnboundedSender<()>,
    alias_cmd_tx: &mpsc::UnboundedSender<AliasCommand>,
    outcome_rx: &mut mpsc::UnboundedReceiver<FetchOutcome>,
) -> anyhow::Result<()> {
    let mut last_refreshed: Option<Instant> = None;

    loop {
        terminal.draw(|frame| draw(frame, app, last_refreshed))?;

        if event::poll(POLL_INTERVAL)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') if app.mode != Mode::AliasEdit => return Ok(()),
                        KeyCode::Esc if app.mode == Mode::AliasEdit => app.cancel_alias_edit(),
                        KeyCode::Esc if app.mode == Mode::SkillAudit => app.mode = Mode::Dashboard,
                        KeyCode::Esc => return Ok(()),

                        // Mode::AliasEdit — the overlay swallows all typing.
                        KeyCode::Enter if app.mode == Mode::AliasEdit => app.submit_alias_edit(alias_cmd_tx),
                        KeyCode::Backspace if app.mode == Mode::AliasEdit => app.alias_edit_backspace(),
                        KeyCode::Up if app.mode == Mode::AliasEdit => app.alias_edit_move_suggestion(-1),
                        KeyCode::Down if app.mode == Mode::AliasEdit => app.alias_edit_move_suggestion(1),
                        KeyCode::Char(c) if app.mode == Mode::AliasEdit => app.alias_edit_push_char(c),

                        KeyCode::Char('r') => {
                            app.loading = true;
                            app.status_line = "refresh...".to_string();
                            let _ = refresh_tx.send(());
                        }
                        KeyCode::Char('a') => app.toggle_skill_audit(),
                        KeyCode::Enter if app.mode == Mode::SkillAudit => app.enter_alias_edit(),
                        KeyCode::Char('d') if app.mode == Mode::SkillAudit => app.defuse_selected_skill(alias_cmd_tx),
                        KeyCode::Left if app.mode == Mode::SkillAudit => app.audit_panel = AuditPanel::Orphans,
                        KeyCode::Right if app.mode == Mode::SkillAudit => app.audit_panel = AuditPanel::Consolidated,
                        KeyCode::Down | KeyCode::Char('j') if app.mode == Mode::SkillAudit => {
                            app.focused_list_state_mut().select_next();
                        }
                        KeyCode::Up | KeyCode::Char('k') if app.mode == Mode::SkillAudit => {
                            app.focused_list_state_mut().select_previous();
                        }
                        KeyCode::PageDown if app.mode == Mode::SkillAudit => {
                            for _ in 0..10 {
                                app.focused_list_state_mut().select_next();
                            }
                        }
                        KeyCode::PageUp if app.mode == Mode::SkillAudit => {
                            for _ in 0..10 {
                                app.focused_list_state_mut().select_previous();
                            }
                        }
                        KeyCode::Home if app.mode == Mode::SkillAudit => {
                            app.focused_list_state_mut().select_first();
                        }
                        KeyCode::End if app.mode == Mode::SkillAudit => {
                            app.focused_list_state_mut().select_last();
                        }
                        _ => {}
                    }
                }
            }
        }

        while let Ok(outcome) = outcome_rx.try_recv() {
            app.apply(outcome);
            last_refreshed = Some(Instant::now());
        }
    }
}

fn draw(frame: &mut Frame, app: &mut App, last_refreshed: Option<Instant>) {
    // Stateful (`&mut ListState`) rendering for the skill audit view is why
    // `app` is borrowed mutably here, unlike the rest of the dashboard.
    if app.mode == Mode::SkillAudit || app.mode == Mode::AliasEdit {
        draw_skill_audit(frame, app);
        if app.mode == Mode::AliasEdit {
            draw_alias_edit_overlay(frame, app);
        }
        return;
    }

    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(9),
            Constraint::Fill(2),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(root[0]);

    draw_market_metrics(frame, top[0], &app.metrics);
    draw_top_skills(frame, top[1], &app.metrics);
    draw_live_feed(frame, root[1], &app.resolved_fiches);
    draw_digest(frame, root[2]);
    draw_footer(frame, root[3], app, last_refreshed);
}

/// Two columns, `←`/`→` to switch focus between them — orphans (raw,
/// unmerged skill names) on the left, consolidated groups (the target of
/// one or more aliases) on the right. See `AuditPanel`'s doc comment for
/// why this split exists (it's not just cosmetic — it's what makes an
/// already-merged group reliably selectable for `d`).
fn draw_skill_audit(frame: &mut Frame, app: &mut App) {
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

    let focus_style = |focused: bool| {
        if focused {
            (Style::default().fg(ACCENT_COLOR).add_modifier(Modifier::BOLD), "> ")
        } else {
            (Style::default(), "  ")
        }
    };

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
        Span::styled("[←/→]", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" colonne  "),
        Span::styled("[↑/↓ ou j/k]", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" naviguer  "),
        Span::styled("[Entrée]", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" fusionner/renommer  "),
        Span::styled("[d]", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" défusionner  "),
        Span::styled("[a/Esc]", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" retour  "),
        Span::styled("[q]", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" quitter"),
    ]);
    frame.render_widget(Paragraph::new(footer), root[1]);
}

/// Small centered popup rendered on top of the Skill Audit view — lets the
/// user type the canonical name `alias_edit.target` should merge into,
/// with a live-filtered, alphabetically sorted list of existing canonical
/// names shown above the input as suggestions.
fn draw_alias_edit_overlay(frame: &mut Frame, app: &App) {
    let Some(state) = &app.alias_edit else { return };

    let area = frame.area();
    let popup_width = area.width.saturating_sub(10).clamp(20, 60);
    let popup_height = (state.suggestions.len() as u16 + 4).min(area.height.saturating_sub(4));
    let popup = Rect {
        x: (area.width.saturating_sub(popup_width)) / 2,
        y: (area.height.saturating_sub(popup_height)) / 2,
        width: popup_width,
        height: popup_height,
    };

    frame.render_widget(ratatui::widgets::Clear, popup);

    let mut lines = vec![
        Line::from(format!("Fusionner \"{}\" vers :", state.target)),
        Line::from(format!("> {}", state.input)),
        Line::from(""),
    ];
    for (i, suggestion) in state.suggestions.iter().enumerate() {
        let style = if state.suggestion_index == Some(i) {
            Style::default().fg(ACCENT_COLOR).add_modifier(Modifier::BOLD)
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

fn draw_market_metrics(frame: &mut Frame, area: Rect, metrics: &MarketMetrics) {
    let avg_tjm = metrics
        .avg_tjm
        .map(|v| format!("{v:.0} EUR/j"))
        .unwrap_or_else(|| "n/a".to_string());
    let top_skill = metrics
        .top_skills
        .first()
        .map(|s| format!("{} ({}%)", s.name, s.percent_rounded))
        .unwrap_or_else(|| "n/a".to_string());
    let lines = vec![
        Line::from(format!("Fiches : {}", metrics.fiche_count)),
        Line::from(format!("TJM moyen : {avg_tjm}")),
        Line::from(format!("Top skill demandé : {top_skill}")),
    ];
    let paragraph = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Market Metrics"));
    frame.render_widget(paragraph, area);
}

fn draw_top_skills(frame: &mut Frame, area: Rect, metrics: &MarketMetrics) {
    let lines: Vec<Line> = metrics
        .top_skills
        .iter()
        .map(|skill| {
            // Fraction absolue de 100%, pas relative au plus grand count
            // affiché — sinon "Autres" (souvent le plus gros poste) remplit
            // toujours sa barre à 100% quel que soit son vrai pourcentage.
            let filled = ((skill.percent / 100.0) * SKILL_BAR_WIDTH as f64).round() as usize;
            let bar = format!("{}{}", "█".repeat(filled), " ".repeat(SKILL_BAR_WIDTH - filled));
            let name = truncate_label(&skill.name, SKILL_NAME_WIDTH);
            Line::from(vec![
                Span::raw(format!("{name:<SKILL_NAME_WIDTH$}")),
                Span::raw("["),
                Span::styled(bar, Style::default().fg(ACCENT_COLOR)),
                Span::raw(format!("] {}%", skill.percent_rounded)),
            ])
        })
        .collect();
    let paragraph = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Top Skills"));
    frame.render_widget(paragraph, area);
}

fn draw_digest(frame: &mut Frame, area: Rect) {
    let paragraph = Paragraph::new("Digest hebdomadaire : Phase 3 pas encore implémentée.")
        .block(Block::default().borders(Borders::ALL).title("DAILY AI STRATEGY DIGEST"));
    frame.render_widget(paragraph, area);
}

fn draw_live_feed(frame: &mut Frame, area: Rect, fiches: &[Fiche]) {
    let items: Vec<ListItem> = fiches
        .iter()
        .enumerate()
        .map(|(i, fiche)| {
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

            // Sur deux lignes : la ligne stack débordait souvent la largeur
            // du panneau (List ne wrap pas comme Paragraph), d'où la coupure
            // sans retour à la ligne observée.
            let text = Text::from(vec![
                Line::from(format!("{title} — {company} — {tjm}")),
                Line::from(format!("  {stack}")),
            ]);

            // Une ligne sur deux en couleur d'accent (même teinte que les
            // barres de Top Skills) pour distinguer les annonces d'un coup
            // d'œil, sans toucher au fond.
            let style = if i % 2 == 1 {
                Style::default().fg(ACCENT_COLOR)
            } else {
                Style::default()
            };

            ListItem::new(text).style(style)
        })
        .collect();
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Live Feed ({})", fiches.len())),
    );
    frame.render_widget(list, area);
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App, last_refreshed: Option<Instant>) {
    let age = last_refreshed
        .map(|t| format!("{}s", t.elapsed().as_secs()))
        .unwrap_or_else(|| "jamais".to_string());
    let loading_marker = if app.loading { " (chargement...)" } else { "" };
    let line = Line::from(vec![
        Span::styled("[q]", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" quitter  "),
        Span::styled("[r]", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" refresh  "),
        Span::styled("[a]", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" skill audit  "),
        Span::styled(&app.status_line, Style::default().fg(Color::Yellow)),
        Span::raw(format!("  (dernier refresh il y a {age}){loading_marker}")),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}
