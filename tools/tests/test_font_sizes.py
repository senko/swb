"""Tests of `swbtools measure font-size-sweep` (`swbtools.font_sizes`).

The tests without Chromium check the arithmetic and the committed data
files. The Chromium tests measure samples and compare them with the data
files; they are skipped when Chromium cannot start.
"""

import asyncio

import pytest

from swbtools import font_sizes as fs
from swbtools import paths
from swbtools.browser import in_chromium

# The bundled fonts, as `read_units` reads them from Chromium.
UNITS = fs.Units(zero=(1229, 1024, 1303), a=1366, v=1366, kern_av=-152, kern_va=-152)


def widths_of(sizes: fs.Sizes) -> list[float]:
    """The four widths (px) that Chromium shows for `sizes`."""
    zeros = [fs.zeros_width(u, sizes.advance) / 64 for u in UNITS.zero]
    pairs = int(fs.pairs_widths(UNITS, sizes.advance, sizes.kerning)) / 64
    return [*zeros, pairs]


def test_model_sizes():
    # 16.21 in f32 is just below 16.21: kerning uses 16.2 px.
    assert fs.model_sizes(16210) == fs.Sizes(1620, 1036)
    assert fs.model_sizes(16211) == fs.Sizes(1621, 1037)
    assert fs.model_sizes(16000) == fs.Sizes(1600, 1024)
    assert fs.model_sizes(14400) == fs.Sizes(1440, 921)


def test_cache_key_truncates_twice():
    # 9.11 in f32 is below 9.11: the second truncation gives 9.10.
    assert fs.cache_key(fs.css_to_f32(9110)) == 910
    assert fs.cache_key(fs.css_to_f32(9111)) == 910
    assert fs.cache_key(fs.css_to_f32(9119)) == 910
    # 9.12 is the first size of its own key.
    assert fs.cache_key(fs.css_to_f32(9120)) == 912


def test_the_first_size_of_a_key_decides():
    isolated = {m: fs.model_sizes(m) for m in (9100, 9110, 9111, 9119, 9120)}
    shared = fs.shared_sizes([9110, 9111, 9119, 9120], isolated)
    assert shared[9110] == isolated[9110]
    assert shared[9111] == isolated[9110]
    assert shared[9119] == isolated[9110]
    assert shared[9120] == isolated[9120]
    # In the other order, 9.111 px creates the font.
    shared = fs.shared_sizes([9119, 9111, 9110], isolated)
    assert shared[9110] == shared[9111] == isolated[9119]


@pytest.mark.parametrize(
    "milli",
    # Sizes where the exact sum of the advances is a few 1/65536 px above a
    # multiple of 1/64 px: the width is the sum as f32.
    [17450, 20350, 20860, 21450, 9000, 12345, 24000],
)
def test_solve_finds_the_sizes_of_the_widths(milli):
    sizes = fs.model_sizes(milli)
    assert fs.solve(widths_of(sizes), UNITS) == sizes


def test_solve_rejects_widths_of_no_sizes():
    widths = widths_of(fs.model_sizes(12000))
    widths[3] += 50
    assert fs.solve(widths, UNITS) is None
    with pytest.raises(ValueError, match="multiple of 1/64"):
        fs.solve([1.001, 1, 1, 1], UNITS)


def test_size_ranges_join_equal_neighbors():
    a, b = fs.Sizes(900, 576), fs.Sizes(901, 576)
    ranges = fs.size_ranges({9000: a, 9001: a, 9002: b, 9004: b})
    assert ranges == [
        fs.SizeRange(9000, 9001, a),
        fs.SizeRange(9002, 9002, b),
        fs.SizeRange(9004, 9004, b),
    ]
    text = fs.format_sizes(ranges, "148")
    assert fs.parse_sizes(text) == {9000: a, 9001: a, 9002: b, 9004: b}


def test_sum_width_is_the_sum_as_f32():
    # 2068.4 px is between f32 numbers 1/4096 px apart.
    total16 = 135555077
    assert int(fs.sum_width([total16])) == 132378
    assert int(fs.sum_width([total16 - 5])) == 132378
    assert int(fs.sum_width([total16 + 12])) == 132379


def test_scale_is_truncated():
    # 17.45 in f32 is 17.4500007629..., times 65536 is 1143603.2...
    assert int(fs.scale16(1745)) == 1143603
    assert int(fs.scale16(1600)) == 16 * 65536


def committed_sizes() -> dict[int, fs.Sizes]:
    return fs.parse_sizes((paths.repo_root() / fs.SIZES_FILE).read_text(encoding="utf-8"))


def test_committed_sizes_are_swb_rule():
    by_size = committed_sizes()
    assert sorted(by_size) == fs.sweep_sizes()
    assert fs.swb_differences(by_size) == []


def test_committed_widths_are_explained_by_the_sizes():
    by_size = committed_sizes()
    rows = 0
    for line in (paths.repo_root() / fs.WIDTHS_FILE).read_text(encoding="utf-8").splitlines():
        if line.startswith("#") or not line.strip():
            continue
        milli, *widths = (int(field) for field in line.split())
        assert [w / 64 for w in widths] == widths_of(by_size[milli])
        rows += 1
    assert rows == len(range(fs.FIRST, fs.LAST + 1, fs.WIDTH_STEP))


@pytest.mark.usefixtures("chromium")
def test_chromium_sizes_match_the_data():
    # About 150 sizes, with the sizes where Chromium differs from the
    # unrounded rule.
    sample = [*range(fs.FIRST, fs.LAST + 1, 97), 9111, 16211, 17450, 20350]

    async def run(page, directory):
        chromium = page.context.browser
        units = await fs.read_units(chromium, directory)
        widths = await fs.measure_isolated(chromium, directory, sample)
        return units, *fs.solve_all(widths, units)

    units, by_size, unexplained = asyncio.run(in_chromium(run))
    assert units == UNITS
    assert unexplained == []
    data = committed_sizes()
    assert by_size == {m: data[m] for m in sample}


@pytest.mark.usefixtures("chromium")
def test_chromium_shares_fonts_in_one_page():
    sizes = [9110, 9111, 9112, 9119, 9120, 16200, 16211, 16220, 16221]

    async def run(page, directory):
        chromium = page.context.browser
        units = await fs.read_units(chromium, directory)
        isolated = await fs.measure_isolated(chromium, directory, sizes)
        scan = await fs.measure_scan(page, directory, sizes)
        return fs.solve_all(isolated, units)[0], fs.solve_all(scan, units)[0]

    isolated, scan = asyncio.run(in_chromium(run))
    assert scan == fs.shared_sizes(sizes, isolated)
    # The later sizes get the font of the first size of their key.
    assert scan[9111] == scan[9112] == scan[9119] == isolated[9110] != isolated[9111]
    assert scan[9120] == isolated[9120]
    assert scan[16211] == scan[16220] == isolated[16200] != isolated[16211]
    assert scan[16221] == isolated[16221]
