# malt-radar-tui

Poste de travail (Phase 4 de `../ROADMAP.md`) : TUI Rust/Ratatui qui lit
l'API de l'`aggregator/` (VPS) via Tailscale. Client pur — ne touche
jamais la DB ni le filesystem du cluster directement.

## Structure (workspace Cargo)

- `core/` — types de domaine (`Fiche`) et calculs purs
  (`compute_metrics` : TJM moyen, fréquence des stacks), sans I/O. Testé
  indépendamment du réseau et du rendu.
- `client/` — client HTTP async (`reqwest`) vers `GET /fiches`, config via
  variables d'env, testé contre un serveur HTTP mocké (`wiremock`).
- `app/` — binaire `malt-radar-tui` : boucle d'événements Ratatui/Crossterm
  + tâche de fond qui rafraîchit les fiches toutes les 60s (ou sur `r`).

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

Touches : `q`/`Esc` quitte, `r` force un refresh immédiat.

## Tests

```
cargo test
```
