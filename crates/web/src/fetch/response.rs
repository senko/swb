//! `Response` (Fetch §5.6, <https://fetch.spec.whatwg.org/#response-class>):
//! the constructor, the static methods `error()`, `redirect()` and
//! `json()`, the attributes, `clone()`, and the response of a `fetch()`
//! ([`from_net`]).
//!
//! The headers of a response from the network are immutable and hold what
//! the filtered response of `net` let through (`swb_net::filter_response`).
//! An opaque response has status 0, no headers and no body.

use swb_js::{Gc, GenericData, Object, Runtime, Tracer, Value};

use super::body::{self, BodyState, HasBody, Read};
use super::generic;
use super::headers::{self, Guard};
use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::generated as g;
use crate::generated::types::{ResponseInit, ResponseType};
use crate::state::WebState;

/// The data of a `Response` object.
#[derive(Debug)]
pub(crate) struct ResponseData {
    kind: ResponseType,
    url: String,
    redirected: bool,
    status: u16,
    status_text: String,
    headers: Gc<Object>,
    body: Option<BodyState>,
    /// The signal of the `fetch()` that made the response.
    signal: Option<Gc<Object>>,
}

impl GenericData for ResponseData {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.object(self.headers);
        if let Some(signal) = self.signal {
            tracer.object(signal);
        }
    }

    fn heap_size(&self) -> usize {
        128 + self.url.capacity()
            + self.status_text.capacity()
            + BodyState::heap_size(self.body.as_ref())
    }
}

impl HasBody for ResponseData {
    fn body_mut(&mut self) -> &mut Option<BodyState> {
        &mut self.body
    }

    fn abort_signal(&self) -> Option<Gc<Object>> {
        self.signal
    }

    fn headers_object(&self) -> Gc<Object> {
        self.headers
    }
}

/// The reason phrase that HTTP/1.1 servers send for the common status
/// codes (Chromium's `statusText` of a response without a phrase of its
/// own, and of a replayed one).
pub(crate) fn reason_phrase(status: u16) -> &'static str {
    match status {
        100 => "Continue",
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        203 => "Non-Authoritative Information",
        204 => "No Content",
        205 => "Reset Content",
        206 => "Partial Content",
        300 => "Multiple Choices",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        402 => "Payment Required",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        407 => "Proxy Authentication Required",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        412 => "Precondition Failed",
        413 => "Payload Too Large",
        414 => "URI Too Long",
        415 => "Unsupported Media Type",
        416 => "Range Not Satisfiable",
        417 => "Expectation Failed",
        418 => "I'm a teapot",
        421 => "Misdirected Request",
        422 => "Unprocessable Entity",
        425 => "Too Early",
        426 => "Upgrade Required",
        428 => "Precondition Required",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        451 => "Unavailable For Legal Reasons",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        505 => "HTTP Version Not Supported",
        _ => "",
    }
}

/// A null body status: no body is allowed.
///
/// <https://fetch.spec.whatwg.org/#null-body-status>
fn is_null_body_status(status: u16) -> bool {
    matches!(status, 101 | 204 | 205 | 304)
}

