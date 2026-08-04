from malt_radar_worker.skills import normalize_skill, normalize_stack


def test_normalize_skill_strips_trailing_punctuation_but_preserves_casing():
    assert normalize_skill("rag.") == "rag"
    assert normalize_skill("Rag") == "Rag"
    assert normalize_skill("RAG") == "RAG"


def test_normalize_skill_collapses_internal_whitespace():
    assert normalize_skill("Python   3") == "Python 3"


def test_normalize_skill_passes_through_unknown_skill_cleaned():
    assert normalize_skill("  LangChain  ") == "LangChain"


def test_normalize_stack_dedupes_after_normalization_preserving_order():
    stack = ["Python", "rag", "FastAPI", "RAG.", "Rag Systems"]

    # "RAG." cleans to "RAG", a case-duplicate of "rag" (already seen) so
    # it's dropped; "Rag Systems" is a genuinely distinct key, kept as-is
    # (no more semantic synonym folding here — see skills.py's docstring).
    assert normalize_stack(stack) == ["Python", "rag", "FastAPI", "Rag Systems"]


def test_normalize_stack_drops_empty_entries():
    assert normalize_stack(["Python", "", "   ", "."]) == ["Python"]
