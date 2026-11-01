# ADR 0027: `Date` and time zones

- Status: accepted
- Date: 2026-10-18 (sections 5a, 6 and 8, the other forms and the zone names: 2026-10-21;
  sections 7, 9 and 10, `Intl`: 2026-11-01)

## Context

M7 feature 12 adds `Date` to the JavaScript engine (ADR 0026 section 11,
section 15). `Date` needs three inputs that the language core does not
have: a calendar over the whole time range of ECMA-262 (±8.64e15 ms,
years -271821 to 275760), the rules of a local time zone, and a clock.
The target scripts use `Date` as follows: `new Date` in 27 files,
`Date.now` in 27, `toISOString` in 11, `toUTCString` in 13, `toGMTString`
in 5 (cookie expiry), the local getters and setters in date formatters,
`getTimezoneOffset` in 4, and `Date.parse` in 2 (one of them with free
text and a trailing `EDT` or `EST`).

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
years), then the variants of ISO 8601 and the written forms (section
5a). The exact parsers read the string in place and stop at the first
unit that does not fit, so a long string costs no more than a short one.
The two readers of section 5a look at each unit of the text at most once
each; the built-in charges the units that they looked at against the time
limit (`charge_units`), so a loop over long texts ends at the deadline.

### 5a. The other forms (feature 12b)

Chromium 148 reads many texts beyond the forms of section 5: the date of
an e-mail, of an HTTP header or of a log, English dates, numeric dates,
and many texts that no standard has. The reader of swb is a set of named
forms. Each form has a published source. Every rule that a source does
not give (separators, optional parts, ranges, two-digit years, a day
past the month end, local time or UTC, what is NaN) was set by a probe in
Chromium 148 (`tools/probes/js-date.json`: the groups `parse-fallback`,
`parse-fallback-more`, `parse-fallback-long`, `parse-forms`,
`parse-free`; also lists of more than 600,000 texts in America/New_York
that are not in the repository). A text that is in no form is NaN. A
first version of this reader (feature 12b, first session) was withdrawn:
its design came from the author's memory of an engine's date parser
(ADR 0025). This version has no such design: the forms and the rules
below come from the sources and the probes named here.

| Form | Source | File |
|---|---|---|
| ISO 8601 with `t`, `z`, a fraction of any length, `+hhmm`, a day up to 31 in any month | ISO 8601-1 (decimal fraction; basic and extended offset), RFC 3339 §5.6 (lower case `t` and `z`), ECMA-262 §21.4.1.32 | `iso_variants.rs` |
| An ISO date with a space, `2024-7-1`, `2024/07/01`, with a clock and a zone | RFC 3339 §5.6 (a space for `T`), then the numeric form below | `written.rs` |
| `Mon, 01 Jul 2024 11:05:09 GMT` and its obsolete syntax: no comma, no weekday, two-digit years, `UT GMT EST EDT CST CDT MST MDT PST PDT`, no seconds, comments | RFC 5322 §3.3, §4.3, §3.2.2 (comments) | `written.rs` |
| `Monday, 01-Jul-24 11:05:09 GMT` | RFC 850 §2.1.2, RFC 1036 §2.1.2 | `written.rs` |
| `Mon Jul  1 11:05:09 2024` (the year after the clock, also with a zone before or after the year) | ISO C `asctime`; the three formats of HTTP dates, RFC 9110 §5.6.7 | `written.rs` |
| `Jul 1, 2024`, `July 1 2024`, `1 Jul 2024`, `Thursday, January 1, 1970`, `2024 Jul 1`, with a clock and a zone | none: measured | `written.rs` |
| `7/1/2024`, `2024/07/01`, `1-2-2003`, `1.2.2003`, `7/1`, with a clock and a zone | none: measured | `written.rs` |
| The en-US output of `toDateString`, `toLocaleString` and `toLocaleDateString`, read back: `7/1/2024, 1:05:09 PM` | the forms above | `written.rs` |
| A clock before the date: `5:06 PM Jan 1 2003` | none: measured | `written.rs` |

What the probes set (all in Chromium 148):

- Month names: any word whose first three letters, in any case, are those
  of a month (`Janx`, `Sept`, `MARCH`). `Ja` is not a month. Weekday
  names are not read: an unknown word before the first number is free
  text (below).
