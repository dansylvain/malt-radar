# Sources de données — Malt-Radar

Statut légal et méthode de collecte par source. Ce fichier fait foi pour
tout code d'ingestion — voir la contrainte associée dans `CLAUDE.md`.

## Malt

- **robots.txt** : `/mission` et `/project-*` (pages d'annonces) explicitement
  `Disallow`.
- **CGU** (lues le 2026-08-03, article 10.2 « Interdiction du scraping ») :
  « Il est formellement interdit de procéder à toute forme d'extraction, de
  collecte, de transfert et de réutilisation des informations depuis les
  pages web hébergées par Malt, **de manière automatisée ou non**, à l'aide
  de logiciels dédiés, de robots, ou de **tout autre dispositif** visant à
  réaliser, notamment, un web scraping des données des utilisateurs
  disponibles sur la Marketplace. »
- **Décision : exclu du projet, sans exception.** La clause couvre
  explicitement le manuel autant que l'automatisé — un clipper semi-manuel
  (extraction déclenchée à la main) reste "un dispositif" visant à
  extraire/réutiliser des données de la Marketplace, donc dans le champ de
  l'interdiction. Aucun outil, même à déclenchement manuel, ne doit cibler
  malt.fr / malt.com pour en extraire des données structurées.
- Note : Malt propose des API officielles (art. 10.4), mais réservées à
  l'interopérabilité des données propres d'un compte (factures, missions
  confiées) — pas d'accès au catalogue global des annonces. Pas une
  alternative exploitable pour ce projet.

## LinkedIn

- **robots.txt** : bannière explicite « l'usage de robots ou d'autres
  moyens automatisés... est strictement interdit sans autorisation
  expresse » ; `/jobs*` en `Disallow`.
- **User Agreement (§8.2)** : interdit de "scrape or copy profiles and
  information of others through any means (including crawlers, browser
  plugins and add-ons, and **any other technology or manual work**)".
- **Décision : exclu du projet, sans exception.** Le texte nomme
  explicitement le travail manuel — la semi-automatisation ne réduit pas le
  risque contractuel. Aucun code ne doit cibler linkedin.com.

## Free-Work

- **robots.txt** : pas de restriction sur les pages d'annonces. Bloque
  nommément des outils de scraping connus (Wget, HTTrack, MJ12bot...) —
  signal de vigilance, pas une interdiction générale.
- **CGU** : texte exact non localisé par recherche — à vérifier avant mise
  en production du scraper.
- **Décision** : scraping automatisé Playwright, poli (User-Agent
  identifié + contact, rate-limit ~1x/jour, retry/backoff sur erreurs
  réseau et 429).

## Comet

- **robots.txt** (comet.co) : entièrement ouvert (`Disallow:` vide).
- **CGU** : texte exact non confirmé (plusieurs entités "Comet" existent
  sous ce nom — à vérifier qu'on cible bien la plateforme freelance
  comet.co et pas une autre société).
- **Décision** : scraping automatisé Playwright, mêmes règles de politesse
  que Free-Work.

## France Travail (API officielle)

- **Statut** : API publique officielle (api.francetravail.io), gratuite,
  RGPD-compliant. Aucune ambiguïté légale.
- **Décision** : client API, clé à obtenir via inscription développeur.
- **Volume/fréquence** : selon quotas documentés par France Travail.
