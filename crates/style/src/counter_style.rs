//! Counter styles: the text of a counter value in one of the
//! `list-style-type` styles, for `counter()`, `counters()` and list item
//! markers.
//!
//! The predefined styles of CSS Counter Styles 3
//! (<https://www.w3.org/TR/css-counter-styles-3/#predefined-counters>)
//! that swb supports. Values outside the range of a style (roman numerals
//! outside 1..=3999, alphabetic styles below 1) fall back to `decimal`.
//! As in Chromium 148, the symbol of `square` is U+25A0 (the specification
//! has U+25AA). Marker text: <https://www.w3.org/TR/css-lists-3/#text-markers>;
//! like Chromium, a symbol is followed by a space and a number by ". ".

use std::fmt::Write as _;

use crate::values::ListStyleType;

/// The text of a list item marker with `content: normal`: the
/// representation of `value` in `style` and its suffix. Empty for `none`.
pub(crate) fn marker_text(style: ListStyleType, value: i32) -> String {
    let mut text = String::new();
    counter_text(style, value, &mut text);
    if !text.is_empty() {
        text.push_str(if symbol(style).is_some() { " " } else { ". " });
    }
    text
}

/// Appends the representation of `value` in `style` (the text of
/// `counter(name, style)`, without a suffix) to `out`.
pub(crate) fn counter_text(style: ListStyleType, value: i32, out: &mut String) {
    if let Some(symbol) = symbol(style) {
        out.push_str(symbol);
        return;
    }
    let value = i64::from(value);
    match style {
        ListStyleType::DecimalLeadingZero if (0..10).contains(&value) => {
            let _ = write!(out, "0{value}");
        }
        ListStyleType::LowerRoman if (1..=3999).contains(&value) => {
            out.push_str(&roman(value).to_ascii_lowercase());
        }
        ListStyleType::UpperRoman if (1..=3999).contains(&value) => out.push_str(&roman(value)),
        ListStyleType::LowerAlpha if value >= 1 => alphabetic(value, &LATIN_LOWER, out),
        ListStyleType::UpperAlpha if value >= 1 => alphabetic(value, &LATIN_UPPER, out),
        ListStyleType::LowerGreek if value >= 1 => alphabetic(value, &GREEK_LOWER, out),
        _ => {
            let _ = write!(out, "{value}");
        }
    }
}

/// The symbol of a symbolic style (`""` for `none`), `None` for the
/// numeric styles.
fn symbol(style: ListStyleType) -> Option<&'static str> {
    Some(match style {
        ListStyleType::None => "",
        ListStyleType::Disc => "\u{2022}",
        ListStyleType::Circle => "\u{25E6}",
        ListStyleType::Square => "\u{25A0}",
        ListStyleType::DisclosureOpen => "\u{25BE}",
        ListStyleType::DisclosureClosed => "\u{25B8}",
        _ => return None,
    })
}

/// Roman numerals for 1..=3999 (the caller checks the range).
fn roman(mut n: i64) -> String {
    const TABLE: [(i64, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for (value, symbol) in TABLE {
        while n >= value {
            out.push_str(symbol);
            n -= value;
        }
    }
    out
}

const LATIN_LOWER: [char; 26] = [
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's',
    't', 'u', 'v', 'w', 'x', 'y', 'z',
];
const LATIN_UPPER: [char; 26] = [
    'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S',
    'T', 'U', 'V', 'W', 'X', 'Y', 'Z',
];
/// α..ω without the final sigma.
const GREEK_LOWER: [char; 24] = [
    'α', 'β', 'γ', 'δ', 'ε', 'ζ', 'η', 'θ', 'ι', 'κ', 'λ', 'μ', 'ν', 'ξ', 'ο', 'π', 'ρ', 'σ', 'τ',
    'υ', 'φ', 'χ', 'ψ', 'ω',
];

/// The alphabetic system: 1 is the first symbol, then aa, ab, ... for a
/// positive `n`. <https://www.w3.org/TR/css-counter-styles-3/#alphabetic-system>
fn alphabetic(mut n: i64, symbols: &[char], out: &mut String) {
    let base = i64::try_from(symbols.len()).expect("a symbol table has few entries");
    let mut letters: Vec<char> = Vec::new();
    while n > 0 {
        n -= 1;
        let index = usize::try_from(n % base).expect("a remainder of a positive value is positive");
        letters.extend(symbols.get(index));
        n /= base;
    }
    out.extend(letters.iter().rev());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(style: ListStyleType, value: i32) -> String {
        let mut out = String::new();
        counter_text(style, value, &mut out);
        out
    }

    #[test]
    fn markers() {
        assert_eq!(marker_text(ListStyleType::Decimal, 3), "3. ");
        assert_eq!(marker_text(ListStyleType::Disc, 1), "\u{2022} ");
        assert_eq!(marker_text(ListStyleType::Square, 1), "\u{25A0} ");
        assert_eq!(marker_text(ListStyleType::UpperRoman, 1994), "MCMXCIV. ");
        assert_eq!(marker_text(ListStyleType::LowerAlpha, 28), "ab. ");
        assert_eq!(marker_text(ListStyleType::LowerGreek, 2), "β. ");
        assert_eq!(marker_text(ListStyleType::DecimalLeadingZero, 7), "07. ");
        assert_eq!(marker_text(ListStyleType::None, 7), "");
    }

    /// Values from Chromium 148 (`counter(c, <style>)` and markers).
    #[test]
    fn counter_values_like_chromium() {
        use ListStyleType as T;
        let all = [
            T::Decimal,
            T::LowerRoman,
            T::Disc,
            T::Circle,
            T::Square,
            T::None,
            T::DisclosureOpen,
            T::DisclosureClosed,
            T::DecimalLeadingZero,
            T::LowerGreek,
            T::UpperAlpha,
        ];
        let join = |value| {
            all.iter()
                .map(|&s| text(s, value))
                .collect::<Vec<_>>()
                .join("|")
        };
        assert_eq!(join(-5), "-5|-5|•|◦|■||▾|▸|-5|-5|-5");
        assert_eq!(join(7), "7|vii|•|◦|■||▾|▸|07|η|G");
        assert_eq!(text(T::LowerAlpha, 0), "0");
        assert_eq!(text(T::LowerAlpha, 27), "aa");
        assert_eq!(text(T::UpperRoman, 0), "0");
        assert_eq!(text(T::UpperRoman, 3999), "MMMCMXCIX");
        assert_eq!(text(T::UpperRoman, 4000), "4000");
        assert_eq!(text(T::DecimalLeadingZero, 0), "00");
        assert_eq!(text(T::DecimalLeadingZero, 123), "123");
        assert_eq!(text(T::LowerGreek, 25), "αα");
        assert_eq!(text(T::Decimal, i32::MIN), "-2147483648");
        assert_eq!(text(T::LowerAlpha, i32::MAX), "fxshrxw");
    }
}
