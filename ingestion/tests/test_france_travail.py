import time

import httpx
import pytest

from malt_radar_ingestion.sources import france_travail

RAW_SAMPLE = {
    "id": "197WXYZ",
    "intitule": "Ingénieur IA / LLM (H/F)",
    "appellationlibelle": "Ingénieur / Ingénieure intelligence artificielle",
    "romeLibelle": "Études et développement informatique",
    "competences": [{"libelle": "Python"}, {"libelle": "RAG"}],
    "lieuTravail": {"libelle": "Paris (75)"},
    "entreprise": {"nom": "Acme Corp"},
    "typeContratLibelle": "Contrat à durée déterminée - 6 Mois",
    "dateCreation": "2026-08-01T10:00:00.000Z",
    "salaire": {"libelle": "Annuel de 45000,00 Euros à 55000,00 Euros"},
    "origineOffre": {"urlOrigine": "https://candidat.francetravail.fr/offres/recherche/detail/197WXYZ"},
}


@pytest.fixture(autouse=True)
def no_real_sleep(monkeypatch):
    monkeypatch.setattr(time, "sleep", lambda seconds: None)


def test_to_mission_maps_fields():
    mission = france_travail._to_mission(RAW_SAMPLE)

    assert mission.source == "france_travail"
    assert mission.external_id == "197WXYZ"
    assert mission.title == "Ingénieur IA / LLM (H/F)"
    assert mission.job_name == "Ingénieur / Ingénieure intelligence artificielle"
    assert mission.skills == ["Python", "RAG"]
    assert mission.location_label == "Paris (75)"
    assert mission.company_name == "Acme Corp"
    assert mission.contracts == ["Contrat à durée déterminée - 6 Mois"]
    assert mission.min_daily_salary is None
    assert mission.raw_salary_text == "Annuel de 45000,00 Euros à 55000,00 Euros"
    assert mission.url == "https://candidat.francetravail.fr/offres/recherche/detail/197WXYZ"


def test_to_mission_handles_missing_optional_fields():
    raw = {**RAW_SAMPLE, "lieuTravail": None, "entreprise": None, "competences": [], "salaire": None, "origineOffre": None}

    mission = france_travail._to_mission(raw)

    assert mission.location_label is None
    assert mission.company_name is None
    assert mission.skills == []
    assert mission.raw_salary_text is None
    assert mission.url == "https://candidat.francetravail.fr/offres/recherche/detail/197WXYZ"


def test_get_access_token_sends_client_credentials_and_returns_token():
    captured = {}

    def handler(request: httpx.Request) -> httpx.Response:
        captured["url"] = str(request.url)
        captured["body"] = request.read().decode()
        return httpx.Response(200, json={"access_token": "tok-123", "expires_in": 1499})

    client = httpx.Client(transport=httpx.MockTransport(handler))

    token = france_travail.get_access_token("cid", "csecret", client=client)

    assert token == "tok-123"
    assert "grant_type=client_credentials" in captured["body"]
    assert "client_id=cid" in captured["body"]
    assert "client_secret=csecret" in captured["body"]


def test_fetch_missions_raises_without_credentials(monkeypatch):
    monkeypatch.delenv("FRANCE_TRAVAIL_CLIENT_ID", raising=False)
    monkeypatch.delenv("FRANCE_TRAVAIL_CLIENT_SECRET", raising=False)

    with pytest.raises(RuntimeError):
        list(france_travail.fetch_missions())


def test_fetch_missions_paginates_until_short_page():
    def handler(request: httpx.Request) -> httpx.Response:
        range_param = request.url.params.get("range")
        start = int(range_param.split("-")[0])
        if start == 0:
            results = [{**RAW_SAMPLE, "id": str(i)} for i in range(france_travail.PAGE_SIZE)]
        else:
            results = [{**RAW_SAMPLE, "id": "last"}]  # shorter than PAGE_SIZE -> stop
        return httpx.Response(206, json={"resultats": results})

    client = httpx.Client(transport=httpx.MockTransport(handler))

    missions = list(
        france_travail.fetch_missions(max_results=1000, client=client, token="fake-token")
    )

    assert len(missions) == france_travail.PAGE_SIZE + 1


def test_get_with_retry_backs_off_on_429():
    attempts = {"count": 0}

    def handler(request: httpx.Request) -> httpx.Response:
        attempts["count"] += 1
        if attempts["count"] < 2:
            return httpx.Response(429, headers={"Retry-After": "0"})
        return httpx.Response(200, json={"resultats": []})

    client = httpx.Client(transport=httpx.MockTransport(handler))

    response = france_travail._get_with_retry(client, france_travail.SEARCH_URL, {"range": "0-49"}, {})

    assert response.status_code == 200
    assert attempts["count"] == 2
