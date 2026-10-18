# ADR 0027: `Date` and time zones

- Status: accepted
- Date: 2026-10-18

## Context

M7 feature 12 adds `Date` to the JavaScript engine (ADR 0026 section 11,
section 15). `Date` needs three inputs that the language core does not
have: a calendar over the whole time range of ECMA-262 (±8.64e15 ms,
years -271821 to 275760), the rules of a local time zone, and a clock.
The target scripts use `Date` as follows: `new Date` in 27 files,
`Date.now` in 27, `toISOString` in 11, `toUTCString` in 13, `toGMTString`
in 5 (cookie expiry), the local getters and setters in date formatters,
`getTimezoneOffset` in 4, and `Date.parse` in 2.

Tests need a fixed zone and a fixed clock, or the scores of test262 and
the results of the snippet tests would depend on the machine.

ADR 0003 allows general libraries that solve a problem that also exists
outside browsers. A time zone database and its rules are such a problem.

## Decision

### 1. The library: `jiff`

`jiff` 0.2 (`Unlicense OR MIT`; the author maintains `regex`; 0.2.37 in
`Cargo.lock`), with `default-features = false` and the features `std`,
`tz-system` and `tzdb-zoneinfo`. The workspace dependency is in the root
`Cargo.toml`; only `swb-js` uses it.

Evaluated, with the numbers measured on 2026-10-18 (hello-world programs
with the same lookup, release profile with LTO, `strip`):

| | `jiff` | `tz-rs` |
|---|---|---|
| License | Unlicense OR MIT | MIT OR Apache-2.0 |
| Version, maintenance | 0.2.37 (0.2.38 current); one author with a long record | 0.7.3; one author |
| Dependencies | `jiff-core` (same author); no other crate with these features | none |
| Size added to a program that only looks up an offset | 337 KB | 45 KB |
| Size added to `swb-js` (all of `Date` and `jiff`) | 310 KB of 4.35 MB (stripped) | not measured |
| System zone | `TimeZone::system()` and `try_system()`: the `TZ` variable (a name, a POSIX rule or `:path`), then `/etc/localtime` (a link into the database or a TZif file) | `TimeZone::local()`: the `TZ` variable or `/etc/localtime`; names only as paths below the system directory |
| Zone by name | `TimeZone::get(name)`: reads `/usr/share/zoneinfo` (and `TZDIR`); `Etc/Unknown`; case-insensitive `UTC` | `from_posix_tz(name)` reads a file by name |
| POSIX rules without a database | `TimeZone::posix(rule)` | `from_posix_tz(rule)` |
| Local time that is skipped or repeated | `to_ambiguous_timestamp` gives `Gap`/`Fold` with the offsets before and after the transition | `DateTime::find` with year as `i32`; the rule has to be built from it |
| Abbreviation and DST flag of an instant | `to_offset_info` | `LocalTimeType::time_zone_designation`, `is_dst` |
| Range | civil dates and timestamps for the years -9999 to 9999 | `i64` seconds and `i32` years |
| Without a database | `get` and `try_system` return an error | `local` returns an error |

`jiff` is already in `Cargo.lock` and in the browser binary: `env_logger`
uses it for its timestamps (`humantime`). The new features add no package
(`windows-link` was in the lock already and is used on Windows only).
`jiff` has the API that `Date` needs without a layer of our own (the
offsets before and after a transition, the zone by name and as a rule),
so the code of the zone is 150 lines. `tz-rs` is smaller by 290 KB, but it
is a clone of the libc functions (`localtime`, `mktime`): the UTC time of
a local time that is repeated or skipped has to be built from its search
over the transitions. We took `jiff`; if the size of the browser becomes a
concern, `crates/js/src/date/zone.rs` is the only file that uses it.

### 2. The zone: `crate::date::TimeZone`

A zone answers two questions in integral milliseconds
(`crates/js/src/date/zone.rs`):

- the offset of local time at a UTC time (`LocalTZA(t, true)`,
  §21.4.1.25), seconds included (the local mean time of a zone before the
  1880s has an offset such as -4:56:02 in New York, and `toString` of
  such a date shows the local time with the seconds; measured in Chromium
  148);
- the UTC time of a local time (`UTC(t)`, §21.4.1.26). A local time that
  happens twice (the clock goes back) is the first instant; a time that
  never happens (the clock jumps forward) is interpreted with the offset
  before the transition, so 02:30 on the day of the change to summer time
  is 03:30. Both are the offset *before* the transition (ES2025,
  `GetNamedTimeZoneEpochNanoseconds`), and measured Chromium 148 agrees in
  seven zones (UTC, Berlin, New York, Kolkata, Adelaide, Lord Howe, Apia).

`jiff` handles the years -9999 to 9999. A time outside that range moves by
whole 400-year eras (146,097 days, 20,871 weeks) into the span from year
-7600 to year 9890, and the offset of that time is used. The rules of a
zone depend only on the month, the weekday and the leap year, and a zone
before its first transition has one fixed offset, so the result equals
what ICU gives: Chromium 148 shows summer time in July of the year 270,000
and the local mean time before the first transition (measured for
±275,760 years).

