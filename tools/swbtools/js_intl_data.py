"""`swbtools js-intl-data`: the locale data of `Intl` (en-US and en-GB).

Writes `crates/js/src/intl/data.rs` from the files of the Unicode CLDR 48
(`common/main/en.xml`, `en_001.xml`, `en_GB.xml`, `root.xml`,
`common/supplemental/metaZones.xml`, `supplementalData.xml` (the fraction
digits of the currencies), `common/bcp47/timezone.xml`; the Unicode License v3), which
`swbtools js-time-zone-names` and this module download from unicode.org
into `out/cldr/48/` as the `cldr-common-48.zip` archive, unless they are
there already.

The inheritance of CLDR is resolved here: a value comes from the first of
`en_GB`, `en_001`, `en` that has the element at the same path (the parent
chain of `en_GB`, `common/supplemental/supplementalData.xml`
`parentLocales`). `en-US` is `en`. The patterns that CLDR gives in two
forms (`alt="ascii"` for a time with a plain space before the day period
instead of U+202F) are taken in the ASCII form, as Chromium 148 prints them
(measured: ADR 0027 section 10).

What the data has (ADR 0027 section 10):

- The Gregorian calendar: the names of months, weekdays, day periods
  `am`/`pm` and eras; the standard date and time patterns with their
  skeletons; the patterns that join a date and a time; `availableFormats`;
  `appendItems` and the field names for them.
- The short time zone names of the two locales (the metazones and the
  zones of `timeZoneNames`, resolved to the zone names of `date/zone_names`).
- Numbers: symbols, the patterns of percent, currency and accounting, a
  list of currencies and of units.
- Relative times: the units second to year in the widths long, short and
  narrow.
"""

import logging
import xml.etree.ElementTree as ET
from pathlib import Path

from swbtools import paths
from swbtools.js_time_zone_names import (
    VERSION,
    current_metazone,
    read_aliases,
    read_intervals,
    rust_string,
)

log = logging.getLogger(__name__)

FILES = (
    "common/main/en.xml",
    "common/main/en_001.xml",
    "common/main/en_GB.xml",
    "common/main/root.xml",
    "common/supplemental/metaZones.xml",
    "common/supplemental/supplementalData.xml",
    "common/bcp47/timezone.xml",
)

CURRENCIES = [
    "USD",
    "EUR",
    "GBP",
    "JPY",
    "CAD",
    "AUD",
    "CHF",
    "CNY",
    "INR",
    "KRW",
    "BRL",
    "MXN",
    "SEK",
    "NOK",
    "DKK",
    "PLN",
    "NZD",
    "RUB",
    "TRY",
    "ZAR",
    "HKD",
    "SGD",
]
"""The currencies of the table."""

UNITS = [
    "acre",
    "bit",
    "byte",
    "celsius",
    "centimeter",
    "day",
    "degree",
    "fahrenheit",
    "fluid-ounce",
    "foot",
    "gallon",
    "gigabit",
    "gigabyte",
    "gram",
    "hectare",
    "hour",
    "inch",
    "kilobit",
    "kilobyte",
    "kilogram",
    "kilometer",
    "liter",
    "megabit",
    "megabyte",
    "meter",
    "microsecond",
    "mile",
    "mile-scandinavian",
    "milliliter",
    "millimeter",
    "millisecond",
    "minute",
    "month",
    "nanosecond",
    "ounce",
    "percent",
    "petabyte",
    "pound",
    "second",
    "stone",
    "terabit",
    "terabyte",
    "week",
    "yard",
    "year",
    "kilometer-per-hour",
    "mile-per-hour",
    "meter-per-second",
    "mile-per-gallon",
    "liter-per-kilometer",
]
"""The units of ECMA-402 (the sanctioned simple units, and the compound
ones that CLDR has a pattern for)."""

RELATIVE_UNITS = ["second", "minute", "hour", "day", "week", "month", "quarter", "year"]
WIDTHS = ["long", "short", "narrow"]
MONTH_WIDTHS = ["wide", "abbreviated", "narrow"]
WEEKDAYS = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"]

APPEND_KEYS = {
    "Era": "G",
    "Year": "y",
    "Quarter": "Q",
    "Month": "M",
    "Week": "w",
    "Day-Of-Week": "E",
    "Day": "d",
    "Hour": "h",
    "Minute": "m",
    "Second": "s",
    "Timezone": "z",
}
FIELD_NAMES = {
    "era": "G",
    "year": "y",
    "quarter": "Q",
    "month": "M",
    "week": "w",
    "weekday": "E",
    "day": "d",
    "hour": "h",
    "minute": "m",
    "second": "s",
    "zone": "z",
}

