//! `Intl.DateTimeFormat` (ECMA-402 §11,
//! <https://tc39.es/ecma402/#datetimeformat-objects>): the constructor with
//! the options of `weekday`, `era`, `year`, `month`, `day`, `hour`,
//! `minute`, `second`, `fractionalSecondDigits`, `timeZoneName`, `hour12`,
//! `hourCycle`, `timeZone`, `dateStyle` and `timeStyle`; `format` (a bound
//! getter), `formatToParts`, `resolvedOptions` and `supportedLocalesOf`;
//! and the arguments of `Date.prototype.toLocaleString`,
//! `toLocaleDateString` and `toLocaleTimeString` (§20.4). The formatting is
//! in `crate::intl::datetime`.

use super::intl::{
    Options, constructor_on, is_type_sequence, new_object, parts_array, plain_object,
    resolve_locale, set, set_text, string_value, supported_locales_of, this_format, to_text,
};
use crate::date::time_value::time_clip;
use crate::date::{TimeZone, canonical_case};
use crate::heap::Gc;
use crate::intl::datetime::{
    Components, DateTimeFormat, HourCycle, Request, Style, Width, ZoneName,
};
use crate::intl::{IntlData, canonical_zone};
use crate::object::Object;
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::{NativeCall, NativeReturn, VmError, VmResult};

const CONSTRUCTOR: &str = "Intl.DateTimeFormat";

/// `Intl.DateTimeFormat.supportedLocalesOf(locales [, options])`.
fn supported_locales_of_fn(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    supported_locales_of(rt, call, "Intl.DateTimeFormat.supportedLocalesOf")
}

pub(super) fn install(rt: &mut Runtime, realm: u32, intl: Gc<Object>) -> VmResult<()> {
    let (constructor, proto) = constructor_on(rt, realm, intl, "DateTimeFormat", constructor_fn)?;
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
    super::to_string_tag(rt, proto, "Intl.DateTimeFormat")
}

/// What a call needs of the options (`ToDateTimeOptions`, §11.1.1).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Required {
    /// A date or a time field: `toLocaleString` and the constructor.
    Any,
    /// A date field: `toLocaleDateString`.
    Date,
    /// A time field: `toLocaleTimeString`.
    Time,
}

/// The fields that an absent request is given.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Defaults {
    /// Year, month and day.
    Date,
    /// Hour, minute and second.
    Time,
    /// Both.
    All,
}

fn width(value: Option<String>) -> Option<Width> {
    Some(match value?.as_str() {
        "numeric" => Width::Numeric,
        "2-digit" => Width::TwoDigit,
        "narrow" => Width::Narrow,
        "short" => Width::Short,
        _ => Width::Long,
    })
}

fn style(value: Option<String>) -> Option<Style> {
    Some(match value?.as_str() {
        "full" => Style::Full,
        "long" => Style::Long,
        "medium" => Style::Medium,
        _ => Style::Short,
    })
}

const NAMES: [&str; 3] = ["narrow", "short", "long"];
const DIGITS: [&str; 2] = ["2-digit", "numeric"];
const MONTHS: [&str; 5] = ["2-digit", "numeric", "narrow", "short", "long"];
const STYLES: [&str; 4] = ["full", "long", "medium", "short"];

