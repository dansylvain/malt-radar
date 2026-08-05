from datetime import datetime, timedelta, timezone

import pytest

from malt_radar_aggregator import digest

NOW_ISO = datetime.now(timezone.utc).isoformat()


class FakeToolUseBlock:
    type = "tool_use"

    def __init__(self, name: str, input: dict):
        self.name = name
        self.input = input


class FakeResponse:
    def __init__(
        self,
        summary_short: str = "Tout va bien.",
        content_full: str = "## Résumé exécutif\n...",
        stop_reason: str = "end_turn",
        tool_name: str = "emit_digest",
    ):
        self.content = [FakeToolUseBlock(tool_name, {"summary_short": summary_short, "content_full": content_full})]
        self.stop_reason = stop_reason


class FakeMessages:
    def __init__(self, response: FakeResponse):
        self._response = response
        self.last_kwargs: dict | None = None

    def create(self, **kwargs):
        self.last_kwargs = kwargs
        return self._response


class FakeClient:
    def __init__(self, response: FakeResponse | None = None):
        self.messages = FakeMessages(response or FakeResponse())


def make_fiche(**overrides) -> dict:
    defaults = {
        "title": "Consultant RAG",
        "company": "Acme",
        "url": "https://example.com/mission",
        "published_at": NOW_ISO,
        "tjm_min": 500,
        "tjm_max": 500,
        "stack": ["Python", "RAG"],
        "categorie": "RAG",
        "niveau": "senior",
    }
    defaults.update(overrides)
    return defaults


def write_fiche(fiches_dir, filename: str, **overrides) -> None:
    import yaml

    fiches_dir.mkdir(parents=True, exist_ok=True)
    frontmatter = make_fiche(**overrides)
    (fiches_dir / filename).write_text(
        "---\n" + yaml.safe_dump(frontmatter, allow_unicode=True) + "---\n\nbody\n", encoding="utf-8"
    )


@pytest.fixture(autouse=True)
def env(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "sk-test")
    monkeypatch.setenv("AGGREGATOR_DB_PATH", ":memory:")


def test_run_persists_digest_and_returns_it(tmp_path, monkeypatch):
    fiches_dir = tmp_path / "fiches"
    write_fiche(fiches_dir, "free_work_1.md")
    monkeypatch.setattr(digest, "FICHES_DIR", fiches_dir)
    fake_client = FakeClient(FakeResponse("Résumé court.", "## Résumé exécutif\nTout va bien."))

    result = digest.run(window_days=7, client=fake_client)

    assert result is not None
    assert result["fiche_count"] == 1
    assert result["summary_short"] == "Résumé court."
    assert result["content"] == "## Résumé exécutif\nTout va bien."
    assert result["id"] is not None
    assert result["metrics"]["fiche_count"] == 1


def test_run_truncates_oversized_summary_short(tmp_path, monkeypatch):
    fiches_dir = tmp_path / "fiches"
    write_fiche(fiches_dir, "free_work_1.md")
    monkeypatch.setattr(digest, "FICHES_DIR", fiches_dir)
    oversized = "mot " * 200  # well past SUMMARY_SHORT_MAX_CHARS
    fake_client = FakeClient(FakeResponse(oversized, "## Résumé exécutif\n..."))

    result = digest.run(window_days=7, client=fake_client)

    assert result is not None
    assert len(result["summary_short"]) <= digest.SUMMARY_SHORT_MAX_CHARS + 1  # +1 for the trailing "…"
    assert result["summary_short"].endswith("…")


def test_run_raises_systemexit_when_tool_call_missing(tmp_path, monkeypatch):
    fiches_dir = tmp_path / "fiches"
    write_fiche(fiches_dir, "free_work_1.md")
    monkeypatch.setattr(digest, "FICHES_DIR", fiches_dir)
    fake_client = FakeClient(FakeResponse(tool_name="some_other_tool"))

    with pytest.raises(SystemExit):
        digest.run(window_days=7, client=fake_client)


def test_run_returns_none_and_skips_api_call_when_window_empty(tmp_path, monkeypatch):
    fiches_dir = tmp_path / "fiches"
    fiches_dir.mkdir()
    monkeypatch.setattr(digest, "FICHES_DIR", fiches_dir)
    fake_client = FakeClient()

    result = digest.run(window_days=7, client=fake_client)

    assert result is None
    assert fake_client.messages.last_kwargs is None


def test_run_raises_systemexit_when_api_key_missing(tmp_path, monkeypatch):
    monkeypatch.delenv("ANTHROPIC_API_KEY", raising=False)
    fiches_dir = tmp_path / "fiches"
    monkeypatch.setattr(digest, "FICHES_DIR", fiches_dir)

    with pytest.raises(SystemExit):
        digest.run(window_days=7, client=None)


def test_run_raises_systemexit_on_refusal(tmp_path, monkeypatch):
    fiches_dir = tmp_path / "fiches"
    write_fiche(fiches_dir, "free_work_1.md")
    monkeypatch.setattr(digest, "FICHES_DIR", fiches_dir)
    fake_client = FakeClient(FakeResponse(stop_reason="refusal"))

    with pytest.raises(SystemExit):
        digest.run(window_days=7, client=fake_client)


def test_prompt_never_contains_raw_fiche_fields(tmp_path, monkeypatch):
    """Regression guard for the France Travail licence constraint (SOURCES.md):
    the digest prompt must only ever carry aggregated metrics, never a raw
    fiche's title/company/url."""
    fiches_dir = tmp_path / "fiches"
    write_fiche(
        fiches_dir,
        "free_work_1.md",
        title="Unique Title Marker XYZ",
        company="Unique Company Marker XYZ",
        url="https://unique-url-marker-xyz.example.com/mission",
    )
    monkeypatch.setattr(digest, "FICHES_DIR", fiches_dir)
    fake_client = FakeClient()

    digest.run(window_days=7, client=fake_client)

    sent_messages = fake_client.messages.last_kwargs["messages"]
    prompt_text = " ".join(m["content"] for m in sent_messages)
    assert "Unique Title Marker XYZ" not in prompt_text
    assert "Unique Company Marker XYZ" not in prompt_text
    assert "unique-url-marker-xyz" not in prompt_text