LOCALES = (("en-US", ("en", "root")), ("en-GB", ("en_GB", "en_001", "en", "root")))
"""The tags of the table and the chain of CLDR locales of each, the most
specific first."""

HOUR12 = {"en-US": True, "en-GB": False}
"""`timeData` of `supplementalData.xml`: the region US prefers `h`, GB `H`."""


def output_path() -> Path:
    """Returns `crates/js/src/intl/data.rs`."""
    return paths.repo_root() / "crates" / "js" / "src" / "intl" / "data.rs"


def cldr_files() -> dict[str, Path]:
    """Returns the local copies of the CLDR files; downloads the archive and
    extracts them if one is missing."""
    import io
    import urllib.request
    import zipfile

    directory = paths.out_dir() / "cldr" / VERSION
    wanted = {name: directory / name for name in FILES}
    if all(path.is_file() for path in wanted.values()):
        return wanted
    url = f"https://unicode.org/Public/cldr/{VERSION}/cldr-common-{VERSION}.zip"
    log.info("downloading %s", url)
    with urllib.request.urlopen(url, timeout=300) as response:
        archive = zipfile.ZipFile(io.BytesIO(response.read()))
    for name, path in wanted.items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(archive.read(name))
    return wanted


def find_path(root: ET.Element, path: str) -> ET.Element | None:
    """The first element at `path` below `root` (the steps of `Chain.find`;
    ElementTree has no "attribute is missing" test)."""
    elements = [root]
    for step in path.split("/"):
        tag, _, rest = step.partition("[")
        test = rest.rstrip("]")
        found = []
        for element in elements:
            for child in element.findall(tag):
                if not test:
                    found.append(child)
                elif test.startswith("-"):
                    if test[1:] not in child.attrib:
                        found.append(child)
                else:
                    name, _, value = test[1:].partition("=")
                    if child.get(name) == value.strip("'"):
                        found.append(child)
        elements = found
        if not elements:
            return None
    return elements[0]


class Chain:
    """The parsed files of a locale and its parents, the most specific
    first."""

    def __init__(self, roots: list[ET.Element]) -> None:
        self.roots = roots

    def find(self, path: str) -> ET.Element | None:
        """The element at `path` in the first locale that has it. A step is
        `tag`, `tag[@attribute='value']` or `tag[-attribute]` (an element
        without that attribute)."""
        for root in self.roots:
            element = find_path(root, path)
            if element is not None:
                return element
        return None

    def text(self, path: str) -> str:
        element = self.find(path)
        assert element is not None and element.text is not None, path
        return element.text


def ascii_form(chain: Chain, base: str, item: str) -> str:
    """The pattern at `base/item` of the first locale that has one, in its
    `alt="ascii"` form if that locale has it."""
    for root in chain.roots:
        parent = find_path(root, base)
        if parent is None or not parent.findall(item):
            continue
        for child in parent.findall(item):
            if child.get("alt") == "ascii":
                return (child.text or "").strip()
        return (find_path(parent, f"{item}[-alt]").text or "").strip()  # type: ignore[union-attr]
    raise AssertionError(base)


CAL = "dates/calendars/calendar[@type='gregorian']"


