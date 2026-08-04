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

/// Market-wide metrics computed client-side from the raw fiche list — a
/// stand-in for Phase 3's weekly aggregation, which doesn't exist yet.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MarketMetrics {
    pub fiche_count: usize,
    pub avg_tjm: Option<f64>,
    pub top_skills: Vec<(String, usize)>,
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
    let mut top_skills: Vec<(String, usize)> = counts.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    top_skills.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    top_skills.truncate(top_n);

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
            vec![("Python".to_string(), 2), ("RAG".to_string(), 1)]
        );
    }

    #[test]
    fn compute_metrics_handles_no_data() {
        let metrics = compute_metrics(&[], 10);
        assert_eq!(metrics, MarketMetrics::default());
    }

    #[test]
    fn compute_metrics_truncates_to_top_n() {
        let fiches = vec![fiche(&["A", "B", "C"], None, None)];
        let metrics = compute_metrics(&fiches, 2);
        assert_eq!(metrics.top_skills.len(), 2);
    }
}