- Fields: month 1 to 12, day 1 to 31 in every month. A day past the end of
  the month rolls over (`Feb 30 2024` is March 1; decision: as Chromium
  does). `Dec 32` is NaN.
- Order: a month name first (`Jul 1 2024`); a number first (`1 Jul
  2024`; a first number above 31 is the year, `2024 Jul 1`); only
  numbers: a first number above 31 is the year (`2024/7/1`, `99-12-31`),
  else month, day, year (`7/1/24`). `1 2 2003` is January 2.
- Separators between the fields of a date: white space, or one of `,` `-`
  `/` `.`, also mixed (`1/2-2003`). Next to a month name no separator is
  needed (`Jan1`, `1Jan2003`). A `-` after white space is a sign: `Jan 1
  -5` is NaN, but `Jan -1 2003` (after a month name that starts the date)
  is read. A separator at the end is NaN.
- Years: 0 to 49 mean 2000 to 2049, 50 to 99 mean 1950 to 1999, whatever
  the number of digits (`0050` is 1950); 100 and above are the year
  itself, also with three digits (RFC 5322 §4.3 adds 1900 to a three-digit
  year, Chromium does not: `124` is the year 124). A date without a year
  has the year 2001 (`Jul 1`). The expanded year (a sign and six digits)
  is read only at the very start of the text: a negative one is the year
  as written (`-000001`), a positive one takes the rule above (`+000050`
  is 1950, `+002024` is 2024); `-000000` is NaN. After free text, white
  space or a comment Chromium ignores the sign (` -000001 01` is January
  1, 2001); swb gives NaN there.
- Free text: in front of the first number, every word except a month
  name (weekday names, `Date:`, `foo`, also the zone names, `AM`, `PM` and
  `T`) and the characters `. , : ; )` are skipped; `GMT Jul 1 2024` is
  local time. A word is a run of ASCII letters and of non-ASCII units that
  are not white space: `é Jul 1 2024` and `Āx Jul 1 2024` are read, but
  `ĀJul 1 2024` is one unknown word and no month (NaN). A word that
  touches a number (`x1`, `Ā1`) is NaN, and so is an unknown word after
  the date.
- Comments `(...)` nest and count as white space. One that is not closed
  runs to the end. A lone `)` is NaN. A comment that touches the last
  number of a clock (`5:06(x)`) is NaN. A NUL unit ends the text. White
  space is that of ECMAScript without U+2028 and U+2029.
- Clock: `h:mm`, `h:mm:ss`, `h:mm:ss.f`; one or more digits for each
  number (`5:6`, `005:06`); hour 0 to 24 (24 only with 0 minutes, 0 seconds
  and 0 ms), minute and second 0 to 59. The fraction has one or more
  digits and is cut to milliseconds, not rounded; the dot and its digits
  must touch the seconds. `AM` and `PM` (any case) follow after white
  space; the hour is 0 to 12 (`12 AM` is 0, `0:30 PM` is 12:30, `13:00
  PM` is NaN); `5:06PM`, `5pm`, `5 PM` and `p.m.` are NaN. The clock
  follows the date after white space, or after `,` or `;`. For `asctime`
  the year follows the clock. A clock that comes first is followed by
  white space and the date.
- Zones: `Z`, `UT`, `UTC`, `GMT`, then an offset directly or after white
  space (`GMT+0200`, `GMT +02:00`); `EST EDT CST CDT MST MDT PST PDT`, any
  case. An offset alone is read only after a clock (`5:06 +0200`,
  `5:06+2`): `h`, `hh`, `hmm`, `hhmm`, `h:mm`, `hh:mm`. After a clock only
  `Z` may touch it (`5:06EST` is NaN). A sign needs its digits next to it
  (`+ 0100` is NaN). Military letters other than `Z` are NaN (RFC 5322
  §4.3 reads them as -0000), and so are `CET`, `BST` and `IST`.
- Local time or UTC: a text without a zone is local time, also a date alone
  (`Jul 1 2024`); with a zone the time is that of the zone. The exact ISO
  date alone is UTC (section 5); an ISO date alone with a day past the
  month end (`2000-02-30`) is also UTC, with the year as written.
- ISO variants: with `T` or `t` the shape is strict: two digits for each
  field, no space, a zone only `Z`, `z`, `+hh:mm` or `+hhmm` (hours 0 to
  23, minutes 0 to 59); the year is as written (`0050` is the year 50).
  A date with `Z` and no time (`2000-01-01Z`) takes the rule for years
  above (`0050-01-01Z` is 1950).

