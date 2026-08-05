import sqlite3

from malt_radar_aggregator.storage import (
    delete_skill_alias,
    get_latest_digest,
    init_digests_table,
    init_skill_aliases_table,
    list_digests,
    list_skill_aliases,
    save_digest,
    upsert_skill_alias,
)


def conn() -> sqlite3.Connection:
    c = sqlite3.connect(":memory:")
    init_skill_aliases_table(c)
    return c


def digests_conn() -> sqlite3.Connection:
    c = sqlite3.connect(":memory:")
    init_digests_table(c)
    return c


def make_digest_kwargs(**overrides) -> dict:
    defaults = dict(
        window_start="2026-07-29T00:00:00+00:00",
        window_end="2026-08-05T00:00:00+00:00",
        window_days=7,
        model="claude-haiku-4-5",
        fiche_count=12,
        metrics={"fiche_count": 12, "tjm": {"median": 500}},
        trend={"delta": None},
        summary_short="Résumé court.",
        content="## Résumé exécutif\n...",
    )
    defaults.update(overrides)
    return defaults


def test_upsert_inserts_new_alias():
    c = conn()

    result = upsert_skill_alias(c, "PowerBI", "Power BI")

    assert result == {"alias_key": "powerbi", "canonical": "Power BI", "updated_at": result["updated_at"]}
    row = c.execute("SELECT alias_key, canonical FROM skill_aliases").fetchone()
    assert row == ("powerbi", "Power BI")


def test_upsert_is_idempotent_on_alias_key():
    c = conn()
    upsert_skill_alias(c, "powerbi", "Power BI")

    upsert_skill_alias(c, "powerbi", "Microsoft Power BI")

    rows = c.execute("SELECT alias_key, canonical FROM skill_aliases").fetchall()
    assert rows == [("powerbi", "Microsoft Power BI")]


def test_upsert_normalizes_case_and_whitespace_of_alias_key():
    c = conn()

    upsert_skill_alias(c, "  PowerBI  ", "Power BI")

    row = c.execute("SELECT alias_key FROM skill_aliases").fetchone()
    assert row[0] == "powerbi"


def test_delete_removes_existing_alias():
    c = conn()
    upsert_skill_alias(c, "rag", "RAG")

    deleted = delete_skill_alias(c, "rag")

    assert deleted is True
    assert c.execute("SELECT * FROM skill_aliases").fetchall() == []


def test_delete_missing_alias_returns_false():
    c = conn()

    assert delete_skill_alias(c, "does-not-exist") is False


def test_list_returns_all_aliases_alphabetically():
    c = conn()
    upsert_skill_alias(c, "rag systems", "RAG")
    upsert_skill_alias(c, "powerbi", "Power BI")

    aliases = list_skill_aliases(c)

    assert [a["alias_key"] for a in aliases] == ["powerbi", "rag systems"]
    assert aliases[0]["canonical"] == "Power BI"


def test_save_digest_and_get_latest_round_trip():
    c = digests_conn()

    saved = save_digest(c, **make_digest_kwargs())

    assert saved["id"] is not None
    assert saved["metrics"] == make_digest_kwargs()["metrics"]
    assert saved["trend"] == make_digest_kwargs()["trend"]

    latest = get_latest_digest(c)
    assert latest == saved


def test_get_latest_digest_returns_none_on_empty_table():
    c = digests_conn()

    assert get_latest_digest(c) is None


def test_get_latest_digest_falls_back_to_content_slice_for_pre_migration_rows():
    """Rows written before `summary_short` existed (pre-migration digests
    table) have no value for it — the read path must still return something
    usable in the panel instead of an empty string."""
    c = digests_conn()
    kwargs = make_digest_kwargs()
    c.execute(
        """
        INSERT INTO digests
            (created_at, window_start, window_end, window_days, model, fiche_count, metrics_json, trend_json, content)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        """,
        (
            "2026-08-01T00:00:00+00:00",
            kwargs["window_start"],
            kwargs["window_end"],
            kwargs["window_days"],
            kwargs["model"],
            kwargs["fiche_count"],
            "{}",
            "{}",
            kwargs["content"],
        ),
    )
    c.commit()

    latest = get_latest_digest(c)

    assert latest["summary_short"] == kwargs["content"][:280]


def test_list_digests_orders_newest_first_and_respects_limit():
    c = digests_conn()
    for i in range(3):
        save_digest(c, **make_digest_kwargs(fiche_count=i))

    digests = list_digests(c, limit=2)

    assert len(digests) == 2
    assert [d["fiche_count"] for d in digests] == [2, 1]


def test_list_digests_parses_metrics_and_trend_not_raw_json():
    c = digests_conn()
    save_digest(c, **make_digest_kwargs())

    digests = list_digests(c)

    assert isinstance(digests[0]["metrics"], dict)
    assert isinstance(digests[0]["trend"], dict)
