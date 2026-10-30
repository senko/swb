//! `XMLHttpRequest` (XHR Standard, <https://xhr.spec.whatwg.org/>), the
//! asynchronous part, with `XMLHttpRequestUpload` and the progress events
//! (M8 feature 10b, ADR 0028 section 12).
//!
//! # Data flow
//!
//! `send()` builds a `swb_net::Request` (mode `cors`, credentials
//! `same-origin` or `include` with `withCredentials`) and starts it as
//! `fetch()` does ([`crate::fetch::start_request`]: the engine's loader, its
//! 64 places and queue, the CORS check and the budget of the document). The
//! engine hands the result back in a task ([`completed`]); the whole
//! sequence of events of the response runs in that task, in the order that
//! Chromium shows (measured, `tools/probes/web-xhr.json`):
//!
//! - `send()`: `loadstart` at the request; with a body also `loadstart` at
//!   `upload` (computable, `loaded` 0, `total` the body's length).
//! - the response: with a body of more than 0 bytes `progress`, `load` and
//!   `loadend` at `upload` first (a body of 0 bytes fires none of them);
//!   `readystatechange` 2; then, if the body is not empty, `readystatechange`
//!   3 and one `progress`; `readystatechange` 4, `load`, `loadend`.
//! - a network error, `abort()` and the `timeout`: `readystatechange` 4, the
//!   events at `upload` (for `error` only if the request had a body; for
//!   `abort` and `timeout` always), the event, `loadend`.
//!
//! After each event of the sequence the code checks that the request is
//! still in the state it expects (a listener may call `abort()`, `open()`
//! or `send()`); `load` and `loadend` fire without the check (XHR "handle
//! response end-of-body").
//!
//! # Deviations
//!
//! - Synchronous requests (`open(…, false)`) throw `InvalidAccessError` (ADR
//!   0028 section 12) and log a warning.
//! - One `progress` event; Chromium fires one per chunk of a large body and
//!   a last one with `readyState` 4.
//! - `responseXML` and `responseType` `"document"` are `null` for an XML
//!   response (swb has no XML parser); a `Document` as the body of `send()`
//!   is sent as its string.
//! - `statusText` is the standard reason phrase of the status.
//! - The credentials in the URL and the `user` and `password` arguments of
//!   `open()` are ignored.
//! - `setRequestHeader` keeps at most [`MAX_REQUEST_HEADERS`] headers and
//!   [`MAX_REQUEST_HEADER_BYTES`] bytes of them per request; the rest is
//!   dropped with a warning (ADR 0028 section 16).
//! - `Page::stop` drops the requests in flight without events
//!   ([`drop_requests`]); the objects keep their `readyState`.
//!
//! # Memory
//!
//! The response bytes, the text cache and the request headers are outside
//! the script heap; every change of them goes through [`xhr_resize`], which
//! charges the change to the heap at once (a collection would count it only
//! later, and the tasks of a response run no collection).

use swb_js::{Gc, HostData, Object, Runtime, Tracer, Value, VmResult};

use crate::bindings::{self as b, Cx, Exception, NodeId, Platform};
use crate::event_loop::{Task, TaskKey};
use crate::events::{EventData, Extra, TargetData};
use crate::fetch::{body, headers};
use crate::generated as g;
use crate::generated::types::{EventInit, XMLHttpRequestResponseType as ResponseType};
use crate::state::{WebState, dom_mut, engine_mut, state_mut};

/// The most request headers of one request (`setRequestHeader`); a real page
/// sets a dozen.
const MAX_REQUEST_HEADERS: usize = 256;

/// The most bytes of names and values of the request headers of one request
/// (64 KiB; a real page sends a few hundred bytes, a token a few KiB).
const MAX_REQUEST_HEADER_BYTES: usize = 64 << 10;

/// The state of an `XMLHttpRequest` (in the `TargetData` of the object).
#[derive(Debug)]
pub(crate) struct XhrState {
    /// `readyState`.
    state: u16,
    /// The `send()` flag.
    send: bool,
    /// Counts the requests, `abort()` and `open()`, so that a sequence of
    /// events can tell that it is over.
    generation: u64,
    method: String,
    url: String,
    /// The request headers, combined by name.
    request_headers: Vec<(String, String)>,
    /// True after the warning about the limits of the request headers.
    headers_warned: bool,
    timeout: u32,
    /// The task of the `timeout` while the request runs.
    timeout_key: Option<TaskKey>,
    with_credentials: bool,
    response_type: ResponseType,
    upload: Gc<Object>,
    upload_len: Option<u64>,
    upload_complete: bool,
    /// The id of the request in flight.
    request_id: Option<u64>,
    mime_override: Option<String>,
    status: u16,
    status_text: String,
    response_url: String,
    /// The response headers as received.
    response_headers: Vec<(String, String)>,
    bytes: Vec<u8>,
    /// The decoded `responseText` and the object of `response`, once made.
    text: Option<String>,
    object: Option<Value>,
}

