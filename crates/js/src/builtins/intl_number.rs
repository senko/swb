//! `Intl.NumberFormat` (ECMA-402 §15,
//! <https://tc39.es/ecma402/#numberformat-objects>): the constructor with
//! the styles `decimal`, `percent`, `currency` and `unit` and the options
//! for the digits, the grouping and the sign; `format` (a bound getter),
//! `formatToParts`, `resolvedOptions`, `supportedLocalesOf`; and the
//! arguments of `Number.prototype.toLocaleString` and
//! `BigInt.prototype.toLocaleString` (§19.3.1). The formatting is in
//! `crate::intl::number`. Compact and scientific notation, the rounding
//! options and other numbering systems are not there (ADR 0027 section 10).

use super::intl::{
    Options, constructor_on, new_object, parts_array, plain_object, resolve_locale, set, set_text,
    string_value, supported_locales_of, this_format,
};
use crate::heap::Gc;
use crate::intl::IntlData;
use crate::intl::number::{
    CurrencyDisplay, Grouping, Number, NumberFormat, Rounding, SignDisplay, Style, UnitDisplay,
    currency_digits,
};
use crate::object::Object;
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::convert::Hint;
use crate::vm::{NativeCall, NativeReturn, VmError, VmResult};

const CONSTRUCTOR: &str = "Intl.NumberFormat";

/// `Intl.NumberFormat.supportedLocalesOf(locales [, options])`.
fn supported_locales_of_fn(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    supported_locales_of(rt, call, "Intl.NumberFormat.supportedLocalesOf")
}

/// The sanctioned simple units (ECMA-402 §6.6.2).
const SIMPLE_UNITS: [&str; 45] = [
    "acre",
    "bit",
    "byte",
    "celsius",
    "centimeter",
    "day",
    "degree",
    "fahrenheit",
    "fluid-ounce",
    "foot",
    "gallon",
    "gigabit",
    "gigabyte",
    "gram",
    "hectare",
    "hour",
    "inch",
    "kilobit",
    "kilobyte",
    "kilogram",
    "kilometer",
    "liter",
    "megabit",
    "megabyte",
    "meter",
    "microsecond",
    "mile",
    "mile-scandinavian",
    "milliliter",
    "millimeter",
    "millisecond",
    "minute",
    "month",
    "nanosecond",
    "ounce",
    "percent",
    "petabyte",
    "pound",
    "second",
    "stone",
    "terabit",
    "terabyte",
    "week",
    "yard",
    "year",
];

pub(super) fn install(rt: &mut Runtime, realm: u32, intl: Gc<Object>) -> VmResult<()> {
    let (constructor, proto) = constructor_on(rt, realm, intl, "NumberFormat", constructor_fn)?;
    super::method(
        rt,
        realm,
        constructor,
        "supportedLocalesOf",
        1,
        supported_locales_of_fn,
    )?;
    super::method(rt, realm, proto, "resolvedOptions", 0, resolved_options)?;
    super::method(rt, realm, proto, "formatToParts", 1, format_to_parts)?;
    super::getter(rt, realm, proto, "format", format_getter)?;
    super::to_string_tag(rt, proto, "Intl.NumberFormat")
}

/// A sanctioned unit identifier: a simple unit or `X-per-Y`.
fn sanctioned(unit: &str) -> bool {
    if SIMPLE_UNITS.contains(&unit) {
        return true;
    }
    unit.split_once("-per-")
        .is_some_and(|(a, b)| SIMPLE_UNITS.contains(&a) && SIMPLE_UNITS.contains(&b))
}

