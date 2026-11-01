//! `Intl.NumberFormat` without the object plumbing (ECMA-402 §15,
//! <https://tc39.es/ecma402/#numberformat-objects>): exact decimal values,
//! rounding, grouping and the patterns of the styles `decimal`, `percent`,
//! `currency` and `unit` (ADR 0027 section 10; no compact or scientific
//! notation).
//!
//! A number is kept as decimal digits, so a `BigInt` or a numeric string
//! formats exactly and `1.005` rounds to `1.01` (the digits of a double are
//! the shortest ones that read back, as in `Number.prototype.toString`).

use super::data::{CURRENCY_DIGITS, DEFAULT_CURRENCY_DIGITS};
use super::locale::Resolved;
use super::tables::{CurrencyData, UnitData};

/// A finite decimal value: `0.d1d2d3... * 10^exponent`; no digits is zero.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Decimal {
    /// Whether the sign is minus (also for zero).
    pub(crate) negative: bool,
    /// The digits, 0 to 9, without a leading zero.
    pub(crate) digits: Vec<u8>,
    /// The position of the decimal point relative to the digits.
    pub(crate) exponent: i32,
}

/// The most digits that the text of a number may have: the text of a
/// larger number is not made (a `RangeError`; Chromium formats up to the
/// string limit of 2^29 characters, ADR 0027 section 10). It applies to an
/// exact `BigInt` and to a tiny number with significant digits; a numeric
/// string is cut to [`KEPT_DIGITS`] digits and has at most 309 places
/// before the point.
pub(crate) const MAX_DIGITS: usize = 1 << 20;

/// A number to format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Number {
    /// NaN.
    Nan,
    /// Infinity, with its sign.
    Infinite(bool),
    /// A finite value.
    Finite(Decimal),
}

impl Number {
    /// The value of a double.
    pub(crate) fn from_f64(x: f64) -> Number {
        if x.is_nan() {
            return Number::Nan;
        }
        let negative = x.is_sign_negative();
        if x.is_infinite() {
            return Number::Infinite(negative);
        }
        if x == 0.0 {
            return Number::Finite(Decimal {
                negative,
                digits: Vec::new(),
                exponent: 0,
            });
        }
        let (text, exponent) = swb_js_text::number::shortest_digits(x.abs());
        Number::Finite(Decimal {
            negative,
            digits: text.bytes().map(|b| b - b'0').collect(),
            exponent,
        })
    }

    /// The value of a decimal literal (`-123.45e-6`, no white space) read
    /// from code units, or `None` if the text is not one. The units are
    /// read once and no copy of the text is made.
    ///
    /// `exact` is for the text of a `BigInt`: all digits are kept. A
    /// numeric string is read as a number string of `ToIntlMathematicalValue`
    /// (§15.5.1) as Chromium 148 does it (measured): a value whose
    /// `Number` is infinite is the infinity (`1e400`), and only the digits
    /// that a rounding can need (`KEPT_DIGITS`) are kept, so the length of
    /// the text does not matter (ADR 0027 section 10).
    pub(crate) fn parse_decimal(units: impl Iterator<Item = u16>, exact: bool) -> Option<Number> {
        let mut units = units.peekable();
        let negative = match units.peek() {
            Some(&unit) if unit == u16::from(b'-') => {
                units.next();
                true
            }
            Some(&unit) if unit == u16::from(b'+') => {
                units.next();
                false
            }
            _ => false,
        };
        if units.peek() == Some(&u16::from(b'I')) {
            let word: Vec<u16> = units.by_ref().take(9).collect();
            let infinity = word.iter().copied().eq("Infinity".bytes().map(u16::from));
            return (infinity && units.next().is_none()).then_some(Number::Infinite(negative));
        }
        let keep = if exact { usize::MAX } else { KEPT_DIGITS };
        let mut reader = Reader {
            digits: Vec::new(),
            keep,
            leading: 0,
        };
        let int_len = reader.read_digits(&mut units);
        let mut any = int_len > 0;
        if units.peek() == Some(&u16::from(b'.')) {
            units.next();
            any |= reader.read_digits(&mut units) > 0;
        }
        if !any {
            return None;
        }
        let mut exp: i64 = 0;
        if matches!(units.peek(), Some(&u) if u == u16::from(b'e') || u == u16::from(b'E')) {
            units.next();
            exp = read_exponent(&mut units)?;
        }
        if units.next().is_some() {
            return None;
        }
        Some(reader.finish(negative, int_len, exp, exact))
    }
}

