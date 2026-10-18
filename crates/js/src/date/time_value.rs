//! Time values and their operations (ECMA-262 §21.4.1,
//! <https://tc39.es/ecma262/#sec-time-values-and-time-range>).
//!
//! A time value is a Number of milliseconds since 1970-01-01T00:00:00Z,
//! integral and within ±8.64e15, or NaN. The calendar functions work on
//! integers (`i64`) and are exact over the whole range; `MakeDay`,
//! `MakeTime` and `MakeDate` take the Numbers of a script (also huge and
//! non-finite ones) and follow the IEEE 754 evaluation order of the
//! specification. The days-from-civil conversion is the proleptic
//! Gregorian calendar in 400-year eras (146,097 days), a standard
//! algorithm (docs/credits.md).

/// Milliseconds in a day (`msPerDay`).
pub(crate) const MS_PER_DAY: f64 = 86_400_000.0;
/// Milliseconds in an hour, a minute and a second.
const MS_PER_HOUR: f64 = 3_600_000.0;
const MS_PER_MINUTE: f64 = 60_000.0;
const MS_PER_SECOND: f64 = 1_000.0;

/// The largest absolute time value (§21.4.1.1, 100,000,000 days).
pub(crate) const MAX_TIME_VALUE: f64 = 8.64e15;

/// The most years from 1970 that `MakeDay` computes (§21.4.1.28 "if not
/// possible, return NaN"): 10^12 years are 3.65e14 days, still exact in a
/// Number. V8 stops at 10^6 years; the difference shows only for a date
/// argument that brings such a year back into the range.
const MAX_MAKE_DAY_YEAR: f64 = 1e12;

/// The length of a Gregorian era of 400 years in days; it has a whole
/// number of weeks (20,871), so a year in a later era has the same
/// calendar as the year 400 years before.
pub(crate) const DAYS_PER_ERA: i64 = 146_097;

/// The calendar and clock fields of a time value (or of a local time).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Fields {
    /// `YearFromTime`.
    pub(crate) year: i64,
    /// `MonthFromTime`, 0 to 11.
    pub(crate) month: u8,
    /// `DateFromTime`, 1 to 31.
    pub(crate) day: u8,
    /// `WeekDay`, 0 (Sunday) to 6.
    pub(crate) weekday: u8,
    /// `HourFromTime`.
    pub(crate) hour: u8,
    /// `MinFromTime`.
    pub(crate) minute: u8,
    /// `SecFromTime`.
    pub(crate) second: u8,
    /// `msFromTime`.
    pub(crate) ms: u16,
}

/// `Day(t)` (§21.4.1.3) of an integral time.
pub(crate) fn day_of(t: i64) -> i64 {
    t.div_euclid(86_400_000)
}

/// `TimeWithinDay(t)` (§21.4.1.3) of an integral time.
pub(crate) fn time_within_day(t: i64) -> i64 {
    t.rem_euclid(86_400_000)
}

/// Whether `year` is a leap year (§21.4.1.4, `DaysInYear`).
pub(crate) fn is_leap_year(year: i64) -> bool {
    year.rem_euclid(4) == 0 && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0)
}

/// The number of days in `month` (1 to 12) of `year`.
pub(crate) fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if is_leap_year(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// The number of days from 1970-01-01 to the first of `month` (1 to 12) of
/// `year` plus `day - 1` (`DayFromYear` and the days of the months,
/// §21.4.1.4 to §21.4.1.5).
pub(crate) fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let year_of_era = y - era * 400;
    let month_from_march = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_from_march + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * DAYS_PER_ERA + day_of_era - 719_468
}

/// The year, month (1 to 12) and day of the month of a day number (days
/// since 1970-01-01): `YearFromTime`, `MonthFromTime` and `DateFromTime`.
fn civil_from_days(days: i64) -> (i64, u8, u8) {
    let z = days + 719_468;
    let era = z.div_euclid(DAYS_PER_ERA);
    let day_of_era = z - era * DAYS_PER_ERA;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    // `month` is 1 to 12 and `day` 1 to 31 by construction.
    (year, month as u8, day as u8)
}

/// The fields of an integral time value (or local time) `t`.
pub(crate) fn fields(t: i64) -> Fields {
    let days = day_of(t);
    let (year, month, day) = civil_from_days(days);
    let within = time_within_day(t);
    Fields {
        year,
        month: month - 1,
        day,
        weekday: (days + 4).rem_euclid(7) as u8,
        hour: (within / 3_600_000) as u8,
        minute: (within / 60_000 % 60) as u8,
        second: (within / 1_000 % 60) as u8,
        ms: (within % 1_000) as u16,
    }
}

/// `ToIntegerOrInfinity` of a Number (§7.1.5), as a Number.
fn integer(x: f64) -> f64 {
    if x.is_nan() { 0.0 } else { x.trunc() + 0.0 }
}

/// `MakeTime(hour, min, sec, ms)` (§21.4.1.27): NaN if an argument is not
/// finite. The sums evaluate left to right in IEEE 754 arithmetic.
pub(crate) fn make_time(hour: f64, min: f64, sec: f64, ms: f64) -> f64 {
    if !(hour.is_finite() && min.is_finite() && sec.is_finite() && ms.is_finite()) {
        return f64::NAN;
    }
    let (h, m, s, milli) = (integer(hour), integer(min), integer(sec), integer(ms));
    ((h * MS_PER_HOUR + m * MS_PER_MINUTE) + s * MS_PER_SECOND) + milli
}

