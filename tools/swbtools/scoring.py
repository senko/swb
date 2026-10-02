"""Geometry scores: compare a swb box dump with a Chromium box dump.

The algorithm is described in docs/testing.md ("Scores"). In short:

1. Align elements. If both dumps have the same tag sequence, element i
   pairs with element i. Otherwise the tag sequences are aligned with the
   fewest insertions and deletions (Myers' algorithm), and only elements
   with equal tags pair.
2. Every element that has a box in Chromium counts once in each score. It
   passes if its swb partner has a box within the tolerance:
   - `geometry`: x, y, width and height;
   - `size`: width and height;
   - `relative`: x and y relative to the nearest ancestor that has a box in
     both browsers, and width and height.
3. `missing`: elements with a box only in Chromium (or without a partner).
   `extra`: elements with a box only in swb (or without a partner).
"""

import difflib
from collections.abc import Sequence
from dataclasses import dataclass, field

from swbtools.boxes import BoxDump, Rect

DEFAULT_TOLERANCE = 2.0
"""Maximum difference in CSS px for each of x, y, width and height."""


MAX_EDITS = 2000
"""Edit distance limit for the exact alignment. Above it, difflib aligns."""


@dataclass(frozen=True)
class Difference:
    """A block of the alignment where the tag sequences differ."""

    kind: str
    """`replace`, `delete` (only in Chromium) or `insert` (only in swb)."""
    reference: tuple[int, int]
    """Index range in the Chromium dump (end exclusive)."""
    swb: tuple[int, int]
    """Index range in the swb dump (end exclusive)."""


@dataclass(frozen=True)
class Alignment:
    """Element pairs between the Chromium dump and the swb dump."""

    pairs: dict[int, int]
    """Chromium index to swb index."""
    identical: bool
    """True if the tag sequences are equal."""
    differences: list[Difference]


def _common_prefix(a: Sequence[str], b: Sequence[str]) -> int:
    n = 0
    for x, y in zip(a, b, strict=False):
        if x != y:
            break
        n += 1
    return n


def myers_matches(
    a: Sequence[str], b: Sequence[str], max_edits: int = MAX_EDITS
) -> list[tuple[int, int]] | None:
    """Returns the index pairs of a longest common subsequence of `a` and
    `b`, in order, or None if more than `max_edits` insertions and deletions
    are needed.

    Myers' O(ND) difference algorithm: "An O(ND) Difference Algorithm and
    Its Variations", Algorithmica 1(2), 1986. On ties, it deletes from `a`
    before it inserts from `b`.
    """
    n, m = len(a), len(b)
    limit = min(n + m, max_edits)
    offset = limit + 1
    v = [0] * (2 * limit + 3)
    # trace[d] holds v[-d-1 .. d+1] as it was before round d.
    trace: list[list[int]] = []
    for d in range(limit + 1):
        trace.append(v[offset - d - 1 : offset + d + 2])
        for k in range(-d, d + 1, 2):
            if k == -d or (k != d and v[offset + k - 1] < v[offset + k + 1]):
                x = v[offset + k + 1]
            else:
                x = v[offset + k - 1] + 1
            y = x - k
            while x < n and y < m and a[x] == b[y]:
                x += 1
                y += 1
            v[offset + k] = x
            if x >= n and y >= m:
                return _backtrack(trace, n, m)
    return None


def _backtrack(trace: list[list[int]], n: int, m: int) -> list[tuple[int, int]]:
    """Follows the path of `myers_matches` back from (n, m) and collects the
    diagonal moves (the matches)."""
    matches: list[tuple[int, int]] = []
    x, y = n, m
    for d in range(len(trace) - 1, 0, -1):
        v = trace[d]  # v[k + d + 1] is the furthest x on diagonal k
        k = x - y
        down = k == -d or (k != d and v[k + d] < v[k + d + 2])
        previous_k = k + 1 if down else k - 1
        previous_x = v[previous_k + d + 1]
        previous_y = previous_x - previous_k
        while x > previous_x and y > previous_y:
            x -= 1
            y -= 1
            matches.append((x, y))
        x, y = previous_x, previous_y
    # Round 0: the snake from (0, 0).
    while x > 0 and y > 0:
        x -= 1
        y -= 1
        matches.append((x, y))
    matches.reverse()
    return matches


