import pytest
from fastapi.testclient import TestClient

from malt_radar_aggregator import api
from malt_radar_ingestion.models import Mission
from malt_radar_ingestion.storage import init_db, upsert_missions

TOKEN = "test-token"


def make_mission(**overrides) -> Mission:
    defaults = dict(
        source="free_work",
        external_id=1,
        slug="a-mission",
        url="https://www.free-work.com/fr/tech-it/jobs/a-mission",
        title="Consultant RAG",
        job_name="Data Engineer",
        description="Texte complet de l'annonce.",
        skills=["RAG", "Python"],
        min_daily_salary=500,
        max_daily_salary=700,
        currency="EUR",
        remote_mode="full",
        contracts=["contractor"],
        duration_value=6,
        duration_period="month",
        location_label="Paris",
        company_name="Acme",
        published_at="2026-08-03T10:00:00+02:00",
    )
    defaults.update(overrides)
    return Mission(**defaults)


@pytest.fixture
def conn():
    return init_db(":memory:")


@pytest.fixture
def client(conn, tmp_path, monkeypatch):
    monkeypatch.setenv("AGGREGATOR_API_TOKEN", TOKEN)
    monkeypatch.setattr(api, "FICHES_DIR", tmp_path / "fiches")
    api.app.dependency_overrides[api.get_conn] = lambda: conn
    try:
        yield TestClient(api.app)
    finally:
        api.app.dependency_overrides.clear()


def auth_headers():
    return {"Authorization": f"Bearer {TOKEN}"}


def test_list_unprocessed_requires_token(client):
    response = client.get("/missions/unprocessed")

    assert response.status_code == 401


def test_list_unprocessed_rejects_wrong_token(client):
    response = client.get("/missions/unprocessed", headers={"Authorization": "Bearer wrong"})

    assert response.status_code == 401


def test_list_unprocessed_returns_missions(conn, client):
    upsert_missions(conn, [make_mission()])

    response = client.get("/missions/unprocessed", headers=auth_headers())

    assert response.status_code == 200
    body = response.json()
    assert len(body) == 1
    assert body[0]["title"] == "Consultant RAG"
    assert body[0]["skills"] == ["RAG", "Python"]


def test_submit_fiche_writes_file_and_marks_processed(conn, client, tmp_path):
    upsert_missions(conn, [make_mission()])
    mission_id = conn.execute("SELECT id FROM missions").fetchone()[0]

    response = client.post(
        "/fiches",
        json={"mission_id": mission_id, "markdown": "---\ntitle: test\n---\nfiche"},
        headers=auth_headers(),
    )

    assert response.status_code == 201
    filename = response.json()["filename"]
    written = (tmp_path / "fiches" / filename).read_text(encoding="utf-8")
    assert written == "---\ntitle: test\n---\nfiche"

    row = conn.execute("SELECT processed FROM missions WHERE id = ?", (mission_id,)).fetchone()
    assert row[0] == 1

    unprocessed = client.get("/missions/unprocessed", headers=auth_headers()).json()
    assert unprocessed == []


def test_submit_fiche_unknown_mission_returns_404(client):
    response = client.post(
        "/fiches",
        json={"mission_id": 999, "markdown": "fiche"},
        headers=auth_headers(),
    )

    assert response.status_code == 404


def test_missing_server_token_config_returns_503(conn, tmp_path, monkeypatch):
    monkeypatch.delenv("AGGREGATOR_API_TOKEN", raising=False)
    monkeypatch.setattr(api, "FICHES_DIR", tmp_path / "fiches")
    api.app.dependency_overrides[api.get_conn] = lambda: conn
    try:
        client = TestClient(api.app)
        response = client.get("/missions/unprocessed", headers=auth_headers())
    finally:
        api.app.dependency_overrides.clear()

    assert response.status_code == 503
