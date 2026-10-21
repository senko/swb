//! Hostile cases for `Date` (M7 feature 12a): long strings to
//! `Date.parse` and the constructor (also as ropes, and in loops against
//! the deadline), huge and non-finite components in every position of the
//! constructor, `Date.UTC` and the setters, setters whose conversions
//! change the Date or collect garbage (also in the GC stress mode),
//! the ends of the time range in zones with rules, and loops of the string
//! forms against the deadline. A case ends with a result, an error or a
//! termination; none may panic, run far past the deadline or leave the
//! runtime unusable.

mod common;

use std::time::{Duration, Instant};

use common::{on_big_stack, run_in, runtime};
use swb_js::{Runtime, ScriptError, Termination, TimeZone, Value};

const DEADLINE: Duration = Duration::from_millis(300);

/// The deadline of the cases that must complete (a guard against a hang).
const LONG: Duration = Duration::from_secs(60);

/// The slack after the deadline (scheduling noise and the debug build).
const SLACK: Duration = Duration::from_millis(1500);

/// A runtime in the zone `zone`, with the test functions.
fn runtime_in(zone: &str, stress: bool) -> Runtime {
    let mut rt = runtime(stress);
    rt.set_time_zone(TimeZone::from_spec(zone).expect("a zone of the tests"));
    rt
}

fn eval_with_deadline(rt: &mut Runtime, source: &str) -> (Result<Value, ScriptError>, Duration) {
    eval_within(rt, source, DEADLINE)
}

/// Runs `source` with the deadline `limit` after now.
fn eval_within(
    rt: &mut Runtime,
    source: &str,
    limit: Duration,
) -> (Result<Value, ScriptError>, Duration) {
    let start = Instant::now();
    rt.set_deadline(Some(start + limit));
    let result = rt.eval(source);
    rt.set_deadline(None);
    (result, start.elapsed())
}

fn assert_clean(rt: &Runtime) {
    assert_eq!(rt.stack_depths(), (0, 0));
    assert_eq!(rt.heap().scope_depth(), 0);
    assert_eq!(rt.heap().no_gc_depth(), 0);
    assert_eq!(rt.heap().stats().total_stale_roots, 0);
}

/// A loop that never ends ends at the deadline with the time limit, and
/// the runtime works afterwards.
fn assert_ends_at_the_deadline(source: &str) {
    let source = source.to_owned();
    on_big_stack(move || {
        let mut rt = runtime_in("EST5EDT,M3.2.0,M11.1.0", false);
        let (result, elapsed) = eval_with_deadline(&mut rt, &source);
        assert!(
            matches!(result, Err(ScriptError::Terminated(Termination::TimeLimit))),
            "{source}: {result:?}"
        );
        assert!(elapsed < DEADLINE + SLACK, "{source}: took {elapsed:?}");
        assert_clean(&rt);
        assert_eq!(
            run_in(&mut rt, "new Date(2020, 0, 1).getFullYear()"),
            "=> 2020"
        );
    });
}

/// `Date.parse` of long strings: a text that is wrong from its first
/// units ends at once, however long it is, also as a rope; the closing
/// parenthesis of the name in the `toString` form is looked up at the end
/// without a scan.
#[test]
fn long_strings_to_parse() {
    on_big_stack(|| {
        let mut rt = runtime_in("Europe/Berlin", false);
        let cases = [
            ("Date.parse('x'.repeat(100_000_000))", "=> NaN"),
            (
                "Date.parse('2000-01-01T00:00:00.000Z' + 'x'.repeat(50_000_000))",
                "=> NaN",
            ),
            // Chromium reads the zeros as the number 0, the year 2000; the
            // reader of the written forms has no month 0 and gives NaN
            // (ADR 0027 section 8).
            ("Date.parse('0'.repeat(50_000_000) + '-01-01')", "=> NaN"),
            // Chromium: a sign is ignored, the zeros are the year 2000.
            ("Date.parse('+' + '0'.repeat(50_000_000))", "=> NaN"),
            // The fraction can have any number of digits.
            (
                "Date.parse('2000-01-01T00:00:00.' + '0'.repeat(50_000_000) + 'Z')",
                "=> 946684800000",
            ),
            (
                "Date.parse('Mon Jul 01 2024 13:05:09 GMT+0200 (' + 'x'.repeat(50_000_000) + ')')",
                "=> 1719831909000",
            ),
            (
                "Date.parse('Mon Jul 01 2024 13:05:09 GMT+0200 (' + 'x'.repeat(50_000_000))",
                "=> 1719831909000",
            ),
            (
                "var s = ''; for (var i = 0; i < 20; i++) s = s + s + 'ab'; \
                 [s.length > 1_000_000, Date.parse(s), new Date(s).getTime()].join()",
                "=> true,NaN,NaN",
            ),
            ("new Date('\\u0100'.repeat(30_000_000)).getTime()", "=> NaN"),
        ];
        for (source, expected) in cases {
            let (result, elapsed) = eval_with_deadline(&mut rt, source);
            assert!(result.is_ok(), "{source}: {result:?}");
            assert!(elapsed < DEADLINE + SLACK, "{source}: took {elapsed:?}");
            assert_eq!(run_in(&mut rt, source), expected, "{source}");
        }
        assert_clean(&rt);
    });
}

