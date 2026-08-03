# malt-radar-aggregator

Service VPS (Phase 2 de `../ROADMAP.md`, amorcé maintenant) : API HTTP
consommée par le worker Pi5 pour le flux pull/push d'entités extraites. La
synthèse hebdomadaire (agrégation + digest Claude, Phase 3) viendra se
greffer sur ce même service plus tard.

Réutilise `storage.py`/`Mission` d'`../ingestion` (dépendance locale
éditable) plutôt que de dupliquer le schéma SQLite — les deux services
lisent/écrivent la même `malt_radar.db`.

## Endpoints

- `GET /missions/unprocessed?limit=50` — missions dont `processed = 0`,
  pour que le worker sache quoi traiter.
- `POST /fiches` — `{"mission_id": int, "markdown": str}` : écrit la fiche
  sur disque (`data/fiches/<source>_<external_id>.md`) et marque la
  mission comme traitée.

Les deux exigent `Authorization: Bearer <AGGREGATOR_API_TOKEN>`. Défense
en profondeur uniquement : cette API n'est de toute façon jamais exposée
que sur le réseau Tailscale (voir `server.py` et `../CLAUDE.md`).

## Setup

```
python3 -m venv .venv
source .venv/bin/activate
pip install -e ../ingestion -e ".[dev]"
```

## Lancer le serveur

```
export AGGREGATOR_API_TOKEN=...       # secret partagé avec le worker
export AGGREGATOR_HOST=$(tailscale ip -4)
python -m malt_radar_aggregator.server
```

`AGGREGATOR_HOST` est obligatoire — le serveur refuse de démarrer sans,
pour éviter un bind accidentel sur `0.0.0.0` (voir `../CLAUDE.md` : pas de
ports exposés publiquement).

## Tests

```
.venv/bin/pytest -q
```
