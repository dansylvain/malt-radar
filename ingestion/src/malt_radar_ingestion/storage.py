"""SQLite storage for missions, shared across every source.

Each source module only needs to produce `Mission` objects (see models.py) —
this module doesn't know or care whether a mission came from Free-Work,
Comet, or France Travail.
"""

from __future__ import annotations

import json
import sqlite3
from collections.abc import Iterable
from datetime import datetime, timezone
from pathlib import Path

from malt_radar_ingestion.models import Mission

DEFAULT_DB_PATH = Path(__file__).resolve().parent.parent.parent / "data" / "malt_radar.db"

SCHEMA = """
CREATE TABLE IF NOT EXISTS missions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source TEXT NOT NULL,
    external_id TEXT NOT NULL,
    slug TEXT,
    url TEXT,
    job_slug TEXT,
    title TEXT NOT NULL,
    job_name TEXT,
    description TEXT,
    skills TEXT NOT NULL DEFAULT '[]',
    min_daily_salary REAL,
    max_daily_salary REAL,
    currency TEXT,
    remote_mode TEXT,
    contracts TEXT NOT NULL DEFAULT '[]',
    duration_value INTEGER,
    duration_period TEXT,
    location_label TEXT,
    company_name TEXT,
    published_at TEXT,
    raw_salary_text TEXT,
    scraped_at TEXT NOT NULL,
    processed INTEGER NOT NULL DEFAULT 0,
    processed_at TEXT,
    UNIQUE(source, external_id)
);
"""

# Columns added after the initial release — kept here so an existing
# malt_radar.db (already deployed) gets migrated in place via ALTER TABLE
# instead of requiring a fresh database. Safe to re-run: init_db only adds
# a column if it isn't already there.
_MIGRATION_COLUMNS = {
    "description": "TEXT",
    "processed": "INTEGER NOT NULL DEFAULT 0",
    "processed_at": "TEXT",
    "job_slug": "TEXT",
}

_UPSERT_SQL = """
INSERT INTO missions (
    source, external_id, slug, url, job_slug, title, job_name, description, skills,
    min_daily_salary, max_daily_salary, currency, remote_mode,
    contracts, duration_value, duration_period, location_label,
    company_name, published_at, raw_salary_text, scraped_at
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
ON CONFLICT(source, external_id) DO UPDATE SET
    slug=excluded.slug,
    url=excluded.url,
    job_slug=excluded.job_slug,
    title=excluded.title,
    job_name=excluded.job_name,
    description=excluded.description,
    skills=excluded.skills,
    min_daily_salary=excluded.min_daily_salary,
    max_daily_salary=excluded.max_daily_salary,
    currency=excluded.currency,
    remote_mode=excluded.remote_mode,
    contracts=excluded.contracts,
    duration_value=excluded.duration_value,
    duration_period=excluded.duration_period,
    location_label=excluded.location_label,
    company_name=excluded.company_name,
    published_at=excluded.published_at,
    raw_salary_text=excluded.raw_salary_text,
    scraped_at=excluded.scraped_at
"""
# Note: processed / processed_at are deliberately absent from the upsert —
# re-ingesting a mission (e.g. a daily re-scrape) must not reset the
# processing state the worker has already recorded for it.


def _migrate(conn: sqlite3.Connection) -> None:
    existing = {row[1] for row in conn.execute("PRAGMA table_info(missions)")}
    for column, ddl_type in _MIGRATION_COLUMNS.items():
        if column not in existing:
            conn.execute(f"ALTER TABLE missions ADD COLUMN {column} {ddl_type}")


def init_db(path: str | Path = DEFAULT_DB_PATH) -> sqlite3.Connection:
    """Open (creating if needed) the missions database and ensure the schema exists."""
    path = Path(path)
    if str(path) != ":memory:":
        path.parent.mkdir(parents=True, exist_ok=True)
    # check_same_thread=False: the ingestion CLI is single-threaded so this
    # changes nothing there, but the aggregator's HTTP API (FastAPI) serves
    # sync endpoints from a threadpool while reusing one connection.
    conn = sqlite3.connect(path, check_same_thread=False)
    conn.execute(SCHEMA)
    _migrate(conn)
    conn.commit()
    return conn


def upsert_missions(conn: sqlite3.Connection, missions: Iterable[Mission]) -> int:
    """Insert or update missions, keyed on (source, external_id). Returns the count processed."""
    scraped_at = datetime.now(timezone.utc).isoformat()
    count = 0
    for mission in missions:
        conn.execute(
            _UPSERT_SQL,
            (
                mission.source,
                str(mission.external_id),
                mission.slug,
                mission.url,
                mission.job_slug,
                mission.title,
                mission.job_name,
                mission.description,
                json.dumps(mission.skills, ensure_ascii=False),
                mission.min_daily_salary,
                mission.max_daily_salary,
                mission.currency,
                mission.remote_mode,
                json.dumps(mission.contracts, ensure_ascii=False),
                mission.duration_value,
                mission.duration_period,
                mission.location_label,
                mission.company_name,
                mission.published_at,
                mission.raw_salary_text,
                scraped_at,
            ),
        )
        count += 1
    conn.commit()
    return count


def fetch_unprocessed(conn: sqlite3.Connection, limit: int = 50) -> list[dict]:
    """Return unprocessed missions as plain dicts, ready for JSON serialization."""
    conn.row_factory = sqlite3.Row
    rows = conn.execute(
        "SELECT * FROM missions WHERE processed = 0 ORDER BY id LIMIT ?",
        (limit,),
    ).fetchall()
    conn.row_factory = None
    missions = []
    for row in rows:
        mission = dict(row)
        mission["skills"] = json.loads(mission["skills"])
        mission["contracts"] = json.loads(mission["contracts"])
        mission["processed"] = bool(mission["processed"])
        missions.append(mission)
    return missions


def get_mission(conn: sqlite3.Connection, mission_id: int) -> dict | None:
    """Fetch a single mission by its internal id, as a plain dict, or None if it doesn't exist."""
    conn.row_factory = sqlite3.Row
    row = conn.execute("SELECT * FROM missions WHERE id = ?", (mission_id,)).fetchone()
    conn.row_factory = None
    if row is None:
        return None
    mission = dict(row)
    mission["skills"] = json.loads(mission["skills"])
    mission["contracts"] = json.loads(mission["contracts"])
    mission["processed"] = bool(mission["processed"])
    return mission


def mark_processed(conn: sqlite3.Connection, mission_id: int) -> bool:
    """Flag a mission as processed. Returns False if no mission has that id."""
    cursor = conn.execute(
        "UPDATE missions SET processed = 1, processed_at = ? WHERE id = ?",
        (datetime.now(timezone.utc).isoformat(), mission_id),
    )
    conn.commit()
    return cursor.rowcount > 0