impl XhrState {
    fn new(upload: Gc<Object>) -> XhrState {
        XhrState {
            state: 0,
            send: false,
            generation: 0,
            method: String::new(),
            url: String::new(),
            request_headers: Vec::new(),
            headers_warned: false,
            timeout: 0,
            timeout_key: None,
            with_credentials: false,
            response_type: ResponseType::Empty,
            upload,
            upload_len: None,
            upload_complete: false,
            request_id: None,
            mime_override: None,
            status: 0,
            status_text: String::new(),
            response_url: String::new(),
            response_headers: Vec::new(),
            bytes: Vec::new(),
            text: None,
            object: None,
        }
    }

    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.object(self.upload);
        if let Some(object) = self.object {
            tracer.value(object);
        }
    }

    pub(crate) fn heap_size(&self) -> usize {
        256 + self.bytes.capacity()
            + self.text.as_ref().map_or(0, String::capacity)
            + self.url.capacity()
            + self
                .response_headers
                .iter()
                .chain(&self.request_headers)
                .map(|(n, v)| n.capacity() + v.capacity() + 48)
                .sum::<usize>()
    }

    /// Adds a request header (XHR "setRequestHeader": a repeated name
    /// combines its values with `, `). Past [`MAX_REQUEST_HEADERS`] headers
    /// or [`MAX_REQUEST_HEADER_BYTES`] bytes the header is dropped, with one
    /// warning per request. The lookup scans at most that many headers.
    fn add_request_header(&mut self, name: String, value: String) {
        let used: usize = self
            .request_headers
            .iter()
            .map(|(n, v)| n.len() + v.len() + 2)
            .sum();
        let existing = self
            .request_headers
            .iter()
            .position(|(n, _)| n.eq_ignore_ascii_case(&name));
        // A repeated name adds `, ` and the value; a new one its name, the
        // value and 2 (as `used` counts them).
        let added = match existing {
            Some(_) => value.len() + 2,
            None => name.len() + value.len() + 2,
        };
        let fits = used + added <= MAX_REQUEST_HEADER_BYTES
            && (existing.is_some() || self.request_headers.len() < MAX_REQUEST_HEADERS);
        if !fits {
            if !self.headers_warned {
                self.headers_warned = true;
                log::warn!(
                    "XMLHttpRequest: a request holds at most {MAX_REQUEST_HEADERS} headers and {MAX_REQUEST_HEADER_BYTES} bytes of them; setRequestHeader({name}) is ignored"
                );
            }
            return;
        }
        match existing.and_then(|i| self.request_headers.get_mut(i)) {
            Some((_, old)) => {
                old.push_str(", ");
                old.push_str(&value);
            }
            None => self.request_headers.push((name, value)),
        }
    }

    /// Forgets the response (XHR "response" is a network error).
    fn clear_response(&mut self) {
        self.status = 0;
        self.status_text.clear();
        self.response_url.clear();
        self.response_headers.clear();
        self.bytes = Vec::new();
        self.text = None;
        self.object = None;
    }

    /// The value of the response header `name` (lower case), the values of
    /// repeated headers joined with `, `.
    fn header(&self, name: &str) -> Option<String> {
        let values: Vec<&str> = self
            .response_headers
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
            .collect();
        (!values.is_empty()).then(|| values.join(", "))
    }

    /// The MIME type of the response: the override, else `Content-Type`.
    fn mime(&self) -> String {
        self.mime_override
            .clone()
            .or_else(|| self.header("content-type"))
            .unwrap_or_default()
    }

    /// The loaded and total bytes and whether the length is computable, as
    /// the events of the response report them.
    fn progress(&self) -> (u64, u64, bool) {
        let len = self.bytes.len() as u64;
        if len > 0 {
            (len, len, true)
        } else {
            (0, 0, false)
        }
    }
}

/// The state of the `XMLHttpRequest` object `object`.
fn xhr_mut(rt: &mut Runtime, object: Gc<Object>) -> b::Result<&mut XhrState> {
    let illegal = || Exception::type_error("Illegal invocation");
    let Some((_, HostData::Generic(handle))) = rt.host_object(object.into()) else {
        return Err(illegal());
    };
    rt.heap_mut()
        .generic_mut::<TargetData>(handle)
        .map_err(|_| illegal())?
        .xhr
        .as_deref_mut()
        .ok_or_else(illegal)
}

/// Runs `change` on the state of the `XMLHttpRequest` `object` and charges
/// the change of its size to the heap (`Heap::generic_resized`). Use it for
/// every change that takes or frees a buffer.
fn xhr_resize<R>(
    rt: &mut Runtime,
    object: Gc<Object>,
    change: impl FnOnce(&mut XhrState) -> R,
) -> b::Result<R> {
    let illegal = || Exception::type_error("Illegal invocation");
    let Some((_, HostData::Generic(handle))) = rt.host_object(object.into()) else {
        return Err(illegal());
    };
    rt.heap_mut()
        .generic_resized::<TargetData, _>(handle, |data| data.xhr.as_deref_mut().map(change))
        .map_err(|_| illegal())?
        .ok_or_else(illegal)
}

