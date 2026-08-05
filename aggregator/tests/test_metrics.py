from datetime import datetime, timedelta, timezone

from malt_radar_aggregator.metrics import (
    MISSING_LABEL,
    OTHERS_LABEL,
    compute_metrics,
    compute_trend,
    filter_window,
)

NOW = datetime(2026, 8, 5, 12, 0, 0, tzinfo=timezone.utc)


def fiche(**overrides) -> dict:
    defaults = {
        "published_at": NOW.isoformat(),
        "tjm_min": None,
        "tjm_max": None,
        "stack": [],
        "categorie": None,
        "niveau": None,
    }
    defaults.update(overrides)
    return defaults


def days_ago(days: float) -> str:
    return (NOW - timedelta(days=days)).isoformat()


# --- filter_window ---


def test_filter_window_excludes_missing_or_unparseable_dates():
    fiches = [
        fiche(published_at=None),
        fiche(published_at="not-a-date"),
        fiche(published_at=days_ago(1)),
    ]

    result = filter_window(fiches, NOW - timedelta(days=7), NOW)

    assert len(result) == 1


def test_filter_window_start_exclusive_end_inclusive():
    start = NOW - timedelta(days=7)
    fiches = [
        fiche(published_at=start.isoformat()),  # exactly at start -> excluded
        fiche(published_at=NOW.isoformat()),  # exactly at end -> included
    ]

    result = filter_window(fiches, start, NOW)

    assert len(result) == 1
    assert result[0]["published_at"] == NOW.isoformat()


# --- compute_metrics: edge cases ---


def test_compute_metrics_handles_zero_fiches():
    metrics = compute_metrics([], NOW - timedelta(days=7), NOW)

    assert metrics["fiche_count"] == 0
    assert metrics["tjm"]["sample"] == 0
    assert metrics["tjm"]["total"] == 0
    assert metrics["tjm"]["median"] is None
    assert metrics["top_skills"] == []
    assert metrics["categorie_breakdown"] == {}
    assert metrics["niveau_breakdown"] == {}


def test_compute_metrics_handles_one_fiche():
    fiches = [fiche(tjm_min=500, tjm_max=500)]

    metrics = compute_metrics(fiches, NOW - timedelta(days=7), NOW)

    assert metrics["fiche_count"] == 1
    assert metrics["tjm"]["sample"] == 1
    assert metrics["tjm"]["median"] == 500
    assert metrics["tjm"]["p25"] == 500
    assert metrics["tjm"]["p75"] == 500
    assert metrics["tjm"]["min"] == 500
    assert metrics["tjm"]["max"] == 500


# --- compute_metrics: R-7 quartile correctness ---


def test_compute_metrics_r7_quartiles_on_known_set():
    fiches = [
        fiche(tjm_min=100, tjm_max=100),
        fiche(tjm_min=200, tjm_max=200),
        fiche(tjm_min=300, tjm_max=300),
        fiche(tjm_min=400, tjm_max=400),
    ]

    metrics = compute_metrics(fiches, NOW - timedelta(days=7), NOW)

    assert metrics["tjm"]["p25"] == 175
    assert metrics["tjm"]["median"] == 250
    assert metrics["tjm"]["p75"] == 325
    assert metrics["tjm"]["min"] == 100
    assert metrics["tjm"]["max"] == 400


# --- compute_metrics: skill frequency / Autres ---


def test_skill_frequency_percentages_sum_to_100_including_autres():
    fiches = [fiche(stack=["A", "B", "C"])]

    metrics = compute_metrics(fiches, NOW - timedelta(days=7), NOW, top_n_skills=2)

    assert len(metrics["top_skills"]) == 3
    assert metrics["top_skills"][-1]["name"] == OTHERS_LABEL
    total_percent = sum(s["percent"] for s in metrics["top_skills"])
    assert abs(total_percent - 100.0) < 1e-6


def test_skill_frequency_omits_autres_when_everything_fits():
    fiches = [fiche(stack=["A", "B"])]

    metrics = compute_metrics(fiches, NOW - timedelta(days=7), NOW, top_n_skills=10)

    assert all(s["name"] != OTHERS_LABEL for s in metrics["top_skills"])
    total_percent = sum(s["percent"] for s in metrics["top_skills"])
    assert abs(total_percent - 100.0) < 1e-6


# --- compute_metrics: breakdown missing-value bucketing ---


def test_categorie_breakdown_buckets_missing_values():
    fiches = [
        fiche(categorie="RAG"),
        fiche(categorie=None),
        fiche(categorie="RAG"),
    ]

    metrics = compute_metrics(fiches, NOW - timedelta(days=7), NOW)

    assert metrics["categorie_breakdown"] == {"RAG": 2, MISSING_LABEL: 1}


def test_niveau_breakdown_buckets_missing_values():
    fiches = [fiche(niveau="senior"), fiche(niveau=None)]

    metrics = compute_metrics(fiches, NOW - timedelta(days=7), NOW)

    assert metrics["niveau_breakdown"] == {"senior": 1, MISSING_LABEL: 1}


# --- compute_trend ---


def test_compute_trend_gates_delta_on_min_sample_both_directions():
    current = compute_metrics(
        [fiche(tjm_min=500, tjm_max=500)] * 3, NOW - timedelta(days=7), NOW
    )
    previous = compute_metrics(
        [fiche(tjm_min=400, tjm_max=400)] * 10, NOW - timedelta(days=14), NOW - timedelta(days=7)
    )

    trend = compute_trend(current, previous, min_sample=5)
    assert trend["delta"] is None  # current sample (3) below min_sample

    trend2 = compute_trend(current, previous, min_sample=3)
    assert trend2["delta"] == 100.0


def test_compute_trend_handles_empty_both_windows():
    current = compute_metrics([], NOW - timedelta(days=7), NOW)
    previous = compute_metrics([], NOW - timedelta(days=14), NOW - timedelta(days=7))

    trend = compute_trend(current, previous)

    assert trend["delta"] is None
    assert trend["current_median"] is None
    assert trend["previous_median"] is None
    assert trend["fiche_count_delta"] == 0
