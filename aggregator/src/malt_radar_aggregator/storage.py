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

import json
import sqlite3
from datetime import datetime, timezone

SCHEMA = """
CREATE TABLE IF NOT EXISTS skill_aliases (
    alias_key TEXT PRIMARY KEY,
    canonical TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
"""

DIGESTS_SCHEMA = """
CREATE TABLE IF NOT EXISTS digests (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL,
    window_start TEXT NOT NULL,
    window_end TEXT NOT NULL,
    window_days INTEGER NOT NULL,
    model TEXT NOT NULL,
    fiche_count INTEGER NOT NULL,
    metrics_json TEXT NOT NULL,
    trend_json TEXT NOT NULL,
    content TEXT NOT NULL
);
"""

# Added after the initial release, same rationale/pattern as
# `malt_radar_ingestion.storage._MIGRATION_COLUMNS`: ALTER TABLE in place so
# an already-deployed digests table on the VPS picks it up without a fresh
# database. Nullable (no NOT NULL/DEFAULT) since pre-migration rows have no
# short summary — `_row_to_digest` falls back to a slice of `content` for
# those.
_DIGESTS_MIGRATION_COLUMNS = {
    "summary_short": "TEXT",
}

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


def _migrate_digests(conn: sqlite3.Connection) -> None:
    existing = {row[1] for row in conn.execute("PRAGMA table_info(digests)")}
    for column, ddl_type in _DIGESTS_MIGRATION_COLUMNS.items():
        if column not in existing:
            conn.execute(f"ALTER TABLE digests ADD COLUMN {column} {ddl_type}")


def init_digests_table(conn: sqlite3.Connection) -> None:
    conn.execute(DIGESTS_SCHEMA)
    _migrate_digests(conn)
    conn.commit()


def save_digest(
    conn: sqlite3.Connection,
    *,
    window_start: str,
    window_end: str,
    window_days: int,
    model: str,
    fiche_count: int,
    metrics: dict,
    trend: dict,
    summary_short: str,
    content: str,
) -> dict:
    created_at = datetime.now(timezone.utc).isoformat()
    metrics_json = json.dumps(metrics)
    trend_json = json.dumps(trend)
    cursor = conn.execute(
        """
        INSERT INTO digests
            (created_at, window_start, window_end, window_days, model, fiche_count, metrics_json, trend_json,
             summary_short, content)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        """,
        (
            created_at,
            window_start,
            window_end,
            window_days,
            model,
            fiche_count,
            metrics_json,
            trend_json,
            summary_short,
            content,
        ),
    )
    conn.commit()
    return {
        "id": cursor.lastrowid,
        "created_at": created_at,
        "window_start": window_start,
        "window_end": window_end,
        "window_days": window_days,
        "model": model,
        "fiche_count": fiche_count,
        "metrics": metrics,
        "trend": trend,
        "summary_short": summary_short,
        "content": content,
    }


def _row_to_digest(row: sqlite3.Row) -> dict:
    digest = dict(row)
    digest["metrics"] = json.loads(digest.pop("metrics_json"))
    digest["trend"] = json.loads(digest.pop("trend_json"))
    # Pre-migration rows (see `_DIGESTS_MIGRATION_COLUMNS`) have no stored
    # short summary — fall back to a slice of the full content rather than
    # surfacing an empty panel.
    if not digest.get("summary_short"):
        digest["summary_short"] = digest["content"][:280]
    return digest


def get_latest_digest(conn: sqlite3.Connection) -> dict | None:
    conn.row_factory = sqlite3.Row
    row = conn.execute("SELECT * FROM digests ORDER BY id DESC LIMIT 1").fetchone()
    conn.row_factory = None
    if row is None:
        return None
    return _row_to_digest(row)


def list_digests(conn: sqlite3.Connection, limit: int = 20) -> list[dict]:
    conn.row_factory = sqlite3.Row
    rows = conn.execute("SELECT * FROM digests ORDER BY id DESC LIMIT ?", (limit,)).fetchall()
    conn.row_factory = None
    return [_row_to_digest(row) for row in rows]