/// `xhr_mut` for the code that runs in a task: a failure is a `VmError`.
fn xhr_task(rt: &mut Runtime, object: Gc<Object>) -> VmResult<&mut XhrState> {
    xhr_mut(rt, object).map_err(crate::fetch::exception_error)
}

/// [`xhr_resize`] for the code that runs in a task.
fn xhr_resize_task<R>(
    rt: &mut Runtime,
    object: Gc<Object>,
    change: impl FnOnce(&mut XhrState) -> R,
) -> VmResult<R> {
    xhr_resize(rt, object, change).map_err(crate::fetch::exception_error)
}

/// Removes the task of the `timeout` of `xhr`, if one is queued.
fn cancel_timeout(rt: &mut Runtime, xhr: Gc<Object>) {
    let key = xhr_mut(rt, xhr).ok().and_then(|s| s.timeout_key.take());
    if let (Some(key), Ok(app)) = (key, state_mut(rt)) {
        app.event_loop.tasks.remove(key);
    }
}

// ----- Events -----

/// Fires the trusted event `kind` (an `Event`) at `target`.
fn fire_plain(rt: &mut Runtime, target: Gc<Object>, kind: &str) -> VmResult<()> {
    let init = EventInit {
        bubbles: false,
        cancelable: false,
        composed: false,
    };
    let now = crate::events::now(rt);
    let mut data = EventData::new(kind.to_owned(), &init, now, Extra::None);
    data.is_trusted = true;
    let scope = rt.heap_mut().open_scope();
    let result = crate::events::new_event_object(rt, 0, g::EVENT, data)
        .and_then(|event| crate::events::dispatch(rt, 0, target, event, None));
    if let Err(error) = rt.heap_mut().close_scope(scope) {
        log::warn!("the handle scope of an event was not the last one: {error:?}");
    }
    result.map(|_| ())
}

/// Fires the progress event `kind` at the request with its current
/// progress.
fn fire_at_request(rt: &mut Runtime, xhr: Gc<Object>, kind: &str) -> VmResult<()> {
    let (loaded, total, computable) = xhr_task(rt, xhr)?.progress();
    crate::events::misc::fire_progress(rt, xhr, kind, computable, loaded, total)
}

/// Fires a progress event at the upload object.
fn fire_at_upload(
    rt: &mut Runtime,
    xhr: Gc<Object>,
    kind: &str,
    loaded: u64,
    total: u64,
    computable: bool,
) -> VmResult<()> {
    let upload = xhr_task(rt, xhr)?.upload;
    crate::events::misc::fire_progress(rt, upload, kind, computable, loaded, total)
}

fn set_state(rt: &mut Runtime, xhr: Gc<Object>, state: u16) -> VmResult<()> {
    xhr_task(rt, xhr)?.state = state;
    fire_plain(rt, xhr, "readystatechange")
}

/// True if the request of generation `gen` is still sending in `state`.
fn still(rt: &mut Runtime, xhr: Gc<Object>, gen_: u64, state: u16) -> bool {
    xhr_task(rt, xhr).is_ok_and(|s| s.generation == gen_ && s.send && s.state == state)
}

// ----- The life of a request -----

/// Puts the head and the body of `response` in the state. Returns the length
/// of the request body if the upload ends with this response (the events at
/// `upload` follow).
fn store_response(s: &mut XhrState, response: swb_net::Response) -> Option<u64> {
    s.status = response.status;
    crate::fetch::response::reason_phrase(response.status).clone_into(&mut s.status_text);
    let mut url = response.url.clone();
    url.set_fragment(None);
    s.response_url = url.to_string();
    s.response_headers = response
        .headers
        .iter()
        .map(|(n, v)| {
            (
                n.to_ascii_lowercase(),
                headers::normalize_value(v).to_owned(),
            )
        })
        .collect();
    s.bytes = if s.method == "HEAD" {
        Vec::new()
    } else {
        response.body
    };
    // The request body went out before the response. A request without a
    // body keeps the flag unset: an `abort()` after the headers still fires
    // `abort` and `loadend` at `upload` (Chromium, measured; XHR "send"
    // sets the flag at once for a null body).
    let len = s.upload_len?;
    let pending = !s.upload_complete;
    s.upload_complete = true;
    pending.then_some(len)
}

