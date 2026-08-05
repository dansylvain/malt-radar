"""Fiche reading: parses the YAML frontmatter the worker writes into each
generated Markdown fiche.

Extracted from `api.py` (originally only used by `GET /fiches`) so
`digest.py` can also read the full fiche corpus without duplicating the
frontmatter-parsing logic.
"""

from __future__ import annotations

import re
from pathlib import Path

import yaml

FICHES_DIR = Path(__file__).resolve().parent.parent.parent / "data" / "fiches"
_FRONTMATTER_RE = re.compile(r"\A---\n(.*?\n)---\n", re.DOTALL)


def read_fiche(path: Path) -> dict | None:
    """Parse a fiche's YAML frontmatter. Returns None if the file doesn't
    start with a well-formed `---`-delimited block (skipped rather than
    failing the whole listing — one bad file shouldn't blank the caller)."""
    match = _FRONTMATTER_RE.match(path.read_text(encoding="utf-8"))
    if match is None:
        return None
    frontmatter = yaml.safe_load(match.group(1))
    if not isinstance(frontmatter, dict):
        return None
    frontmatter["filename"] = path.name
    return frontmatter


def list_fiches(fiches_dir: Path, limit: int | None = None) -> list[dict]:
    """Directory listing + parsing, newest-first by mtime. `limit=None`
    (the default) returns the whole corpus — `digest.py` needs every fiche
    to slice into trend windows, unlike `GET /fiches`'s capped feed."""
    if not fiches_dir.exists():
        return []
    paths = sorted(fiches_dir.glob("*.md"), key=lambda p: p.stat().st_mtime, reverse=True)
    if limit is not None:
        paths = paths[:limit]
    fiches = []
    for path in paths:
        fiche = read_fiche(path)
        if fiche is not None:
            fiches.append(fiche)
    return fiches
