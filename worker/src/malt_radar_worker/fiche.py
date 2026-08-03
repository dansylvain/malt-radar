"""Renders a Mission + Extraction into a Markdown fiche with YAML
frontmatter — the artefact the Phase 3 aggregator will later read to
compute weekly metrics and feed the Claude digest."""

from __future__ import annotations

import yaml

from malt_radar_worker.schema import Extraction


def render_fiche(mission: dict, extraction: Extraction) -> str:
    frontmatter = {
        "source": mission["source"],
        "external_id": mission["external_id"],
        "title": mission["title"],
        "company": mission.get("company_name"),
        "location": mission.get("location_label"),
        "url": mission.get("url"),
        "published_at": mission.get("published_at"),
        "tjm_min": extraction.tjm_min,
        "tjm_max": extraction.tjm_max,
        "devise": extraction.devise,
        "stack": extraction.stack,
        "duree_valeur": extraction.duree_valeur,
        "duree_unite": extraction.duree_unite,
        "niveau": extraction.niveau,
        "categorie": extraction.categorie,
    }
    yaml_block = yaml.safe_dump(frontmatter, allow_unicode=True, sort_keys=False)
    return f"---\n{yaml_block}---\n\n# {mission['title']}\n"