/// The response (XHR "handle response").
fn handle_response(
    rt: &mut Runtime,
    xhr: Gc<Object>,
    gen_: u64,
    response: swb_net::Response,
) -> VmResult<()> {
    // The bytes of the body count against the heap as soon as they are in
    // the state.
    let upload = xhr_resize_task(rt, xhr, |s| store_response(s, response))?;
    if let Some(len) = upload.filter(|&l| l > 0) {
        fire_at_upload(rt, xhr, "progress", len, len, true)?;
        fire_at_upload(rt, xhr, "load", len, len, true)?;
        fire_at_upload(rt, xhr, "loadend", len, len, true)?;
        if !still(rt, xhr, gen_, 1) {
            return Ok(());
        }
    }
    set_state(rt, xhr, 2)?;
    if !still(rt, xhr, gen_, 2) {
        return Ok(());
    }
    if !xhr_task(rt, xhr)?.bytes.is_empty() {
        set_state(rt, xhr, 3)?;
        if !still(rt, xhr, gen_, 3) {
            return Ok(());
        }
        fire_at_request(rt, xhr, "progress")?;
        if !still(rt, xhr, gen_, 3) {
            return Ok(());
        }
    }
    {
        let s = xhr_task(rt, xhr)?;
        s.state = 4;
        s.send = false;
    }
    fire_plain(rt, xhr, "readystatechange")?;
    fire_at_request(rt, xhr, "load")?;
    fire_at_request(rt, xhr, "loadend")
}

/// XHR "request error steps" for `event` (`error`, `abort` or `timeout`).
fn request_error(rt: &mut Runtime, xhr: Gc<Object>, event: &str) -> VmResult<()> {
    let upload = xhr_resize_task(rt, xhr, |s| {
        s.state = 4;
        s.send = false;
        s.clear_response();
        let fire = !s.upload_complete && (s.upload_len.is_some() || event != "error");
        s.upload_complete = true;
        fire
    })?;
    fire_plain(rt, xhr, "readystatechange")?;
    if upload {
        fire_at_upload(rt, xhr, event, 0, 0, false)?;
        fire_at_upload(rt, xhr, "loadend", 0, 0, false)?;
    }
    fire_at_request(rt, xhr, event)?;
    fire_at_request(rt, xhr, "loadend")
}

/// The engine's result of the request `id` of the object `xhr`. The caller
/// removed the object from the table of requests in flight, so it is no
/// longer reachable from there: it stays alive as long as the task's handle
/// scope (a script may drop its last reference in a listener, and the
/// events that follow collect).
pub(crate) fn completed(
    rt: &mut Runtime,
    xhr: Gc<Object>,
    id: u64,
    result: Result<swb_net::Response, swb_net::NetError>,
) -> VmResult<()> {
    rt.heap_mut().record(xhr);
    let gen_ = {
        let s = xhr_task(rt, xhr)?;
        if s.request_id != Some(id) || !s.send {
            return Ok(());
        }
        s.request_id = None;
        s.generation
    };
    cancel_timeout(rt, xhr);
    match result {
        Ok(response) => handle_response(rt, xhr, gen_, response),
        Err(error) => {
            match error.console_message("XMLHttpRequest") {
                Some(message) => crate::runtime::report_line(rt, "XMLHttpRequest", &message),
                None if matches!(error, swb_net::NetError::OverBudget { .. }) => {
                    log::warn!("XMLHttpRequest failed: {error}");
                }
                None => log::debug!("XMLHttpRequest failed: {error}"),
            }
            request_error(rt, xhr, "error")
        }
    }
}

/// The `timeout` of the request `id` is over.
pub(crate) fn timeout_fired(rt: &mut Runtime, id: u64) -> VmResult<()> {
    let Some(xhr) = state_mut(rt)?.fetches.xhrs.remove(&id) else {
        return Ok(());
    };
    // Out of the table, the object lives as long as the task's scope.
    rt.heap_mut().record(xhr);
    {
        let s = xhr_task(rt, xhr)?;
        if s.request_id != Some(id) || !s.send {
            return Ok(());
        }
        s.request_id = None;
        s.timeout_key = None;
        s.generation += 1;
    }
    if let Ok(host) = engine_mut(rt) {
        host.cancel_api_fetch(id);
    }
    request_error(rt, xhr, "timeout")
}

/// Drops the request in flight of `xhr` without events (XHR "terminate the
/// ongoing fetch").
fn terminate(rt: &mut Runtime, xhr: Gc<Object>) -> b::Result<()> {
    let id = xhr_mut(rt, xhr)?.request_id.take();
    cancel_timeout(rt, xhr);
    if let Some(id) = id {
        state_mut(rt)?.fetches.xhrs.remove(&id);
        if let Ok(host) = engine_mut(rt) {
            host.cancel_api_fetch(id);
        }
    }
    Ok(())
}

// ----- open() -----

/// The HTTP methods that `open()` refuses (Fetch "forbidden method").
fn is_forbidden_method(method: &str) -> bool {
    ["connect", "trace", "track"]
        .iter()
        .any(|m| method.eq_ignore_ascii_case(m))
}

/// The method as the request uses it: the six standard methods in upper
/// case (XHR "open", step 5).
fn normalize_method(method: &str) -> String {
    for known in ["DELETE", "GET", "HEAD", "OPTIONS", "POST", "PUT"] {
        if method.eq_ignore_ascii_case(known) {
            return known.to_owned();
        }
    }
    method.to_owned()
}

