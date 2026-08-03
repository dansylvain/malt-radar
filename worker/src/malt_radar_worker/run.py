"""CLI entrypoint: pull unprocessed missions from the aggregator, run
entity extraction via Ollama, render fiches, push them back."""

from __future__ import annotations

import argparse

from malt_radar_worker import client as aggregator_client
from malt_radar_worker.extract import ExtractionFailed, extract
from malt_radar_worker.fiche import render_fiche


def run(limit: int = 20) -> int:
    missions = aggregator_client.fetch_unprocessed(limit=limit)
    processed = 0
    for mission in missions:
        try:
            extraction = extract(mission)
        except ExtractionFailed as exc:
            print(f"[skip] mission {mission['id']} ({mission['title']}): {exc}")
            continue
        markdown = render_fiche(mission, extraction)
        aggregator_client.push_fiche(mission["id"], markdown)
        processed += 1
    return processed


def main() -> None:
    parser = argparse.ArgumentParser(description="Malt-Radar worker: entity extraction pass")
    parser.add_argument("--limit", type=int, default=20)
    args = parser.parse_args()

    count = run(limit=args.limit)
    print(f"{count} mission(s) traitée(s)")


if __name__ == "__main__":
    main()