/// The reader of the written forms (ADR 0027 section 5a) scans a text once
/// and keeps at most 32 tokens: long runs of digits, words, symbols, white
/// space, unclosed parentheses, many numbers and many names, NUL units and
/// lone surrogates each end in bounded time with NaN, or a time value for
/// a text that has a date after free text or before a NUL.
#[test]
fn the_reader_of_other_forms_is_bounded() {
    on_big_stack(|| {
        let mut rt = runtime_in("Europe/Berlin", false);
        let cases = [
            ("Date.parse('1'.repeat(20_000_000))", "=> NaN"),
            (
                "Date.parse('Jan 1 2000 ' + 'x'.repeat(20_000_000))",
                "=> NaN",
            ),
            (
                "Date.parse('Jan 1 2000 ' + ' '.repeat(20_000_000))",
                "=> 946681200000",
            ),
            (
                "Date.parse('Jan 1 2000 (' + 'x'.repeat(20_000_000))",
                "=> 946681200000",
            ),
            ("Date.parse('('.repeat(20_000_000))", "=> NaN"),
            ("Date.parse(')'.repeat(20_000_000))", "=> NaN"),
            (
                "Date.parse('Jan 1 2000 ' + ')'.repeat(1_000_000))",
                "=> NaN",
            ),
            ("Date.parse('1 '.repeat(5_000_000))", "=> NaN"),
            // Chromium skips the repeated names; a month name is not free
            // text here.
            ("Date.parse('Jan '.repeat(5_000_000) + '1 2000')", "=> NaN"),
            (
                "Date.parse('x '.repeat(5_000_000) + '1 Jan 2000')",
                "=> 946681200000",
            ),
            ("Date.parse('1:'.repeat(5_000_000))", "=> NaN"),
            ("Date.parse('+'.repeat(20_000_000))", "=> NaN"),
            (
                "Date.parse('1/1/2000 5:06:07.' + '9'.repeat(20_000_000))",
                "=> 946699567999",
            ),
            (
                "Date.parse('1/1/2000 5:06 GMT+' + '1'.repeat(20_000_000))",
                "=> NaN",
            ),
            (
                "Date.parse(String.fromCharCode(0).repeat(20_000_000))",
                "=> NaN",
            ),
            (
                "Date.parse('Jan 1 2000' + String.fromCharCode(0) + 'x'.repeat(20_000_000))",
                "=> 946681200000",
            ),
            (
                "Date.parse(String.fromCharCode(0xd800).repeat(10_000_000))",
                "=> NaN",
            ),
            (
                "Date.parse('Jan 1 2000 ' + String.fromCharCode(0xd800))",
                "=> NaN",
            ),
            (
                "Date.parse(String.fromCharCode(0xd800) + ' Jan 1 2000')",
                "=> 946681200000",
            ),
        ];
        for (source, expected) in cases {
            let (result, elapsed) = eval_within(&mut rt, source, LONG);
            assert!(result.is_ok(), "{source}: {result:?}");
            assert!(
                elapsed < Duration::from_secs(5),
                "{source}: took {elapsed:?}"
            );
            assert_eq!(run_in(&mut rt, source), expected, "{source}");
        }
        assert_clean(&rt);
    });
}

