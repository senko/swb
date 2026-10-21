//! Cases for `Date` (M7 features 12a and 12b): the measured Chromium cases
//! (`tools/probes/js-date.json`, results in seven time zones and, for the
//! zone names, fifteen more: the
//! shape of the objects, the receiver errors, the constructor and
//! `Date.UTC` with every kind of argument, the getters, the string forms
//! and `Date.parse` over a list of instants from year -271821 to 275760,
//! the local times around the transitions of each zone, the setters and
//! the order of their conversions, V8's messages), and the host hooks
//! (the clock and the zone) and `console.log` of dates. Each Chromium case
//! also runs in the GC stress mode. The limits are in `date_hostile.rs`.

mod common;

use common::{check_cases, run_in_zone};

/// The zones of `data/gen-date-cases.js` and the file of the results.
const ZONES: [(&str, &str); 22] = [
    ("UTC", include_str!("data/date_cases_utc.expected")),
    (
        "Europe/Berlin",
        include_str!("data/date_cases_europe_berlin.expected"),
    ),
    (
        "America/New_York",
        include_str!("data/date_cases_america_new_york.expected"),
    ),
    (
        "Asia/Kolkata",
        include_str!("data/date_cases_asia_kolkata.expected"),
    ),
    (
        "Australia/Adelaide",
        include_str!("data/date_cases_australia_adelaide.expected"),
    ),
    (
        "Australia/Lord_Howe",
        include_str!("data/date_cases_australia_lord_howe.expected"),
    ),
    (
        "Pacific/Apia",
        include_str!("data/date_cases_pacific_apia.expected"),
    ),
    (
        "Europe/London",
        include_str!("data/date_cases_europe_london.expected"),
    ),
    (
        "Europe/Dublin",
        include_str!("data/date_cases_europe_dublin.expected"),
    ),
    (
        "America/Los_Angeles",
        include_str!("data/date_cases_america_los_angeles.expected"),
    ),
    (
        "America/Argentina/Buenos_Aires",
        include_str!("data/date_cases_america_argentina_buenos_aires.expected"),
    ),
    (
        "Asia/Kathmandu",
        include_str!("data/date_cases_asia_kathmandu.expected"),
    ),
    (
        "Africa/Casablanca",
        include_str!("data/date_cases_africa_casablanca.expected"),
    ),
    (
        "Africa/Windhoek",
        include_str!("data/date_cases_africa_windhoek.expected"),
    ),
    (
        "Antarctica/Troll",
        include_str!("data/date_cases_antarctica_troll.expected"),
    ),
    (
        "Asia/Almaty",
        include_str!("data/date_cases_asia_almaty.expected"),
    ),
    (
        "Africa/Cairo",
        include_str!("data/date_cases_africa_cairo.expected"),
    ),
    (
        "America/Sao_Paulo",
        include_str!("data/date_cases_america_sao_paulo.expected"),
    ),
    (
        "Asia/Tehran",
        include_str!("data/date_cases_asia_tehran.expected"),
    ),
    (
        "Asia/Amman",
        include_str!("data/date_cases_asia_amman.expected"),
    ),
    (
        "America/Santiago",
        include_str!("data/date_cases_america_santiago.expected"),
    ),
    (
        "Pacific/Fiji",
        include_str!("data/date_cases_pacific_fiji.expected"),
    ),
];

/// The script of `data/date_cases.js` in the zone `zone`, normally and in
/// the GC stress mode, against the results of Chromium 148 in that zone
/// (`data/date_cases_ZONE.expected`; the script and the expected output
/// are made by `data/gen-date-cases.js` from the output of `just probe`).
fn chromium_cases(zone: &str) {
    let script = include_str!("data/date_cases.js");
    let expected = ZONES
        .iter()
        .find(|(name, _)| *name == zone)
        .map(|(_, text)| *text)
        .expect("a zone of the cases");
    let script = format!("var ZONE = {zone:?};\n{script}");
    for stress in [false, true] {
        let actual = run_in_zone(&script, stress, zone, 1_700_000_000_000.0);
        let failures: Vec<String> = actual
            .lines()
            .zip(expected.lines())
            .filter(|(actual, expected)| actual != expected)
            .take(40)
            .map(|(actual, expected)| format!("expected: {expected}\nactual:   {actual}"))
            .collect();
        assert_eq!(
            actual.lines().count(),
            expected.lines().count(),
            "number of lines ({zone}, stress: {stress})"
        );
        assert!(
            failures.is_empty(),
            "{zone}, stress: {stress}\n{}",
            failures.join("\n")
        );
    }
}

#[test]
fn chromium_cases_utc() {
    chromium_cases("UTC");
}

