//! `Intl.DateTimeFormat` without the object plumbing (ECMA-402 §11,
//! <https://tc39.es/ecma402/#datetimeformat-objects>): the resolved
//! options, the pattern, and the formatting of a time value into parts.
//!
//! The builtin (`builtins/intl_datetime.rs`) reads the options and gives
//! a [`Request`]; [`DateTimeFormat::new`] finds the pattern (`pattern.rs`)
//! and [`DateTimeFormat::format_to_parts`] fills it. The pattern letters
//! are those of UTS #35 part 4 (`G y M E d a h H K k m s S z O v`).

use super::locale::Resolved;
use super::pattern::{self, Kind, Pattern, Skeleton, Token};
use super::tables::CalendarData;
use crate::date::time_value::{Fields, fields};
use crate::date::{TimeZone, gmt_format};

/// The width or style of a component (`"numeric"`, `"2-digit"`,
/// `"narrow"`, `"short"`, `"long"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Width {
    /// `"numeric"`.
    Numeric,
    /// `"2-digit"`.
    TwoDigit,
    /// `"narrow"`.
    Narrow,
    /// `"short"`.
    Short,
    /// `"long"`.
    Long,
}

impl Width {
    /// The name in the options.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Width::Numeric => "numeric",
            Width::TwoDigit => "2-digit",
            Width::Narrow => "narrow",
            Width::Short => "short",
            Width::Long => "long",
        }
    }
}

/// The values of `timeZoneName`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ZoneName {
    /// `"short"`.
    Short,
    /// `"long"`.
    Long,
    /// `"shortOffset"`.
    ShortOffset,
    /// `"longOffset"`.
    LongOffset,
    /// `"shortGeneric"`.
    ShortGeneric,
    /// `"longGeneric"`.
    LongGeneric,
}

impl ZoneName {
    /// The name in the options.
    pub(crate) fn name(self) -> &'static str {
        match self {
            ZoneName::Short => "short",
            ZoneName::Long => "long",
            ZoneName::ShortOffset => "shortOffset",
            ZoneName::LongOffset => "longOffset",
            ZoneName::ShortGeneric => "shortGeneric",
            ZoneName::LongGeneric => "longGeneric",
        }
    }
}

/// The values of `hourCycle`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HourCycle {
    /// `"h11"`: 0 to 11.
    H11,
    /// `"h12"`: 1 to 12.
    H12,
    /// `"h23"`: 0 to 23.
    H23,
    /// `"h24"`: 1 to 24.
    H24,
}

impl HourCycle {
    /// The name in the options.
    pub(crate) fn name(self) -> &'static str {
        match self {
            HourCycle::H11 => "h11",
            HourCycle::H12 => "h12",
            HourCycle::H23 => "h23",
            HourCycle::H24 => "h24",
        }
    }

    /// The letter of the hour field.
    fn letter(self) -> char {
        match self {
            HourCycle::H11 => 'K',
            HourCycle::H12 => 'h',
            HourCycle::H23 => 'H',
            HourCycle::H24 => 'k',
        }
    }

    fn of_letter(letter: char) -> Option<HourCycle> {
        Some(match letter {
            'K' => HourCycle::H11,
            'h' => HourCycle::H12,
            'H' => HourCycle::H23,
            'k' => HourCycle::H24,
            _ => return None,
        })
    }

    /// Whether the clock has 12 hours.
    pub(crate) fn is_12(self) -> bool {
        matches!(self, HourCycle::H11 | HourCycle::H12)
    }
}

/// The values of `dateStyle` and `timeStyle`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Style {
    /// `"full"`.
    Full,
    /// `"long"`.
    Long,
    /// `"medium"`.
    Medium,
    /// `"short"`.
    Short,
}

