import json

from malt_radar_ingestion.models import Mission
from malt_radar_ingestion.storage import init_db, upsert_missions


def make_mission(**overrides) -> Mission:
    defaults = dict(
        source="free_work",
        external_id=1,
        slug="a-mission",
        url="https://www.free-work.com/fr/tech-it/jobs/a-mission",
        title="Consultant RAG",
        job_name="Data Engineer",
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


def test_upsert_inserts_new_mission():
    conn = init_db(":memory:")

    count = upsert_missions(conn, [make_mission()])

    assert count == 1
    row = conn.execute("SELECT title, skills, contracts, min_daily_salary FROM missions").fetchone()
    assert row[0] == "Consultant RAG"
    assert json.loads(row[1]) == ["RAG", "Python"]
    assert json.loads(row[2]) == ["contractor"]
    assert row[3] == 500


def test_upsert_is_idempotent_on_source_and_external_id():
    conn = init_db(":memory:")
    upsert_missions(conn, [make_mission(title="Old title")])

    upsert_missions(conn, [make_mission(title="New title")])

    rows = conn.execute("SELECT title FROM missions").fetchall()
    assert len(rows) == 1
    assert rows[0][0] == "New title"


def test_different_sources_with_same_external_id_are_distinct():
    conn = init_db(":memory:")

    upsert_missions(conn, [make_mission(source="free_work", external_id=1)])
    upsert_missions(conn, [make_mission(source="comet", external_id=1)])

    rows = conn.execute("SELECT source FROM missions ORDER BY source").fetchall()
    assert [r[0] for r in rows] == ["comet", "free_work"]
