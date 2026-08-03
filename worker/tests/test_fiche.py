import yaml

from malt_radar_worker.fiche import render_fiche
from malt_radar_worker.schema import Extraction

MISSION = {
    "source": "free_work",
    "external_id": 42,
    "title": "Ingénieur RAG",
    "company_name": "Acme",
    "location_label": "Paris",
    "url": "https://www.free-work.com/fr/tech-it/jobs/ingenieur-rag",
    "published_at": "2026-08-03T10:00:00+02:00",
}


def test_render_fiche_has_yaml_frontmatter_and_title():
    extraction = Extraction(tjm_min=500, tjm_max=700, stack=["Python", "RAG"], categorie="RAG")

    markdown = render_fiche(MISSION, extraction)

    assert markdown.startswith("---\n")
    assert markdown.count("---") == 2
    assert "# Ingénieur RAG" in markdown

    frontmatter_block = markdown.split("---")[1]
    frontmatter = yaml.safe_load(frontmatter_block)
    assert frontmatter["source"] == "free_work"
    assert frontmatter["external_id"] == 42
    assert frontmatter["tjm_min"] == 500
    assert frontmatter["stack"] == ["Python", "RAG"]
    assert frontmatter["categorie"] == "RAG"
