"""Normalization tables for the JavaScript engine (`swbtools js-unicode-tables`).

Writes `crates/js-text/src/unicode/norm_tables.rs` from the UCD
(UAX #15, Unicode Normalization Forms):

- `CCC`: Canonical_Combining_Class of the non-starters, as ranges
  `(first, last, class)`.
- `CANON_INDEX`, `CANON_DATA`: the full canonical decomposition (applied
  recursively) of every code point with a canonical decomposition mapping,
  except the Hangul syllables (algorithmic). `CANON_INDEX` is sorted by
  code point: `(code point, offset in CANON_DATA, length)`.
- `COMPAT_INDEX`, `COMPAT_DATA`: the same for the full compatibility
  decomposition of the code points where it differs from the canonical one
  (the code points with a compatibility mapping, and those whose
  canonical decomposition has characters with one).
- `COMPOSE`: the primary composites `(first, second, composite)`, sorted:
  the canonical decompositions of two code points of the characters that
  are not in Full_Composition_Exclusion.
- `NFD_NO`, `NFC_NO`, `NFKD_NO`, `NFKC_NO`, `NFC_MAYBE`: the code points
  with NFD_QC=No, NFC_QC=No, NFKD_QC=No, NFKC_QC=No and NFC_QC=Maybe
  (`DerivedNormalizationProps.txt`; NFKC_QC=Maybe is the same set). The
  quick check of UAX #15 section 9.1 uses them.
"""

from pathlib import Path

from swbtools.ucd import range_table

HANGUL_FIRST = 0xAC00
HANGUL_LAST = 0xD7A3


def parse_unicode_data(
    path: Path,
) -> tuple[dict[int, int], dict[int, list[int]], dict[int, list[int]]]:
    """Canonical combining classes, canonical and compatibility mappings
    (one step; the compatibility ones include the tag)."""
    ccc: dict[int, int] = {}
    canonical: dict[int, list[int]] = {}
    compat: dict[int, list[int]] = {}
    for raw in path.read_text(encoding="utf-8").splitlines():
        fields = raw.split(";")
        if len(fields) < 15:
            continue
        cp = int(fields[0], 16)
        if int(fields[3]):
            ccc[cp] = int(fields[3])
        decomposition = fields[5]
        if not decomposition:
            continue
        if decomposition.startswith("<"):
            compat[cp] = [int(x, 16) for x in decomposition.split()[1:]]
        else:
            canonical[cp] = [int(x, 16) for x in decomposition.split()]
    return ccc, canonical, compat


def parse_props(path: Path) -> tuple[set[int], dict[str, set[int]]]:
    """The Full_Composition_Exclusion set and the quick check values of
    `DerivedNormalizationProps.txt`: `{"NFC_QC=N": {...}, ...}`."""
    exclusion: set[int] = set()
    quick: dict[str, set[int]] = {}
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        fields = [field.strip() for field in line.split(";")]
        first_text, _, last_text = fields[0].partition("..")
        first = int(first_text, 16)
        last = int(last_text, 16) if last_text else first
        cps = range(first, last + 1)
        if fields[1] == "Full_Composition_Exclusion":
            exclusion.update(cps)
        elif fields[1] in ("NFC_QC", "NFKC_QC", "NFD_QC", "NFKD_QC"):
            quick.setdefault(f"{fields[1]}={fields[2]}", set()).update(cps)
    return exclusion, quick


def full_decompositions(
    canonical: dict[int, list[int]], compat: dict[int, list[int]]
) -> tuple[dict[int, list[int]], dict[int, list[int]]]:
    """The recursive canonical decomposition of each code point with a
    canonical mapping, and the recursive compatibility decomposition of
    each code point where it differs."""

    def expand(cp: int, mappings: list[dict[int, list[int]]]) -> list[int]:
        out: list[int] = []
        stack = [cp]
        while stack:
            current = stack.pop()
            for mapping in mappings:
                if current in mapping:
                    stack.extend(reversed(mapping[current]))
                    break
            else:
                out.append(current)
        return out

    full_canon = {cp: expand(cp, [canonical]) for cp in canonical}
    full_compat: dict[int, list[int]] = {}
    for cp in set(canonical) | set(compat):
        both = expand(cp, [canonical, compat])
        if both != full_canon.get(cp):
            full_compat[cp] = both
    return full_canon, full_compat


