//! The base64 and hex methods of `Uint8Array`: `Uint8Array.fromBase64`,
//! `Uint8Array.fromHex`, `Uint8Array.prototype.toBase64`, `toHex`,
//! `setFromBase64` and `setFromHex` (the proposal text at
//! <https://tc39.es/proposal-arraybuffer-base64/spec/>: `FromBase64`,
//! `DecodeBase64Chunk`, `FromHex`, `GetUint8ArrayBytes`,
//! `SetUint8ArrayBytes`), with the options `alphabet`, `lastChunkHandling`
//! and `omitPadding`.
//!
//! Decoding follows `FromBase64` step by step: a state machine over the
//! code units with the pending chunk of up to four sextets; the result is
//! how many units were read, the bytes, and a failure if the input is
//! malformed. The methods that write into an array write the bytes that
//! were decoded before a failure, then throw (as the proposal says and
//! Chromium 148 does). The work runs in blocks, and each block charges the
//! time countdown. Every buffer whose size the input controls is reserved
//! first (`RangeError` at the heap limit; "Invalid string length" for an
//! encoded text above the string limit).
//!
//! V8's messages (measured in Chromium 148): the three `SyntaxError`s of
//! base64 ("Found a character that cannot be part of a valid base64
//! string.", "The base64 input terminates with a single character,
//! excluding padding (=).", "The base64 input terminates with non-zero
//! padding bits."), one for hex ("Input string must contain hex characters
//! in even length"), "input argument must be a string", "invalid option
//! X" and `invalid_argument` for options that are not an object. A
//! receiver that is not a `Uint8Array` is "Method
//! Uint8Array.prototype.toBase64 called on incompatible receiver X" (X is
//! `undefined` for a typed array of another type).

use swb_js_text::String16;

use crate::error::ThrowKind;
use crate::heap::Gc;
use crate::object::{ElementType, Object, TypedArrayData};
use crate::runtime::Runtime;
use crate::string::JsString;
use crate::value::Value;
use crate::vm::convert::MAX_STRING_LENGTH;
use crate::vm::{NativeCall, NativeFn, NativeReturn, VmError, VmResult};

use super::array_buffer::ALLOCATION_FAILED;
use super::string::{INVALID_STRING_LENGTH, string_result};
use super::typed_array_create::out_of_bounds;

/// The steps of the decoder between two charges of the time countdown.
const STEPS_PER_BLOCK: usize = 1 << 14;

// --- Install ---

/// The static methods of `Uint8Array`, in Chromium 148's key order.
const STATICS: [(&str, u32, NativeFn); 2] =
    [("fromBase64", 1, from_base64), ("fromHex", 1, from_hex)];

/// The prototype methods of `Uint8Array.prototype`.
const METHODS: [(&str, u32, NativeFn); 4] = [
    ("toBase64", 0, to_base64),
    ("setFromBase64", 1, set_from_base64),
    ("toHex", 0, to_hex),
    ("setFromHex", 1, set_from_hex),
];

/// Defines the methods on `Uint8Array` and its prototype.
pub(super) fn install(
    rt: &mut Runtime,
    realm: u32,
    constructor: Gc<Object>,
    proto: Gc<Object>,
) -> VmResult<()> {
    for (name, length, func) in STATICS {
        super::method(rt, realm, constructor, name, length, func)?;
    }
    for (name, length, func) in METHODS {
        super::method(rt, realm, proto, name, length, func)?;
    }
    Ok(())
}

// --- Options ---

/// The alphabet of an encoding.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Alphabet {
    Standard,
    Url,
}

/// `lastChunkHandling`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LastChunk {
    Loose,
    Strict,
    StopBeforePartial,
}

/// `GetOptionsObject(options)` and `Get(options, name)`: the value of the
/// option, or `undefined` when there are no options.
fn option(rt: &mut Runtime, options: Option<Gc<Object>>, name: &str) -> VmResult<Value> {
    let Some(options) = options else {
        return Ok(Value::Undefined);
    };
    let key = rt.heap.key_from_str(name)?;
    rt.get_value(options.into(), key)
}

