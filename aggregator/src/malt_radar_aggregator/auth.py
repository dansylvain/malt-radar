"""Shared-secret bearer auth for the aggregator API.

This API is only meant to be reachable over the Tailscale mesh (see
server.py and CLAUDE.md's network section — no public ports), but the
token is defense in depth: anything joining the tailnet with a
misconfigured ACL shouldn't get a free write path onto the mission DB.
"""

from __future__ import annotations

import os

from fastapi import Header, HTTPException


def require_token(authorization: str | None = Header(default=None)) -> None:
    expected = os.environ.get("AGGREGATOR_API_TOKEN")
    if not expected:
        raise HTTPException(status_code=503, detail="AGGREGATOR_API_TOKEN is not configured on the server")
    if authorization != f"Bearer {expected}":
        raise HTTPException(status_code=401, detail="Invalid or missing bearer token")