impl Style {
    /// The name in the options.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Style::Full => "full",
            Style::Long => "long",
            Style::Medium => "medium",
            Style::Short => "short",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// The components of a date-time format.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Components {
    /// `weekday`.
    pub(crate) weekday: Option<Width>,
    /// `era`.
    pub(crate) era: Option<Width>,
    /// `year`.
    pub(crate) year: Option<Width>,
    /// `month`.
    pub(crate) month: Option<Width>,
    /// `day`.
    pub(crate) day: Option<Width>,
    /// `hour`.
    pub(crate) hour: Option<Width>,
    /// `minute`.
    pub(crate) minute: Option<Width>,
    /// `second`.
    pub(crate) second: Option<Width>,
    /// `fractionalSecondDigits`, 1 to 3.
    pub(crate) fraction: Option<u8>,
    /// `timeZoneName`.
    pub(crate) zone_name: Option<ZoneName>,
}

/// What the options ask for, after their validation.
#[derive(Clone, Debug, Default)]
pub(crate) struct Request {
    /// The components (empty with a style).
    pub(crate) components: Components,
    /// `dateStyle`.
    pub(crate) date_style: Option<Style>,
    /// `timeStyle`.
    pub(crate) time_style: Option<Style>,
    /// `hour12`.
    pub(crate) hour12: Option<bool>,
    /// `hourCycle`.
    pub(crate) hour_cycle: Option<HourCycle>,
}

/// The kind of a part of the output (`formatToParts`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PartKind {
    /// `"literal"`.
    Literal,
    /// `"era"`.
    Era,
    /// `"year"`.
    Year,
    /// `"month"`.
    Month,
    /// `"weekday"`.
    Weekday,
    /// `"day"`.
    Day,
    /// `"dayPeriod"`.
    DayPeriod,
    /// `"hour"`.
    Hour,
    /// `"minute"`.
    Minute,
    /// `"second"`.
    Second,
    /// `"fractionalSecond"`.
    Fraction,
    /// `"timeZoneName"`.
    Zone,
}

impl PartKind {
    /// The `type` of the part.
    pub(crate) fn name(self) -> &'static str {
        match self {
            PartKind::Literal => "literal",
            PartKind::Era => "era",
            PartKind::Year => "year",
            PartKind::Month => "month",
            PartKind::Weekday => "weekday",
            PartKind::Day => "day",
            PartKind::DayPeriod => "dayPeriod",
            PartKind::Hour => "hour",
            PartKind::Minute => "minute",
            PartKind::Second => "second",
            PartKind::Fraction => "fractionalSecond",
            PartKind::Zone => "timeZoneName",
        }
    }
}

/// A part of the output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Part {
    /// The kind.
    pub(crate) kind: PartKind,
    /// The text.
    pub(crate) text: String,
}

/// A date-time format: the locale, the zone and the pattern.
#[derive(Clone, Debug)]
pub(crate) struct DateTimeFormat {
    /// The locale.
    pub(crate) locale: Resolved,
    /// The zone.
    pub(crate) zone: TimeZone,
    /// The name of the zone for `resolvedOptions`.
    pub(crate) zone_id: String,
    /// The components of the pattern.
    pub(crate) components: Components,
    /// `dateStyle` and `timeStyle`.
    pub(crate) date_style: Option<Style>,
    /// See `date_style`.
    pub(crate) time_style: Option<Style>,
    /// The hour cycle of the pattern, if it has an hour.
    pub(crate) hour_cycle: Option<HourCycle>,
    /// The pattern.
    pub(crate) pattern: Pattern,
}

fn width_of(count: u8) -> Width {
    if count >= 2 {
        Width::TwoDigit
    } else {
        Width::Numeric
    }
}

fn name_width(count: u8) -> Width {
    match count {
        0..=3 => Width::Short,
        4 => Width::Long,
        _ => Width::Narrow,
    }
}

/// The number of letters of a width in a skeleton (`M`, `MM`, `MMM`, ...).
fn width_count(width: Width) -> u8 {
    match width {
        Width::Numeric => 1,
        Width::TwoDigit => 2,
        Width::Short => 3,
        Width::Long => 4,
        Width::Narrow => 5,
    }
}

