"""Skill-alias table: the aggregator's own schema.

Everything else the aggregator persists (missions) is delegated to
`malt_radar_ingestion.storage` — reused rather than duplicated, per
CLAUDE.md. Skill-vocabulary curation isn't a mission-scraping concern
though, so it doesn't belong in ingestion's schema; this is the
aggregator's first table of its own, living in the same SQLite file/
connection `get_conn()` already returns.

Resolution happens at *display* time (the TUI applies this table to raw
fiche `stack` entries when computing stats), not at extraction time —
fiches are never mutated, so correcting an alias here instantly fixes
every fiche that ever used the raw string, no backfill needed.
"""

from __future__ import annotations

import sqlite3
from datetime import datetime, timezone

SCHEMA = """
CREATE TABLE IF NOT EXISTS skill_aliases (
    alias_key TEXT PRIMARY KEY,
    canonical TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
"""

_UPSERT_SQL = """
INSERT INTO skill_aliases (alias_key, canonical, updated_at)
VALUES (?, ?, ?)
ON CONFLICT(alias_key) DO UPDATE SET
    canonical=excluded.canonical,
    updated_at=excluded.updated_at
"""


def init_skill_aliases_table(conn: sqlite3.Connection) -> None:
    conn.execute(SCHEMA)
    conn.commit()


def upsert_skill_alias(conn: sqlite3.Connection, alias: str, canonical: str) -> dict:
    alias_key = alias.strip().lower()
    canonical = canonical.strip()
    updated_at = datetime.now(timezone.utc).isoformat()
    conn.execute(_UPSERT_SQL, (alias_key, canonical, updated_at))
    conn.commit()
    return {"alias_key": alias_key, "canonical": canonical, "updated_at": updated_at}


def delete_skill_alias(conn: sqlite3.Connection, alias: str) -> bool:
    """Never 404s on a missing key — deleting an alias that isn't there is a
    no-op, since delete IS the "undo" mechanism (see ROADMAP.md)."""
    cursor = conn.execute("DELETE FROM skill_aliases WHERE alias_key = ?", (alias.strip().lower(),))
    conn.commit()
    return cursor.rowcount > 0


def list_skill_aliases(conn: sqlite3.Connection) -> list[dict]:
    conn.row_factory = sqlite3.Row
    rows = conn.execute("SELECT alias_key, canonical, updated_at FROM skill_aliases ORDER BY alias_key").fetchall()
    conn.row_factory = None
    return [dict(row) for row in rows]
