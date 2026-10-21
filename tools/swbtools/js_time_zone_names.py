"""`swbtools js-time-zone-names`: the English time zone names of `Date`.

Writes `crates/js/src/date/zone_names_data.rs` from three files of the
Unicode CLDR (`common/main/en.xml`, `common/supplemental/metaZones.xml`,
`common/bcp47/timezone.xml`; the Unicode License v3), which this module
downloads from unicode.org into `out/cldr/VERSION/` as the
`cldr-common-VERSION.zip` archive, unless they are there already.

The table follows UTS #35 part 4, "Using Time Zone Names" and "Metazones".
Chromium prints the name that ICU gives for the zone at the time of the
call, not at the date that `toString` prints (measured: the name of
Asia/Almaty in 2005 is the current "Kazakhstan Time", not the "East
Kazakhstan Time" of its metazone then), so the table has one metazone per
zone: the one in force now, which is the last interval of `metaZones.xml`
(CLDR has no future intervals). A zone whose last interval has an end has
none.

- `METAZONES`: for each metazone, its long standard and daylight name
  (an empty text if CLDR has none).
- `ZONE_OVERRIDES`: the zones that have their own long names
  (`<zone type=...><long>` in `en.xml`).
- `ZONE_TEXT`, `ZONE_ENDS`, `ZONE_METAZONE`, `ZONE_OVERRIDE_OF`: every
  zone name (the canonical identifiers of CLDR and their aliases from
  `bcp47/timezone.xml`), sorted, and for each its metazone (index + 1, 0
  for none) and its override (index + 1, 0 for none).
"""

import io
import logging
import urllib.request
import xml.etree.ElementTree as ET
import zipfile
from calendar import timegm
from pathlib import Path
from time import strptime

from swbtools import paths

log = logging.getLogger(__name__)

VERSION = "48"
"""The CLDR version of the table (the version of the data of Chromium 148,
found by probes: `Africa_Western` has the single name "West Africa Time",
Hovd is "Khovd", Turkey is "Türkiye")."""

FILES = (
    "common/main/en.xml",
    "common/supplemental/metaZones.xml",
    "common/bcp47/timezone.xml",
)

NO_END = 2**31 - 1
"""`until` of the last interval of a list."""


def output_path() -> Path:
    """Returns `crates/js/src/date/zone_names_data.rs`."""
    return paths.repo_root() / "crates" / "js" / "src" / "date" / "zone_names_data.rs"


def cldr_files(version: str) -> dict[str, Path]:
    """Returns the local copies of the CLDR files; downloads the archive
    `cldr-common-VERSION.zip` and extracts them if one is missing."""
    directory = paths.out_dir() / "cldr" / version
    wanted = {name: directory / name for name in FILES}
    if all(path.is_file() for path in wanted.values()):
        return wanted
    url = f"https://unicode.org/Public/cldr/{version}/cldr-common-{version}.zip"
    log.info("downloading %s", url)
    with urllib.request.urlopen(url, timeout=300) as response:
        archive = zipfile.ZipFile(io.BytesIO(response.read()))
    for name, path in wanted.items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(archive.read(name))
    return wanted


def minutes(text: str | None) -> int:
    """The minutes since 1970 of a time such as `1984-03-16 00:00` (UTC)."""
    if text is None:
        return NO_END
    return timegm(strptime(text, "%Y-%m-%d %H:%M")) // 60


def read_names(path: Path) -> tuple[dict[str, tuple[str, str]], dict[str, tuple[str, str]]]:
    """The long standard and daylight names of the metazones and of the
    zones of `en.xml` (an empty text for a name that is missing)."""
    names = ET.parse(path).getroot().find("dates/timeZoneNames")
    assert names is not None
    result: dict[str, dict[str, tuple[str, str]]] = {"metazone": {}, "zone": {}}
    for kind in result:
        for element in names.findall(kind):
            long = element.find("long")
            if long is None:
                continue
            standard = long.findtext("standard") or ""
            daylight = long.findtext("daylight") or ""
            if standard or daylight:
                result[kind][element.attrib["type"]] = (standard, daylight)
    return result["metazone"], result["zone"]


def read_intervals(path: Path) -> dict[str, list[tuple[int, int, str]]]:
    """For each zone of `metaZones.xml`, its metazone intervals as
    `(from, to, metazone)` in minutes, in the order of the file."""
    root = ET.parse(path).getroot().find("metaZones/metazoneInfo")
    assert root is not None
    zones: dict[str, list[tuple[int, int, str]]] = {}
    for zone in root.findall("timezone"):
        zones[zone.attrib["type"]] = [
            (
                minutes(use.get("from")) if use.get("from") else -NO_END,
                minutes(use.get("to")),
                use.attrib["mzone"],
            )
            for use in zone.findall("usesMetazone")
        ]
    return zones


