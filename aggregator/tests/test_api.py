import os

import pytest
from fastapi.testclient import TestClient

from malt_radar_aggregator import api
from malt_radar_aggregator.storage import init_digests_table, init_skill_aliases_table, save_digest
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
    # api.get_conn() is bypassed by the `client` fixture's dependency
    # override below, so init_skill_aliases_table must be called here too
    # (get_conn() only runs it lazily on the first real request).
    c = init_db(":memory:")
    init_skill_aliases_table(c)
    init_digests_table(c)
    return c


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


def test_delete_fiche_requires_token(client, tmp_path):
    fiches_dir = tmp_path / "fiches"
    fiches_dir.mkdir()
    (fiches_dir / "good.md").write_text("---\ntitle: OK\n---\n\n# OK\n", encoding="utf-8")

    response = client.delete("/fiches/good.md")

    assert response.status_code == 401
    assert (fiches_dir / "good.md").exists()


def test_delete_fiche_removes_file(client, tmp_path):
    fiches_dir = tmp_path / "fiches"
    fiches_dir.mkdir()
    (fiches_dir / "good.md").write_text("---\ntitle: OK\n---\n\n# OK\n", encoding="utf-8")

    response = client.delete("/fiches/good.md", headers=auth_headers())

    assert response.status_code == 200
    assert response.json() == {"filename": "good.md", "deleted": True}
    assert not (fiches_dir / "good.md").exists()


def test_delete_fiche_missing_reports_false_not_404(client, tmp_path):
    (tmp_path / "fiches").mkdir()

    response = client.delete("/fiches/does-not-exist.md", headers=auth_headers())

    assert response.status_code == 200
    assert response.json() == {"filename": "does-not-exist.md", "deleted": False}


def test_delete_fiche_rejects_path_traversal(client, tmp_path):
    outside = tmp_path / "secret.txt"
    outside.write_text("do not touch", encoding="utf-8")
    (tmp_path / "fiches").mkdir()

    response = client.delete("/fiches/..%2Fsecret.txt", headers=auth_headers())

    assert response.status_code in (400, 404)
    assert outside.exists()


def test_get_skill_aliases_requires_token(client):
    response = client.get("/skill-aliases")

    assert response.status_code == 401


def test_get_skill_aliases_returns_empty_list_initially(client):
    response = client.get("/skill-aliases", headers=auth_headers())

    assert response.status_code == 200
    assert response.json() == []


def test_put_skill_alias_requires_token(client):
    response = client.put("/skill-aliases", json={"alias": "rag", "canonical": "RAG"})

    assert response.status_code == 401


def test_put_skill_alias_creates_row(client):
    response = client.put("/skill-aliases", json={"alias": "PowerBI", "canonical": "Power BI"}, headers=auth_headers())

    assert response.status_code == 200
    body = response.json()
    assert body["alias_key"] == "powerbi"
    assert body["canonical"] == "Power BI"

    listed = client.get("/skill-aliases", headers=auth_headers()).json()
    assert len(listed) == 1
    assert listed[0]["alias_key"] == "powerbi"


def test_put_skill_alias_upserts_existing_key_case_insensitively(client):
    client.put("/skill-aliases", json={"alias": "PowerBI", "canonical": "Power BI"}, headers=auth_headers())

    response = client.put(
        "/skill-aliases", json={"alias": "powerbi", "canonical": "Microsoft Power BI"}, headers=auth_headers()
    )

    assert response.status_code == 200
    listed = client.get("/skill-aliases", headers=auth_headers()).json()
    assert len(listed) == 1
    assert listed[0]["canonical"] == "Microsoft Power BI"


def test_put_skill_alias_rejects_empty_alias(client):
    response = client.put("/skill-aliases", json={"alias": "   ", "canonical": "RAG"}, headers=auth_headers())

    assert response.status_code == 422


def test_put_skill_alias_rejects_empty_canonical(client):
    response = client.put("/skill-aliases", json={"alias": "rag", "canonical": "  "}, headers=auth_headers())

    assert response.status_code == 422


def test_delete_skill_alias_requires_token(client):
    response = client.delete("/skill-aliases", params={"alias": "rag"})

    assert response.status_code == 401


def test_delete_skill_alias_removes_and_reports_true(client):
    client.put("/skill-aliases", json={"alias": "rag", "canonical": "RAG"}, headers=auth_headers())

    response = client.delete("/skill-aliases", params={"alias": "rag"}, headers=auth_headers())

    assert response.status_code == 200
    assert response.json() == {"alias_key": "rag", "deleted": True}
    assert client.get("/skill-aliases", headers=auth_headers()).json() == []


def test_delete_skill_alias_missing_reports_false_not_404(client):
    response = client.delete("/skill-aliases", params={"alias": "does-not-exist"}, headers=auth_headers())

    assert response.status_code == 200
    assert response.json() == {"alias_key": "does-not-exist", "deleted": False}


def test_delete_skill_alias_missing_query_param_returns_422(client):
    response = client.delete("/skill-aliases", headers=auth_headers())

    assert response.status_code == 422


def make_digest_kwargs(**overrides) -> dict:
    defaults = dict(
        window_start="2026-07-29T00:00:00+00:00",
        window_end="2026-08-05T00:00:00+00:00",
        window_days=7,
        model="claude-haiku-4-5",
        fiche_count=3,
        metrics={"fiche_count": 3},
        trend={"delta": None},
        content="## Résumé exécutif\n...",
    )
    defaults.update(overrides)
    return defaults


def test_get_digest_requires_token(client):
    response = client.get("/digest")

    assert response.status_code == 401


def test_get_digest_returns_404_when_none_generated(client):
    response = client.get("/digest", headers=auth_headers())

    assert response.status_code == 404


def test_get_digest_returns_latest_with_parsed_metrics(conn, client):
    save_digest(conn, **make_digest_kwargs())

    response = client.get("/digest", headers=auth_headers())

    assert response.status_code == 200
    body = response.json()
    assert body["content"] == "## Résumé exécutif\n..."
    assert body["metrics"] == {"fiche_count": 3}
    assert body["trend"] == {"delta": None}


def test_get_digests_requires_token(client):
    response = client.get("/digests")

    assert response.status_code == 401


def test_get_digests_returns_history_ordered_and_limited(conn, client):
    for i in range(3):
        save_digest(conn, **make_digest_kwargs(fiche_count=i))

    response = client.get("/digests", params={"limit": 2}, headers=auth_headers())

    assert response.status_code == 200
    body = response.json()
    assert len(body) == 2
    assert [d["fiche_count"] for d in body] == [2, 1]


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
