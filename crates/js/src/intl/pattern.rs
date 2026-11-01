//! Date-time patterns and skeletons (UTS #35 part 4, "Date Format Patterns"
//! and "Matching Skeletons",
//! <https://www.unicode.org/reports/tr35/tr35-dates.html#Date_Format_Patterns>).
//!
//! A *pattern* is text with fields (`h:mm a`); a *skeleton* is a set of
//! fields with widths (`hmm`) that says what a request needs, not how it
//! looks. The data has a pattern for each of many skeletons; a request is
//! served by the pattern of the closest skeleton whose fields are exactly
//! the requested ones, with the widths of the request (the rules below are
//! UTS #35's, and what Chromium 148 prints decided the rest; ADR 0027
//! section 10). A request that no skeleton serves is split into a date and
//! a time (joined by the pattern of the locale), or served by a smaller
//! skeleton with the missing fields appended (`appendItems`: `2024 (day:
//! 1)`).

use super::tables::CalendarData;

/// The kind of a field of a pattern, in the order of a skeleton.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Kind {
    /// `G`.
    Era,
    /// `y`.
    Year,
    /// `M`, `L`.
    Month,
    /// `E`, `c`.
    Weekday,
    /// `d`.
    Day,
    /// `a`.
    Period,
    /// `h`, `H`, `K`, `k`.
    Hour,
    /// `m`.
    Minute,
    /// `s`.
    Second,
    /// `S`.
    Fraction,
    /// `z`, `O`, `v`.
    Zone,
}

impl Kind {
    /// The kind of a pattern letter, `None` for a letter that has no
    /// field here.
    pub(crate) fn of(letter: char) -> Option<Kind> {
        Some(match letter {
            'G' => Kind::Era,
            'y' => Kind::Year,
            'M' | 'L' => Kind::Month,
            'E' | 'c' => Kind::Weekday,
            'd' => Kind::Day,
            'a' => Kind::Period,
            'h' | 'H' | 'K' | 'k' => Kind::Hour,
            'm' => Kind::Minute,
            's' => Kind::Second,
            'S' => Kind::Fraction,
            'z' | 'O' | 'v' => Kind::Zone,
            _ => return None,
        })
    }

    /// Whether a field of this kind and width is a number (`M`, `MM`) and
    /// not a name (`MMM`).
    fn numeric(self, width: u8) -> bool {
        match self {
            Kind::Month => width <= 2,
            Kind::Era | Kind::Weekday | Kind::Zone | Kind::Period => false,
            _ => true,
        }
    }

    /// The key of `appendItems` and of the field names.
    fn append_key(self) -> &'static str {
        match self {
            Kind::Era => "G",
            Kind::Year => "y",
            Kind::Month => "M",
            Kind::Weekday => "E",
            Kind::Day => "d",
            Kind::Hour => "h",
            Kind::Minute => "m",
            Kind::Second | Kind::Fraction => "s",
            Kind::Zone => "z",
            Kind::Period => "a",
        }
    }
}

/// One element of a pattern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Token {
    /// Text that is printed as it is.
    Literal(String),
    /// A field: its letter and the number of times it is repeated.
    Field(char, u8),
}

/// A parsed pattern.
pub(crate) type Pattern = Vec<Token>;

/// Parses a pattern: letters of fields in runs, `'quoted text'` and `''`
/// (an apostrophe) as text, every other character as text.
pub(crate) fn parse(text: &str) -> Pattern {
    let mut out: Pattern = Vec::new();
    let mut chars = text.chars().peekable();
    let push_text = |out: &mut Pattern, s: &str| match out.last_mut() {
        Some(Token::Literal(last)) => last.push_str(s),
        _ => out.push(Token::Literal(s.to_owned())),
    };
    while let Some(c) = chars.next() {
        if c == '\'' {
            if chars.peek() == Some(&'\'') {
                chars.next();
                push_text(&mut out, "'");
                continue;
            }
            let mut quoted = String::new();
            while let Some(q) = chars.next() {
                if q == '\'' {
                    if chars.peek() == Some(&'\'') {
                        chars.next();
                        quoted.push('\'');
                        continue;
                    }
                    break;
                }
                quoted.push(q);
            }
            push_text(&mut out, &quoted);
        } else if c.is_ascii_alphabetic() {
            let mut width = 1u8;
            while chars.peek() == Some(&c) {
                chars.next();
                width = width.saturating_add(1);
            }
            out.push(Token::Field(c, width));
        } else {
            let mut buffer = [0u8; 4];
            push_text(&mut out, c.encode_utf8(&mut buffer));
        }
    }
    out
}