/// The skeleton of the components with the hour cycle `cycle`.
fn skeleton_of(components: &Components, cycle: HourCycle) -> Skeleton {
    let mut skeleton = Skeleton::default();
    let mut add = |letter: char, width: Option<Width>| {
        if let Some(width) = width {
            skeleton.push(letter, width_count(width));
        }
    };
    add('G', components.era);
    add('y', components.year);
    add('M', components.month);
    add('E', components.weekday);
    add('d', components.day);
    add(cycle.letter(), components.hour);
    add('m', components.minute);
    add('s', components.second);
    match components.zone_name {
        Some(ZoneName::Short) => skeleton.push('z', 1),
        Some(ZoneName::Long) => skeleton.push('z', 4),
        Some(ZoneName::ShortOffset) => skeleton.push('O', 1),
        Some(ZoneName::LongOffset) => skeleton.push('O', 4),
        Some(ZoneName::ShortGeneric) => skeleton.push('v', 1),
        Some(ZoneName::LongGeneric) => skeleton.push('v', 4),
        None => {}
    }
    skeleton
}

/// The pattern of a time style with the hour cycle of the request: the
/// pattern of the data, or, for another cycle, the pattern of its skeleton
/// with the other hour letter (UTS #35: the styles are only the defaults).
fn time_style_pattern(
    cal: &CalendarData,
    style: Style,
    cycle: HourCycle,
    default: HourCycle,
) -> Pattern {
    let base = pattern::parse(cal.time_formats[style.index()]);
    if cycle == default {
        return base;
    }
    let mut skeleton = Skeleton::of_pattern(&base);
    for field in &mut skeleton.0 {
        if field.0 == Kind::Hour {
            field.1 = cycle.letter();
        }
    }
    pattern::best_pattern(cal, &skeleton)
}

impl DateTimeFormat {
    /// The format for a request in a locale and zone.
    pub(crate) fn new(
        locale: Resolved,
        zone: TimeZone,
        zone_id: String,
        request: &Request,
    ) -> DateTimeFormat {
        let cal = &locale.data.calendar;
        let default = if locale.data.hour12 {
            HourCycle::H12
        } else {
            HourCycle::H23
        };
        let keyword = locale.hour_cycle.as_deref().and_then(|name| match name {
            "h11" => Some(HourCycle::H11),
            "h12" => Some(HourCycle::H12),
            "h23" => Some(HourCycle::H23),
            "h24" => Some(HourCycle::H24),
            _ => None,
        });
        let cycle = match (request.hour12, request.hour_cycle.or(keyword)) {
            (Some(true), _) => HourCycle::H12,
            (Some(false), _) => HourCycle::H23,
            (None, Some(cycle)) => cycle,
            (None, None) => default,
        };
        let (components, pattern) = if request.date_style.is_some() || request.time_style.is_some()
        {
            let date = request
                .date_style
                .map(|s| pattern::parse(cal.date_formats[s.index()]));
            let time = request
                .time_style
                .map(|s| time_style_pattern(cal, s, cycle, default));
            let pattern = match (date, time) {
                (Some(d), Some(t)) => {
                    let style = request.date_style.map_or(3, Style::index);
                    pattern::join(cal, style, &d, &t)
                }
                (Some(p), None) | (None, Some(p)) => p,
                (None, None) => Vec::new(),
            };
            (Components::default(), pattern)
        } else {
            let mut skeleton = skeleton_of(&request.components, cycle);
            // A fraction needs seconds: they are added next to other time
            // fields, and a fraction alone is the digits alone.
            let has_time =
                skeleton.get(Kind::Hour).is_some() || skeleton.get(Kind::Minute).is_some();
            if request.components.fraction.is_some() && has_time {
                skeleton.push('s', 1);
            }
            let has_seconds = has_time || request.components.second.is_some();
            let pattern = match request.components.fraction {
                // A fraction without a time field is a time part of its
                // own, joined to the date like a time (`1/1/1970, 000`).
                Some(digits) if !has_seconds => pattern::with_fraction(cal, &skeleton, digits),
                Some(digits) => {
                    let mut pattern = pattern::best_pattern(cal, &skeleton);
                    add_fraction(&mut pattern, digits);
                    pattern
                }
                None => pattern::best_pattern(cal, &skeleton),
            };
            (request.components.clone(), pattern)
        };
        let hour_cycle = pattern.iter().find_map(|token| match token {
            Token::Field(letter, _) => HourCycle::of_letter(*letter),
            Token::Literal(_) => None,
        });
        let mut format = DateTimeFormat {
            locale,
            zone,
            zone_id,
            components,
            date_style: request.date_style,
            time_style: request.time_style,
            hour_cycle,
            pattern,
        };
        if format.date_style.is_none() && format.time_style.is_none() {
            format.components = components_of(&format.pattern, format.components.fraction);
        }
        format
    }