The cost is linear in the length of the text and no memory grows with
it. The reader of the ISO variants stops at the first unit that does not
fit. The written forms cut the text into tokens in one pass: white space
and comments are skipped, free text is skipped without being stored, and
only the first 32 tokens after it are kept, in an array of fixed size (a
text with more tokens is NaN). The forms then read these tokens once. A
text of 100 million units costs one pass (release build: 138 ms for
digits, 29 ms for words), and the built-in charges the units that were
looked at.

Not read (NaN; section 8 counts them): a number alone (`Date.parse("5")`
is May 2001 in Chromium), offsets above 23:59, minute or second 60 in a
clock, a stray number after the date, two zones, a zone before the
clock, a sign before the year without six digits, and the many readings
of random mixes of tokens. None of them is in a standard, and the target
pages do not use them.

### 6. `toString` and the zone name

`toString` and `toTimeString` end with the long English name of the zone
in parentheses, as Chromium prints it (`Central European Summer Time`,
`Coordinated Universal Time`, `India Standard Time`, `British Summer
Time`, `GMT+01:00`). `TimeZone::display_name_at_utc`
(`crates/js/src/date/zone.rs`) is the one place; the names are in
`crates/js/src/date/zone_names.rs` over the generated table
`zone_names_data.rs`.

The rules are those of UTS #35 part 4 ("Using Time Zone Names") and
Chromium's behavior, measured over the 484 zones of the system database
at 18 instants each (`names-cmp`: 8,000 results equal; the others differ
by the data of the database, see section 8):

1. The zone name maps to the identifier of CLDR (`Asia/Kolkata` to
   `Asia/Calcutta`) through the aliases of `bcp47/timezone.xml`.
2. A name of the zone itself wins (CLDR has three: `Etc/UTC` "Coordinated
   Universal Time", `Europe/London` daylight "British Summer Time",
   `Europe/Dublin` daylight "Irish Standard Time").
3. Else the standard or the daylight name of the zone's metazone. The
   metazone is the one in force **now**, not at the printed date:
   Chromium asks ICU for the name of the zone without a date (the name of
   `Asia/Almaty` in 2005 is "Kazakhstan Time", the name since 2024; for
   zones whose last metazone interval ended, such as `Africa/Casablanca`,
   there is none). The table has one metazone per zone name.
4. Without a name (no metazone, or a daylight name that CLDR does not
   have, as for `Antarctica/Troll`, `Africa/Windhoek`), the localized GMT
   format `GMT+01:00` of the offset of the present rules: the lower of
   the offsets in January and July 2025 plus, in daylight saving time,
   their difference. A zone of the database that CLDR does not have
   (`Etc/GMT+5`) gets the same.
5. Whether the zone is in daylight saving time is decided at the time
   that Chromium uses: the time itself from 1970 to 2038-01-19, else the
   same month, day and time of day (UTC) in an equivalent year (same
   weekday of January 1, same leap year; ES5.1 §15.9.1.8 allowed this
   mapping). The year of each of the 14 kinds is a table in `zone.rs`
   (`EQUIVALENT_YEARS`), the result of a measurement and not a formula:
   common years that start on Sunday to Saturday take 2023, 2035, 2019,
   2031, 2015, 2027, 2011; leap years take 2012, 2024, 2008, 2020, 2032,
   2016, 2028. Measured with seven zones whose rules change from year to
   year (Cairo, Sao Paulo, Tehran, Amman, Santiago, Fiji, Casablanca):
   each of the 133 years from 1900 to 1969 and from 2038 to 2100 has the
   names of exactly one year of 1970 to 2037 on 122 days of the year in
   all seven zones, and it is the same year for a kind before 1970 and
   after 2037 (snippet group `names-years`, 196 results). The start of the
   range is exact: Adelaide is in summer time at -1 ms (it maps to 2031)
   and not at 0 ms (`names-edge-1970`). The end is bracketed: Casablanca
   has the real Ramadan of 2037 (from October) and the Ramadan of 2027 in
   February 2038 (`names-edge`). No zone with year-dependent rules has a
   change near 2038-01-19, so 2^31 seconds is not proved, but it fits.
   Cairo in July 1000 is "Eastern European Summer Time", as the rules of
   its equivalent year say; Algiers in July 1945 is "Central European
   Standard Time". `Europe/Dublin`, `Africa/Casablanca`,
   `Africa/El_Aaiun` and `Africa/Windhoek` (1994 to 2017) are in the
   database with a negative saving; ICU has them with the summer time as
   the saving time, and `zone.rs` converts that (`Rearguard`).