/// The fields of a request or of the skeleton of a pattern: kind, letter
/// and width, in the order of [`Kind`]. A day period is never part of it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Skeleton(pub(crate) Vec<(Kind, char, u8)>);

impl Skeleton {
    /// Parses a skeleton such as `yMMMd` (`ahmm` without its `a`).
    pub(crate) fn parse(text: &str) -> Skeleton {
        let mut skeleton = Skeleton::default();
        for token in parse(text) {
            if let Token::Field(letter, width) = token {
                skeleton.push(letter, width);
            }
        }
        skeleton
    }

    /// The skeleton of the fields of a pattern.
    pub(crate) fn of_pattern(pattern: &Pattern) -> Skeleton {
        let mut skeleton = Skeleton::default();
        for token in pattern {
            if let Token::Field(letter, width) = token {
                skeleton.push(*letter, *width);
            }
        }
        skeleton
    }

    /// Adds a field (a kind that is already there or has no meaning here
    /// is ignored).
    pub(crate) fn push(&mut self, letter: char, width: u8) {
        let Some(kind) = Kind::of(letter) else {
            return;
        };
        if kind == Kind::Period || self.get(kind).is_some() {
            return;
        }
        let letter = match letter {
            'L' => 'M',
            'c' => 'E',
            other => other,
        };
        self.0.push((kind, letter, width));
        self.0.sort_by_key(|field| field.0);
    }

    /// The letter and width of the field of a kind.
    pub(crate) fn get(&self, kind: Kind) -> Option<(char, u8)> {
        self.0
            .iter()
            .find(|field| field.0 == kind)
            .map(|field| (field.1, field.2))
    }

