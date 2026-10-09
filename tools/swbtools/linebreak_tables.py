"""`swbtools linebreak-tables`: Rust tables for line breaking.

Writes `crates/text/src/linebreak/tables.rs` from two sources:

1. The Unicode Character Database. swb gets the Line_Break property from
   the `unicode-linebreak` crate, which has the data of Unicode 15.0;
   Chromium 148 uses Unicode 17.0. The tables:
   - `CLASS_OVERRIDES`: the code point ranges whose Line_Break value in 17.0
     differs from 15.0 (new classes AK, AP, AS, VF, VI, HH; new characters).
   - `EAST_ASIAN`: East_Asian_Width F, W or H (`$EastAsian` of UAX #14).
   - `SA_MARKS`: Line_Break SA with General_Category Mn or Mc (LB1).
   - `INITIAL_QUOTES`, `FINAL_QUOTES`: Line_Break QU with General_Category
     Pi or Pf (LB15a, LB15b, LB19).
   - `UNASSIGNED_PICTOGRAPHIC`: Extended_Pictographic and unassigned (LB30b).
   - `LETTER_UNITS`: General_Category L* or N*, the "typographic letter
     units" of `word-break: keep-all` (CSS Text 3 §5.2).
   The files are downloaded from unicode.org into `out/ucd/` unless they are
   there already (`ucd.py`).

2. The measurements of Chromium in `crates/text/tests/linebreak/latin1.txt`
   (`swbtools linebreaks`): the break opportunity between every pair of
   code points U+0020..U+00FF, between letters (`a x y a`), for
   `word-break: normal` (the same as `keep-all`) and `break-all`. Code
   points with equal rows and columns form a group; the tables give the
   group of each code point and, for each group, the set of groups before
   which the line can break.
"""

import logging
from pathlib import Path

from swbtools import paths
from swbtools.linebreaks import LATIN1, MATRIX_SYMBOLS, data_dir
from swbtools.ucd import has_property, ranges, read_property, ucd_file, value_ranges

log = logging.getLogger(__name__)

OLD_VERSION = "15.0.0"
"""The Unicode version of the `unicode-linebreak` crate's data."""

VERSION = "17.0.0"
"""The Unicode version of Chromium 148's line breaking, as measured (for
example, U+2013 breaks like the new class HH of 17.0)."""

FILES = {
    "LineBreak": "ucd/LineBreak.txt",
    "EastAsianWidth": "ucd/EastAsianWidth.txt",
    "DerivedGeneralCategory": "ucd/extracted/DerivedGeneralCategory.txt",
    "emoji-data": "ucd/emoji/emoji-data.txt",
}

SOFT_HYPHEN = 0xAD


def _range_list(name: str, doc: str, items: list[tuple[int, int]]) -> list[str]:
    lines = [f"/// {doc}", f"pub(super) static {name}: [(u32, u32); {len(items)}] = ["]
    lines += [f"    (0x{a:04X}, 0x{b:04X})," for a, b in items]
    lines.append("];")
    lines.append("")
    return lines


