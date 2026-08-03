from malt_radar_ingestion.sources.free_work import _to_mission

RAW_SAMPLE = {
    "id": 659126,
    "slug": "consultant-pmo-support-gestion-de-projet",
    "title": "Consultant PMO / Support Gestion de Projet",
    "job": {"name": "Assistant·e chef·fe de projet"},
    "skills": [{"name": "Pilotage"}],
    "minDailySalary": 400,
    "maxDailySalary": 550,
    "currency": "EUR",
    "remoteMode": "partial",
    "contracts": ["contractor", "permanent"],
    "durationValue": 1,
    "durationPeriod": "year",
    "location": {"label": "Toulouse, Occitanie"},
    "company": {"name": "Signe +"},
    "publishedAt": "2026-08-03T17:35:24+02:00",
}


def test_to_mission_maps_fields():
    mission = _to_mission(RAW_SAMPLE)

    assert mission.external_id == 659126
    assert mission.url == "https://www.free-work.com/fr/tech-it/jobs/consultant-pmo-support-gestion-de-projet"
    assert mission.skills == ["Pilotage"]
    assert mission.min_daily_salary == 400
    assert mission.max_daily_salary == 550
    assert mission.contracts == ["contractor", "permanent"]
    assert mission.location_label == "Toulouse, Occitanie"
    assert mission.company_name == "Signe +"


def test_to_mission_handles_missing_optional_fields():
    raw = {**RAW_SAMPLE, "location": None, "company": None, "skills": []}

    mission = _to_mission(raw)

    assert mission.location_label is None
    assert mission.company_name is None
    assert mission.skills == []
