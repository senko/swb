//! `fetch()`, `Headers`, `Request` and `Response` (M8 feature 10a, ADR 0028
//! section 12; Fetch Standard, <https://fetch.spec.whatwg.org/>).
//!
//! - `headers`: the header list with its guards, sorted iteration.
//! - `body`: the bytes of a body, `extract` and the Body mixin methods.
//! - `request`, `response`: the two classes and their constructors.
//! - this module: `fetch()` ([`fetch_impl`]), the table of calls in
//!   flight, the completion that the engine hands back
//!   ([`complete`], [`abort_fetch`]).
//!
//! # Data flow
//!
//! `fetch(input, init)` builds the request ([`request::build`]), checks the
//! scheme, hands a `swb_net::Request` to the engine
//! ([`crate::EngineHost::start_api_fetch`]) and returns a pending promise
//! that the table of calls keeps. The engine's loader fetches it (CORS,
//! credentials and redirects are in `net`, `swb_net::fetch_request`) and the
//! page's loop hands the result back as a task
//! ([`crate::ScriptRuntime::api_fetch_completed`]), which resolves the
//! promise with a `Response` or rejects it with `TypeError: Failed to fetch`
//! and logs Chromium's console message for a CORS failure. An abort of the
//! request's signal rejects the promise with the signal's reason and tells
//! the engine to drop the request ([`crate::EngineHost::cancel_api_fetch`]).
//!
//! # Deviations
//!
//! - `Response.body` and `Request.body` are `null` (no streams) with one
//!   warning per document.
//! - `blob()` and `formData()` reject with `NotSupportedError` until
//!   feature 10b.
//! - The response is complete when the promise resolves (the engine reads
//!   whole bodies), so an abort after the response does not interrupt the
//!   body read.
//! - Only `http:`, `https:` and `data:` URLs are fetched; `blob:` comes with
//!   feature 10b.

pub(crate) mod body;
pub(crate) mod headers;
pub(crate) mod request;
pub(crate) mod response;

use std::collections::HashMap;

pub(crate) use body::utf8_text;

use swb_js::{Gc, HostData, Object, Runtime, ThrowKind, Tracer, Value, VmResult};

use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::generated::types::RequestInit;
use crate::state::{engine_mut, state_mut};

/// A call of `fetch()` in flight.
#[derive(Debug)]
struct Pending {
    promise: Gc<Object>,
    /// The signal that aborts the call (the caller's, or a signal that
    /// never aborts).
    signal: Gc<Object>,
    /// The request URL, for an opaque-redirect response.
    url: String,
    /// True for a `HEAD` request (its response has no body).
    head: bool,
}

/// The `fetch()` calls in flight, by the id that the engine knows.
#[derive(Debug, Default)]
pub(crate) struct Fetches {
    pending: HashMap<u64, Pending>,
    /// The `XMLHttpRequest` objects in flight, by request id (`xhr.rs`).
    pub(crate) xhrs: HashMap<u64, Gc<Object>>,
    /// The beacons in flight (`navigator.sendBeacon`): their sizes by id.
    pub(crate) beacons: HashMap<u64, usize>,
    /// The requests that did not go to the network (Blob URLs, refused
    /// requests), by id, until their task runs.
    local: HashMap<u64, Local>,
    /// The entries of `local` that hold a Blob (at most [`MAX_LOCAL_BLOBS`]).
    local_blobs: usize,
    /// True after the warning about [`MAX_LOCAL_BLOBS`].
    local_warned: bool,
    next_local: u64,
    /// True after the warning about `body` (no streams).
    stream_warned: bool,
}

/// A request that does not go to the network, until its task runs.
#[derive(Debug)]
enum Local {
    /// The result is known.
    Done(Result<swb_net::Response, swb_net::NetError>),
    /// A `GET` of a Blob URL: the handle of the Blob (a cheap clone; the
    /// bytes are copied into the response when the task runs, one request at
    /// a time) and its type.
    Blob {
        url: swb_net::Url,
        bytes: crate::file_api::BlobBytes,
        kind: String,
    },
}