/// `GetOptionsObject(options)` (§ proposal): `undefined` is no options, an
/// object is itself, anything else is a `TypeError`.
fn options_object(value: Value) -> VmResult<Option<Gc<Object>>> {
    match value {
        Value::Undefined => Ok(None),
        Value::Object(object) => Ok(Some(object)),
        _ => Err(VmError::type_error("invalid_argument")),
    }
}

/// The `TypeError` for an option with a value that is not allowed.
fn invalid_option(rt: &Runtime, value: Value) -> VmError {
    VmError::type_error(format!("invalid option {}", rt.primitive_text(value)))
}

/// The value of an option that must be one of `names` (compared as a
/// string, without conversion), as an index; `undefined` is 0.
fn choice(rt: &Runtime, value: Value, names: &[&str]) -> VmResult<usize> {
    if value.is_undefined() {
        return Ok(0);
    }
    if let Value::String(string) = value {
        let text = rt.heap.text(string)?;
        if let Some(index) = names.iter().position(|name| text.eq_str(name)) {
            return Ok(index);
        }
    }
    Err(invalid_option(rt, value))
}

/// The `alphabet` option.
fn alphabet_option(rt: &mut Runtime, options: Option<Gc<Object>>) -> VmResult<Alphabet> {
    let value = option(rt, options, "alphabet")?;
    Ok(match choice(rt, value, &["base64", "base64url"])? {
        0 => Alphabet::Standard,
        _ => Alphabet::Url,
    })
}

/// The `lastChunkHandling` option.
fn last_chunk_option(rt: &mut Runtime, options: Option<Gc<Object>>) -> VmResult<LastChunk> {
    let value = option(rt, options, "lastChunkHandling")?;
    Ok(
        match choice(rt, value, &["loose", "strict", "stop-before-partial"])? {
            0 => LastChunk::Loose,
            1 => LastChunk::Strict,
            _ => LastChunk::StopBeforePartial,
        },
    )
}

/// The options of the decoding methods.
fn decode_options(rt: &mut Runtime, value: Value) -> VmResult<(Alphabet, LastChunk)> {
    let options = options_object(value)?;
    let alphabet = alphabet_option(rt, options)?;
    let last = last_chunk_option(rt, options)?;
    Ok((alphabet, last))
}

// --- The decoder ---

/// Why the input is malformed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Failure {
    BadChar,
    SingleChar,
    ExtraBits,
    BadHex,
}

impl Failure {
    /// The `SyntaxError` with V8's message.
    fn error(self) -> VmError {
        VmError::Raise {
            kind: ThrowKind::SyntaxError,
            message: match self {
                Failure::BadChar => {
                    "Found a character that cannot be part of a valid base64 string."
                }
                Failure::SingleChar => {
                    "The base64 input terminates with a single character, excluding padding (=)."
                }
                Failure::ExtraBits => "The base64 input terminates with non-zero padding bits.",
                Failure::BadHex => "Input string must contain hex characters in even length",
            }
            .into(),
        }
    }
}

/// The outcome of `FromBase64` and `FromHex`: how many code units were
/// read, the bytes, and the failure if there is one.
#[derive(Debug)]
struct Decoded {
    read: usize,
    bytes: Vec<u8>,
    failure: Option<Failure>,
}

/// `FromBase64` (proposal) as a state machine.
struct Decoder {
    alphabet: Alphabet,
    last: LastChunk,
    max: usize,
    index: usize,
    read: usize,
    bytes: Vec<u8>,
    chunk: [u8; 4],
    chunk_len: usize,
}

/// The result of one iteration of the loop of `FromBase64`.
enum Step {
    More,
    Finished(Option<Failure>),
}

/// The sextet of a unit in the standard alphabet.
fn standard_sextet(unit: u16) -> Option<u8> {
    let byte = u8::try_from(unit).ok()?;
    Some(match byte {
        b'A'..=b'Z' => byte - b'A',
        b'a'..=b'z' => byte - b'a' + 26,
        b'0'..=b'9' => byte - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => return None,
    })
}

/// `SkipAsciiWhitespace` (proposal): tab, line feed, form feed, carriage
/// return and space.
fn skip_whitespace(text: swb_js_text::Str16<'_>, mut index: usize) -> usize {
    while matches!(text.get(index), Some(0x09 | 0x0A | 0x0C | 0x0D | 0x20)) {
        index += 1;
    }
    index
}

