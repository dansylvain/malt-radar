"""Free-Work client: consumes their public job_postings JSON API directly.

No headless rendering needed — free-work.com serves listings through an
unauthenticated API Platform (Hydra) endpoint that the site's own frontend
calls. See SOURCES.md for the legal/robots.txt review of this source.
"""

from __future__ import annotations

import time
from typing import Any, Iterator

import httpx

from malt_radar_ingestion.models import Mission

USER_AGENT = "MaltRadarBot/0.1 (personal market-research project; contact: dansylvain84@gmail.com)"
BASE_URL = "https://www.free-work.com/api/job_postings"
ITEMS_PER_PAGE = 30
REQUEST_DELAY_SECONDS = 1.5
MAX_RETRIES = 3


def _get_with_retry(client: httpx.Client, url: str) -> httpx.Response:
    response: httpx.Response | None = None
    for attempt in range(1, MAX_RETRIES + 1):
        response = client.get(url)
        if response.status_code == 429:
            wait = float(response.headers.get("Retry-After", attempt * 5))
            time.sleep(wait)
            continue
        response.raise_for_status()
        return response
    response.raise_for_status()
    return response


def _to_mission(raw: dict[str, Any]) -> Mission:
    location = raw.get("location") or {}
    company = raw.get("company") or {}
    job_slug = (raw.get("job") or {}).get("slug")
    # The `/fr/tech-it/jobs/{slug}` form redirects to the site's homepage —
    # verified against the live site. The canonical, non-redirecting URL
    # needs the job category slug too, so without it there's no reliable
    # link to construct (no fallback to the known-dead form).
    url = f"https://www.free-work.com/fr/tech-it/job-mission/{job_slug}/{raw['slug']}" if job_slug else None
    return Mission(
        source="free_work",
        external_id=raw["id"],
        slug=raw["slug"],
        url=url,
        job_slug=job_slug,
        title=raw["title"],
        job_name=(raw.get("job") or {}).get("name", ""),
        description=raw.get("description"),
        skills=[s["name"] for s in raw.get("skills", [])],
        min_daily_salary=raw.get("minDailySalary"),
        max_daily_salary=raw.get("maxDailySalary"),
        currency=raw.get("currency"),
        remote_mode=raw.get("remoteMode"),
        contracts=raw.get("contracts", []),
        duration_value=raw.get("durationValue"),
        duration_period=raw.get("durationPeriod"),
        location_label=location.get("label"),
        company_name=company.get("name"),
        published_at=raw.get("publishedAt"),
    )


def fetch_missions(
    max_pages: int = 5,
    freelance_only: bool = True,
    client: httpx.Client | None = None,
) -> Iterator[Mission]:
    """Yield missions from Free-Work, paginating politely (delay between pages,
    retry/backoff on errors and 429s, identified User-Agent).

    `client` is an injection point for tests (a client wired to
    `httpx.MockTransport`) — leave it unset in normal use.
    """
    owns_client = client is None
    if owns_client:
        client = httpx.Client(headers={"User-Agent": USER_AGENT}, timeout=15.0)
    try:
        page = 1
        while page <= max_pages:
            url = f"{BASE_URL}?itemsPerPage={ITEMS_PER_PAGE}&premium=false&page={page}"
            response = _get_with_retry(client, url)
            data = response.json()
            members = data.get("hydra:member", [])
            if not members:
                break
            for raw in members:
                if freelance_only and "contractor" not in raw.get("contracts", []):
                    continue
                yield _to_mission(raw)
            if "hydra:next" not in (data.get("hydra:view") or {}):
                break
            page += 1
            time.sleep(REQUEST_DELAY_SECONDS)
    finally:
        if owns_client:
            client.close()


if __name__ == "__main__":
    for mission in fetch_missions(max_pages=1):
        print(mission.title, "-", mission.min_daily_salary, mission.max_daily_salary, mission.skills)