/// The most requests of Blob URLs that wait for their task. A request past
/// it fails like one that the engine refuses (a network error). The
/// requests of the network are bounded by the places and the queue of the
/// engine (ADR 0028 section 16).
const MAX_LOCAL_BLOBS: usize = 256;

impl Fetches {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for pending in self.pending.values() {
            tracer.object(pending.promise);
            tracer.object(pending.signal);
        }
        for xhr in self.xhrs.values() {
            tracer.object(*xhr);
        }
    }
}

/// The data of a platform object `this` as a `T`.
pub(super) fn generic<T: swb_js::GenericData>(rt: &Runtime, this: Platform) -> b::Result<&T> {
    match this.data {
        HostData::Generic(handle) => rt
            .heap()
            .generic::<T>(handle)
            .map_err(|_| Exception::type_error("Illegal invocation")),
        HostData::Word(_) => Err(Exception::type_error("Illegal invocation")),
    }
}

/// [`generic`] for changes.
pub(super) fn generic_mut<T: swb_js::GenericData>(
    rt: &mut Runtime,
    this: Platform,
) -> b::Result<&mut T> {
    match this.data {
        HostData::Generic(handle) => rt
            .heap_mut()
            .generic_mut::<T>(handle)
            .map_err(|_| Exception::type_error("Illegal invocation")),
        HostData::Word(_) => Err(Exception::type_error("Illegal invocation")),
    }
}

/// `value` parsed against the document's base URL and serialized, or
/// `None` if it does not parse.
pub(crate) fn engine_parse_url(rt: &mut Runtime, value: &str) -> b::Result<Option<String>> {
    Ok(engine_mut(rt)?.parse_url(value))
}

/// Logs the warning for `Response.body` and `Request.body` once.
fn stream_body(rt: &mut Runtime, has_body: bool) {
    if !has_body {
        return;
    }
    if let Ok(state) = state_mut(rt)
        && !state.fetches.stream_warned
    {
        state.fetches.stream_warned = true;
        log::warn!("Request.body and Response.body are null: swb has no streams");
    }
}

/// A `TypeError: Failed to fetch` for a rejection.
fn network_error(rt: &mut Runtime) -> VmResult<Value> {
    Ok(rt
        .new_error(ThrowKind::TypeError, "Failed to fetch")?
        .into())
}

/// Rejects a new promise with `reason`.
fn rejected(rt: &mut Runtime, realm: u32, reason: Value) -> b::Result<Gc<Object>> {
    match rt.rejected_promise(realm, reason)? {
        Value::Object(promise) => Ok(promise),
        _ => Err(Exception::type_error("a rejected promise is not an object")),
    }
}

/// `fetch(input, init)` (Fetch "fetch method").
///
/// <https://fetch.spec.whatwg.org/#fetch-method>
fn fetch_impl(cx: &mut Cx<'_>, input: Value, init: RequestInit) -> b::Result<Gc<Object>> {
    let realm = cx.realm();
    let request = request::build(cx.rt, realm, input, init, false)?;
    if crate::abort::is_aborted(cx.rt, request.signal)? {
        let reason = crate::abort::reason_of(cx.rt, request.signal)?;
        return rejected(cx.rt, realm, reason);
    }
    let Some(net_request) = request::to_net_request(cx.rt, &request)? else {
        let error = network_error(cx.rt)?;
        return rejected(cx.rt, realm, error);
    };
    let scheme = net_request.url.scheme().to_owned();
    if !matches!(scheme.as_str(), "http" | "https" | "data" | "blob") {
        crate::runtime::report_line(
            cx.rt,
            "fetch",
            &format!(
                "Fetch API cannot load {}. URL scheme \"{scheme}\" is not supported.",
                request.url
            ),
        );
        let error = network_error(cx.rt)?;
        return rejected(cx.rt, realm, error);
    }
    let head = net_request.method == swb_net::Method::Head;
    let promise = cx.rt.new_pending_promise(realm)?;
    let Some(id) = start_request(cx.rt, net_request)? else {
        log::warn!("fetch({}) was refused: too many requests", request.url);
        let error = network_error(cx.rt)?;
        cx.rt.reject_pending_promise(promise, error)?;
        return Ok(promise);
    };
    crate::abort::register_fetch(cx.rt, request.signal, id)?;
    state_mut(cx.rt)?.fetches.pending.insert(
        id,
        Pending {
            promise,
            signal: request.signal,
            url: request.url,
            head,
        },
    );
    Ok(promise)
}

