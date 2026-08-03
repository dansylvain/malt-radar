import pytest
from pydantic import ValidationError

from malt_radar_worker.schema import Extraction


def test_extraction_applies_defaults():
    extraction = Extraction()

    assert extraction.devise == "EUR"
    assert extraction.stack == []
    assert extraction.categorie == "Autre"
    assert extraction.niveau is None


def test_extraction_accepts_full_payload():
    extraction = Extraction(
        tjm_min=500,
        tjm_max=700,
        devise="EUR",
        stack=["Python", "RAG"],
        duree_valeur=6,
        duree_unite="mois",
        niveau="senior",
        categorie="RAG",
    )

    assert extraction.tjm_min == 500
    assert extraction.categorie == "RAG"


def test_extraction_rejects_unknown_categorie():
    with pytest.raises(ValidationError):
        Extraction(categorie="Blockchain")


def test_extraction_rejects_unknown_niveau():
    with pytest.raises(ValidationError):
        Extraction(niveau="stagiaire")
