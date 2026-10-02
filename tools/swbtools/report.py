"""The HTML report of one comparison (`out/compare/<name>/report.html`)."""

from collections.abc import Sequence
from html import escape
from pathlib import Path

from swbtools.boxes import Rect
from swbtools.scoring import Comparison, Scores, element_paths

TABLE_ROWS = 100
"""Maximum number of rows in each element table."""

_STYLE = """
body { font: 14px/1.4 system-ui, sans-serif; margin: 16px; color: #222; background: #fff; }
h1 { font-size: 20px; margin: 0 0 4px; }
h2 { font-size: 16px; margin: 24px 0 8px; }
table { border-collapse: collapse; }
th, td { border: 1px solid #ccc; padding: 2px 6px; text-align: left; vertical-align: top; }
td.num { text-align: right; font-variant-numeric: tabular-nums; white-space: nowrap; }
td.path { font-family: monospace; font-size: 12px; overflow-wrap: anywhere; max-width: 520px; }
.bad { color: #b00; }
.shots { display: flex; gap: 8px; flex-wrap: wrap; }
.shots figure { margin: 0; flex: 1 1 400px; min-width: 0; }
.shots img { width: 100%; border: 1px solid #ccc; }
.meta { color: #555; }
"""

RECT_HEADER = "x, y, w&times;h"


def _rect(rect: Rect | None) -> str:
    if rect is None:
        return "none"
    x, y, width, height = rect
    return f"{x:.2f}, {y:.2f}, {width:.2f}&times;{height:.2f}"


def _delta(values: Sequence[float] | None, tolerance: float) -> str:
    if values is None:
        return ""
    cells = []
    for value in values:
        css = ' class="bad"' if abs(value) > tolerance else ""
        cells.append(f"<span{css}>{value:+.2f}</span>")
    return " ".join(cells)


def _yes_no(value: bool) -> str:
    return "yes" if value else "no"


def _cell(value: str, kind: str = "") -> str:
    """A table cell. `value` is HTML. `kind` is a CSS class: num or path."""
    return f"<td class={kind}>{value}</td>" if kind else f"<td>{value}</td>"


def _table(headers: Sequence[str], rows: Sequence[Sequence[str]]) -> str:
    """A table. Headers and rows are HTML; rows are lists of `_cell` output."""
    head = "".join(f"<th>{header}</th>" for header in headers)
    body = "".join(f"<tr>{''.join(row)}</tr>" for row in rows)
    return f"<table><tr>{head}</tr>{body}</table>"


def _scores_table(scores: Scores, comparison: Comparison) -> str:
    reference, swb = comparison.reference, comparison.swb
    pixels = "n/a" if scores.pixels is None else f"{scores.pixels:.4f}"
    tags = "same tag sequence" if comparison.alignment.identical else "tag sequences differ"
    rows = [
        ("geometry", f"{scores.geometry:.4f}", "x, y, width, height within tolerance"),
        ("size", f"{scores.size:.4f}", "width and height within tolerance"),
        ("relative", f"{scores.relative:.4f}", "position relative to nearest common ancestor"),
        ("pixels", pixels, "first viewport, max channel difference within threshold"),
        ("missing", str(scores.missing), "box only in Chromium"),
        ("extra", str(scores.extra), "box only in swb"),
        ("elements", f"{len(reference.elements)} / {len(swb.elements)}", f"Chromium / swb; {tags}"),
        ("boxes", str(len(comparison.results)), "elements with a box in Chromium"),
    ]
    return _table(
        ["score", "value", "meaning"],
        [[_cell(name), _cell(value, "num"), _cell(escape(note))] for name, value, note in rows],
    )


def _tags(tags: Sequence[str], start: int, end: int) -> str:
    shown = " ".join(tags[start:end][:20])
    return escape(shown + (" ..." if end - start > 20 else ""))