/// `SetNumberFormatDigitOptions` (§15.1.3): the rounding and the integer
/// digits.
fn digit_options(
    rt: &mut Runtime,
    options: &Options,
    default_min: u32,
    default_max: u32,
) -> VmResult<(u8, Rounding)> {
    let min_integer = options
        .number(rt, "minimumIntegerDigits", 1.0, 21.0)?
        .unwrap_or(1);
    let min_fraction = options.number(rt, "minimumFractionDigits", 0.0, 100.0)?;
    let max_fraction = options.number(rt, "maximumFractionDigits", 0.0, 100.0)?;
    let min_sig = options.number(rt, "minimumSignificantDigits", 1.0, 21.0)?;
    let max_sig = options.number(rt, "maximumSignificantDigits", 1.0, 21.0)?;
    options.number(rt, "roundingIncrement", 1.0, 5000.0)?;
    options.string(
        rt,
        "roundingMode",
        &[
            "ceil",
            "floor",
            "expand",
            "trunc",
            "halfCeil",
            "halfFloor",
            "halfExpand",
            "halfTrunc",
            "halfEven",
        ],
    )?;
    options.string(
        rt,
        "roundingPriority",
        &["auto", "morePrecision", "lessPrecision"],
    )?;
    options.string(rt, "trailingZeroDisplay", &["auto", "stripIfInteger"])?;
    let out_of_range = |name: &str| VmError::range_error(format!("{name} value is out of range."));
    let rounding = if min_sig.is_some() || max_sig.is_some() {
        let min = min_sig.unwrap_or(1);
        let max = max_sig.unwrap_or(21);
        if max < min {
            return Err(out_of_range("maximumSignificantDigits"));
        }
        Rounding::Significant(min as u8, max as u8)
    } else if min_fraction.is_some() || max_fraction.is_some() {
        let (min, max) = match (min_fraction, max_fraction) {
            (Some(min), None) => (min, default_max.max(min)),
            (None, Some(max)) => (default_min.min(max), max),
            (Some(min), Some(max)) if min > max => {
                return Err(out_of_range("maximumFractionDigits"));
            }
            (Some(min), Some(max)) => (min, max),
            (None, None) => (default_min, default_max),
        };
        Rounding::Fraction(min as u8, max as u8)
    } else {
        Rounding::Fraction(default_min as u8, default_max as u8)
    };
    Ok((min_integer as u8, rounding))
}

/// `useGrouping` (§15.1.2 `GetBooleanOrStringNumberFormatOption`).
fn grouping(rt: &mut Runtime, options: &Options) -> VmResult<Grouping> {
    let value = options.get(rt, "useGrouping")?;
    let text = match value {
        Value::Undefined => return Ok(Grouping::Auto),
        Value::Bool(true) => return Ok(Grouping::Always),
        Value::Bool(false) => return Ok(Grouping::Off),
        _ if !rt.to_boolean(value)? => return Ok(Grouping::Off),
        other => super::intl::to_text(rt, other)?,
    };
    match text.as_str() {
        "min2" => Ok(Grouping::Min2),
        "auto" | "true" | "false" => Ok(Grouping::Auto),
        "always" => Ok(Grouping::Always),
        _ => Err(VmError::range_error(format!(
            "Value {text} out of range for {} options property useGrouping",
            options.site()
        ))),
    }
}

/// `InitializeNumberFormat` (§15.1.2).
pub(super) fn create(
    rt: &mut Runtime,
    locales: Value,
    options: Value,
    site: &'static str,
) -> VmResult<NumberFormat> {
    let locale = resolve_locale(rt, locales)?;
    let null_message = format!("{site} called on null or undefined");
    let options = Options::new(rt, options, site, &null_message)?;
    options.string(rt, "localeMatcher", &["lookup", "best fit"])?;
    options.numbering_system(rt)?;
    let style = unit_options(rt, &options)?;
    options.string(
        rt,
        "notation",
        &["standard", "scientific", "engineering", "compact"],
    )?;
    let (default_min, default_max) = match &style {
        Style::Currency(code, ..) => {
            let digits = u32::from(currency_digits(code));
            (digits, digits)
        }
        Style::Percent => (0, 0),
        _ => (0, 3),
    };
    let (min_integer, rounding) = digit_options(rt, &options, default_min, default_max)?;
    options.string(rt, "compactDisplay", &["short", "long"])?;
    let grouping = grouping(rt, &options)?;
    let sign = match options
        .string(
            rt,
            "signDisplay",
            &["auto", "never", "always", "exceptZero", "negative"],
        )?
        .as_deref()
    {
        Some("never") => SignDisplay::Never,
        Some("always") => SignDisplay::Always,
        Some("exceptZero") => SignDisplay::ExceptZero,
        Some("negative") => SignDisplay::Negative,
        _ => SignDisplay::Auto,
    };
    Ok(NumberFormat {
        locale,
        style,
        min_integer,
        rounding,
        grouping,
        sign,
    })
}