fn open(cx: &mut Cx<'_>, this: Platform, method: &str, url: &str) -> b::Result<()> {
    if !swb_net::is_token(method) {
        return Err(Exception::dom(
            "SyntaxError",
            format!("'{method}' is not a valid HTTP method."),
        ));
    }
    if is_forbidden_method(method) {
        return Err(Exception::dom(
            "SecurityError",
            format!("'{method}' HTTP method is unsupported."),
        ));
    }
    let Some(parsed) = crate::fetch::engine_parse_url(cx.rt, url)? else {
        return Err(Exception::dom("SyntaxError", "Invalid URL"));
    };
    terminate(cx.rt, this.object)?;
    let fire = xhr_resize(cx.rt, this.object, |s| {
        s.generation += 1;
        s.method = normalize_method(method);
        s.url = parsed;
        s.request_headers = Vec::new();
        s.headers_warned = false;
        s.send = false;
        s.upload_len = None;
        s.upload_complete = false;
        s.clear_response();
        let changed = s.state != 1;
        s.state = 1;
        changed
    })?;
    if fire {
        fire_plain(cx.rt, this.object, "readystatechange")?;
    }
    Ok(())
}

// ----- send() -----

/// The body of `send()`: `None` for a null body or a `GET` or `HEAD`
/// request, else the bytes and the `Content-Type` the body implies.
fn send_body(rt: &mut Runtime, method: &str, value: Value) -> b::Result<Option<body::Extracted>> {
    if matches!(value, Value::Null | Value::Undefined) || matches!(method, "GET" | "HEAD") {
        return Ok(None);
    }
    Ok(Some(body::extract(rt, value)?))
}

/// The listeners at `upload` that force a preflight (XHR "send", step 11).
const UPLOAD_EVENTS: [&str; 7] = [
    "loadstart",
    "progress",
    "abort",
    "error",
    "load",
    "timeout",
    "loadend",
];

/// Builds the request of `send()` from the state and the extracted body, and
/// marks the state as sending. Returns the request and the length of its
/// body.
fn build_request(
    s: &mut XhrState,
    extracted: Option<body::Extracted>,
    upload_listener: bool,
) -> b::Result<(swb_net::Request, Option<u64>)> {
    let Ok(url) = swb_net::Url::parse(&s.url) else {
        return Err(Exception::dom("SyntaxError", "Invalid URL"));
    };
    let mut request = swb_net::Request::get(url, swb_net::Destination::Other);
    request.method = swb_net::Method::parse(&s.method).unwrap_or(swb_net::Method::Get);
    for (name, value) in &s.request_headers {
        request.headers.append(name, value.clone());
    }
    request.mode = swb_net::RequestMode::Cors;
    request.credentials = if s.with_credentials {
        swb_net::CredentialsMode::Include
    } else {
        swb_net::CredentialsMode::SameOrigin
    };
    request.exposed_to_script = true;
    let mut upload_len = None;
    if let Some(extracted) = extracted {
        // A body and a listener at `upload` force a preflight (XHR "send",
        // step 11; Chromium sends none for a request without a body,
        // measured).
        request.use_cors_preflight = upload_listener;
        if let Some(content_type) = &extracted.content_type
            && !s
                .request_headers
                .iter()
                .any(|(n, _)| n.eq_ignore_ascii_case("content-type"))
        {
            request.headers.append("content-type", content_type.clone());
        }
        upload_len = Some(extracted.bytes.len() as u64);
        request.body = Some(extracted.bytes);
    }
    s.send = true;
    s.generation += 1;
    s.upload_len = upload_len;
    s.upload_complete = false;
    Ok((request, upload_len))
}

/// Hands `request` to the loader (or finishes it locally if the loader
/// refuses it) and records the request in the state and the tables.
fn start(rt: &mut Runtime, xhr: Gc<Object>, request: swb_net::Request) -> b::Result<()> {
    let scheme = request.url.scheme().to_owned();
    let id = if matches!(scheme.as_str(), "http" | "https" | "data" | "blob") {
        crate::fetch::start_request(rt, request)?
    } else {
        None
    };
    let id = if let Some(id) = id {
        id
    } else {
        log::warn!("XMLHttpRequest to {scheme}: URL was refused");
        let error = swb_net::NetError::UnsupportedScheme(scheme);
        crate::fetch::finish_locally(rt, Err(error))?
    };
    let timeout = {
        let s = xhr_mut(rt, xhr)?;
        s.request_id = Some(id);
        s.timeout
    };
    let app = state_mut(rt)?;
    app.fetches.xhrs.insert(id, xhr);
    if timeout > 0 {
        let due = app.clock.page_now() + f64::from(timeout);
        let key = app.event_loop.tasks.push(due, Task::XhrTimeout(id));
        xhr_mut(rt, xhr)?.timeout_key = Some(key);
    }
    Ok(())
}

