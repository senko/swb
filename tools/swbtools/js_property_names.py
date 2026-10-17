"""Names of the Unicode properties of regular expressions (`\\p{...}`) for
`swbtools js-unicode-tables`: writes `property_names.rs`.

ECMA-262 §22.2.2.9.7 and §22.2.2.9.8 (UnicodeMatchProperty,
UnicodeMatchPropertyValue) accept:

- the non-binary properties General_Category, Script and Script_Extensions
  with their aliases (Table 65), and as values the values and aliases of
  `PropertyValueAliases.txt` (`gc` and `sc` lines; Script_Extensions takes
  the values of Script);
- the binary properties of Table 66 with their aliases;
- the properties of strings of Table 67 (only with the `v` flag).

The lists of properties come from the specification (below); the aliases
come from `PropertyAliases.txt`. `PropertyAliases.txt` 17.0.0 also gives
`WSpace` for White_Space, which Table 66 leaves out; V8 (Chromium 148)
accepts it, and so does swb. Names match exactly (case, `_` and `-`
matter; no `Is` prefix).

The tables map each accepted name to the index of its canonical name, so
that the code point sets (`js_property_sets.py`) use the same indices.
"""

from pathlib import Path

NON_BINARY = {
    "General_Category": "gc",
    "Script": "sc",
    "Script_Extensions": "scx",
}
"""Table 65: the canonical names and their alias."""

BINARY = [
    "ASCII", "ASCII_Hex_Digit", "Alphabetic", "Any", "Assigned", "Bidi_Control",
    "Bidi_Mirrored", "Case_Ignorable", "Cased", "Changes_When_Casefolded",
    "Changes_When_Casemapped", "Changes_When_Lowercased", "Changes_When_NFKC_Casefolded",
    "Changes_When_Titlecased", "Changes_When_Uppercased", "Dash",
    "Default_Ignorable_Code_Point", "Deprecated", "Diacritic", "Emoji", "Emoji_Component",
    "Emoji_Modifier", "Emoji_Modifier_Base", "Emoji_Presentation", "Extended_Pictographic",
    "Extender", "Grapheme_Base", "Grapheme_Extend", "Hex_Digit", "IDS_Binary_Operator",
    "IDS_Trinary_Operator", "ID_Continue", "ID_Start", "Ideographic", "Join_Control",
    "Logical_Order_Exception", "Lowercase", "Math", "Noncharacter_Code_Point",
    "Pattern_Syntax", "Pattern_White_Space", "Quotation_Mark", "Radical",
    "Regional_Indicator", "Sentence_Terminal", "Soft_Dotted", "Terminal_Punctuation",
    "Unified_Ideograph", "Uppercase", "Variation_Selector", "White_Space", "XID_Continue",
    "XID_Start",
]  # fmt: skip
"""Table 66: the canonical names of the binary properties."""

SPECIAL_BINARY = {"ASCII", "Any", "Assigned"}
"""Binary properties of Table 66 that `PropertyAliases.txt` does not list."""

STRING_PROPERTIES = [
    "Basic_Emoji", "Emoji_Keycap_Sequence", "RGI_Emoji_Modifier_Sequence",
    "RGI_Emoji_Flag_Sequence", "RGI_Emoji_Tag_Sequence", "RGI_Emoji_ZWJ_Sequence", "RGI_Emoji",
]  # fmt: skip
"""Table 67: the binary properties of strings."""


def _fields(path: Path) -> list[list[str]]:
    """The fields of the data lines of an alias file, without comments."""
    rows = []
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.split("#", 1)[0].strip()
        if line:
            rows.append([field.strip() for field in line.split(";")])
    return rows


def property_aliases(path: Path) -> dict[str, list[str]]:
    """The names of each property of `PropertyAliases.txt`, by long name:
    the long name, the short name and any other aliases."""
    aliases = {}
    for fields in _fields(path):
        short, long, *others = fields
        aliases[long] = [long, short, *others]
    return aliases


def value_aliases(path: Path, prop: str) -> list[list[str]]:
    """The values of one property of `PropertyValueAliases.txt` (by its
    short name): each with its short name first, then the long name and
    any other aliases, in file order."""
    return [fields[1:] for fields in _fields(path) if fields[0] == prop]


def _name_table(name: str, doc: str, entries: dict[str, int], index_type: str) -> list[str]:
    items = sorted(entries.items())
    lines = [f"/// {doc}", f"pub(super) static {name}: [(&str, {index_type}); {len(items)}] = ["]
    lines += [f'    ("{key}", {index}),' for key, index in items]
    return [*lines, "];", ""]


def _list(name: str, doc: str, names: list[str]) -> list[str]:
    lines = [f"/// {doc}", f"pub(super) static {name}: [&str; {len(names)}] = ["]
    lines += [f'    "{n}",' for n in names]
    return [*lines, "];", ""]


def _values(rows: list[list[str]]) -> tuple[list[str], dict[str, int]]:
    """The canonical (short) names and the map from every name to its index."""
    canonical = [row[0] for row in rows]
    names: dict[str, int] = {}
    for index, row in enumerate(rows):
        for alias in row:
            if alias in names and names[alias] != index:
                raise ValueError(f"value alias {alias} names two values")
            names[alias] = index
    return canonical, names


def generate_property_names(aliases_path: Path, values_path: Path, version: str) -> str:
    """Returns the Rust source of `property_names.rs`."""
    aliases = property_aliases(aliases_path)
    for long, short in NON_BINARY.items():
        if aliases.get(long, [None, None])[1] != short:
            raise ValueError(f"PropertyAliases.txt does not give {short} for {long}")
    binary: dict[str, int] = {}
    for index, long in enumerate(BINARY):
        names = [long] if long in SPECIAL_BINARY else aliases.get(long)
        if names is None:
            raise ValueError(f"PropertyAliases.txt has no binary property {long}")
        for alias in names:
            binary[alias] = index
    gc_names, gc = _values(value_aliases(values_path, "gc"))
    sc_names, sc = _values(value_aliases(values_path, "sc"))
    lines = [
        "//! Names of the Unicode properties of regular expressions. Generated by",
        "//! `swbtools js-unicode-tables` (`tools/swbtools/js_property_names.py`) from",
        f"//! the Unicode Character Database {version}",
        f"//! (<https://www.unicode.org/Public/{version}/ucd/>): `PropertyAliases.txt` and",
        "//! `PropertyValueAliases.txt`, for the properties of ECMA-262 tables 65 to 67.",
        "//! Do not edit.",
        "",
    ]
    lines += _list(
        "GENERAL_CATEGORIES", "The short names of the `General_Category` values.", gc_names
    )
    lines += _name_table(
        "GENERAL_CATEGORY_NAMES",
        "Every name and alias of a `General_Category` value, with the index of its value.",
        gc,
        "u8",
    )
    lines += _list("SCRIPTS", "The short names of the `Script` values.", sc_names)
    lines += _name_table(
        "SCRIPT_NAMES",
        "Every name and alias of a `Script` value, with the index of its value.",
        sc,
        "u16",
    )
    lines += _list("BINARY_PROPERTIES", "The binary properties of ECMA-262 table 66.", BINARY)
    lines += _name_table(
        "BINARY_PROPERTY_NAMES",
        "Every name and alias of a binary property, with its index.",
        binary,
        "u8",
    )
    lines += _list(
        "STRING_PROPERTIES",
        "The binary properties of strings of ECMA-262 table 67 (only with the `v` flag).",
        STRING_PROPERTIES,
    )
    return "\n".join(lines).rstrip("\n") + "\n"
