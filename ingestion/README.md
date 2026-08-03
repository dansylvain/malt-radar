# malt-radar-ingestion

Service d'ingestion (VPS Hetzner, Phase 1 de `../ROADMAP.md`) : Free-Work
(API JSON publique) + API officielle France Travail, normalisées en
`Mission` (voir `models.py`) et stockées en SQLite (`storage.py`). Sources
et contraintes de collecte : voir `../SOURCES.md` et `../CLAUDE.md`.

## Setup

```
python3 -m venv .venv
source .venv/bin/activate
pip install -e ".[dev]"
```

## France Travail : identifiants requis

Cette source nécessite un compte développeur sur francetravail.io :

1. Créer un compte sur https://francetravail.io et déclarer une
   "application".
2. Abonner cette application à l'API "Offres d'emploi v2".
3. Récupérer le `client_id`/`client_secret` fournis, et vérifier le scope
   exact indiqué dans le tableau de bord (par défaut le code utilise
   `api_offresdemploiv2 o2dsoffre`, à confirmer).
4. Exporter les variables d'environnement avant de lancer l'ingestion :

```
export FRANCE_TRAVAIL_CLIENT_ID=...
export FRANCE_TRAVAIL_CLIENT_SECRET=...
```

## Lancer une collecte

```
python -m malt_radar_ingestion.ingest free_work --max-pages 3
python -m malt_radar_ingestion.ingest france_travail --max-pages 3
```