#[test]
fn chromium_cases_berlin() {
    chromium_cases("Europe/Berlin");
}

#[test]
fn chromium_cases_new_york() {
    chromium_cases("America/New_York");
}

#[test]
fn chromium_cases_kolkata() {
    chromium_cases("Asia/Kolkata");
}

#[test]
fn chromium_cases_adelaide() {
    chromium_cases("Australia/Adelaide");
}

#[test]
fn chromium_cases_lord_howe() {
    chromium_cases("Australia/Lord_Howe");
}

#[test]
fn chromium_cases_apia() {
    chromium_cases("Pacific/Apia");
}

#[test]
fn chromium_cases_london() {
    chromium_cases("Europe/London");
}

#[test]
fn chromium_cases_dublin() {
    chromium_cases("Europe/Dublin");
}

#[test]
fn chromium_cases_los_angeles() {
    chromium_cases("America/Los_Angeles");
}

#[test]
fn chromium_cases_argentina_buenos_aires() {
    chromium_cases("America/Argentina/Buenos_Aires");
}

#[test]
fn chromium_cases_kathmandu() {
    chromium_cases("Asia/Kathmandu");
}

#[test]
fn chromium_cases_casablanca() {
    chromium_cases("Africa/Casablanca");
}

#[test]
fn chromium_cases_windhoek() {
    chromium_cases("Africa/Windhoek");
}

#[test]
fn chromium_cases_troll() {
    chromium_cases("Antarctica/Troll");
}

#[test]
fn chromium_cases_almaty() {
    chromium_cases("Asia/Almaty");
}

#[test]
fn chromium_cases_cairo() {
    chromium_cases("Africa/Cairo");
}

#[test]
fn chromium_cases_sao_paulo() {
    chromium_cases("America/Sao_Paulo");
}

#[test]
fn chromium_cases_tehran() {
    chromium_cases("Asia/Tehran");
}

#[test]
fn chromium_cases_amman() {
    chromium_cases("Asia/Amman");
}

#[test]
fn chromium_cases_santiago() {
    chromium_cases("America/Santiago");
}

#[test]
fn chromium_cases_fiji() {
    chromium_cases("Pacific/Fiji");
}

/// The host clock: `Date.now()`, `new Date()` and `Date()` read it, and a
/// clock that is not an integer is floored.
#[test]
fn fixed_clock() {
    let script = "print(Date.now(), new Date().getTime(), new Date().toISOString(), \
                  Date().slice(0, 15), Date.now() === Date.now())";
    let out = run_in_zone(script, false, "UTC", 1_700_000_000_123.9);
    assert_eq!(
        out,
        "1700000000123 1700000000123 2023-11-14T22:13:20.123Z Tue Nov 14 2023 true"
    );
    // A clock beyond the range is clipped.
    let out = run_in_zone(
        "print(Date.now(), new Date().getTime())",
        false,
        "UTC",
        9e15,
    );
    assert_eq!(out, "NaN NaN");
}

/// A POSIX rule is a zone that needs no database.
#[test]
fn posix_rule_zone() {
    let script = "var a = new Date(2024, 2, 10, 2, 30); var b = new Date(2024, 10, 3, 1, 30); \
                  print(a.toISOString(), b.toISOString(), \
                  new Date(2024, 6, 1).getTimezoneOffset(), new Date(2024, 0, 1).getTimezoneOffset())";
    let out = run_in_zone(script, false, "EST5EDT,M3.2.0,M11.1.0", 0.0);
    assert_eq!(
        out,
        "2024-03-10T07:30:00.000Z 2024-11-03T05:30:00.000Z 240 300"
    );
}

/// Node.js prints a Date as its ISO string, `Invalid Date` for NaN, with
/// the name of a subclass before it and the own properties after it.
#[test]
fn console_log() {
    let cases: &[(&str, &str)] = &[
        ("console.log(new Date(0))", "1970-01-01T00:00:00.000Z"),
        ("console.log(new Date(NaN))", "Invalid Date"),
        (
            "console.log({ d: new Date(86400000), e: [new Date(NaN)] })",
            "{ d: 1970-01-02T00:00:00.000Z, e: [ Invalid Date ] }",
        ),
        (
            "class D extends Date {} console.log(new D(0))",
            "D 1970-01-01T00:00:00.000Z",
        ),
        (
            "var d = new Date(0); d.x = 1; console.log(d)",
            "1970-01-01T00:00:00.000Z { x: 1 }",
        ),
        ("console.log(Date.prototype)", "{}"),
        ("console.log(String(new Date(NaN)))", "Invalid Date"),
    ];
    check_cases(cases);
}
