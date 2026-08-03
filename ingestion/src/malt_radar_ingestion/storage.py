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
    title TEXT NOT NULL,
    job_name TEXT,
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
    scraped_at TEXT NOT NULL,
    UNIQUE(source, external_id)
);
"""

_UPSERT_SQL = """
INSERT INTO missions (
    source, external_id, slug, url, title, job_name, skills,
    min_daily_salary, max_daily_salary, currency, remote_mode,
    contracts, duration_value, duration_period, location_label,
    company_name, published_at, scraped_at
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
ON CONFLICT(source, external_id) DO UPDATE SET
    slug=excluded.slug,
    url=excluded.url,
    title=excluded.title,
    job_name=excluded.job_name,
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
    scraped_at=excluded.scraped_at
"""


def init_db(path: str | Path = DEFAULT_DB_PATH) -> sqlite3.Connection:
    """Open (creating if needed) the missions database and ensure the schema exists."""
    path = Path(path)
    if str(path) != ":memory:":
        path.parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(path)
    conn.execute(SCHEMA)
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
                mission.title,
                mission.job_name,
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
                scraped_at,
            ),
        )
        count += 1
    conn.commit()
    return count