def calendar(chain: Chain) -> str:
    """The Rust literal of the `CalendarData` of a locale."""
    months = [
        [
            chain.text(
                f"{CAL}/months/monthContext[@type='format']/monthWidth[@type='{width}']"
                f"/month[@type='{m}']"
            )
            for m in range(1, 13)
        ]
        for width in MONTH_WIDTHS
    ]
    days = [
        [
            chain.text(
                f"{CAL}/days/dayContext[@type='format']/dayWidth[@type='{width}']/day[@type='{d}']"
            )
            for d in WEEKDAYS
        ]
        for width in MONTH_WIDTHS
    ]
    periods = [
        chain.text(
            f"{CAL}/dayPeriods/dayPeriodContext[@type='format']"
            f"/dayPeriodWidth[@type='abbreviated']/dayPeriod[@type='{p}']"
        )
        for p in ("am", "pm")
    ]
    eras = [
        [chain.text(f"{CAL}/eras/{kind}/era[@type='{e}']") for e in (0, 1)]
        for kind in ("eraNames", "eraAbbr", "eraNarrow")
    ]
    lengths = ["full", "long", "medium", "short"]
    available: dict[str, str] = {}
    date_formats = []
    for length in lengths:
        base = f"{CAL}/dateFormats/dateFormatLength[@type='{length}']/dateFormat"
        pattern = chain.text(f"{base}/pattern")
        date_formats.append(pattern)
        available[chain.text(f"{base}/datetimeSkeleton")] = pattern
    time_formats = []
    for length in lengths:
        base = f"{CAL}/timeFormats/timeFormatLength[@type='{length}']/timeFormat"
        pattern = ascii_form(chain, base, "pattern")
        time_formats.append(pattern)
        available[chain.text(f"{base}/datetimeSkeleton")] = pattern
    glue = []
    for length in lengths:
        base = f"{CAL}/dateTimeFormats/dateTimeFormatLength[@type='{length}']"
        standard = chain.text(f"{base}/dateTimeFormat[-type]/pattern")
        at_time = chain.text(f"{base}/dateTimeFormat[@type='atTime']/pattern")
        glue.append((standard, at_time))
    # availableFormats: the entries with their ASCII alternative.
    items: dict[str, str] = {}
    for root in reversed(chain.roots):
        parent = root.find(f"{CAL}/dateTimeFormats/availableFormats")
        if parent is None:
            continue
        # The plain entries first, then the ASCII ones of the same file.
        for alt in (None, "ascii"):
            for item in parent.findall("dateFormatItem"):
                if "count" not in item.attrib and item.get("alt") == alt:
                    items[item.attrib["id"]] = (item.text or "").strip()
    available.update(items)
    # Patterns with fields that a request cannot name are no use.
    available = {k: v for k, v in available.items() if not any(c in k for c in "BWwYQb")}
    append = {}
    for root in reversed(chain.roots):
        parent = root.find(f"{CAL}/dateTimeFormats/appendItems")
        if parent is not None:
            for item in parent:
                append[APPEND_KEYS[item.attrib["request"]]] = (item.text or "").strip()
    names = {}
    for field, key in FIELD_NAMES.items():
        element = chain.find(f"dates/fields/field[@type='{field}']/displayName")
        assert element is not None and element.text, field
        names[key] = element.text
    short = zone_short(chain)
    return (
        "CalendarData {\n"
        f"        months: {nested(months)},\n"
        f"        weekdays: {nested(days)},\n"
        f"        day_periods: {flat(periods)},\n"
        f"        eras: {nested(eras)},\n"
        f"        date_formats: {flat(date_formats)},\n"
        f"        time_formats: {flat(time_formats)},\n"
        f"        glue: {nested(glue)},\n"
        f"        available: &{pairs(sorted(available.items()))},\n"
        f"        append: &{pairs(sorted(append.items()))},\n"
        f"        field_names: &{pairs(sorted(names.items()))},\n"
        f"        zone_short: &{triples(short)},\n"
        "    }"
    )


def zone_short(chain: Chain) -> list[tuple[str, str, str]]:
    """The short standard and daylight names of the zones that have one in
    the locale: from the zone itself or from its current metazone. A zone
    that has neither is left out (its name is the GMT format)."""
    metazones: dict[str, tuple[str, str]] = {}
    zones: dict[str, tuple[str, str]] = {}
    for root in reversed(chain.roots):
        names = root.find("dates/timeZoneNames")
        if names is None:
            continue
        for kind, target in (("metazone", metazones), ("zone", zones)):
            for element in names.findall(kind):
                short = element.find("short")
                if short is None:
                    continue
                old = target.get(element.attrib["type"], ("", ""))
                # The text `∅∅∅` removes the name that a parent locale has.
                standard = short.findtext("standard")
                daylight = short.findtext("daylight")
                target[element.attrib["type"]] = (
                    old[0] if standard is None else ("" if standard == "∅∅∅" else standard),
                    old[1] if daylight is None else ("" if daylight == "∅∅∅" else daylight),
                )
    files = cldr_files()
    intervals = read_intervals(files["common/supplemental/metaZones.xml"])
    aliases = read_aliases(files["common/bcp47/timezone.xml"])
    result: dict[str, tuple[str, str]] = {}
    for zone, zone_intervals in intervals.items():
        current = current_metazone(zone_intervals)
        names = zones.get(zone) or ("", "")
        meta = metazones.get(current or "", ("", ""))
        standard, daylight = names[0] or meta[0], names[1] or meta[1]
        if standard or daylight:
            result[zone] = (standard, daylight)
    for zone, names in zones.items():
        result.setdefault(zone, names)
    for alias, canonical in aliases.items():
        if canonical in result and alias not in result:
            result[alias] = result[canonical]
    return sorted((zone, *names) for zone, names in result.items())


