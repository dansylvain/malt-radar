"""Structured shape of what the LLM extraction step produces for a mission.

Categories are deliberately narrow (see ROADMAP.md Phase 2: "catégorie
RAG/Agent/Vision/Fine-tuning") — this is a market-radar for AI engineering
freelance work, not a general job taxonomy. "Autre" is the honest fallback
when a mission doesn't fit, rather than forcing a bad guess.
"""

from __future__ import annotations

from typing import Literal

from pydantic import BaseModel, Field

Categorie = Literal["RAG", "Agent", "Vision", "Fine-tuning", "Autre"]
Niveau = Literal["junior", "confirmé", "senior", "expert"]


class Extraction(BaseModel):
    tjm_min: float | None = None
    tjm_max: float | None = None
    devise: str = "EUR"
    stack: list[str] = Field(default_factory=list)
    duree_valeur: int | None = None
    duree_unite: str | None = None
    niveau: Niveau | None = None
    categorie: Categorie = "Autre"