def ucd_tables() -> list[str]:
    """The Rust source of the UCD tables."""
    old = read_property(ucd_file(FILES["LineBreak"], OLD_VERSION), "XX")
    new = read_property(ucd_file(FILES["LineBreak"], VERSION), "XX")
    width = read_property(ucd_file(FILES["EastAsianWidth"], VERSION), "N")
    category = read_property(ucd_file(FILES["DerivedGeneralCategory"], VERSION), "Cn")
    pictographic = has_property(ucd_file(FILES["emoji-data"], VERSION), "Extended_Pictographic")
    pairs = list(zip(new, category, strict=True))

    overrides = value_ranges([n if n != o else None for o, n in zip(old, new, strict=True)])
    lines = [
        "/// The first code point whose Line_Break value differs from the data of",
        "/// `unicode-linebreak`.",
        f"pub(super) const FIRST_OVERRIDE: u32 = 0x{overrides[0][0]:04X};",
        "",
        f"/// Line_Break values of Unicode {VERSION} that differ from those of Unicode",
        f"/// {OLD_VERSION} (the data of the `unicode-linebreak` crate), as sorted,",
        "/// disjoint ranges.",
        f"pub(super) static CLASS_OVERRIDES: [(u32, u32, Class); {len(overrides)}] = [",
    ]
    lines += [f"    (0x{a:04X}, 0x{b:04X}, Class::{v})," for a, b, v in overrides]
    lines += ["];", ""]
    lines += _range_list(
        "EAST_ASIAN",
        "East_Asian_Width F, W or H (`$EastAsian` in UAX #14).",
        ranges([w in ("F", "W", "H") for w in width]),
    )
    lines += _range_list(
        "SA_MARKS",
        "Line_Break SA with General_Category Mn or Mc: resolved to CM (LB1).",
        ranges([lb == "SA" and gc in ("Mn", "Mc") for lb, gc in pairs]),
    )
    lines += _range_list(
        "INITIAL_QUOTES",
        "Line_Break QU with General_Category Pi.",
        ranges([lb == "QU" and gc == "Pi" for lb, gc in pairs]),
    )
    lines += _range_list(
        "FINAL_QUOTES",
        "Line_Break QU with General_Category Pf.",
        ranges([lb == "QU" and gc == "Pf" for lb, gc in pairs]),
    )
    lines += _range_list(
        "UNASSIGNED_PICTOGRAPHIC",
        "Extended_Pictographic code points that are unassigned (LB30b).",
        ranges([e and gc == "Cn" for e, gc in zip(pictographic, category, strict=True)]),
    )
    lines += _range_list(
        "LETTER_UNITS",
        "General_Category L* or N*: typographic letter units (`keep-all`).",
        ranges([gc[0] in "LN" for gc in category]),
    )
    return lines


def read_matrices(path: Path) -> dict[str, list[list[bool | None]]]:
    """The Latin-1 matrices of a data file by mode directive; a cell is
    True (break), False (no break) or None (unknown)."""
    symbols = {v: k for k, v in MATRIX_SYMBOLS.items()}
    out: dict[str, list[list[bool | None]]] = {}
    rows: list[list[bool | None]] | None = None
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.startswith("@matrix "):
            fields = line.split()[1:]
            mode = " ".join(f for f in fields if f.startswith(("word-break=", "hyphens=")))
            rows = out.setdefault(mode, [])
        elif line.startswith(("@chars", "#")) or not line:
            continue
        elif rows is not None:
            rows.append([{"÷": True, "×": False}.get(symbols[c]) for c in line])
    for mode, matrix in out.items():
        if len(matrix) != len(LATIN1) or any(len(r) != len(LATIN1) for r in matrix):
            raise ValueError(f"{path}: the {mode} matrix is not {len(LATIN1)}x{len(LATIN1)}")
    return out


def _filled(matrix: list[list[bool | None]], fallback: list[list[bool | None]]) -> list[list[bool]]:
    """`matrix` with unknown cells taken from `fallback` (or no break)."""
    out = []
    for i, row in enumerate(matrix):
        out.append([bool(fallback[i][j]) if v is None else v for j, v in enumerate(row)])
    unknown = sum(v is None for row in matrix for v in row)
    if unknown:
        log.info("%d unknown cells filled", unknown)
    return out


