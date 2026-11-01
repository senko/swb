"""The generator of the locale data of `Intl` (without network)."""

import re
import xml.etree.ElementTree as ET

import pytest

from swbtools.js_intl_data import (
    Chain,
    find_path,
    output_path,
    read_currency_fractions,
    unit_pattern,
)

EN = ET.fromstring(
    """<ldml><dates><calendars><calendar type="gregorian"><timeFormats>
<timeFormatLength type="short"><timeFormat>
<pattern>h:mm_a</pattern><pattern alt="ascii">h:mm a</pattern>
<datetimeSkeleton>ahmm</datetimeSkeleton></timeFormat></timeFormatLength>
</timeFormats></calendar></calendars></dates>
<units><unitLength type="long">
<unit type="duration-minute"><unitPattern count="one">{0} minute</unitPattern>
<unitPattern count="other">{0} minutes</unitPattern></unit>
</unitLength></units></ldml>"""
)
GB = ET.fromstring(
    """<ldml><dates><calendars><calendar type="gregorian"><timeFormats>
<timeFormatLength type="short"><timeFormat><pattern>HH:mm</pattern>
</timeFormat></timeFormatLength></timeFormats></calendar></calendars></dates></ldml>"""
)

SHORT = "dates/calendars/calendar[@type='gregorian']/timeFormats/timeFormatLength[@type='short']"


def test_find_path_steps():
    assert find_path(EN, f"{SHORT}/timeFormat/pattern[-alt]").text == "h:mm_a"
    assert find_path(EN, f"{SHORT}/timeFormat/pattern[@alt='ascii']").text == "h:mm a"
    assert find_path(EN, f"{SHORT}/timeFormat/nothing") is None
    assert find_path(EN, "dates/calendars/calendar[@type='buddhist']") is None


def test_the_first_locale_with_the_element_wins():
    chain = Chain([GB, EN])
    assert chain.text(f"{SHORT}/timeFormat/pattern") == "HH:mm"
    # The skeleton is only in `en`: found at the same path.
    assert chain.text(f"{SHORT}/timeFormat/datetimeSkeleton") == "ahmm"


def test_unit_patterns_come_from_the_type_suffix():
    chain = Chain([GB, EN])
    assert unit_pattern(chain, "long", "minute", "other") == "{0} minutes"
    assert unit_pattern(chain, "long", "hour", "other") is None


SUPPLEMENTAL = """<supplementalData><currencyData><fractions>
<info iso4217="DEFAULT" digits="2" rounding="0" cashDigits="2" cashRounding="0"/>
<info iso4217="KWD" digits="3" rounding="0"/>
<info iso4217="JPY" digits="0" rounding="0" cashDigits="0" cashRounding="0"/>
<info iso4217="ADP" digits="0" rounding="0"/>
<info iso4217="CHF" digits="2" rounding="0" cashRounding="5"/>
</fractions></currencyData></supplementalData>"""


def test_currency_fractions(tmp_path):
    path = tmp_path / "supplementalData.xml"
    path.write_text(SUPPLEMENTAL, encoding="utf-8")
    default, listed = read_currency_fractions(path)
    assert default == 2
    # Sorted by code; the codes with the default digits are left out.
    assert listed == [("ADP", 0), ("JPY", 0), ("KWD", 3)]


def test_currency_fractions_need_a_default(tmp_path):
    path = tmp_path / "supplementalData.xml"
    path.write_text(
        '<supplementalData><currencyData><fractions><info iso4217="KWD" digits="3"/>'
        "</fractions></currencyData></supplementalData>",
        encoding="utf-8",
    )
    with pytest.raises(AssertionError):
        read_currency_fractions(path)


def test_currency_digits_table_in_data_rs_is_sorted_and_complete():
    source = output_path().read_text(encoding="utf-8")
    start = source.index("pub(super) static CURRENCY_DIGITS")
    table = source[start : source.index("];", start)]
    rows = re.findall(r'\("([A-Z]{3})", (\d)\)', table)
    codes = [code for code, _ in rows]
    assert codes == sorted(codes)
    digits = dict(rows)
    assert digits["KWD"] == "3"
    assert digits["JPY"] == "0"
    assert "USD" not in digits
    assert "DEFAULT_CURRENCY_DIGITS: u8 = 2;" in source


def test_output_path():
    assert output_path().parts[-4:] == ("js", "src", "intl", "data.rs")
