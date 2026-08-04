//! Domain types and pure computations shared between the HTTP client and
//! the TUI rendering layer. No I/O here — keeps both testable in isolation.

use std::collections::HashMap;

use serde::Deserialize;

/// A generated fiche, as returned by the aggregator's `GET /fiches`.
///
/// Every field but `filename` is optional: the frontmatter comes from an
/// LLM extraction (`worker/schema.py::Extraction`) that can leave fields
/// null, and a fiche predating a schema change may simply not have a
/// given key at all.
#[derive(Debug, Clone, Deserialize)]
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

/// Market-wide metrics computed client-side from the raw fiche list — a
/// stand-in for Phase 3's weekly aggregation, which doesn't exist yet.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MarketMetrics {
    pub fiche_count: usize,
    pub avg_tjm: Option<f64>,
    pub top_skills: Vec<SkillStat>,
}

pub fn compute_metrics(fiches: &[Fiche], top_n: usize) -> MarketMetrics {
    let tjms: Vec<f64> = fiches.iter().filter_map(Fiche::tjm_mid).collect();
    let avg_tjm = if tjms.is_empty() {
        None
    } else {
        Some(tjms.iter().sum::<f64>() / tjms.len() as f64)
    };

    let mut counts: HashMap<&str, usize> = HashMap::new();
    for fiche in fiches {
        for skill in &fiche.stack {
            *counts.entry(skill.as_str()).or_insert(0) += 1;
        }
    }
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
        avg_tjm,
        top_skills,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fiche(stack: &[&str], tjm_min: Option<f64>, tjm_max: Option<f64>) -> Fiche {
        Fiche {
            filename: "f.md".into(),
            source: None,
            external_id: None,
            title: None,
            company: None,
            location: None,
            url: None,
            published_at: None,
            tjm_min,
            tjm_max,
            devise: None,
            stack: stack.iter().map(|s| s.to_string()).collect(),
            duree_valeur: None,
            duree_unite: None,
            niveau: None,
            categorie: None,
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
        assert_eq!(metrics.avg_tjm, Some(500.0));
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
    fn compute_metrics_omits_others_when_everything_fits() {
        let fiches = vec![fiche(&["Python", "RAG"], None, None), fiche(&["Python"], None, None)];

        let metrics = compute_metrics(&fiches, 10);

        assert!(!metrics.top_skills.iter().any(|s| s.name == OTHERS_LABEL));
        let total_percent: f64 = metrics.top_skills.iter().map(|s| s.percent).sum();
        assert!((total_percent - 100.0).abs() < 1e-9, "expected ~100, got {total_percent}");
    }
}