def latin1_tables(path: Path) -> list[str]:
    """The Rust source of the Latin-1 pair tables."""
    matrices = read_matrices(path)
    normal_raw = matrices["word-break=normal hyphens=manual"]
    keep_all = matrices["word-break=keep-all hyphens=manual"]
    break_all_raw = matrices["word-break=break-all hyphens=manual"]
    no_hyphens = matrices["word-break=break-all hyphens=none"]
    normal = _filled(normal_raw, keep_all)
    if _filled(keep_all, normal_raw) != normal:
        raise ValueError("keep-all differs from normal in Latin-1")
    break_all = _filled(break_all_raw, normal_raw)
    shy = SOFT_HYPHEN - 0x20
    shy_row = [bool(v) for v in no_hyphens[shy]]
    for i, row in enumerate(no_hyphens):
        if i != shy and _filled([row], [break_all[i]])[0] != break_all[i]:
            raise ValueError(f"hyphens: none changes the row of U+{i + 0x20:04X}")

    n = len(LATIN1)

    def signature(i: int) -> tuple[object, ...]:
        return (
            tuple(normal[i]),
            tuple(r[i] for r in normal),
            tuple(break_all[i]),
            tuple(r[i] for r in break_all),
            shy_row[i],
            i == shy,
        )

    groups: dict[tuple[object, ...], int] = {}
    group_of = [groups.setdefault(signature(i), len(groups)) for i in range(n)]
    count = len(groups)
    if count > 64:
        raise ValueError(f"{count} Latin-1 groups do not fit into u64")
    first = [group_of.index(g) for g in range(count)]

    def row_bits(matrix: list[list[bool]], i: int) -> int:
        return sum(1 << g for g in range(count) if matrix[i][first[g]])

    lines = [
        "/// The pair group of each code point U+0020..U+00FF (index: code point -",
        "/// 0x20). Code points of a group break alike with every other code point.",
        "#[rustfmt::skip]",
        f"pub(super) static LATIN1_GROUPS: [u8; {n}] = [",
    ]
    for start in range(0, n, 16):
        lines.append("    " + " ".join(f"{g}," for g in group_of[start : start + 16]))
    lines += ["];", ""]
    for name, doc, matrix in (
        ("LATIN1_NORMAL", "`word-break: normal` and `keep-all`", normal),
        ("LATIN1_BREAK_ALL", "`word-break: break-all`", break_all),
    ):
        lines += [
            f"/// {doc}: bit g of entry f is set if the line can break",
            "/// between a code point of group f and a following code point of group g.",
            f"pub(super) static {name}: [u64; {count}] = [",
        ]
        lines += [f"    0x{row_bits(matrix, first[g]):019_X}," for g in range(count)]
        lines += ["];", ""]
    bits = sum(1 << g for g in range(count) if shy_row[first[g]])
    lines += [
        "/// `word-break: break-all` with `hyphens: none`: the entry of U+00AD (the",
        "/// other entries are those of `LATIN1_BREAK_ALL`).",
        f"pub(super) static LATIN1_BREAK_ALL_SHY_NO_HYPHENS: u64 = 0x{bits:019_X};",
        "",
    ]
    return lines


def generate_tables() -> str:
    """Returns the Rust source of `tables.rs`."""
    lines = [
        "//! Data for line breaking. Generated by `swbtools linebreak-tables`",
        "//! (`tools/swbtools/linebreak_tables.py`) from the Unicode Character Database",
        f"//! {VERSION} (<https://www.unicode.org/Public/{VERSION}/ucd/>) and from",
        "//! measurements of Chromium (`crates/text/tests/linebreak/latin1.txt`). Do",
        "//! not edit.",
        "",
        "use super::Class;",
        "",
    ]
    lines += ucd_tables()
    lines += latin1_tables(data_dir() / "latin1.txt")
    return "\n".join(lines).rstrip("\n") + "\n"


def tables_path() -> Path:
    """Returns `crates/text/src/linebreak/tables.rs`."""
    return paths.repo_root() / "crates" / "text" / "src" / "linebreak" / "tables.rs"


def write_tables() -> int:
    """Writes `tables.rs`. Returns the exit code."""
    path = tables_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(generate_tables(), encoding="utf-8")
    print(f"wrote {path.relative_to(paths.repo_root())}")
    return 0