/// The significant digits of a numeric string that are kept: more than the
/// 309 before the point and the 100 after it that a number below 10^309
/// can show, and more than a rounding to 21 significant digits needs.
const KEPT_DIGITS: usize = 1024;

/// The exponent (the value is `0.d1d2... * 10^exponent`) from which a
/// finite double cannot be: 10^309 is more than the largest double.
const INFINITE_EXPONENT: i64 = 310;

/// The digits of a decimal literal while it is read.
struct Reader {
    /// The significant digits kept so far.
    digits: Vec<u8>,
    /// The most digits to keep.
    keep: usize,
    /// The zeros before the first significant digit (integer and fraction
    /// part together).
    leading: i64,
}

impl Reader {
    /// Reads decimal digits; returns how many.
    fn read_digits(&mut self, units: &mut std::iter::Peekable<impl Iterator<Item = u16>>) -> i64 {
        let mut count = 0;
        while let Some(&unit) = units.peek() {
            let Some(digit) = unit.checked_sub(u16::from(b'0')).filter(|d| *d < 10) else {
                break;
            };
            units.next();
            count += 1;
            if self.digits.is_empty() && digit == 0 {
                self.leading += 1;
            } else if self.digits.len() < self.keep {
                self.digits.push(digit as u8);
            }
        }
        count
    }

    /// The number: `int_len` digits before the point, the exponent `exp`
    /// of the literal.
    fn finish(self, negative: bool, int_len: i64, exp: i64, exact: bool) -> Number {
        let digits = trim_zeros(self.digits);
        if digits.is_empty() {
            return Number::Finite(Decimal {
                negative,
                digits,
                exponent: 0,
            });
        }
        let exponent = (int_len - self.leading).saturating_add(exp);
        if !exact
            && (exponent >= INFINITE_EXPONENT
                || exponent == INFINITE_EXPONENT - 1 && overflows(&digits))
        {
            return Number::Infinite(negative);
        }
        let limit = i64::from(i32::MAX / 2);
        Number::Finite(Decimal {
            negative,
            digits,
            exponent: exponent.clamp(-limit, limit) as i32,
        })
    }
}

/// Whether digits `0.d1d2...` times 10^309 are not a finite double (the
/// `Number` of the text is infinite).
fn overflows(digits: &[u8]) -> bool {
    let text: String = digits
        .iter()
        .take(800)
        .map(|d| char::from(b'0' + d))
        .collect();
    format!("0.{text}e309")
        .parse::<f64>()
        .is_ok_and(f64::is_infinite)
}

/// The value of an exponent part after `e`: a sign and at least one digit
/// (saturated at 2^40).
fn read_exponent(units: &mut std::iter::Peekable<impl Iterator<Item = u16>>) -> Option<i64> {
    let negative = match units.peek() {
        Some(&unit) if unit == u16::from(b'-') => {
            units.next();
            true
        }
        Some(&unit) if unit == u16::from(b'+') => {
            units.next();
            false
        }
        _ => false,
    };
    let mut value: i64 = 0;
    let mut any = false;
    while let Some(&unit) = units.peek() {
        let Some(digit) = unit.checked_sub(u16::from(b'0')).filter(|d| *d < 10) else {
            break;
        };
        units.next();
        any = true;
        value = (value * 10 + i64::from(digit)).min(1 << 40);
    }
    any.then_some(if negative { -value } else { value })
}

fn trim_zeros(mut digits: Vec<u8>) -> Vec<u8> {
    while digits.last() == Some(&0) {
        digits.pop();
    }
    digits
}

/// How a number is rounded: to a number of fraction digits, or to a number
/// of significant digits (`roundingType` of §15.1.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Rounding {
    /// The minimum and maximum number of fraction digits.
    Fraction(u8, u8),
    /// The minimum and maximum number of significant digits.
    Significant(u8, u8),
}

/// `useGrouping`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Grouping {
    /// `false`.
    Off,
    /// `"auto"`.
    Auto,
    /// `"always"`.
    Always,
    /// `"min2"`.
    Min2,
}

/// `signDisplay`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SignDisplay {
    /// `"auto"`.
    Auto,
    /// `"never"`.
    Never,
    /// `"always"`.
    Always,
    /// `"exceptZero"`.
    ExceptZero,
    /// `"negative"`.
    Negative,
}

