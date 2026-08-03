import httpx
import pytest

from malt_radar_worker import client


@pytest.fixture(autouse=True)
def aggregator_env(monkeypatch):
    monkeypatch.setenv("AGGREGATOR_URL", "https://aggregator.test")
    monkeypatch.setenv("AGGREGATOR_API_TOKEN", "secret-token")


def test_fetch_unprocessed_sends_bearer_token_and_limit():
    captured = {}

    def handler(request: httpx.Request) -> httpx.Response:
        captured["headers"] = request.headers
        captured["params"] = dict(request.url.params)
        return httpx.Response(200, json=[{"id": 1, "title": "Mission A"}])

    mock_client = httpx.Client(transport=httpx.MockTransport(handler), base_url="https://aggregator.test")

    missions = client.fetch_unprocessed(limit=10, client=mock_client)

    assert missions == [{"id": 1, "title": "Mission A"}]
    assert captured["headers"]["authorization"] == "Bearer secret-token"
    assert captured["params"] == {"limit": "10"}


def test_push_fiche_sends_mission_id_and_markdown():
    captured = {}

    def handler(request: httpx.Request) -> httpx.Response:
        captured["body"] = request.read()
        return httpx.Response(201, json={"mission_id": 1, "filename": "free_work_1.md"})

    mock_client = httpx.Client(transport=httpx.MockTransport(handler), base_url="https://aggregator.test")

    result = client.push_fiche(1, "---\ntitle: x\n---\n", client=mock_client)

    assert result == {"mission_id": 1, "filename": "free_work_1.md"}
    assert b'"mission_id":1' in captured["body"]


def test_fetch_unprocessed_raises_without_url(monkeypatch):
    monkeypatch.delenv("AGGREGATOR_URL", raising=False)

    with pytest.raises(RuntimeError):
        client.fetch_unprocessed()


def test_fetch_unprocessed_raises_without_token(monkeypatch):
    monkeypatch.delenv("AGGREGATOR_API_TOKEN", raising=False)

    with pytest.raises(RuntimeError):
        client.fetch_unprocessed(client=httpx.Client())
