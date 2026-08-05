//! Domain types and pure computations shared between the HTTP client and
//! the TUI rendering layer. No I/O here — keeps both testable in isolation.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;

/// A generated fiche, as returned by the aggregator's `GET /fiches`.
///
/// Every field but `filename` is optional: the frontmatter comes from an
/// LLM extraction (`worker/schema.py::Extraction`) that can leave fields
/// null, and a fiche predating a schema change may simply not have a
/// given key at all. `Default` follows from that — it also keeps test
/// fixtures from having to spell out every field, so adding one to the
/// frontmatter doesn't mechanically break every existing test.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Fiche {
    pub filename: String,
    pub source: Option<String>,
    pub external_id: Option<String>,
    pub title: Option<String>,
    pub company: Option<String>,
    pub location: Option<String>,
    pub url: Option<String>,
    pub published_at: Option<String>,
    pub tjm_min: Option<f64>,
    pub tjm_max: Option<f64>,
    pub devise: Option<String>,
    #[serde(default)]
    pub stack: Vec<String>,
    pub duree_valeur: Option<i64>,
    pub duree_unite: Option<String>,
    pub niveau: Option<String>,
    pub categorie: Option<String>,
}

/// Truncates `label` to at most `max_width` chars, replacing the last char
/// with `…` when it doesn't fit — keeps fixed-width table/column layouts
/// aligned regardless of how long a value (e.g. a skill name) turns out to
/// be. Counts chars, not bytes, so multi-byte UTF-8 isn't cut mid-character.
pub fn truncate_label(label: &str, max_width: usize) -> String {
    if label.chars().count() <= max_width {
        return label.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    let truncated: String = label.chars().take(max_width - 1).collect();
    format!("{truncated}…")
}

impl Fiche {
    /// Midpoint of the tjm range, or whichever bound is present. `None` if
    /// the extraction didn't produce a rate at all.
    pub fn tjm_mid(&self) -> Option<f64> {
        match (self.tjm_min, self.tjm_max) {
            (Some(min), Some(max)) => Some((min + max) / 2.0),
            (Some(v), None) | (None, Some(v)) => Some(v),
            (None, None) => None,
        }
    }
}

/// A persisted skill-alias mapping, as returned by the aggregator's
/// `GET /skill-aliases` — `alias_key` (lowercased/cleaned raw skill
/// string) -> `canonical` (the display form it should resolve to).
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct SkillAlias {
    pub alias_key: String,
    pub canonical: String,
}

/// A generated strategic digest, as returned by the aggregator's
/// `GET /digest` (latest) / `GET /digests` (history) —
/// `aggregator/src/malt_radar_aggregator/digest.py`. `metrics`/`trend`
/// (the aggregated numbers the Claude prompt was built from) aren't
/// modeled here: the TUI already computes its own `MarketMetrics`/
/// `TjmTrend` client-side from the raw fiche list, so this only needs
/// `summary_short`/`content` (the narrative text) plus enough metadata to
/// label it. Claude is asked (via a forced `emit_digest` tool call, not a
/// text delimiter) to produce both a compact panel summary and the full
/// 5-section Markdown in one response — see `summary_short`'s doc comment.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct Digest {
    pub id: i64,
    pub created_at: String,
    pub window_start: String,
    pub window_end: String,
    pub window_days: i64,
    pub model: String,
    pub fiche_count: i64,
    /// Plain-text, ~5 sentences, sized for the compact Digest panel
    /// (`DIGEST_PANEL_HEIGHT` in `ui/dashboard.rs`) — never Markdown, unlike
    /// `content`. Falls back to a slice of `content` server-side for
    /// digests generated before this field existed (see aggregator's
    /// `_row_to_digest`), so it's never empty for an existing digest.
    pub summary_short: String,
    /// Full 5-section Markdown digest, shown in the dedicated full-screen
    /// view (`Mode::DigestFull`) rather than the dashboard panel.
    pub content: String,
}

/// Strips whitespace/punctuation noise, preserving casing — mirrors the
/// worker's `_clean` (`worker/src/malt_radar_worker/skills.py`). Casing
/// isn't folded here: that's what the alias-map lookup in `resolve_skill`
/// is for.
fn clean_skill(raw: &str) -> String {
    raw.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches(['.', ',', ';', ':', '!', '?'])
        .to_string()
}

/// Resolves one raw skill string to its display form: clean it, then look
/// up the cleaned/lowercased form in `aliases`, falling back to the
/// cleaned (case-preserved) string if there's no alias for it.
///
/// Single lookup, no chain-following: renaming an already-merged group is
/// handled by updating every alias row that pointed at the old name
/// (cascade-on-write, see `alias_keys_targeting` and the TUI's alias-edit
/// overlay), not by following alias chains here — keeps this function,
/// called on every render/refresh, trivially simple and cycle-free.
pub fn resolve_skill(raw: &str, aliases: &HashMap<String, String>) -> String {
    let cleaned = clean_skill(raw);
    aliases.get(&cleaned.to_lowercase()).cloned().unwrap_or(cleaned)
}

/// Applies `resolve_skill` to every fiche's `stack`, never mutating the
/// input — fiches are never rewritten, resolution happens fresh at
/// display time from whatever the alias table currently says. Re-dedupes
/// each fiche's own stack case-insensitively after resolution: two raw
/// entries on the same mission (e.g. "Power BI" and "PowerBI") can
/// collapse onto the same canonical string and must not double-count.
pub fn resolve_fiche_skills(fiches: &[Fiche], aliases: &HashMap<String, String>) -> Vec<Fiche> {
    fiches
        .iter()
        .map(|fiche| {
            let mut seen: HashSet<String> = HashSet::new();
            let mut stack = Vec::new();
            for raw in &fiche.stack {
                let skill = resolve_skill(raw, aliases);
                if skill.is_empty() {
                    continue;
                }
                if seen.insert(skill.to_lowercase()) {
                    stack.push(skill);
                }
            }
            Fiche {
                stack,
                ..fiche.clone()
            }
        })
        .collect()
}