/// The result of a fetch of the engine: the final response with its kind,
/// or the error.
pub(crate) fn complete(
    rt: &mut Runtime,
    id: u64,
    result: Result<swb_net::Response, swb_net::NetError>,
    kind: swb_net::ResponseKind,
) -> VmResult<()> {
    if let Some(size) = state_mut(rt)?.fetches.beacons.remove(&id) {
        // A beacon has no handler; the quota returns.
        let state = state_mut(rt)?;
        state.beacon_bytes = state.beacon_bytes.saturating_sub(size);
        return Ok(());
    }
    if let Some(xhr) = state_mut(rt)?.fetches.xhrs.remove(&id) {
        return crate::xhr::completed(rt, xhr, id, result);
    }
    let Some(pending) = state_mut(rt)?.fetches.pending.remove(&id) else {
        // Aborted before.
        return Ok(());
    };
    // Out of the table, the promise and the signal live as long as the
    // task's scope (the response is made before the promise settles).
    rt.heap_mut().record(pending.promise);
    rt.heap_mut().record(pending.signal);
    crate::abort::unregister_fetch(rt, pending.signal, id);
    complete_fetch(rt, &pending, result, kind)
}

fn complete_fetch(
    rt: &mut Runtime,
    pending: &Pending,
    result: Result<swb_net::Response, swb_net::NetError>,
    kind: swb_net::ResponseKind,
) -> VmResult<()> {
    match result {
        Ok(response) => {
            let object = response::from_net(
                rt,
                0,
                response,
                kind,
                &pending.url,
                pending.head,
                pending.signal,
            );
            match object {
                Ok(object) => rt.resolve_pending_promise(pending.promise, object.into()),
                Err(error) => {
                    // The response does not fit in the heap: the promise
                    // rejects (a pending promise would never settle).
                    log::warn!("fetch({}): {:?}", pending.url, exception_error(error));
                    let reason = network_error(rt)?;
                    rt.reject_pending_promise(pending.promise, reason)
                }
            }
        }
        Err(error) => {
            match error.console_message("fetch") {
                Some(message) => crate::runtime::report_line(rt, "fetch", &message),
                None if matches!(error, swb_net::NetError::OverBudget { .. }) => {
                    log::warn!("fetch({}) failed: {error}", pending.url);
                }
                None => log::debug!("fetch({}) failed: {error}", pending.url),
            }
            let reason = network_error(rt)?;
            rt.reject_pending_promise(pending.promise, reason)
        }
    }
}

/// The first id of the requests that do not go to the network.
const LOCAL_BASE: u64 = 1 << 62;

/// Starts `request`: a `blob:` URL is answered from the Blob URL store in
/// a task of its own, anything else goes to the engine. `None` if the
/// engine refused it or too many requests of Blob URLs wait.
pub(crate) fn start_request(rt: &mut Runtime, request: swb_net::Request) -> VmResult<Option<u64>> {
    if request.url.scheme() != "blob" {
        return Ok(engine_mut(rt)?.start_api_fetch(request));
    }
    let entry = state_mut(rt)?.blob_urls.get(request.url.as_str());
    let local = match entry {
        Some((bytes, kind)) if request.method == swb_net::Method::Get => Local::Blob {
            url: request.url,
            bytes,
            kind,
        },
        // A revoked or unknown URL, a method other than `GET`.
        _ => Local::Done(Err(swb_net::NetError::UnsupportedScheme("blob".to_owned()))),
    };
    if let Local::Blob { bytes, .. } = &local {
        let state = state_mut(rt)?;
        if state.fetches.local_blobs >= MAX_LOCAL_BLOBS {
            if !state.fetches.local_warned {
                state.fetches.local_warned = true;
                log::warn!("too many requests of Blob URLs wait: {MAX_LOCAL_BLOBS}");
            }
            return Ok(None);
        }
        // The page may drop the Blob while the request waits.
        if !state.held_blobs.hold(bytes) {
            return Ok(None);
        }
    }
    Ok(Some(queue_local(rt, local)?))
}

