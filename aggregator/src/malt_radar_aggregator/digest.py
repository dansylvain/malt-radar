"""Daily strategic digest: python -m malt_radar_aggregator.digest

Reads the fiche corpus, computes aggregate metrics for a rolling window
and the comparable prior window, and asks Claude to synthesize a French
Markdown digest from those metrics — never from raw fiche content. This
is the enforcement point for the France Travail licence constraint in
SOURCES.md (no third-party exposure of individual offer data): only
aggregated counts/percentages ever reach the model.

Triggered by `aggregator/deploy/malt-radar-digest.timer` (systemd), same
oneshot pattern as `ingestion/deploy/` and `worker/deploy/`.
"""

from __future__ import annotations

import json
import os
import sqlite3
from datetime import datetime, timedelta, timezone

import anthropic

from malt_radar_aggregator.fiches import FICHES_DIR, list_fiches
from malt_radar_aggregator.metrics import compute_metrics, compute_trend, filter_window
from malt_radar_aggregator.storage import init_digests_table, save_digest
from malt_radar_ingestion.storage import DEFAULT_DB_PATH, init_db

DEFAULT_MODEL = "claude-haiku-4-5"
DEFAULT_WINDOW_DAYS = 7

# Soft cap on the short summary the model returns, enforced client-side as a
# safety net (the TUI's Digest panel is only ~6 lines tall — see
# `DIGEST_PANEL_HEIGHT` in tui/app/src/ui/dashboard.rs) in case the model
# doesn't respect the length guidance in `DIGEST_TOOL`'s schema.
SUMMARY_SHORT_MAX_CHARS = 480

SYSTEM_PROMPT = """Tu es un analyste marché spécialisé dans le freelance AI \
Engineering en France. Tu rédiges un digest stratégique quotidien à partir \
de métriques agrégées sur les missions freelance disponibles (TJM, \
compétences demandées, catégories de mission).

Règles strictes :
- Tu ne reçois jamais le détail d'une mission individuelle (titre, \
entreprise, url) — uniquement des métriques déjà agrégées (comptes, \
moyennes, médianes, pourcentages). N'invente aucun détail d'annonce \
individuelle : tu ne les connais pas et ne dois pas prétendre les connaître.
- Le pipeline ne couvre que deux sources (Free-Work et France Travail) et \
le volume quotidien peut être faible. Calibre systématiquement ta \
confiance à la taille d'échantillon indiquée (`sample`, `fiche_count`) : \
sur un petit échantillon, dis-le explicitement plutôt que de généraliser.
- Si une métrique vaut `null` (ex: `delta` de tendance TJM), n'invente \
jamais de valeur de remplacement — indique simplement que la donnée est \
insuffisante pour se prononcer.
- Réponds uniquement en français, en appelant systématiquement l'outil \
`emit_digest` — jamais de texte libre en dehors de cet appel."""

# Structured output via a forced tool call rather than a text delimiter: the
# TUI needs two independently-sized texts (a compact panel summary and the
# full narrative), and parsing a Markdown marker out of free text is fragile
# if the model ever drifts from the exact format. Field-level descriptions
# carry the formatting contract instead of the system prompt.
DIGEST_TOOL = {
    "name": "emit_digest",
    "description": (
        "Enregistre le digest stratégique quotidien : un résumé court pour un panneau "
        "compact, et le digest complet en Markdown."
    ),
    "input_schema": {
        "type": "object",
        "properties": {
            "summary_short": {
                "type": "string",
                "description": (
                    "Résumé ultra-condensé en texte brut (pas de Markdown, pas de titres), "
                    "3 à 5 phrases maximum, pensé pour tenir dans un panneau compact d'environ "
                    "6 lignes affichées (soit environ 450 caractères). Va à l'essentiel : TJM "
                    "du moment, tendance, compétence dominante, un point de vigilance si "
                    "pertinent."
                ),
            },
            "content_full": {
                "type": "string",
                "description": (
                    "Digest complet en français, en Markdown, avec exactement ces cinq "
                    "sections dans cet ordre : ## Résumé exécutif, ## Tendances marché, "
                    "## Compétences en tension, ## Recommandations actionnables, "
                    "## Points de vigilance."
                ),
            },
        },
        "required": ["summary_short", "content_full"],
    },
}


