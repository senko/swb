//! The types of the locale data of `Intl` (`data.rs` is generated from CLDR
//! 48 by `swbtools js-intl-data`, `tools/swbtools/js_intl_data.py`).
//!
//! The data has two locales, `en-US` and `en-GB`, with the inheritance of
//! CLDR (`en`, `en_001`, `en_GB`) resolved when the file is generated.

/// The widths of a name: wide, abbreviated, narrow.
pub(crate) const WIDTHS: usize = 3;

/// The unit patterns of one width: the plural categories `one` and `other`.
pub(crate) type CountPatterns = [&'static str; 2];

/// The data of one unit of `Intl.NumberFormat` (`style: "unit"`).
#[derive(Debug)]
pub(crate) struct UnitData {
    /// The identifier of ECMA-402 (`kilometer-per-hour`).
    pub(crate) name: &'static str,
    /// The patterns of the long, short and narrow width (`{0} minutes`).
    pub(crate) patterns: [CountPatterns; WIDTHS],
}

/// The data of one currency (its fraction digits are in the table
/// `CURRENCY_DIGITS` of `data.rs`).
#[derive(Debug)]
pub(crate) struct CurrencyData {
    /// The ISO 4217 code.
    pub(crate) code: &'static str,
    /// The symbol in the locale (`US$`), and the narrow symbol (`$`).
    pub(crate) symbol: &'static str,
    /// See `symbol`.
    pub(crate) narrow: &'static str,
    /// The display names for the plural categories `one` and `other`.
    pub(crate) names: [&'static str; 2],
}

/// The data of one unit of `Intl.RelativeTimeFormat` in one width.
#[derive(Debug)]
pub(crate) struct RelativeData {
    /// The phrases for -1, 0 and 1 with `numeric: "auto"` (empty for none).
    pub(crate) auto: [&'static str; 3],
    /// The patterns for a future and a past time, plural categories `one`
    /// and `other`.
    pub(crate) future: CountPatterns,
    /// See `future`.
    pub(crate) past: CountPatterns,
}

/// The symbols and patterns of numbers.
#[derive(Debug)]
pub(crate) struct NumberData {
    /// The decimal separator, the group separator, the percent sign, the
    /// plus sign, the minus sign, the infinity sign and the text of NaN.
    pub(crate) symbols: [&'static str; 7],
    /// The pattern that joins a number and a currency name (`{0} {1}`).
    pub(crate) currency_name: &'static str,
    /// The currencies that the data has.
    pub(crate) currencies: &'static [CurrencyData],
    /// The units that the data has, sorted by name.
    pub(crate) units: &'static [UnitData],
}

/// The relative time data of the units second, minute, hour, day, week,
/// month, quarter, year in the widths long, short and narrow.
pub(crate) type RelativeTable = [[RelativeData; 8]; WIDTHS];

/// The calendar data of a locale.
#[derive(Debug)]
pub(crate) struct CalendarData {
    /// The names of the months (wide, abbreviated, narrow).
    pub(crate) months: [[&'static str; 12]; WIDTHS],
    /// The names of the weekdays, Sunday first.
    pub(crate) weekdays: [[&'static str; 7]; WIDTHS],
    /// The abbreviated day periods `am` and `pm`.
    pub(crate) day_periods: [&'static str; 2],
    /// The names of the eras BC and AD (wide, abbreviated, narrow).
    pub(crate) eras: [[&'static str; 2]; WIDTHS],
    /// The patterns of `dateStyle` full, long, medium, short.
    pub(crate) date_formats: [&'static str; 4],
    /// The patterns of `timeStyle` full, long, medium, short.
    pub(crate) time_formats: [&'static str; 4],
    /// The patterns that join a date and a time for full, long, medium
    /// and short: the standard one and the one for a time of day (`{1}
    /// 'at' {0}`).
    pub(crate) glue: [[&'static str; 2]; 4],
    /// The patterns for the skeletons: `availableFormats` and the
    /// skeletons of the standard formats, sorted by skeleton.
    pub(crate) available: &'static [(&'static str, &'static str)],
    /// The patterns that append a missing field (`appendItems`), keyed by
    /// the field letter of the skeleton.
    pub(crate) append: &'static [(&'static str, &'static str)],
    /// The names of the fields for those patterns, keyed by the same
    /// letter.
    pub(crate) field_names: &'static [(&'static str, &'static str)],
    /// The short names of time zones: zone, standard, daylight (sorted by
    /// zone).
    pub(crate) zone_short: &'static [(&'static str, &'static str, &'static str)],
}

/// All data of one locale.
#[derive(Debug)]
pub(crate) struct LocaleData {
    /// Whether the clock of the locale has 12 hours by default.
    pub(crate) hour12: bool,
    /// The calendar.
    pub(crate) calendar: CalendarData,
    /// The numbers.
    pub(crate) numbers: NumberData,
    /// The relative times.
    pub(crate) relative: RelativeTable,
}
