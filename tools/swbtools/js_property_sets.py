"""The code point sets of the Unicode properties of regular expressions
(`\\p{...}`) for `swbtools js-unicode-tables`: writes `property_data.rs`.

The sets are the ones that ECMA-262 §22.2.2.9 (`CompileToCharSet` of
`UnicodePropertyValueExpression`) needs:

- `General_Category`: one set for each of the 30 values that are not
  groups (`DerivedGeneralCategory.txt`); the groups (`L`, `LC`, `M`, `N`,
  `P`, `S`, `Z`, `C`) are lists of those values that the code unites;
- `Script` (`Scripts.txt`) and `Script_Extensions`
  (`ScriptExtensions.txt`; a code point without an entry has the extensions
  `{Script}`), one set for each script;
- the binary properties of ECMA-262 table 66, from `PropList.txt`,
  `DerivedCoreProperties.txt`, `DerivedNormalizationProps.txt`,
  `DerivedBinaryProperties.txt` and `emoji/emoji-data.txt`. `ASCII`, `Any`
  and `Assigned` have no data: the code computes them;
- the binary properties of strings of table 67 (`emoji-sequences.txt`,
  `emoji-zwj-sequences.txt`): the single code points and the sequences of
  each. `RGI_Emoji` has no data: it is the union of the other six
  (`emoji-sequences.txt`).

Encoding. A set is a list of sorted, disjoint ranges. For each range, two
unsigned LEB128 numbers: the gap from the end of the previous range
(`first - previous_last - 1`; `first` for the first range) and the
length minus one. A sequence of code points is its length (one byte) and
then each code point as an unsigned LEB128 number. All data is in one byte
string; the offset tables give the start of each item (the end of an item
is the start of the next one).
"""

from collections.abc import Iterator
from pathlib import Path

from swbtools.js_property_names import BINARY, SPECIAL_BINARY, STRING_PROPERTIES
from swbtools.ucd import MAX_CODE_POINT, read_property

FILES = {
    "DerivedGeneralCategory": "ucd/extracted/DerivedGeneralCategory.txt",
    "Scripts": "ucd/Scripts.txt",
    "ScriptExtensions": "ucd/ScriptExtensions.txt",
    "PropList": "ucd/PropList.txt",
    "DerivedCoreProperties": "ucd/DerivedCoreProperties.txt",
    "DerivedNormalizationProps": "ucd/DerivedNormalizationProps.txt",
    "DerivedBinaryProperties": "ucd/extracted/DerivedBinaryProperties.txt",
    "emoji-data": "ucd/emoji/emoji-data.txt",
    "emoji-sequences": "emoji/emoji-sequences.txt",
    "emoji-zwj-sequences": "emoji/emoji-zwj-sequences.txt",
}
"""The UCD files of the sets, by name, with their path below
`https://www.unicode.org/Public/VERSION/`."""

BINARY_FILES = (
    "PropList",
    "DerivedCoreProperties",
    "DerivedNormalizationProps",
    "DerivedBinaryProperties",
    "emoji-data",
)
"""The files that give binary properties, as lines `range ; Name`."""

RGI_EMOJI = "RGI_Emoji"
"""The union of the other properties of strings."""

Ranges = list[tuple[int, int]]


def leb128(number: int) -> bytes:
    """An unsigned LEB128 number."""
    out = bytearray()
    while True:
        low = number & 0x7F
        number >>= 7
        if number:
            out.append(low | 0x80)
        else:
            out.append(low)
            return bytes(out)


def read_leb128(data: bytes, pos: int) -> tuple[int, int]:
    """Reads a LEB128 number: the value and the position after it."""
    value = shift = 0
    while True:
        byte = data[pos]
        pos += 1
        value |= (byte & 0x7F) << shift
        if byte < 0x80:
            return value, pos
        shift += 7


def encode_ranges(ranges: Ranges) -> bytes:
    """Encodes sorted, disjoint ranges (see the module comment)."""
    out = bytearray()
    next_free = 0
    for first, last in ranges:
        if first < next_free or last < first:
            raise ValueError(f"ranges not sorted and disjoint at {first:#x}")
        out += leb128(first - next_free) + leb128(last - first)
        next_free = last + 1
    return bytes(out)


def decode_ranges(data: bytes) -> Ranges:
    """The inverse of `encode_ranges`."""
    out: Ranges = []
    pos = next_free = 0
    while pos < len(data):
        gap, pos = read_leb128(data, pos)
        length, pos = read_leb128(data, pos)
        first = next_free + gap
        out.append((first, first + length))
        next_free = first + length + 1
    return out


def encode_sequences(sequences: list[tuple[int, ...]]) -> bytes:
    """Encodes sequences of code points: the length, then the code points."""
    out = bytearray()
    for sequence in sorted(set(sequences)):
        if not 2 <= len(sequence) < 256:
            raise ValueError(f"unexpected sequence length {sequence}")
        out.append(len(sequence))
        for cp in sequence:
            out += leb128(cp)
    return bytes(out)