/// The response of a `fetch()`: the final response of the network with its
/// kind. `request_url` is the URL of an opaque-redirect response; `head`
/// is true for a `HEAD` request (no body).
pub(crate) fn from_net(
    rt: &mut Runtime,
    realm: u32,
    response: swb_net::Response,
    kind: swb_net::ResponseKind,
    request_url: &str,
    head: bool,
    signal: Gc<Object>,
) -> b::Result<Gc<Object>> {
    let (kind_js, url) = match kind {
        swb_net::ResponseKind::Basic => (ResponseType::Basic, response.url.clone()),
        swb_net::ResponseKind::Cors => (ResponseType::Cors, response.url.clone()),
        swb_net::ResponseKind::Opaque => (ResponseType::Opaque, response.url.clone()),
        swb_net::ResponseKind::OpaqueRedirect => (
            ResponseType::Opaqueredirect,
            swb_net::Url::parse(request_url).unwrap_or_else(|_| response.url.clone()),
        ),
    };
    let opaque = matches!(
        kind,
        swb_net::ResponseKind::Opaque | swb_net::ResponseKind::OpaqueRedirect
    );
    let mut url = url;
    url.set_fragment(None);
    let list = response
        .headers
        .iter()
        .map(|(n, v)| (n.to_owned(), headers::normalize_value(v).to_owned()))
        .collect();
    let headers = headers::new_object(rt, realm, Guard::Immutable, list)?;
    let has_body = !opaque && !head && !is_null_body_status(response.status);
    let data = ResponseData {
        kind: kind_js,
        url: if matches!(kind, swb_net::ResponseKind::Opaque) {
            String::new()
        } else {
            url.to_string()
        },
        redirected: response.redirected,
        status: response.status,
        status_text: if opaque {
            String::new()
        } else {
            reason_phrase(response.status).to_owned()
        },
        headers,
        body: has_body.then(|| BodyState::new(response.body)),
        signal: Some(signal),
    };
    let handle = rt.alloc_host_data(data)?;
    Ok(rt.new_host_object(realm, g::RESPONSE, handle)?)
}

fn data(rt: &Runtime, this: Platform) -> b::Result<&ResponseData> {
    generic::<ResponseData>(rt, this)
}

/// A new response (Fetch "initialize a response"): the status, the
/// status text, the headers (guard `response`) and the body of `init`.
fn initialize(
    rt: &mut Runtime,
    realm: u32,
    body: Option<Value>,
    init: ResponseInit,
    default_type: Option<&'static str>,
) -> b::Result<ResponseData> {
    if !(200..=599).contains(&init.status) {
        return Err(Exception::range_error(format!(
            "The status provided ({}) is outside the range [200, 599].",
            init.status
        )));
    }
    let valid_text = init
        .status_text
        .chars()
        .all(|c| matches!(c, '\t' | ' '..='~' | '\u{80}'..='\u{ff}'));
    if !valid_text {
        return Err(Exception::type_error("Invalid statusText"));
    }
    let headers = headers::new_object(rt, realm, Guard::Response, Vec::new())?;
    if let Some(list) = init.headers {
        headers::fill(rt, headers, list).map_err(|e| e.member("headers", "ResponseInit"))?;
    }
    let mut bytes = None;
    if let Some(value) = body.filter(|v| !matches!(v, Value::Null | Value::Undefined)) {
        if is_null_body_status(init.status) {
            return Err(Exception::type_error(
                "Response with null body status cannot have body",
            ));
        }
        let extracted = body::extract(rt, value)?;
        let content_type = default_type.map(str::to_owned).or(extracted.content_type);
        if let Some(content_type) = content_type
            && headers::value_of(rt, headers, "content-type")?.is_none()
        {
            headers::append_to(rt, headers, "Content-Type", &content_type)?;
        }
        bytes = Some(extracted.bytes);
    }
    Ok(ResponseData {
        kind: ResponseType::Default,
        url: String::new(),
        redirected: false,
        status: init.status,
        status_text: init.status_text,
        headers,
        body: bytes.map(BodyState::new),
        signal: None,
    })
}

fn wrap(cx: &mut Cx<'_>, data: ResponseData) -> b::Result<Gc<Object>> {
    let realm = cx.realm();
    let handle = cx.rt.alloc_host_data(data)?;
    Ok(cx.rt.new_host_object(realm, g::RESPONSE, handle)?)
}

/// An immutable response with no body and the type `kind`.
fn simple(
    cx: &mut Cx<'_>,
    kind: ResponseType,
    status: u16,
    list: Vec<(String, String)>,
) -> b::Result<Gc<Object>> {
    let realm = cx.realm();
    let headers = headers::new_object(cx.rt, realm, Guard::Immutable, list)?;
    wrap(
        cx,
        ResponseData {
            kind,
            url: String::new(),
            redirected: false,
            status,
            status_text: String::new(),
            headers,
            body: None,
            signal: None,
        },
    )
}

impl g::response::ResponseImpl for WebState {
    fn constructor(cx: &mut Cx<'_>, body: Value, init: ResponseInit) -> b::Result<Gc<Object>> {
        let realm = cx.realm();
        let data = initialize(cx.rt, realm, Some(body), init, None)?;
        let handle = cx.rt.alloc_host_data(data)?;
        Ok(cx.new_instance::<WebState>(handle)?)
    }