#[test]
fn loops_of_long_parses_end_at_the_deadline() {
    assert_ends_at_the_deadline(
        "var long = 'x'.repeat(1_000_000); for (;;) { Date.parse(long); new Date(long); }",
    );
    assert_ends_at_the_deadline(
        "var long = 'Mon Jul 01 2024 13:05:09 GMT+0200 (' + 'y'.repeat(1_000_000) + ')'; \
         for (;;) { Date.parse(long); }",
    );
    // The scan of the reader of other forms is charged.
    assert_ends_at_the_deadline(
        "var long = 'Jan 1 2000 ' + 'x'.repeat(1_000_000); for (;;) { Date.parse(long); }",
    );
    assert_ends_at_the_deadline(
        "var long = '1 '.repeat(500_000); for (;;) { Date.parse(long); new Date(long); }",
    );
    // A new rope each time: the flatten is charged.
    assert_ends_at_the_deadline(
        "var base = '2000-01-01T00:00:00.000Z' + 'x'.repeat(100_000); \
         for (var i = 0; ; i++) { Date.parse(base + i); }",
    );
}

#[test]
fn loops_of_string_forms_end_at_the_deadline() {
    for body in [
        "d.toString()",
        "d.toDateString() + d.toTimeString()",
        "d.toUTCString() + d.toISOString()",
        "d.toLocaleString() + d.toJSON()",
        "String(d) + (d + 1)",
        "Date.parse(d.toString())",
        "new Date(d.getTime() + 1).toString()",
        "d.setTime(d.getTime() + 86400000), d.getFullYear()",
        "d.setMonth(d.getMonth() + 1), d.setHours(d.getHours() + 5), d.getTimezoneOffset()",
    ] {
        assert_ends_at_the_deadline(&format!(
            "var d = new Date(2020, 0, 1); for (;;) {{ {body}; }}"
        ));
    }
}

/// Huge and non-finite components in every position of the constructor,
/// `Date.UTC` and the setters: no panic, NaN or a value in the range.
#[test]
fn huge_and_non_finite_components() {
    on_big_stack(|| {
        let mut rt = runtime_in("America/New_York", false);
        let source = "var values = [0, -0, 1, -1, 0.5, 1e9, -1e9, 1e12, 2 ** 31, 2 ** 32, 2 ** 53, -(2 ** 53), \
            1e15, 1e16, 1e20, 1e100, 1e300, 1.7976931348623157e308, -1.7976931348623157e308, \
            5e-324, Infinity, -Infinity, NaN, 8.64e15, -8.64e15, 8.64e15 + 1, 275760, -271821, 4294967295, 4294967296]; \
            var count = 0, valid = 0; \
            function check(t) { count++; if (t === t) { valid++; if (Math.abs(t) > 8.64e15 || t !== Math.trunc(t) || Object.is(t, -0)) throw new Error('bad ' + t); } } \
            for (var a of values) { for (var b of values) { \
              check(Date.UTC(a, b)); check(Date.UTC(2020, a, b)); check(Date.UTC(2020, 1, a, b)); check(Date.UTC(2020, 1, 1, a, b)); \
              check(Date.UTC(2020, 1, 1, 1, a, b)); check(Date.UTC(2020, 1, 1, 1, 1, a, b)); \
              check(new Date(a, b).getTime()); check(new Date(2020, a, b).getTime()); check(new Date(2020, 1, a, b).getTime()); \
              check(new Date(2020, 1, 1, a, b).getTime()); check(new Date(2020, 1, 1, 1, a, b).getTime()); check(new Date(2020, 1, 1, 1, 1, a, b).getTime()); \
              var d = new Date(2020, 5, 15, 12); check(d.setFullYear(a, b)); check(d.setMonth(a, b)); check(d.setDate(a)); \
              check(d.setHours(a, b)); check(d.setMinutes(a, b)); check(d.setSeconds(a, b)); check(d.setMilliseconds(a)); \
              check(d.setUTCFullYear(a, b, a)); check(d.setUTCMonth(a, b)); check(d.setUTCHours(a, b, a, b)); check(d.setYear(a)); check(d.setTime(a)); \
              var e = new Date(a); check(e.getTime()); \
              if (e.getTime() === e.getTime()) { e.toString(); e.toUTCString(); e.toISOString(); e.toLocaleString(); e.getTimezoneOffset(); e.getFullYear(); e.getUTCDay(); } \
            } } \
            count + ':' + (valid > 0)";
        let (result, _) = eval_within(&mut rt, source, LONG);
        assert!(result.is_ok(), "{result:?}");
        assert_clean(&rt);
        assert_eq!(
            run_in(&mut rt, "Date.UTC(275760, 8, 13)"),
            "=> 8640000000000000"
        );
    });
}