def decode_sequences(data: bytes) -> list[tuple[int, ...]]:
    """The inverse of `encode_sequences`."""
    out = []
    pos = 0
    while pos < len(data):
        count = data[pos]
        pos += 1
        sequence = []
        for _ in range(count):
            cp, pos = read_leb128(data, pos)
            sequence.append(cp)
        out.append(tuple(sequence))
    return out


def merge(ranges: Ranges) -> Ranges:
    """Sorts ranges and merges overlapping and adjacent ones."""
    out: Ranges = []
    for first, last in sorted(ranges):
        if out and first <= out[-1][1] + 1:
            out[-1] = (out[-1][0], max(out[-1][1], last))
        else:
            out.append((first, last))
    return out


def _data_fields(path: Path) -> Iterator[tuple[str, list[str]]]:
    """The data lines of a UCD file: the code point field and the others."""
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.split("#", 1)[0].strip()
        if line:
            fields = [field.strip() for field in line.split(";")]
            yield fields[0], fields[1:]


def _code_points(field: str) -> tuple[int, int]:
    first, _, last = field.partition("..")
    return int(first, 16), int(last or first, 16)


def binary_ranges(path: Path) -> dict[str, Ranges]:
    """The ranges of every property in a file with lines `range ; Name`."""
    found: dict[str, Ranges] = {}
    for field, rest in _data_fields(path):
        found.setdefault(rest[0], []).append(_code_points(field))
    return {name: merge(ranges) for name, ranges in found.items()}


def value_sets(values: list[str | None], names: list[str]) -> dict[str, Ranges]:
    """The ranges of each name in `names`, from the value of each code point."""
    found: dict[str, Ranges] = {name: [] for name in names}
    start = 0
    for cp in range(1, MAX_CODE_POINT + 2):
        if cp > MAX_CODE_POINT or values[cp] != values[start]:
            value = values[start]
            if value is not None:
                found[value].append((start, cp - 1))
            start = cp
    return {name: merge(ranges) for name, ranges in found.items()}


def script_extensions(path: Path, scripts: list[str], short: dict[str, str]) -> dict[str, Ranges]:
    """The set of each script for `Script_Extensions`. `scripts` is the
    `Script` of each code point (by short name); `short` maps the names in
    `ScriptExtensions.txt` to short names."""
    listed: list[tuple[str, ...] | None] = [None] * (MAX_CODE_POINT + 1)
    for field, rest in _data_fields(path):
        first, last = _code_points(field)
        names = tuple(short.get(name, name) for name in rest[0].split())
        for cp in range(first, last + 1):
            listed[cp] = names
    sets: dict[str, Ranges] = {}
    for cp in range(MAX_CODE_POINT + 1):
        for name in listed[cp] or (scripts[cp],):
            ranges = sets.setdefault(name, [])
            if ranges and ranges[-1][1] == cp - 1:
                ranges[-1] = (ranges[-1][0], cp)
            else:
                ranges.append((cp, cp))
    return sets


def read_sequences(path: Path) -> tuple[dict[str, Ranges], dict[str, list[tuple[int, ...]]]]:
    """The single code points and the sequences of each property of
    strings in `emoji-sequences.txt` or `emoji-zwj-sequences.txt`."""
    singles: dict[str, Ranges] = {}
    sequences: dict[str, list[tuple[int, ...]]] = {}
    for field, rest in _data_fields(path):
        name = rest[0]
        if " " in field:
            sequences.setdefault(name, []).append(tuple(int(cp, 16) for cp in field.split()))
        else:
            singles.setdefault(name, []).append(_code_points(field))
    return {name: merge(ranges) for name, ranges in singles.items()}, sequences


class Blob:
    """The byte string of all data, and the offset of each item."""

    def __init__(self) -> None:
        self.data = bytearray()

    def add(self, items: list[bytes]) -> list[int]:
        """Appends items; returns their offsets and then the end offset."""
        offsets = []
        for item in items:
            offsets.append(len(self.data))
            self.data += item
        offsets.append(len(self.data))
        return offsets


def group_members(names: list[str]) -> list[list[int]]:
    """For each `General_Category` value, the indices of the values that
    are not groups and that it unites (itself for a value that is not a
    group). `LC` is `Lu`, `Ll` and `Lt`; a one-letter value is every value
    that starts with the letter."""
    members = []
    for name in names:
        if name == "LC":
            wanted = ["Lu", "Ll", "Lt"]
        elif len(name) == 1:
            wanted = [n for n in names if len(n) == 2 and n[0] == name and n != "LC"]
        else:
            wanted = [name]
        members.append([names.index(n) for n in wanted])
    return members


def _rust_bytes(name: str, doc: str, data: bytes) -> list[str]:
    lines = [f"/// {doc}", f'pub(super) static {name}: &[u8] = b"\\']
    for i in range(0, len(data), 24):
        chunk = "".join(f"\\x{b:02X}" for b in data[i : i + 24])
        lines.append(f"    {chunk}\\")
    lines += ['    ";', ""]
    return lines


