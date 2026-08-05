"""Pure metrics computation over the fiche corpus — no I/O.

Statistical conventions deliberately mirror `tui/core/src/lib.rs`
(`compute_metrics`, `tjm_stats`, `tjm_trend`, `OTHERS_LABEL`,
`TREND_MIN_SAMPLE`-style gating): R-7 linear-interpolation quartiles,
median-based trend with a minimum-sample gate, an "Autres" bucket for
skill frequency beyond `top_n`. Same reasoning, independently applied to
the same fiche corpus for digest.py, a second consumer.

A missing `categorie`/`niveau` on a fiche buckets into "Non renseigné"
rather than being dropped, since the digest cares about coverage gaps too.
"""

from __future__ import annotations

from collections import Counter
from datetime import date, datetime

OTHERS_LABEL = "Autres"
MISSING_LABEL = "Non renseigné"


def _parse_date(value: object) -> datetime | None:
    # PyYAML's SafeLoader auto-converts ISO-8601-looking scalars into
    # datetime/date objects on load (see fiches.read_fiche) — a fiche's
    # `published_at` reaches here already parsed, not as a string, so
    # both shapes have to be accepted rather than just re-parsing a str.
    if isinstance(value, datetime):
        return value
    if isinstance(value, date):
        return datetime(value.year, value.month, value.day)
    if not isinstance(value, str) or not value:
        return None
    try:
        return datetime.fromisoformat(value)
    except ValueError:
        return None


def filter_window(fiches: list[dict], start: datetime, end: datetime) -> list[dict]:
    """Fiches whose `published_at` falls in `(start, end]`. Fiches with a
    missing or unparseable `published_at` are excluded — never guessed
    into a window."""
    result = []
    for fiche in fiches:
        published = _parse_date(fiche.get("published_at"))
        if published is None:
            continue
        if published.tzinfo is None:
            published = published.replace(tzinfo=start.tzinfo)
        if start < published <= end:
            result.append(fiche)
    return result


def _tjm_mid(fiche: dict) -> float | None:
    tjm_min = fiche.get("tjm_min")
    tjm_max = fiche.get("tjm_max")
    if tjm_min is not None and tjm_max is not None:
        return (tjm_min + tjm_max) / 2
    if tjm_min is not None:
        return tjm_min
    if tjm_max is not None:
        return tjm_max
    return None


def _percentile(sorted_values: list[float], q: float) -> float | None:
    """R-7 linear-interpolation percentile (numpy/Excel PERCENTILE.INC
    default). `sorted_values` must already be sorted ascending."""
    n = len(sorted_values)
    if n == 0:
        return None
    if n == 1:
        return sorted_values[0]
    h = (n - 1) * q
    lo = int(h)
    hi = min(lo + 1, n - 1)
    frac = h - lo
    if lo == hi:
        return sorted_values[lo]
    return sorted_values[lo] + (sorted_values[hi] - sorted_values[lo]) * frac


def _tjm_stats(fiches: list[dict]) -> dict:
    total = len(fiches)
    tjms = sorted(v for f in fiches if (v := _tjm_mid(f)) is not None)
    sample = len(tjms)
    if sample == 0:
        return {
            "sample": 0,
            "total": total,
            "avg": None,
            "median": None,
            "p25": None,
            "p75": None,
            "min": None,
            "max": None,
        }
    return {
        "sample": sample,
        "total": total,
        "avg": sum(tjms) / sample,
        "median": _percentile(tjms, 0.5),
        "p25": _percentile(tjms, 0.25),
        "p75": _percentile(tjms, 0.75),
        "min": tjms[0],
        "max": tjms[-1],
    }


def _skill_breakdown(fiches: list[dict], top_n: int) -> list[dict]:
    counts: Counter[str] = Counter()
    for fiche in fiches:
        for skill in fiche.get("stack") or []:
            counts[skill] += 1

    total_mentions = sum(counts.values())
    if total_mentions == 0:
        return []

    ranked = sorted(counts.items(), key=lambda kv: (-kv[1], kv[0]))
    shown = ranked[:top_n]
    shown_mentions = sum(count for _, count in shown)

    result = [
        {"name": name, "count": count, "percent": 100 * count / total_mentions} for name, count in shown
    ]

    others_count = total_mentions - shown_mentions
    if others_count > 0:
        result.append(
            {
                "name": OTHERS_LABEL,
                "count": others_count,
                "percent": 100 * others_count / total_mentions,
            }
        )
    return result


def _value_breakdown(fiches: list[dict], field: str) -> dict[str, int]:
    counts: Counter[str] = Counter()
    for fiche in fiches:
        value = fiche.get(field)
        counts[value if value else MISSING_LABEL] += 1
    return dict(sorted(counts.items(), key=lambda kv: (-kv[1], kv[0])))


def compute_metrics(
    fiches: list[dict],
    window_start: datetime,
    window_end: datetime,
    top_n_skills: int = 10,
) -> dict:
    """Aggregate metrics over `fiches` (already filtered to a window by
    the caller). Handles 0 fiches (all None/empty, no exception) and 1
    fiche (short-circuits the R-7 quantile call, which needs >=2 points
    for interpolation but degenerates cleanly to that single value)."""
    return {
        "window_start": window_start.isoformat(),
        "window_end": window_end.isoformat(),
        "fiche_count": len(fiches),
        "tjm": _tjm_stats(fiches),
        "top_skills": _skill_breakdown(fiches, top_n_skills),
        "categorie_breakdown": _value_breakdown(fiches, "categorie"),
        "niveau_breakdown": _value_breakdown(fiches, "niveau"),
    }


def compute_trend(current: dict, previous: dict, min_sample: int = 5) -> dict:
    """Compares the current window's metrics dict to the previous window's.
    `delta` is None unless *both* windows' TJM sample clears `min_sample`
    — this is what keeps a thin-data day from being dressed up as a
    market move."""
    current_tjm = current["tjm"]
    previous_tjm = previous["tjm"]
    current_median = current_tjm["median"]
    previous_median = previous_tjm["median"]
    current_sample = current_tjm["sample"]
    previous_sample = previous_tjm["sample"]

    delta = None
    if current_sample >= min_sample and previous_sample >= min_sample:
        if current_median is not None and previous_median is not None:
            delta = current_median - previous_median

    return {
        "current_median": current_median,
        "previous_median": previous_median,
        "delta": delta,
        "current_sample": current_sample,
        "previous_sample": previous_sample,
        "fiche_count_delta": current["fiche_count"] - previous["fiche_count"],
    }
