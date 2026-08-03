# Malt-Radar — Roadmap

## Décision de sourcing (2026-08-03, mise à jour finale)

Audit `robots.txt` + lecture des CGU exactes : **LinkedIn et Malt exclus du
projet, sans exception, sous aucune forme.** Les deux interdisent
explicitement le scraping "manuel ou automatisé" (LinkedIn §8.2 : "any
other technology or manual work" ; Malt art. 10.2 : "de manière
automatisée ou non... ou tout autre dispositif"). Le clipper semi-manuel
envisagé pour Malt a été écarté après lecture du texte exact — voir
`SOURCES.md`. Sources retenues :

- **Free-Work** (job board freelance tech) — scraping automatisé poli
  (User-Agent identifié, rate-limit).
- **Comet** — scraping automatisé poli, `robots.txt` ouvert.
- **France Travail API** (api.francetravail.io) — API officielle, publique,
  gratuite.

Le projet garde le nom "Malt-Radar" (branding/portfolio) mais ne source
plus aucune donnée depuis Malt lui-même.

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

## Phase 1 — Ingestion (VPS Hetzner, Python/Playwright)
- Setup projet Python (uv ou poetry), Playwright headless.
- Scraper Free-Work + Comet (rate-limited, retry/backoff sur erreurs réseau
  et 429).
- Client France Travail API.
- Normalisation JSON brut → SQLite.
- Timer systemd quotidien.
- **Sortie** : collecte stable pendant 1 semaine sans blocage IP ni erreur
  non gérée.

## Phase 2 — Traitement (Raspberry Pi 5, Ollama/Qwen 2.5 3B)
- Ollama + `qwen2.5:3b` sur le Pi.
- Endpoint HTTP minimal, réception du JSON brut via Tailscale.
- Prompt + JSON Schema pour extraction (TJM, stack, durée, niveau,
  catégorie RAG/Agent/Vision/Fine-tuning).
- Validation stricte du schema, retry si échec de parsing.
- Génération de fiches Markdown + frontmatter YAML.
- **Sortie** : taux d'extraction correcte > 90 % sur un échantillon annoté
  manuellement.

## Phase 3 — Synthèse (VPS + API Claude)
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