def _difflib_matches(a: Sequence[str], b: Sequence[str]) -> list[tuple[int, int]]:
    matcher = difflib.SequenceMatcher(None, a, b, autojunk=False)
    return [
        (block.a + i, block.b + i)
        for block in matcher.get_matching_blocks()
        for i in range(block.size)
    ]


def _differences(matches: list[tuple[int, int]], n: int, m: int, start: int) -> list[Difference]:
    """The gaps between matched pairs, as difference blocks. Indices are
    shifted by `start`."""
    differences = []
    i = j = 0
    for x, y in [*matches, (n, m)]:
        if x > i or y > j:
            kind = "replace" if x > i and y > j else ("delete" if x > i else "insert")
            differences.append(Difference(kind, (start + i, start + x), (start + j, start + y)))
        i, j = x + 1, y + 1
    return differences


def align(reference: Sequence[str], swb: Sequence[str]) -> Alignment:
    """Aligns two tag sequences so that the most elements pair.

    The common prefix and suffix pair directly. Myers' algorithm aligns the
    rest with the fewest insertions and deletions. If that needs more than
    `MAX_EDITS` edits, difflib aligns the rest instead (faster, but it can
    pair the wrong copies of repeated structures).
    """
    if list(reference) == list(swb):
        return Alignment({i: i for i in range(len(reference))}, True, [])
    prefix = _common_prefix(reference, swb)
    max_suffix = min(len(reference), len(swb)) - prefix
    suffix = min(_common_prefix(reference[::-1], swb[::-1]), max_suffix)
    ref_middle = reference[prefix : len(reference) - suffix]
    swb_middle = swb[prefix : len(swb) - suffix]
    matches = myers_matches(ref_middle, swb_middle)
    if matches is None:
        matches = _difflib_matches(ref_middle, swb_middle)
    pairs = {i: i for i in range(prefix)}
    pairs.update((prefix + x, prefix + y) for x, y in matches)
    for offset in range(1, suffix + 1):
        pairs[len(reference) - offset] = len(swb) - offset
    differences = _differences(matches, len(ref_middle), len(swb_middle), prefix)
    return Alignment(pairs, False, differences)


def within(a: Sequence[float], b: Sequence[float], tolerance: float) -> bool:
    """True if every component differs by at most `tolerance`."""
    return all(abs(x - y) <= tolerance for x, y in zip(a, b, strict=True))


@dataclass(frozen=True)
class ElementResult:
    """The comparison of one Chromium element that has a box."""

    index: int
    """Index in the Chromium dump."""
    swb_index: int | None
    reference: Rect
    swb: Rect | None
    anchor: int | None
    """Chromium index of the ancestor used for `relative`, if any."""
    geometry: bool
    size: bool
    relative: bool

    @property
    def delta(self) -> tuple[float, float, float, float] | None:
        """swb minus Chromium, per component."""
        if self.swb is None:
            return None
        return (
            self.swb[0] - self.reference[0],
            self.swb[1] - self.reference[1],
            self.swb[2] - self.reference[2],
            self.swb[3] - self.reference[3],
        )

    @property
    def error(self) -> float:
        """The largest absolute component of `delta` (infinite if missing)."""
        delta = self.delta
        return float("inf") if delta is None else max(abs(value) for value in delta)


@dataclass(frozen=True)
class Scores:
    """Scores of one comparison. Fractions are in [0, 1]."""

    geometry: float
    size: float
    relative: float
    missing: int
    extra: int
    pixels: float | None = None