A zone is made by `TimeZone::utc()`, `TimeZone::system()` or
`TimeZone::from_spec(text)`: `UTC`, an IANA name that the database of the
system has, or a POSIX `TZ` rule (`EST5EDT,M3.2.0,M11.1.0`), which needs
no database.

### 3. The system zone, and no database

The zone of the system is looked up once, at the first use of the local
time (a runtime that never touches a `Date` reads no file). The order is
`jiff`'s: the `TZ` variable, then `/etc/localtime`. If `jiff` finds no
zone, the zone is UTC and a `warn!` says so. A name that the database
does not have is `TimeZone::from_spec`'s error (the shell exits with
code 64).

### 4. The host hook

`Runtime::set_time_zone(TimeZone)` and `Runtime::set_clock(Clock)`
(`Clock` is `Box<dyn Fn() -> f64>`: milliseconds since the epoch; the
runtime floors the value and clips it to the time range). The default is
the system clock and the system zone. The `swb-js` shell has
`--time-zone ZONE` (a `TimeZone::from_spec` text) and `--now MS`.

The test262 runner uses one zone, `EST5EDT,M3.2.0,M11.1.0` (the US
Eastern rule: UTC-5, UTC-4 from the second Sunday in March to the first
Sunday in November, applied to every year). A rule needs no database, so
the scores do not depend on the machine; the offset is not 0 and changes
within a year, so the tests of the local time functions see both
offsets and the transitions. The clock of the runner is the system clock.
The snippet tests (`builtins_date.rs`) use the database of the system,
because they measure Chromium's names of real zones; they need a database
that has the zones of `gen-date-cases.js` and fail without one.

### 5. `Date.parse`

The parser (`crates/js/src/date/parse.rs`) reads, in this order, the date
time string format of §21.4.1.32 exactly (date-only forms are UTC,
date-time forms without an offset are local time, expanded years,
`-000000` rejected, a day beyond the month rejected), the output of
`toString` and the output of `toUTCString` (the specification requires
that `Date.parse` reads them back, also for negative and six-digit
years), and last `parse_fallback`, which returns nothing in 12a. Feature
12b puts Chromium's other forms in `parse_fallback`. The parsers read the
string in place and stop at the first unit that does not fit, so a long
string costs no more than a short one.

### 6. `toString` and the zone name

`toString` and `toTimeString` end with the name of the zone in
parentheses. Chromium prints the long English name (`Central European
Summer Time`, `Coordinated Universal Time`); until feature 12b we print
the abbreviation of the database (`CEST`, `UTC`, `+09` for a zone with
no letters). The name comes from `TimeZone::display_name_at_utc`, the
only place that 12b changes.

### 7. `toLocaleString`, `toLocaleDateString`, `toLocaleTimeString`

Without `Intl`, these give Chromium 148's output for en-US and ignore
their arguments: `7/1/2024, 1:05:09 PM`. The space before `AM` and `PM`
is U+0020 in the Chromium of the tools (measured; newer ICU versions use
U+202F). The year of a date before 1 is counted down from 1 (the year -1
shows as 2, as ICU's Gregorian calendar with eras does).

### 8. Differences from Chromium 148 (KNOWN)

Where V8 differs from the specification, `Date` follows the specification
(ADR 0026 section 15 lists the snippets):

- `getTimezoneOffset` is `(t - LocalTime(t)) / msPerMinute`, fractional
  for an offset with seconds (-554.33 in Adelaide before 1895); V8
  truncates to whole minutes.
- The setters read the time value before converting their arguments
  (ES2025); V8's `setFullYear` and `setYear` read it after, so a
  `valueOf` that changes the date changes the result there.
- `Date.parse` rejects `2000-02-30` and the other days beyond the month
  (§21.4.1.32 "illegal values"); V8 reads them as the next month.
- `Date.parse` reads back `toString` and `toUTCString` of the years
  before 100 (negative years, 0000); V8 reads `Sat Jan 01 0000 ...` as
  the year 2000 and fails for a negative year.
- `MakeDay` computes years up to 10^12 away from 1970 exactly; V8 stops at
  10^6 and is a day off at the edge.
- `Date.prototype.toTemporalInstant` does not exist (no `Temporal`).

### 9. What stays for 12b, and the backlog

Feature 12b: the other date forms of Chromium in `parse_fallback`, and the
long zone names in `display_name_at_utc` (the English names of CLDR if
the data is small). Not in M7: the `locales` and `options` arguments of
`toLocaleString` and `Intl.DateTimeFormat` (the targets use
`toLocaleDateString('en-GB', ...)` and `timeZone: 'UTC'`).

## Consequences

- `swb-js` has one more direct dependency, but no new package, and
  grows by 310 KB (stripped).
- Scripts see the zone of the machine by default, as in any browser;
  tests and tools set the zone.
- The snippet tests depend on the system's database (`tzdata`); a
  different version can change an old offset (the local mean time of
  Zagreb differs between Debian's and ICU's data, which is why the
  tests use Berlin).
- `crates/js/src/date/` is independent of the heap and the VM, so a
  later `Intl.DateTimeFormat` can use `TimeZone` and the time arithmetic.
