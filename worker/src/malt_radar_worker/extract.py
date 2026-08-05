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
from malt_radar_worker.skills import normalize_stack

OLLAMA_HOST = os.environ.get("OLLAMA_HOST", "http://localhost:11434")
OLLAMA_MODEL = os.environ.get("OLLAMA_MODEL", "qwen2.5:3b")
# CPU-only inference on a Pi5 is slow and varies a lot with description
# length — 60s turned out too tight in practice and blew up as an
# unhandled httpx.ReadTimeout instead of a clean retry.
OLLAMA_TIMEOUT_SECONDS = float(os.environ.get("OLLAMA_TIMEOUT_SECONDS", "180"))
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
}

Précisions sur tjm_min/tjm_max : ce sont des TAUX JOURNALIERS MOYENS (TJM)
en euros, pas un salaire annuel ni un montant total de mission. Un TJM
plausible pour une mission freelance IA/Data en France se situe entre 300
et 1500 €/jour, rarement jusqu'à 2000-2500 € pour un profil senior ou une
expertise rare. Si le texte mentionne un salaire annuel, un montant global
de mission, ou un chiffre dont tu n'es pas sûr qu'il s'agisse bien d'un TJM
journalier, renvoie null plutôt que de deviner ou de convertir toi-même."""

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
            "Catégorie de métier Free-Work (classification large — les exemples "
            "entre parenthèses illustrent le type de métier en général, ce ne sont "
            f"PAS des compétences requises pour cette mission précise): {mission.get('job_name', '')}",
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
        "La stack ne doit contenir QUE des technologies mentionnées dans "
        "\"Compétences déjà listées\" ou dans \"Description\". Le Titre et la "
        "Catégorie de métier servent uniquement de contexte pour comprendre la "
        "mission — ce ne sont jamais des sources pour la stack, que le terme y "
        "apparaisse ou non n'a aucune importance : seul compte ce qui est écrit "
        "dans \"Compétences déjà listées\" ou \"Description\". Si aucune "
        "technologie précise n'y apparaît, renvoie stack: [] plutôt que d'en "
        "inventer une. "
        "La catégorie doit refléter le sujet dominant de la mission — choisis "
        "\"Autre\" plutôt que de forcer une catégorie qui ne correspond pas. "
        "Le TJM (tjm_min/tjm_max) doit rester un taux JOURNALIER — si \"Texte "
        "salaire brut\" ou la description semble donner un salaire annuel, un "
        "forfait total, ou un chiffre ambigu, ne le convertis pas toi-même : "
        "renvoie null.\n\n"
        f"{context}\n\n{_SCHEMA_HINT}"
    )


def extract(mission: dict, client: httpx.Client | None = None) -> Extraction:
    """Call Ollama, validate its JSON output against Extraction, retrying on
    malformed/invalid output. Raises ExtractionFailed once retries run out."""
    owns_client = client is None
    if owns_client:
        client = httpx.Client(timeout=OLLAMA_TIMEOUT_SECONDS)
    try:
        prompt = build_prompt(mission)
        last_error: Exception | None = None
        for _attempt in range(1, MAX_RETRIES + 1):
            try:
                response = client.post(
                    f"{OLLAMA_HOST}/api/generate",
                    json={
                        "model": OLLAMA_MODEL,
                        "prompt": prompt,
                        # A full JSON Schema here (Ollama >=0.5) constrains
                        # decoding token-by-token — categorie/niveau literally
                        # can't leave their enum, unlike plain "format": "json"
                        # which only guarantees syntactically valid JSON and
                        # let the model put e.g. a job title in categorie.
                        "format": Extraction.model_json_schema(),
                        "stream": False,
                        # Deterministic extraction, not creative generation —
                        # Ollama's default (~0.8) was sampling a different
                        # categorie/stack on every call for the same input.
                        "options": {"temperature": 0},
                    },
                )
                response.raise_for_status()
                raw_output = response.json()["response"]
                data = json.loads(raw_output)
                extraction = Extraction.model_validate(data)
                extraction.stack = normalize_stack(extraction.stack)
                return extraction
            except (httpx.HTTPError, json.JSONDecodeError, ValidationError) as exc:
                last_error = exc
                continue
        raise ExtractionFailed(
            f"Ollama never returned valid JSON matching Extraction after {MAX_RETRIES} attempts: {last_error}"
        )
    finally:
        if owns_client:
            client.close()