/// `currencyDisplay`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CurrencyDisplay {
    /// `"symbol"`.
    Symbol,
    /// `"narrowSymbol"`.
    Narrow,
    /// `"code"`.
    Code,
    /// `"name"`.
    Name,
}

/// `unitDisplay`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UnitDisplay {
    /// `"short"`.
    Short,
    /// `"narrow"`.
    Narrow,
    /// `"long"`.
    Long,
}

/// The style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Style {
    /// `"decimal"`.
    Decimal,
    /// `"percent"`.
    Percent,
    /// `"currency"`: the code (upper case), the display and whether the
    /// sign is `"accounting"`.
    Currency(String, CurrencyDisplay, bool),
    /// `"unit"`: the unit and the display.
    Unit(String, UnitDisplay),
}

/// The part types of `formatToParts`.
pub(crate) type Part = (&'static str, String);

/// A number format.
#[derive(Clone, Debug)]
pub(crate) struct NumberFormat {
    /// The locale.
    pub(crate) locale: Resolved,
    /// The style.
    pub(crate) style: Style,
    /// `minimumIntegerDigits`.
    pub(crate) min_integer: u8,
    /// The rounding.
    pub(crate) rounding: Rounding,
    /// `useGrouping`.
    pub(crate) grouping: Grouping,
    /// `signDisplay`.
    pub(crate) sign: SignDisplay,
}

/// The data of a currency (symbols and names), if the data has the code;
/// another code shows as the code.
pub(crate) fn currency(locale: &Resolved, code: &str) -> Option<&'static CurrencyData> {
    locale
        .data
        .numbers
        .currencies
        .iter()
        .find(|c| c.code == code)
}

/// The fraction digits of a currency: the ISO 4217 minor units of CLDR
/// (`currencyData/fractions`), 2 for a code that the table does not list.
pub(crate) fn currency_digits(code: &str) -> u8 {
    CURRENCY_DIGITS
        .binary_search_by(|(listed, _)| (*listed).cmp(code))
        .map_or(DEFAULT_CURRENCY_DIGITS, |at| CURRENCY_DIGITS[at].1)
}

/// The unit of the data by its identifier.
pub(crate) fn unit(locale: &Resolved, name: &str) -> Option<&'static UnitData> {
    let units = locale.data.numbers.units;
    units
        .binary_search_by(|u| u.name.cmp(name))
        .ok()
        .map(|i| &units[i])
}

/// Rounds the digits to `keep` digits, half away from zero; the digits
/// are empty if the value is zero after it.
fn round_to(value: &mut Decimal, keep: i64) {
    let len = value.digits.len() as i64;
    if keep >= len {
        return;
    }
    if keep < 0 {
        value.digits.clear();
        return;
    }
    let up = value.digits[keep as usize] >= 5;
    value.digits.truncate(keep as usize);
    if up {
        let mut carried = true;
        for digit in value.digits.iter_mut().rev() {
            if *digit == 9 {
                *digit = 0;
            } else {
                *digit += 1;
                carried = false;
                break;
            }
        }
        if carried {
            value.digits.insert(0, 1);
            value.exponent += 1;
        }
    }
    value.digits = trim_zeros(std::mem::take(&mut value.digits));
}

/// The integer digits, the fraction digits and whether the value is zero,
/// after the rounding.
fn split(value: &Decimal, rounding: Rounding) -> (String, String, bool) {
    let mut value = value.clone();
    match rounding {
        Rounding::Fraction(_, max) => {
            let keep = i64::from(value.exponent) + i64::from(max);
            round_to(&mut value, keep);
        }
        Rounding::Significant(_, max) => round_to(&mut value, i64::from(max)),
    }
    let zero = value.digits.is_empty();
    let exponent = value.exponent;
    let digit = |index: i64| -> char {
        if index < 0 {
            return '0';
        }
        value
            .digits
            .get(index as usize)
            .map_or('0', |d| char::from(b'0' + d))
    };
    let int_len = i64::from(exponent.max(0));
    let mut int: String = (0..int_len).map(digit).collect();
    if int.is_empty() {
        int.push('0');
    }
    let frac_len = if zero {
        0
    } else {
        (value.digits.len() as i64 - i64::from(exponent)).max(0)
    };
    let frac: String = (0..frac_len)
        .map(|k| digit(i64::from(exponent) + k))
        .collect();
    (int, frac, zero)
}