/// Every existing `alias_key` in `aliases` whose value equals `canonical`
/// exactly. Shared by the TUI's rename-cascade (which rows need their
/// canonical updated when a merged group is renamed) and its `d`-key
/// defuse (which rows to delete to undo a merge).
pub fn alias_keys_targeting(canonical: &str, aliases: &HashMap<String, String>) -> Vec<String> {
    aliases
        .iter()
        .filter(|(_, v)| v.as_str() == canonical)
        .map(|(k, _)| k.clone())
        .collect()
}

/// Synthetic bucket name for every skill beyond `compute_metrics`'s `top_n`,
/// so the displayed percentages sum to 100.
pub const OTHERS_LABEL: &str = "Autres";

/// One skill's demand within the current fiche sample.
///
/// `percent` is this skill's share of *total skill mentions* across the
/// sample (a fiche listing 3 skills contributes 3 mentions), so the
/// `top_n` entries plus the synthetic [`OTHERS_LABEL`] bucket (present
/// whenever skills exist beyond `top_n`) sum to 100.
///
/// `percent_rounded` is `percent` rounded to the nearest integer using the
/// largest-remainder method *across the whole `top_skills` list*, not each
/// value rounded independently — independent rounding doesn't preserve the
/// sum (e.g. 6% + 6% + 6% + 3% + 3% + 74% = 98, not 100), which would
/// silently undermine the reason `Autres` exists. Always display
/// `percent_rounded`, never re-round `percent` yourself.
#[derive(Debug, Clone, PartialEq)]
pub struct SkillStat {
    pub name: String,
    pub count: usize,
    pub percent: f64,
    pub percent_rounded: i64,
}

/// Rounds a list of percentages (assumed to sum to ~100) to integers whose
/// sum is exactly 100, by flooring each value and then distributing the
/// leftover units to the entries with the largest fractional remainder.
fn round_percentages_to_100(percents: &[f64]) -> Vec<i64> {
    if percents.is_empty() {
        return Vec::new();
    }

    let floors: Vec<i64> = percents.iter().map(|p| p.floor() as i64).collect();
    let leftover = 100 - floors.iter().sum::<i64>();

    let mut by_remainder: Vec<usize> = (0..percents.len()).collect();
    by_remainder.sort_by(|&a, &b| {
        let remainder_a = percents[a] - percents[a].floor();
        let remainder_b = percents[b] - percents[b].floor();
        remainder_b.partial_cmp(&remainder_a).unwrap()
    });

    let mut rounded = floors;
    for &i in by_remainder.iter().take(leftover.clamp(0, percents.len() as i64) as usize) {
        rounded[i] += 1;
    }
    rounded
}

/// TJM distribution over a fiche sample, not just its mean — a raw average
/// hides how many fiches actually carry a rate (`sample`/`total`) and how
/// spread out the ones that do are. Display `median`/`p25`/`p75` alongside
/// `avg`, never `avg` alone: on a small, coverage-poor sample a mean is
/// easily dragged by one or two outlier fourchettes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TjmStats {
    /// Fiches whose `tjm_mid()` is `Some`.
    pub sample: usize,
    /// Fiches in the sample overall (`sample <= total`).
    pub total: usize,
    pub avg: Option<f64>,
    pub median: Option<f64>,
    pub p25: Option<f64>,
    pub p75: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

impl TjmStats {
    /// Fraction of the sample that carries a TJM at all. `0.0` when there
    /// are no fiches to begin with (not `NaN`), so callers can format it
    /// unconditionally.
    pub fn coverage_ratio(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.sample as f64 / self.total as f64
        }
    }
}

/// Linear-interpolation percentile (the "R-7" method — same convention as
/// numpy's and Excel's default `PERCENTILE.INC`). `sorted` must already be
/// sorted ascending. `q` is in `[0.0, 1.0]`.
fn percentile(sorted: &[f64], q: f64) -> Option<f64> {
    match sorted.len() {
        0 => None,
        1 => Some(sorted[0]),
        n => {
            let h = (n - 1) as f64 * q;
            let lo = h.floor() as usize;
            let hi = h.ceil() as usize;
            if lo == hi {
                Some(sorted[lo])
            } else {
                let frac = h - lo as f64;
                Some(sorted[lo] + (sorted[hi] - sorted[lo]) * frac)
            }
        }
    }
}

/// Computes the full TJM distribution over `fiches`, via `Fiche::tjm_mid()`
/// — a 400-600 fourchette counts once, at 500, exactly like `compute_metrics`
/// used to average it.
pub fn tjm_stats(fiches: &[Fiche]) -> TjmStats {
    let total = fiches.len();
    let mut tjms: Vec<f64> = fiches.iter().filter_map(Fiche::tjm_mid).collect();
    let sample = tjms.len();
    if tjms.is_empty() {
        return TjmStats {
            sample,
            total,
            ..Default::default()
        };
    }

    tjms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    TjmStats {
        sample,
        total,
        avg: Some(tjms.iter().sum::<f64>() / tjms.len() as f64),
        median: percentile(&tjms, 0.5),
        p25: percentile(&tjms, 0.25),
        p75: percentile(&tjms, 0.75),
        min: tjms.first().copied(),
        max: tjms.last().copied(),
    }
}

