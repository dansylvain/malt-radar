"""Tests for the network-facing behaviour of fetch_missions/_get_with_retry:
pagination control flow and 429 retry/backoff. All HTTP calls are mocked via
httpx.MockTransport — nothing here touches the real network."""

import time

import httpx
import pytest

from malt_radar_ingestion.sources import free_work


def _hydra_response(members, next_page=None, page=1):
    view = {"@id": f"/job_postings?page={page}"}
    if next_page:
        view["hydra:next"] = f"/job_postings?page={next_page}"
    return {"hydra:member": members, "hydra:view": view}


def _mission_raw(id_, contracts=("contractor",)):
    return {
        "id": id_,
        "slug": f"mission-{id_}",
        "title": f"Mission {id_}",
        "job": {"name": "Data Engineer"},
        "skills": [],
        "minDailySalary": None,
        "maxDailySalary": None,
        "currency": "EUR",
        "remoteMode": None,
        "contracts": list(contracts),
        "durationValue": None,
        "durationPeriod": None,
        "location": None,
        "company": None,
        "publishedAt": None,
    }


@pytest.fixture(autouse=True)
def no_real_sleep(monkeypatch):
    monkeypatch.setattr(time, "sleep", lambda seconds: None)


def test_fetch_missions_stops_when_no_next_page():
    calls = []

    def handler(request: httpx.Request) -> httpx.Response:
        calls.append(str(request.url))
        page = int(request.url.params.get("page", "1"))
        if page == 1:
            body = _hydra_response([_mission_raw(1)], next_page=2, page=1)
        else:
            body = _hydra_response([_mission_raw(2)], next_page=None, page=2)
        return httpx.Response(200, json=body)

    client = httpx.Client(transport=httpx.MockTransport(handler))

    missions = list(free_work.fetch_missions(max_pages=5, client=client))

    assert len(calls) == 2  # stopped after page 2, ignored the max_pages=5 budget
    assert [m.external_id for m in missions] == [1, 2]


def test_fetch_missions_stops_at_max_pages_even_if_more_available():
    calls = []

    def handler(request: httpx.Request) -> httpx.Response:
        calls.append(str(request.url))
        page = int(request.url.params.get("page", "1"))
        body = _hydra_response([_mission_raw(page)], next_page=page + 1, page=page)
        return httpx.Response(200, json=body)

    client = httpx.Client(transport=httpx.MockTransport(handler))

    missions = list(free_work.fetch_missions(max_pages=2, client=client))

    assert len(calls) == 2
    assert [m.external_id for m in missions] == [1, 2]


def test_fetch_missions_filters_non_freelance_contracts():
    def handler(request: httpx.Request) -> httpx.Response:
        body = _hydra_response(
            [_mission_raw(1, contracts=["permanent"]), _mission_raw(2, contracts=["contractor"])],
        )
        return httpx.Response(200, json=body)

    client = httpx.Client(transport=httpx.MockTransport(handler))

    missions = list(free_work.fetch_missions(max_pages=1, client=client))

    assert [m.external_id for m in missions] == [2]


def test_get_with_retry_backs_off_then_succeeds():
    attempts = {"count": 0}

    def handler(request: httpx.Request) -> httpx.Response:
        attempts["count"] += 1
        if attempts["count"] < 3:
            return httpx.Response(429, headers={"Retry-After": "0"})
        return httpx.Response(200, json={"ok": True})

    client = httpx.Client(transport=httpx.MockTransport(handler))

    response = free_work._get_with_retry(client, "https://example.test/x")

    assert response.status_code == 200
    assert attempts["count"] == 3


def test_get_with_retry_raises_after_exhausting_retries():
    attempts = {"count": 0}

    def handler(request: httpx.Request) -> httpx.Response:
        attempts["count"] += 1
        return httpx.Response(429, headers={"Retry-After": "0"})

    client = httpx.Client(transport=httpx.MockTransport(handler))

    with pytest.raises(httpx.HTTPStatusError):
        free_work._get_with_retry(client, "https://example.test/x")

    assert attempts["count"] == free_work.MAX_RETRIES