    /// The fields of the time `t` (finite, within the time range) in the
    /// zone, and the offset in milliseconds.
    fn local_fields(&self, t: i64) -> (Fields, i64) {
        let offset = self.zone.offset_at_utc(t);
        (fields(t + offset), offset)
    }

    /// `FormatDateTimePattern` (§11.5.5): the parts of the time `t`, an
    /// integral time value within the range.
    pub(crate) fn format_to_parts(&self, t: i64) -> Vec<Part> {
        let (local, _) = self.local_fields(t);
        let mut parts: Vec<Part> = Vec::new();
        let mut push = |kind: PartKind, text: String| match parts.last_mut() {
            Some(last) if kind == PartKind::Literal && last.kind == PartKind::Literal => {
                last.text.push_str(&text);
            }
            _ => parts.push(Part { kind, text }),
        };
        for token in &self.pattern {
            match token {
                Token::Literal(text) => push(PartKind::Literal, text.clone()),
                Token::Field(letter, width) => {
                    if let Some((kind, text)) = self.field(*letter, *width, &local, t) {
                        push(kind, text);
                    }
                }
            }
        }
        parts
    }

    /// The text of the time `t`.
    pub(crate) fn format(&self, t: i64) -> String {
        self.format_to_parts(t)
            .into_iter()
            .map(|part| part.text)
            .collect()
    }

    fn field(&self, letter: char, width: u8, f: &Fields, t: i64) -> Option<(PartKind, String)> {
        let cal = &self.locale.data.calendar;
        let name_row = |width: u8| match width {
            0..=3 => 1,
            4 => 0,
            _ => 2,
        };
        let number = |value: i64, width: u8| {
            let digits = value.unsigned_abs().to_string();
            let pad = usize::from(width).saturating_sub(digits.len());
            format!("{}{digits}", "0".repeat(pad))
        };
        let hour = i64::from(f.hour);
        Some(match letter {
            'G' => {
                let era = usize::from(f.year > 0);
                (PartKind::Era, cal.eras[name_row(width)][era].to_owned())
            }
            'y' => {
                let year = if f.year > 0 { f.year } else { 1 - f.year };
                let text = if width == 2 {
                    number(year % 100, 2)
                } else {
                    number(year, width)
                };
                (PartKind::Year, text)
            }
            'M' | 'L' => {
                let text = match width {
                    1 | 2 => number(i64::from(f.month) + 1, width),
                    _ => cal.months[name_row(width)][usize::from(f.month)].to_owned(),
                };
                (PartKind::Month, text)
            }
            'E' | 'c' => (
                PartKind::Weekday,
                cal.weekdays[name_row(width)][usize::from(f.weekday)].to_owned(),
            ),
            'd' => (PartKind::Day, number(i64::from(f.day), width)),
            'a' => (
                PartKind::DayPeriod,
                cal.day_periods[usize::from(f.hour >= 12)].to_owned(),
            ),
            'h' => (
                PartKind::Hour,
                number(if hour % 12 == 0 { 12 } else { hour % 12 }, width),
            ),
            'H' => (PartKind::Hour, number(hour, width)),
            'K' => (PartKind::Hour, number(hour % 12, width)),
            'k' => (
                PartKind::Hour,
                number(if hour == 0 { 24 } else { hour }, width),
            ),
            'm' => (PartKind::Minute, number(i64::from(f.minute), width)),
            's' => (PartKind::Second, number(i64::from(f.second), width)),
            'S' => {
                let digits = format!("{:03}", f.ms);
                (
                    PartKind::Fraction,
                    digits[..usize::from(width).min(3)].to_owned(),
                )
            }
            'z' | 'O' | 'v' => (PartKind::Zone, self.zone_text(letter, width, t)),
            _ => return None,
        })
    }