def pool_table(name: str, doc: str, decompositions: dict[int, list[int]]) -> tuple[list[str], int]:
    """The index and data tables of decompositions. Returns the lines and
    the length of the longest."""
    data: list[int] = []
    index: list[tuple[int, int, int]] = []
    for cp in sorted(decompositions):
        mapped = decompositions[cp]
        index.append((cp, len(data), len(mapped)))
        data.extend(mapped)
    assert len(data) < 65536
    longest = max(length for _, _, length in index)
    lines = [
        f"/// {doc} Entries `(code point, offset in {name}_DATA, length)`.",
        f"pub(super) static {name}_INDEX: [(u32, u16, u8); {len(index)}] = [",
    ]
    lines += [f"    (0x{cp:04X}, {offset}, {length})," for cp, offset, length in index]
    lines += ["];", ""]
    lines += [
        f"/// The decompositions of `{name}_INDEX`.",
        f"pub(super) static {name}_DATA: [u32; {len(data)}] = [",
    ]
    for start in range(0, len(data), 8):
        lines.append("    " + " ".join(f"0x{x:04X}," for x in data[start : start + 8]))
    lines += ["];", ""]
    return lines, longest


def ccc_ranges(ccc: dict[int, int]) -> list[tuple[int, int, int]]:
    """Ranges of equal nonzero combining class."""
    out: list[tuple[int, int, int]] = []
    for cp in sorted(ccc):
        if out and out[-1][1] == cp - 1 and out[-1][2] == ccc[cp]:
            out[-1] = (out[-1][0], cp, ccc[cp])
        else:
            out.append((cp, cp, ccc[cp]))
    return out


def cp_ranges(values: set[int]) -> list[tuple[int, int]]:
    """Ranges of a set of code points."""
    out: list[tuple[int, int]] = []
    for cp in sorted(values):
        if out and out[-1][1] == cp - 1:
            out[-1] = (out[-1][0], cp)
        else:
            out.append((cp, cp))
    return out


def is_hangul(cp: int) -> bool:
    """Whether `cp` is a precomposed Hangul syllable."""
    return HANGUL_FIRST <= cp <= HANGUL_LAST


def generate_norm_tables(unicode_data: Path, props: Path, version: str) -> str:
    """Returns the Rust source of `norm_tables.rs`."""
    ccc, canonical, compat = parse_unicode_data(unicode_data)
    exclusion, quick = parse_props(props)
    compose: dict[tuple[int, int], int] = {}
    for cp, mapped in canonical.items():
        if len(mapped) == 2 and cp not in exclusion and not is_hangul(cp):
            compose[(mapped[0], mapped[1])] = cp
    full_canon, full_compat = full_decompositions(canonical, compat)
    assert quick["NFC_QC=M"] == quick["NFKC_QC=M"]
    lines = [
        "//! Normalization data. Generated by `swbtools js-unicode-tables`",
        "//! (`tools/swbtools/js_norm_tables.py`) from the Unicode Character Database",
        f"//! {version} (<https://www.unicode.org/Public/{version}/ucd/>): `UnicodeData.txt`",
        "//! and `DerivedNormalizationProps.txt`. Do not edit.",
        "",
    ]
    classes = ccc_ranges(ccc)
    lines += [
        "/// `Canonical_Combining_Class` of the non-starters: `(first, last, class)`.",
        f"pub(super) static CCC: [(u32, u32, u8); {len(classes)}] = [",
    ]
    lines += [f"    (0x{a:04X}, 0x{b:04X}, {c})," for a, b, c in classes]
    lines += ["];", ""]
    canon_lines, canon_longest = pool_table(
        "CANON", "The full canonical decompositions.", full_canon
    )
    compat_lines, compat_longest = pool_table(
        "COMPAT",
        "The full compatibility decompositions that differ from the canonical ones.",
        full_compat,
    )
    lines += canon_lines + compat_lines
    lines += [
        "/// The longest decomposition (U+FDFA in NFKD).",
        f"pub(super) const MAX_DECOMPOSITION: usize = {max(canon_longest, compat_longest)};",
        "",
        "/// The primary composites `(first, second, composite)`, sorted.",
        f"pub(super) static COMPOSE: [(u32, u32, u32); {len(compose)}] = [",
    ]
    lines += [f"    (0x{a:04X}, 0x{b:04X}, 0x{c:04X})," for (a, b), c in sorted(compose.items())]
    lines += ["];", ""]
    for name, key, doc in (
        ("NFD_NO", "NFD_QC=N", "`NFD_QC=No`."),
        ("NFC_NO", "NFC_QC=N", "`NFC_QC=No`."),
        ("NFKD_NO", "NFKD_QC=N", "`NFKD_QC=No`."),
        ("NFKC_NO", "NFKC_QC=N", "`NFKC_QC=No`."),
        ("NFC_MAYBE", "NFC_QC=M", "`NFC_QC=Maybe` (and `NFKC_QC=Maybe`)."),
    ):
        lines += range_table(name, doc, cp_ranges(quick[key]))
    return "\n".join(lines).rstrip("\n") + "\n"
