"""CLI entrypoint: fetch missions from a source and persist them to SQLite."""

from __future__ import annotations

import argparse

from malt_radar_ingestion.sources import free_work
from malt_radar_ingestion.storage import init_db, upsert_missions

SOURCES = {
    "free_work": free_work.fetch_missions,
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