def _rust_offsets(name: str, doc: str, offsets: list[int]) -> list[str]:
    lines = [
        f"/// {doc}",
        "#[rustfmt::skip]",
        f"pub(super) static {name}: [u32; {len(offsets)}] = [",
    ]
    for i in range(0, len(offsets), 12):
        lines.append("    " + ", ".join(str(o) for o in offsets[i : i + 12]) + ",")
    return [*lines, "];", ""]


def generate_property_data(
    files: dict[str, Path],
    categories: list[str],
    scripts: list[list[str]],
    version: str,
) -> str:
    """Returns the Rust source of `property_data.rs`.

    `files` maps the keys of `FILES` to local files. `categories` is the
    list of short names of the `General_Category` values and `scripts` the
    rows of the `sc` lines of `PropertyValueAliases.txt` (short name first,
    in the order of `property_names.rs`)."""
    members = group_members(categories)
    gc_values = read_property(files["DerivedGeneralCategory"], "Cn")
    gc_sets = value_sets(list(gc_values), categories)
    script_short = [row[0] for row in scripts]
    to_short = {alias: row[0] for row in scripts for alias in row}
    sc_values = [to_short[v] for v in read_property(files["Scripts"], "Unknown")]
    sc_sets = value_sets(list(sc_values), script_short)
    scx_sets = script_extensions(files["ScriptExtensions"], sc_values, to_short)
    binary: dict[str, Ranges] = {}
    for key in BINARY_FILES:
        for name, ranges in binary_ranges(files[key]).items():
            binary.setdefault(name, ranges)
    for name in BINARY:
        if name not in SPECIAL_BINARY and name not in binary:
            raise ValueError(f"no data for the binary property {name}")
    emoji_singles, emoji_sequences = read_sequences(files["emoji-sequences"])
    _, zwj_sequences = read_sequences(files["emoji-zwj-sequences"])
    emoji_sequences.update(zwj_sequences)
    blob = Blob()
    tables = {
        "GENERAL_CATEGORY": blob.add(
            [
                encode_ranges(gc_sets[name]) if members[i] == [i] else b""
                for i, name in enumerate(categories)
            ]
        ),
        "SCRIPT": blob.add([encode_ranges(sc_sets[name]) for name in script_short]),
        "SCRIPT_EXTENSIONS": blob.add([encode_ranges(scx_sets.get(n, [])) for n in script_short]),
        "BINARY": blob.add(
            [b"" if name in SPECIAL_BINARY else encode_ranges(binary[name]) for name in BINARY]
        ),
        "STRING_CODE_POINTS": blob.add(
            [encode_ranges(emoji_singles.get(name, [])) for name in STRING_PROPERTIES]
        ),
        "STRING_SEQUENCES": blob.add(
            [encode_sequences(emoji_sequences.get(name, [])) for name in STRING_PROPERTIES]
        ),
    }
    for name in STRING_PROPERTIES:
        if name != RGI_EMOJI and not (emoji_singles.get(name) or emoji_sequences.get(name)):
            raise ValueError(f"no data for the property of strings {name}")
    lines = [
        "//! The code point sets of the Unicode properties of regular expressions.",
        "//! Generated by `swbtools js-unicode-tables`",
        "//! (`tools/swbtools/js_property_sets.py`) from the Unicode Character Database",
        f"//! {version} (<https://www.unicode.org/Public/{version}/>):",
        *[f"//! - `{Path(p).name}`" for p in FILES.values()],
        "//!",
        "//! Do not edit. The encoding is in the generator.",
        "",
    ]
    lines += _rust_bytes("DATA", "All sets and sequences, encoded.", bytes(blob.data))
    docs = {
        "GENERAL_CATEGORY": "The sets of the `General_Category` values that are not groups",
        "SCRIPT": "The sets of the `Script` values",
        "SCRIPT_EXTENSIONS": "The sets of the `Script_Extensions` values",
        "BINARY": "The sets of the binary properties (empty for `ASCII`, `Any`, `Assigned`)",
        "STRING_CODE_POINTS": "The single code points of the properties of strings",
        "STRING_SEQUENCES": "The sequences of the properties of strings",
    }
    for key, offsets in tables.items():
        lines += _rust_offsets(
            f"{key}_OFFSETS", f"{docs[key]}: offsets in [`DATA`], by index.", offsets
        )
    rows = [", ".join(str(m) for m in row) for row in members]
    lines += [
        "/// For each `General_Category` value, the indices of the values that are not",
        "/// groups and that it unites.",
        "#[rustfmt::skip]",
        f"pub(super) static GENERAL_CATEGORY_MEMBERS: [&[u8]; {len(members)}] = [",
        *[f"    &[{row}]," for row in rows],
        "];",
    ]
    return "\n".join(lines) + "\n"