/// `MakeDay(year, month, date)` (§21.4.1.28): the day number of the date,
/// NaN if an argument is not finite or the year is out of reach.
pub(crate) fn make_day(year: f64, month: f64, date: f64) -> f64 {
    if !(year.is_finite() && month.is_finite() && date.is_finite()) {
        return f64::NAN;
    }
    let (y, m, dt) = (integer(year), integer(month), integer(date));
    let month_in_year = m.rem_euclid(12.0);
    let ym = y + (m - month_in_year) / 12.0;
    if !ym.is_finite() || ym.abs() > MAX_MAKE_DAY_YEAR {
        return f64::NAN;
    }
    let first = days_from_civil(ym as i64, month_in_year as i64 + 1, 1);
    first as f64 + dt - 1.0
}

/// `MakeDate(day, time)` (§21.4.1.29).
pub(crate) fn make_date(day: f64, time: f64) -> f64 {
    if !(day.is_finite() && time.is_finite()) {
        return f64::NAN;
    }
    let tv = day * MS_PER_DAY + time;
    if tv.is_finite() { tv } else { f64::NAN }
}

/// `MakeFullYear(year)` (§21.4.1.30): the years 0 to 99 are 1900 to 1999.
pub(crate) fn make_full_year(year: f64) -> f64 {
    if year.is_nan() {
        return f64::NAN;
    }
    let truncated = integer(year);
    if (0.0..=99.0).contains(&truncated) {
        1900.0 + truncated
    } else {
        truncated
    }
}

/// `TimeClip(time)` (§21.4.1.31): NaN outside ±8.64e15, else the integer
/// (never -0).
pub(crate) fn time_clip(time: f64) -> f64 {
    if !time.is_finite() || time.abs() > MAX_TIME_VALUE {
        return f64::NAN;
    }
    integer(time)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_and_leap_days() {
        let f = fields(0);
        assert_eq!((f.year, f.month, f.day, f.weekday), (1970, 0, 1, 4));
        // 2000-02-29T23:59:59.999Z
        let f = fields(951_868_799_999);
        assert_eq!((f.year, f.month, f.day, f.weekday), (2000, 1, 29, 2));
        assert_eq!((f.hour, f.minute, f.second, f.ms), (23, 59, 59, 999));
        // 1900 is not a leap year, 2000 is.
        assert!(!is_leap_year(1900) && is_leap_year(2000) && !is_leap_year(-100));
        assert!(is_leap_year(-400) && is_leap_year(0));
    }

    #[test]
    fn range_ends() {
        let f = fields(8_640_000_000_000_000);
        assert_eq!((f.year, f.month, f.day, f.weekday), (275_760, 8, 13, 6));
        let f = fields(-8_640_000_000_000_000);
        assert_eq!((f.year, f.month, f.day, f.weekday), (-271_821, 3, 20, 2));
        let f = fields(-62_198_755_200_000);
        assert_eq!((f.year, f.month, f.day), (-1, 0, 1));
        let f = fields(-1);
        assert_eq!(
            (f.year, f.month, f.day, f.hour, f.ms),
            (1969, 11, 31, 23, 999)
        );
    }

    #[test]
    fn days_round_trip_over_the_range() {
        for days in (-100_000_000_i64..=100_000_000).step_by(9_973) {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, i64::from(m), i64::from(d)), days);
        }
        // Every day around the era boundaries.
        for days in -800_000..800_000 {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, i64::from(m), i64::from(d)), days);
        }
    }

    #[test]
    fn make_day_edges() {
        assert_eq!(make_day(1970.0, 0.0, 1.0), 0.0);
        assert_eq!(make_day(1970.0, 12.0, 1.0), 365.0);
        assert_eq!(make_day(1970.0, -1.0, 1.0), -31.0);
        assert!(make_day(f64::NAN, 0.0, 1.0).is_nan());
        assert!(make_day(1970.0, f64::INFINITY, 1.0).is_nan());
        assert!(make_day(1e300, 0.0, 1.0).is_nan());
        assert!(make_day(2020.0, 1e300, 1.0).is_nan());
        assert!(make_day(2020.0, 0.0, 1e300).is_finite());
        assert_eq!(make_day(2020.5, 0.9, 1.9), make_day(2020.0, 0.0, 1.0));
        assert_eq!(make_day(275_760.0, 8.0, 13.0) * MS_PER_DAY, MAX_TIME_VALUE);
    }

    #[test]
    fn make_time_and_date() {
        assert_eq!(make_time(1.0, 2.0, 3.0, 4.0), 3_723_004.0);
        assert!(make_time(1.0, f64::INFINITY, 3.0, 4.0).is_nan());
        assert_eq!(make_time(1.9, -0.0, 0.0, 0.9), 3_600_000.0);
        assert!(make_date(1e305, 1e305).is_nan());
        assert!(make_date(f64::NAN, 0.0).is_nan());
        assert_eq!(make_date(1.0, 5.0), 86_400_005.0);
    }

    #[test]
    fn clip_and_full_year() {
        assert!(time_clip(8.64e15 + 1.0).is_nan());
        assert_eq!(time_clip(8.64e15), 8.64e15);
        assert!(time_clip(f64::INFINITY).is_nan());
        assert!(time_clip(-0.0).is_sign_positive());
        assert_eq!(time_clip(-1.9), -1.0);
        assert_eq!(make_full_year(99.9), 1999.0);
        assert_eq!(make_full_year(100.0), 100.0);
        assert_eq!(make_full_year(-1.0), -1.0);
        assert!(make_full_year(f64::NAN).is_nan());
    }
}