def numbers(chain: Chain) -> str:
    """The Rust literal of the `NumberData` of a locale."""
    base = "numbers"
    symbols = [
        chain.text(f"{base}/symbols[@numberSystem='latn']/{name}")
        for name in ("decimal", "group", "percentSign", "plusSign", "minusSign", "infinity", "nan")
    ]
    name_pattern = chain.text(
        f"{base}/currencyFormats[@numberSystem='latn']/unitPattern[@count='other']"
    )
    currencies = []
    for code in CURRENCIES:
        symbol = narrow = code
        names = [code, code]
        for root in reversed(chain.roots):
            element = root.find(f"{base}/currencies/currency[@type='{code}']")
            if element is None:
                continue
            for child in element:
                if child.tag == "symbol" and child.get("alt") is None:
                    symbol = child.text or symbol
                elif child.tag == "symbol" and child.get("alt") == "narrow":
                    narrow = child.text or narrow
                elif child.tag == "displayName" and child.get("count") == "one":
                    names[0] = child.text or names[0]
                elif child.tag == "displayName" and child.get("count") == "other":
                    names[1] = child.text or names[1]
        if narrow == code:
            narrow = symbol
        currencies.append(
            f"CurrencyData {{ code: {rust_string(code)}, symbol: {rust_string(symbol)}, "
            f"narrow: {rust_string(narrow)}, names: {flat(names)} }}"
        )
    units = []
    for unit in sorted(UNITS):
        widths = []
        for width in WIDTHS:
            forms = []
            for count in ("one", "other"):
                text = unit_pattern(chain, width, unit, count)
                assert text, (unit, width, count)
                forms.append(text)
            widths.append(forms)
        units.append(f"UnitData {{ name: {rust_string(unit)}, patterns: {nested(widths)} }}")
    return (
        "NumberData {\n"
        f"        symbols: {flat(symbols)},\n"
        f"        currency_name: {rust_string(name_pattern)},\n"
        "        currencies: &[\n            "
        + ",\n            ".join(currencies)
        + ",\n        ],\n"
        "        units: &[\n            " + ",\n            ".join(units) + ",\n        ],\n"
        "    }"
    )


def read_currency_fractions(path: Path) -> tuple[int, list[tuple[str, int]]]:
    """The fraction digits of the currencies from `currencyData/fractions` of
    `supplementalData.xml` (ISO 4217 minor units): the default and the
    sorted list of the codes that differ from it."""
    fractions = ET.parse(path).getroot().find("currencyData/fractions")
    assert fractions is not None, "no currencyData/fractions"
    default = None
    listed: dict[str, int] = {}
    for info in fractions.findall("info"):
        code, digits = info.attrib["iso4217"], int(info.attrib["digits"])
        if code == "DEFAULT":
            default = digits
        else:
            listed[code] = digits
    assert default is not None, "no DEFAULT in currencyData/fractions"
    return default, sorted((c, d) for c, d in listed.items() if d != default)


def unit_pattern(chain: Chain, width: str, unit: str, count: str) -> str | None:
    """The pattern of a unit (`minute` for the CLDR type `duration-minute`)
    from the first locale that has it."""
    for root in chain.roots:
        for element in root.findall(f"units/unitLength[@type='{width}']/unit"):
            if element.attrib["type"].split("-", 1)[-1] != unit:
                continue
            pattern = element.find(f"unitPattern[@count='{count}']")
            if pattern is not None:
                return pattern.text
    return None


def first_item(chain: Chain, bases: list[str], path: str) -> ET.Element | None:
    """The first element with text at `path` below one of `bases`."""
    for base in bases:
        element = chain.find(f"{base}/{path}")
        if element is not None and element.text:
            return element
    return None


