//! `Intl.RelativeTimeFormat` (ECMA-402 §17,
//! <https://tc39.es/ecma402/#relativetimeformat-objects>): the constructor
//! (`style`, `numeric`), `format`, `formatToParts`, `resolvedOptions` and
//! `supportedLocalesOf`. The formatting is in `crate::intl::relative`.

use super::intl::{
    Options, constructor_on, new_object, parts_array, plain_object, resolve_locale, set_text,
    string_value, supported_locales_of, this_format, to_text,
};
use crate::heap::Gc;
use crate::intl::IntlData;
use crate::intl::relative::{RelativeStyle, RelativeTimeFormat, unit_index};
use crate::object::Object;
use crate::runtime::Runtime;
use crate::vm::{NativeCall, NativeReturn, VmError, VmResult};

const CONSTRUCTOR: &str = "Intl.RelativeTimeFormat";

/// `Intl.RelativeTimeFormat.supportedLocalesOf(locales [, options])`.
fn supported_locales_of_fn(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    supported_locales_of(rt, call, "Intl.RelativeTimeFormat.supportedLocalesOf")
}

pub(super) fn install(rt: &mut Runtime, realm: u32, intl: Gc<Object>) -> VmResult<()> {
    let (constructor, proto) =
        constructor_on(rt, realm, intl, "RelativeTimeFormat", constructor_fn)?;
    super::method(
        rt,
        realm,
        constructor,
        "supportedLocalesOf",
        1,
        supported_locales_of_fn,
    )?;
    super::method(rt, realm, proto, "resolvedOptions", 0, resolved_options)?;
    super::method(rt, realm, proto, "format", 2, format)?;
    super::method(rt, realm, proto, "formatToParts", 2, format_to_parts)?;
    super::to_string_tag(rt, proto, "Intl.RelativeTimeFormat")
}

/// `Intl.RelativeTimeFormat([locales [, options]])` (§17.1.1): only with
/// `new`.
fn constructor_fn(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    if call.new_target().is_undefined() {
        return Err(VmError::type_error(
            "Constructor Intl.RelativeTimeFormat requires 'new'",
        ));
    }
    let locale = resolve_locale(rt, rt.arg(call, 0))?;
    let options = Options::new(
        rt,
        rt.arg(call, 1),
        CONSTRUCTOR,
        "Intl.RelativeTimeFormat called on null or undefined",
    )?;
    options.string(rt, "localeMatcher", &["lookup", "best fit"])?;
    options.numbering_system(rt)?;
    let style = match options
        .string(rt, "style", &["long", "short", "narrow"])?
        .as_deref()
    {
        Some("short") => RelativeStyle::Short,
        Some("narrow") => RelativeStyle::Narrow,
        _ => RelativeStyle::Long,
    };
    let auto = options
        .string(rt, "numeric", &["always", "auto"])?
        .as_deref()
        == Some("auto");
    let format = RelativeTimeFormat {
        locale,
        style,
        auto,
    };
    let object = new_object(rt, call, IntlData::Relative(format))?;
    Ok(NativeReturn::Value(object.into()))
}

fn this_relative(rt: &Runtime, call: &NativeCall, method: &str) -> VmResult<RelativeTimeFormat> {
    match this_format(rt, call.this(), |d| matches!(d, IntlData::Relative(_))) {
        Some((_, IntlData::Relative(format))) => Ok(format),
        _ => Err(rt.incompatible_receiver(method, call.this())),
    }
}

/// The parts for the arguments of `format` and `formatToParts`
/// (`PartitionRelativeTimePattern`, §17.5.2).
fn parts_of(
    rt: &mut Runtime,
    call: &NativeCall,
    format: &RelativeTimeFormat,
) -> VmResult<Vec<(&'static str, String, Option<&'static str>)>> {
    let value = rt.to_number(rt.arg(call, 0))?;
    let unit = to_text(rt, rt.arg(call, 1))?;
    if !value.is_finite() {
        return Err(VmError::range_error(
            "Value need to be finite number for Intl.RelativeTimeFormat.prototype.format()",
        ));
    }
    let Some(index) = unit_index(&unit) else {
        return Err(VmError::range_error(format!(
            "Invalid unit argument for Intl.RelativeTimeFormat.prototype.format() '{unit}'"
        )));
    };
    Ok(format.format_to_parts(value, index))
}

/// `Intl.RelativeTimeFormat.prototype.format(value, unit)` (§17.3.3).
fn format(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let f = this_relative(rt, call, "Intl.RelativeTimeFormat.prototype.format")?;
    let parts = parts_of(rt, call, &f)?;
    let text: String = parts.iter().map(|p| p.1.as_str()).collect();
    let value = string_value(rt, &text)?;
    Ok(NativeReturn::Value(value))
}

/// `Intl.RelativeTimeFormat.prototype.formatToParts(value, unit)` (§17.3.4).
fn format_to_parts(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let f = this_relative(rt, call, "Intl.RelativeTimeFormat.prototype.formatToParts")?;
    let parts = parts_of(rt, call, &f)?;
    Ok(NativeReturn::Value(parts_array(rt, call.realm(), &parts)?))
}

/// `Intl.RelativeTimeFormat.prototype.resolvedOptions()` (§17.3.5).
fn resolved_options(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let f = this_relative(
        rt,
        call,
        "Intl.RelativeTimeFormat.prototype.resolvedOptions",
    )?;
    let object = plain_object(rt, call.realm())?;
    set_text(rt, object, "locale", &f.locale.tag)?;
    set_text(rt, object, "style", f.style.name())?;
    set_text(
        rt,
        object,
        "numeric",
        if f.auto { "auto" } else { "always" },
    )?;
    set_text(rt, object, "numberingSystem", "latn")?;
    Ok(NativeReturn::Value(object.into()))
}
