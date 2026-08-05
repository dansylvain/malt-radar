//! Application state and the pure transitions over it.
//!
//! Everything here is synchronous and terminal-free: state changes come in
//! as method calls (from `events`) or as `FetchOutcome`s (from `tasks`),
//! and rendering only ever reads. That's what makes the interaction logic
//! testable without a TTY.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use chrono::Utc;
use malt_radar_core::{
    alias_keys_targeting, compute_metrics, filter_fiches, new_filenames, resolve_fiche_skills, skill_audit,
    tjm_trend, Digest, Fiche, MarketMetrics, SkillCount, SkillFilter, TjmTrend, OTHERS_LABEL, TREND_MIN_SAMPLE,
    TREND_WINDOW_DAYS,
};
use ratatui::widgets::ListState;
use tokio::sync::mpsc;

use crate::tasks::{AliasCommand, FetchOutcome, FicheCommand, FICHES_LIMIT};

/// Passed to `compute_metrics` — no cap, so every distinct skill shows in
/// Top Skills (which scrolls now) rather than being folded into "Autres".
/// `compute_metrics`'s own `top_n` parameter and its `OTHERS_LABEL` bucket
/// stay in `core` (tested there, and still meaningful for other callers);
/// this just stops the TUI from invoking it with a cap.
pub const ALL_SKILLS: usize = usize::MAX;
pub const MAX_ALIAS_SUGGESTIONS: usize = 8;

/// `SkillAudit` is a full-screen scrollable dump of every distinct skill
/// name + mention count (see `malt_radar_core::skill_audit`) — the Top
/// Skills panel only ever shows the top 5 + "Autres", which hides the
/// long-tail name variants that need merging. `AliasEdit` is a small
/// overlay on top of `SkillAudit` (see `App::alias_edit`) for doing that
/// merge live, without leaving the TUI.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Mode {
    Dashboard,
    SkillAudit,
    AliasEdit,
    /// A text-entry mode like `AliasEdit`, but rendered *inside* the
    /// dashboard layout (only the footer changes) rather than full-screen
    /// — the Live Feed stays visible and filters live as you type.
    FeedFilter,
    /// A centered overlay on top of the dashboard showing one fiche's full
    /// frontmatter — entered with `Enter` on a Live Feed row.
    FicheDetail,
    /// A centered overlay listing the keymap, since it outgrew what the
    /// one-line footer can show.
    Help,
    /// Full-screen view of the current digest's full Markdown (`content`),
    /// entered with `Enter` while `Focus::Digest` — the dashboard panel
    /// only ever shows `summary_short`, which has no room for the whole
    /// 5-section narrative.
    DigestFull,
}

/// Which dashboard panel arrow-key navigation and `Enter` apply to. Market
/// Metrics never participates — it has no rows or scroll position of its
/// own. Digest does participate despite being read-only content (no
/// selection, no `Enter` action): focusing it is what routes `↑`/`↓` to
/// `digest_scroll` instead of the feed/skills selection.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Focus {
    TopSkills,
    LiveFeed,
    Digest,
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
pub enum AuditPanel {
    Orphans,
    Consolidated,
}

