"""Mechanical skill-string cleanup for extracted stacks.

The LLM extraction step (extract.py) doesn't produce clean skill strings —
stray whitespace and trailing punctuation are common. That noise-removal
happens here, once, at extraction time, since it's cheap and never needs
"undo" (unlike semantic/casing normalization — e.g. "RAG Systems" vs
"RAG", "python" vs "Python" — which now lives on the aggregator's
skill_aliases table and is applied at *display* time by the TUI, so
correcting an alias doesn't require rewriting already-generated fiches).
"""

from __future__ import annotations

import re

_WHITESPACE = re.compile(r"\s+")
_TRAILING_PUNCT = re.compile(r"[.,;:!?]+$")


def _clean(raw: str) -> str:
    text = _WHITESPACE.sub(" ", raw).strip()
    return _TRAILING_PUNCT.sub("", text).strip()


def normalize_skill(raw: str) -> str:
    """Strip whitespace/punctuation noise. Casing is left untouched —
    cross-fiche case folding is the aggregator alias table's job."""
    return _clean(raw)


def normalize_stack(stack: list[str]) -> list[str]:
    """Normalize each skill and dedupe, preserving first-seen order."""
    seen: set[str] = set()
    result: list[str] = []
    for raw in stack:
        skill = normalize_skill(raw)
        if not skill:
            continue
        key = skill.lower()
        if key in seen:
            continue
        seen.add(key)
        result.append(skill)
    return result