/// Width (in days) of each of `tjm_trend`'s two sliding windows.
pub const TREND_WINDOW_DAYS: i64 = 7;
/// Minimum fiches carrying a TJM a window needs before its median feeds a
/// delta — below this, a single fourchette could swing the whole trend.
pub const TREND_MIN_SAMPLE: usize = 5;

/// Comparison of two adjacent sliding windows of `window_days`, anchored on
/// `now` and keyed on each fiche's `published_at` — the market's own
/// calendar, not when this TUI happened to fetch it. Compares medians, never
/// means: at these sample sizes a single outlier fourchette would swing a
/// mean but barely move a median.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TjmTrend {
    pub current: Option<f64>,
    pub previous: Option<f64>,
    /// `current - previous`. `None` unless *both* windows clear
    /// `min_sample` — a lone outlier in a thin window must never be
    /// dressed up as a market-wide move.
    pub delta: Option<f64>,
    pub current_n: usize,
    pub previous_n: usize,
    /// Fiches whose `published_at` is missing or unparseable — excluded
    /// from both windows rather than guessed into one.
    pub undated: usize,
}

fn median_of(mut tjms: Vec<f64>) -> Option<f64> {
    if tjms.is_empty() {
        return None;
    }
    tjms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    percentile(&tjms, 0.5)
}

/// Computes [`TjmTrend`] over `fiches`. `now` is injected rather than read
/// from the clock so this stays pure and deterministic in tests; the TUI's
/// only caller passes `Utc::now()`.
pub fn tjm_trend(fiches: &[Fiche], now: DateTime<Utc>, window_days: i64, min_sample: usize) -> TjmTrend {
    let window = Duration::days(window_days);
    let current_start = now - window;
    let previous_start = now - window * 2;

    let mut current_tjms = Vec::new();
    let mut previous_tjms = Vec::new();
    let mut undated = 0usize;

    for fiche in fiches {
        let published = match fiche.published_at.as_deref().map(DateTime::parse_from_rfc3339) {
            Some(Ok(dt)) => dt.with_timezone(&Utc),
            _ => {
                undated += 1;
                continue;
            }
        };
        let Some(tjm) = fiche.tjm_mid() else { continue };

        if published > current_start && published <= now {
            current_tjms.push(tjm);
        } else if published > previous_start && published <= current_start {
            previous_tjms.push(tjm);
        }
        // Older than 2×window: outside both windows, simply ignored.
    }

    let current_n = current_tjms.len();
    let previous_n = previous_tjms.len();
    let current = median_of(current_tjms);
    let previous = median_of(previous_tjms);
    let delta = if current_n >= min_sample && previous_n >= min_sample {
        current.zip(previous).map(|(c, p)| c - p)
    } else {
        None
    };

    TjmTrend {
        current,
        previous,
        delta,
        current_n,
        previous_n,
        undated,
    }
}

/// Projects a `TjmStats`'s (p25, median, p75) onto a gauge of `width`
/// characters, spanning the sample's own min..max. `None` when there's no
/// proportion to draw: an empty sample, or one flat enough that
/// `min == max`.
pub fn iqr_gauge_positions(stats: &TjmStats, width: usize) -> Option<(usize, usize, usize)> {
    let (min, max, p25, median, p75) = match (stats.min, stats.max, stats.p25, stats.median, stats.p75) {
        (Some(min), Some(max), Some(p25), Some(median), Some(p75)) => (min, max, p25, median, p75),
        _ => return None,
    };
    if width == 0 || max <= min {
        return None;
    }
    let pos = |v: f64| -> usize {
        let ratio = ((v - min) / (max - min)).clamp(0.0, 1.0);
        (ratio * (width - 1) as f64).round() as usize
    };
    Some((pos(p25), pos(median), pos(p75)))
}

/// Market-wide metrics computed client-side from the raw fiche list — a
/// stand-in for Phase 3's weekly aggregation, which doesn't exist yet.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MarketMetrics {
    pub fiche_count: usize,
    pub tjm: TjmStats,
    pub top_skills: Vec<SkillStat>,
}

fn count_skill_mentions(fiches: &[Fiche]) -> HashMap<&str, usize> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for fiche in fiches {
        for skill in &fiche.stack {
            *counts.entry(skill.as_str()).or_insert(0) += 1;
        }
    }
    counts
}

pub fn compute_metrics(fiches: &[Fiche], top_n: usize) -> MarketMetrics {
    let tjm = tjm_stats(fiches);

    let counts = count_skill_mentions(fiches);
    let total_mentions: usize = counts.values().sum();
    let percent_of = |count: usize| -> f64 {
        if total_mentions == 0 {
            0.0
        } else {
            100.0 * count as f64 / total_mentions as f64
        }
    };

    let mut ranked: Vec<(String, usize)> = counts.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let shown_mentions: usize = ranked.iter().take(top_n).map(|(_, count)| *count).sum();
    let mut top_skills: Vec<SkillStat> = ranked
        .into_iter()
        .take(top_n)
        .map(|(name, count)| SkillStat {
            name,
            count,
            percent: percent_of(count),
            percent_rounded: 0,
        })
        .collect();

    let others_count = total_mentions - shown_mentions;
    if others_count > 0 {
        top_skills.push(SkillStat {
            name: OTHERS_LABEL.to_string(),
            count: others_count,
            percent: percent_of(others_count),
            percent_rounded: 0,
        });
    }

    let rounded = round_percentages_to_100(&top_skills.iter().map(|s| s.percent).collect::<Vec<_>>());
    for (skill, rounded) in top_skills.iter_mut().zip(rounded) {
        skill.percent_rounded = rounded;
    }

    MarketMetrics {
        fiche_count: fiches.len(),
        tjm,
        top_skills,
    }
}