/// Pads the fraction with zeros to the minimum.
fn pad_fraction(frac: String, int: &str, zero: bool, rounding: Rounding) -> String {
    let wanted = match rounding {
        Rounding::Fraction(min, _) => usize::from(min),
        Rounding::Significant(min, _) => {
            let shown = if zero {
                1
            } else if int == "0" {
                frac.trim_start_matches('0').len()
            } else {
                int.len() + frac.len()
            };
            frac.len() + usize::from(min).saturating_sub(shown)
        }
    };
    let mut frac = frac;
    while frac.len() < wanted {
        frac.push('0');
    }
    frac
}

fn group(int: &str, grouping: Grouping) -> Vec<String> {
    let enough = match grouping {
        Grouping::Off => false,
        Grouping::Auto | Grouping::Always => int.len() >= 4,
        Grouping::Min2 => int.len() >= 5,
    };
    if !enough {
        return vec![int.to_owned()];
    }
    let first = int.len() % 3;
    let mut groups = Vec::new();
    if first > 0 {
        groups.push(int[..first].to_owned());
    }
    let mut at = first;
    while at < int.len() {
        groups.push(int[at..at + 3].to_owned());
        at += 3;
    }
    groups
}

impl NumberFormat {
    /// Whether the text of the number would have more than [`MAX_DIGITS`]
    /// digits: the digits of an exact `BigInt`, or the zeros after the
    /// point of a tiny number with significant digits (a rounding to
    /// fraction digits shows at most 100 places).
    pub(crate) fn is_too_long(&self, number: &Number) -> bool {
        let Number::Finite(value) = number else {
            return false;
        };
        let tiny = matches!(self.rounding, Rounding::Significant(..))
            && value.exponent < 0
            && value.exponent.unsigned_abs() as usize > MAX_DIGITS;
        value.digits.len() > MAX_DIGITS || tiny
    }

    /// The text of the number.
    pub(crate) fn format(&self, number: &Number) -> String {
        self.format_to_parts(number)
            .into_iter()
            .map(|part| part.1)
            .collect()
    }

    /// `FormatNumericToParts` (§15.5.5): the parts of the number.
    pub(crate) fn format_to_parts(&self, number: &Number) -> Vec<Part> {
        let symbols = &self.locale.data.numbers.symbols;
        let [decimal, group_sep, percent, plus, minus, infinity, nan] = *symbols;
        let (negative, zero, digits, one): (bool, bool, Vec<Part>, bool) = match number {
            Number::Nan => (false, false, vec![("nan", nan.to_owned())], false),
            Number::Infinite(negative) => (
                *negative,
                false,
                vec![("infinity", infinity.to_owned())],
                false,
            ),
            Number::Finite(value) => {
                let mut value = value.clone();
                if self.style == Style::Percent && !value.digits.is_empty() {
                    value.exponent += 2;
                }
                let (int, frac, zero) = split(&value, self.rounding);
                let frac = pad_fraction(frac, &int, zero, self.rounding);
                // The plural category `one` (CLDR plural rules of English:
                // the integer value is 1 and no fraction digit is shown)
                // depends on the value, not on the padding of the integer
                // part (`01 minute`; measured in Chromium 148).
                let one = int == "1" && frac.is_empty();
                let int = if int.len() < usize::from(self.min_integer) {
                    format!(
                        "{}{int}",
                        "0".repeat(usize::from(self.min_integer) - int.len())
                    )
                } else {
                    int
                };
                let mut parts = Vec::new();
                for (index, piece) in group(&int, self.grouping).into_iter().enumerate() {
                    if index > 0 {
                        parts.push(("group", group_sep.to_owned()));
                    }
                    parts.push(("integer", piece));
                }
                if !frac.is_empty() {
                    parts.push(("decimal", decimal.to_owned()));
                    parts.push(("fraction", frac.clone()));
                }
                (value.negative, zero, parts, one)
            }
        };
        let is_nan = matches!(number, Number::Nan);
        let sign = match self.sign {
            SignDisplay::Never => None,
            SignDisplay::Auto => negative.then_some(minus).filter(|_| !is_nan),
            SignDisplay::Always => Some(if negative && !is_nan { minus } else { plus }),
            SignDisplay::ExceptZero => {
                if zero || is_nan {
                    None
                } else {
                    Some(if negative { minus } else { plus })
                }
            }
            SignDisplay::Negative => (negative && !zero && !is_nan).then_some(minus),
        };
        let sign_part = sign.map(|s| {
            (
                if s == minus { "minusSign" } else { "plusSign" },
                s.to_owned(),
            )
        });
        self.compose(digits, sign_part, percent, one, negative && sign.is_some())
    }