/// `style` and `SetNumberFormatUnitOptions` (§15.1.3): each check follows
/// the read of its option, in this order (measured with a logging proxy in
/// Chromium 148).
fn unit_options(rt: &mut Runtime, options: &Options) -> VmResult<Style> {
    let style = options
        .string(rt, "style", &["decimal", "percent", "currency", "unit"])?
        .unwrap_or_else(|| "decimal".to_owned());
    let currency = options.string(rt, "currency", &[])?;
    if let Some(code) = &currency
        && !(code.len() == 3 && code.bytes().all(|b| b.is_ascii_alphabetic()))
    {
        return Err(VmError::range_error(format!(
            "Invalid currency code : {code}"
        )));
    }
    if style == "currency" && currency.is_none() {
        return Err(VmError::type_error(
            "Currency code is required with currency style.",
        ));
    }
    let currency_display = options.string(
        rt,
        "currencyDisplay",
        &["code", "symbol", "narrowSymbol", "name"],
    )?;
    let currency_sign = options.string(rt, "currencySign", &["standard", "accounting"])?;
    let unit = options.string(rt, "unit", &[])?;
    if let Some(name) = &unit
        && !sanctioned(name)
    {
        return Err(VmError::range_error(format!(
            "Invalid unit argument for Intl.NumberFormat() '{name}'"
        )));
    }
    if style == "unit" && unit.is_none() {
        return Err(VmError::type_error(
            "Invalid unit argument for Intl.NumberFormat() ''",
        ));
    }
    let unit_display = options.string(rt, "unitDisplay", &["short", "narrow", "long"])?;
    Ok(match style.as_str() {
        "currency" => {
            let code = currency.unwrap_or_default();
            let display = match currency_display.as_deref() {
                Some("code") => CurrencyDisplay::Code,
                Some("narrowSymbol") => CurrencyDisplay::Narrow,
                Some("name") => CurrencyDisplay::Name,
                _ => CurrencyDisplay::Symbol,
            };
            Style::Currency(
                code.to_ascii_uppercase(),
                display,
                currency_sign.as_deref() == Some("accounting"),
            )
        }
        "unit" => {
            let name = unit.unwrap_or_default();
            let display = match unit_display.as_deref() {
                Some("narrow") => UnitDisplay::Narrow,
                Some("long") => UnitDisplay::Long,
                _ => UnitDisplay::Short,
            };
            Style::Unit(name, display)
        }
        "percent" => Style::Percent,
        _ => Style::Decimal,
    })
}

/// `Intl.NumberFormat([locales [, options]])` (§15.1.1).
fn constructor_fn(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let format = create(rt, rt.arg(call, 0), rt.arg(call, 1), CONSTRUCTOR)?;
    let object = new_object(rt, call, IntlData::Number(format))?;
    Ok(NativeReturn::Value(object.into()))
}

/// The number format of `this`; V8 words every failure "Method
/// `UnwrapNumberFormat` called on incompatible receiver undefined".
fn this_number(rt: &Runtime, call: &NativeCall, method: Option<&str>) -> VmResult<NumberFormat> {
    match this_format(rt, call.this(), |d| matches!(d, IntlData::Number(_))) {
        Some((_, IntlData::Number(format))) => Ok(format),
        _ => Err(match method {
            Some(name) => rt.incompatible_receiver(name, call.this()),
            None => rt.incompatible_receiver("UnwrapNumberFormat", Value::Undefined),
        }),
    }
}