fn send(cx: &mut Cx<'_>, this: Platform, value: Value) -> b::Result<()> {
    let xhr = this.object;
    let (state, sending, method) = {
        let s = xhr_mut(cx.rt, xhr)?;
        (s.state, s.send, s.method.clone())
    };
    if state != 1 || sending {
        return Err(Exception::dom(
            "InvalidStateError",
            "The object's state must be OPENED.",
        ));
    }
    let extracted = send_body(cx.rt, &method, value)?;
    let upload = xhr_mut(cx.rt, xhr)?.upload;
    let upload_listener = crate::events::has_listener_for(cx.rt, upload, &UPLOAD_EVENTS);
    let (request, upload_len) = build_request(xhr_mut(cx.rt, xhr)?, extracted, upload_listener)?;
    let gen_ = xhr_mut(cx.rt, xhr)?.generation;
    // A listener of `loadstart` may have aborted or reopened.
    if !fire_loadstart(cx.rt, xhr, gen_, upload_len)? {
        return Ok(());
    }
    start(cx.rt, xhr, request)
}

/// The `loadstart` events of `send()`, with a check after each: a listener
/// may abort (the upload is then complete and fires nothing). True if the
/// request is still sending.
fn fire_loadstart(
    rt: &mut Runtime,
    xhr: Gc<Object>,
    gen_: u64,
    upload: Option<u64>,
) -> VmResult<bool> {
    crate::events::misc::fire_progress(rt, xhr, "loadstart", false, 0, 0)?;
    if !still(rt, xhr, gen_, 1) {
        return Ok(false);
    }
    if let Some(len) = upload {
        fire_at_upload(rt, xhr, "loadstart", 0, len, true)?;
    }
    Ok(still(rt, xhr, gen_, 1))
}

/// Drops the requests in flight without events (the page stopped loading):
/// the objects keep their `readyState`, and no `abort`, `error` or `timeout`
/// event follows.
pub(crate) fn drop_requests(rt: &mut Runtime) {
    let Ok(app) = state_mut(rt) else {
        return;
    };
    let objects: Vec<Gc<Object>> = app.fetches.xhrs.drain().map(|(_, xhr)| xhr).collect();
    for xhr in objects {
        cancel_timeout(rt, xhr);
        if let Ok(s) = xhr_mut(rt, xhr) {
            s.request_id = None;
        }
    }
}

// ----- Reading the response -----

/// `charset=` of a MIME type, if any.
fn charset_of(mime: &str) -> Option<&str> {
    let lower = mime.to_ascii_lowercase();
    let at = lower.find("charset=")? + "charset=".len();
    let rest = mime.get(at..)?;
    let value = rest.split(';').next()?.trim().trim_matches('"');
    (!value.is_empty()).then_some(value)
}

/// The decoded text of the response (XHR "text response"): the charset of
/// the MIME type, else UTF-8; a BOM wins.
fn decode(state: &XhrState) -> String {
    let mime = state.mime();
    let encoding = charset_of(&mime)
        .and_then(|label| encoding_rs::Encoding::for_label(label.as_bytes()))
        .unwrap_or(encoding_rs::UTF_8);
    encoding.decode(&state.bytes).0.into_owned()
}

fn accessible_text(state: &XhrState) -> bool {
    matches!(
        state.response_type,
        ResponseType::Empty | ResponseType::Text
    )
}

fn type_name(kind: ResponseType) -> &'static str {
    match kind {
        ResponseType::Empty => "",
        ResponseType::Arraybuffer => "arraybuffer",
        ResponseType::Blob => "blob",
        ResponseType::Document => "document",
        ResponseType::Json => "json",
        ResponseType::Text => "text",
    }
}

/// The document of an HTML response, or `None` (XML is not parsed).
fn html_document(rt: &mut Runtime, realm: u32, text: &str, mime: &str) -> b::Result<Option<Value>> {
    let essence = mime
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if !(essence == "text/html" || essence.is_empty()) {
        return Ok(None);
    }
    let result = swb_dom::parse_document(dom_mut(rt)?, text);
    let node = crate::dom::dom_result(rt, result)?;
    state_mut(rt)?.parsed_documents.insert(node);
    Ok(Some(
        <WebState as b::Host>::wrap_node(rt, realm, node)?.into(),
    ))
}

impl g::xml_http_request::XMLHttpRequestImpl for WebState {
    fn constructor(cx: &mut Cx<'_>) -> b::Result<Gc<Object>> {
        let realm = cx.realm();
        let upload_data = cx.rt.alloc_host_data(TargetData::default())?;
        let upload = cx
            .rt
            .new_host_object(realm, g::XML_HTTP_REQUEST_UPLOAD, upload_data)?;
        let mut data = TargetData::default();
        data.xhr = Some(Box::new(XhrState::new(upload)));
        let handle = cx.rt.alloc_host_data(data)?;
        Ok(cx.new_instance::<WebState>(handle)?)
    }

    fn ready_state(cx: &mut Cx<'_>, this: Platform) -> b::Result<u16> {
        Ok(xhr_mut(cx.rt, this.object)?.state)
    }

    fn timeout(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        Ok(xhr_mut(cx.rt, this.object)?.timeout)
    }

    fn set_timeout(cx: &mut Cx<'_>, this: Platform, value: u32) -> b::Result<()> {
        xhr_mut(cx.rt, this.object)?.timeout = value;
        Ok(())
    }

