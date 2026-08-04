"""Aggregator HTTP API: the VPS side of the worker's pull/push flow, plus
the read side consumed by the Phase 4 TUI.

- GET  /missions/unprocessed — the Pi5 worker pulls raw missions it hasn't
  extracted entities from yet.
- POST /fiches               — the worker pushes back the generated
  Markdown fiche once extraction succeeds; the mission is then flagged
  processed so it isn't served again.
- GET  /fiches               — the TUI (poste de travail) reads back the
  fiches already generated, as parsed frontmatter. Minimal stand-in until
  Phase 3's weekly aggregation/digest lands — no metrics here, just the
  raw fiche fields.

See ROADMAP.md (Phase 2/4) and CLAUDE.md's network section — this API is
reached only over Tailscale, never a public port (see server.py).
"""

from __future__ import annotations

import os
import re
import sqlite3
from pathlib import Path

import yaml
from fastapi import Depends, FastAPI, HTTPException
from pydantic import BaseModel

from malt_radar_aggregator.auth import require_token
from malt_radar_ingestion.storage import DEFAULT_DB_PATH, fetch_unprocessed, get_mission, init_db, mark_processed

FICHES_DIR = Path(__file__).resolve().parent.parent.parent / "data" / "fiches"
_SAFE_CHARS = re.compile(r"[^a-zA-Z0-9_-]+")
_FRONTMATTER_RE = re.compile(r"\A---\n(.*?\n)---\n", re.DOTALL)

app = FastAPI(title="malt-radar-aggregator")

_conn: sqlite3.Connection | None = None


def get_conn() -> sqlite3.Connection:
    """Lazily open (once) the shared connection to ingestion's malt_radar.db.

    Overridable via AGGREGATOR_DB_PATH — used by tests to point at a
    throwaway database instead of the real one.
    """
    global _conn
    if _conn is None:
        db_path = os.environ.get("AGGREGATOR_DB_PATH", str(DEFAULT_DB_PATH))
        _conn = init_db(db_path)
    return _conn


class FicheSubmission(BaseModel):
    mission_id: int
    markdown: str


def _fiche_filename(source: str, external_id: str) -> str:
    """Server-derived filename (never trust a client-supplied path)."""
    safe_source = _SAFE_CHARS.sub("-", source)
    safe_external_id = _SAFE_CHARS.sub("-", str(external_id))
    return f"{safe_source}_{safe_external_id}.md"


@app.get("/missions/unprocessed")
def list_unprocessed(
    limit: int = 50,
    conn: sqlite3.Connection = Depends(get_conn),
    _token: None = Depends(require_token),
) -> list[dict]:
    return fetch_unprocessed(conn, limit=limit)


@app.post("/fiches", status_code=201)
def submit_fiche(
    payload: FicheSubmission,
    conn: sqlite3.Connection = Depends(get_conn),
    _token: None = Depends(require_token),
) -> dict:
    mission = get_mission(conn, payload.mission_id)
    if mission is None:
        raise HTTPException(status_code=404, detail=f"No mission with id={payload.mission_id}")

    FICHES_DIR.mkdir(parents=True, exist_ok=True)
    filename = _fiche_filename(mission["source"], mission["external_id"])
    (FICHES_DIR / filename).write_text(payload.markdown, encoding="utf-8")

    mark_processed(conn, payload.mission_id)
    return {"mission_id": payload.mission_id, "filename": filename}


def _read_fiche(path: Path) -> dict | None:
    """Parse a fiche's YAML frontmatter. Returns None if the file doesn't
    start with a well-formed `---`-delimited block (skipped rather than
    failing the whole listing — one bad file shouldn't blank the TUI)."""
    match = _FRONTMATTER_RE.match(path.read_text(encoding="utf-8"))
    if match is None:
        return None
    frontmatter = yaml.safe_load(match.group(1))
    if not isinstance(frontmatter, dict):
        return None
    frontmatter["filename"] = path.name
    return frontmatter


@app.get("/fiches")
def list_fiches(
    limit: int = 100,
    _token: None = Depends(require_token),
) -> list[dict]:
    if not FICHES_DIR.exists():
        return []
    paths = sorted(FICHES_DIR.glob("*.md"), key=lambda p: p.stat().st_mtime, reverse=True)
    fiches = []
    for path in paths[:limit]:
        fiche = _read_fiche(path)
        if fiche is not None:
            fiches.append(fiche)
    return fiches
