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
- `GET /fiches?limit=100` — liste les fiches déjà générées (les plus
  récentes d'abord), sous forme de frontmatter YAML parsé + `filename`.
  Lu par le TUI (Phase 4, poste de travail). Volontairement minimal : pas
  de métriques agrégées ni de digest — ça reste le rôle de la Phase 3, pas
  encore construite. Un fichier dont le frontmatter est absent/invalide
  est silencieusement ignoré plutôt que de faire échouer tout l'appel.
- `DELETE /fiches/{filename}` — supprime une fiche (extraction erronée,
  TJM hors bornes, etc.). Déclenché par la touche `d` dans le détail de
  fiche du TUI. `filename` reste un simple nom de fichier dans
  `data/fiches/` — jamais résolu tel quel, pour bloquer toute tentative de
  path traversal. Idempotent comme `DELETE /skill-aliases` : supprimer une
  fiche déjà absente répond `{"deleted": false}`, pas une 404 (le TUI n'a
  pas de confirmation avant suppression, donc un double `d` ne doit pas
  afficher une erreur).

Ces quatre exigent `Authorization: Bearer <AGGREGATOR_API_TOKEN>`. Défense
en profondeur uniquement : cette API n'est de toute façon jamais exposée
que sur le réseau Tailscale (voir `server.py` et `../CLAUDE.md`) — le TUI
devra donc, comme le worker, connaître ce secret partagé pour lire les
fiches.

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