/// One distinct skill name and how many fiches mention it, across the
/// *entire* sample — unlike `MarketMetrics::top_skills`, not capped to a
/// `top_n` and no synthetic [`OTHERS_LABEL`] bucket. Feeds the TUI's skill
/// audit view, whose purpose is to surface raw name variants (e.g. "RAG"
/// vs "RAG Systems") that should be added to the worker's normalization
/// alias table (`worker/src/malt_radar_worker/skills.py::CANONICAL_ALIASES`)
/// — a gap the top-5-only Top Skills panel can't reveal.
#[derive(Debug, Clone, PartialEq)]
pub struct SkillCount {
    pub name: String,
    pub count: usize,
}

pub fn skill_audit(fiches: &[Fiche]) -> Vec<SkillCount> {
    let counts = count_skill_mentions(fiches);
    let mut result: Vec<SkillCount> = counts
        .into_iter()
        .map(|(name, count)| SkillCount {
            name: name.to_string(),
            count,
        })
        .collect();
    // Alphabetical (case-insensitive), not by count: this view's job is to
    // let variants of the same skill (e.g. "RAG" / "RAG Systems") land next
    // to each other so they're easy to spot and alias — a count-first sort
    // scatters them across the list instead.
    result.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.name.cmp(&b.name)));
    result
}

/// Lowercases and folds common French/Latin accents to their plain ASCII
/// letter, so a search for "donnees" finds "Données". Hand-rolled rather
/// than pulling in a normalization crate — the corpus is French text from
/// two job-board APIs, so the accented range that actually shows up is
/// small and known in advance.
pub fn fold_for_search(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            'ÿ' => 'y',
            other => other,
        })
        .collect()
}

/// Whether `fiche` matches a text search, against title/company/location/
/// catégorie/stack. `folded_needle` must already be folded (see
/// [`fold_for_search`]) — `filter_fiches` folds it once per call rather
/// than once per fiche. An empty needle matches everything.
pub fn fiche_matches_text(fiche: &Fiche, folded_needle: &str) -> bool {
    if folded_needle.is_empty() {
        return true;
    }
    let haystack = [
        fiche.title.as_deref().unwrap_or_default(),
        fiche.company.as_deref().unwrap_or_default(),
        fiche.location.as_deref().unwrap_or_default(),
        fiche.categorie.as_deref().unwrap_or_default(),
        &fiche.stack.join(" "),
    ]
    .join(" ");
    fold_for_search(&haystack).contains(folded_needle)
}

/// A constraint on which skill(s) a fiche's (already-resolved) `stack` must
/// satisfy to pass the Live Feed filter.
#[derive(Debug, Clone, PartialEq)]
pub enum SkillFilter {
    /// No skill constraint.
    Any,
    /// Stack must contain this exact skill name (case-insensitive, not a
    /// substring match — filtering on "SQL" must not pull in "MySQL").
    Named(String),
    /// Stack must be non-empty and contain none of these names — the
    /// "Autres" bucket: everything not in the current top-N.
    NotIn(Vec<String>),
}

fn fiche_matches_skill(fiche: &Fiche, filter: &SkillFilter) -> bool {
    match filter {
        SkillFilter::Any => true,
        SkillFilter::Named(name) => fiche.stack.iter().any(|s| s.eq_ignore_ascii_case(name)),
        SkillFilter::NotIn(names) => {
            !fiche.stack.is_empty() && fiche.stack.iter().all(|s| !names.iter().any(|n| n.eq_ignore_ascii_case(s)))
        }
    }
}

/// Indices (into `fiches`) of every fiche matching both `text` and `skill`.
/// Returning indices rather than clones lets a `ListState` selection map
/// straight back onto the original (already-resolved) slice.
pub fn filter_fiches(fiches: &[Fiche], text: &str, skill: &SkillFilter) -> Vec<usize> {
    let folded_needle = fold_for_search(text.trim());
    fiches
        .iter()
        .enumerate()
        .filter(|(_, fiche)| fiche_matches_text(fiche, &folded_needle) && fiche_matches_skill(fiche, skill))
        .map(|(i, _)| i)
        .collect()
}

/// Filenames present in `incoming` but not in `previous` — "new since the
/// last refresh". Deliberately returns empty when `previous` is empty:
/// otherwise the very first load (nothing to compare against) would light
/// up every fiche in the feed as "new", which is meaningless noise on
/// startup rather than a signal of market movement.
pub fn new_filenames(previous: &[Fiche], incoming: &[Fiche]) -> HashSet<String> {
    if previous.is_empty() {
        return HashSet::new();
    }
    let previous_names: HashSet<&str> = previous.iter().map(|f| f.filename.as_str()).collect();
    incoming
        .iter()
        .filter(|f| !previous_names.contains(f.filename.as_str()))
        .map(|f| f.filename.clone())
        .collect()
}

