//! The URI handling functions `encodeURI`, `encodeURIComponent`,
//! `decodeURI` and `decodeURIComponent` (ECMA-262 §19.2.6) and the Annex B
//! functions `escape` and `unescape` (§B.2.1).
//!
//! `Encode` and `Decode` (§19.2.6.5, §19.2.6.6) work on the code units of
//! the string: an unpaired surrogate when encoding, a bad escape, a bad
//! UTF-8 sequence (overlong, surrogate or above U+10FFFF) or a missing
//! digit when decoding is a `URIError` with V8's message "URI malformed".
//! The text is processed in chunks of [`CHUNK`] units: before each chunk
//! the builder checks the maximum length and the heap limit for the
//! largest output of the chunk, and the time is charged after it, so a
//! hostile string ends with `RangeError`, the heap limit or the time
//! limit, never in an unbounded Rust loop.

use swb_js_text::{Str16, is_lead_surrogate, is_trail_surrogate};

use crate::error::ThrowKind;
use crate::heap::Gc;
use crate::runtime::Runtime;
use crate::string::JsString;
use crate::vm::{NativeCall, NativeReturn, VmError, VmResult};

use super::string::{Builder, string_result};

/// The input units between two checks of the limits.
const CHUNK: usize = 4096;

/// The units of input per time step (an escape costs about 10 ns a unit).
const UNITS_PER_STEP: usize = 8;

/// The most output units for one input unit of `Encode`: a unit of three
/// UTF-8 bytes gives 9 units.
const ENCODE_EXPANSION: usize = 9;

/// Installs the global functions.
pub(super) fn install(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    for (name, func) in [
        ("decodeURI", decode_uri as crate::vm::NativeFn),
        ("decodeURIComponent", decode_uri_component),
        ("encodeURI", encode_uri),
        ("encodeURIComponent", encode_uri_component),
        ("escape", escape),
        ("unescape", unescape),
    ] {
        let function = rt.new_builtin(realm, name, 1, func, false)?;
        super::define_global(rt, realm, name, function.into())?;
    }
    Ok(())
}

/// The `URIError` of a bad URI.
fn malformed() -> VmError {
    VmError::Raise {
        kind: ThrowKind::UriError,
        message: "URI malformed".into(),
    }
}

/// `;/?:@&=+$,` (`uriReserved`, §19.2.6).
fn is_reserved(unit: u16) -> bool {
    u8::try_from(unit).is_ok_and(|b| b";/?:@&=+$,".contains(&b))
}

/// `uriUnescaped` (§19.2.6): letters, digits and `-_.!~*'()`.
fn is_unescaped(unit: u16) -> bool {
    u8::try_from(unit).is_ok_and(|b| b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b))
}

/// The unescaped set of `escape` (§B.2.1.1): letters, digits and
/// `@*_+-./`.
fn is_escape_unescaped(unit: u16) -> bool {
    u8::try_from(unit).is_ok_and(|b| b.is_ascii_alphanumeric() || b"@*_+-./".contains(&b))
}

/// The value of a hexadecimal digit unit.
fn hex_value(unit: Option<u16>) -> Option<u32> {
    char::from_u32(u32::from(unit?))?.to_digit(16)
}

/// Appends `%XX` for a byte.
fn push_percent(out: &mut Builder, byte: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out.push_unit(u16::from(b'%'));
    out.push_unit(u16::from(HEX[usize::from(byte >> 4)]));
    out.push_unit(u16::from(HEX[usize::from(byte & 15)]));
}

