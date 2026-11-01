//! `Intl.RelativeTimeFormat` without the object plumbing (ECMA-402 §17,
//! <https://tc39.es/ecma402/#relativetimeformat-objects>): `in 3 days`,
//! `1 hour ago`, and with `numeric: "auto"` `tomorrow`.

use super::locale::Resolved;
use super::number::{Grouping, Number, NumberFormat, Rounding, SignDisplay, Style};

/// `style`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RelativeStyle {
    /// `"long"`.
    Long,
    /// `"short"`.
    Short,
    /// `"narrow"`.
    Narrow,
}

impl RelativeStyle {
    /// The name in the options.
    pub(crate) fn name(self) -> &'static str {
        match self {
            RelativeStyle::Long => "long",
            RelativeStyle::Short => "short",
            RelativeStyle::Narrow => "narrow",
        }
    }
}

/// The units, in the order of the data.
const UNITS: [&str; 8] = [
    "second", "minute", "hour", "day", "week", "month", "quarter", "year",
];

/// The index of a unit name, singular or plural (`SingularRelativeTimeUnit`,
/// §17.5.1).
pub(crate) fn unit_index(name: &str) -> Option<usize> {
    let singular = name.strip_suffix('s').unwrap_or(name);
    UNITS.iter().position(|u| *u == singular)
}

/// The name of a unit by its index.
pub(crate) fn unit_name(index: usize) -> &'static str {
    UNITS.get(index).copied().unwrap_or("")
}

/// A part of the output: the type (`None` for a literal), the text and
/// the unit of a number part.
pub(crate) type Part = (&'static str, String, Option<&'static str>);

/// A relative time format.
#[derive(Clone, Debug)]
pub(crate) struct RelativeTimeFormat {
    /// The locale.
    pub(crate) locale: Resolved,
    /// `style`.
    pub(crate) style: RelativeStyle,
    /// Whether `numeric` is `"auto"`.
    pub(crate) auto: bool,
}

impl RelativeTimeFormat {
    /// `PartitionRelativeTimePattern` (§17.5.2) for a finite value and the
    /// index of a unit.
    pub(crate) fn format_to_parts(&self, value: f64, unit: usize) -> Vec<Part> {
        let data = &self.locale.data.relative[self.style as usize][unit];
        let unit_text = unit_name(unit);
        if self.auto {
            // The phrase is chosen by the value rounded to two fraction
            // digits (measured: 0.001 is "today", 0.01 is "in 0.01
            // days", 0.9999 is "tomorrow").
            let rounded = (value * 100.0).round() / 100.0;
            let phrase = if rounded == -1.0 {
                data.auto[0]
            } else if rounded == 0.0 {
                data.auto[1]
            } else if rounded == 1.0 {
                data.auto[2]
            } else {
                ""
            };
            if !phrase.is_empty() {
                return vec![("literal", phrase.to_owned(), None)];
            }
        }
        let past = value.is_sign_negative();
        let number = NumberFormat {
            locale: self.locale.clone(),
            style: Style::Decimal,
            min_integer: 1,
            rounding: Rounding::Fraction(0, 3),
            grouping: Grouping::Auto,
            sign: SignDisplay::Never,
        };
        let parts = number.format_to_parts(&Number::from_f64(value.abs()));
        let one = parts.len() == 1 && parts[0].1 == "1";
        let patterns = if past { data.past } else { data.future };
        let pattern = patterns[usize::from(!one)];
        let (before, after) = pattern.split_once("{0}").unwrap_or((pattern, ""));
        let mut out: Vec<Part> = Vec::new();
        if !before.is_empty() {
            out.push(("literal", before.to_owned(), None));
        }
        out.extend(
            parts
                .into_iter()
                .map(|(kind, text)| (kind, text, Some(unit_text))),
        );
        if !after.is_empty() {
            out.push(("literal", after.to_owned(), None));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intl::locale;

    fn format(auto: bool, style: RelativeStyle, value: f64, unit: &str) -> String {
        let f = RelativeTimeFormat {
            locale: locale::resolve(&["en".to_owned()]),
            style,
            auto,
        };
        let index = unit_index(unit).unwrap();
        f.format_to_parts(value, index)
            .into_iter()
            .map(|p| p.1)
            .collect()
    }

    #[test]
    fn phrases() {
        assert_eq!(format(false, RelativeStyle::Long, 3.0, "days"), "in 3 days");
        assert_eq!(
            format(false, RelativeStyle::Long, -1.0, "hour"),
            "1 hour ago"
        );
        assert_eq!(
            format(false, RelativeStyle::Long, -0.0, "day"),
            "0 days ago"
        );
        assert_eq!(format(true, RelativeStyle::Long, 1.0, "day"), "tomorrow");
        assert_eq!(format(true, RelativeStyle::Long, 2.0, "day"), "in 2 days");
        assert_eq!(format(true, RelativeStyle::Short, -1.0, "week"), "last wk.");
        assert_eq!(format(false, RelativeStyle::Narrow, 2.0, "month"), "in 2mo");
    }
}