/// Position of the item in `items` whose key (via `key_of`) equals `key`,
/// or `None` if `key` is `None` or absent. A generalization of the
/// by-name reselection already used for the skill audit view (see
/// `reselect` in the `app` crate) to any key type, for callers whose
/// key is derived directly from the item itself (`key_of`'s output
/// borrows from its input, matching plain `Fn(&T) -> &str` elision).
/// The Live Feed's own reselection needs a different shape — its items
/// are indices into a *separate* slice — and does its own lookup instead
/// of going through this helper; see `App::recompute_feed_visible`.
pub fn index_of_key<T>(items: &[T], key: Option<&str>, key_of: impl Fn(&T) -> &str) -> Option<usize> {
    let key = key?;
    items.iter().position(|item| key_of(item) == key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fiche(stack: &[&str], tjm_min: Option<f64>, tjm_max: Option<f64>) -> Fiche {
        Fiche {
            filename: "f.md".into(),
            tjm_min,
            tjm_max,
            stack: stack.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn truncate_label_leaves_short_labels_untouched() {
        assert_eq!(truncate_label("RAG", 16), "RAG");
        assert_eq!(truncate_label("exactly16chars!!", 16), "exactly16chars!!");
    }

    #[test]
    fn truncate_label_truncates_and_adds_ellipsis_at_max_width() {
        let result = truncate_label("Google Cloud Platform", 16);
        assert_eq!(result.chars().count(), 16);
        assert!(result.ends_with('…'));
        assert_eq!(result, "Google Cloud Pl…");
    }

    #[test]
    fn truncate_label_handles_zero_width() {
        assert_eq!(truncate_label("anything", 0), "");
    }

    #[test]
    fn tjm_mid_averages_the_range() {
        let f = fiche(&[], Some(400.0), Some(600.0));
        assert_eq!(f.tjm_mid(), Some(500.0));
    }

    #[test]
    fn tjm_mid_falls_back_to_whichever_bound_is_present() {
        assert_eq!(fiche(&[], Some(400.0), None).tjm_mid(), Some(400.0));
        assert_eq!(fiche(&[], None, Some(600.0)).tjm_mid(), Some(600.0));
        assert_eq!(fiche(&[], None, None).tjm_mid(), None);
    }

    #[test]
    fn compute_metrics_averages_tjm_and_ranks_skills() {
        let fiches = vec![
            fiche(&["Python", "RAG"], Some(400.0), Some(600.0)),
            fiche(&["Python"], Some(500.0), None),
            fiche(&[], None, None),
        ];

        let metrics = compute_metrics(&fiches, 10);

        assert_eq!(metrics.fiche_count, 3);
        assert_eq!(metrics.tjm.avg, Some(500.0));
        assert_eq!(metrics.tjm.sample, 2);
        assert_eq!(metrics.tjm.total, 3);
        assert_eq!(
            metrics.top_skills,
            vec![
                SkillStat {
                    name: "Python".to_string(),
                    count: 2,
                    percent: 200.0 / 3.0,
                    percent_rounded: 67,
                },
                SkillStat {
                    name: "RAG".to_string(),
                    count: 1,
                    percent: 100.0 / 3.0,
                    percent_rounded: 33,
                },
            ]
        );
    }

    #[test]
    fn compute_metrics_handles_no_data() {
        let metrics = compute_metrics(&[], 10);
        assert_eq!(metrics, MarketMetrics::default());
    }

    #[test]
    fn compute_metrics_bundles_the_rest_into_others_so_percents_sum_to_100() {
        let fiches = vec![fiche(&["A", "B", "C"], None, None)];

        let metrics = compute_metrics(&fiches, 2);

        assert_eq!(metrics.top_skills.len(), 3);
        assert_eq!(metrics.top_skills[2].name, OTHERS_LABEL);
        assert_eq!(metrics.top_skills[2].count, 1);
        let total_percent: f64 = metrics.top_skills.iter().map(|s| s.percent).sum();
        assert!((total_percent - 100.0).abs() < 1e-9, "expected ~100, got {total_percent}");
    }

    #[test]
    fn rounded_percents_sum_to_100_despite_many_ties() {
        // Reproduces the bug reported live: 5 skills at low, close counts plus a
        // large "Autres" — rounding each percent independently gave 6+6+6+3+3+74
        // = 98, not 100. `percent_rounded` must not repeat that.
        let mut stack: Vec<&str> = vec!["A", "A", "B", "B", "C", "C", "D", "E"];
        let others: Vec<String> = (0..23).map(|i| format!("Other{i}")).collect();
        stack.extend(others.iter().map(String::as_str));

        let fiches = vec![fiche(&stack, None, None)];
        let metrics = compute_metrics(&fiches, 5);

        let sum: i64 = metrics.top_skills.iter().map(|s| s.percent_rounded).sum();
        assert_eq!(sum, 100);
        assert_eq!(metrics.top_skills.last().unwrap().name, OTHERS_LABEL);
    }

    #[test]
    fn skill_audit_lists_every_distinct_skill_uncapped() {
        let fiches = vec![
            fiche(&["Python", "RAG"], None, None),
            fiche(&["Python"], None, None),
            fiche(&["RAG Systems"], None, None),
        ];

        let audit = skill_audit(&fiches);

        assert_eq!(
            audit,
            vec![
                SkillCount { name: "Python".to_string(), count: 2 },
                SkillCount { name: "RAG".to_string(), count: 1 },
                SkillCount { name: "RAG Systems".to_string(), count: 1 },
            ]
        );
    }

    #[test]
    fn skill_audit_sorts_alphabetically_regardless_of_count() {
        let fiches = vec![
            fiche(&["C"], None, None),
            fiche(&["C"], None, None),
            fiche(&["A"], None, None),
            fiche(&["B"], None, None),
        ];

        let audit = skill_audit(&fiches);

        assert_eq!(audit.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), vec!["A", "B", "C"]);
    }

    #[test]
    fn skill_audit_sorts_case_insensitively() {
        let fiches = vec![fiche(&["rust", "Python", "aws"], None, None)];

        let audit = skill_audit(&fiches);

        assert_eq!(audit.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), vec!["aws", "Python", "rust"]);
    }

    #[test]
    fn skill_audit_handles_no_data() {
        assert_eq!(skill_audit(&[]), Vec::new());
    }

    #[test]
    fn resolve_skill_uses_alias_when_present_case_insensitively() {
        let aliases = HashMap::from([("powerbi".to_string(), "Power BI".to_string())]);

        assert_eq!(resolve_skill("PowerBI", &aliases), "Power BI");
        assert_eq!(resolve_skill("powerbi", &aliases), "Power BI");
        assert_eq!(resolve_skill("PowerBI.", &aliases), "Power BI");
    }

    #[test]
    fn resolve_skill_falls_back_to_cleaned_raw_when_unmapped() {
        let aliases = HashMap::new();

        assert_eq!(resolve_skill("  LangChain  ", &aliases), "LangChain");
        assert_eq!(resolve_skill("Rag.", &aliases), "Rag");
    }

    #[test]
    fn resolve_fiche_skills_never_mutates_input_and_resolves_fresh() {
        let fiches = vec![fiche(&["powerbi"], None, None)];
        let aliases = HashMap::from([("powerbi".to_string(), "Power BI".to_string())]);

        let resolved = resolve_fiche_skills(&fiches, &aliases);

        assert_eq!(fiches[0].stack, vec!["powerbi".to_string()]);
        assert_eq!(resolved[0].stack, vec!["Power BI".to_string()]);
    }

    #[test]
    fn resolve_fiche_skills_dedupes_within_a_fiche_after_resolution() {
        let fiches = vec![fiche(&["PowerBI", "Power BI"], None, None)];
        let aliases = HashMap::from([("powerbi".to_string(), "Power BI".to_string())]);

        let resolved = resolve_fiche_skills(&fiches, &aliases);

        assert_eq!(resolved[0].stack, vec!["Power BI".to_string()]);
    }

    #[test]
    fn resolve_fiche_skills_is_identity_with_empty_alias_map() {
        let fiches = vec![fiche(&["Python", "RAG"], None, None)];

        let resolved = resolve_fiche_skills(&fiches, &HashMap::new());

        assert_eq!(resolved[0].stack, vec!["Python".to_string(), "RAG".to_string()]);
    }

    #[test]
    fn alias_keys_targeting_finds_every_matching_key() {
        let aliases = HashMap::from([
            ("powerbi".to_string(), "Power BI".to_string()),
            ("microsoft power bi".to_string(), "Power BI".to_string()),
            ("rag".to_string(), "RAG".to_string()),
        ]);

        let mut keys = alias_keys_targeting("Power BI", &aliases);
        keys.sort();

        assert_eq!(keys, vec!["microsoft power bi".to_string(), "powerbi".to_string()]);
    }

    #[test]
    fn alias_keys_targeting_returns_empty_when_no_match() {
        let aliases = HashMap::from([("rag".to_string(), "RAG".to_string())]);

        assert_eq!(alias_keys_targeting("Power BI", &aliases), Vec::<String>::new());
    }

    #[test]
    fn tjm_stats_computes_quartiles_via_r7_interpolation() {
        let fiches = vec![
            fiche(&[], Some(100.0), None),
            fiche(&[], Some(200.0), None),
            fiche(&[], Some(300.0), None),
            fiche(&[], Some(400.0), None),
        ];

        let stats = tjm_stats(&fiches);

        assert_eq!(stats.p25, Some(175.0));
        assert_eq!(stats.median, Some(250.0));
        assert_eq!(stats.p75, Some(325.0));
        assert_eq!(stats.min, Some(100.0));
        assert_eq!(stats.max, Some(400.0));
    }

    #[test]
    fn tjm_stats_median_on_odd_count_is_the_middle_value() {
        let fiches = vec![
            fiche(&[], Some(100.0), None),
            fiche(&[], Some(300.0), None),
            fiche(&[], Some(500.0), None),
        ];

        assert_eq!(tjm_stats(&fiches).median, Some(300.0));
    }

    #[test]
    fn tjm_stats_reports_coverage_over_the_whole_sample() {
        let fiches = vec![
            fiche(&[], Some(400.0), Some(600.0)), // has a rate
            fiche(&[], None, None),               // no rate
            fiche(&[], None, None),               // no rate
        ];

        let stats = tjm_stats(&fiches);

        assert_eq!(stats.sample, 1);
        assert_eq!(stats.total, 3);
        assert!((stats.coverage_ratio() - 1.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn tjm_stats_uses_tjm_mid_not_raw_bounds() {
        let fiches = vec![fiche(&[], Some(400.0), Some(600.0))];
        let stats = tjm_stats(&fiches);
        assert_eq!(stats.avg, Some(500.0));
        assert_eq!(stats.median, Some(500.0));
    }

    #[test]
    fn tjm_stats_handles_no_fiches() {
        let stats = tjm_stats(&[]);
        assert_eq!(stats, TjmStats::default());
        assert_eq!(stats.coverage_ratio(), 0.0);
    }

    #[test]
    fn tjm_stats_handles_fiches_with_no_rate_at_all() {
        let fiches = vec![fiche(&[], None, None), fiche(&[], None, None)];
        let stats = tjm_stats(&fiches);
        assert_eq!(stats.sample, 0);
        assert_eq!(stats.total, 2);
        assert_eq!(stats.avg, None);
        assert_eq!(stats.median, None);
        assert_eq!(stats.coverage_ratio(), 0.0);
    }

    fn dated_fiche(published_at: Option<&str>, tjm: Option<f64>) -> Fiche {
        Fiche {
            filename: "f.md".into(),
            published_at: published_at.map(String::from),
            tjm_min: tjm,
            tjm_max: tjm,
            ..Default::default()
        }
    }

    fn days_ago(now: DateTime<Utc>, days: i64) -> String {
        (now - Duration::days(days)).to_rfc3339()
    }

    #[test]
    fn tjm_trend_splits_fiches_into_current_and_previous_windows() {
        use chrono::TimeZone;
        let now = Utc.with_ymd_and_hms(2026, 8, 10, 12, 0, 0).unwrap();
        let fiches = vec![
            dated_fiche(Some(&days_ago(now, 1)), Some(500.0)),
            dated_fiche(Some(&days_ago(now, 3)), Some(520.0)),
            dated_fiche(Some(&days_ago(now, 9)), Some(400.0)),
            dated_fiche(Some(&days_ago(now, 12)), Some(420.0)),
        ];

        let trend = tjm_trend(&fiches, now, 7, 1);

        assert_eq!(trend.current_n, 2);
        assert_eq!(trend.previous_n, 2);
        assert_eq!(trend.current, Some(510.0));
        assert_eq!(trend.previous, Some(410.0));
        assert_eq!(trend.delta, Some(100.0));
    }

    #[test]
    fn tjm_trend_ignores_fiches_older_than_two_windows() {
        use chrono::TimeZone;
        let now = Utc.with_ymd_and_hms(2026, 8, 10, 12, 0, 0).unwrap();
        let fiches = vec![
            dated_fiche(Some(&days_ago(now, 1)), Some(500.0)),
            dated_fiche(Some(&days_ago(now, 30)), Some(100.0)),
        ];

        let trend = tjm_trend(&fiches, now, 7, 1);

        assert_eq!(trend.current_n, 1);
        assert_eq!(trend.previous_n, 0);
        assert_eq!(trend.undated, 0);
    }

    #[test]
    fn tjm_trend_counts_unreadable_dates_as_undated_never_in_a_window() {
        use chrono::TimeZone;
        let now = Utc.with_ymd_and_hms(2026, 8, 10, 12, 0, 0).unwrap();
        let fiches = vec![
            dated_fiche(Some("not-a-date"), Some(500.0)),
            dated_fiche(None, Some(500.0)),
            dated_fiche(Some(&days_ago(now, 1)), Some(500.0)),
        ];

        let trend = tjm_trend(&fiches, now, 7, 1);

        assert_eq!(trend.undated, 2);
        assert_eq!(trend.current_n, 1);
    }

    #[test]
    fn tjm_trend_withholds_delta_below_min_sample() {
        use chrono::TimeZone;
        let now = Utc.with_ymd_and_hms(2026, 8, 10, 12, 0, 0).unwrap();
        let fiches = vec![
            dated_fiche(Some(&days_ago(now, 1)), Some(500.0)),
            dated_fiche(Some(&days_ago(now, 9)), Some(400.0)),
            dated_fiche(Some(&days_ago(now, 9)), Some(420.0)),
        ];

        let trend = tjm_trend(&fiches, now, 7, 5);

        assert_eq!(trend.current, Some(500.0));
        assert_eq!(trend.previous, Some(410.0));
        assert_eq!(trend.delta, None);
    }

    #[test]
    fn tjm_trend_delta_sign_reflects_direction() {
        use chrono::TimeZone;
        let now = Utc.with_ymd_and_hms(2026, 8, 10, 12, 0, 0).unwrap();
        let fiches = vec![
            dated_fiche(Some(&days_ago(now, 1)), Some(400.0)),
            dated_fiche(Some(&days_ago(now, 9)), Some(500.0)),
        ];

        let trend = tjm_trend(&fiches, now, 7, 1);

        assert_eq!(trend.delta, Some(-100.0));
    }

    #[test]
    fn tjm_trend_handles_empty_sample() {
        use chrono::TimeZone;
        let now = Utc.with_ymd_and_hms(2026, 8, 10, 12, 0, 0).unwrap();

        let trend = tjm_trend(&[], now, 7, 1);

        assert_eq!(trend, TjmTrend::default());
    }

    #[test]
    fn iqr_gauge_positions_projects_quartiles_onto_the_bar_width() {
        let stats = TjmStats {
            sample: 4,
            total: 4,
            avg: Some(250.0),
            median: Some(250.0),
            p25: Some(175.0),
            p75: Some(325.0),
            min: Some(100.0),
            max: Some(400.0),
        };

        // width=31 -> 30 usable slots; ratio = (value - min) / (max - min):
        // p25 0.25*30=7.5->8, median 0.5*30=15, p75 0.75*30=22.5->23.
        let (p25_pos, median_pos, p75_pos) = iqr_gauge_positions(&stats, 31).unwrap();

        assert_eq!(p25_pos, 8);
        assert_eq!(median_pos, 15);
        assert_eq!(p75_pos, 23);
    }

    #[test]
    fn iqr_gauge_positions_is_none_for_an_empty_sample() {
        assert_eq!(iqr_gauge_positions(&TjmStats::default(), 20), None);
    }

    #[test]
    fn iqr_gauge_positions_is_none_when_the_sample_is_flat() {
        let stats = TjmStats {
            sample: 2,
            total: 2,
            avg: Some(500.0),
            median: Some(500.0),
            p25: Some(500.0),
            p75: Some(500.0),
            min: Some(500.0),
            max: Some(500.0),
        };
        assert_eq!(iqr_gauge_positions(&stats, 20), None);
    }

    #[test]
    fn iqr_gauge_positions_keeps_p25_median_p75_in_order() {
        let stats = TjmStats {
            sample: 4,
            total: 4,
            avg: Some(250.0),
            median: Some(250.0),
            p25: Some(175.0),
            p75: Some(325.0),
            min: Some(100.0),
            max: Some(400.0),
        };

        let (p25_pos, median_pos, p75_pos) = iqr_gauge_positions(&stats, 31).unwrap();

        assert!(p25_pos <= median_pos);
        assert!(median_pos <= p75_pos);
    }

    #[test]
    fn compute_metrics_omits_others_when_everything_fits() {
        let fiches = vec![fiche(&["Python", "RAG"], None, None), fiche(&["Python"], None, None)];

        let metrics = compute_metrics(&fiches, 10);

        assert!(!metrics.top_skills.iter().any(|s| s.name == OTHERS_LABEL));
        let total_percent: f64 = metrics.top_skills.iter().map(|s| s.percent).sum();
        assert!((total_percent - 100.0).abs() < 1e-9, "expected ~100, got {total_percent}");
    }

    fn titled_fiche(title: &str, company: &str, stack: &[&str]) -> Fiche {
        Fiche {
            filename: format!("{title}.md"),
            title: Some(title.to_string()),
            company: Some(company.to_string()),
            stack: stack.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn fold_for_search_lowercases_and_strips_accents() {
        assert_eq!(fold_for_search("Données"), "donnees");
        assert_eq!(fold_for_search("Ingénieur À Paris"), "ingenieur a paris");
    }

    #[test]
    fn fiche_matches_text_is_case_and_accent_insensitive_across_fields() {
        let f = titled_fiche("Ingénieur Données", "Acme", &["Python"]);
        assert!(fiche_matches_text(&f, &fold_for_search("donnees")));
        assert!(fiche_matches_text(&f, &fold_for_search("ACME")));
        assert!(fiche_matches_text(&f, &fold_for_search("python")));
    }

    #[test]
    fn fiche_matches_text_empty_needle_matches_everything() {
        let f = titled_fiche("Anything", "Anyone", &[]);
        assert!(fiche_matches_text(&f, ""));
    }

    #[test]
    fn filter_fiches_combines_text_and_skill_filters() {
        let fiches = vec![
            titled_fiche("RAG Engineer", "Acme", &["RAG", "Python"]),
            titled_fiche("RAG Engineer", "Other Co", &["RAG"]),
            titled_fiche("Backend Dev", "Acme", &["Python"]),
        ];

        let indices = filter_fiches(&fiches, "acme", &SkillFilter::Named("RAG".to_string()));

        assert_eq!(indices, vec![0]);
    }

    #[test]
    fn filter_fiches_named_is_exact_not_substring() {
        let fiches = vec![titled_fiche("A", "X", &["MySQL"]), titled_fiche("B", "X", &["SQL"])];

        let indices = filter_fiches(&fiches, "", &SkillFilter::Named("SQL".to_string()));

        assert_eq!(indices, vec![1]);
    }

    #[test]
    fn filter_fiches_not_in_finds_the_long_tail_and_skips_empty_stacks() {
        let fiches = vec![
            titled_fiche("A", "X", &["Python"]),
            titled_fiche("B", "X", &["Rust"]),
            titled_fiche("C", "X", &[]),
        ];

        let indices = filter_fiches(&fiches, "", &SkillFilter::NotIn(vec!["Python".to_string()]));

        assert_eq!(indices, vec![1]);
    }

    #[test]
    fn filter_fiches_empty_needle_and_any_skill_returns_everything() {
        let fiches = vec![titled_fiche("A", "X", &[]), titled_fiche("B", "Y", &[])];

        let indices = filter_fiches(&fiches, "", &SkillFilter::Any);

        assert_eq!(indices, vec![0, 1]);
    }

    #[test]
    fn new_filenames_returns_only_additions() {
        let previous = vec![titled_fiche("A", "X", &[]), titled_fiche("B", "X", &[])];
        let incoming = vec![
            titled_fiche("A", "X", &[]),
            titled_fiche("B", "X", &[]),
            titled_fiche("C", "X", &[]),
        ];

        let fresh = new_filenames(&previous, &incoming);

        assert_eq!(fresh, HashSet::from(["C.md".to_string()]));
    }

    #[test]
    fn new_filenames_empty_when_previous_is_empty_first_load_rule() {
        let incoming = vec![titled_fiche("A", "X", &[]), titled_fiche("B", "X", &[])];

        assert_eq!(new_filenames(&[], &incoming), HashSet::new());
    }

    #[test]
    fn new_filenames_ignores_dropped_fiches() {
        let previous = vec![titled_fiche("A", "X", &[]), titled_fiche("B", "X", &[])];
        let incoming = vec![titled_fiche("A", "X", &[])];

        assert_eq!(new_filenames(&previous, &incoming), HashSet::new());
    }

    #[test]
    fn index_of_key_finds_and_reports_absence() {
        let items = vec!["a.md".to_string(), "b.md".to_string()];

        assert_eq!(index_of_key(&items, Some("b.md"), |s| s.as_str()), Some(1));
        assert_eq!(index_of_key(&items, Some("z.md"), |s| s.as_str()), None);
        assert_eq!(index_of_key(&items, None, |s| s.as_str()), None);
    }
}