/// Queues the completion of a request that did not go to the network.
pub(crate) fn finish_locally(
    rt: &mut Runtime,
    result: Result<swb_net::Response, swb_net::NetError>,
) -> VmResult<u64> {
    queue_local(rt, Local::Done(result))
}

fn queue_local(rt: &mut Runtime, local: Local) -> VmResult<u64> {
    let state = state_mut(rt)?;
    state.fetches.next_local += 1;
    let id = LOCAL_BASE + state.fetches.next_local;
    if matches!(local, Local::Blob { .. }) {
        state.fetches.local_blobs += 1;
    }
    state.fetches.local.insert(id, local);
    let due = state.clock.page_now();
    state
        .event_loop
        .tasks
        .push(due, crate::event_loop::Task::LocalFetch(id));
    Ok(id)
}

/// The response for a `blob:` request: the stored bytes with their type.
fn blob_response(
    url: swb_net::Url,
    bytes: &crate::file_api::BlobBytes,
    kind: String,
) -> swb_net::Response {
    let mut headers = swb_net::Headers::default();
    if !kind.is_empty() {
        headers.append("content-type", kind);
    }
    headers.append("content-length", bytes.len().to_string());
    swb_net::Response {
        url,
        status: 200,
        headers,
        body: bytes.to_vec(),
        redirected: false,
    }
}

/// The task of a request that did not go to the network.
pub(crate) fn local_completed(rt: &mut Runtime, id: u64) -> VmResult<()> {
    let state = state_mut(rt)?;
    let Some(local) = state.fetches.local.remove(&id) else {
        return Ok(());
    };
    let result = match local {
        Local::Done(result) => result,
        Local::Blob { url, bytes, kind } => {
            state.fetches.local_blobs = state.fetches.local_blobs.saturating_sub(1);
            // The copy happens here, one request at a time; its owner (an
            // XHR or a `Response`) charges it to the heap.
            let response = blob_response(url, &bytes, kind);
            state.held_blobs.release(&bytes);
            Ok(response)
        }
    };
    complete(rt, id, result, swb_net::ResponseKind::Basic)
}

/// The signal of the call `id` aborted: rejects its promise with `reason`
/// and tells the engine to drop the request.
pub(crate) fn abort_fetch(rt: &mut Runtime, _realm: u32, id: u64, reason: Value) -> VmResult<()> {
    let Some(pending) = state_mut(rt)?.fetches.pending.remove(&id) else {
        return Ok(());
    };
    if let Ok(host) = engine_mut(rt) {
        host.cancel_api_fetch(id);
    }
    rt.reject_pending_promise(pending.promise, reason)
}

/// The error of a binding in a place that returns a `VmResult` (a task):
/// the engine's error passes, anything else becomes a `TypeError` with its
/// description.
pub(crate) fn exception_error(exception: Exception) -> swb_js::VmError {
    match exception {
        Exception::Js(error) => error,
        Exception::Type(message) => swb_js::VmError::type_error(message),
        other => swb_js::VmError::type_error(format!("{other:?}")),
    }
}

/// The `fetch` member of `Window` (`window.rs` calls it).
pub(crate) fn window_fetch(
    cx: &mut Cx<'_>,
    input: Value,
    init: RequestInit,
) -> b::Result<Gc<Object>> {
    fetch_impl(cx, input, init)
}