/// `ToIntlMathematicalValue` (§15.5.1): a `BigInt` and a numeric string
/// keep their exact value.
pub(super) fn intl_value(rt: &mut Runtime, value: Value) -> VmResult<Number> {
    let primitive = rt.to_primitive(value, Hint::Number)?;
    match primitive {
        Value::BigInt(big) => {
            let text = rt.bigint_text(big);
            rt.charge_units(text.len())?;
            Ok(Number::parse_decimal(text.bytes().map(u16::from), true).unwrap_or(Number::Nan))
        }
        Value::String(string) => {
            // The units are read in place: no copy of a long string is made
            // (`parse_decimal` keeps only the digits that matter).
            let units = rt.flat_text(string)?;
            let length = units.len();
            let is_space = |unit: u16| {
                char::from_u32(u32::from(unit))
                    .is_some_and(|c| c.is_whitespace() || c == '\u{feff}')
            };
            let start = (0..length)
                .find(|i| !units.get(*i).is_some_and(is_space))
                .unwrap_or(length);
            let end = (start..length)
                .rev()
                .find(|i| !units.get(*i).is_some_and(is_space))
                .map_or(start, |last| last + 1);
            let trimmed = units.slice(start, end);
            let number = trimmed.and_then(|text| Number::parse_decimal(text.units(), false));
            rt.charge_units(length)?;
            if start == end {
                return Ok(Number::from_f64(0.0));
            }
            if let Some(number) = number {
                return Ok(number);
            }
            Ok(Number::from_f64(rt.to_number(primitive)?))
        }
        other => Ok(Number::from_f64(rt.to_number(other)?)),
    }
}

/// The function of the getter `format`.
fn format_getter(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let Some((object, _)) = this_format(rt, call.this(), |d| matches!(d, IntlData::Number(_)))
    else {
        return Err(rt.incompatible_receiver("UnwrapNumberFormat", Value::Undefined));
    };
    if let Some(bound) = super::intl::bound_format(rt, object)? {
        return Ok(NativeReturn::Value(bound.into()));
    }
    let slots = Box::new([Value::Object(object)]) as Box<[Value]>;
    let function = rt.new_builtin_with_slots(call.realm(), 1, bound_format, slots)?;
    super::intl::set_bound_format(rt, object, function)?;
    Ok(NativeReturn::Value(function.into()))
}

/// The bound `format` function (§15.5.3 `Number Format Functions`).
fn bound_format(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let Value::Object(object) = rt.native_slot(call.callee(), 0)? else {
        return Err(VmError::invariant("a bound format without its object"));
    };
    let Some((_, IntlData::Number(format))) = this_format(rt, Value::Object(object), |d| {
        matches!(d, IntlData::Number(_))
    }) else {
        return Err(VmError::invariant("a bound format of another object"));
    };
    let number = intl_value(rt, rt.arg(call, 0))?;
    check_size(&format, &number)?;
    let text = format.format(&number);
    rt.charge_units(text.len())?;
    let value = string_value(rt, &text)?;
    Ok(NativeReturn::Value(value))
}

/// The `RangeError` for a number whose text would have too many digits.
fn check_size(format: &NumberFormat, number: &Number) -> VmResult<()> {
    if format.is_too_long(number) {
        return Err(VmError::range_error("Invalid string length"));
    }
    Ok(())
}

/// `Intl.NumberFormat.prototype.formatToParts(value)` (§15.3.4).
fn format_to_parts(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let format = this_number(rt, call, Some("Intl.NumberFormat.prototype.formatToParts"))?;
    let number = intl_value(rt, rt.arg(call, 0))?;
    check_size(&format, &number)?;
    let parts: Vec<(&str, String, Option<&str>)> = format
        .format_to_parts(&number)
        .into_iter()
        .map(|(kind, text)| (kind, text, None))
        .collect();
    Ok(NativeReturn::Value(parts_array(rt, call.realm(), &parts)?))
}

