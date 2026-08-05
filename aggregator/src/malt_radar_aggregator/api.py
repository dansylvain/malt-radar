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
- DELETE /fiches/{filename}  — the TUI deletes a single fiche (e.g. a
  bad extraction) from its detail overlay. `filename` is a path segment
  but never trusted as a filesystem path directly — see
  `_resolve_fiche_path`. Idempotent like DELETE /skill-aliases: a missing
  file reports `deleted: false` rather than 404.
- GET/PUT/DELETE /skill-aliases — the persisted, editable skill-name
  alias table (raw/cleaned string -> canonical display name). Fiches are
  never mutated; the TUI applies this table to `stack` entries at display
  time, so editing an alias here instantly corrects every fiche that ever
  used that raw string, no backfill. Edited live from the TUI's Skill
  Audit view.
- GET /digest / GET /digests — the persisted history of Claude-generated
  strategic digests (see digest.py), written by the daily systemd timer,
  not this API. Read-only.

See ROADMAP.md (Phase 2/4) and CLAUDE.md's network section — this API is
reached only over Tailscale, never a public port (see server.py).
"""

from __future__ import annotations

import os
import re
import sqlite3
from pathlib import Path

from fastapi import Depends, FastAPI, HTTPException
from pydantic import BaseModel

from malt_radar_aggregator.auth import require_token
from malt_radar_aggregator.fiches import FICHES_DIR, list_fiches as _list_fiches
from malt_radar_aggregator.storage import (
    delete_skill_alias,
    get_latest_digest,
    init_digests_table,
    init_skill_aliases_table,
    list_digests,
    list_skill_aliases,
    upsert_skill_alias,
)
from malt_radar_ingestion.storage import DEFAULT_DB_PATH, fetch_unprocessed, get_mission, init_db, mark_processed

_SAFE_CHARS = re.compile(r"[^a-zA-Z0-9_-]+")

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
        init_skill_aliases_table(_conn)
        init_digests_table(_conn)
    return _conn


class FicheSubmission(BaseModel):
    mission_id: int
    markdown: str


class SkillAliasUpsert(BaseModel):
    alias: str
    canonical: str


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


@app.get("/fiches")
def list_fiches(
    limit: int = 100,
    _token: None = Depends(require_token),
) -> list[dict]:
    return _list_fiches(FICHES_DIR, limit=limit)


def _resolve_fiche_path(filename: str) -> Path:
    """Rejects anything that isn't a bare filename inside `FICHES_DIR` —
    unlike `_fiche_filename` (server-derived, trusted), this one takes a
    filename back from the client, so `..`/`/`/an absolute path all need
    to be ruled out before it ever touches the filesystem."""
    if "/" in filename or "\\" in filename or filename in {".", ".."}:
        raise HTTPException(status_code=400, detail="invalid filename")
    path = (FICHES_DIR / filename).resolve()
    if path.parent != FICHES_DIR.resolve():
        raise HTTPException(status_code=400, detail="invalid filename")
    return path


@app.delete("/fiches/{filename}")
def delete_fiche(
    filename: str,
    _token: None = Depends(require_token),
) -> dict:
    # Never 404s on a missing file — same reasoning as delete_skill_alias:
    # deleting a fiche that's already gone (e.g. a double `d` press from
    # the TUI, with no confirmation step in between) is a no-op, not an
    # error. Only `_resolve_fiche_path` (invalid/unsafe filename) still
    # raises.
    path = _resolve_fiche_path(filename)
    if not path.is_file():
        return {"filename": filename, "deleted": False}
    path.unlink()
    return {"filename": filename, "deleted": True}


@app.get("/skill-aliases")
def list_aliases(
    conn: sqlite3.Connection = Depends(get_conn),
    _token: None = Depends(require_token),
) -> list[dict]:
    return list_skill_aliases(conn)


@app.put("/skill-aliases")
def put_alias(
    payload: SkillAliasUpsert,
    conn: sqlite3.Connection = Depends(get_conn),
    _token: None = Depends(require_token),
) -> dict:
    if not payload.alias.strip() or not payload.canonical.strip():
        raise HTTPException(status_code=422, detail="alias and canonical must be non-empty")
    return upsert_skill_alias(conn, payload.alias, payload.canonical)


@app.delete("/skill-aliases")
def delete_alias(
    alias: str,
    conn: sqlite3.Connection = Depends(get_conn),
    _token: None = Depends(require_token),
) -> dict:
    # `alias` as a required query param, not a path segment: skill strings
    # can contain "/" (e.g. "CI/CD"), which would break path routing.
    return {"alias_key": alias.strip().lower(), "deleted": delete_skill_alias(conn, alias)}


@app.get("/digest")
def get_digest(
    conn: sqlite3.Connection = Depends(get_conn),
    _token: None = Depends(require_token),
) -> dict:
    digest = get_latest_digest(conn)
    if digest is None:
        raise HTTPException(status_code=404, detail="No digest has been generated yet")
    return digest


@app.get("/digests")
def list_digests_endpoint(
    limit: int = 20,
    conn: sqlite3.Connection = Depends(get_conn),
    _token: None = Depends(require_token),
) -> list[dict]:
    return list_digests(conn, limit=limit)
