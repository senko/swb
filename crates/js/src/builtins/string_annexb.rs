//! The Annex B string methods (ECMA-262 §B.2.2): `substr` and the HTML
//! methods `anchor`, `big`, `blink`, `bold`, `fixed`, `fontcolor`,
//! `fontsize`, `italics`, `link`, `small`, `strike`, `sub` and `sup`
//! (`CreateHTML`, §B.2.2.2.1). `trimLeft` and `trimRight` are the same
//! function objects as `trimStart` and `trimEnd`
//! ([`super::string`]); `escape` and `unescape` are in [`super::uri`].

use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::convert::to_integer_or_infinity;
use crate::vm::{NativeCall, NativeReturn, VmResult};

use super::string::{Builder, string_result};

/// `String.prototype.substr(start, length)` (§B.2.2.1).
pub(super) fn substr(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let string = rt.this_string(call, "String.prototype.substr")?;
    let size = rt.string_len(string)? as f64;
    let int_start = rt.integer_arg(call, 0)?;
    let start = if int_start == f64::NEG_INFINITY {
        0.0
    } else if int_start < 0.0 {
        (size + int_start).max(0.0)
    } else {
        int_start.min(size)
    };
    let length = match rt.arg(call, 1) {
        Value::Undefined => size,
        value => to_integer_or_infinity(rt.to_number(value)?),
    }
    .clamp(0.0, size);
    let end = (start + length).min(size);
    string_result(rt.substring(string, start as usize, end as usize)?)
}

/// `CreateHTML(string, tag, attribute, value)` (§B.2.2.2.1): the tag
/// around the string, with the attribute (if any) from argument 0, whose
/// `"` become `&quot;`.
fn html(
    rt: &mut Runtime,
    call: &NativeCall,
    method: &str,
    tag: &str,
    attribute: Option<&str>,
) -> VmResult<NativeReturn> {
    let string = rt.this_string(call, method)?;
    let mut out = Builder::new();
    out.check(rt, tag.len() + 1)?;
    out.push_unit(u16::from(b'<'));
    push_ascii(&mut out, tag);
    if let Some(attribute) = attribute {
        let value = rt.string_arg(call, 0)?;
        out.check(rt, attribute.len() + 3)?;
        out.push_unit(u16::from(b' '));
        push_ascii(&mut out, attribute);
        push_ascii(&mut out, "=\"");
        push_escaped(rt, &mut out, value)?;
        out.check(rt, 1)?;
        out.push_unit(u16::from(b'"'));
    }
    out.check(rt, 1)?;
    out.push_unit(u16::from(b'>'));
    out.push_string(rt, string)?;
    out.check(rt, tag.len() + 3)?;
    push_ascii(&mut out, "</");
    push_ascii(&mut out, tag);
    out.push_unit(u16::from(b'>'));
    string_result(out.finish(rt)?)
}

/// Appends ASCII text; the caller checked the room.
fn push_ascii(out: &mut Builder, text: &str) {
    for byte in text.bytes() {
        out.push_unit(u16::from(byte));
    }
}

/// Appends `value` with each `"` replaced by `&quot;`.
fn push_escaped(
    rt: &mut Runtime,
    out: &mut Builder,
    value: crate::heap::Gc<crate::string::JsString>,
) -> VmResult<()> {
    // The scan runs in chunks, so a long value reaches the time check.
    const CHUNK: usize = 1 << 16;
    rt.flatten(value)?;
    let length = rt.string_len(value)?;
    let mut start = 0;
    let mut index = 0;
    while index < length {
        let end = index.saturating_add(CHUNK).min(length);
        let quote = {
            let text = rt.heap.text(value)?;
            (index..end).find(|&i| text.get(i) == Some(u16::from(b'"')))
        };
        rt.charge_units(quote.unwrap_or(end) - index)?;
        match quote {
            Some(quote) => {
                out.push_range(rt, value, start, quote)?;
                out.check(rt, 6)?;
                push_ascii(out, "&quot;");
                start = quote + 1;
                index = quote + 1;
            }
            None => index = end,
        }
    }
    out.push_range(rt, value, start, length)
}

/// Defines a native HTML method.
macro_rules! html_method {
    ($name:ident, $method:literal, $tag:literal, $attribute:expr) => {
        #[doc = concat!("`String.prototype.", $method, "` (§B.2.2).")]
        pub(super) fn $name(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
            html(
                rt,
                call,
                concat!("String.prototype.", $method),
                $tag,
                $attribute,
            )
        }
    };
}

html_method!(anchor, "anchor", "a", Some("name"));
html_method!(big, "big", "big", None);
html_method!(blink, "blink", "blink", None);
html_method!(bold, "bold", "b", None);
html_method!(fixed, "fixed", "tt", None);
html_method!(fontcolor, "fontcolor", "font", Some("color"));
html_method!(fontsize, "fontsize", "font", Some("size"));
html_method!(italics, "italics", "i", None);
html_method!(link, "link", "a", Some("href"));
html_method!(small, "small", "small", None);
html_method!(strike, "strike", "strike", None);
html_method!(sub, "sub", "sub", None);
html_method!(sup, "sup", "sup", None);
