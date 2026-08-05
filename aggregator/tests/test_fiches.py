import os

from malt_radar_aggregator.fiches import list_fiches, read_fiche


def test_read_fiche_parses_frontmatter(tmp_path):
    path = tmp_path / "f.md"
    path.write_text("---\ntitle: Consultant RAG\ntjm_min: 500\n---\n\n# Consultant RAG\n", encoding="utf-8")

    fiche = read_fiche(path)

    assert fiche["title"] == "Consultant RAG"
    assert fiche["tjm_min"] == 500
    assert fiche["filename"] == "f.md"


def test_read_fiche_returns_none_for_malformed_frontmatter(tmp_path):
    path = tmp_path / "broken.md"
    path.write_text("not a fiche at all", encoding="utf-8")

    assert read_fiche(path) is None


def test_read_fiche_returns_none_when_frontmatter_is_not_a_mapping(tmp_path):
    path = tmp_path / "weird.md"
    path.write_text("---\n- a\n- b\n---\n\nbody\n", encoding="utf-8")

    assert read_fiche(path) is None


def test_list_fiches_returns_empty_for_missing_dir(tmp_path):
    assert list_fiches(tmp_path / "does-not-exist") == []


def test_list_fiches_orders_newest_first_by_mtime(tmp_path):
    older = tmp_path / "a.md"
    older.write_text("---\ntitle: Older\n---\n\nbody\n", encoding="utf-8")
    newer = tmp_path / "b.md"
    newer.write_text("---\ntitle: Newer\n---\n\nbody\n", encoding="utf-8")
    os.utime(older, (1_700_000_000, 1_700_000_000))
    os.utime(newer, (1_700_000_100, 1_700_000_100))

    result = list_fiches(tmp_path)

    assert [f["title"] for f in result] == ["Newer", "Older"]


def test_list_fiches_respects_limit(tmp_path):
    for i in range(3):
        (tmp_path / f"f{i}.md").write_text(f"---\ntitle: F{i}\n---\n\nbody\n", encoding="utf-8")

    result = list_fiches(tmp_path, limit=2)

    assert len(result) == 2


def test_list_fiches_skips_malformed_files(tmp_path):
    (tmp_path / "broken.md").write_text("nope", encoding="utf-8")
    (tmp_path / "good.md").write_text("---\ntitle: OK\n---\n\nbody\n", encoding="utf-8")

    result = list_fiches(tmp_path)

    assert len(result) == 1
    assert result[0]["title"] == "OK"
