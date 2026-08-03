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

- Ingestion : Python, Playwright, SQLite.
- Traitement : Ollama (Qwen 2.5 3B), JSON Schema strict avec validation +
  retry.
- Synthèse : API Claude, appelée uniquement pour la synthèse hebdomadaire
  (pas par annonce individuelle — maîtrise du coût).
- TUI : Rust, Ratatui, Tokio, Reqwest, Crossterm.
- Réseau : Tailscale entre VPS / Pi5 / poste de travail, pas de ports
  exposés publiquement.

## Conventions

Le repo est un mono-repo multi-langage (Python, Rust) organisé par service
(ingestion, worker Pi5, TUI). Structure de dossiers à définir au démarrage
de la Phase 1 — mettre à jour cette section une fois posée.