/// The time zone of a name or an offset (§11.1.1 step 28 with
/// `CanonicalizeTimeZoneName`): the zone and the name to report.
fn resolve_zone(rt: &mut Runtime, value: Value) -> VmResult<(TimeZone, String)> {
    if value.is_undefined() {
        let zone = rt.vm.time.zone().clone();
        let name = if is_utc_name(zone.name()) {
            "UTC"
        } else {
            canonical_zone(zone.name()).unwrap_or(zone.name())
        }
        .to_owned();
        return Ok((zone, name));
    }
    let text = to_text(rt, value)?;
    let invalid = || VmError::range_error(format!("Invalid time zone specified: {text}"));
    if let Some(offset) = parse_offset(&text) {
        let zone = match &offset.rule {
            Some(rule) => TimeZone::from_spec(rule).map_err(|_| invalid())?,
            // The offset zero shows the names of Greenwich Mean Time
            // (`GMT`, measured), not those of UTC.
            None => TimeZone::from_spec("Etc/GMT")
                .ok()
                .filter(TimeZone::is_named)
                .unwrap_or_else(TimeZone::utc),
        };
        return Ok((zone, offset.name));
    }
    let lower = text.to_ascii_lowercase();
    if UTC_NAMES.contains(&lower.as_str()) {
        return Ok((TimeZone::utc(), "UTC".to_owned()));
    }
    if text != text.trim() || lower == "etc/unknown" {
        return Err(invalid());
    }
    // The database of the system may lack the old name of CLDR or have
    // only that name: try the name as given, the canonical one and the
    // current identifier of a legacy link.
    let canonical = canonical_zone(&text).unwrap_or(&text);
    let spelled = canonical_case(&text).unwrap_or(&text);
    let current = LEGACY_LINKS
        .iter()
        .find(|(old, _)| *old == canonical)
        .map_or(canonical, |(_, current)| current);
    let zone = [spelled, canonical, current]
        .into_iter()
        .find_map(|name| TimeZone::from_spec(name).ok().filter(TimeZone::is_named))
        .ok_or_else(invalid)?;
    let name = if is_utc_name(&text) { "UTC" } else { canonical };
    Ok((zone, name.to_owned()))
}

/// The names of the IANA database that are the zone `UTC` (CLDR calls
/// them `Etc/UTC` or `Etc/GMT`; Chromium 148 reports `UTC` and the display
/// names of UTC for all of them: measured).
const UTC_NAMES: [&str; 16] = [
    "utc",
    "gmt",
    "etc/utc",
    "etc/gmt",
    "zulu",
    "uct",
    "etc/uct",
    "etc/zulu",
    "universal",
    "etc/universal",
    "gmt0",
    "etc/gmt0",
    "etc/gmt+0",
    "etc/gmt-0",
    "gmt+0",
    "gmt-0",
];

/// Whether `resolvedOptions().timeZone` is `UTC` for the name: the names
/// of [`UTC_NAMES`], and `Greenwich` and `Etc/Greenwich` (whose display
/// names stay those of Greenwich Mean Time).
fn is_utc_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    UTC_NAMES.contains(&lower.as_str()) || lower == "greenwich" || lower == "etc/greenwich"
}

/// Names that CLDR spells with the old identifier of the IANA database and
/// that a database without the "backward" links lacks: the current
/// identifier of each.
const LEGACY_LINKS: [(&str, &str); 11] = [
    ("Pacific/Ponape", "Pacific/Pohnpei"),
    ("Africa/Asmera", "Africa/Asmara"),
    ("America/Indianapolis", "America/Indiana/Indianapolis"),
    ("Asia/Calcutta", "Asia/Kolkata"),
    ("Asia/Katmandu", "Asia/Kathmandu"),
    ("Asia/Saigon", "Asia/Ho_Chi_Minh"),
    ("Asia/Rangoon", "Asia/Yangon"),
    ("Europe/Kiev", "Europe/Kyiv"),
    ("America/Buenos_Aires", "America/Argentina/Buenos_Aires"),
    ("Atlantic/Faeroe", "Atlantic/Faroe"),
    ("Pacific/Truk", "Pacific/Chuuk"),
];

/// A time zone given as an offset (`+02:00`, `-0530`, `+03`).
struct Offset {
    /// The name to report: `+02:00`.
    name: String,
    /// The POSIX rule of the zone; none for the offset zero (UTC, also
    /// written `-00:00`).
    rule: Option<String>,
}

