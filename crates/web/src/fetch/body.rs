//! The body of a `Request` or a `Response` (Fetch §5.1 "Body mixin",
//! <https://fetch.spec.whatwg.org/#body-mixin>).
//!
//! swb has no streams (ADR 0028 section 12): a body is the bytes in
//! memory, as the body source (a string, `URLSearchParams`, an
//! `ArrayBuffer` or a view) gives them, or as the network delivered them.
//! `body` is `null`. Reading a body takes the bytes out and marks the body
//! used; a second read rejects with a `TypeError`. The bytes count against
//! the heap of the document (`GenericData::heap_size` of the owner).
//!
//! # `Blob` and `FormData` (feature 10b)
//!
//! [`extract`] is the only place that tells the kinds of body sources
//! apart: a `Blob` (its bytes and its type) and a `FormData` (a
//! `multipart/form-data` body with a boundary) have their branches there.
//! [`read`] makes `blob()` (type from the `Content-Type` header) and
//! `formData()` (parses `multipart/form-data` and
//! `application/x-www-form-urlencoded`; another type rejects with a
//! `TypeError`, as Chromium does).

use swb_js::{Gc, GenericData, Object, Runtime, Value};

use super::generic_mut;
use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::generated as g;

/// The bytes of a body that is not null.
#[derive(Debug)]
pub(crate) struct BodyState {
    /// The bytes; empty after a read took them.
    pub(crate) bytes: Vec<u8>,
    /// True after a read ([`read`]).
    pub(crate) used: bool,
}

impl BodyState {
    pub(crate) fn new(bytes: Vec<u8>) -> BodyState {
        BodyState { bytes, used: false }
    }

    /// The bytes that a body of `body` counts against the heap.
    pub(crate) fn heap_size(body: Option<&BodyState>) -> usize {
        body.map_or(0, |b| b.bytes.capacity())
    }
}

/// The data of a platform object that has a body.
pub(crate) trait HasBody: GenericData {
    fn body_mut(&mut self) -> &mut Option<BodyState>;

    /// The signal of the `fetch()` that made the object: after it aborts,
    /// the body cannot be read (Chromium's stream errors).
    fn abort_signal(&self) -> Option<Gc<Object>> {
        None
    }

    /// The `Headers` object of the owner (for the type of `blob()` and
    /// `formData()`).
    fn headers_object(&self) -> Gc<Object>;
}

/// What `BodyInit` extracted: the bytes and the `Content-Type` that the
/// source implies.
pub(crate) struct Extracted {
    pub(crate) bytes: Vec<u8>,
    pub(crate) content_type: Option<String>,
}

/// Fetch "extract a body" for a `BodyInit` value that is not `null`.
///
/// <https://fetch.spec.whatwg.org/#concept-bodyinit-extract>
pub(crate) fn extract(rt: &mut Runtime, value: Value) -> b::Result<Extracted> {
    if let Value::Object(object) = value {
        if rt.implements(value, g::URL_SEARCH_PARAMS)
            && let Some(query) = crate::url_api::query_of(rt, object)
        {
            return Ok(Extracted {
                bytes: query.into_bytes(),
                content_type: Some("application/x-www-form-urlencoded;charset=UTF-8".to_owned()),
            });
        }
        if let Some(blob) = crate::file_api::blob_of(rt, value) {
            let bytes = blob.bytes.to_vec();
            let content_type = (!blob.kind.is_empty()).then(|| blob.kind.clone());
            return Ok(Extracted {
                bytes,
                content_type,
            });
        }
        if crate::file_api::is_form_data(rt, value) {
            let (bytes, content_type) = crate::file_api::multipart_body(rt, object)?;
            return Ok(Extracted {
                bytes,
                content_type: Some(content_type),
            });
        }
        if let Some(bytes) = crate::file_api::buffer_bytes(rt, object)? {
            return Ok(Extracted {
                bytes,
                content_type: None,
            });
        }
    }
    let text = b::convert::usv_string(rt, value)?;
    Ok(Extracted {
        bytes: text.into_bytes(),
        content_type: Some("text/plain;charset=UTF-8".to_owned()),
    })
}

/// What a read of the body returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Read {
    ArrayBuffer,
    Blob,
    Bytes,
    FormData,
    Json,
    Text,
}