def _truncate_summary(text: str, max_chars: int = SUMMARY_SHORT_MAX_CHARS) -> str:
    """Safety net for `summary_short`: the tool schema asks for ~450 chars,
    but nothing guarantees the model respects it. Cuts at a word boundary
    rather than mid-word."""
    text = text.strip()
    if len(text) <= max_chars:
        return text
    return text[:max_chars].rsplit(" ", 1)[0] + "…"


def _user_prompt(current_metrics: dict, previous_metrics: dict, trend: dict) -> str:
    return (
        "Voici les seules données disponibles pour ce digest. Synthétise "
        "uniquement à partir de ces chiffres — n'invente aucun détail au-delà.\n\n"
        "Métriques de la fenêtre courante :\n"
        f"{json.dumps(current_metrics, indent=2, ensure_ascii=False)}\n\n"
        "Métriques de la fenêtre précédente (pour comparaison) :\n"
        f"{json.dumps(previous_metrics, indent=2, ensure_ascii=False)}\n\n"
        "Tendance du TJM médian entre les deux fenêtres :\n"
        f"{json.dumps(trend, indent=2, ensure_ascii=False)}"
    )


def _get_conn() -> sqlite3.Connection:
    db_path = os.environ.get("AGGREGATOR_DB_PATH", str(DEFAULT_DB_PATH))
    conn = init_db(db_path)
    init_digests_table(conn)
    return conn


def run(window_days: int | None = None, client: anthropic.Anthropic | None = None) -> dict | None:
    api_key = os.environ.get("ANTHROPIC_API_KEY")
    if client is None and not api_key:
        raise SystemExit("ANTHROPIC_API_KEY is not set")

    model = os.environ.get("DIGEST_MODEL", DEFAULT_MODEL)
    if window_days is None:
        window_days = int(os.environ.get("DIGEST_WINDOW_DAYS", DEFAULT_WINDOW_DAYS))

    now = datetime.now(timezone.utc)
    current_start = now - timedelta(days=window_days)
    previous_start = now - timedelta(days=window_days * 2)

    fiches = list_fiches(FICHES_DIR)
    current_fiches = filter_window(fiches, current_start, now)
    previous_fiches = filter_window(fiches, previous_start, current_start)

    current_metrics = compute_metrics(current_fiches, current_start, now)
    previous_metrics = compute_metrics(previous_fiches, previous_start, current_start)
    trend = compute_trend(current_metrics, previous_metrics)

    if current_metrics["fiche_count"] == 0:
        print("Digest skipped: no fiches in the current window.")
        return None

    if client is None:
        client = anthropic.Anthropic(api_key=api_key)

    response = client.messages.create(
        model=model,
        max_tokens=2048,
        system=SYSTEM_PROMPT,
        tools=[DIGEST_TOOL],
        tool_choice={"type": "tool", "name": "emit_digest"},
        messages=[{"role": "user", "content": _user_prompt(current_metrics, previous_metrics, trend)}],
    )

    if response.stop_reason == "refusal":
        raise SystemExit("Digest generation refused by the model (stop_reason=refusal)")

    tool_call = next(
        (block for block in response.content if block.type == "tool_use" and block.name == "emit_digest"),
        None,
    )
    if tool_call is None:
        raise SystemExit("Digest generation did not return the expected emit_digest tool call")

    summary_short = _truncate_summary(tool_call.input.get("summary_short", ""))
    content = tool_call.input.get("content_full", "")

    conn = _get_conn()
    digest = save_digest(
        conn,
        window_start=current_start.isoformat(),
        window_end=now.isoformat(),
        window_days=window_days,
        model=model,
        fiche_count=current_metrics["fiche_count"],
        metrics=current_metrics,
        trend=trend,
        summary_short=summary_short,
        content=content,
    )
    print(f"Digest saved: id={digest['id']} fiches={digest['fiche_count']} window_days={window_days}")
    return digest


if __name__ == "__main__":
    run()
