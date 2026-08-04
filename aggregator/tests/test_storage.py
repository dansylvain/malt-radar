import sqlite3

from malt_radar_aggregator.storage import (
    delete_skill_alias,
    init_skill_aliases_table,
    list_skill_aliases,
    upsert_skill_alias,
)


def conn() -> sqlite3.Connection:
    c = sqlite3.connect(":memory:")
    init_skill_aliases_table(c)
    return c


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
