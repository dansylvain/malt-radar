"""CLI entrypoint: fetch missions from a source and persist them to SQLite."""

from __future__ import annotations

import argparse

from malt_radar_ingestion.sources import france_travail, free_work
from malt_radar_ingestion.storage import init_db, upsert_missions

SOURCES = {
    "free_work": lambda max_pages: free_work.fetch_missions(max_pages=max_pages),
    "france_travail": lambda max_pages: france_travail.fetch_missions(max_results=max_pages * 50),
}


def main() -> None:
    parser = argparse.ArgumentParser(description="Malt-Radar ingestion")
    parser.add_argument("source", choices=sorted(SOURCES))
    parser.add_argument("--max-pages", type=int, default=1)
    parser.add_argument("--db", default=None, help="Override the default SQLite path")
    args = parser.parse_args()

    conn = init_db(args.db) if args.db else init_db()
    missions = SOURCES[args.source](max_pages=args.max_pages)
    count = upsert_missions(conn, missions)
    print(f"{count} missions upserted from {args.source}")


if __name__ == "__main__":
    main()
