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
- **Comet : exclu, pas de CGU en cause.** Site vitrine (Webflow) pour un
  cabinet de placement — aucun catalogue public de missions. Les données
  ne sont visibles que dans l'app authentifiée (`app.comet.co`), donc y
  toucher violerait la règle "pas de contournement d'authentification".
- **Free-Work : scraping automatisé autorisé**, avec politesse :
  User-Agent identifiable avec contact, respect du `Crawl-delay` s'il
  existe (sinon ~1x/jour), pas de contournement d'authentification/CAPTCHA,
  retry/backoff sur erreurs réseau et 429 (jamais de martèlement).
- **France Travail : API officielle**, mais soumise à la "licence offre
  d'emploi" : usage interne/personnel OK, **jamais de mise à disposition
  de tiers des données brutes** (pas de commit de la DB dans un repo
  public, pas d'API publique exposant les fiches individuelles). En démo
  publique (portfolio), préférer les métriques agrégées aux annonces
  individuelles verbatim. Détail dans `SOURCES.md`.

Toute nouvelle source doit être validée (robots.txt + CGU) et ajoutée à
`SOURCES.md` avant intégration au code.

## Architecture (pipeline distribué)

1. **Ingestion** (VPS Hetzner, Python/Playwright) — scraping des sources
   autorisées + API France Travail → JSON brut → SQLite.
2. **Traitement** (Raspberry Pi 5, Ollama/Qwen 2.5 3B, via Tailscale) —
   extraction d'entités structurées (TJM, stack, durée, catégorie) → fiches
   Markdown avec frontmatter YAML.
3. **Synthèse** (VPS + API Claude) — agrégation quotidienne (fenêtre
   glissante de 7 jours), métriques, digest stratégique généré par Claude
   (un appel/jour, jamais par fiche — avec `claude-haiku-4-5` le coût
   d'un appel quotidien est négligeable, ~0,10-0,15 $/mois, donc c'est le
   volume de données par fenêtre qui dicte la cadence, plus le coût).
4. **Restitution** (Rust/Ratatui, poste de travail) — TUI qui interroge
   l'API du cluster.

## Stack par composant

- Ingestion : Python, `httpx` (Free-Work : API JSON publique directe, pas
  de rendu navigateur nécessaire — à revérifier par source, Comet n'est pas
  encore implémenté), SQLite.
- Traitement : Ollama (Qwen 2.5 3B), JSON Schema strict avec validation +
  retry.
- Synthèse : API Claude (`claude-haiku-4-5` par défaut, configurable via
  `DIGEST_MODEL`), appelée une fois par jour sur une fenêtre glissante de
  7 jours — jamais par annonce individuelle.
- TUI : Rust, Ratatui, Tokio, Reqwest, Crossterm.
- Réseau : Tailscale entre VPS / Pi5 / poste de travail, pas de ports
  exposés publiquement.

## Conventions

Mono-repo multi-langage (Python, Rust), organisé par service. Un service =
un dossier à la racine, déployé/exécuté sur une seule machine du cluster.

- `ingestion/` — Python (src-layout, `pyproject.toml` standard, pas
  d'uv/poetry sur ce poste). Scraper Free-Work/Comet + client France
  Travail. Tourne sur la VPS Hetzner. **Existe déjà** (Phase 1).
- `worker/` — Pi5, Ollama/Qwen 2.5 3B, extraction d'entités + génération de
  fiches Markdown. Pull les missions non traitées / push les fiches vers
  `aggregator/` (jamais de serveur en écoute sur le Pi5 — voir
  `ROADMAP.md` Phase 2). **Existe déjà** (Phase 2, code complet —
  déploiement Pi5 restant).
- `aggregator/` — VPS, API HTTP pull/push consommée par `worker/` (amorcé
  en Phase 2) ; la synthèse quotidienne + appel API Claude (Phase 3, code
  complet côté `digest.py`) s'y greffent. Dépend de `ingestion/` (réutilise
  `storage.py` en éditable local plutôt que de dupliquer le schéma
  SQLite). **Existe déjà** (Phase 2 + Phase 3 code complet — timer
  systemd et `.env` restants côté Dan).
- `tui/` — Rust workspace (Ratatui/Tokio), poste de travail/WSL2. Client
  HTTP pur vers `aggregator/` (jamais d'accès direct DB/filesystem
  cluster). **Existe déjà** (Phase 4, scaffold complet — connexion au
  vrai VPS via `.env` restante côté Dan).

Chaque service a son propre gestionnaire de dépendances/build (pas de
tooling partagé forcé entre Python et Rust) — exception : `aggregator/`
installe `ingestion/` en dépendance locale éditable (`pip install -e
../ingestion`) pour réutiliser son `storage.py`/`Mission` plutôt que de
dupliquer le schéma SQLite ; les deux tournent sur la même machine (VPS).
`.gitignore` à la racine couvre les deux écosystèmes.

## Workflow git

**Dan gère git lui-même** (add/commit/push). Ne pas exécuter de commandes
git qui modifient l'état du dépôt (commit, push, etc.) sauf demande
explicite dans le message en cours. Un simple rappel ("tu voudras sans
doute committer ça") est bienvenu quand il y a des changements en attente,
mais l'action reste la sienne.