impl Decoder {
    fn new(alphabet: Alphabet, last: LastChunk, max: usize) -> Self {
        Decoder {
            alphabet,
            last,
            max,
            index: 0,
            read: 0,
            bytes: Vec::new(),
            chunk: [0; 4],
            chunk_len: 0,
        }
    }

    /// The sextet of `unit` in the alphabet of the decoding (step 4.f
    /// and g: the URL alphabet swaps `+` `/` for `-` `_`).
    fn sextet(&self, unit: u16) -> Option<u8> {
        if self.alphabet == Alphabet::Url {
            return match unit {
                0x2B | 0x2F => None,
                0x2D => Some(62),
                0x5F => Some(63),
                _ => standard_sextet(unit),
            };
        }
        standard_sextet(unit)
    }

    /// `DecodeBase64Chunk(chunk, throwOnExtraBits)`: the bytes of the
    /// pending chunk are added; the chunk is empty afterwards.
    fn flush(&mut self, throw_on_extra_bits: bool) -> Result<(), Failure> {
        let [a, b, c, d] = self.chunk;
        let (c, d) = match self.chunk_len {
            2 => (0, 0),
            3 => (c, 0),
            _ => (c, d),
        };
        let first = (a << 2) | (b >> 4);
        let second = ((b & 0x0F) << 4) | (c >> 2);
        let third = ((c & 0x03) << 6) | d;
        let produced: &[u8] = match self.chunk_len {
            2 => {
                if throw_on_extra_bits && second != 0 {
                    return Err(Failure::ExtraBits);
                }
                &[first]
            }
            3 => {
                if throw_on_extra_bits && third != 0 {
                    return Err(Failure::ExtraBits);
                }
                &[first, second]
            }
            _ => &[first, second, third],
        };
        self.bytes.extend_from_slice(produced);
        self.chunk_len = 0;
        Ok(())
    }

    /// One iteration of the loop of `FromBase64` (step 4).
    fn step(&mut self, text: swb_js_text::Str16<'_>) -> Step {
        let length = text.len();
        self.index = skip_whitespace(text, self.index);
        if self.index >= length {
            return self.finish(length);
        }
        let Some(unit) = text.get(self.index) else {
            return Step::Finished(Some(Failure::BadChar));
        };
        self.index += 1;
        if unit == u16::from(b'=') {
            return self.padding(text);
        }
        let Some(sextet) = self.sextet(unit) else {
            return Step::Finished(Some(Failure::BadChar));
        };
        let remaining = self.max - self.bytes.len();
        if (remaining == 1 && self.chunk_len == 2) || (remaining == 2 && self.chunk_len == 3) {
            return Step::Finished(None);
        }
        if let Some(slot) = self.chunk.get_mut(self.chunk_len) {
            *slot = sextet;
        }
        self.chunk_len += 1;
        if self.chunk_len == 4 {
            if let Err(failure) = self.flush(false) {
                return Step::Finished(Some(failure));
            }
            self.read = self.index;
            if self.bytes.len() == self.max {
                return Step::Finished(None);
            }
        }
        Step::More
    }

    /// Step 4.b: the end of the input.
    fn finish(&mut self, length: usize) -> Step {
        if self.chunk_len > 0 {
            match self.last {
                LastChunk::StopBeforePartial => return Step::Finished(None),
                // V8 words a partial chunk in strict mode like a single
                // character.
                LastChunk::Strict => return Step::Finished(Some(Failure::SingleChar)),
                LastChunk::Loose => {
                    if self.chunk_len == 1 {
                        return Step::Finished(Some(Failure::SingleChar));
                    }
                    if let Err(failure) = self.flush(false) {
                        return Step::Finished(Some(failure));
                    }
                }
            }
        }
        self.read = length;
        Step::Finished(None)
    }

