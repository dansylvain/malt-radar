import json

import httpx
import pytest

from malt_radar_worker.extract import ExtractionFailed, build_prompt, extract

VALID_JSON = json.dumps(
    {
        "tjm_min": 500,
        "tjm_max": 700,
        "devise": "EUR",
        "stack": ["Python", "RAG"],
        "duree_valeur": 6,
        "duree_unite": "mois",
        "niveau": "senior",
        "categorie": "RAG",
    }
)

MISSION = {
    "id": 1,
    "title": "Ingénieur RAG",
    "job_name": "Data Engineer",
    "skills": ["Python"],
    "min_daily_salary": None,
    "max_daily_salary": None,
    "raw_salary_text": None,
    "duration_value": None,
    "duration_period": None,
    "description": "<p>Mission de mise en place d'un pipeline RAG.</p>",
}


def _ollama_response(body: str) -> httpx.Response:
    return httpx.Response(200, json={"response": body})


def test_build_prompt_strips_html_and_includes_known_fields():
    prompt = build_prompt(MISSION)

    assert "<p>" not in prompt
    assert "Ingénieur RAG" in prompt
    assert "Python" in prompt


def test_extract_returns_validated_extraction_on_first_success():
    calls = []

    def handler(request: httpx.Request) -> httpx.Response:
        calls.append(request)
        return _ollama_response(VALID_JSON)

    client = httpx.Client(transport=httpx.MockTransport(handler))

    extraction = extract(MISSION, client=client)

    assert len(calls) == 1
    assert extraction.categorie == "RAG"
    assert extraction.stack == ["Python", "RAG"]


def test_extract_retries_on_malformed_json_then_succeeds():
    attempts = {"count": 0}

    def handler(request: httpx.Request) -> httpx.Response:
        attempts["count"] += 1
        if attempts["count"] < 3:
            return _ollama_response("not json")
        return _ollama_response(VALID_JSON)

    client = httpx.Client(transport=httpx.MockTransport(handler))

    extraction = extract(MISSION, client=client)

    assert attempts["count"] == 3
    assert extraction.categorie == "RAG"


def test_extract_retries_on_schema_violation():
    attempts = {"count": 0}
    invalid_categorie = json.dumps({**json.loads(VALID_JSON), "categorie": "Blockchain"})

    def handler(request: httpx.Request) -> httpx.Response:
        attempts["count"] += 1
        if attempts["count"] < 2:
            return _ollama_response(invalid_categorie)
        return _ollama_response(VALID_JSON)

    client = httpx.Client(transport=httpx.MockTransport(handler))

    extraction = extract(MISSION, client=client)

    assert attempts["count"] == 2
    assert extraction.categorie == "RAG"


def test_extract_raises_after_exhausting_retries():
    def handler(request: httpx.Request) -> httpx.Response:
        return _ollama_response("not json")

    client = httpx.Client(transport=httpx.MockTransport(handler))

    with pytest.raises(ExtractionFailed):
        extract(MISSION, client=client)


def test_extract_retries_on_read_timeout_then_succeeds():
    attempts = {"count": 0}

    def handler(request: httpx.Request) -> httpx.Response:
        attempts["count"] += 1
        if attempts["count"] < 2:
            raise httpx.ReadTimeout("timed out", request=request)
        return _ollama_response(VALID_JSON)

    client = httpx.Client(transport=httpx.MockTransport(handler))

    extraction = extract(MISSION, client=client)

    assert attempts["count"] == 2
    assert extraction.categorie == "RAG"


def test_extract_raises_extraction_failed_after_repeated_timeouts():
    def handler(request: httpx.Request) -> httpx.Response:
        raise httpx.ReadTimeout("timed out", request=request)

    client = httpx.Client(transport=httpx.MockTransport(handler))

    with pytest.raises(ExtractionFailed):
        extract(MISSION, client=client)