@dataclass
class Comparison:
    """The full result of comparing two box dumps."""

    reference: BoxDump
    swb: BoxDump
    alignment: Alignment
    tolerance: float
    results: list[ElementResult] = field(default_factory=list)
    missing: list[int] = field(default_factory=list)
    """Chromium indices of elements with a box only in Chromium."""
    extra: list[int] = field(default_factory=list)
    """swb indices of elements with a box only in swb."""

    def scores(self, pixels: float | None = None) -> Scores:
        total = len(self.results)

        def fraction(passed: int) -> float:
            return passed / total if total else 1.0

        return Scores(
            geometry=fraction(sum(result.geometry for result in self.results)),
            size=fraction(sum(result.size for result in self.results)),
            relative=fraction(sum(result.relative for result in self.results)),
            missing=len(self.missing),
            extra=len(self.extra),
            pixels=pixels,
        )


def _relative(rect: Rect, anchor: Rect | None) -> Rect:
    if anchor is None:
        return rect
    return (rect[0] - anchor[0], rect[1] - anchor[1], rect[2], rect[3])


def compare(reference: BoxDump, swb: BoxDump, tolerance: float = DEFAULT_TOLERANCE) -> Comparison:
    """Compares the swb dump with the Chromium dump."""
    alignment = align(
        [element.tag for element in reference.elements], [element.tag for element in swb.elements]
    )
    comparison = Comparison(reference, swb, alignment, tolerance)
    pairs = alignment.pairs

    def swb_rect(index: int) -> Rect | None:
        partner = pairs.get(index)
        return None if partner is None else swb.elements[partner].rect

    for index, element in enumerate(reference.elements):
        if element.rect is None:
            continue
        partner = pairs.get(index)
        other = swb_rect(index)
        if other is None:
            comparison.missing.append(index)
            comparison.results.append(
                ElementResult(index, partner, element.rect, None, None, False, False, False)
            )
            continue
        anchor = element.parent
        while anchor is not None and (
            reference.elements[anchor].rect is None or swb_rect(anchor) is None
        ):
            anchor = reference.elements[anchor].parent
        anchor_ref = None if anchor is None else reference.elements[anchor].rect
        anchor_swb = None if anchor is None else swb_rect(anchor)
        comparison.results.append(
            ElementResult(
                index=index,
                swb_index=partner,
                reference=element.rect,
                swb=other,
                anchor=anchor,
                geometry=within(element.rect, other, tolerance),
                size=within(element.rect[2:], other[2:], tolerance),
                relative=within(
                    _relative(element.rect, anchor_ref), _relative(other, anchor_swb), tolerance
                ),
            )
        )

    paired_swb = set(pairs.values())
    for index, element in enumerate(swb.elements):
        if element.rect is None:
            continue
        if index not in paired_swb:
            comparison.extra.append(index)
    for index, partner in pairs.items():
        if reference.elements[index].rect is None and swb.elements[partner].rect is not None:
            comparison.extra.append(partner)
    comparison.extra.sort()
    return comparison


def element_paths(dump: BoxDump) -> list[str]:
    """Returns a CSS-like path for each element, for example
    `html > body > div:nth-child(3)`. `:nth-child` is added when the parent
    has more than one element child. Without parent indices, the path is
    the tag and the index (`div #12`)."""
    if not dump.has_parents:
        return [f"{element.tag} #{index}" for index, element in enumerate(dump.elements)]
    children: dict[int | None, list[int]] = {}
    for index, element in enumerate(dump.elements):
        children.setdefault(element.parent, []).append(index)
    position: dict[int, int] = {}
    for siblings in children.values():
        for number, index in enumerate(siblings, start=1):
            position[index] = number
    result: list[str] = []
    for index, element in enumerate(dump.elements):
        step = element.tag
        if len(children[element.parent]) > 1:
            step += f":nth-child({position[index]})"
        parent = element.parent
        result.append(step if parent is None else f"{result[parent]} > {step}")
    return result
