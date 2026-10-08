"""The committed page fixtures do not publish photos or commercial fonts
(docs/testing.md, "Substitute copyrighted content")."""

import pytest

from swbtools import paths
from swbtools.pages import list_fixtures
from swbtools.substitute import check_fixture

EXEMPT = {
    "senko-net": "the owner's own site",
    "hacker-news": "a 1x1 spacer GIF; captured before the substitution rule",
    "wikipedia-web-browser": "Wikimedia media under free licenses; captured before the rule",
}
"""Fixtures that keep their original images, with the reason."""


@pytest.mark.parametrize("name", list_fixtures())
def test_fixture_has_only_placeholders_and_free_fonts(name: str) -> None:
    if name in EXEMPT:
        pytest.skip(EXEMPT[name])
    assert check_fixture(paths.fixture_dir(name)) == []
