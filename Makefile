.DEFAULT_GOAL := help
.PHONY: help setup-ingestion setup-aggregator setup-worker \
        test test-ingestion test-aggregator test-worker test-tui \
        ingest-free-work ingest-france-travail \
        run-aggregator run-worker clean

help:
	@echo "make setup-ingestion        - create ingestion venv, install deps"
	@echo "make setup-aggregator       - create aggregator venv (+ editable ingestion dep), install deps"
	@echo "make setup-worker           - create worker venv, install deps"
	@echo "make test                   - run all test suites (Python + Rust)"
	@echo "make ingest-free-work       - run a Free-Work ingestion pass (--max-pages 3)"
	@echo "make ingest-france-travail  - run a France Travail pass (needs ingestion/.env)"
	@echo "make run-aggregator         - start the aggregator HTTP API (needs aggregator/.env)"
	@echo "make run-worker             - run one worker extraction pass (needs worker/.env)"
	@echo "make clean                  - remove venvs and caches"

setup-ingestion:
	cd ingestion && python3 -m venv .venv && .venv/bin/pip install -e ".[dev]"

setup-aggregator:
	cd aggregator && python3 -m venv .venv && .venv/bin/pip install -e ../ingestion -e ".[dev]"

setup-worker:
	cd worker && python3 -m venv .venv && .venv/bin/pip install -e ".[dev]"

test: test-ingestion test-aggregator test-worker test-tui

test-tui:
	cd tui && cargo test

test-ingestion:
	cd ingestion && .venv/bin/pytest -q

test-aggregator:
	cd aggregator && .venv/bin/pytest -q

test-worker:
	cd worker && .venv/bin/pytest -q

ingest-free-work:
	cd ingestion && .venv/bin/python -m malt_radar_ingestion.ingest free_work --max-pages 3

ingest-france-travail:
	cd ingestion && set -a && . ./.env && set +a && .venv/bin/python -m malt_radar_ingestion.ingest france_travail --max-pages 3

run-aggregator:
	cd aggregator && set -a && . ./.env && set +a && .venv/bin/python -m malt_radar_aggregator.server

run-worker:
	cd worker && set -a && . ./.env && set +a && .venv/bin/python -m malt_radar_worker.run --limit 20

clean:
	rm -rf ingestion/.venv ingestion/.pytest_cache aggregator/.venv aggregator/.pytest_cache worker/.venv worker/.pytest_cache
	find ingestion aggregator worker -type d -name __pycache__ -exec rm -rf {} +