    fn with_credentials(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(xhr_mut(cx.rt, this.object)?.with_credentials)
    }

    fn set_with_credentials(cx: &mut Cx<'_>, this: Platform, value: bool) -> b::Result<()> {
        let s = xhr_mut(cx.rt, this.object)?;
        if !matches!(s.state, 0 | 1) || s.send {
            return Err(Exception::dom(
                "InvalidStateError",
                "The value may only be set if the object's state is UNSENT or OPENED.",
            ));
        }
        s.with_credentials = value;
        Ok(())
    }

    fn upload(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        Ok(xhr_mut(cx.rt, this.object)?.upload)
    }

    fn response_url(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(xhr_mut(cx.rt, this.object)?.response_url.clone())
    }

    fn status(cx: &mut Cx<'_>, this: Platform) -> b::Result<u16> {
        Ok(xhr_mut(cx.rt, this.object)?.status)
    }

    fn status_text(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(xhr_mut(cx.rt, this.object)?.status_text.clone())
    }

    fn response_type(cx: &mut Cx<'_>, this: Platform) -> b::Result<ResponseType> {
        Ok(xhr_mut(cx.rt, this.object)?.response_type)
    }

    fn set_response_type(cx: &mut Cx<'_>, this: Platform, value: ResponseType) -> b::Result<()> {
        let s = xhr_mut(cx.rt, this.object)?;
        if matches!(s.state, 3 | 4) {
            return Err(Exception::dom(
                "InvalidStateError",
                "The response type cannot be set if the object's state is LOADING or DONE.",
            ));
        }
        s.response_type = value;
        Ok(())
    }

    fn response(cx: &mut Cx<'_>, this: Platform) -> b::Result<Value> {
        let realm = cx.realm();
        let (kind, state) = {
            let s = xhr_mut(cx.rt, this.object)?;
            (s.response_type, s.state)
        };
        if matches!(kind, ResponseType::Empty | ResponseType::Text) {
            return if matches!(state, 3 | 4) {
                let text = text_of(cx.rt, this.object)?;
                Ok(b::to_js::string(cx.rt, &text)?)
            } else {
                Ok(b::to_js::string(cx.rt, "")?)
            };
        }
        if state != 4 {
            return Ok(Value::Null);
        }
        if let Some(object) = xhr_mut(cx.rt, this.object)?.object {
            return Ok(object);
        }
        cx.rt.check_time()?;
        let value = match kind {
            ResponseType::Arraybuffer => {
                let bytes = xhr_mut(cx.rt, this.object)?.bytes.clone();
                cx.rt.create_array_buffer(realm, bytes, None)?.into()
            }
            ResponseType::Blob => {
                let (bytes, mime) = {
                    let s = xhr_mut(cx.rt, this.object)?;
                    (s.bytes.clone(), s.mime())
                };
                crate::file_api::make_blob(cx.rt, realm, bytes, &mime)?.into()
            }
            ResponseType::Json => {
                let text = body::utf8_text(&xhr_mut(cx.rt, this.object)?.bytes);
                body::parse_json_or_null(cx.rt, &text)
            }
            ResponseType::Document => {
                let (text, mime) = {
                    let s = xhr_mut(cx.rt, this.object)?;
                    (decode(s), s.mime())
                };
                html_document(cx.rt, realm, &text, &mime)?.unwrap_or(Value::Null)
            }
            ResponseType::Empty | ResponseType::Text => Value::Null,
        };
        xhr_mut(cx.rt, this.object)?.object = Some(value);
        Ok(value)
    }

    fn response_text(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        let (accessible, kind, state) = {
            let s = xhr_mut(cx.rt, this.object)?;
            (accessible_text(s), s.response_type, s.state)
        };
        if !accessible {
            return Err(Exception::dom(
                "InvalidStateError",
                format!(
                    "The value is only accessible if the object's 'responseType' is '' or 'text' (was '{}').",
                    type_name(kind)
                ),
            ));
        }
        if !matches!(state, 3 | 4) {
            return Ok(String::new());
        }
        text_of(cx.rt, this.object)
    }