fn parse_offset(text: &str) -> Option<Offset> {
    let sign = match text.as_bytes().first()? {
        b'+' => '+',
        b'-' => '-',
        _ => return None,
    };
    let digits: String = text[1..].chars().filter(|c| *c != ':').collect();
    let colon_ok = !text[1..].contains(':') || text.len() == 6 && text.as_bytes()[3] == b':';
    if !colon_ok || !digits.bytes().all(|b| b.is_ascii_digit()) || !matches!(digits.len(), 2 | 4) {
        return None;
    }
    let hours: u32 = digits[..2].parse().ok()?;
    let minutes: u32 = if digits.len() == 4 {
        digits[2..].parse().ok()?
    } else {
        0
    };
    if hours > 23 || minutes > 59 {
        return None;
    }
    if hours == 0 && minutes == 0 {
        return Some(Offset {
            name: "+00:00".to_owned(),
            rule: None,
        });
    }
    // POSIX counts the offset west of Greenwich: `<+02:00>-2:00`.
    let opposite = if sign == '+' { '-' } else { '+' };
    let name = format!("{sign}{hours:02}:{minutes:02}");
    let rule = format!("<{sign}{hours:02}{minutes:02}>{opposite}{hours}:{minutes:02}");
    Some(Offset {
        name,
        rule: Some(rule),
    })
}

/// The component options in the order of the specification (§11.1.2
/// steps 28 to 36): the components, and whether `dayPeriod` was given. It
/// has no effect and does not count as a time field for the defaults (ADR
/// 0027 section 10), but it conflicts with a style.
fn read_components(rt: &mut Runtime, options: &Options) -> VmResult<(Components, bool)> {
    let mut c = Components {
        weekday: width(options.string(rt, "weekday", &NAMES)?),
        era: width(options.string(rt, "era", &NAMES)?),
        year: width(options.string(rt, "year", &DIGITS)?),
        month: width(options.string(rt, "month", &MONTHS)?),
        day: width(options.string(rt, "day", &DIGITS)?),
        ..Components::default()
    };
    let day_period = options.string(rt, "dayPeriod", &NAMES)?.is_some();
    c.hour = width(options.string(rt, "hour", &DIGITS)?);
    c.minute = width(options.string(rt, "minute", &DIGITS)?);
    c.second = width(options.string(rt, "second", &DIGITS)?);
    c.fraction = options
        .number(rt, "fractionalSecondDigits", 1.0, 3.0)?
        .map(|n| n as u8);
    c.zone_name = options
        .string(
            rt,
            "timeZoneName",
            &[
                "short",
                "long",
                "shortOffset",
                "longOffset",
                "shortGeneric",
                "longGeneric",
            ],
        )?
        .map(|name| match name.as_str() {
            "short" => ZoneName::Short,
            "long" => ZoneName::Long,
            "shortOffset" => ZoneName::ShortOffset,
            "longOffset" => ZoneName::LongOffset,
            "shortGeneric" => ZoneName::ShortGeneric,
            _ => ZoneName::LongGeneric,
        });
    Ok((c, day_period))
}

/// The checks and the defaults of `ToDateTimeOptions` (§11.1.1): a style
/// cannot be mixed with components; without styles and components of the
/// kind that the call needs, the defaults are added.
fn check_and_default(
    c: &mut Components,
    day_period: bool,
    styles: (Option<Style>, Option<Style>),
    required: Required,
    defaults: Defaults,
) -> VmResult<()> {
    let (date_style, time_style) = styles;
    let date_fields =
        c.weekday.is_some() || c.year.is_some() || c.month.is_some() || c.day.is_some();
    let time_fields =
        c.hour.is_some() || c.minute.is_some() || c.second.is_some() || c.fraction.is_some();
    if date_style.is_some() || time_style.is_some() {
        if required == Required::Date && time_style.is_some() {
            return Err(VmError::type_error("Invalid option : timeStyle"));
        }
        if required == Required::Time && date_style.is_some() {
            return Err(VmError::type_error("Invalid option : dateStyle"));
        }
        if date_fields || time_fields || day_period || c.era.is_some() || c.zone_name.is_some() {
            return Err(VmError::type_error("Invalid option : option"));
        }
        return Ok(());
    }
    let need = match required {
        Required::Date => !date_fields,
        Required::Time => !time_fields,
        Required::Any => !date_fields && !time_fields,
    };
    if need {
        if defaults != Defaults::Time {
            c.year = Some(Width::Numeric);
            c.month = Some(Width::Numeric);
            c.day = Some(Width::Numeric);
        }
        if defaults != Defaults::Date {
            c.hour = Some(Width::Numeric);
            c.minute = Some(Width::Numeric);
            c.second = Some(Width::Numeric);
        }
    }
    Ok(())
}