    fn error(cx: &mut Cx<'_>) -> b::Result<Gc<Object>> {
        simple(cx, ResponseType::Error, 0, Vec::new())
    }

    fn redirect(cx: &mut Cx<'_>, url: String, status: u16) -> b::Result<Gc<Object>> {
        let parsed = super::engine_parse_url(cx.rt, &url)?
            .ok_or_else(|| Exception::type_error(format!("Failed to parse URL from {url}")))?;
        if !matches!(status, 301 | 302 | 303 | 307 | 308) {
            return Err(Exception::range_error("Invalid status code"));
        }
        let object = simple(
            cx,
            ResponseType::Default,
            status,
            vec![("location".to_owned(), parsed)],
        )?;
        Ok(object)
    }

    fn static_json(cx: &mut Cx<'_>, data: Value, init: ResponseInit) -> b::Result<Gc<Object>> {
        let text = body::stringify_json(cx.rt, data)?
            .ok_or_else(|| Exception::type_error("The data is not JSON serializable"))?;
        let realm = cx.realm();
        let text = b::to_js::string(cx.rt, &text)?;
        let data = initialize(cx.rt, realm, Some(text), init, Some("application/json"))?;
        wrap(cx, data)
    }

    fn type_(cx: &mut Cx<'_>, this: Platform) -> b::Result<ResponseType> {
        Ok(data(cx.rt, this)?.kind)
    }

    fn url(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(data(cx.rt, this)?.url.clone())
    }

    fn redirected(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(data(cx.rt, this)?.redirected)
    }

    fn status(cx: &mut Cx<'_>, this: Platform) -> b::Result<u16> {
        Ok(data(cx.rt, this)?.status)
    }

    fn ok(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok((200..300).contains(&data(cx.rt, this)?.status))
    }

    fn status_text(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(data(cx.rt, this)?.status_text.clone())
    }

    fn headers(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        Ok(data(cx.rt, this)?.headers)
    }

    fn body(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<Gc<Object>>> {
        let has_body = data(cx.rt, this)?.body.is_some();
        super::stream_body(cx.rt, has_body);
        Ok(None)
    }

    fn body_used(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(data(cx.rt, this)?.body.as_ref().is_some_and(|b| b.used))
    }

    fn clone(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        let realm = cx.realm();
        let source = data(cx.rt, this)?;
        if source.body.as_ref().is_some_and(|b| b.used) {
            return Err(Exception::type_error("Response body is already used"));
        }
        let copy = ResponseData {
            kind: source.kind,
            url: source.url.clone(),
            redirected: source.redirected,
            status: source.status,
            status_text: source.status_text.clone(),
            headers: source.headers,
            body: source
                .body
                .as_ref()
                .map(|b| BodyState::new(b.bytes.clone())),
            signal: source.signal,
        };
        let guard = if copy.kind == ResponseType::Default {
            Guard::Response
        } else {
            Guard::Immutable
        };
        let headers = headers::copy_of(cx.rt, realm, copy.headers, guard)?;
        wrap(cx, ResponseData { headers, ..copy })
    }

    fn array_buffer(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        body::read::<ResponseData>(cx, this, Read::ArrayBuffer)
    }

    fn blob(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        body::read::<ResponseData>(cx, this, Read::Blob)
    }

    fn bytes(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        body::read::<ResponseData>(cx, this, Read::Bytes)
    }

    fn form_data(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        body::read::<ResponseData>(cx, this, Read::FormData)
    }

    fn json(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        body::read::<ResponseData>(cx, this, Read::Json)
    }

    fn text(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        body::read::<ResponseData>(cx, this, Read::Text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reason_phrases_and_null_body_statuses() {
        assert_eq!(reason_phrase(200), "OK");
        assert_eq!(reason_phrase(404), "Not Found");
        assert_eq!(reason_phrase(299), "");
        assert!(is_null_body_status(204));
        assert!(!is_null_body_status(200));
    }
}