/// Live state for the `AliasEdit` overlay. Not part of `Mode` itself (which
/// stays a plain unit-variant enum, comparable with `==` everywhere it
/// already is) — this changes on every keystroke, so it lives in its own
/// `Option` field instead, `Some` iff `mode == Mode::AliasEdit`.
pub struct AliasEditState {
    /// The `skill_audit` row selected when Enter was pressed, cloned (not
    /// stored as an index) so it stays valid even if a background refresh
    /// reshuffles `skill_audit` while the overlay is open.
    pub target: String,
    pub input: String,
    pub suggestions: Vec<String>,
    pub suggestion_index: Option<usize>,
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

/// Live Feed state: which fiches are currently visible under the active
/// filter, which one is selected, and which filenames just arrived.
///
/// `visible` holds *indices into `App::resolved_fiches`*, not clones — the
/// selection in `list_state` is itself an index into `visible` (the Nth
/// visible row), not into `resolved_fiches` directly. Re-anchored by
/// `filename` on every recompute (`App::recompute_feed_visible`), never by
/// index: a refresh prepends new fiches ahead of the ones already shown,
/// which shifts every plain index — the exact bug `AuditPanel`'s doc
/// comment already documents for the skill audit view.
pub struct FeedState {
    pub list_state: ListState,
    pub filter_text: String,
    pub skill_filter: SkillFilter,
    pub visible: Vec<usize>,
    /// Filenames that arrived on the most recent successful fetch (see
    /// `malt_radar_core::new_filenames`) — cleared implicitly by being
    /// overwritten on the next fetch, never accumulated.
    pub fresh: HashSet<String>,
}

impl FeedState {
    fn new() -> Self {
        Self {
            list_state: ListState::default(),
            filter_text: String::new(),
            skill_filter: SkillFilter::Any,
            visible: Vec::new(),
            fresh: HashSet::new(),
        }
    }
}

/// Re-selects by *name*, not index — a recompute can resort/reshuffle the
/// list (a merge moves the target to a higher count, a defuse splits one
/// row into several), and following the old numeric index would silently
/// land on an unrelated row. Falls back to a clamped index only when the
/// previous selection's name is gone entirely (or there wasn't one).
pub fn reselect(list_state: &mut ListState, items: &[SkillCount], previous_name: Option<&str>) {
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

pub struct App {
    /// Exactly as fetched from the aggregator — never mutated. Resolution
    /// happens fresh into `resolved_fiches` on every fetch/alias change,
    /// never in place, so fiches themselves stay a faithful record of what
    /// the LLM actually extracted.
    pub fiches: Vec<Fiche>,
    pub aliases: HashMap<String, String>,
    pub resolved_fiches: Vec<Fiche>,
    pub metrics: MarketMetrics,
    pub trend: TjmTrend,
    /// Full uncapped distribution — kept around only to source the
    /// alias-edit overlay's suggestions; the Skill Audit view itself
    /// renders `audit_orphans`/`audit_consolidated` instead.
    pub skill_audit: Vec<SkillCount>,
    pub audit_orphans: Vec<SkillCount>,
    pub audit_consolidated: Vec<SkillCount>,
    pub orphans_list_state: ListState,
    pub consolidated_list_state: ListState,
    pub audit_panel: AuditPanel,
    pub mode: Mode,
    pub alias_edit: Option<AliasEditState>,
    pub feed: FeedState,
    /// `filter_text` as it was just before entering `Mode::FeedFilter`,
    /// restored on `Esc` — mirrors `AliasEditState.target` in spirit
    /// (state that must survive a cancelled edit), but the feed filter has
    /// no separate overlay to hold it in.
    feed_filter_backup: Option<String>,
    /// The fiche shown by `Mode::FicheDetail`, held by *filename* rather
    /// than by index or clone — same reasoning as `AliasEditState.target`:
    /// a background refresh must not leave the overlay pointing at a
    /// fiche that's since moved or been dropped.
    detail_filename: Option<String>,
    pub detail_scroll: u16,
    pub focus: Focus,
    /// Every distinct skill now shows (see `ALL_SKILLS`), so unlike Round 1
    /// this panel scrolls and needs a real viewport/offset — hence
    /// `ListState` rather than a plain index. Re-anchored by *name*
    /// (`reselect_top_skills`) on every recompute, same discipline as the
    /// Live Feed and the skill audit view.
    pub top_skills_state: ListState,
    /// `None` before the daily digest job's first run (see
    /// `FetchOutcome::DigestOk`) — a normal state, not an error; the panel
    /// renders an explanatory message rather than blank content.
    pub digest: Option<Digest>,
    /// Line offset into `digest.summary_short` as shown in the dashboard
    /// panel (see `digest_scroll_by` for the lower bound and
    /// `ui::dashboard::draw_digest` for the upper one) — reset to 0 only
    /// when a genuinely new digest arrives (`apply`), not on every periodic
    /// refetch of the same one.
    pub digest_scroll: u16,
    /// Line offset into `digest.content` as shown by `Mode::DigestFull` —
    /// separate from `digest_scroll` since the two views scroll
    /// independently (leaving the full view mid-read while dashboard state
    /// updates around it shouldn't move it, and vice versa).
    pub digest_full_scroll: u16,
    pub status_line: String,
    pub loading: bool,
    pub last_refreshed: Option<Instant>,
}

impl App {
    pub fn new() -> Self {
        Self {
            fiches: Vec::new(),
            aliases: HashMap::new(),
            resolved_fiches: Vec::new(),
            metrics: MarketMetrics::default(),
            trend: TjmTrend::default(),
            skill_audit: Vec::new(),
            audit_orphans: Vec::new(),
            audit_consolidated: Vec::new(),
            orphans_list_state: ListState::default(),
            consolidated_list_state: ListState::default(),
            audit_panel: AuditPanel::Orphans,
            mode: Mode::Dashboard,
            alias_edit: None,
            feed: FeedState::new(),
            feed_filter_backup: None,
            detail_filename: None,
            detail_scroll: 0,
            focus: Focus::LiveFeed,
            top_skills_state: ListState::default(),
            digest: None,
            digest_scroll: 0,
            digest_full_scroll: 0,
            status_line: "chargement des fiches...".to_string(),
            loading: true,
            last_refreshed: None,
        }
    }

    pub fn focused_items(&self) -> &[SkillCount] {
        match self.audit_panel {
            AuditPanel::Orphans => &self.audit_orphans,
            AuditPanel::Consolidated => &self.audit_consolidated,
        }
    }

    pub fn focused_list_state_mut(&mut self) -> &mut ListState {
        match self.audit_panel {
            AuditPanel::Orphans => &mut self.orphans_list_state,
            AuditPanel::Consolidated => &mut self.consolidated_list_state,
        }
    }

    pub fn selected_skill_name(&self) -> Option<String> {
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
    pub fn recompute(&mut self) {
        // Captured *before* `resolved_fiches` is overwritten below: a
        // refetch can drop or reorder fiches, so an index recorded against
        // the old slice would silently point at a different fiche once
        // `resolved_fiches` is replaced.
        let previous_feed_filename = self
            .feed
            .list_state
            .selected()
            .and_then(|i| self.feed.visible.get(i))
            .and_then(|&idx| self.resolved_fiches.get(idx))
            .map(|f| f.filename.clone());

        let previous_top_skill = self
            .top_skills_state
            .selected()
            .and_then(|i| self.metrics.top_skills.get(i))
            .map(|s| s.name.clone());

        self.resolved_fiches = resolve_fiche_skills(&self.fiches, &self.aliases);
        self.metrics = compute_metrics(&self.resolved_fiches, ALL_SKILLS);
        self.trend = tjm_trend(&self.resolved_fiches, Utc::now(), TREND_WINDOW_DAYS, TREND_MIN_SAMPLE);
        // A fiche count landing exactly on the fetch limit means the older
        // (7-14 day) window may have been cut off server-side (`GET
        // /fiches` sorts by mtime and truncates) — the delta would then be
        // silently wrong but plausible-looking, so withhold it rather than
        // trust a possibly-truncated sample. `current`/`previous` stay:
        // they're each read from a single window, unaffected either way.
        if self.fiches.len() == FICHES_LIMIT as usize {
            self.trend.delta = None;
        }
        self.skill_audit = skill_audit(&self.resolved_fiches);
        self.reselect_top_skills(previous_top_skill.as_deref());

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

        self.recompute_feed_visible(previous_feed_filename.as_deref());

        // The detail overlay is keyed by filename precisely so it can
        // detect this: if the fiche it was showing dropped out of the
        // corpus entirely (rather than just moving), there's nothing left
        // to show — close it instead of leaving a stale/empty popup up.
        if self.mode == Mode::FicheDetail && self.detail_fiche().is_none() {
            self.close_fiche_detail();
        }
    }

    /// Re-derives `feed.visible` from the current filter over
    /// `resolved_fiches`, and re-anchors the selection onto
    /// `previous_filename` if it's still visible, falling back to a
    /// clamped index — same shape as `reselect`. Doesn't go through
    /// `index_of_key`: `feed.visible` holds indices into a *separate*
    /// slice (`resolved_fiches`), not the keyed items themselves, so the
    /// lookup needs one extra level of indirection that helper doesn't
    /// express.
    fn recompute_feed_visible(&mut self, previous_filename: Option<&str>) {
        self.feed.visible = filter_fiches(&self.resolved_fiches, &self.feed.filter_text, &self.feed.skill_filter);

        let new_index = previous_filename.and_then(|name| {
            self.feed
                .visible
                .iter()
                .position(|&idx| self.resolved_fiches[idx].filename == name)
        });

        match new_index {
            Some(i) => self.feed.list_state.select(Some(i)),
            None if !self.feed.visible.is_empty() => {
                let clamped = self.feed.list_state.selected().unwrap_or(0).min(self.feed.visible.len() - 1);
                self.feed.list_state.select(Some(clamped));
            }
            None => self.feed.list_state.select(None),
        }
    }

    /// Re-derives `feed.visible` in place, keeping the current selection
    /// anchored by filename — used when only the filter changed (typing in
    /// `FeedFilter`, or a skill selected from Top Skills), where
    /// `resolved_fiches`/`metrics`/`skill_audit` don't need recomputing.
    fn on_feed_filter_changed(&mut self) {
        let previous_filename = self
            .feed
            .list_state
            .selected()
            .and_then(|i| self.feed.visible.get(i))
            .and_then(|&idx| self.resolved_fiches.get(idx))
            .map(|f| f.filename.clone());
        self.recompute_feed_visible(previous_filename.as_deref());
    }

    pub fn has_active_feed_filter(&self) -> bool {
        !self.feed.filter_text.trim().is_empty() || !matches!(self.feed.skill_filter, SkillFilter::Any)
    }

    pub fn clear_feed_filter(&mut self) {
        self.feed.filter_text.clear();
        self.feed.skill_filter = SkillFilter::Any;
        self.on_feed_filter_changed();
    }

    pub fn enter_feed_filter(&mut self) {
        self.feed_filter_backup = Some(self.feed.filter_text.clone());
        self.mode = Mode::FeedFilter;
    }

    pub fn cancel_feed_filter(&mut self) {
        if let Some(backup) = self.feed_filter_backup.take() {
            self.feed.filter_text = backup;
            self.on_feed_filter_changed();
        }
        self.mode = Mode::Dashboard;
    }

    pub fn commit_feed_filter(&mut self) {
        self.feed_filter_backup = None;
        self.mode = Mode::Dashboard;
    }

    pub fn feed_filter_push_char(&mut self, c: char) {
        self.feed.filter_text.push(c);
        self.on_feed_filter_changed();
    }

    pub fn feed_filter_backspace(&mut self) {
        self.feed.filter_text.pop();
        self.on_feed_filter_changed();
    }

    pub fn feed_filter_clear_line(&mut self) {
        self.feed.filter_text.clear();
        self.on_feed_filter_changed();
    }

    /// Label describing the active filter for the Live Feed's block title,
    /// or `None` when nothing is filtered — the panel is otherwise
    /// indistinguishable from an unfiltered feed showing everything.
    pub fn feed_filter_label(&self) -> Option<String> {
        let text = self.feed.filter_text.trim();
        let skill = match &self.feed.skill_filter {
            SkillFilter::Any => None,
            SkillFilter::Named(name) => Some(format!("skill: {name}")),
            SkillFilter::NotIn(_) => Some("hors top 5".to_string()),
        };
        match (skill, text.is_empty()) {
            (None, true) => None,
            (Some(skill), true) => Some(skill),
            (None, false) => Some(format!("texte: {text}")),
            (Some(skill), false) => Some(format!("{skill}, texte: {text}")),
        }
    }

    /// Moves the feed selection by `delta` rows, clamped (never wrapping)
    /// to `[0, visible.len() - 1]`. Deliberately not `ListState::select_next`
    /// / `select_previous` (which don't know the item count and would
    /// happily walk `selected` past the end of a filtered-down list) —
    /// same clamping discipline as `alias_edit_move_suggestion`.
    pub fn feed_move(&mut self, delta: isize) {
        let len = self.feed.visible.len();
        if len == 0 {
            self.feed.list_state.select(None);
            return;
        }
        let current = self.feed.list_state.selected().map(|i| i as isize).unwrap_or(-1);
        let next = (current + delta).clamp(0, len as isize - 1) as usize;
        self.feed.list_state.select(Some(next));
    }

    pub fn feed_select_first(&mut self) {
        if !self.feed.visible.is_empty() {
            self.feed.list_state.select(Some(0));
        }
    }

    pub fn feed_select_last(&mut self) {
        if !self.feed.visible.is_empty() {
            self.feed.list_state.select(Some(self.feed.visible.len() - 1));
        }
    }

    /// Re-anchors `top_skills_state`'s selection by *name*, not index — same
    /// reason as `reselect`: `compute_metrics` re-ranks `top_skills` by
    /// count on every recompute, so a numeric index can silently land on an
    /// unrelated skill after a refresh. Not expressed via the generic
    /// `reselect` (typed for `&[SkillCount]`) since `top_skills` is a
    /// `Vec<SkillStat>` — a few duplicated lines beat a generic parameter
    /// just to share this shape once.
    fn reselect_top_skills(&mut self, previous_name: Option<&str>) {
        let items = &self.metrics.top_skills;
        if items.is_empty() {
            self.top_skills_state.select(None);
            return;
        }
        if let Some(name) = previous_name {
            if let Some(idx) = items.iter().position(|s| s.name == name) {
                self.top_skills_state.select(Some(idx));
                return;
            }
        }
        let clamped = self.top_skills_state.selected().unwrap_or(0).min(items.len() - 1);
        self.top_skills_state.select(Some(clamped));
    }

    /// `Tab`/`BackTab` both step forward through this fixed order — same as
    /// before Digest joined focus, just no longer a 2-way toggle.
    pub fn cycle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::TopSkills => Focus::LiveFeed,
            Focus::LiveFeed => Focus::Digest,
            Focus::Digest => Focus::TopSkills,
        };
    }

    /// Scrolls the Digest panel by `delta` lines (negative = up), clamped
    /// at 0 here — the upper bound (true wrapped-line count) needs the
    /// render width `App` doesn't have, so it's enforced downstream every
    /// frame instead (`ui::dashboard::draw_digest`, which takes
    /// `digest_scroll` by `&mut` and clamps it in place).
    pub fn digest_scroll_by(&mut self, delta: i32) {
        let current = self.digest_scroll as i32;
        self.digest_scroll = (current + delta).max(0) as u16;
    }

    /// Opens the full-screen digest view — a no-op if there's no digest to
    /// show yet (mirrors `open_fiche_detail`'s guard), so `Enter` on an
    /// empty Digest panel just does nothing rather than opening a blank
    /// screen.
    pub fn open_digest_full(&mut self) {
        if self.digest.is_none() {
            return;
        }
        self.digest_full_scroll = 0;
        self.mode = Mode::DigestFull;
    }

    pub fn close_digest_full(&mut self) {
        self.mode = Mode::Dashboard;
    }

    /// Same clamped-at-0-here, clamped-at-the-top-downstream discipline as
    /// `digest_scroll_by` (upper bound enforced by
    /// `ui::dashboard::draw_digest_full`).
    pub fn digest_full_scroll_by(&mut self, delta: i32) {
        let current = self.digest_full_scroll as i32;
        self.digest_full_scroll = (current + delta).max(0) as u16;
    }

    pub fn top_skills_move(&mut self, delta: isize) {
        let len = self.metrics.top_skills.len();
        if len == 0 {
            self.top_skills_state.select(None);
            return;
        }
        let current = self.top_skills_state.selected().map(|i| i as isize).unwrap_or(-1);
        let next = (current + delta).clamp(0, len as isize - 1) as usize;
        self.top_skills_state.select(Some(next));
    }

    pub fn top_skills_select_first(&mut self) {
        if !self.metrics.top_skills.is_empty() {
            self.top_skills_state.select(Some(0));
        }
    }

    pub fn top_skills_select_last(&mut self) {
        if !self.metrics.top_skills.is_empty() {
            self.top_skills_state.select(Some(self.metrics.top_skills.len() - 1));
        }
    }

    /// `Enter` on a Top Skills row: filters the Live Feed on it and moves
    /// focus there. Deliberately scoped to the feed only, never to Market
    /// Metrics or Top Skills itself — filtering Top Skills on its own
    /// selection would show that one skill at 100%, which is circular and
    /// tells you nothing. The block title is what shows a filter is active;
    /// the metrics stay a read of the whole market.
    ///
    /// Selecting "Autres" applies `SkillFilter::NotIn` over every *named*
    /// row currently shown (i.e. the real top-N, not counting "Autres"
    /// itself) — "show me the long tail" is what that row actually means,
    /// not a dead end.
    ///
    /// In practice this branch is now dead: `recompute` calls
    /// `compute_metrics` with `ALL_SKILLS`, so `core` never has anything
    /// left over to bucket into `OTHERS_LABEL`. Kept rather than deleted —
    /// it's still correct, still covered by `core`'s own tests, and
    /// `compute_metrics` remains callable with a real cap elsewhere (or
    /// again here, if Top Skills ever grows a re-introduced limit).
    pub fn apply_selected_skill_filter(&mut self) {
        let Some(selected) = self.top_skills_state.selected() else {
            return;
        };
        let Some(skill) = self.metrics.top_skills.get(selected) else {
            return;
        };
        self.feed.skill_filter = if skill.name == OTHERS_LABEL {
            let named = self
                .metrics
                .top_skills
                .iter()
                .filter(|s| s.name != OTHERS_LABEL)
                .map(|s| s.name.clone())
                .collect();
            SkillFilter::NotIn(named)
        } else {
            SkillFilter::Named(skill.name.clone())
        };
        self.on_feed_filter_changed();
        self.focus = Focus::LiveFeed;
    }

    fn selected_feed_filename(&self) -> Option<String> {
        self.feed
            .list_state
            .selected()
            .and_then(|i| self.feed.visible.get(i))
            .and_then(|&idx| self.resolved_fiches.get(idx))
            .map(|f| f.filename.clone())
    }

    /// The fiche currently shown by `Mode::FicheDetail`, looked up fresh by
    /// filename on every call rather than cached — `resolved_fiches` can
    /// change under the overlay (an alias merge, a background refresh).
    pub fn detail_fiche(&self) -> Option<&Fiche> {
        let filename = self.detail_filename.as_deref()?;
        self.resolved_fiches.iter().find(|f| f.filename == filename)
    }

    pub fn open_fiche_detail(&mut self) {
        let Some(filename) = self.selected_feed_filename() else {
            return;
        };
        self.detail_filename = Some(filename);
        self.detail_scroll = 0;
        self.mode = Mode::FicheDetail;
    }

    pub fn close_fiche_detail(&mut self) {
        self.detail_filename = None;
        self.mode = Mode::Dashboard;
    }

    pub fn detail_scroll_down(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_add(1);
    }

    pub fn detail_scroll_up(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_sub(1);
    }

    /// Opens the detail fiche's URL in a browser (best-effort — see
    /// `crate::browser::open_url`), or reports why it couldn't via
    /// `status_line`. Never panics: a missing URL, an unrecognized scheme,
    /// or no available opener are all just a status message.
    pub fn open_detail_url(&mut self) {
        let Some(fiche) = self.detail_fiche() else { return };
        let Some(url) = fiche.url.clone() else {
            self.status_line = "pas d'URL pour cette fiche".to_string();
            return;
        };
        match crate::browser::open_url(&url) {
            Ok(()) => self.status_line = format!("ouverture de {url}"),
            Err(err) => self.status_line = format!("erreur: {err}"),
        }
    }

    /// Deletes the fiche shown by `Mode::FicheDetail`, both locally
    /// (optimistic, same discipline as `defuse_selected_skill`) and on the
    /// aggregator. Dropping it from `self.fiches` before `recompute()` is
    /// what closes the overlay: `recompute` already closes `FicheDetail`
    /// whenever `detail_fiche()` comes back empty (see its doc comment), so
    /// there's no separate `close_fiche_detail()` call needed here.
    pub fn delete_detail_fiche(&mut self, fiche_cmd_tx: &mpsc::UnboundedSender<FicheCommand>) {
        let Some(filename) = self.detail_filename.clone() else {
            return;
        };
        self.fiches.retain(|f| f.filename != filename);
        self.recompute();
        let _ = fiche_cmd_tx.send(FicheCommand::Delete { filename });
        self.status_line = "fiche supprimée".to_string();
    }

    pub fn toggle_help(&mut self) {
        self.mode = if self.mode == Mode::Help { Mode::Dashboard } else { Mode::Help };
    }

    pub fn apply(&mut self, outcome: FetchOutcome) {
        match outcome {
            FetchOutcome::Ok(fiches) => {
                self.loading = false;
                // Computed against the *old* `self.fiches`, before it's
                // overwritten below — see `new_filenames`'s first-load rule.
                self.feed.fresh = new_filenames(&self.fiches, &fiches);
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
            FetchOutcome::FicheDeleteErr(message) => {
                // Same shape as AliasWriteErr: the fiche was already
                // dropped locally (see `delete_detail_fiche`); a failed
                // server-side delete just means it'll reappear on the next
                // periodic refetch rather than staying gone.
                self.status_line = format!("erreur suppression: {message}");
            }
            FetchOutcome::DigestOk(digest) => {
                // Reset scroll only when the digest actually changed (by
                // `id`, including the None<->Some transitions) — every
                // periodic refetch of the *same* digest must leave a
                // mid-read scroll position alone.
                let changed = self.digest.as_ref().map(|d| d.id) != digest.as_ref().map(|d| d.id);
                self.digest = digest;
                if changed {
                    self.digest_scroll = 0;
                    self.digest_full_scroll = 0;
                }
            }
        }
    }

    pub fn toggle_skill_audit(&mut self) {
        self.mode = match self.mode {
            Mode::SkillAudit | Mode::AliasEdit => {
                self.alias_edit = None;
                Mode::Dashboard
            }
            _ => {
                reselect(&mut self.orphans_list_state, &self.audit_orphans, None);
                reselect(&mut self.consolidated_list_state, &self.audit_consolidated, None);
                Mode::SkillAudit
            }
        };
    }

    pub fn enter_alias_edit(&mut self) {
        let Some(name) = self.selected_skill_name() else {
            return;
        };
        self.alias_edit = Some(AliasEditState::new(name));
        self.mode = Mode::AliasEdit;
    }

    pub fn cancel_alias_edit(&mut self) {
        self.alias_edit = None;
        self.mode = Mode::SkillAudit;
    }

    pub fn alias_edit_push_char(&mut self, c: char) {
        if let Some(state) = &mut self.alias_edit {
            state.input.push(c);
            state.suggestion_index = None;
        }
        self.update_alias_suggestions();
    }

    pub fn alias_edit_backspace(&mut self) {
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
    pub fn alias_edit_move_suggestion(&mut self, delta: isize) {
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
    pub fn submit_alias_edit(&mut self, alias_cmd_tx: &mpsc::UnboundedSender<AliasCommand>) {
        let Some(state) = self.alias_edit.take() else { return };
        let new_canonical = state.input.trim().to_string();
        if new_canonical.is_empty() {
            // No-op: put the state back so the overlay stays open.
            self.alias_edit = Some(state);
            return;
        }
        let source_panel = self.audit_panel;

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

        match source_panel {
            // Merging from Orphans: stay there (recompute()'s generic
            // reselect-by-name already fell back to a clamped index, so
            // the cursor naturally lands on whatever slid into the merged
            // skill's old spot) — jumping to Consolidated after *every*
            // single merge would interrupt working through a list of
            // duplicates one by one. Only follow across if there's
            // nothing left to stay on.
            AuditPanel::Orphans if !self.audit_orphans.is_empty() => {}
            // Renaming an already-consolidated group, or the last orphan
            // just got merged away: follow the result into Consolidated.
            _ => {
                self.audit_panel = AuditPanel::Consolidated;
                reselect(&mut self.consolidated_list_state, &self.audit_consolidated, Some(&new_canonical));
            }
        }
    }

    /// The "undo": deletes every alias row pointing at the selected row's
    /// name, scattering its raw variants back to resolving as themselves.
    pub fn defuse_selected_skill(&mut self, alias_cmd_tx: &mpsc::UnboundedSender<AliasCommand>) {
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
