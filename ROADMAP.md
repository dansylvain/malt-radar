# Malt-Radar — Roadmap

## Décision de sourcing (2026-08-03, mise à jour finale)

Audit `robots.txt` + lecture des CGU exactes : **LinkedIn et Malt exclus du
projet, sans exception, sous aucune forme.** Les deux interdisent
explicitement le scraping "manuel ou automatisé" (LinkedIn §8.2 : "any
other technology or manual work" ; Malt art. 10.2 : "de manière
automatisée ou non... ou tout autre dispositif"). Le clipper semi-manuel
envisagé pour Malt a été écarté après lecture du texte exact — voir
`SOURCES.md`.

**Comet également écarté** (2026-08-03) : ce n'est pas un job board public,
c'est un site vitrine pour un cabinet de placement — aucune fiche de
mission publique à scraper, tout est derrière l'app authentifiée. Sources
retenues :

- **Free-Work** (job board freelance tech) — implémenté, API JSON publique
  directe (voir `SOURCES.md`).
- **France Travail API** (api.francetravail.io) — API officielle, publique,
  gratuite. Pas encore implémentée.

Le projet garde le nom "Malt-Radar" (branding/portfolio) mais ne source
plus aucune donnée depuis Malt lui-même. Avec Malt, LinkedIn et Comet tous
écartés, le pipeline repose sur seulement 2 sources pour l'instant — à
garder en tête si le volume de données s'avère trop faible en Phase 3
(synthèse des tendances).

Politique de scraping automatisé (Free-Work/Comet, voir `CLAUDE.md`) :
respecter `robots.txt` et `Crawl-delay`, User-Agent identifiable avec
contact, pas de contournement d'authentification, fréquence raisonnable
(1x/jour).

## Phase 0 — Cadrage sourcing & légal
- Lire les CGU exactes de Malt (confirmer le mot "automatisée"), Free-Work
  et Comet — bloqué par 403 côté agent, à faire par Dan en session
  connectée.
- Créer `SOURCES.md` : par source, statut légal, méthode de collecte,
  contraintes de volume.
- Obtenir une clé API France Travail.
- **Sortie** : sources validées, aucun doute légal résiduel.

## Phase 1 — Ingestion (VPS Hetzner) — code complet, déploiement restant
- ✅ Scraper Free-Work (API JSON publique directe, pas de Playwright).
- ✅ Client France Travail API (OAuth2, validé en conditions réelles).
- ✅ Normalisation → `Mission` → SQLite (upsert idempotent, multi-source).
- ✅ Unités systemd prêtes (`ingestion/deploy/`) : service + timer
  quotidien, voir `ingestion/README.md` pour le déploiement.
- ⬜ **Reste à faire par Dan** : cloner le repo sur la VPS, créer le venv,
  remplir `ingestion/.env` avec les vrais identifiants, adapter les
  `CHANGE_ME` des unités systemd et les activer (`systemctl enable --now`)
  — je n'ai pas d'accès SSH à la VPS pour le faire moi-même.
- **Sortie** : collecte stable pendant 1 semaine sans blocage IP ni erreur
  non gérée — à confirmer une fois le timer actif sur la VPS.
- ✅ **Amendement (2026-08-03, pour Phase 2)** : `Mission` gagne un champ
  `description` (texte complet, mappé depuis les deux sources) — sans ça
  la catégorisation LLM de Phase 2 n'a que titre + intitulé + compétences
  à se mettre sous la dent. Migration SQLite idempotente (`storage.py`
  ajoute la colonne via `ALTER TABLE` si absente) : pas besoin de
  redéployer une DB neuve sur la VPS.

## Phase 2 — Traitement (Raspberry Pi 5, Ollama/Qwen 2.5 3B) — ✅ code complet, déploiement restant
Décisions d'architecture (2026-08-03) :
- **Flux missions VPS→Pi5 : pull, pas push.** Le Pi5 n'est pas garanti
  disponible en permanence (coupures électriques/réseau domestiques) —
  contrairement au VPS. Un endpoint HTTP toujours en écoute sur le Pi5
  aurait réintroduit cette dépendance. Le worker va donc chercher son
  travail sur son propre timer, plutôt que de le recevoir en push.
- **`aggregator/` amorcé dès maintenant** (normalement Phase 3) pour
  héberger le service HTTP côté VPS dont le worker a besoin — le VPS
  reste le hub toujours disponible du cluster.
- Suivi d'état : colonne `processed`/`processed_at` sur `missions`
  (`ingestion/storage.py`), jamais réinitialisée par un re-scrape.

- ✅ `aggregator/` : `GET /missions/unprocessed` + `POST /fiches`
  (auth par token partagé, écriture des fiches sur disque + flag
  processed). Réutilise `ingestion/storage.py` en dépendance locale.
- ✅ `worker/` : client Ollama (`qwen2.5:3b`), prompt + JSON Schema strict
  (TJM, stack, durée, niveau, catégorie RAG/Agent/Vision/Fine-tuning),
  validation Pydantic + retry sur échec de parsing/schema.
- ✅ Génération de fiches Markdown + frontmatter YAML (`worker/fiche.py`).
- ✅ Client HTTP pull/push (`worker/client.py`) + CLI (`worker/run.py`).
- ✅ Unité systemd timer (`worker/deploy/`), décalée après le timer
  d'ingestion (7h UTC vs 6h) pour trouver les missions du jour déjà en
  base.
- ⬜ **Reste à faire par Dan** : sur le Pi5, installer Ollama + `ollama
  pull qwen2.5:3b`, créer le venv (`worker/`), remplir `worker/.env`
  (`AGGREGATOR_URL`, `AGGREGATOR_API_TOKEN`) ; sur le VPS, créer le venv
  `aggregator/`, choisir/poser `AGGREGATOR_API_TOKEN` (même secret des
  deux côtés), lancer le serveur (`aggregator/README.md`) ; adapter les
  `CHANGE_ME` des unités systemd et les activer.
- **Sortie** : taux d'extraction correcte > 90 % sur un échantillon annoté
  manuellement — à mesurer une fois le pipeline tournant en conditions
  réelles sur le Pi5.

## Phase 3 — Synthèse (VPS + API Claude)
- ✅ `aggregator/` existe déjà (amorcé en Phase 2) avec l'API HTTP
  pull/push ; cette phase l'étend, ne repart pas de zéro.
- Agrégation hebdomadaire des fiches Markdown.
- Calcul des métriques (TJM moyen par stack, fréquence des compétences,
  évolution semaine/semaine).
- Appel Claude API pour le digest stratégique — usage hebdo, jamais par
  fiche individuelle (maîtrise du coût).
- Exposition des résultats via une API locale consommée par le TUI.

## Phase 4 — Dashboard TUI (Rust/Ratatui, poste de travail/WSL2)
- Workspace Cargo (`core`, `tui`, `client`).
- Layout Ratatui : Market Metrics, Live Feed, Skills Frequency, Digest,
  footer de commandes.
- Client HTTP async (reqwest/tokio) vers l'API VPS/Pi.
- Navigation clavier : filtre stack, refresh, export markdown, system
  health.
- ⚠️ **Rappel licence France Travail** (voir `SOURCES.md`) : le panneau
  "Live Feed" ne doit pas exposer publiquement des annonces individuelles
  issues de France Travail à des tiers — usage personnel uniquement, pas
  d'API publique par-dessus.
- **Sortie** : TUI fonctionnel en lecture contre les données réelles du
  cluster.

## Phase 5 — Réseau & déploiement (Tailscale, systemd, cluster complet)
- ACLs Tailscale entre VPS / Pi5 / poste de travail.
- Services systemd (timer d'ingestion, endpoint Pi5, agrégateur VPS).
- Heartbeat / system health remonté dans le TUI.
- Sauvegarde SQLite + fiches Markdown.

## Phase 6 — Polish portfolio
- README avec démo (asciinema/gif du TUI).
- Schéma d'architecture + choix techniques documentés.
- Tests, CI basique, nettoyage.
- ⚠️ **Rappel licence France Travail** (voir `SOURCES.md`) : pour toute
  démo publique (gif, vidéo, captures), montrer des métriques agrégées
  (TJM moyen, % de tendance), pas d'annonces individuelles verbatim avec
  nom d'entreprise/TJM réel. Vérifier aussi qu'aucun export de données
  brutes ne finit dans le repo public.