def _differences(comparison: Comparison) -> str:
    alignment = comparison.alignment
    if alignment.identical:
        return ""
    reference_tags = [element.tag for element in comparison.reference.elements]
    swb_tags = [element.tag for element in comparison.swb.elements]
    rows = []
    for difference in alignment.differences[:TABLE_ROWS]:
        r1, r2 = difference.reference
        s1, s2 = difference.swb
        rows.append(
            [
                _cell(difference.kind),
                _cell(f"{r1}&ndash;{r2}", "num"),
                _cell(_tags(reference_tags, r1, r2)),
                _cell(f"{s1}&ndash;{s2}", "num"),
                _cell(_tags(swb_tags, s1, s2)),
            ]
        )
    return (
        "<h2>Tag sequence differences</h2>"
        f"<p>{len(alignment.differences)} blocks differ. Elements in these blocks have no "
        "partner and count as missing or extra. Index ranges are end-exclusive.</p>"
        + _table(["kind", "Chromium indices", "Chromium tags", "swb indices", "swb tags"], rows)
    )


def _worst(comparison: Comparison, paths: list[str]) -> str:
    candidates = [result for result in comparison.results if result.swb is not None]
    candidates.sort(key=lambda result: (-result.error, result.index))
    rows = []
    for result in candidates[:TABLE_ROWS]:
        if result.geometry:
            break
        rows.append(
            [
                _cell(str(result.index), "num"),
                _cell(escape(paths[result.index]), "path"),
                _cell(_rect(result.reference), "num"),
                _cell(_rect(result.swb), "num"),
                _cell(_delta(result.delta, comparison.tolerance), "num"),
                _cell(_yes_no(result.size)),
                _cell(_yes_no(result.relative)),
            ]
        )
    if not rows:
        return "<h2>Worst matches</h2><p>All elements match within the tolerance.</p>"
    headers = [
        "#",
        "path",
        f"Chromium {RECT_HEADER}",
        f"swb {RECT_HEADER}",
        "delta",
        "size ok",
        "relative ok",
    ]
    return (
        f"<h2>Worst matches (up to {TABLE_ROWS})</h2>"
        "<p>Elements with a box in both browsers that do not match, sorted by the largest "
        "difference. Delta: swb minus Chromium for x, y, width, height.</p>" + _table(headers, rows)
    )


def _missing(comparison: Comparison, paths: list[str]) -> str:
    if not comparison.missing:
        return ""
    rows = [
        [
            _cell(str(index), "num"),
            _cell(escape(paths[index]), "path"),
            _cell(_rect(comparison.reference.elements[index].rect), "num"),
        ]
        for index in comparison.missing[:TABLE_ROWS]
    ]
    return f"<h2>Missing in swb ({len(comparison.missing)})</h2>" + _table(
        ["#", "path", f"Chromium {RECT_HEADER}"], rows
    )


def _extra(comparison: Comparison) -> str:
    if not comparison.extra:
        return ""
    swb_paths = element_paths(comparison.swb)
    rows = [
        [
            _cell(str(index), "num"),
            _cell(escape(swb_paths[index]), "path"),
            _cell(_rect(comparison.swb.elements[index].rect), "num"),
        ]
        for index in comparison.extra[:TABLE_ROWS]
    ]
    return f"<h2>Extra in swb ({len(comparison.extra)})</h2>" + _table(
        ["swb #", "path", f"swb {RECT_HEADER}"], rows
    )


def write_report(
    path: Path,
    name: str,
    comparison: Comparison,
    scores: Scores,
    images: dict[str, str],
    log_file: str | None,
) -> None:
    """Writes the report. `images` maps a caption to an image path relative
    to the report."""
    paths = element_paths(comparison.reference)
    shots = "".join(
        f'<figure><figcaption>{escape(caption)}</figcaption><a href="{escape(src)}">'
        f'<img src="{escape(src)}" alt="{escape(caption)}"></a></figure>'
        for caption, src in images.items()
    )
    log_link = f' · <a href="{escape(log_file)}">swb log</a>' if log_file else ""
    title = f"swb vs Chromium: {escape(name)}"
    html = (
        "<!doctype html><html lang=en><head><meta charset=utf-8>"
        '<meta name="viewport" content="width=device-width, initial-scale=1">'
        f"<title>{title}</title><style>{_STYLE}</style></head><body>"
        f"<h1>{title}</h1>"
        f"<p class=meta>{escape(comparison.reference.url)} · tolerance "
        f"{comparison.tolerance:g} px{log_link}</p>"
        f"{_scores_table(scores, comparison)}"
        f"<h2>First viewport</h2><div class=shots>{shots}</div>"
        f"{_differences(comparison)}{_worst(comparison, paths)}"
        f"{_missing(comparison, paths)}{_extra(comparison)}"
        "</body></html>\n"
    )
    path.write_text(html, encoding="utf-8")