    fn response_xml(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<NodeId>> {
        let (kind, state) = {
            let s = xhr_mut(cx.rt, this.object)?;
            (s.response_type, s.state)
        };
        if !matches!(kind, ResponseType::Empty | ResponseType::Document) {
            return Err(Exception::dom(
                "InvalidStateError",
                format!(
                    "The value is only accessible if the object's 'responseType' is '' or 'document' (was '{}').",
                    type_name(kind)
                ),
            ));
        }
        if state != 4 || kind == ResponseType::Empty {
            // A `""` response is a document only for an XML type, which
            // swb does not parse.
            return Ok(None);
        }
        let value = Self::response(cx, this)?;
        Ok(b::node_of(
            cx.rt
                .host_object(value)
                .map_or(HostData::Word(u64::MAX), |(_, data)| data),
        ))
    }

    fn open_1(cx: &mut Cx<'_>, this: Platform, method: String, url: String) -> b::Result<()> {
        open(cx, this, &method, &url)
    }

    fn open_2(
        cx: &mut Cx<'_>,
        this: Platform,
        method: String,
        url: String,
        async_: bool,
        _username: Option<String>,
        _password: Option<String>,
    ) -> b::Result<()> {
        if !async_ {
            log::warn!("synchronous XMLHttpRequest is not supported: open({method}, {url}, false)");
            return Err(Exception::dom(
                "InvalidAccessError",
                "Synchronous requests are not supported.",
            ));
        }
        open(cx, this, &method, &url)
    }

    fn set_request_header(
        cx: &mut Cx<'_>,
        this: Platform,
        name: String,
        value: String,
    ) -> b::Result<()> {
        {
            let s = xhr_mut(cx.rt, this.object)?;
            if s.state != 1 || s.send {
                return Err(Exception::dom(
                    "InvalidStateError",
                    "The object's state must be OPENED.",
                ));
            }
        }
        let value = headers::normalize_value(&value).to_owned();
        if !swb_net::is_token(&name) {
            return Err(Exception::dom(
                "SyntaxError",
                format!("'{name}' is not a valid HTTP header field name."),
            ));
        }
        if value.chars().any(|c| matches!(c, '\0' | '\n' | '\r')) {
            return Err(Exception::dom(
                "SyntaxError",
                format!("'{value}' is not a valid HTTP header field value."),
            ));
        }
        if headers::is_forbidden_request_header(&name.to_ascii_lowercase(), &value) {
            log::debug!("XMLHttpRequest: the header {name} is refused");
            return Ok(());
        }
        xhr_resize(cx.rt, this.object, |s| s.add_request_header(name, value))
    }

    fn send(cx: &mut Cx<'_>, this: Platform, body: Value) -> b::Result<()> {
        send(cx, this, body)
    }

    fn abort(cx: &mut Cx<'_>, this: Platform) -> b::Result<()> {
        let xhr = this.object;
        terminate(cx.rt, xhr)?;
        let in_flight = {
            let s = xhr_mut(cx.rt, xhr)?;
            s.generation += 1;
            (s.state == 1 && s.send) || matches!(s.state, 2 | 3)
        };
        if in_flight {
            request_error(cx.rt, xhr, "abort")?;
        }
        xhr_resize(cx.rt, xhr, |s| {
            if s.state == 4 {
                s.state = 0;
                s.clear_response();
            }
        })
    }

    fn get_response_header(
        cx: &mut Cx<'_>,
        this: Platform,
        name: String,
    ) -> b::Result<Option<String>> {
        let s = xhr_mut(cx.rt, this.object)?;
        if s.state < 2 || headers::is_forbidden_response_header(&name.to_ascii_lowercase()) {
            return Ok(None);
        }
        Ok(s.header(&name))
    }

    fn get_all_response_headers(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        let s = xhr_mut(cx.rt, this.object)?;
        if s.state < 2 {
            return Ok(String::new());
        }
        let mut names: Vec<&str> = s
            .response_headers
            .iter()
            .map(|(n, _)| n.as_str())
            .filter(|n| !headers::is_forbidden_response_header(n))
            .collect();
        names.sort_unstable();
        names.dedup();
        let mut out = String::new();
        for name in names {
            if let Some(value) = s.header(name) {
                out.push_str(name);
                out.push_str(": ");
                out.push_str(&value);
                out.push_str("\r\n");
            }
        }
        Ok(out)
    }

    fn override_mime_type(cx: &mut Cx<'_>, this: Platform, mime: String) -> b::Result<()> {
        let s = xhr_mut(cx.rt, this.object)?;
        if matches!(s.state, 3 | 4) {
            return Err(Exception::dom(
                "InvalidStateError",
                "MIME types cannot be overridden when the state is LOADING or DONE.",
            ));
        }
        s.mime_override = (!mime.trim().is_empty()).then_some(mime);
        Ok(())
    }
}

/// The decoded text of the response, cached.
fn text_of(rt: &mut Runtime, xhr: Gc<Object>) -> b::Result<String> {
    if let Some(text) = &xhr_mut(rt, xhr)?.text {
        return Ok(text.clone());
    }
    rt.check_time()?;
    xhr_resize(rt, xhr, |s| {
        let text = decode(s);
        if s.state == 4 {
            s.text = Some(text.clone());
        }
        text
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn methods() {
        assert_eq!(normalize_method("get"), "GET");
        assert_eq!(normalize_method("patch"), "patch");
        assert!(is_forbidden_method("Trace"));
        assert!(!is_forbidden_method("PUT"));
    }

    #[test]
    fn charsets() {
        assert_eq!(
            charset_of("text/plain; charset=ISO-8859-1"),
            Some("ISO-8859-1")
        );
        assert_eq!(
            charset_of("text/plain; charset=\"utf-8\"; x=y"),
            Some("utf-8")
        );
        assert_eq!(charset_of("text/plain"), None);
    }
}
