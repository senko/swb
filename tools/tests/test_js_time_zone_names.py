"""The generator of the time zone names of `Date` (without network)."""

from swbtools.js_time_zone_names import (
    NO_END,
    VERSION,
    current_metazone,
    generate,
    minutes,
    output_path,
    read_aliases,
)

EN = """<ldml><dates><timeZoneNames>
<zone type="Etc/UTC"><long><standard>Coordinated Universal Time</standard></long></zone>
<zone type="Europe/Dublin"><long><daylight>Irish Standard Time</daylight></long></zone>
<zone type="Asia/Nowhere"><exemplarCity>Nowhere</exemplarCity></zone>
<metazone type="Europe_Central"><long>
<standard>Central European Standard Time</standard>
<daylight>Central European Summer Time</daylight></long></metazone>
<metazone type="India"><long><standard>India Standard Time</standard></long></metazone>
<metazone type="GMT"><long><standard>Greenwich Mean Time</standard></long></metazone>
</timeZoneNames></dates></ldml>"""

META = """<supplementalData><metaZones><metazoneInfo>
<timezone type="Europe/Berlin"><usesMetazone mzone="Europe_Central"/></timezone>
<timezone type="Asia/Calcutta"><usesMetazone to="1906-01-01 00:00" mzone="Nowhere_Old"/>
<usesMetazone from="1906-01-01 00:00" mzone="India"/></timezone>
<timezone type="Africa/Casablanca">
<usesMetazone to="2018-10-28 02:00" mzone="Europe_Western"/></timezone>
<timezone type="Europe/Dublin"><usesMetazone mzone="GMT"/></timezone>
</metazoneInfo></metaZones></supplementalData>"""

BCP47 = """<ldmlBCP47><keyword><key name="tz">
<type name="inccu" alias="Asia/Calcutta Asia/Kolkata"/>
<type name="deber" alias="Europe/Berlin"/>
<type name="utc" alias="Etc/UTC Etc/UCT UCT"/>
</key></keyword></ldmlBCP47>"""


def files(tmp_path):
    names = {
        "common/main/en.xml": EN,
        "common/supplemental/metaZones.xml": META,
        "common/bcp47/timezone.xml": BCP47,
    }
    result = {}
    for name, text in names.items():
        path = tmp_path / name.replace("/", "_")
        path.write_text(text, encoding="utf-8")
        result[name] = path
    return result


def test_minutes():
    assert minutes("1970-01-01 00:01") == 1
    assert minutes("1970-01-02 00:00") == 1440
    assert minutes("1969-12-31 23:00") == -60
    assert minutes(None) == NO_END


def test_current_metazone_is_the_last_open_interval():
    assert current_metazone([(-NO_END, 5, "A"), (5, NO_END, "B")]) == "B"
    assert current_metazone([(-NO_END, 5, "A")]) is None
    assert current_metazone([]) is None


def test_aliases_point_to_the_first_name(tmp_path):
    path = files(tmp_path)["common/bcp47/timezone.xml"]
    aliases = read_aliases(path)
    assert aliases["Asia/Kolkata"] == "Asia/Calcutta"
    assert aliases["UCT"] == "Etc/UTC"
    assert "Europe/Berlin" not in aliases


def test_generate(tmp_path):
    source = generate(files(tmp_path), "99")
    assert "CLDR 99" in source
    assert "Do not edit." in source
    assert '("Central European Standard Time", "Central European Summer Time"),' in source
    assert '("India Standard Time", ""),' in source
    # The zones with names of their own, and the alias of a canonical zone.
    assert "pub(super) static ZONE_OVERRIDES: [(&str, &str); 2]" in source
    assert '("Coordinated Universal Time", ""),' in source
    assert '("", "Irish Standard Time"),' in source
    text = source.split("ZONE_TEXT: &str = ")[1].split(";\n")[0]
    names = text.strip('"').split("\n")
    assert names == sorted(names)
    for name in (
        "Asia/Calcutta",
        "Asia/Kolkata",
        "Europe/Berlin",
        "Etc/UTC",
        "UCT",
        "Africa/Casablanca",
    ):
        assert name in names
    assert "Asia/Nowhere" not in names


def test_the_committed_data_names_the_generator_version():
    header = output_path().read_text(encoding="utf-8").split("\n\n", 1)[0]
    assert f"CLDR {VERSION}" in header
