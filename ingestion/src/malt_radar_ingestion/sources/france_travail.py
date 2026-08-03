"""France Travail client: official "Offres d'emploi v2" API (OAuth2
client_credentials). See SOURCES.md — the only source here with no legal
ambiguity, but it requires a developer account.

Setup (one-time, done by a human, not by this code):
1. Create an account on https://francetravail.io and register an
   "application".
2. Subscribe that application to the "Offres d'emploi v2" API product.
3. francetravail.io gives you a client_id/client_secret and the exact
   scope string to request — it's usually "api_offresdemploiv2 o2dsoffre"
   but confirm it in your app's dashboard before relying on the default
   below, since this was implemented against public docs without a live
   account to verify against.
4. Set FRANCE_TRAVAIL_CLIENT_ID / FRANCE_TRAVAIL_CLIENT_SECRET (e.g. in a
   local .env, already gitignored).

Note: France Travail is mostly a salaried-job board, not a freelance TJM
board like Free-Work. Its `salaire` field is free text (often an annual
salary range), not a clean daily rate — kept as `raw_salary_text` on the
Mission for the Phase 2 LLM extraction step to parse later, rather than
guessing a TJM here.
"""

from __future__ import annotations

import os
import time
from typing import Any, Iterator

import httpx

from malt_radar_ingestion.models import Mission

TOKEN_URL = "https://entreprise.francetravail.fr/connexion/oauth2/access_token?realm=%2Fpartenaire"
SEARCH_URL = "https://api.francetravail.io/partenaire/offresdemploi/v2/offres/search"
DEFAULT_SCOPE = "api_offresdemploiv2 o2dsoffre"
PAGE_SIZE = 50
REQUEST_DELAY_SECONDS = 1.0  # documented limit: 3 req/s
MAX_RETRIES = 3


def get_access_token(
    client_id: str,
    client_secret: str,
    client: httpx.Client | None = None,
    scope: str = DEFAULT_SCOPE,
) -> str:
    owns_client = client is None
    if owns_client:
        client = httpx.Client(timeout=15.0)
    try:
        response = client.post(
            TOKEN_URL,
            data={
                "grant_type": "client_credentials",
                "client_id": client_id,
                "client_secret": client_secret,
                "scope": scope,
            },
            headers={"Content-Type": "application/x-www-form-urlencoded"},
        )
        response.raise_for_status()
        return response.json()["access_token"]
    finally:
        if owns_client:
            client.close()


def _get_with_retry(client: httpx.Client, url: str, params: dict[str, Any], headers: dict[str, str]) -> httpx.Response:
    response: httpx.Response | None = None
    for attempt in range(1, MAX_RETRIES + 1):
        response = client.get(url, params=params, headers=headers)
        if response.status_code == 429:
            wait = float(response.headers.get("Retry-After", attempt * 5))
            time.sleep(wait)
            continue
        if response.status_code in (200, 206):
            return response
        response.raise_for_status()
        return response
    response.raise_for_status()
    return response


def _to_mission(raw: dict[str, Any]) -> Mission:
    lieu = raw.get("lieuTravail") or {}
    entreprise = raw.get("entreprise") or {}
    salaire = raw.get("salaire") or {}
    contract_label = raw.get("typeContratLibelle") or raw.get("typeContrat")
    origine = raw.get("origineOffre") or {}
    return Mission(
        source="france_travail",
        external_id=raw["id"],
        slug=raw["id"],
        url=origine.get("urlOrigine") or f"https://candidat.francetravail.fr/offres/recherche/detail/{raw['id']}",
        title=raw.get("intitule", ""),
        job_name=raw.get("appellationlibelle") or raw.get("romeLibelle") or "",
        skills=[c["libelle"] for c in raw.get("competences", []) if c.get("libelle")],
        min_daily_salary=None,
        max_daily_salary=None,
        currency="EUR",
        remote_mode=None,
        contracts=[contract_label] if contract_label else [],
        duration_value=None,
        duration_period=None,
        location_label=lieu.get("libelle"),
        company_name=entreprise.get("nom"),
        published_at=raw.get("dateCreation"),
        raw_salary_text=salaire.get("libelle") or salaire.get("commentaire"),
    )


def fetch_missions(
    mots_cles: str = "intelligence artificielle",
    max_results: int = 150,
    client_id: str | None = None,
    client_secret: str | None = None,
    client: httpx.Client | None = None,
    token: str | None = None,
) -> Iterator[Mission]:
    """Yield missions from France Travail's "Offres d'emploi v2" API.

    `client` and `token` are injection points for tests — leave unset in
    normal use. Credentials default to the FRANCE_TRAVAIL_CLIENT_ID /
    FRANCE_TRAVAIL_CLIENT_SECRET environment variables.
    """
    client_id = client_id or os.environ.get("FRANCE_TRAVAIL_CLIENT_ID")
    client_secret = client_secret or os.environ.get("FRANCE_TRAVAIL_CLIENT_SECRET")

    owns_client = client is None
    if owns_client:
        client = httpx.Client(timeout=15.0)
    try:
        if token is None:
            if not client_id or not client_secret:
                raise RuntimeError(
                    "Missing France Travail credentials: set FRANCE_TRAVAIL_CLIENT_ID and "
                    "FRANCE_TRAVAIL_CLIENT_SECRET (see france_travail.py module docstring)."
                )
            token = get_access_token(client_id, client_secret, client=client)
        headers = {"Authorization": f"Bearer {token}"}

        start = 0
        fetched = 0
        while fetched < max_results:
            end = min(start + PAGE_SIZE, max_results) - 1
            params = {"motsCles": mots_cles, "range": f"{start}-{end}"}
            response = _get_with_retry(client, SEARCH_URL, params, headers)
            data = response.json()
            results = data.get("resultats", [])
            if not results:
                break
            for raw in results:
                yield _to_mission(raw)
            fetched += len(results)
            if len(results) < (end - start + 1):
                break  # last page
            start = end + 1
            time.sleep(REQUEST_DELAY_SECONDS)
    finally:
        if owns_client:
            client.close()


if __name__ == "__main__":
    for mission in fetch_missions(max_results=20):
        print(mission.title, "-", mission.raw_salary_text, mission.skills)