/// UTF-8 decode of a body: the BOM goes, invalid bytes become U+FFFD.
pub(crate) fn utf8_text(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

/// A read of the body of `this` (Fetch "consume body"): the promise that
/// `arrayBuffer()`, `text()` and the others return. A body that was used
/// is a rejected promise.
pub(crate) fn read<T: HasBody>(
    cx: &mut Cx<'_>,
    this: Platform,
    what: Read,
) -> b::Result<Gc<Object>> {
    // An aborted `fetch()` errors the body stream, whatever its reason
    // (measured: AbortError "The user aborted a request."); the body stays
    // unread.
    let (signal, headers) = {
        let data = super::generic::<T>(cx.rt, this)?;
        (data.abort_signal(), data.headers_object())
    };
    if let Some(signal) = signal
        && generic_mut::<T>(cx.rt, this)?.body_mut().is_some()
        && crate::abort::is_aborted(cx.rt, signal)?
    {
        // A rejected promise, not an exception: the stream's error has no
        // "Failed to execute" context.
        let realm = cx.realm();
        let error = b::dom_exception::create(
            cx.rt,
            realm,
            g::DOM_EXCEPTION,
            "AbortError",
            "The user aborted a request.",
        )?;
        return match cx.rt.rejected_promise(realm, error.into())? {
            Value::Object(promise) => Ok(promise),
            _ => Err(Exception::type_error("a rejected promise is not an object")),
        };
    }
    let bytes = match generic_mut::<T>(cx.rt, this)?.body_mut() {
        None => Vec::new(),
        Some(body) if body.used => {
            return Err(Exception::type_error("body stream already read"));
        }
        Some(body) => {
            body.used = true;
            std::mem::take(&mut body.bytes)
        }
    };
    let realm = cx.realm();
    let value = match what {
        Read::Text => b::to_js::string(cx.rt, &utf8_text(&bytes))?,
        Read::Json => parse_json(cx.rt, &utf8_text(&bytes))?,
        Read::ArrayBuffer => cx.rt.create_array_buffer(realm, bytes, None)?.into(),
        Read::Bytes => {
            let length = bytes.len();
            let buffer = cx.rt.create_array_buffer(realm, bytes, None)?;
            cx.rt
                .create_view(realm, Some("Uint8"), buffer, 0, Some(length))?
                .into()
        }
        Read::Blob => {
            let kind =
                super::headers::value_of(cx.rt, headers, "content-type")?.unwrap_or_default();
            crate::file_api::make_blob(cx.rt, realm, bytes, &kind)?.into()
        }
        Read::FormData => {
            let kind =
                super::headers::value_of(cx.rt, headers, "content-type")?.unwrap_or_default();
            let Some(form_data) = parse_form_data(cx.rt, realm, &bytes, &kind)? else {
                let error = cx
                    .rt
                    .new_error(swb_js::ThrowKind::TypeError, "Failed to fetch")?;
                return match cx.rt.rejected_promise(realm, error.into())? {
                    Value::Object(promise) => Ok(promise),
                    _ => Err(Exception::type_error("a rejected promise is not an object")),
                };
            };
            form_data.into()
        }
    };
    match cx.rt.resolved_promise(realm, value)? {
        Value::Object(promise) => Ok(promise),
        _ => Err(Exception::type_error("PromiseResolve gave a non-object")),
    }
}

/// `JSON.parse(text)`, or `null` when the text is not JSON (the `json`
/// response of an `XMLHttpRequest`).
pub(crate) fn parse_json_or_null(rt: &mut Runtime, text: &str) -> Value {
    parse_json(rt, text).unwrap_or(Value::Null)
}

/// `JSON.parse(text)`; a `SyntaxError` is the exception of the read.
fn parse_json(rt: &mut Runtime, text: &str) -> b::Result<Value> {
    let (json, parse) = json_function(rt, "parse")?;
    let text = b::to_js::string(rt, text)?;
    match rt.call_function(parse, json, &[text]) {
        Ok(value) => Ok(value),
        Err(swb_js::ScriptError::Uncaught { .. }) => {
            Err(Exception::Js(swb_js::VmError::Throw(rt.last_value())))
        }
        Err(error) => Err(Exception::type_error(error.to_string())),
    }
}

/// `JSON.stringify(value)` as a string; `None` when it gives `undefined`.
pub(crate) fn stringify_json(rt: &mut Runtime, value: Value) -> b::Result<Option<String>> {
    let (json, stringify) = json_function(rt, "stringify")?;
    match rt.call_function(stringify, json, &[value]) {
        Ok(Value::Undefined) => Ok(None),
        Ok(text) => Ok(Some(b::convert::usv_string(rt, text)?)),
        // A `TypeError` of the serializer gets the context of the call
        // (Chromium: "Failed to execute 'json' on 'Response': ...").
        Err(swb_js::ScriptError::Uncaught { name, message, .. }) if name == "TypeError" => {
            Err(Exception::type_error(message))
        }
        Err(swb_js::ScriptError::Uncaught { .. }) => {
            Err(Exception::Js(swb_js::VmError::Throw(rt.last_value())))
        }
        Err(error) => Err(Exception::type_error(error.to_string())),
    }
}

/// The `JSON` object of the global object and its function `name`.
/// (Deviation: a page that replaced `JSON.parse` changes `json()`.)
fn json_function(rt: &mut Runtime, name: &str) -> b::Result<(Value, Value)> {
    let global = rt.global(0)?;
    let key = rt.key_from_str("JSON")?;
    let json = rt.get_key(global.into(), key)?;
    let key = rt.key_from_str(name)?;
    let function = rt.get_key(json, key)?;
    Ok((json, function))
}

/// The `FormData` that a body of type `kind` stands for: a
/// `multipart/form-data` body (HTML "multipart/form-data decoding": the
/// parts with their `Content-Disposition`) or an
/// `application/x-www-form-urlencoded` body. `None` for another type or a
/// malformed multipart body.
fn parse_form_data(
    rt: &mut Runtime,
    realm: u32,
    bytes: &[u8],
    kind: &str,
) -> b::Result<Option<Gc<Object>>> {
    let lower = kind.to_ascii_lowercase();
    let entries = if lower.starts_with("application/x-www-form-urlencoded") {
        crate::file_api::urlencoded_entries(&utf8_text(bytes))
    } else if lower.starts_with("multipart/form-data") {
        let Some(boundary) = boundary_of(kind) else {
            return Ok(None);
        };
        match crate::file_api::parse_multipart(rt, realm, bytes, &boundary)? {
            Some(entries) => entries,
            None => return Ok(None),
        }
    } else {
        return Ok(None);
    };
    Ok(Some(crate::file_api::new_form_data(rt, realm, entries)?))
}

/// The `boundary` parameter of a `multipart/form-data` type.
fn boundary_of(kind: &str) -> Option<String> {
    let lower = kind.to_ascii_lowercase();
    let at = lower.find("boundary=")?;
    let rest = kind.get(at + "boundary=".len()..)?;
    let value = if let Some(quoted) = rest.strip_prefix('"') {
        quoted.split('"').next()?
    } else {
        rest.split(';').next()?.trim()
    };
    (!value.is_empty()).then(|| value.to_owned())
}
