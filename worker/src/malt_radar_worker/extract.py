"""Ollama-backed entity extraction: turns a mission's raw text into a
structured Extraction (TJM, stack, durée, niveau, catégorie).

Runs against a local Ollama (qwen2.5:3b — see ../ROADMAP.md Phase 2),
never over Tailscale: the worker and Ollama both live on the Pi5.
"""

from __future__ import annotations

import json
import os
import re

import httpx
from pydantic import ValidationError

from malt_radar_worker.schema import Extraction

OLLAMA_HOST = os.environ.get("OLLAMA_HOST", "http://localhost:11434")
OLLAMA_MODEL = os.environ.get("OLLAMA_MODEL", "qwen2.5:3b")
MAX_RETRIES = 3
DESCRIPTION_CHAR_BUDGET = 4000

_SCHEMA_HINT = """Réponds UNIQUEMENT avec un objet JSON valide, sans aucun texte autour, avec exactement ces clés :
{
  "tjm_min": nombre ou null,
  "tjm_max": nombre ou null,
  "devise": "EUR",
  "stack": ["liste", "de", "technologies"],
  "duree_valeur": nombre entier ou null,
  "duree_unite": "jour" ou "mois" ou "an" ou null,
  "niveau": "junior" ou "confirmé" ou "senior" ou "expert" ou null,
  "categorie": "RAG" ou "Agent" ou "Vision" ou "Fine-tuning" ou "Autre"
}"""

_HTML_TAG = re.compile(r"<[^>]+>")


class ExtractionFailed(RuntimeError):
    """Raised when the model never returned valid, schema-conformant JSON."""


def _strip_html(text: str) -> str:
    return _HTML_TAG.sub(" ", text)


def build_prompt(mission: dict) -> str:
    description = _strip_html(mission.get("description") or "")[:DESCRIPTION_CHAR_BUDGET]
    context = "\n".join(
        [
            f"Titre: {mission.get('title', '')}",
            f"Métier: {mission.get('job_name', '')}",
            f"Compétences déjà listées: {', '.join(mission.get('skills') or [])}",
            f"TJM min/max déjà connus: {mission.get('min_daily_salary')}/{mission.get('max_daily_salary')}",
            f"Texte salaire brut: {mission.get('raw_salary_text') or ''}",
            f"Durée déjà connue: {mission.get('duration_value')} {mission.get('duration_period') or ''}",
            f"Description: {description}",
        ]
    )
    return (
        "Tu extrais des informations structurées d'une offre de mission freelance "
        "en France dans le domaine de l'IA/Data engineering. Utilise les champs déjà "
        "connus comme indices mais corrige-les si la description les contredit. "
        "Ajoute à stack toute compétence qui n'apparaît que dans la description. "
        "La catégorie doit refléter le sujet dominant de la mission — choisis "
        "\"Autre\" plutôt que de forcer une catégorie qui ne correspond pas.\n\n"
        f"{context}\n\n{_SCHEMA_HINT}"
    )


def extract(mission: dict, client: httpx.Client | None = None) -> Extraction:
    """Call Ollama, validate its JSON output against Extraction, retrying on
    malformed/invalid output. Raises ExtractionFailed once retries run out."""
    owns_client = client is None
    if owns_client:
        client = httpx.Client(timeout=60.0)
    try:
        prompt = build_prompt(mission)
        last_error: Exception | None = None
        for _attempt in range(1, MAX_RETRIES + 1):
            response = client.post(
                f"{OLLAMA_HOST}/api/generate",
                json={"model": OLLAMA_MODEL, "prompt": prompt, "format": "json", "stream": False},
            )
            response.raise_for_status()
            raw_output = response.json()["response"]
            try:
                data = json.loads(raw_output)
                return Extraction.model_validate(data)
            except (json.JSONDecodeError, ValidationError) as exc:
                last_error = exc
                continue
        raise ExtractionFailed(
            f"Ollama never returned valid JSON matching Extraction after {MAX_RETRIES} attempts: {last_error}"
        )
    finally:
        if owns_client:
            client.close()