/// `CreateDateTimeFormat` (§11.1.2).
pub(super) fn create(
    rt: &mut Runtime,
    locales: Value,
    options: Value,
    required: Required,
    defaults: Defaults,
    site: &'static str,
) -> VmResult<DateTimeFormat> {
    let locale = resolve_locale(rt, locales)?;
    let null_message = format!("{site} called on null or undefined");
    let options = Options::new(rt, options, site, &null_message)?;
    options.string(rt, "localeMatcher", &["lookup", "best fit"])?;
    if let Some(calendar) = options.string(rt, "calendar", &[])?
        && !is_type_sequence(&calendar)
    {
        return Err(VmError::range_error(format!(
            "Invalid calendar : {calendar}"
        )));
    }
    options.numbering_system(rt)?;
    let hour12 = options.boolean(rt, "hour12")?;
    let hour_cycle = options.string(rt, "hourCycle", &["h11", "h12", "h23", "h24"])?;
    // The time zone is checked right after it is read (§11.1.2).
    let zone_value = options.get(rt, "timeZone")?;
    let (zone, zone_id) = resolve_zone(rt, zone_value)?;
    let (mut components, day_period) = read_components(rt, &options)?;
    options.string(rt, "formatMatcher", &["basic", "best fit"])?;
    let date_style = style(options.string(rt, "dateStyle", &STYLES)?);
    let time_style = style(options.string(rt, "timeStyle", &STYLES)?);
    check_and_default(
        &mut components,
        day_period,
        (date_style, time_style),
        required,
        defaults,
    )?;
    let request = Request {
        components,
        date_style,
        time_style,
        hour12,
        hour_cycle: hour_cycle.map(|h| match h.as_str() {
            "h11" => HourCycle::H11,
            "h12" => HourCycle::H12,
            "h23" => HourCycle::H23,
            _ => HourCycle::H24,
        }),
    };
    Ok(DateTimeFormat::new(locale, zone, zone_id, &request))
}

/// `Intl.DateTimeFormat([locales [, options]])` (§11.1.1): with or without
/// `new`.
fn constructor_fn(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let format = create(
        rt,
        rt.arg(call, 0),
        rt.arg(call, 1),
        Required::Any,
        Defaults::Date,
        CONSTRUCTOR,
    )?;
    let object = new_object(rt, call, IntlData::DateTime(format))?;
    Ok(NativeReturn::Value(object.into()))
}

/// The date-time format of `this` or V8's `TypeError` with `message`.
fn this_datetime(
    rt: &Runtime,
    call: &NativeCall,
    for_object: &str,
    for_primitive: &str,
) -> VmResult<(Gc<Object>, DateTimeFormat)> {
    if let Some((object, IntlData::DateTime(format))) =
        this_format(rt, call.this(), |d| matches!(d, IntlData::DateTime(_)))
    {
        return Ok((object, format));
    }
    // V8 names the unwrapping function for an object and the method for a
    // primitive.
    let name = match call.this() {
        Value::Object(_) => for_object,
        _ => for_primitive,
    };
    Err(rt.incompatible_receiver(name, call.this()))
}

/// `ToNumber` and `TimeClip` of the date argument of `format`: now if it
/// is `undefined`, a `RangeError` if it is not a time value.
fn time_of(rt: &mut Runtime, date: Value) -> VmResult<i64> {
    let x = if date.is_undefined() {
        rt.vm.time.now()
    } else {
        rt.to_number(date)?
    };
    let t = time_clip(x);
    if t.is_nan() {
        return Err(VmError::range_error("Invalid time value"));
    }
    Ok(t as i64)
}

