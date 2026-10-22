"""Case mapping tables for the JavaScript engine (`swbtools js-unicode-tables`).

Writes `crates/js-text/src/unicode/case_tables.rs` from the UCD:

- `LOWER_RUNS`, `UPPER_RUNS`: the one-to-one mappings of `UnicodeData.txt`
  (the simple mappings, fields 13 and 12) as runs `(first, last, step,
  delta)`: every `first + k * step` up to `last` maps to itself plus `delta`.
- `LOWER_SPECIAL`, `UPPER_SPECIAL`: the unconditional mappings of
  `SpecialCasing.txt` (the full mappings that differ from the simple ones;
  up to three code points, padded with 0). They take precedence over the
  runs. The conditional mappings are language-sensitive or `Final_Sigma`
  (done in the code).
- `FOLD_RUNS`: the simple case folding (`CaseFolding.txt`, status C and S),
  for `localeCompare`.
- `CASED`, `CASE_IGNORABLE`: the properties of the `Final_Sigma` context
  (Unicode Standard §3.13, Table 3-17).

Default Case Conversion: Unicode Standard 17.0 chapter 3.13.
"""

from pathlib import Path

from swbtools.ucd import has_property, range_table, ranges, rust_static, unicode_data_rows

Runs = list[tuple[int, int, int, int]]


def parse_unicode_data(path: Path) -> tuple[dict[int, int], dict[int, int]]:
    """The simple lowercase and uppercase mappings of `UnicodeData.txt`."""
    lower: dict[int, int] = {}
    upper: dict[int, int] = {}
    for fields in unicode_data_rows(path):
        cp = int(fields[0], 16)
        if fields[12]:
            upper[cp] = int(fields[12], 16)
        if fields[13]:
            lower[cp] = int(fields[13], 16)
    return lower, upper


def parse_special_casing(path: Path) -> tuple[dict[int, list[int]], dict[int, list[int]]]:
    """The unconditional lowercase and uppercase mappings of `SpecialCasing.txt`."""
    lower: dict[int, list[int]] = {}
    upper: dict[int, list[int]] = {}
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        fields = [field.strip() for field in line.split(";")]
        # code; lower; title; upper; [condition;]
        if len(fields) > 5 and fields[4]:
            continue
        cp = int(fields[0], 16)
        lower[cp] = [int(x, 16) for x in fields[1].split()]
        upper[cp] = [int(x, 16) for x in fields[3].split()]
    return lower, upper


def parse_simple_folding(path: Path) -> dict[int, int]:
    """The simple case folding of `CaseFolding.txt` (status C and S)."""
    fold: dict[int, int] = {}
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        fields = [field.strip() for field in line.split(";")]
        if fields[1] in ("C", "S"):
            fold[int(fields[0], 16)] = int(fields[2], 16)
    return fold


def make_runs(mapping: dict[int, int]) -> Runs:
    """Groups a mapping into runs of equal delta and step 1 or 2."""
    runs: Runs = []
    for cp in sorted(mapping):
        delta = mapping[cp] - cp
        if runs:
            first, last, step, run_delta = runs[-1]
            gap = cp - last
            if run_delta == delta and (
                (first == last and gap in (1, 2) and gap > 0) or (first != last and gap == step)
            ):
                runs[-1] = (first, cp, gap if first == last else step, delta)
                continue
        runs.append((cp, cp, 1, delta))
    return runs


def expand_runs(runs: Runs) -> dict[int, int]:
    """The mapping that the runs stand for (used by the self-check)."""
    mapping: dict[int, int] = {}
    for first, last, step, delta in runs:
        for cp in range(first, last + 1, step):
            mapping[cp] = cp + delta
    return mapping


def runs_table(name: str, doc: str, runs: Runs) -> list[str]:
    """The Rust source of a table of runs."""
    rows = [f"    (0x{a:04X}, 0x{b:04X}, {s}, {d})," for a, b, s, d in runs]
    return rust_static(name, doc, f"[(u32, u32, u8, i32); {len(runs)}]", rows)


def special_table(name: str, doc: str, special: dict[int, list[int]]) -> list[str]:
    """The Rust source of a table of full mappings."""
    rows = []
    for cp in sorted(special):
        mapped = special[cp] + [0] * (3 - len(special[cp]))
        rows.append(f"    (0x{cp:04X}, [{', '.join(f'0x{x:04X}' for x in mapped)}]),")
    return rust_static(name, doc, f"[(u32, [u32; 3]); {len(special)}]", rows)


def generate_case_tables(
    unicode_data: Path,
    special_casing: Path,
    case_folding: Path,
    core: Path,
    version: str,
) -> str:
    """Returns the Rust source of `case_tables.rs`."""
    simple_lower, simple_upper = parse_unicode_data(unicode_data)
    special_lower, special_upper = parse_special_casing(special_casing)
    # A special mapping of one code point that equals the simple mapping
    # stays in the runs.
    for special, simple in ((special_lower, simple_lower), (special_upper, simple_upper)):
        for cp in list(special):
            if special[cp] == [simple.get(cp, cp)]:
                del special[cp]
    lower_runs = make_runs({cp: m for cp, m in simple_lower.items() if cp not in special_lower})
    upper_runs = make_runs({cp: m for cp, m in simple_upper.items() if cp not in special_upper})
    fold = parse_simple_folding(case_folding)
    fold_runs = make_runs(fold)
    assert expand_runs(fold_runs) == fold
    assert expand_runs(lower_runs) == {
        cp: m for cp, m in simple_lower.items() if cp not in special_lower
    }
    assert expand_runs(upper_runs) == {
        cp: m for cp, m in simple_upper.items() if cp not in special_upper
    }
    lines = [
        "//! Case mapping data. Generated by `swbtools js-unicode-tables`",
        "//! (`tools/swbtools/js_case_tables.py`) from the Unicode Character Database",
        f"//! {version} (<https://www.unicode.org/Public/{version}/ucd/>): `UnicodeData.txt`,",
        "//! `SpecialCasing.txt`, `CaseFolding.txt` and `DerivedCoreProperties.txt`.",
        "//! Do not edit.",
        "",
    ]
    lines += runs_table(
        "LOWER_RUNS",
        "Simple lowercase mappings as runs `(first, last, step, delta)`.",
        lower_runs,
    )
    lines += runs_table(
        "UPPER_RUNS",
        "Simple uppercase mappings as runs `(first, last, step, delta)`.",
        upper_runs,
    )
    lines += special_table(
        "LOWER_SPECIAL",
        "Unconditional full lowercase mappings that are not simple ones.",
        special_lower,
    )
    lines += special_table(
        "UPPER_SPECIAL",
        "Unconditional full uppercase mappings that are not simple ones.",
        special_upper,
    )
    lines += runs_table(
        "FOLD_RUNS",
        "Simple case folding (status C and S) as runs `(first, last, step, delta)`.",
        fold_runs,
    )
    lines += range_table(
        "CASED",
        "`Cased` (Unicode Standard §3.13, `Final_Sigma`).",
        ranges(has_property(core, "Cased")),
    )
    lines += range_table(
        "CASE_IGNORABLE",
        "`Case_Ignorable` (Unicode Standard §3.13, `Final_Sigma`).",
        ranges(has_property(core, "Case_Ignorable")),
    )
    return "\n".join(lines).rstrip("\n") + "\n"
