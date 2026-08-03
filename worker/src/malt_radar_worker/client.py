"""HTTP client for the aggregator's API (see ../aggregator/src/malt_radar_aggregator/api.py):
pull unprocessed missions, push back generated fiches. Only outbound calls
— the worker never listens on a socket (see ROADMAP.md Phase 2 decision:
pull on our own timer, not a push endpoint on the Pi5)."""

from __future__ import annotations

import os

import httpx


def _base_url() -> str:
    url = os.environ.get("AGGREGATOR_URL")
    if not url:
        raise RuntimeError("Set AGGREGATOR_URL to the VPS aggregator's Tailscale URL, e.g. http://100.x.y.z:8321")
    return url


def _headers() -> dict[str, str]:
    token = os.environ.get("AGGREGATOR_API_TOKEN")
    if not token:
        raise RuntimeError("Set AGGREGATOR_API_TOKEN (shared secret configured on the aggregator)")
    return {"Authorization": f"Bearer {token}"}


def fetch_unprocessed(limit: int = 20, client: httpx.Client | None = None) -> list[dict]:
    owns_client = client is None
    if owns_client:
        client = httpx.Client(base_url=_base_url(), timeout=15.0)
    try:
        response = client.get("/missions/unprocessed", params={"limit": limit}, headers=_headers())
        response.raise_for_status()
        return response.json()
    finally:
        if owns_client:
            client.close()


def push_fiche(mission_id: int, markdown: str, client: httpx.Client | None = None) -> dict:
    owns_client = client is None
    if owns_client:
        client = httpx.Client(base_url=_base_url(), timeout=15.0)
    try:
        response = client.post(
            "/fiches",
            json={"mission_id": mission_id, "markdown": markdown},
            headers=_headers(),
        )
        response.raise_for_status()
        return response.json()
    finally:
        if owns_client:
            client.close()