def read_aliases(path: Path) -> dict[str, str]:
    """Each alias of `bcp47/timezone.xml` with its canonical zone: the
    first name of the `alias` attribute."""
    root = ET.parse(path).getroot()
    aliases: dict[str, str] = {}
    for kind in root.iter("type"):
        names = kind.get("alias", "").split()
        for name in names[1:]:
            aliases[name] = names[0]
    return aliases


def current_metazone(intervals: list[tuple[int, int, str]]) -> str | None:
    """The metazone in force now: that of the last interval, if it has no
    end."""
    if not intervals:
        return None
    _, end, metazone = max(intervals)
    return metazone if end == NO_END else None


def rust_string(text: str) -> str:
    """A Rust string literal."""
    return '"' + text.replace("\\", "\\\\").replace('"', '\\"') + '"'


def generate(files: dict[str, Path], version: str = VERSION) -> str:
    """Returns the Rust source of `zone_names_data.rs`."""
    metazone_names, zone_names = read_names(files["common/main/en.xml"])
    intervals = read_intervals(files["common/supplemental/metaZones.xml"])
    aliases = read_aliases(files["common/bcp47/timezone.xml"])

    metazones = sorted(metazone_names)
    index = {name: i for i, name in enumerate(metazones)}
    # A metazone without a long name has the index of an empty entry.
    for zone_intervals in intervals.values():
        for _, _, metazone in zone_intervals:
            if metazone not in index:
                index[metazone] = len(metazones)
                metazones.append(metazone)
                metazone_names[metazone] = ("", "")

    overrides = sorted(zone_names)
    metazone_of: dict[str, int] = {}
    override_of: dict[str, int] = {}
    for zone, zone_intervals in intervals.items():
        current = current_metazone(zone_intervals)
        metazone_of[zone] = index[current] + 1 if current else 0
    for zone in overrides:
        override_of[zone] = overrides.index(zone) + 1
        metazone_of.setdefault(zone, 0)
    for alias, canonical in aliases.items():
        if canonical in metazone_of and alias not in metazone_of:
            metazone_of[alias] = metazone_of[canonical]
            if canonical in override_of:
                override_of[alias] = override_of[canonical]

    names = sorted(metazone_of)
    text = "\n".join(names)
    ends: list[int] = []
    position = 0
    for name in names:
        position += len(name.encode()) + 1
        ends.append(position)
    # The offsets are 16 bits, the indices 8 bits.
    assert position < 2**16 and len(metazones) < 256 and len(overrides) < 256

    metazone_array = ints([metazone_of[n] for n in names])
    override_array = ints([override_of.get(n, 0) for n in names])
    out = [
        "//! The English time zone names of CLDR " + version + ", generated by",
        "//! `swbtools js-time-zone-names` (`tools/swbtools/js_time_zone_names.py`)",
        "//! from `common/main/en.xml`, `common/supplemental/metaZones.xml` and",
        "//! `common/bcp47/timezone.xml` of",
        f"//! <https://unicode.org/Public/cldr/{version}/cldr-common-{version}.zip>.",
        "//! Do not edit. Unicode License v3 (`THIRD_PARTY_NOTICES.md`).",
        "",
        "/// The long standard and daylight name of each metazone (an empty text",
        "/// for a name that CLDR does not have).",
        f"pub(super) static METAZONES: [(&str, &str); {len(metazones)}] = [",
    ]
    out += [
        f"    ({rust_string(metazone_names[m][0])}, {rust_string(metazone_names[m][1])}),"
        for m in metazones
    ]
    out += ["];", "", "/// The zones with names of their own: standard and daylight name."]
    out += [f"pub(super) static ZONE_OVERRIDES: [(&str, &str); {len(overrides)}] = ["]
    out += [
        f"    ({rust_string(zone_names[z][0])}, {rust_string(zone_names[z][1])}),"
        for z in overrides
    ]
    out += [
        "];",
        "",
        "/// The zone names (identifiers of CLDR and their aliases), sorted, one",
        "/// per line.",
        f"pub(super) static ZONE_TEXT: &str = {rust_string(text)};",
        "",
        "/// The end of each name of `ZONE_TEXT` (after its line feed).",
        f"pub(super) static ZONE_ENDS: [u16; {len(names)}] = {ints(ends)};",
        "",
        "/// The metazone (index in `METAZONES` plus one) of each name, 0 for none.",
        f"pub(super) static ZONE_METAZONE: [u8; {len(names)}] = {metazone_array};",
        "",
        "/// The entry of `ZONE_OVERRIDES` plus one of each name, 0 for none.",
        f"pub(super) static ZONE_OVERRIDE_OF: [u8; {len(names)}] = {override_array};",
    ]
    out += [""]
    return "\n".join(out)


def ints(values: list[int]) -> str:
    """A Rust array of numbers."""
    return "[" + ", ".join(str(v) for v in values) + "]"


def write_data() -> int:
    """Writes `zone_names_data.rs`. Returns the exit code."""
    source = generate(cldr_files(VERSION))
    output_path().write_text(source, encoding="utf-8")
    print(f"wrote {output_path().relative_to(paths.repo_root())} ({len(source)} bytes of source)")
    return 0
