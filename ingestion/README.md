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

## Déploiement (VPS Hetzner) : timer systemd quotidien

Unités dans `deploy/` : `malt-radar-ingest.service` (lance les deux
sources l'une après l'autre) + `malt-radar-ingest.timer` (déclenchement
quotidien à 6h + jitter de 30 min, rattrapage si la VPS était éteinte).

Sur la VPS, une fois le repo cloné et le venv créé (voir Setup ci-dessus)
et `ingestion/.env` rempli avec les vrais identifiants :

```
# Remplacer les CHANGE_ME (utilisateur + chemin absolu du repo) dans les
# deux fichiers avant de les copier :
sudo cp ingestion/deploy/malt-radar-ingest.service /etc/systemd/system/
sudo cp ingestion/deploy/malt-radar-ingest.timer /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now malt-radar-ingest.timer
```

Vérifier :

```
systemctl list-timers | grep malt-radar
journalctl -u malt-radar-ingest.service -n 50
```
