# Malt-Radar

Plateforme distribuée d'intelligence marché pour le freelance AI Engineering
en France : TJM, stacks demandées, tendances. Double objectif : démonstrateur
technique (Rust async, architecture edge/cloud, LLM hybride) et outil de
veille métier réel.

Voir `Malt_Radar_Architecture_Spec.pdf` pour la spec d'origine et
`ROADMAP.md` pour le plan de développement à jour (les deux peuvent diverger
légèrement — `ROADMAP.md` fait foi sur le sourcing des données).

## Contrainte non négociable : sourcing des données

Détail complet et statut de vérification par source dans `SOURCES.md`
(source de vérité — à consulter avant d'ajouter/modifier tout code
d'ingestion).

- **LinkedIn et Malt : exclus du projet, sans exception, sous aucune
  forme.** Leurs conditions respectives interdisent explicitement le
  scraping "manuel ou automatisé" :
  - LinkedIn User Agreement §8.2 : "through any means (including
    crawlers, browser plugins and add-ons, and **any other technology or
    manual work**)".
  - Malt CGU art. 10.2 : "de manière **automatisée ou non**, à l'aide de
    logiciels dédiés, de robots, ou de tout autre dispositif".

  Un clipper semi-manuel (déclenchement humain, une annonce à la fois) a
  été envisagé pour Malt puis écarté après lecture des CGU exactes — la
  clause couvre le manuel aussi bien que l'automatisé. **Aucun code, aucune
  extension navigateur, aucun outil, même à déclenchement manuel, ne doit
  cibler malt.fr, malt.com ou linkedin.com pour en extraire des données.**
- **Free-Work et Comet : scraping automatisé autorisé**, avec politesse :
  User-Agent identifiable avec contact, respect du `Crawl-delay` s'il
  existe (sinon ~1x/jour), pas de contournement d'authentification/CAPTCHA,
  retry/backoff sur erreurs réseau et 429 (jamais de martèlement).
- **France Travail : API officielle**, aucune restriction particulière au-
  delà des quotas documentés.

Toute nouvelle source doit être validée (robots.txt + CGU) et ajoutée à
`SOURCES.md` avant intégration au code.

## Architecture (pipeline distribué)

1. **Ingestion** (VPS Hetzner, Python/Playwright) — scraping des sources
   autorisées + API France Travail → JSON brut → SQLite.
2. **Traitement** (Raspberry Pi 5, Ollama/Qwen 2.5 3B, via Tailscale) —
   extraction d'entités structurées (TJM, stack, durée, catégorie) → fiches
   Markdown avec frontmatter YAML.
3. **Synthèse** (VPS + API Claude) — agrégation hebdomadaire, métriques,
   digest stratégique généré par Claude (usage hebdo, jamais par fiche).
4. **Restitution** (Rust/Ratatui, poste de travail) — TUI qui interroge
   l'API du cluster.

## Stack par composant

- Ingestion : Python, `httpx` (Free-Work : API JSON publique directe, pas
  de rendu navigateur nécessaire — à revérifier par source, Comet n'est pas
  encore implémenté), SQLite.
- Traitement : Ollama (Qwen 2.5 3B), JSON Schema strict avec validation +
  retry.
- Synthèse : API Claude, appelée uniquement pour la synthèse hebdomadaire
  (pas par annonce individuelle — maîtrise du coût).
- TUI : Rust, Ratatui, Tokio, Reqwest, Crossterm.
- Réseau : Tailscale entre VPS / Pi5 / poste de travail, pas de ports
  exposés publiquement.

## Conventions

Mono-repo multi-langage (Python, Rust), organisé par service. Un service =
un dossier à la racine, déployé/exécuté sur une seule machine du cluster.

- `ingestion/` — Python (src-layout, `pyproject.toml` standard, pas
  d'uv/poetry sur ce poste). Scraper Free-Work/Comet + client France
  Travail. Tourne sur la VPS Hetzner. **Existe déjà** (Phase 1).
- `worker/` — Pi5, Ollama/Qwen 2.5 3B, extraction d'entités. Pas encore créé
  (Phase 2).
- `aggregator/` — VPS, synthèse hebdomadaire + appel API Claude. Pas encore
  créé (Phase 3).
- `tui/` — Rust workspace (Ratatui/Tokio), poste de travail/WSL2. Pas
  encore créé (Phase 4).

Chaque service a son propre gestionnaire de dépendances/build (pas de
tooling partagé forcé entre Python et Rust). `.gitignore` à la racine
couvre les deux écosystèmes.

## Workflow git

**Dan gère git lui-même** (add/commit/push). Ne pas exécuter de commandes
git qui modifient l'état du dépôt (commit, push, etc.) sauf demande
explicite dans le message en cours. Un simple rappel ("tu voudras sans
doute committer ça") est bienvenu quand il y a des changements en attente,
mais l'action reste la sienne.