/// Runs `chunk_fn` over the chunks of the string until it is consumed.
/// `chunk_fn(text, start, end, out)` handles the units from `start` to at
/// least `end` (a sequence may end after it) and returns the next index;
/// `expansion` is the most output units per input unit.
fn drive(
    rt: &mut Runtime,
    string: Gc<JsString>,
    expansion: usize,
    mut chunk_fn: impl FnMut(Str16<'_>, usize, usize, &mut Builder) -> VmResult<usize>,
) -> VmResult<NativeReturn> {
    rt.flatten(string)?;
    let length = rt.string_len(string)?;
    let mut out = Builder::new();
    let mut k = 0;
    while k < length {
        let end = (k + CHUNK).min(length);
        out.check(rt, expansion * (end - k) + 12)?;
        let text = rt.heap.text(string)?;
        let next = chunk_fn(text, k, end, &mut out)?;
        rt.charge((end - k) / UNITS_PER_STEP)?;
        k = next.max(end);
    }
    string_result(out.finish(rt)?)
}

// --- encodeURI, encodeURIComponent ---

/// `Encode(string, extraUnescaped)` (§19.2.6.5).
fn encode(rt: &mut Runtime, call: &NativeCall, extra: fn(u16) -> bool) -> VmResult<NativeReturn> {
    let string = rt.string_arg(call, 0)?;
    drive(rt, string, ENCODE_EXPANSION, |text, start, end, out| {
        let mut k = start;
        while k < end {
            let unit = text.get(k).ok_or_else(malformed)?;
            k += 1;
            if is_unescaped(unit) || extra(unit) {
                out.push_unit(unit);
                continue;
            }
            let cp = if is_lead_surrogate(unit) {
                match text.get(k) {
                    Some(trail) if is_trail_surrogate(trail) => {
                        k += 1;
                        swb_js_text::combine_surrogates(unit, trail)
                    }
                    _ => return Err(malformed()),
                }
            } else if is_trail_surrogate(unit) {
                return Err(malformed());
            } else {
                u32::from(unit)
            };
            let mut utf8 = [0u8; 4];
            // A code point here is a scalar value, as checked above.
            let encoded = char::from_u32(cp)
                .ok_or_else(malformed)?
                .encode_utf8(&mut utf8);
            for byte in encoded.bytes() {
                push_percent(out, byte);
            }
        }
        Ok(k)
    })
}

/// `encodeURI(uri)` (§19.2.6.3): `;/?:@&=+$,#` stay.
fn encode_uri(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    encode(rt, call, |unit| {
        is_reserved(unit) || unit == u16::from(b'#')
    })
}

/// `encodeURIComponent(uriComponent)` (§19.2.6.4).
fn encode_uri_component(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    encode(rt, call, |_| false)
}

// --- decodeURI, decodeURIComponent ---

/// The byte of the escape `%XX` whose `%` is at `k`.
fn escape_byte(text: Str16<'_>, k: usize) -> VmResult<u8> {
    if text.get(k) != Some(u16::from(b'%')) {
        return Err(malformed());
    }
    let high = hex_value(text.get(k + 1)).ok_or_else(malformed)?;
    let low = hex_value(text.get(k + 2)).ok_or_else(malformed)?;
    Ok((high * 16 + low) as u8)
}

/// `Decode(string, preserveEscapeSet)` (§19.2.6.6).
fn decode(
    rt: &mut Runtime,
    call: &NativeCall,
    preserve: fn(u16) -> bool,
) -> VmResult<NativeReturn> {
    let string = rt.string_arg(call, 0)?;
    drive(rt, string, 1, |text, start, end, out| {
        let mut k = start;
        while k < end {
            let unit = text.get(k).ok_or_else(malformed)?;
            if unit != u16::from(b'%') {
                out.push_unit(unit);
                k += 1;
                continue;
            }
            let escape_start = k;
            let first = escape_byte(text, k)?;
            k += 2;
            if first < 0x80 {
                let unit = u16::from(first);
                if preserve(unit) {
                    out.push_text(text.slice(escape_start, k + 1).ok_or_else(malformed)?);
                } else {
                    out.push_unit(unit);
                }
                k += 1;
                continue;
            }
            let count = first.leading_ones() as usize;
            if count == 1 || count > 4 {
                return Err(malformed());
            }
            let mut octets = [first, 0, 0, 0];
            for octet in octets.iter_mut().take(count).skip(1) {
                k += 1;
                let byte = escape_byte(text, k)?;
                if byte & 0xC0 != 0x80 {
                    return Err(malformed());
                }
                k += 2;
                *octet = byte;
            }
            let cp = decode_utf8(&octets[..count]).ok_or_else(malformed)?;
            out.push_code_point(cp);
            k += 1;
        }
        Ok(k)
    })
}

/// The code point of a UTF-8 sequence, or `None` if the sequence is not
/// a valid encoding of a Unicode scalar value (overlong forms, surrogates
/// and values above U+10FFFF are not).
fn decode_utf8(octets: &[u8]) -> Option<u32> {
    std::str::from_utf8(octets)
        .ok()
        .and_then(|text| text.chars().next())
        .map(u32::from)
}

/// `decodeURI(encodedURI)` (§19.2.6.1): the escapes of `;/?:@&=+$,#` stay.
fn decode_uri(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    decode(rt, call, |unit| {
        is_reserved(unit) || unit == u16::from(b'#')
    })
}

/// `decodeURIComponent(encodedURIComponent)` (§19.2.6.2).
fn decode_uri_component(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    decode(rt, call, |_| false)
}

// --- escape, unescape ---

/// `escape(string)` (§B.2.1.1): `%XX` for units below 256, `%uXXXX` for
/// the others.
fn escape(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let string = rt.string_arg(call, 0)?;
    drive(rt, string, 6, |text, start, end, out| {
        for k in start..end {
            let unit = text.get(k).ok_or_else(malformed)?;
            if is_escape_unescaped(unit) {
                out.push_unit(unit);
            } else if unit < 256 {
                push_percent(out, unit as u8);
            } else {
                out.push_unit(u16::from(b'%'));
                out.push_unit(u16::from(b'u'));
                for shift in [12, 8, 4, 0] {
                    out.push_unit(u16::from(HEX[usize::from((unit >> shift) & 15)]));
                }
            }
        }
        Ok(end)
    })
}

/// `unescape(string)` (§B.2.1.2): `%uXXXX` and `%XX`; anything else stays.
fn unescape(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let string = rt.string_arg(call, 0)?;
    drive(rt, string, 1, |text, start, end, out| {
        let length = text.len();
        let mut k = start;
        while k < end {
            let unit = text.get(k).ok_or_else(malformed)?;
            let mut result = unit;
            if unit == u16::from(b'%') {
                if k + 6 <= length
                    && text.get(k + 1) == Some(u16::from(b'u'))
                    && let Some(value) = hex_run(text, k + 2, 4)
                {
                    result = value as u16;
                    k += 5;
                } else if k + 3 <= length
                    && let Some(value) = hex_run(text, k + 1, 2)
                {
                    result = value as u16;
                    k += 2;
                }
            }
            out.push_unit(result);
            k += 1;
        }
        Ok(k)
    })
}

/// The value of `count` hexadecimal digits at `start`.
fn hex_run(text: Str16<'_>, start: usize, count: usize) -> Option<u32> {
    (start..start + count).try_fold(0u32, |value, index| {
        Some(value * 16 + hex_value(text.get(index))?)
    })
}