/// The function of the getter `format`.
fn format_getter(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (object, _) = this_datetime(
        rt,
        call,
        "UnwrapDateTimeFormat",
        "get Intl.DateTimeFormat.prototype.format",
    )?;
    if let Some(bound) = super::intl::bound_format(rt, object)? {
        return Ok(NativeReturn::Value(bound.into()));
    }
    let realm = call.realm();
    let slots = Box::new([Value::Object(object)]) as Box<[Value]>;
    let function = rt.new_builtin_with_slots(realm, 1, bound_format, slots)?;
    super::intl::set_bound_format(rt, object, function)?;
    Ok(NativeReturn::Value(function.into()))
}

/// The bound `format` function (§11.5.4 `DateTime Format Functions`).
fn bound_format(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let Value::Object(object) = rt.native_slot(call.callee(), 0)? else {
        return Err(VmError::invariant("a bound format without its object"));
    };
    let Some((_, IntlData::DateTime(format))) = this_format(rt, Value::Object(object), |d| {
        matches!(d, IntlData::DateTime(_))
    }) else {
        return Err(VmError::invariant("a bound format of another object"));
    };
    let t = time_of(rt, rt.arg(call, 0))?;
    let text = format.format(t);
    let value = string_value(rt, &text)?;
    Ok(NativeReturn::Value(value))
}

/// `Intl.DateTimeFormat.prototype.formatToParts(date)` (§11.4.4).
fn format_to_parts(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (_, format) = this_datetime(
        rt,
        call,
        "Intl.DateTimeFormat.prototype.formatToParts",
        "Intl.DateTimeFormat.prototype.formatToParts",
    )?;
    let t = time_of(rt, rt.arg(call, 0))?;
    let parts: Vec<(&str, String, Option<&str>)> = format
        .format_to_parts(t)
        .into_iter()
        .map(|p| (p.kind.name(), p.text, None))
        .collect();
    Ok(NativeReturn::Value(parts_array(rt, call.realm(), &parts)?))
}

/// `Intl.DateTimeFormat.prototype.resolvedOptions()` (§11.4.5).
fn resolved_options(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let (_, f) = this_datetime(
        rt,
        call,
        "UnwrapDateTimeFormat",
        "Intl.DateTimeFormat.prototype.resolvedOptions",
    )?;
    let object = plain_object(rt, call.realm())?;
    set_text(rt, object, "locale", &f.locale.tag)?;
    set_text(rt, object, "calendar", "gregory")?;
    set_text(rt, object, "numberingSystem", "latn")?;
    set_text(rt, object, "timeZone", &f.zone_id)?;
    if let Some(cycle) = f.hour_cycle {
        set_text(rt, object, "hourCycle", cycle.name())?;
        set(rt, object, "hour12", Value::Bool(cycle.is_12()))?;
    }
    let c = &f.components;
    for (name, width) in [
        ("weekday", c.weekday),
        ("era", c.era),
        ("year", c.year),
        ("month", c.month),
        ("day", c.day),
        ("hour", c.hour),
        ("minute", c.minute),
        ("second", c.second),
    ] {
        if let Some(width) = width {
            set_text(rt, object, name, width.name())?;
        }
    }
    if let Some(digits) = c.fraction {
        set(
            rt,
            object,
            "fractionalSecondDigits",
            Value::Int(i32::from(digits)),
        )?;
    }
    if let Some(name) = c.zone_name {
        set_text(rt, object, "timeZoneName", name.name())?;
    }
    if let Some(style) = f.date_style {
        set_text(rt, object, "dateStyle", style.name())?;
    }
    if let Some(style) = f.time_style {
        set_text(rt, object, "timeStyle", style.name())?;
    }
    Ok(NativeReturn::Value(object.into()))
}

/// `Date.prototype.toLocaleString` and its two relatives (§20.4.1 to
/// §20.4.3) when they get a locale or options: the date `t` (finite) as
/// text.
pub(super) fn to_locale_text(
    rt: &mut Runtime,
    call: &NativeCall,
    t: i64,
    required: Required,
    defaults: Defaults,
    site: &'static str,
) -> VmResult<String> {
    let format = create(
        rt,
        rt.arg(call, 0),
        rt.arg(call, 1),
        required,
        defaults,
        site,
    )?;
    Ok(format.format(t))
}