    fn kinds(&self) -> Vec<Kind> {
        self.0.iter().map(|field| field.0).collect()
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The fields whose kind `keep` accepts.
    fn filter(&self, keep: impl Fn(Kind) -> bool) -> Skeleton {
        Skeleton(self.0.iter().copied().filter(|f| keep(f.0)).collect())
    }
}

fn is_date_kind(kind: Kind) -> bool {
    kind <= Kind::Day
}

/// The distance of a skeleton of the data from a request with the same
/// kinds: the difference of the widths; a number against a name, or a
/// 12-hour clock against a 24-hour one, is far; a different letter of the
/// zone is near.
fn distance(request: &Skeleton, entry: &Skeleton) -> u32 {
    let mut total = 0;
    for (kind, letter, width) in &request.0 {
        let Some((entry_letter, entry_width)) = entry.get(*kind) else {
            return u32::MAX;
        };
        // A different width of the hour is nearer than one of another
        // field (measured: the request `HHm` takes `Hm` and `Hmm` takes
        // `HHmm`, each from a locale that has both).
        let weight = if *kind == Kind::Hour { 1 } else { 2 };
        if *kind == Kind::Zone {
            // The width of a zone field (short or long name) is replaced
            // after the match; only the letter counts.
            total += u32::from(letter != &entry_letter);
            continue;
        }
        total += if kind.numeric(*width) == kind.numeric(entry_width) {
            weight * u32::from(width.abs_diff(entry_width))
        } else {
            200
        };
        if letter != &entry_letter {
            if *kind == Kind::Hour && letter.is_uppercase() != entry_letter.is_uppercase() {
                // A 12-hour clock is not the answer for a 24-hour request.
                return u32::MAX;
            }
            total += 1;
        }
    }
    total
}

/// Replaces the letters and widths of the fields of a pattern with those of
/// the request where the request differs from the skeleton of the data
/// (UTS #35: a field keeps the width of the pattern when the request and
/// the skeleton agree, which is how `dd/MM/y` serves the request `yMd`).
fn adjust(pattern: &Pattern, entry: &Skeleton, request: &Skeleton) -> Pattern {
    pattern
        .iter()
        .map(|token| {
            let Token::Field(letter, _) = token else {
                return token.clone();
            };
            let Some(kind) = Kind::of(*letter) else {
                return token.clone();
            };
            let (Some(wanted), Some(have)) = (request.get(kind), entry.get(kind)) else {
                return token.clone();
            };
            if wanted == have {
                return token.clone();
            }
            let letter = match kind {
                Kind::Hour | Kind::Zone => wanted.0,
                _ => *letter,
            };
            Token::Field(letter, wanted.1)
        })
        .collect()
}

/// The pattern of the data whose skeleton has exactly the fields of the
/// request, with the closest widths, adjusted to the request.
fn exact_kinds(cal: &CalendarData, request: &Skeleton) -> Option<Pattern> {
    let kinds = request.kinds();
    let mut best: Option<(u32, Skeleton, &str)> = None;
    for (key, text) in cal.available {
        let entry = Skeleton::parse(key);
        if entry.kinds() != kinds {
            continue;
        }
        let d = distance(request, &entry);
        if d != u32::MAX && best.as_ref().is_none_or(|b| d < b.0) {
            best = Some((d, entry, text));
        }
    }
    let (_, entry, text) = best?;
    Some(adjust(&parse(text), &entry, request))
}

/// The pattern for one field alone when the data has none: the letter
/// with its width (a minute or a second alone is not padded).
fn single_field(request: &Skeleton, kind: Kind) -> Pattern {
    match request.get(kind) {
        Some((letter, width)) => {
            let width = if matches!(kind, Kind::Minute | Kind::Second) {
                1
            } else {
                width
            };
            vec![Token::Field(letter, width)]
        }
        None => Vec::new(),
    }
}

/// Fills `{0}`, `{1}` and `{2}` of a pattern of `appendItems` and of the
/// date-time pattern (`{1}, {0}`): the elements are patterns, the field
/// name is text.
fn combine(template: &str, zero: &Pattern, one: &Pattern, name: &str) -> Pattern {
    let mut out: Pattern = Vec::new();
    let append = |out: &mut Pattern, items: &Pattern| {
        for token in items {
            match (token, out.last_mut()) {
                (Token::Literal(text), Some(Token::Literal(last))) => last.push_str(text),
                _ => out.push(token.clone()),
            }
        }
    };
    let literal = |out: &mut Pattern, text: &str| {
        if !text.is_empty() {
            append(out, &vec![Token::Literal(text.to_owned())]);
        }
    };
    for token in parse(template) {
        match token {
            Token::Literal(text) => {
                let mut rest = text.as_str();
                while let Some(open) = rest.find('{') {
                    literal(&mut out, &rest[..open]);
                    let which = rest[open..].chars().nth(1);
                    match which {
                        Some('0') => append(&mut out, zero),
                        Some('1') => append(&mut out, one),
                        _ => literal(&mut out, name),
                    }
                    rest = &rest[(open + 3).min(rest.len())..];
                }
                literal(&mut out, rest);
            }
            field @ Token::Field(..) => out.push(field),
        }
    }
    out
}

/// A pattern for a request that only has date fields, or only time
/// fields: the exact one, else the largest smaller one with the missing
/// fields appended.
fn part_pattern(cal: &CalendarData, request: &Skeleton) -> Pattern {
    if let Some(pattern) = exact_kinds(cal, request) {
        return pattern;
    }
    // The entry with the most of the requested kinds (and no other); on a
    // tie the nearer one, then the one with the earlier kind.
    let mut best: Option<(usize, u32, Skeleton, &str)> = None;
    for (key, text) in cal.available {
        let entry = Skeleton::parse(key);
        let covered = entry.0.len();
        let subset = covered > 0 && entry.0.iter().all(|f| request.get(f.0).is_some());
        if !subset {
            continue;
        }
        let sub = request.filter(|k| entry.get(k).is_some());
        let d = distance(&sub, &entry);
        let rank = |covered: usize, d: u32, first: Kind| (std::cmp::Reverse(covered), d, first);
        let first = entry.0[0].0;
        let better = best
            .as_ref()
            .is_none_or(|b| rank(covered, d, first) < rank(b.0, b.1, b.2.0[0].0));
        if better && d != u32::MAX {
            best = Some((covered, d, entry, text));
        }
    }
    let Some((_, _, entry, text)) = best else {
        return request
            .0
            .iter()
            .flat_map(|(kind, _, _)| {
                [Token::Literal(" ".to_owned())]
                    .into_iter()
                    .chain(single_field(request, *kind))
            })
            .skip(1)
            .collect();
    };
    let sub = request.filter(|k| entry.get(k).is_some());
    let mut pattern = adjust(&parse(text), &entry, &sub);
    for (kind, _, _) in &request.0 {
        if entry.get(*kind).is_some() {
            continue;
        }
        let piece = part_pattern(cal, &request.filter(|k| k == *kind));
        pattern = append_field(cal, *kind, &pattern, &piece);
    }
    pattern
}

/// Appends the pattern `piece` of a missing field of the kind `kind` to
/// `pattern` with the `appendItems` template of the locale.
fn append_field(cal: &CalendarData, kind: Kind, pattern: &Pattern, piece: &Pattern) -> Pattern {
    let key = kind.append_key();
    let template = cal
        .append
        .iter()
        .find(|(k, _)| *k == key)
        .map_or("{0} {1}", |(_, t)| t);
    let name = cal
        .field_names
        .iter()
        .find(|(k, _)| *k == key)
        .map_or("", |(_, n)| n);
    combine(template, pattern, piece, name)
}

/// The pattern for a request that has `digits` digits of a fraction of a
/// second but no hour, minute or second: the fraction is a time part of
/// its own (with the zone name after it), joined to the date part like a
/// time (`1/1/1970, 000`, `January 1 at 000`; measured in Chromium 148).
pub(crate) fn with_fraction(cal: &CalendarData, request: &Skeleton, digits: u8) -> Pattern {
    let date = request.filter(is_date_kind);
    let zone = request.filter(|k| k == Kind::Zone);
    let mut time: Pattern = vec![Token::Field('S', digits)];
    if !zone.is_empty() {
        let zone_pattern = part_pattern(cal, &zone);
        time = append_field(cal, Kind::Zone, &time, &zone_pattern);
    }
    if date.is_empty() {
        return time;
    }
    let date_pattern = best_pattern(cal, &date);
    join(cal, glue_index(&date), &date_pattern, &time)
}

/// Which pattern joins a date and a time (UTS #35: by the style of the
/// date): a long month name (with a weekday: the full style) takes the
/// pattern of the long and full styles; a short name the medium style;
/// anything else the short style.
fn glue_index(date: &Skeleton) -> usize {
    match date.get(Kind::Month) {
        Some((_, 4)) if date.get(Kind::Weekday).is_some() => 0,
        Some((_, 4)) => 1,
        Some((_, 3)) => 2,
        _ => 3,
    }
}

/// The index (0 full, 1 long, 2 medium, 3 short) of a date-time style
/// name.
pub(crate) fn join(cal: &CalendarData, style: usize, date: &Pattern, time: &Pattern) -> Pattern {
    let template = cal.glue.get(style).map_or("{1}, {0}", |g| g[1]);
    combine(template, time, date, "")
}

/// The pattern for a request (UTS #35, "Matching Skeletons"). The hour of
/// the request may have the letter `K` or `k`: the pattern is found for
/// `h` or `H` and the letter is replaced after.
pub(crate) fn best_pattern(cal: &CalendarData, request: &Skeleton) -> Pattern {
    let hour = request.get(Kind::Hour);
    let proxy = match hour {
        Some(('K', width)) => Some(('h', width)),
        Some(('k', width)) => Some(('H', width)),
        _ => None,
    };
    let mut asked = request.clone();
    if let Some((letter, width)) = proxy {
        for field in &mut asked.0 {
            if field.0 == Kind::Hour {
                *field = (Kind::Hour, letter, width);
            }
        }
    }
    let mut pattern = build(cal, &asked);
    if let Some((letter, _)) = hour.filter(|_| proxy.is_some()) {
        for token in &mut pattern {
            if let Token::Field(l, _) = token
                && Kind::of(*l) == Some(Kind::Hour)
            {
                *l = letter;
            }
        }
    }
    pattern
}

fn build(cal: &CalendarData, request: &Skeleton) -> Pattern {
    if request.is_empty() {
        return Vec::new();
    }
    if let Some(pattern) = exact_kinds(cal, request) {
        return pattern;
    }
    let date = request.filter(is_date_kind);
    let time = request.filter(|k| !is_date_kind(k));
    if date.is_empty() || time.is_empty() {
        return part_pattern(cal, request);
    }
    let (date_pattern, time_pattern) = (part_pattern(cal, &date), part_pattern(cal, &time));
    join(cal, glue_index(&date), &date_pattern, &time_pattern)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intl::data::{EN_GB, EN_US};

    /// A pattern as text: fields as letters, literals in braces.
    fn text(pattern: &Pattern) -> String {
        pattern
            .iter()
            .map(|t| match t {
                Token::Literal(s) => format!("{{{s}}}"),
                Token::Field(c, w) => c.to_string().repeat(usize::from(*w)),
            })
            .collect()
    }

    fn best(cal: &CalendarData, skeleton: &str) -> String {
        text(&best_pattern(cal, &Skeleton::parse(skeleton)))
    }

    #[test]
    fn parse_quotes() {
        assert_eq!(text(&parse("h:mm a")), "h{:}mm{ }a");
        assert_eq!(text(&parse("d 'of' MMMM")), "d{ of }MMMM");
        assert_eq!(text(&parse("HH''h")), "HH{'}h");
        assert_eq!(text(&parse("HH'h' v")), "HH{h }v");
    }

    #[test]
    fn exact_and_adjusted_widths() {
        let us = &EN_US.calendar;
        assert_eq!(best(us, "yMd"), "M{/}d{/}y");
        assert_eq!(best(us, "yMMdd"), "MM{/}dd{/}y");
        assert_eq!(best(us, "yyMd"), "M{/}d{/}yy");
        assert_eq!(best(us, "yMMMMEEEEd"), "EEEE{, }MMMM{ }d{, }y");
        assert_eq!(best(us, "hmm"), "h{:}mm{ }a");
        assert_eq!(best(us, "hhmm"), "hh{:}mm{ }a");
        let gb = &EN_GB.calendar;
        assert_eq!(best(gb, "yMd"), "dd{/}MM{/}y");
        assert_eq!(best(gb, "yyMd"), "dd{/}MM{/}yy");
        assert_eq!(best(gb, "Hm"), "HH{:}mm");
        // The hour width is the request's when the skeleton is not exact.
        assert_eq!(best(gb, "Hmm"), "H{:}mm");
        assert_eq!(best(gb, "HHm"), "HH{:}mm");
    }

    #[test]
    fn joined_and_appended() {
        let us = &EN_US.calendar;
        assert_eq!(best(us, "yMMMMdhm"), "MMMM{ }d{, }y{ at }h{:}mm{ }a");
        assert_eq!(best(us, "yMMMdhm"), "MMM{ }d{, }y{, }h{:}mm{ }a");
        assert_eq!(best(us, "yd"), "y{ (day: }d{)}");
        // A 12-hour pattern does not serve a 24-hour request.
        assert_eq!(best(&EN_GB.calendar, "EH"), "ccc{, }HH");
    }

    #[test]
    fn hour_letters() {
        let us = &EN_US.calendar;
        assert_eq!(best(us, "Km"), "K{:}mm{ }a");
        assert_eq!(best(us, "km"), "kk{:}mm");
    }
}
