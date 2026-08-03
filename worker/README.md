# malt-radar-worker

Service Pi5 (Phase 2 de `../ROADMAP.md`) : extraction d'entités
structurées (TJM, stack, durée, niveau, catégorie RAG/Agent/Vision/
Fine-tuning) via Ollama/Qwen 2.5 3B, génération de fiches Markdown avec
frontmatter YAML.

Tourne sur son propre timer systemd, jamais en écoute (voir
`../ROADMAP.md` Phase 2 : mode pull, pas de serveur permanent sur le Pi5).
À chaque passage :

1. Pull `GET /missions/unprocessed` sur l'aggregator (VPS, via Tailscale).
2. Pour chaque mission : extraction via Ollama local (`extract.py`),
   validation stricte + retry sur JSON malformé/invalide (`schema.py`).
3. Génère une fiche Markdown (`fiche.py`).
4. Push `POST /fiches` vers l'aggregator, qui marque la mission traitée.

Les missions dont l'extraction échoue après retries sont laissées
`processed = 0` et retentées au passage suivant.

## Setup

### Ollama + qwen2.5:3b

```
curl -fsSL https://ollama.com/install.sh | sh
ollama pull qwen2.5:3b
```

### Environnement Python

```
python3 -m venv .venv
source .venv/bin/activate
pip install -e ".[dev]"
```

### Configuration

```
cp .env.example .env   # à créer, voir variables ci-dessous
```

Variables (dans `.env`, jamais commité) :

- `AGGREGATOR_URL` — URL Tailscale de l'aggregator, ex.
  `http://100.x.y.z:8321`.
- `AGGREGATOR_API_TOKEN` — même secret que `AGGREGATOR_API_TOKEN` côté
  aggregator.
- `OLLAMA_HOST` — défaut `http://localhost:11434` (Ollama tourne
  localement sur le Pi5, pas besoin de Tailscale pour cet appel).
- `OLLAMA_MODEL` — défaut `qwen2.5:3b`.

## Lancer une passe

```
python -m malt_radar_worker.run --limit 20
```

## Déploiement (Pi5) : timer systemd

Unités dans `deploy/` : `malt-radar-worker.service` (une passe
`--limit 20`) + `malt-radar-worker.timer` (quotidien à 7h UTC + jitter de
30 min, décalé après le timer d'ingestion du VPS pour être sûr de trouver
les missions du jour déjà en base).

```
# Remplacer les CHANGE_ME (utilisateur + chemin absolu du repo) dans les
# deux fichiers avant de les copier :
sudo cp deploy/malt-radar-worker.service /etc/systemd/system/
sudo cp deploy/malt-radar-worker.timer /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now malt-radar-worker.timer
```

Vérifier :

```
systemctl list-timers | grep malt-radar
journalctl -u malt-radar-worker.service -n 50
```

## Tests

```
.venv/bin/pytest -q
```

Tous les appels réseau (Ollama, aggregator) sont mockés via
`httpx.MockTransport` — aucun test ne touche le vrai Ollama ni le vrai
aggregator.
