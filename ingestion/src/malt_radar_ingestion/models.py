"""Shared data model for missions collected from any source."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass
class Mission:
    source: str
    external_id: int | str
    slug: str | None
    url: str
    title: str
    job_name: str
    description: str | None
    skills: list[str]
    min_daily_salary: float | None
    max_daily_salary: float | None
    currency: str | None
    remote_mode: str | None
    contracts: list[str]
    duration_value: int | None
    duration_period: str | None
    location_label: str | None
    company_name: str | None
    published_at: str | None
    raw_salary_text: str | None = None