A zone that is a POSIX rule (`EST5EDT,M3.2.0,M11.1.0`, the zone of the
test262 runner) or a file without a name keeps the abbreviation of the
rule (`EDT`): it has no name to look up.

**Data.** `swbtools js-time-zone-names` (`tools/swbtools/js_time_zone_names.py`)
downloads `cldr-common-48.zip` from unicode.org into `out/cldr/48/` and
writes `zone_names_data.rs` from `common/main/en.xml` (the long names of
191 metazones and the three zone names), `common/supplemental/metaZones.xml`
(the metazone of each zone) and `common/bcp47/timezone.xml` (573 zone
names with their aliases). CLDR 48, 48.1 and 48.2 have the same names and
mappings for these files; Chromium 148 matches them (the names "Türkiye",
"Khovd", "West Africa Time" are in 48 and not in 47). The data is
19.4 KB (strings 14 KB, tables 5.4 KB). License: the Unicode License v3
(`THIRD_PARTY_NOTICES.md`, `docs/credits.md`).

### 7. `toLocaleString`, `toLocaleDateString`, `toLocaleTimeString`

Without arguments, these give Chromium 148's output for en-US:
`7/1/2024, 1:05:09 PM` (the fast path of M7, kept). With a `locales` or
`options` argument they use `Intl.DateTimeFormat` (section 10, ECMA-402
§20.4). The space before `AM` and `PM` is U+0020 in the Chromium of the
tools (measured; newer ICU versions use U+202F). The year of a date before 1 is counted down from 1 (the year -1
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
- `Date.parse` reads back `toString` and `toUTCString` of the years
  before 100 (negative years, 0000); V8 reads `Sat Jan 01 0000 ...` as
  the year 2000 and fails for a negative year.
- `MakeDay` computes years up to 10^12 away from 1970 exactly; V8 stops at
  10^6 and is a day off at the edge.
- `Date.prototype.toTemporalInstant` does not exist (no `Temporal`).
- `Date.parse` of a text outside the forms of section 5a is NaN, where
  Chromium reads a value (feature 12b). 501 snippets of the groups
  `parse-fallback*` and `parse-forms` (and 27 of `parse-free`, such as a
  word after a month name, `_` and `[`, a sign after free text) are
  marked `!known` in six classes
  (the number of snippets, each text counted once): a number alone or
  numbers, junk and punctuation with no month name, date separator or
  clock, where Chromium reads month, day or year (`"5"`, `"100"`, `"1999::"`;
  214, of which 7 are conversions of such a text); fields that Chromium
  carries over: minute or second 60, an offset above 23:59 or with
  minutes above 59 (128); a sign in front of the year that is not a
  six-digit expanded year at the start, or a separator at the end (31); an hour with a
  colon and no minutes (`5:`, `:5`; 20); extra tokens that Chromium
  skips or takes: a stray number or word after the date, two zones, a
  second year, a zone before the clock, AM or PM glued to the clock (57);
  other readings of random token mixes (51). Lists of more than 600,000
  texts (three lists of random token mixes and of the forms, and
  mutations of them) were compared with Chromium: no text that swb reads
  gives another value than Chromium, and none gives a value where
  Chromium has NaN (one exception: the `toString` comment, below). The
  other direction, NaN where Chromium has a value, is the accepted cost
  of the reader.
- A comment in the `toString` form (`GMT+0500 (a)b)`) is read as long as
  it ends with `)`; Chromium reads only balanced parentheses (backlog).

Where the zone names differ because the database differs (no snippet
covers these; the checks were made against the system's `tzdata`):

- A zone whose offsets differ between the system database and ICU's
  (local mean times before 1900, `Africa/El_Aaiun` before 1976) shows the
  offset of the database; the name follows the rules above.
- `America/Vancouver` and other zones whose rules changed after ICU's
  data: the saving flag comes from the system database.
- ICU's rule form of a negative saving is applied to the four zones of
  section 6 only; another zone with a negative saving in the database
  (none is known to differ in a name) uses the flag of the database.
- A zone whose rules in 2008 to 2035 differ between the two databases
  differs in the name for dates before 1970 and after 2038.
- The 1984 and 1985 names of `Africa/Casablanca` (the database has the
  rules of those years with summer time where ICU has none).

### 9. The backlog

Done in M9 feature 3 (section 10): the `locales` and `options` arguments
of `toLocaleString` and its two relatives, `Intl.DateTimeFormat` and
`Intl.DateTimeFormat().resolvedOptions().timeZone`. Still open: the zone
names of other locales (only `en` and `en-GB`) and the generic zone names.

### 10. `Intl` (M9 feature 3)

`Intl` has `DateTimeFormat`, `NumberFormat`, `RelativeTimeFormat` and
`getCanonicalLocales`, for the locales `en`, `en-US` and `en-GB`. The core
is in `crates/js/src/intl/` (no heap, no interpreter), the objects in
`crates/js/src/builtins/intl*.rs`. The rest of ECMA-402 is not there:
`Collator` (`localeCompare` stays as it is), `PluralRules`, `ListFormat`,
`Locale`, `DisplayNames`, `Segmenter`, `supportedValuesOf`, `formatRange`
and `formatRangeToParts`.

**What the pages need.** The scripts of the six JavaScript fixtures call
`Intl` as follows. Ars Technica: `toLocaleTimeString(undefined, {hour,
minute: "numeric"})` and `toLocaleDateString(undefined, {year, month
numeric/short/long, day})` (the story times), and `new
Intl.DateTimeFormat().resolvedOptions().timeZone` (Google Tag Manager,
Snowplow, jstimezonedetect). BBC: `toLocaleDateString("en-GB", {day, month
short/long, year, timeZone: "UTC"})`, `toLocaleTimeString("en-GB", {hour,
minute: "2-digit", hour12, timeZone, timeZoneName: "short"})`,
`toLocaleDateString("en-US", {month: "long", timeZone: "UTC"})`,
`toLocaleDateString(undefined, {weekday: "short"})`, `new
Intl.NumberFormat("en-US", {style: "unit", unit: "minute", unitDisplay:
"long", maximumFractionDigits: 0})`, `new Intl.RelativeTimeFormat("en",
{numeric: "auto"})` and `resolvedOptions().timeZone`. Craigslist (the
legacy path): `Intl.DateTimeFormat(locale, {weekday | month:
narrow/short/long})`, `{month: "short", day}`, `{weekday, month, day}`,
`new Intl.NumberFormat(locale)`, `Intl.RelativeTimeFormat`, the
`toLocale*String` forms of the same options, and the formatjs polyfills
(`Intl.PluralRules`, `Intl.Locale`, `Intl.DisplayNames`, `Intl.Collator`
are used behind `try` blocks and feature tests). Wikipedia: `Intl.Collator`
(guarded by `window.Intl && Intl.Collator`). Hacker News and senko.net:
none. Every option set of these calls is in `tools/probes/js-intl.json`.

**Decision: no ICU and no ICU4X.** Three locales and a few patterns do not
justify a library of the size of a megabyte: all of `Intl` (code and data)
adds 296 KB to `swb-js` (stripped). The data comes from CLDR 48 (the
version of the zone names) by `swbtools js-intl-data`
(`tools/swbtools/js_intl_data.py`) into the generated
`crates/js/src/intl/data.rs`. For `en-US` and `en-GB` (the inheritance
`en_GB`, `en_001`, `en` is resolved when the file is written) it has the
names of months, weekdays, eras and day periods, the standard date and time
patterns with their skeletons, `availableFormats`, `appendItems`, the glue
patterns, the short zone names, the symbols of numbers, 22 currencies, the
fraction digits of all currencies (59 codes that differ from 2, from
`supplementalData.xml`), the 45 sanctioned simple units and a few compound
ones, and the relative-time patterns, and a table of 280 zone aliases. It is 2,964 strings (22 KB of
text, 11 KB distinct) in 52 KB of source. License: the Unicode License v3
(the same files as section 6, with `en_001.xml`, `en_GB.xml` and
`root.xml`).

**Locale negotiation.** `en` and `en-US` resolve to `en` and `en-US` with
the `en-US` data, `en-GB` to `en-GB`; a tag with a Unicode extension
resolves without it (but `-u-hc-` sets the hour cycle). Every other locale
resolves to the default, `en-US`, as the negotiation does with only these
locales available. Chromium 148 has more (`de` gives `de`, `en-AU` gives
`en-AU` with the patterns of `en-AU`); `supportedLocalesOf` lists only the
three. This is the largest difference (departure 1).

**`Intl.DateTimeFormat`.** The options are read in the order of ECMA-402
(a probe with getters shows Chromium's order). The pattern comes from the
matching of skeletons of UTS #35 part 4 (`pattern.rs`): a request is a set
of fields with widths; the pattern of the data whose skeleton has exactly
the fields of the request and the nearest widths is chosen and adjusted to
the request. The rules were set by the measurement of 256 option sets at
five instants in `en-US` and `en-GB` (the group `matrix` of the probe file;
253 equal, the others are departures 3 and 7):

- A field keeps the width of the pattern when the request and the skeleton
  of the data agree (`dd/MM/y` serves `yMd` in `en-GB`: `01/07/2024`), and
  takes the width of the request when they differ (`yMMdd` in `en-US` is
  `MM/dd/y`; `hhmm` is `hh:mm a`).
- In the distance, a width difference of the hour counts half of one of
  another field (measured: in `en-GB`, which has both, `HHm` takes `Hm` and
  `Hmm` takes `HHmm`); a number against a name is far; a 12-hour clock
  against a 24-hour one is no match; the width of a zone field does not
  count.
- The standard date and time patterns of the locale are skeletons too (that
  is how `en-US` has `EEEE, MMMM d, y`).
- A request without such a skeleton is split into a date and a time and
  joined by the glue pattern of the locale. The pattern for a time of day
  is used for every join (`July 1 at 1:05 PM`, `Jul 1, 1:05 PM`: the medium
  and short patterns of `en` have the comma). A part that has no skeleton
  is served by the largest smaller skeleton with the missing field appended
  by `appendItems` (`{year, day}` gives `2024 (day: 1)`).
- `dateStyle` and `timeStyle` use the standard patterns; with `hour12` or
  `hourCycle` the time pattern is found again for its skeleton with the
  other hour letter (`en-GB` with `hour12: true` gives `01:05 pm`).
- The space before the day period is U+0020 (the `alt="ascii"` patterns of
  CLDR). No result of Chromium 148 in the 256 option sets, the styles or
  the joined patterns has U+202F. The day periods of `en-GB` are `am` and
  `pm`.
- `fractionalSecondDigits` appends `.SSS` to the seconds, and seconds are
  added to a request with minutes or an hour (`05:07.00`). Without an
  hour, minute or second the fraction is a time part of its own, joined to
  the date like a time (`1/1/1970, 000`, `January 1 at 000`) with the zone
  name after it (`000 UTC`). A minute or a second alone is not padded
  (`2-digit` gives `5`).
- `timeZoneName`: `short` is the short name of CLDR for the locale and the
  zone (`EST`; `BST` in `en-GB`; none for `Europe/Berlin` in `en-US`), else
  the GMT format of the offset at the time (`GMT+1`, `GMT+5:30`); `long` is
  the name of section 6; `shortOffset` and `longOffset` are `GMT+1` and
  `GMT+01:00` (UTC gives `GMT+0` and `GMT+00:00`, as measured). A zone given
  as an offset (`+05:30`) is shown as the offset.
- `timeZone`: `UTC`, `GMT`, `Etc/UTC`, `Etc/GMT`, `Zulu`, `UCT`, `Etc/UCT`,
  `Etc/Zulu`, `Universal`, `Etc/Universal`, `GMT0`, `Etc/GMT0`, `Etc/GMT+0`,
  `Etc/GMT-0`, `GMT+0` and `GMT-0` are the zone UTC and give `UTC`;
  `Greenwich` and `Etc/Greenwich` give `UTC` too but keep the names of
  Greenwich Mean Time (measured, with one probe snippet for each link of
  the IANA database that is a name of UTC); `Etc/Unknown` is a `RangeError`.
  An identifier in any case is canonicalized to the identifier of CLDR
  (`Asia/Kolkata` gives `Asia/Calcutta`, `US/Eastern` gives
  `America/New_York`: the aliases of `bcp47/timezone.xml`); an offset
  `+hh:mm`, `+hhmm` or `+hh` is a zone (the offset zero, also written
  `-0000`, is `+00:00` and shows the names of Greenwich Mean Time). A name
  that the database of the system does not have is a `RangeError` ("Invalid
  time zone specified: X"); a few old names that a database without the
  links lacks fall back to the current identifier (`Asia/Calcutta` to
  `Asia/Kolkata`). A POSIX rule or a name with spaces is not a zone here.
  The zone is checked right after its option is read (before the
  components), as in ECMA-402 and Chromium.
- `resolvedOptions()` gives `locale`, `calendar` (`gregory`),
  `numberingSystem` (`latn`), `timeZone`, `hourCycle` and `hour12` when the
  pattern has an hour, and the components of the pattern (or the styles).
  `format` is a getter that returns the same bound function each time
  (`[[BoundFormat]]` is in the object and traced by the collector).
- `format(undefined)` is the current time; a value that is not a time value
  is a `RangeError` ("Invalid time value").

**`Intl.NumberFormat`.** The styles `decimal`, `percent`, `currency` (symbols
and names of 22 currencies; another code shows the code; the fraction
digits of every code are the ISO 4217 minor units of CLDR
`currencyData/fractions`: `KWD` 3, `CLP` 0, `CLF` 4, 59 codes differ from
2) and `unit`; `minimumIntegerDigits`, the fraction and significant digit
options (ranges and errors as in Chromium: "maximumFractionDigits value is
out of range."), `useGrouping` (`auto`, `always`, `min2`, `false`),
`signDisplay`, `currencyDisplay`, `currencySign` and `unitDisplay`. The
options are read and checked in the order of ECMA-402 (`numberingSystem`,
`style`, `currency`, `currencyDisplay`, `currencySign`, `unit`,
`unitDisplay`; each check right after the read); `unit` is checked for
every style. The plural category of a unit or a currency name is decided
by the value (`01 minute`), not by the padded digits.

A number is kept as decimal digits (a double as its shortest round-trip
digits, a `BigInt` exactly) and rounded half away from zero, so `1.005`
with two fraction digits is `1.01` as in Chromium. A numeric string is read
in place, without a copy, as the number string of `ToIntlMathematicalValue`
the way Chromium reads it (measured): a value whose `Number` is infinite
(`1e400`, a string of 400 digits, `1e1000000000`) is the infinity, a zero
with any exponent is zero, and only the first 1,024 significant digits are
kept (they decide every rounding, because a value below 10^309 shows at
most 309 places before and 100 after the point). So the length of a string
costs time to read but no memory. The text has at most 2^20 digits (a
`RangeError`, "Invalid string length", above; Chromium's limit is the
string limit, 2^29): this limit applies to a `BigInt` and to a tiny number
with significant digits (`1e-9999999`); the work is charged against the
time limit. `Number.prototype.toLocaleString` and
`BigInt.prototype.toLocaleString` with arguments use it; without arguments
the fast path of M7 stays.

**`Intl.RelativeTimeFormat`.** `style` and `numeric`; the patterns of the
units second to year; `numeric: "auto"` uses the phrases of the data
(`yesterday`, `this minute`) when the value rounded to two fraction digits
is -1, 0 or 1 (measured: 0.001 is `today`, 0.01 is `in 0.01 days`, 0.9999
is `tomorrow`); a negative zero is in the past (`0 days ago`). A narrow
field without an item takes the short one, and a short field the long one
(the aliases of the fields in CLDR's `root.xml`).

**Departures from Chromium 148.** Each snippet of them is marked `!known`
in `crates/js/tests/data/intl-snippets.txt` (81 of 1,206 snippets):

1. Locales other than `en`, `en-US` and `en-GB` resolve to `en-US`
   (`de-DE` gives `1,234.5`); `supportedLocalesOf` and `resolvedOptions`
   report that; a tag with extensions resolves without them.
2. Calendars and numbering systems other than `gregory` and `latn`: the
   options are validated and ignored.
3. `dayPeriod` is read and checked and has no effect (`in the afternoon`
   needs the flexible day periods of CLDR, which are not in the data). It
   does not count as a time field: `{dayPeriod: "long"}` alone gives the
   default date and time (Chromium: `in the evening`), and with an hour the
   text has `AM`/`PM`. A fraction of a second next to an hour but no
   minute is `12 AM (second: 0.000)` where Chromium prints an internal
   marker (`12 AM ├F14: 000┤`).
4. `timeZoneName: "shortGeneric"` and `"longGeneric"` give the offset
   (`GMT+1`), where Chromium has `ET`, `India Time`, `United Kingdom Time`.
5. `notation` other than `standard` (compact, scientific, engineering),
   `compactDisplay`, `roundingMode`, `roundingIncrement`, `roundingPriority`
   and `trailingZeroDisplay` are read and have no effect; the resolved
   values are the defaults.
6. A unit that the table has no pattern for (a compound unit such as
   `meter-per-acre`) shows its identifier (`1,500 meter-per-acre`).
7. `hourCycle: "h24"` in `en-GB` prints `09:30` where Chromium prints
   `9:30`.
8. `formatRange`, `formatRangeToParts`, `Intl.supportedValuesOf` and the
   other constructors do not exist; `Array.prototype.toLocaleString` does
   not pass `locales` and `options` on to the elements (ECMA-402 §19.5.1;
   the test262 tests of ECMA-262 require no arguments).
9. `getCanonicalLocales` rejects a repeated singleton (`en-a-bb-a-cc`) and
   a repeated variant (`en-1996-1996`), but it does not remove a repeated
   keyword of a Unicode extension (`-u-ca-gregory-ca-buddhist`), does not
   sort extensions, keywords or variants (`en-u-hc-h23-ca-gregory`,
   `en-fonipa-1996`), does not drop the value `true` (`-u-kn-true`), and
   applies no language or region aliases (`en-us-posix`, `iw`, `eng`).
10. The text of a `BigInt` or of a tiny number with significant digits that
    has more than 2^20 digits is a `RangeError`. A number string with an
    exponent beyond 2^31 (`0e99999999999`, `1e-99999999999`) is read as
    zero or the infinity where Chromium throws "Internal error. Icu error."
11. Currencies other than the 22 of the data show their code instead of the
    symbol and the name (`KWD 1.000`, `1.000 KWD`; Chromium: `1.000 Kuwaiti
    dinars`, `₫` for `VND`); their fraction digits are right.

**Craigslist.** The page takes its legacy path because `window.SharedWorker`
is missing (`cl.oldBrowser`), not because of `Intl`. With the `Intl` of
this section its polyfill tests (`getCanonicalLocales("und-x-private")`,
the `1E4 bits` test of `NumberFormat`) find the native objects incomplete
and load the polyfills as before; the page, its boxes and its scores equal
those of the run without `Intl` (`compare --js craigslist-posting` with
`--no-shared-worker --no-intl`: geometry 0.9804, pixels 0.9976, before and
after), and `craigslist_posting.rs` passes.

**Tests.** `tools/probes/js-intl.json` (1,206 snippets in 32 cases, three
equal Chromium runs) and `crates/js/tests/builtins_intl.rs` (the same
snippets in six zones, also in the GC stress mode); unit tests of the
patterns, numbers, relative times and tags in `crates/js/src/intl/`;
`tools/tests/test_js_intl_data.py`; the Ars Technica test (the story
times: `data/ars-technica.js`, `ars-technica-chromium.json`); the test262
groups `test/intl402/*` (`crates/js/test262/scores.json`); and three
hostile cases (`tools/swbtools/hostile_cases.py`:
`intl-formatters-loop-js-on`, `intl-number-digits-js-on`,
`intl-locale-lists-js-on`).

## Consequences

- `swb-js` has one more direct dependency, but no new package, and
  grows by 310 KB (stripped); feature 12b adds 56 KB (4.66 MB to 4.72 MB:
  19 KB of CLDR data, the zone names, the two readers of section 5a).
- Scripts see the zone of the machine by default, as in any browser;
  tests and tools set the zone.
- The snippet tests depend on the system's database (`tzdata`); a
  different version can change an old offset (the local mean time of
  Zagreb differs between Debian's and ICU's data, which is why the
  tests use Berlin).
- `crates/js/src/date/` is independent of the heap and the VM;
  `Intl.DateTimeFormat` (section 10) uses `TimeZone` and the time
  arithmetic.
- `Intl` adds 296 KB to `swb-js` (stripped: 5.20 to 5.50 MB); the data
  (section 10) is about 50 KB of it.
