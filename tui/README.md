# malt-radar-tui

Poste de travail (Phase 4 de `../ROADMAP.md`) : TUI Rust/Ratatui qui lit
l'API de l'`aggregator/` (VPS) via Tailscale. Client pur — ne touche
jamais la DB ni le filesystem du cluster directement.

## Structure (workspace Cargo)

- `core/` — types de domaine (`Fiche`) et calculs purs (`compute_metrics` :
  TJM moyen/médian/p25-p75 + couverture, fréquence des stacks ; filtrage du
  Live Feed ; diff « nouveau depuis le dernier refresh »), sans I/O. Testé
  indépendamment du réseau et du rendu.
- `client/` — client HTTP async (`reqwest`) vers `GET /fiches`, config via
  variables d'env, testé contre un serveur HTTP mocké (`wiremock`).
- `app/` — binaire `malt-radar-tui`, découpé par responsabilité :
  `app.rs` (état + transitions), `events.rs` (dispatch clavier par mode),
  `tasks.rs` (boucles réseau de fond), `browser.rs` (ouverture d'URL),
  `ui/` (rendu). Boucle d'événements Ratatui/Crossterm + tâche de fond qui
  rafraîchit les fiches toutes les 60s (ou sur `r`).

Volontairement minimal pour l'instant : `Market Metrics` et
`Skills Frequency` sont calculés côté client à partir de la liste brute
des fiches (`GET /fiches`), pas d'une agrégation hebdomadaire — la
Phase 3 qui produirait ça n'existe pas encore. Le panneau `Digest` est un
placeholder pour la même raison.

## Setup

```
cp .env.example .env
# éditer .env : AGGREGATOR_URL (IP Tailscale du VPS) + AGGREGATOR_API_TOKEN
```

## Lancer

```
cargo run -p malt-radar-tui
```

Échoue immédiatement avec un message clair (avant de toucher au terminal)
si `AGGREGATOR_URL`/`AGGREGATOR_API_TOKEN` ne sont pas configurées.

Touches principales (`?` affiche le keymap complet dans le TUI) :
- `q` quitte, `Esc` efface le filtre actif sinon quitte, `r` force un
  refresh immédiat, `a` ouvre l'audit des compétences.
- `Tab` alterne le focus entre Top Skills et le Live Feed ; `↑↓`/`j k`/
  `PgUp PgDn`/`Home End` naviguent dans le panneau focalisé.
- `/` filtre le Live Feed (texte, insensible à la casse/accents) ; `Entrée`
  sur une ligne de Top Skills filtre le feed sur ce skill (ou sur la
  longue traîne depuis « Autres ») et y déplace le focus.
- `Entrée` sur une fiche du Live Feed ouvre son détail ; `o` y ouvre l'URL
  dans un navigateur, `↑↓`/`j k` y défilent.

## Tests

```
cargo test
```
