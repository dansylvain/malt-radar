"""Skill-name normalization for extracted stacks.

The LLM extraction step (extract.py) doesn't name the same skill
consistently across missions (casing, trailing punctuation, or an
outright synonym like "RAG Systems" vs "RAG") — left alone this dilutes
the TUI's Top Skills counts across near-duplicate labels. Normalization
happens here, once, in the worker, rather than as a TUI-side patch.
"""

from __future__ import annotations

import re

# Alias (already lowercased/whitespace-collapsed) -> canonical display form.
# Hand-maintained: add an entry whenever a real fiche surfaces a known
# variant of an existing skill. Not meant to be an exhaustive taxonomy —
# semantic merging is deliberately grown from real data, not pre-built.
CANONICAL_ALIASES: dict[str, str] = {
    "rag": "RAG",
    "rag systems": "RAG",
}

_WHITESPACE = re.compile(r"\s+")
_TRAILING_PUNCT = re.compile(r"[.,;:!?]+$")


def _clean(raw: str) -> str:
    text = _WHITESPACE.sub(" ", raw).strip()
    return _TRAILING_PUNCT.sub("", text).strip()


def normalize_skill(raw: str) -> str:
    """Fold casing/punctuation noise and known synonyms into one canonical
    label. Unknown skills are returned mechanically cleaned, as-is."""
    cleaned = _clean(raw)
    canonical = CANONICAL_ALIASES.get(cleaned.lower())
    return canonical if canonical is not None else cleaned


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