    /// Puts the sign and the affixes of the style around the digits.
    fn compose(
        &self,
        digits: Vec<Part>,
        sign: Option<Part>,
        percent: &str,
        one: bool,
        shows_minus: bool,
    ) -> Vec<Part> {
        let data = &self.locale.data.numbers;
        let mut out: Vec<Part> = Vec::new();
        match &self.style {
            Style::Decimal => {
                out.extend(sign);
                out.extend(digits);
            }
            Style::Percent => {
                out.extend(sign);
                out.extend(digits);
                out.push(("percentSign", percent.to_owned()));
            }
            Style::Currency(code, display, accounting) => {
                let info = currency(&self.locale, code);
                let accounting_negative = *accounting && shows_minus;
                if *display == CurrencyDisplay::Name {
                    out.extend(sign);
                    out.extend(digits);
                    let name = info.map_or(code.as_str(), |c| c.names[usize::from(!one)]);
                    let template = data.currency_name;
                    // "{0} {1}": the number, then the name.
                    let (_, rest) = template.split_once("{0}").unwrap_or(("", template));
                    let (between, _) = rest.split_once("{1}").unwrap_or((rest, ""));
                    out.push(("literal", between.to_owned()));
                    out.push(("currency", name.to_owned()));
                    return out;
                }
                let symbol = match (display, info) {
                    (CurrencyDisplay::Code, _) | (_, None) => code.clone(),
                    (CurrencyDisplay::Narrow, Some(c)) => c.narrow.to_owned(),
                    (_, Some(c)) => c.symbol.to_owned(),
                };
                let letter_end = symbol.chars().last().is_some_and(char::is_alphabetic);
                if accounting_negative {
                    out.push(("literal", "(".to_owned()));
                } else {
                    out.extend(sign);
                }
                out.push(("currency", symbol));
                if letter_end {
                    out.push(("literal", "\u{a0}".to_owned()));
                }
                out.extend(digits);
                if accounting_negative {
                    out.push(("literal", ")".to_owned()));
                }
            }
            Style::Unit(name, display) => {
                out.extend(sign);
                out.extend(digits);
                let patterns = unit(&self.locale, name).map(|u| {
                    let row = match display {
                        UnitDisplay::Long => 0,
                        UnitDisplay::Short => 1,
                        UnitDisplay::Narrow => 2,
                    };
                    u.patterns[row][usize::from(!one)]
                });
                let Some(pattern) = patterns else {
                    out.push(("literal", " ".to_owned()));
                    out.push(("unit", name.clone()));
                    return out;
                };
                {
                    let (before, after) = pattern.split_once("{0}").unwrap_or(("", pattern));
                    if !before.is_empty() {
                        out.insert(0, ("unit", before.to_owned()));
                    }
                    let trimmed = after.trim_start();
                    let lead = &after[..after.len() - trimmed.len()];
                    if !lead.is_empty() {
                        out.push(("literal", lead.to_owned()));
                    }
                    if !trimmed.is_empty() {
                        out.push(("unit", trimmed.to_owned()));
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intl::locale;

    fn format(style: Style, rounding: Rounding, x: f64) -> String {
        let nf = NumberFormat {
            locale: locale::resolve(&["en-US".to_owned()]),
            style,
            min_integer: 1,
            rounding,
            grouping: Grouping::Auto,
            sign: SignDisplay::Auto,
        };
        nf.format_to_parts(&Number::from_f64(x))
            .into_iter()
            .map(|p| p.1)
            .collect()
    }

    #[test]
    fn decimals() {
        let r = Rounding::Fraction(0, 3);
        assert_eq!(format(Style::Decimal, r, 1_234_567.891), "1,234,567.891");
        assert_eq!(format(Style::Decimal, r, 1.005), "1.005");
        assert_eq!(format(Style::Decimal, r, 0.0005), "0.001");
        assert_eq!(format(Style::Decimal, r, -0.0001), "-0");
        assert_eq!(format(Style::Decimal, r, 999.9995), "1,000");
        assert_eq!(
            format(Style::Decimal, Rounding::Fraction(2, 2), 2.5),
            "2.50"
        );
        assert_eq!(
            format(Style::Decimal, Rounding::Significant(1, 3), 123_456.0),
            "123,000"
        );
        assert_eq!(
            format(Style::Decimal, Rounding::Significant(5, 21), 0.0),
            "0.0000"
        );
        assert_eq!(
            format(Style::Decimal, Rounding::Significant(2, 4), 0.000_123_45),
            "0.0001235"
        );
        assert_eq!(format(Style::Decimal, r, f64::NAN), "NaN");
        assert_eq!(format(Style::Decimal, r, f64::NEG_INFINITY), "-∞");
    }

    fn parse(text: &str, exact: bool) -> Option<Number> {
        Number::parse_decimal(text.encode_utf16(), exact)
    }

    #[test]
    fn number_strings() {
        let text = |s: &str| {
            let nf = NumberFormat {
                locale: locale::resolve(&["en-US".to_owned()]),
                style: Style::Decimal,
                min_integer: 1,
                rounding: Rounding::Fraction(0, 3),
                grouping: Grouping::Off,
                sign: SignDisplay::Auto,
            };
            nf.format(&parse(s, false).expect("a number string"))
        };
        assert_eq!(text("0e999999999"), "0");
        assert_eq!(text("-0e999999999"), "-0");
        assert_eq!(text("1e400"), "∞");
        assert_eq!(text("-1e1000000000"), "-∞");
        assert_eq!(text("1e99999999999"), "∞");
        assert_eq!(text("1e-999999999"), "0");
        assert_eq!(text("1.7976931348623157e308").len(), 309);
        assert_eq!(text("1.7976931348623159e308"), "∞");
        assert_eq!(text("1e309"), "∞");
        assert_eq!(text(&"9".repeat(400)), "∞");
        assert_eq!(text(&format!("0.{}5", "0".repeat(3000))), "0");
        assert_eq!(text(&format!("1.0005{}", "0".repeat(5000))), "1.001");
        assert_eq!(text("12.5e1"), "125");
        assert_eq!(parse("1e", false), None);
        assert_eq!(parse("e5", false), None);
        assert_eq!(parse(".", false), None);
        assert_eq!(parse("1 2", false), None);
        // The text of a BigInt keeps all its digits.
        let big = "1".repeat(2000);
        let Some(Number::Finite(value)) = parse(&big, true) else {
            panic!("a finite number");
        };
        assert_eq!(value.digits.len(), 2000);
    }

    #[test]
    fn plural_depends_on_the_value() {
        let minutes = Style::Unit("minute".into(), UnitDisplay::Long);
        let mut nf = NumberFormat {
            locale: locale::resolve(&["en-US".to_owned()]),
            style: minutes,
            min_integer: 2,
            rounding: Rounding::Fraction(0, 3),
            grouping: Grouping::Auto,
            sign: SignDisplay::Auto,
        };
        let text = |nf: &NumberFormat, x: f64| nf.format(&Number::from_f64(x));
        assert_eq!(text(&nf, 1.0), "01 minute");
        assert_eq!(text(&nf, 2.0), "02 minutes");
        nf.rounding = Rounding::Fraction(1, 3);
        assert_eq!(text(&nf, 1.0), "01.0 minutes");
    }

    #[test]
    fn currency_digits_are_the_minor_units() {
        assert_eq!(currency_digits("USD"), 2);
        assert_eq!(currency_digits("JPY"), 0);
        assert_eq!(currency_digits("CLP"), 0);
        assert_eq!(currency_digits("KWD"), 3);
        assert_eq!(currency_digits("CLF"), 4);
        assert_eq!(currency_digits("ZZZ"), 2);
    }

    #[test]
    fn styles() {
        assert_eq!(
            format(Style::Percent, Rounding::Fraction(0, 0), 0.256),
            "26%"
        );
        let usd = Style::Currency("USD".into(), CurrencyDisplay::Symbol, false);
        assert_eq!(
            format(usd.clone(), Rounding::Fraction(2, 2), -5.0),
            "-$5.00"
        );
        let code = Style::Currency("USD".into(), CurrencyDisplay::Code, false);
        assert_eq!(format(code, Rounding::Fraction(2, 2), 1.0), "USD\u{a0}1.00");
        let name = Style::Currency("USD".into(), CurrencyDisplay::Name, false);
        assert_eq!(
            format(name, Rounding::Fraction(2, 2), 1.0),
            "1.00 US dollars"
        );
        let minutes = Style::Unit("minute".into(), UnitDisplay::Long);
        assert_eq!(
            format(minutes.clone(), Rounding::Fraction(0, 0), 1.0),
            "1 minute"
        );
        assert_eq!(format(minutes, Rounding::Fraction(0, 0), 5.0), "5 minutes");
    }
}