def relative(chain: Chain) -> str:
    """The Rust literal of the `RelativeTable` of a locale."""
    widths = []
    for width in WIDTHS:
        # A narrow field without an item takes the short one, a short
        # field the long one (the aliases of the fields in CLDR's root.xml).
        order = {"long": [""], "short": ["-short", ""], "narrow": ["-narrow", "-short", ""]}[width]
        fields = [f"dates/fields/field[@type='{{unit}}{suffix}']" for suffix in order]
        units = []
        for unit in RELATIVE_UNITS:
            candidates = [f.format(unit=unit) for f in fields]
            auto = []
            for offset in ("-1", "0", "1"):
                element = first_item(chain, candidates, f"relative[@type='{offset}']")
                auto.append(element.text if element is not None and element.text else "")
            forms = {}
            for kind in ("future", "past"):
                forms[kind] = []
                for count in ("one", "other"):
                    element = first_item(
                        chain,
                        candidates,
                        f"relativeTime[@type='{kind}']/relativeTimePattern[@count='{count}']",
                    )
                    assert element is not None and element.text, (unit, kind, count)
                    forms[kind].append(element.text)
            units.append(
                f"RelativeData {{ auto: {flat(auto)}, future: {flat(forms['future'])}, "
                f"past: {flat(forms['past'])} }}"
            )
        widths.append("[\n            " + ",\n            ".join(units) + ",\n        ]")
    return "[\n        " + ",\n        ".join(widths) + ",\n    ]"


def flat(values: list | tuple) -> str:
    """A Rust array of strings (or of arrays of strings)."""
    return "[" + ", ".join(rust_string(v) if isinstance(v, str) else flat(v) for v in values) + "]"


def nested(values: list) -> str:
    return flat(values)


def pairs(items: list[tuple[str, str]]) -> str:
    return "[" + ", ".join(f"({rust_string(a)}, {rust_string(b)})" for a, b in items) + "]"


def triples(items: list[tuple[str, str, str]]) -> str:
    return (
        "[\n            "
        + ",\n            ".join(
            f"({rust_string(a)}, {rust_string(b)}, {rust_string(c)})" for a, b, c in items
        )
        + ",\n        ]"
    )


def generate(files: dict[str, Path]) -> str:
    """Returns the Rust source of `data.rs`."""
    out = [
        f"//! The locale data of `Intl` from CLDR {VERSION}, generated by",
        "//! `swbtools js-intl-data` (`tools/swbtools/js_intl_data.py`) from",
        "//! `common/main/en.xml`, `en_001.xml`, `en_GB.xml`,",
        "//! `common/main/root.xml`, `common/supplemental/metaZones.xml`,",
        "//! `common/supplemental/supplementalData.xml` and `common/bcp47/timezone.xml` of",
        f"//! <https://unicode.org/Public/cldr/{VERSION}/cldr-common-{VERSION}.zip>.",
        "//! Do not edit. Unicode License v3 (`THIRD_PARTY_NOTICES.md`).",
        "",
        "use super::tables::{",
        "    CalendarData, CurrencyData, LocaleData, NumberData, RelativeData, UnitData,",
        "};",
        "",
    ]
    main = files["common/main/en.xml"].parent
    aliases = read_aliases(files["common/bcp47/timezone.xml"])
    out += [
        "/// The zone names that CLDR writes with another identifier: name and",
        "/// canonical identifier (`Asia/Kolkata`, `Asia/Calcutta`), sorted.",
        "pub(super) static ZONE_CANONICAL: &[(&str, &str)] = &[",
    ]
    out += [f"    ({rust_string(a)}, {rust_string(c)})," for a, c in sorted(aliases.items())]
    out += ["];", ""]
    default_digits, fractions = read_currency_fractions(
        files["common/supplemental/supplementalData.xml"]
    )
    out += [
        "/// The fraction digits of a currency whose digits differ from the",
        "/// default (`currencyData/fractions`, ISO 4217 minor units), sorted by code.",
        "pub(super) static CURRENCY_DIGITS: &[(&str, u8)] = &[",
    ]
    out += [f"    ({rust_string(code)}, {digits})," for code, digits in fractions]
    out += [
        "];",
        "",
        "/// The fraction digits of the other currencies.",
        f"pub(super) const DEFAULT_CURRENCY_DIGITS: u8 = {default_digits};",
        "",
    ]
    for tag, chain_names in LOCALES:
        chain = Chain([ET.parse(main / f"{name}.xml").getroot() for name in chain_names])
        const = tag.replace("-", "_").upper()
        out += [
            f"/// The data of `{tag}`.",
            f"pub(super) static {const}: LocaleData = LocaleData {{",
            f"    hour12: {'true' if HOUR12[tag] else 'false'},",
            f"    calendar: {calendar(chain)},",
            f"    numbers: {numbers(chain)},",
            f"    relative: {relative(chain)},",
            "};",
            "",
        ]
    return "\n".join(out)


def write_data() -> int:
    """Writes `data.rs`. Returns the exit code."""
    source = generate(cldr_files())
    output_path().write_text(source, encoding="utf-8")
    print(f"wrote {output_path().relative_to(paths.repo_root())} ({len(source)} bytes of source)")
    return 0