/// The ends of the time range in zones with rules and in the zone of the
/// system: every method answers, and a local time beyond the range is NaN.
#[test]
fn ends_of_the_range_in_zones() {
    for zone in [
        "UTC",
        "EST5EDT,M3.2.0,M11.1.0",
        "AEST-10AEDT,M10.1.0,M4.1.0/3",
        "Asia/Kolkata",
        "Pacific/Kiritimati",
        "Pacific/Pago_Pago",
    ] {
        on_big_stack(move || {
            let mut rt = runtime_in(zone, false);
            let source = "var out = []; \
                for (var t of [-8.64e15, -8.64e15 + 1, -8.64e15 + 86400000, -62198755200000, 0, 8.64e15 - 86400000, 8.64e15 - 1, 8.64e15]) { \
                  var d = new Date(t); \
                  out.push(d.toString().length > 20, d.toDateString(), d.toTimeString().length > 10, d.toUTCString(), d.toISOString(), \
                    d.toLocaleString(), d.getTimezoneOffset(), d.getHours(), d.getDate(), d.getFullYear(), d.getYear()); \
                  var c = new Date(d.getFullYear(), d.getMonth(), d.getDate(), d.getHours(), d.getMinutes(), d.getSeconds(), d.getMilliseconds()); \
                  out.push(Math.abs(c.getTime() - t) <= 86400000 || c.getTime() !== c.getTime()); \
                } \
                out.length";
            let (result, elapsed) = eval_with_deadline(&mut rt, source);
            assert!(result.is_ok(), "{zone}: {result:?}");
            assert!(elapsed < DEADLINE + SLACK, "{zone}: took {elapsed:?}");
            assert_clean(&rt);
        });
    }
}

/// Setters whose conversions change the Date, collect garbage, throw or
/// return objects: the result follows the time value read first (ES2025),
/// in the GC stress mode too.
#[test]
fn conversions_that_change_the_date() {
    for stress in [false, true] {
        on_big_stack(move || {
            let mut rt = runtime_in("Europe/Berlin", stress);
            let source = "var names = ['setMilliseconds', 'setSeconds', 'setMinutes', 'setHours', 'setDate', 'setMonth', 'setFullYear', \
                'setUTCMilliseconds', 'setUTCSeconds', 'setUTCMinutes', 'setUTCHours', 'setUTCDate', 'setUTCMonth', 'setUTCFullYear', 'setTime', 'setYear']; \
                var out = []; \
                for (var name of names) { \
                  var d = new Date(2020, 5, 15, 12, 30, 45, 500); \
                  var arg = { valueOf() { gc(); d.setTime(NaN); gc(); return 2; } }; \
                  var r1 = d[name](arg, arg, arg, arg); \
                  var e = new Date(2020, 5, 15, 12, 30, 45, 500); \
                  var thrower = { valueOf() { e.setTime(0); gc(); throw new RangeError(name); } }; \
                  var msg; try { e[name](1, thrower) } catch (x) { msg = x.message } \
                  out.push(r1 === r1 ? 1 : 0, e.getTime() === e.getTime() ? 1 : 0, msg === undefined ? 'none' : msg); \
                } \
                out.join()";
            let (result, _) = eval_with_deadline(&mut rt, source);
            assert!(result.is_ok(), "stress {stress}: {result:?}");
            assert_clean(&rt);
        });
    }
}

/// A new date in a loop collects: the objects are small, the heap does not
/// grow without bound.
#[test]
fn many_dates_are_collected() {
    on_big_stack(|| {
        let mut rt = runtime_in("Europe/Berlin", false);
        let source = "var total = 0; for (var i = 0; i < 200000; i++) { var d = new Date(i * 86400000); total += d.getDate(); d.toString(); } total";
        let (result, _) = eval_within(&mut rt, source, LONG);
        assert!(result.is_ok(), "{result:?}");
        assert_clean(&rt);
        assert!(
            rt.heap().stats().collections > 0,
            "the loop of dates never collected"
        );
    });
}