/// `Intl.NumberFormat.prototype.resolvedOptions()` (§15.3.5).
fn resolved_options(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let f = this_number(rt, call, None)?;
    let object = plain_object(rt, call.realm())?;
    set_text(rt, object, "locale", &f.locale.tag)?;
    set_text(rt, object, "numberingSystem", "latn")?;
    match &f.style {
        Style::Decimal => set_text(rt, object, "style", "decimal")?,
        Style::Percent => set_text(rt, object, "style", "percent")?,
        Style::Currency(code, display, accounting) => {
            set_text(rt, object, "style", "currency")?;
            set_text(rt, object, "currency", code)?;
            let display = match display {
                CurrencyDisplay::Symbol => "symbol",
                CurrencyDisplay::Narrow => "narrowSymbol",
                CurrencyDisplay::Code => "code",
                CurrencyDisplay::Name => "name",
            };
            set_text(rt, object, "currencyDisplay", display)?;
            set_text(
                rt,
                object,
                "currencySign",
                if *accounting {
                    "accounting"
                } else {
                    "standard"
                },
            )?;
        }
        Style::Unit(unit, display) => {
            set_text(rt, object, "style", "unit")?;
            set_text(rt, object, "unit", unit)?;
            let display = match display {
                UnitDisplay::Short => "short",
                UnitDisplay::Narrow => "narrow",
                UnitDisplay::Long => "long",
            };
            set_text(rt, object, "unitDisplay", display)?;
        }
    }
    set(
        rt,
        object,
        "minimumIntegerDigits",
        Value::Int(i32::from(f.min_integer)),
    )?;
    match f.rounding {
        Rounding::Fraction(min, max) => {
            set(
                rt,
                object,
                "minimumFractionDigits",
                Value::Int(i32::from(min)),
            )?;
            set(
                rt,
                object,
                "maximumFractionDigits",
                Value::Int(i32::from(max)),
            )?;
        }
        Rounding::Significant(min, max) => {
            set(
                rt,
                object,
                "minimumSignificantDigits",
                Value::Int(i32::from(min)),
            )?;
            set(
                rt,
                object,
                "maximumSignificantDigits",
                Value::Int(i32::from(max)),
            )?;
        }
    }
    match f.grouping {
        Grouping::Off => set(rt, object, "useGrouping", Value::Bool(false))?,
        Grouping::Auto => set_text(rt, object, "useGrouping", "auto")?,
        Grouping::Always => set_text(rt, object, "useGrouping", "always")?,
        Grouping::Min2 => set_text(rt, object, "useGrouping", "min2")?,
    }
    set_text(rt, object, "notation", "standard")?;
    let sign = match f.sign {
        SignDisplay::Auto => "auto",
        SignDisplay::Never => "never",
        SignDisplay::Always => "always",
        SignDisplay::ExceptZero => "exceptZero",
        SignDisplay::Negative => "negative",
    };
    set_text(rt, object, "signDisplay", sign)?;
    set(rt, object, "roundingIncrement", Value::Int(1))?;
    set_text(rt, object, "roundingMode", "halfExpand")?;
    set_text(rt, object, "roundingPriority", "auto")?;
    set_text(rt, object, "trailingZeroDisplay", "auto")?;
    Ok(NativeReturn::Value(object.into()))
}

/// `Number.prototype.toLocaleString(locales, options)` and its `BigInt`
/// twin when they get arguments: the text of the exact value.
pub(super) fn to_locale_text(
    rt: &mut Runtime,
    call: &NativeCall,
    number: &Number,
    site: &'static str,
) -> VmResult<String> {
    let format = create(rt, rt.arg(call, 0), rt.arg(call, 1), site)?;
    check_size(&format, number)?;
    let text = format.format(number);
    rt.charge_units(text.len())?;
    Ok(text)
}