    /// Step 4.e: a `=` was read.
    fn padding(&mut self, text: swb_js_text::Str16<'_>) -> Step {
        let length = text.len();
        self.index = skip_whitespace(text, self.index);
        if self.chunk_len < 2 {
            // V8 words a single character before the end of the input as
            // such, and any other padding error as a bad character.
            return Step::Finished(Some(if self.chunk_len == 1 && self.index >= length {
                Failure::SingleChar
            } else {
                Failure::BadChar
            }));
        }
        if self.chunk_len == 2 {
            if self.index >= length {
                return Step::Finished(match self.last {
                    LastChunk::StopBeforePartial => None,
                    // V8 words "AA=" in strict mode as a single character.
                    LastChunk::Strict => Some(Failure::SingleChar),
                    LastChunk::Loose => Some(Failure::BadChar),
                });
            }
            if text.get(self.index) == Some(u16::from(b'=')) {
                self.index = skip_whitespace(text, self.index + 1);
            }
        }
        if self.index < length {
            return Step::Finished(Some(Failure::BadChar));
        }
        if let Err(failure) = self.flush(self.last == LastChunk::Strict) {
            return Step::Finished(Some(failure));
        }
        self.read = length;
        Step::Finished(None)
    }
}

/// Runs `FromBase64` on `string` with at most `max` output bytes, in
/// blocks that charge the time countdown. The output buffer grows by
/// block, each growth reserved against the heap limit, so that its size
/// follows the decoded bytes and not the length of the string.
fn decode_base64(
    rt: &mut Runtime,
    string: Gc<JsString>,
    (alphabet, last): (Alphabet, LastChunk),
    max: usize,
) -> VmResult<Decoded> {
    let length = rt.heap.flat_text(string)?.len();
    let mut decoder = Decoder::new(alphabet, last, max);
    let failure = loop {
        // A step makes at most 3 bytes.
        let rest = (length.saturating_sub(decoder.index) / 4 + 1).saturating_mul(3);
        let extra = rest.min(3 * STEPS_PER_BLOCK).min(max - decoder.bytes.len());
        grow_vec(rt, &mut decoder.bytes, extra)?;
        let text = rt.heap.text(string)?;
        let before = decoder.index;
        let mut finished = None;
        for _ in 0..STEPS_PER_BLOCK {
            if let Step::Finished(failure) = decoder.step(text) {
                finished = Some(failure);
                break;
            }
        }
        let used = decoder.index.saturating_sub(before);
        rt.charge_units(used)?;
        if let Some(failure) = finished {
            break failure;
        }
    };
    Ok(Decoded {
        read: decoder.read,
        bytes: decoder.bytes,
        failure,
    })
}

