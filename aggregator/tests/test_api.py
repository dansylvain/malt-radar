import os

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


def test_list_fiches_requires_token(client):
    response = client.get("/fiches")

    assert response.status_code == 401


def test_list_fiches_returns_parsed_frontmatter_newest_first(client, tmp_path):
    fiches_dir = tmp_path / "fiches"
    fiches_dir.mkdir()
    older = fiches_dir / "free_work_1.md"
    older.write_text(
        "---\ntitle: Consultant RAG\ntjm_min: 500\nstack:\n- Python\n- RAG\n---\n\n# Consultant RAG\n",
        encoding="utf-8",
    )
    newer = fiches_dir / "free_work_2.md"
    newer.write_text("---\ntitle: Consultant Vision\n---\n\n# Consultant Vision\n", encoding="utf-8")
    os.utime(older, (1_700_000_000, 1_700_000_000))
    os.utime(newer, (1_700_000_100, 1_700_000_100))

    response = client.get("/fiches", headers=auth_headers())

    assert response.status_code == 200
    body = response.json()
    assert [f["title"] for f in body] == ["Consultant Vision", "Consultant RAG"]
    assert body[1]["stack"] == ["Python", "RAG"]
    assert body[1]["filename"] == "free_work_1.md"


def test_list_fiches_skips_malformed_files(client, tmp_path):
    fiches_dir = tmp_path / "fiches"
    fiches_dir.mkdir()
    (fiches_dir / "broken.md").write_text("not a fiche at all", encoding="utf-8")
    (fiches_dir / "good.md").write_text("---\ntitle: OK\n---\n\n# OK\n", encoding="utf-8")

    response = client.get("/fiches", headers=auth_headers())

    assert response.status_code == 200
    body = response.json()
    assert len(body) == 1
    assert body[0]["title"] == "OK"


def test_list_fiches_empty_when_dir_missing(client):
    response = client.get("/fiches", headers=auth_headers())

    assert response.status_code == 200
    assert response.json() == []


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
