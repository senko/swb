//! Cases for `Intl` (M9 feature 3, ADR 0027 section 10): the measured
//! Chromium 148 cases of `tools/probes/js-intl.json` for
//! `Intl.DateTimeFormat`, `Intl.NumberFormat`, `Intl.RelativeTimeFormat`,
//! `Intl.getCanonicalLocales`, the arguments of `toLocaleString`,
//! `toLocaleDateString` and `toLocaleTimeString` (Date and Number), and
//! the calls that the scripts of Ars Technica, BBC and Craigslist make;
//! and the limits (`Intl` in a loop, huge options). Each Chromium case
//! also runs in the GC stress mode. `data/gen-intl-cases.js` makes
//! `intl_cases.js` and the expected files; the snippets that swb answers
//! differently on purpose are marked `!known` in `data/intl-snippets.txt`
//! (ADR 0027 section 10) and are not run.

mod common;

use common::{check_cases, run_in_zone};

/// The zones of `data/gen-intl-cases.js` and the file of the results.
const ZONES: [(&str, &str); 6] = [
    ("UTC", include_str!("data/intl_cases_utc.expected")),
    (
        "Europe/Berlin",
        include_str!("data/intl_cases_europe_berlin.expected"),
    ),
    (
        "America/New_York",
        include_str!("data/intl_cases_america_new_york.expected"),
    ),
    (
        "Asia/Kolkata",
        include_str!("data/intl_cases_asia_kolkata.expected"),
    ),
    (
        "Australia/Adelaide",
        include_str!("data/intl_cases_australia_adelaide.expected"),
    ),
    (
        "Australia/Lord_Howe",
        include_str!("data/intl_cases_australia_lord_howe.expected"),
    ),
];

/// The script of `data/intl_cases.js` in the zone `zone`, normally and in
/// the GC stress mode, against the results of Chromium 148 in that zone.
fn chromium_cases(zone: &str) {
    let script = include_str!("data/intl_cases.js");
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
fn departures_are_the_documented_ones() {
    // Locales other than en, en-US and en-GB resolve to en-US; the other
    // Intl constructors do not exist; compact notation, calendars and
    // `formatRange` are not there (ADR 0027 section 10).
    check_cases(&[
        (
            "print(new Intl.DateTimeFormat('de').resolvedOptions().locale)",
            "en-US",
        ),
        (
            "print(typeof Intl.Collator + typeof Intl.PluralRules)",
            "undefinedundefined",
        ),
        ("print((1234.5).toLocaleString('de-DE'))", "1,234.5"),
        (
            "print(new Intl.NumberFormat('en', {notation: 'compact'}).format(1234))",
            "1,234",
        ),
        (
            "print(typeof Intl.DateTimeFormat.prototype.formatRange)",
            "undefined",
        ),
        (
            "print(new Intl.DateTimeFormat('en-US', {timeZone: 'UTC', timeZoneName: 'shortGeneric', hour: 'numeric'}).format(0))",
            "12 AM GMT+0",
        ),
        // `dayPeriod` has no effect and does not count as a time field.
        (
            "print(new Date(0).toLocaleString('en', {dayPeriod: 'long', timeZone: 'UTC'}))",
            "1/1/1970, 12:00:00 AM",
        ),
        (
            "print(new Date(0).toLocaleString('en', {dayPeriod: 'long', hour: 'numeric', timeZone: 'UTC'}))",
            "12 AM",
        ),
    ]);
}

#[test]
fn many_formats_in_a_loop_are_collected() {
    // A formatter made 200,000 times is garbage at once; the heap does not
    // grow with the count.
    check_cases(&[(
        "var n = 0; for (var i = 0; i < 200000; i++) { n += new Intl.DateTimeFormat('en-GB', {hour: 'numeric', timeZone: 'UTC'}).format(i * 3600000).length; } print(n)",
        "400000",
    )]);
}

#[test]
fn limits_of_the_options() {
    check_cases(&[
        (
            "print(new Intl.NumberFormat('en', {maximumFractionDigits: 100, minimumFractionDigits: 100}).format(1/3).length)",
            "102",
        ),
        (
            "try { new Intl.NumberFormat('en', {maximumFractionDigits: 101}) } catch (e) { print(e.name + ': ' + e.message) }",
            "RangeError: maximumFractionDigits value is out of range.",
        ),
        (
            "print(Intl.getCanonicalLocales(Array.from({length: 100000}, (_, i) => 'en-x-a' + i)).length)",
            "100000",
        ),
        // A long numeric string is read in place: no copy, and a value
        // above the largest double is the infinity, as in Chromium.
        (
            "print(new Intl.NumberFormat('en').format('1'.repeat(1 << 24)))",
            "∞",
        ),
        (
            "print(new Intl.NumberFormat('en').format('0'.repeat(1 << 24) + '5'))",
            "5",
        ),
        (
            "try { new Intl.NumberFormat('en', {maximumSignificantDigits: 3}).format('1e-9999999') } catch (e) { print(e.name + ': ' + e.message) }",
            "RangeError: Invalid string length",
        ),
    ]);
}
