from malt_radar_worker import run as run_module
from malt_radar_worker.extract import ExtractionFailed
from malt_radar_worker.schema import Extraction

MISSIONS = [
    {"id": 1, "title": "Mission A"},
    {"id": 2, "title": "Mission B"},
]


def test_run_pushes_a_fiche_per_successfully_extracted_mission(monkeypatch):
    pushed = []
    monkeypatch.setattr(run_module.aggregator_client, "fetch_unprocessed", lambda limit: MISSIONS)
    monkeypatch.setattr(run_module, "extract", lambda mission: Extraction(categorie="RAG"))
    monkeypatch.setattr(run_module, "render_fiche", lambda mission, extraction: f"fiche-{mission['id']}")
    monkeypatch.setattr(
        run_module.aggregator_client, "push_fiche", lambda mission_id, markdown: pushed.append((mission_id, markdown))
    )

    count = run_module.run(limit=5)

    assert count == 2
    assert pushed == [(1, "fiche-1"), (2, "fiche-2")]


def test_run_skips_missions_where_extraction_fails(monkeypatch):
    pushed = []

    def fake_extract(mission):
        if mission["id"] == 1:
            raise ExtractionFailed("boom")
        return Extraction(categorie="Autre")

    monkeypatch.setattr(run_module.aggregator_client, "fetch_unprocessed", lambda limit: MISSIONS)
    monkeypatch.setattr(run_module, "extract", fake_extract)
    monkeypatch.setattr(run_module, "render_fiche", lambda mission, extraction: f"fiche-{mission['id']}")
    monkeypatch.setattr(
        run_module.aggregator_client, "push_fiche", lambda mission_id, markdown: pushed.append((mission_id, markdown))
    )

    count = run_module.run(limit=5)

    assert count == 1
    assert pushed == [(2, "fiche-2")]