    /// The text of a time zone field (`z`: the name, `O`: the offset, `v`:
    /// the generic name, which is the offset here: ADR 0027 section 10).
    fn zone_text(&self, letter: char, width: u8, t: i64) -> String {
        let facts = self.zone.name_facts(t);
        let cal = &self.locale.data.calendar;
        let long = width >= 4;
        // A zone given as an offset has no name of its own, except the
        // offset zero, which is Greenwich Mean Time.
        let offset_zone = self.zone_id.starts_with(['+', '-']) && self.zone_id != "+00:00";
        if letter == 'z' && !offset_zone {
            if let Some(abbreviation) = &facts.abbreviation {
                return abbreviation.clone();
            }
            if long {
                return match facts.long {
                    Some(name) => name.to_owned(),
                    None => gmt_format(facts.name_offset),
                };
            }
            let short = facts.lookup.and_then(|key| {
                let index = cal.zone_short.binary_search_by(|e| e.0.cmp(key)).ok()?;
                let entry = cal.zone_short[index];
                let name = if facts.daylight { entry.2 } else { entry.1 };
                (!name.is_empty()).then_some(name)
            });
            if let Some(name) = short {
                return name.to_owned();
            }
        }
        offset_text(facts.offset, long)
    }
}

/// The localized GMT format of an offset: `GMT+1`, `GMT+5:30` (short) and
/// `GMT+01:00` (long).
fn offset_text(seconds: i32, long: bool) -> String {
    if long {
        return gmt_format(seconds);
    }
    let sign = if seconds < 0 { '-' } else { '+' };
    let total = seconds.unsigned_abs();
    let (hours, minutes) = (total / 3600, total / 60 % 60);
    if minutes == 0 {
        format!("GMT{sign}{hours}")
    } else {
        format!("GMT{sign}{hours}:{minutes:02}")
    }
}

/// Inserts the fraction after the seconds: `ss` becomes `ss.SSS`.
fn add_fraction(pattern: &mut Pattern, digits: u8) {
    let at = pattern
        .iter()
        .position(|t| matches!(t, Token::Field('s', _)));
    match at {
        Some(index) => {
            let tail = [Token::Literal(".".to_owned()), Token::Field('S', digits)];
            for (offset, token) in tail.into_iter().enumerate() {
                pattern.insert(index + 1 + offset, token);
            }
        }
        None => pattern.push(Token::Field('S', digits)),
    }
}

/// The components that a pattern has (`resolvedOptions`).
fn components_of(pattern: &Pattern, fraction: Option<u8>) -> Components {
    let mut c = Components::default();
    for token in pattern {
        let Token::Field(letter, count) = token else {
            continue;
        };
        match Kind::of(*letter) {
            Some(Kind::Era) => c.era = Some(name_width(*count)),
            Some(Kind::Year) => c.year = Some(width_of(*count)),
            Some(Kind::Month) => {
                c.month = Some(if *count <= 2 {
                    width_of(*count)
                } else {
                    name_width(*count)
                });
            }
            Some(Kind::Weekday) => c.weekday = Some(name_width(*count)),
            Some(Kind::Day) => c.day = Some(width_of(*count)),
            Some(Kind::Hour) => c.hour = Some(width_of(*count)),
            Some(Kind::Minute) => c.minute = Some(width_of(*count)),
            Some(Kind::Second) => c.second = Some(width_of(*count)),
            Some(Kind::Fraction) => c.fraction = fraction.or(Some(*count)),
            Some(Kind::Zone) => {
                c.zone_name = Some(match (letter, *count >= 4) {
                    ('z', false) => ZoneName::Short,
                    ('z', true) => ZoneName::Long,
                    ('O', false) => ZoneName::ShortOffset,
                    ('O', true) => ZoneName::LongOffset,
                    (_, false) => ZoneName::ShortGeneric,
                    (_, true) => ZoneName::LongGeneric,
                });
            }
            _ => {}
        }
    }
    c
}
