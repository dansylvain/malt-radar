# malt-radar-ingestion

Service d'ingestion (VPS Hetzner, Phase 1 de `../ROADMAP.md`) : scrape
Free-Work et Comet, interroge l'API France Travail, normalise en JSON et
stocke en SQLite. Sources et contraintes de collecte : voir `../SOURCES.md`
et `../CLAUDE.md`.

## Setup

```
python3 -m venv .venv
source .venv/bin/activate
pip install -e ".[dev]"
playwright install chromium
```
