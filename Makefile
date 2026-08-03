.DEFAULT_GOAL := help
.PHONY: help setup-ingestion test test-ingestion ingest-free-work ingest-france-travail clean

help:
	@echo "make setup-ingestion        - create ingestion venv, install deps"
	@echo "make test                   - run all test suites"
	@echo "make ingest-free-work       - run a Free-Work ingestion pass (--max-pages 3)"
	@echo "make ingest-france-travail  - run a France Travail pass (needs ingestion/.env)"
	@echo "make clean                  - remove venvs and caches"

setup-ingestion:
	cd ingestion && python3 -m venv .venv && .venv/bin/pip install -e ".[dev]"

test: test-ingestion

test-ingestion:
	cd ingestion && .venv/bin/pytest -q

ingest-free-work:
	cd ingestion && .venv/bin/python -m malt_radar_ingestion.ingest free_work --max-pages 3

ingest-france-travail:
	cd ingestion && set -a && . .env && set +a && .venv/bin/python -m malt_radar_ingestion.ingest france_travail --max-pages 3

clean:
	rm -rf ingestion/.venv ingestion/.pytest_cache
	find ingestion -type d -name __pycache__ -exec rm -rf {} +
