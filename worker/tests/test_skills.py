from malt_radar_worker.skills import normalize_skill, normalize_stack


def test_normalize_skill_folds_case_and_trailing_punctuation():
    assert normalize_skill("rag.") == "RAG"
    assert normalize_skill("Rag") == "RAG"
    assert normalize_skill("RAG") == "RAG"


def test_normalize_skill_folds_known_synonym():
    assert normalize_skill("RAG Systems") == "RAG"
    assert normalize_skill("rag systems") == "RAG"


def test_normalize_skill_collapses_internal_whitespace():
    assert normalize_skill("Python   3") == "Python 3"


def test_normalize_skill_passes_through_unknown_skill_cleaned():
    assert normalize_skill("  LangChain  ") == "LangChain"


def test_normalize_stack_dedupes_after_normalization_preserving_order():
    stack = ["Python", "rag", "FastAPI", "RAG.", "Rag Systems"]

    assert normalize_stack(stack) == ["Python", "RAG", "FastAPI"]


def test_normalize_stack_drops_empty_entries():
    assert normalize_stack(["Python", "", "   ", "."]) == ["Python"]