/// The value of a hex digit.
fn hex_digit(unit: u16) -> Option<u8> {
    let byte = u8::try_from(unit).ok()?;
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Runs `FromHex` (proposal) on `string` with at most `max` output bytes.
fn decode_hex(rt: &mut Runtime, string: Gc<JsString>, max: usize) -> VmResult<Decoded> {
    let length = rt.heap.flat_text(string)?.len();
    let mut decoded = Decoded {
        read: 0,
        bytes: Vec::new(),
        failure: None,
    };
    if length % 2 != 0 {
        decoded.failure = Some(Failure::BadHex);
        return Ok(decoded);
    }
    while decoded.read < length && decoded.bytes.len() < max {
        let room = (length - decoded.read) / 2;
        let extra = room.min(STEPS_PER_BLOCK).min(max - decoded.bytes.len());
        grow_vec(rt, &mut decoded.bytes, extra)?;
        let text = rt.heap.text(string)?;
        let end = (decoded.read + 2 * STEPS_PER_BLOCK).min(length);
        let start = decoded.read;
        while decoded.read < end && decoded.bytes.len() < max {
            let pair = (
                text.get(decoded.read).and_then(hex_digit),
                text.get(decoded.read + 1).and_then(hex_digit),
            );
            let (Some(high), Some(low)) = pair else {
                decoded.failure = Some(Failure::BadHex);
                return Ok(decoded);
            };
            decoded.bytes.push((high << 4) | low);
            decoded.read += 2;
        }
        rt.charge_units(decoded.read - start)?;
    }
    Ok(decoded)
}

/// Reserves `capacity` bytes for `vec` against the heap limit (a
/// `RangeError`) and in the allocator.
fn reserve_vec(rt: &mut Runtime, vec: &mut Vec<u8>, capacity: usize) -> VmResult<()> {
    if !rt.heap.reserve_buffer(capacity, &rt.vm) || vec.try_reserve_exact(capacity).is_err() {
        return Err(VmError::range_error(ALLOCATION_FAILED));
    }
    Ok(())
}

/// Makes room for `extra` more bytes in `vec`. A growth doubles the size
/// when the limit allows, and is reserved against the heap limit (a
/// `RangeError`) and in the allocator before it happens.
fn grow_vec(rt: &mut Runtime, vec: &mut Vec<u8>, extra: usize) -> VmResult<()> {
    if vec.capacity() - vec.len() >= extra {
        return Ok(());
    }
    let wanted = vec.len().saturating_add(extra);
    let doubled = wanted.max(vec.capacity().saturating_mul(2));
    // Near the limit, take what is needed and no more.
    let total = if rt.heap.reserve_buffer(doubled, &rt.vm) {
        doubled
    } else {
        wanted
    };
    if (total == wanted && !rt.heap.reserve_buffer(wanted, &rt.vm))
        || vec.try_reserve_exact(total - vec.len()).is_err()
    {
        return Err(VmError::range_error(ALLOCATION_FAILED));
    }
    Ok(())
}

// --- The arguments and the receiver ---

/// The string argument (V8: "input argument must be a string").
fn string_arg(rt: &Runtime, call: &NativeCall) -> VmResult<Gc<JsString>> {
    match rt.arg(call, 0) {
        Value::String(string) => Ok(string),
        _ => Err(VmError::type_error("input argument must be a string")),
    }
}

/// `ValidateUint8Array(this)`: the object and its slots.
fn uint8_this(
    rt: &Runtime,
    call: &NativeCall,
    name: &str,
) -> VmResult<(Gc<Object>, TypedArrayData)> {
    match rt.typed_slots(call.this())? {
        Some((object, data)) if data.ty == ElementType::Uint8 => Ok((object, data)),
        // V8 shows no receiver for a typed array of another type.
        Some(_) => {
            Err(rt.incompatible_receiver(&format!("Uint8Array.prototype.{name}"), Value::Undefined))
        }
        None => Err(rt.incompatible_receiver(&format!("Uint8Array.prototype.{name}"), call.this())),
    }
}

/// `GetUint8ArrayBytes`' check: the length of the array, or the
/// `TypeError` of a detached or out-of-bounds array.
fn length_of(rt: &Runtime, data: &TypedArrayData, name: &str) -> VmResult<usize> {
    rt.heap
        .typed_length(data)?
        .ok_or_else(|| out_of_bounds(&format!("Uint8Array.prototype.{name}")))
}

/// A new `Uint8Array` of the current realm holding `bytes`.
fn new_uint8_array(rt: &mut Runtime, bytes: &[u8]) -> VmResult<NativeReturn> {
    let realm = rt.current_realm();
    let (_, proto) = rt.typed_array_intrinsics(realm, ElementType::Uint8)?;
    let (array, data) =
        rt.allocate_typed_array(realm, proto, ElementType::Uint8, bytes.len() as u64)?;
    rt.heap.typed_put_bytes(&data, 0, bytes)?;
    rt.charge_bytes(bytes.len())?;
    Ok(NativeReturn::Value(array.into()))
}

// --- fromBase64 and fromHex ---

/// `Uint8Array.fromBase64(string [, options])`.
fn from_base64(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let string = string_arg(rt, call)?;
    let options = decode_options(rt, rt.arg(call, 1))?;
    let decoded = decode_base64(rt, string, options, usize::MAX)?;
    if let Some(failure) = decoded.failure {
        return Err(failure.error());
    }
    new_uint8_array(rt, &decoded.bytes)
}

/// `Uint8Array.fromHex(string)`.
fn from_hex(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let string = string_arg(rt, call)?;
    let decoded = decode_hex(rt, string, usize::MAX)?;
    if let Some(failure) = decoded.failure {
        return Err(failure.error());
    }
    new_uint8_array(rt, &decoded.bytes)
}

// --- setFromBase64 and setFromHex ---

/// `{ read, written }` as a new object.
fn read_written(rt: &mut Runtime, read: usize, written: usize) -> VmResult<NativeReturn> {
    let proto = rt.intrinsic(rt.current_realm(), crate::vm::Intrinsic::ObjectPrototype)?;
    let result = rt.heap.new_object(Some(proto))?;
    rt.heap.record(result);
    for (name, value) in [("read", read), ("written", written)] {
        let key = rt.heap.key_from_str(name)?;
        rt.create_data_property_or_throw(result, key, Value::number(value as f64))?;
    }
    Ok(NativeReturn::Value(result.into()))
}

/// The common part of `setFromBase64` and `setFromHex`: writes the bytes
/// that were decoded (also before a failure), then throws the failure.
fn write_decoded(
    rt: &mut Runtime,
    data: &TypedArrayData,
    decoded: &Decoded,
) -> VmResult<NativeReturn> {
    let written = rt.heap.typed_put_bytes(data, 0, &decoded.bytes)?;
    rt.charge_bytes(written)?;
    if let Some(failure) = decoded.failure {
        return Err(failure.error());
    }
    read_written(rt, decoded.read, written)
}

/// `Uint8Array.prototype.setFromBase64(string [, options])`.
fn set_from_base64(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    const NAME: &str = "setFromBase64";
    let (_, data) = uint8_this(rt, call, NAME)?;
    let string = string_arg(rt, call)?;
    let options = decode_options(rt, rt.arg(call, 1))?;
    let length = length_of(rt, &data, NAME)?;
    let decoded = if length == 0 {
        Decoded {
            read: 0,
            bytes: Vec::new(),
            failure: None,
        }
    } else {
        decode_base64(rt, string, options, length)?
    };
    write_decoded(rt, &data, &decoded)
}

/// `Uint8Array.prototype.setFromHex(string)`.
fn set_from_hex(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    const NAME: &str = "setFromHex";
    let (_, data) = uint8_this(rt, call, NAME)?;
    let string = string_arg(rt, call)?;
    let length = length_of(rt, &data, NAME)?;
    let decoded = decode_hex(rt, string, length)?;
    write_decoded(rt, &data, &decoded)
}

// --- toBase64 and toHex ---

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";
const BASE64_STANDARD: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const BASE64_URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// The bytes per block of an encoding (a multiple of 3).
const ENCODE_BLOCK: usize = 3 * 4096;

/// Reserves the output text of `units` code units (V8's `RangeError` above
/// the string limit, the heap limit) and gives an empty vector for it.
fn output_text(rt: &mut Runtime, units: usize) -> VmResult<Vec<u8>> {
    if units > MAX_STRING_LENGTH {
        return Err(VmError::range_error(INVALID_STRING_LENGTH));
    }
    let mut out = Vec::new();
    reserve_vec(rt, &mut out, units)?;
    Ok(out)
}

/// A string of Latin-1 text built by an encoding.
fn latin1_string(rt: &mut Runtime, bytes: Vec<u8>) -> VmResult<NativeReturn> {
    let string = rt.new_string(String16::from_latin1(bytes))?;
    string_result(string)
}

/// Encodes the bytes of the array in blocks.
fn encode_array(
    rt: &mut Runtime,
    data: &TypedArrayData,
    out: &mut Vec<u8>,
    mut block: impl FnMut(&[u8], &mut Vec<u8>),
    step: usize,
) -> VmResult<()> {
    let mut at = 0;
    while let Some(window) = rt.heap.typed_window(data)? {
        let Some(rest) = window.get(at..).filter(|rest| !rest.is_empty()) else {
            break;
        };
        let part = rest.get(..step.min(rest.len())).unwrap_or_default();
        block(part, out);
        at += part.len();
        rt.charge_bytes(part.len() * 2)?;
    }
    Ok(())
}

/// Encodes whole groups of three bytes (and the last group) as base64.
fn encode_base64_block(
    alphabet: &[u8; 64],
    omit_padding: bool,
    last: bool,
    bytes: &[u8],
    out: &mut Vec<u8>,
) {
    let table = |index: u32| alphabet.get((index & 63) as usize).copied().unwrap_or(b'A');
    let (groups, remainder) = bytes.as_chunks::<3>();
    for group in groups {
        let word = u32::from(group[0]) << 16 | u32::from(group[1]) << 8 | u32::from(group[2]);
        out.extend_from_slice(&[
            table(word >> 18),
            table(word >> 12),
            table(word >> 6),
            table(word),
        ]);
    }
    if !last {
        return;
    }
    match *remainder {
        [a] => {
            let word = u32::from(a) << 16;
            out.extend_from_slice(&[table(word >> 18), table(word >> 12)]);
            if !omit_padding {
                out.extend_from_slice(b"==");
            }
        }
        [a, b] => {
            let word = u32::from(a) << 16 | u32::from(b) << 8;
            out.extend_from_slice(&[table(word >> 18), table(word >> 12), table(word >> 6)]);
            if !omit_padding {
                out.push(b'=');
            }
        }
        _ => {}
    }
}

/// `Uint8Array.prototype.toBase64([options])`.
fn to_base64(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    const NAME: &str = "toBase64";
    let (_, data) = uint8_this(rt, call, NAME)?;
    let options = options_object(rt.arg(call, 0))?;
    let alphabet = alphabet_option(rt, options)?;
    let omit_value = option(rt, options, "omitPadding")?;
    let omit_padding = rt.to_boolean(omit_value)?;
    let length = length_of(rt, &data, NAME)?;
    let units = if omit_padding {
        length.saturating_mul(4).div_ceil(3)
    } else {
        length.div_ceil(3).saturating_mul(4)
    };
    let mut out = output_text(rt, units)?;
    let table = if alphabet == Alphabet::Url {
        BASE64_URL
    } else {
        BASE64_STANDARD
    };
    let mut done = 0;
    encode_array(
        rt,
        &data,
        &mut out,
        |part, out| {
            done += part.len();
            encode_base64_block(table, omit_padding, done >= length, part, out);
        },
        ENCODE_BLOCK,
    )?;
    latin1_string(rt, out)
}

/// `Uint8Array.prototype.toHex()`.
fn to_hex(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    const NAME: &str = "toHex";
    let (_, data) = uint8_this(rt, call, NAME)?;
    let length = length_of(rt, &data, NAME)?;
    let mut out = output_text(rt, length.saturating_mul(2))?;
    encode_array(
        rt,
        &data,
        &mut out,
        |part, out| {
            for &byte in part {
                out.push(
                    HEX_DIGITS
                        .get(usize::from(byte >> 4))
                        .copied()
                        .unwrap_or(b'0'),
                );
                out.push(
                    HEX_DIGITS
                        .get(usize::from(byte & 15))
                        .copied()
                        .unwrap_or(b'0'),
                );
            }
        },
        ENCODE_BLOCK,
    )?;
    latin1_string(rt, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `FromBase64` on `text` with at most `max` bytes: the bytes, the
    /// units read and the failure.
    fn decode(
        text: &str,
        alphabet: Alphabet,
        last: LastChunk,
        max: usize,
    ) -> (Vec<u8>, usize, Option<Failure>) {
        let units = String16::from(text);
        let mut decoder = Decoder::new(alphabet, last, max);
        let failure = if max == 0 {
            None
        } else {
            loop {
                if let Step::Finished(failure) = decoder.step(units.as_str16()) {
                    break failure;
                }
            }
        };
        (decoder.bytes, decoder.read, failure)
    }

    fn loose(text: &str) -> (Vec<u8>, usize, Option<Failure>) {
        decode(text, Alphabet::Standard, LastChunk::Loose, usize::MAX)
    }

    #[test]
    fn decodes_with_and_without_padding() {
        assert_eq!(loose("SGVsbG8=").0, b"Hello");
        assert_eq!(loose("SGVsbG8").0, b"Hello");
        assert_eq!(loose(" S G\nV\tsb\u{c}G\r8 = ").0, b"Hello");
        assert_eq!(loose("").0, b"");
        assert_eq!(loose("AA==").2, None);
        assert_eq!(loose("SGVsbG8=").1, 8);
    }

    #[test]
    fn rejects_bad_input_with_the_message_classes_of_v8() {
        assert_eq!(loose("=").2, Some(Failure::BadChar));
        assert_eq!(loose("A=").2, Some(Failure::SingleChar));
        assert_eq!(loose("A===").2, Some(Failure::BadChar));
        assert_eq!(loose("S").2, Some(Failure::SingleChar));
        assert_eq!(loose("AA=").2, Some(Failure::BadChar));
        assert_eq!(loose("AA==AA").2, Some(Failure::BadChar));
        assert_eq!(loose("AAAA=").2, Some(Failure::BadChar));
        assert_eq!(loose("AA\u{a0}AA").2, Some(Failure::BadChar));
        assert_eq!(loose("-_").2, Some(Failure::BadChar));
        let strict = |text: &str| decode(text, Alphabet::Standard, LastChunk::Strict, usize::MAX);
        assert_eq!(strict("SGU").2, Some(Failure::SingleChar));
        assert_eq!(strict("AA=").2, Some(Failure::SingleChar));
        assert_eq!(strict("SGV=").2, Some(Failure::ExtraBits));
        assert_eq!(strict("SH==").2, Some(Failure::ExtraBits));
        assert_eq!(strict("SGU=").0, b"He");
        assert_eq!(loose("SGV=").0, b"He");
    }

    #[test]
    fn stops_before_a_partial_chunk() {
        let stop = |text: &str| {
            decode(
                text,
                Alphabet::Standard,
                LastChunk::StopBeforePartial,
                usize::MAX,
            )
        };
        assert_eq!(stop("SGVsbGU"), (b"Hel".to_vec(), 4, None));
        assert_eq!(stop("SGVsbGU=").0, b"Helle");
        assert_eq!(stop("AA=").0, b"");
    }

    #[test]
    fn the_url_alphabet_swaps_two_characters() {
        let url = |text: &str| decode(text, Alphabet::Url, LastChunk::Loose, usize::MAX);
        assert_eq!(url("-_-_").0, [251, 255, 191]);
        assert_eq!(url("+/+/").2, Some(Failure::BadChar));
    }

    /// The output limit: a chunk that does not fit is not decoded, and
    /// the units read count only the whole chunks (the specification
    /// returns when the output is full, without reading on).
    #[test]
    fn stops_when_the_output_is_full() {
        let limited =
            |text: &str, max: usize| decode(text, Alphabet::Standard, LastChunk::Loose, max);
        assert_eq!(limited("SGVsbG8=", 3), (b"Hel".to_vec(), 4, None));
        assert_eq!(limited("SGVsbG8=", 2), (Vec::new(), 0, None));
        assert_eq!(limited("SGVsbG8=", 4), (b"Hel".to_vec(), 4, None));
        assert_eq!(limited("SGVsbG8=", 5), (b"Hello".to_vec(), 8, None));
        assert_eq!(limited("AAAA$", 3), (vec![0, 0, 0], 4, None));
        assert_eq!(limited("$", 0), (Vec::new(), 0, None));
    }

    #[test]
    fn hex_digits() {
        assert_eq!(hex_digit(u16::from(b'f')), Some(15));
        assert_eq!(hex_digit(u16::from(b'F')), Some(15));
        assert_eq!(hex_digit(u16::from(b'g')), None);
        assert_eq!(hex_digit(0x0660), None);
        assert_eq!(hex_digit(0xFF10), None);
    }

    #[test]
    fn encodes_groups_and_the_padding() {
        let encode = |bytes: &[u8], table: &[u8; 64], omit: bool| {
            let mut out = Vec::new();
            encode_base64_block(table, omit, true, bytes, &mut out);
            String::from_utf8(out).expect("ASCII")
        };
        assert_eq!(encode(b"Hello", BASE64_STANDARD, false), "SGVsbG8=");
        assert_eq!(encode(b"He", BASE64_STANDARD, true), "SGU");
        assert_eq!(encode(b"H", BASE64_STANDARD, false), "SA==");
        assert_eq!(encode(&[251, 255, 254], BASE64_STANDARD, false), "+//+");
        assert_eq!(encode(&[251, 255, 254], BASE64_URL, false), "-__-");
        assert_eq!(encode(b"", BASE64_URL, false), "");
    }
}
