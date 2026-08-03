import json

from malt_radar_ingestion.models import Mission
from malt_radar_ingestion.storage import fetch_unprocessed, get_mission, init_db, mark_processed, upsert_missions


def make_mission(**overrides) -> Mission:
    defaults = dict(
        source="free_work",
        external_id=1,
        slug="a-mission",
        url="https://www.free-work.com/fr/tech-it/jobs/a-mission",
        title="Consultant RAG",
        job_name="Data Engineer",
        description="Mission de conseil RAG pour un client grand compte.",
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


def test_description_is_persisted():
    conn = init_db(":memory:")

    upsert_missions(conn, [make_mission(description="Texte complet de l'annonce.")])

    row = conn.execute("SELECT description FROM missions").fetchone()
    assert row[0] == "Texte complet de l'annonce."


def test_new_missions_are_unprocessed_by_default():
    conn = init_db(":memory:")

    upsert_missions(conn, [make_mission()])

    row = conn.execute("SELECT processed, processed_at FROM missions").fetchone()
    assert row[0] == 0
    assert row[1] is None


def test_fetch_unprocessed_returns_only_unprocessed_missions_as_dicts():
    conn = init_db(":memory:")
    upsert_missions(conn, [make_mission(external_id=1, title="A"), make_mission(external_id=2, title="B")])
    mission_id = conn.execute("SELECT id FROM missions WHERE title = 'A'").fetchone()[0]
    mark_processed(conn, mission_id)

    unprocessed = fetch_unprocessed(conn)

    assert [m["title"] for m in unprocessed] == ["B"]
    assert unprocessed[0]["skills"] == ["RAG", "Python"]
    assert unprocessed[0]["processed"] is False


def test_mark_processed_sets_flag_and_timestamp():
    conn = init_db(":memory:")
    upsert_missions(conn, [make_mission()])
    mission_id = conn.execute("SELECT id FROM missions").fetchone()[0]

    result = mark_processed(conn, mission_id)

    row = conn.execute("SELECT processed, processed_at FROM missions").fetchone()
    assert result is True
    assert row[0] == 1
    assert row[1] is not None


def test_mark_processed_returns_false_for_unknown_id():
    conn = init_db(":memory:")

    assert mark_processed(conn, 999) is False


def test_get_mission_returns_dict_for_known_id():
    conn = init_db(":memory:")
    upsert_missions(conn, [make_mission(title="Consultant RAG")])
    mission_id = conn.execute("SELECT id FROM missions").fetchone()[0]

    mission = get_mission(conn, mission_id)

    assert mission["title"] == "Consultant RAG"
    assert mission["skills"] == ["RAG", "Python"]


def test_get_mission_returns_none_for_unknown_id():
    conn = init_db(":memory:")

    assert get_mission(conn, 999) is None


def test_migration_adds_new_columns_to_pre_existing_table():
    conn = init_db(":memory:")
    conn.executescript(
        """
        DROP TABLE missions;
        CREATE TABLE missions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            source TEXT NOT NULL,
            external_id TEXT NOT NULL,
            slug TEXT,
            url TEXT,
            title TEXT NOT NULL,
            job_name TEXT,
            skills TEXT NOT NULL DEFAULT '[]',
            min_daily_salary REAL,
            max_daily_salary REAL,
            currency TEXT,
            remote_mode TEXT,
            contracts TEXT NOT NULL DEFAULT '[]',
            duration_value INTEGER,
            duration_period TEXT,
            location_label TEXT,
            company_name TEXT,
            published_at TEXT,
            raw_salary_text TEXT,
            scraped_at TEXT NOT NULL,
            UNIQUE(source, external_id)
        );
        """
    )

    from malt_radar_ingestion.storage import _migrate

    _migrate(conn)

    columns = {row[1] for row in conn.execute("PRAGMA table_info(missions)")}
    assert {"description", "processed", "processed_at"} <= columns
