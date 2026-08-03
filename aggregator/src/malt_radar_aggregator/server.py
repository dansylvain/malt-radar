"""Entrypoint for running the aggregator API under systemd.

Binds to the Tailscale interface's IP, never 0.0.0.0 — see CLAUDE.md's
network section ("pas de ports exposés publiquement"). Find the VPS's
Tailscale IP with `tailscale ip -4`.
"""

from __future__ import annotations

import os

import uvicorn


def main() -> None:
    host = os.environ.get("AGGREGATOR_HOST")
    if not host:
        raise SystemExit(
            "Set AGGREGATOR_HOST to the VPS's Tailscale IP (`tailscale ip -4`) before starting the server."
        )
    port = int(os.environ.get("AGGREGATOR_PORT", "8321"))
    uvicorn.run("malt_radar_aggregator.api:app", host=host, port=port)


if __name__ == "__main__":
    main()
