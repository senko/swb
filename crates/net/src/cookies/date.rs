//! Parsing the cookie date format of the `Expires` attribute.
//!
//! <https://datatracker.ietf.org/doc/html/draft-ietf-httpbis-rfc6265bis#section-5.1.1>

/// Parses a cookie date and returns it in seconds since the Unix epoch
/// (UTC). Returns `None` if the string is not a valid cookie date.
pub(super) fn parse(date: &str) -> Option<i64> {
    let mut time = None;
    let mut day = None;
    let mut month = None;
    let mut year = None;
    // Step 1: split into date tokens.
    for token in date.as_bytes().split(|&b| is_delimiter(b)) {
        if token.is_empty() {
            continue;
        }
        // Step 2: each token sets the first field that it matches.
        if time.is_none()
            && let Some(value) = parse_time(token)
        {
            time = Some(value);
        } else if day.is_none()
            && let Some(value) = leading_digits(token, 1, 2)
        {
            day = Some(value);
        } else if month.is_none()
            && let Some(value) = parse_month(token)
        {
            month = Some(value);
        } else if year.is_none()
            && let Some(value) = leading_digits(token, 2, 4)
        {
            year = Some(value);
        }
    }
    let ((hour, minute, second), day, month, mut year) = (time?, day?, month?, year?);
    // Steps 3 and 4: two-digit years.
    if (70..=99).contains(&year) {
        year += 1900;
    } else if year <= 69 {
        year += 2000;
    }
    // Steps 5 and 6.
    if year < 1601 || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    if day < 1 || day > days_in_month(year, month) {
        return None;
    }
    let days = days_from_civil(i64::from(year), month, day);
    Some(days * 86_400 + i64::from(hour * 3600 + minute * 60 + second))
}

/// `delimiter = %x09 / %x20-2F / %x3B-40 / %x5B-60 / %x7B-7E`
fn is_delimiter(b: u8) -> bool {
    matches!(b, 0x09 | 0x20..=0x2F | 0x3B..=0x40 | 0x5B..=0x60 | 0x7B..=0x7E)
}

/// Matches `min*maxDIGIT [ non-digit *OCTET ]` and returns the number.
fn leading_digits(token: &[u8], min: usize, max: usize) -> Option<u32> {
    let count = token.iter().take_while(|b| b.is_ascii_digit()).count();
    if count < min || count > max {
        return None;
    }
    let digits = token.get(..count)?;
    Some(digits.iter().fold(0, |n, &d| n * 10 + u32::from(d - b'0')))
}

/// Matches `time = hms-time [ non-digit *OCTET ]` with
/// `hms-time = time-field ":" time-field ":" time-field` and
/// `time-field = 1*2DIGIT`.
fn parse_time(token: &[u8]) -> Option<(u32, u32, u32)> {
    let mut rest = token;
    let mut fields = [0; 3];
    for (index, field) in fields.iter_mut().enumerate() {
        let count = rest.iter().take_while(|b| b.is_ascii_digit()).count();
        *field = leading_digits(rest, 1, 2)?;
        rest = rest.get(count..)?;
        if index < 2 {
            rest = rest.strip_prefix(b":")?;
        }
    }
    // After the third field, `leading_digits` has checked that no third
    // digit follows.
    Some((fields[0], fields[1], fields[2]))
}

/// Matches `month = ( "jan" / ... / "dec" ) *OCTET`, case-insensitively.
fn parse_month(token: &[u8]) -> Option<u32> {
    const MONTHS: [&[u8]; 12] = [
        b"jan", b"feb", b"mar", b"apr", b"may", b"jun", b"jul", b"aug", b"sep", b"oct", b"nov",
        b"dec",
    ];
    let prefix = token.get(..3)?;
    let index = MONTHS
        .iter()
        .position(|month| month.eq_ignore_ascii_case(prefix))?;
    Some(index as u32 + 1)
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Returns the number of days from 1970-01-01 to the given date in the
/// proleptic Gregorian calendar. `year` must not be negative.
///
/// The algorithm is `days_from_civil` from
/// <https://howardhinnant.github.io/date_algorithms.html>.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year / 400;
    let year_of_era = year - era * 400;
    let month_from_march = i64::from((month + 9) % 12);
    let day_of_year = (153 * month_from_march + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_formats() {
        // 2015-10-21 07:28:00 UTC
        let expected = Some(1_445_412_480);
        for date in [
            "Wed, 21 Oct 2015 07:28:00 GMT",
            "Wednesday, 21-Oct-15 07:28:00 GMT",
            "Wed Oct 21 07:28:00 2015",
            "21 October 2015 07:28:00",
            "Wed, 21-Oct-2015 07:28:00 UTC",
            "2015 Oct 21 07:28:00",
            "Oct 21st 2015 07:28:00 GMT",
        ] {
            assert_eq!(parse(date), expected, "{date}");
        }
    }

    #[test]
    fn epoch_and_leap_days() {
        assert_eq!(parse("Thu, 01 Jan 1970 00:00:00 GMT"), Some(0));
        assert_eq!(parse("Thu, 01 Jan 1970 00:00:01 GMT"), Some(1));
        assert_eq!(parse("Wed, 31 Dec 1969 23:59:59 GMT"), Some(-1));
        assert_eq!(parse("29 Feb 2000 00:00:00"), Some(951_782_400));
        assert_eq!(parse("29 Feb 1900 00:00:00"), None);
        assert_eq!(parse("29 Feb 2023 00:00:00"), None);
        assert_eq!(parse("31 Apr 2023 00:00:00"), None);
        assert_eq!(parse("01 Jan 1601 00:00:00"), Some(-11_644_473_600));
        assert_eq!(parse("31 Dec 9999 23:59:59"), Some(253_402_300_799));
    }

    #[test]
    fn two_digit_years() {
        assert_eq!(parse("01 Jan 70 00:00:00"), Some(0));
        assert_eq!(parse("01 Jan 69 00:00:00"), parse("01 Jan 2069 00:00:00"));
        assert_eq!(parse("01 Jan 00 00:00:00"), parse("01 Jan 2000 00:00:00"));
    }

    #[test]
    fn invalid_dates() {
        for date in [
            "",
            "garbage",
            "Wed, 21 Oct 2015",
            "Wed, 21 Oct 07:28:00 GMT",
            "21 2015 07:28:00",
            "Wed, 32 Oct 2015 07:28:00 GMT",
            "Wed, 00 Oct 2015 07:28:00 GMT",
            "Wed, 21 Oct 1600 07:28:00 GMT",
            "Wed, 21 Oct 2015 24:00:00 GMT",
            "Wed, 21 Oct 2015 07:60:00 GMT",
            "Wed, 21 Oct 2015 07:28:60 GMT",
            "Wed, 21 Oct 2015 007:28:00 GMT",
            "Wed, 21 Oct 20150 07:28:00 GMT",
            "\u{0}\u{ff}:::",
            "99999999999999999999 Oct 2015 07:28:00",
        ] {
            assert_eq!(parse(date), None, "{date:?}");
        }
    }

    #[test]
    fn first_match_wins() {
        // The time is already found, so the second time-like token matches
        // day-of-month ("10" followed by a non-digit).
        assert_eq!(
            parse("07:28:00 10:00:00 Oct 2015"),
            parse("10 Oct 2015 07:28:00")
        );
        // The day is already found, so "10" is the year 2010.
        assert_eq!(
            parse("21 10 Oct 07:28:00 2015"),
            parse("21 Oct 2010 07:28:00")
        );
    }
}
