//! `Request` (Fetch §5.5, <https://fetch.spec.whatwg.org/#request-class>):
//! the constructor, the attributes and `clone()`. The same code builds the
//! request of `fetch()` ([`build`]).
//!
//! A request keeps its URL as the parser serialized it (with the fragment),
//! its method, the `Headers` object (guard `request`, `request-no-cors` in
//! mode `no-cors`), the body bytes, the options as the generated
//! enumerations, and its own `AbortSignal`, which follows the signal that
//! the caller gave. The referrer is not sent: swb sends no `Referer`
//! header.

use swb_js::{Gc, GenericData, HostData, Object, Runtime, Tracer, Value};

use super::body::{self, BodyState, HasBody, Read};
use super::generic;
use super::headers::{self, Guard};
use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::generated as g;
use crate::generated::types::{
    ReferrerPolicy, RequestCache, RequestCredentials, RequestDestination, RequestDuplex,
    RequestInit, RequestMode, RequestRedirect,
};
use crate::state::WebState;

/// The data of a `Request` object.
#[derive(Debug)]
pub(crate) struct RequestData {
    pub(crate) url: String,
    pub(crate) method: String,
    pub(crate) headers: Gc<Object>,
    pub(crate) body: Option<BodyState>,
    pub(crate) signal: Gc<Object>,
    pub(crate) mode: RequestMode,
    pub(crate) credentials: RequestCredentials,
    pub(crate) cache: RequestCache,
    pub(crate) redirect: RequestRedirect,
    pub(crate) referrer: String,
    pub(crate) referrer_policy: ReferrerPolicy,
    pub(crate) integrity: String,
    pub(crate) keepalive: bool,
}

impl GenericData for RequestData {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.object(self.headers);
        tracer.object(self.signal);
    }

    fn heap_size(&self) -> usize {
        160 + self.url.capacity()
            + self.method.capacity()
            + self.referrer.capacity()
            + self.integrity.capacity()
            + BodyState::heap_size(self.body.as_ref())
    }
}

/// The options of a request that a new request takes from its input and
/// `RequestInit` overrides.
struct Options {
    url: String,
    method: String,
    mode: RequestMode,
    credentials: RequestCredentials,
    cache: RequestCache,
    redirect: RequestRedirect,
    referrer: String,
    referrer_policy: ReferrerPolicy,
    integrity: String,
    keepalive: bool,
}

impl Options {
    /// The options of a request for `url` that is made from a string.
    fn for_url(url: String) -> Options {
        Options {
            url,
            method: "GET".to_owned(),
            mode: RequestMode::Cors,
            credentials: RequestCredentials::SameOrigin,
            cache: RequestCache::Default,
            redirect: RequestRedirect::Follow,
            referrer: "about:client".to_owned(),
            referrer_policy: ReferrerPolicy::Empty,
            integrity: String::new(),
            keepalive: false,
        }
    }

    /// Applies `RequestInit` (steps 12 to 27 of the constructor).
    fn apply(&mut self, rt: &mut Runtime, init: &mut RequestInit) -> b::Result<()> {
        if let Some(value) = init.mode {
            if value == RequestMode::Navigate {
                return Err(Exception::type_error(
                    "Cannot construct a Request with a RequestInit whose mode member is set as 'navigate'.",
                ));
            }
            self.mode = value;
        }
        self.credentials = init.credentials.unwrap_or(self.credentials);
        self.cache = init.cache.unwrap_or(self.cache);
        self.redirect = init.redirect.unwrap_or(self.redirect);
        self.referrer_policy = init.referrer_policy.unwrap_or(self.referrer_policy);
        self.keepalive = init.keepalive.unwrap_or(self.keepalive);
        if let Some(value) = init.integrity.take() {
            self.integrity = value;
        }
        if let Some(value) = init.referrer.take() {
            self.referrer = if value.is_empty() || value == "about:client" {
                value
            } else {
                super::engine_parse_url(rt, &value)?.ok_or_else(|| {
                    Exception::type_error(format!("Failed to parse referrer URL: {value}"))
                })?
            };
        }
        if self.cache == RequestCache::OnlyIfCached && self.mode != RequestMode::SameOrigin {
            return Err(Exception::type_error(
                "'only-if-cached' can be set only with 'same-origin' mode",
            ));
        }
        if let Some(value) = init.method.take() {
            self.method = init_method(value)?;
        }
        if self.mode == RequestMode::NoCors
            && !swb_net::Method::parse(&self.method).is_some_and(|m| m.is_cors_safelisted())
        {
            return Err(Exception::type_error(format!(
                "'{}' is unsupported in no-cors mode.",
                self.method
            )));
        }
        Ok(())
    }
}

/// What a `Request` input stands for: the options, and for a request the
/// objects that the new request shares or takes.
struct Source {
    options: Options,
    signal: Option<Gc<Object>>,
    headers: Option<Gc<Object>>,
    /// The input request, whose body the new request takes.
    body_from: Option<Gc<Object>>,
}

impl RequestData {
    fn options(&self) -> Options {
        Options {
            url: self.url.clone(),
            method: self.method.clone(),
            mode: self.mode,
            credentials: self.credentials,
            cache: self.cache,
            redirect: self.redirect,
            referrer: self.referrer.clone(),
            referrer_policy: self.referrer_policy,
            integrity: self.integrity.clone(),
            keepalive: self.keepalive,
        }
    }
}

/// The source of `input`: a `Request` (not used), or a URL string.
fn source_of(rt: &mut Runtime, input: Value) -> b::Result<Source> {
    let request = match input {
        Value::Object(object) => of_object(rt, object).map(|data| {
            (
                object,
                data.options(),
                data.signal,
                data.headers,
                data.body.as_ref().is_some_and(|b| b.used),
            )
        }),
        _ => None,
    };
    let Some((object, options, signal, headers, used)) = request else {
        let text = b::convert::usv_string(rt, input)?;
        return Ok(Source {
            options: Options::for_url(parse_url(rt, &text)?),
            signal: None,
            headers: None,
            body_from: None,
        });
    };
    if used {
        return Err(Exception::type_error(
            "Cannot construct a Request with a Request object that has already been used.",
        ));
    }
    Ok(Source {
        options,
        signal: Some(signal),
        headers: Some(headers),
        body_from: Some(object),
    })
}

impl HasBody for RequestData {
    fn body_mut(&mut self) -> &mut Option<BodyState> {
        &mut self.body
    }

    fn headers_object(&self) -> Gc<Object> {
        self.headers
    }
}

/// The data of the `Request` object `object`, if it is one.
fn of_object(rt: &mut Runtime, object: Gc<Object>) -> Option<&mut RequestData> {
    if !rt.implements(object.into(), g::REQUEST) {
        return None;
    }
    let Some((_, HostData::Generic(handle))) = rt.host_object(object.into()) else {
        return None;
    };
    rt.heap_mut().generic_mut::<RequestData>(handle).ok()
}

/// The guard of the headers of a request in `mode`.
fn guard_of(mode: RequestMode) -> Guard {
    if mode == RequestMode::NoCors {
        Guard::RequestNoCors
    } else {
        Guard::Request
    }
}

/// The URL of a request: `value` parsed against the document's base URL.
fn parse_url(rt: &mut Runtime, value: &str) -> b::Result<String> {
    let serialized = super::engine_parse_url(rt, value)?
        .ok_or_else(|| Exception::type_error(format!("Failed to parse URL from {value}")))?;
    if let Ok(url) = swb_net::Url::parse(&serialized)
        && (!url.username().is_empty() || url.password().is_some())
    {
        return Err(Exception::type_error(format!(
            "Request cannot be constructed from a URL that includes credentials: {value}"
        )));
    }
    Ok(serialized)
}

/// The body of a new request: the init's, else the input's (which becomes
/// used). `Content-Type` of the new body is added to `headers` if they have
/// none.
fn take_body(
    rt: &mut Runtime,
    headers: Gc<Object>,
    method: &str,
    init_body: Option<Value>,
    input: Option<Gc<Object>>,
) -> b::Result<Option<BodyState>> {
    let no_body = || Exception::type_error("Request with GET/HEAD method cannot have body.");
    match init_body {
        Some(value) if !matches!(value, Value::Null) => {
            if matches!(method, "GET" | "HEAD") {
                return Err(no_body());
            }
            let extracted = body::extract(rt, value)?;
            if let Some(content_type) = extracted.content_type
                && headers::value_of(rt, headers, "content-type")?.is_none()
            {
                headers::append_to(rt, headers, "Content-Type", &content_type)?;
            }
            Ok(Some(BodyState::new(extracted.bytes)))
        }
        other => {
            let Some(source) = input else {
                return Ok(None);
            };
            let Some(state) = of_object(rt, source).and_then(|data| data.body.as_mut()) else {
                return Ok(None);
            };
            if matches!(method, "GET" | "HEAD") && other.is_some() {
                return Err(no_body());
            }
            state.used = true;
            Ok(Some(BodyState::new(std::mem::take(&mut state.bytes))))
        }
    }
}

/// The method of a request from `RequestInit.method`: a token that is not
/// forbidden, with the case normalized.
fn init_method(value: String) -> b::Result<String> {
    if !swb_net::is_token(&value) {
        return Err(Exception::type_error(format!(
            "'{value}' is not a valid HTTP method."
        )));
    }
    if swb_net::Method::is_forbidden(&value) {
        return Err(Exception::type_error(format!(
            "'{value}' HTTP method is unsupported."
        )));
    }
    Ok(swb_net::Method::parse(&value).map_or(value, |m| m.as_str().to_owned()))
}

/// Fetch "new Request(input, init)", steps 5 to 35: the request that the
/// constructor wraps and `fetch()` sends. With `follow_signal` the request
/// has a signal of its own that follows the given one; without it (for
/// `fetch()`) it is the given one.
pub(crate) fn build(
    rt: &mut Runtime,
    realm: u32,
    input: Value,
    mut init: RequestInit,
    follow_signal: bool,
) -> b::Result<RequestData> {
    let mut source = source_of(rt, input)?;
    source.options.apply(rt, &mut init)?;
    let options = source.options;
    if let Some(signal) = init.signal {
        source.signal = signal.map(|p| p.object);
    }
    // The headers: the init's, else a copy of the input's, under the guard.
    let headers = headers::new_object(rt, realm, guard_of(options.mode), Vec::new())?;
    if let Some(init_headers) = init.headers {
        headers::fill(rt, headers, init_headers).map_err(|e| e.member("headers", "RequestInit"))?;
    } else if let Some(from) = source.headers {
        for (name, value) in headers::list_of(rt, from)? {
            headers::append_to(rt, headers, &name, &value)?;
        }
    }
    let body = take_body(rt, headers, &options.method, init.body, source.body_from)?;
    let signal = if follow_signal {
        crate::abort::follow(rt, realm, source.signal)?
    } else {
        crate::abort::source_or_new(rt, realm, source.signal)?
    };
    Ok(RequestData {
        url: options.url,
        method: options.method,
        headers,
        body,
        signal,
        mode: options.mode,
        credentials: options.credentials,
        cache: options.cache,
        redirect: options.redirect,
        referrer: options.referrer,
        referrer_policy: options.referrer_policy,
        integrity: options.integrity,
        keepalive: options.keepalive,
    })
}

/// The request for the network that `data` stands for.
pub(crate) fn to_net_request(
    rt: &mut Runtime,
    data: &RequestData,
) -> b::Result<Option<swb_net::Request>> {
    let Ok(url) = swb_net::Url::parse(&data.url) else {
        return Ok(None);
    };
    let Some(method) = swb_net::Method::parse(&data.method) else {
        return Ok(None);
    };
    let mut request = swb_net::Request::get(url, swb_net::Destination::Other);
    request.method = method;
    for (name, value) in headers::list_of(rt, data.headers)? {
        request.headers.append(&name, value);
    }
    request.body = data
        .body
        .as_ref()
        .map(|b| b.bytes.clone())
        .filter(|b| !b.is_empty() || data.method != "GET");
    request.mode = match data.mode {
        RequestMode::Navigate => swb_net::RequestMode::Navigate,
        RequestMode::SameOrigin => swb_net::RequestMode::SameOrigin,
        RequestMode::NoCors => swb_net::RequestMode::NoCors,
        RequestMode::Cors => swb_net::RequestMode::Cors,
    };
    request.credentials = match data.credentials {
        RequestCredentials::Omit => swb_net::CredentialsMode::Omit,
        RequestCredentials::SameOrigin => swb_net::CredentialsMode::SameOrigin,
        RequestCredentials::Include => swb_net::CredentialsMode::Include,
    };
    request.redirect = match data.redirect {
        RequestRedirect::Follow => swb_net::RedirectMode::Follow,
        RequestRedirect::Error => swb_net::RedirectMode::Error,
        RequestRedirect::Manual => swb_net::RedirectMode::Manual,
    };
    request.exposed_to_script = true;
    Ok(Some(request))
}

fn data(rt: &Runtime, this: Platform) -> b::Result<&RequestData> {
    generic::<RequestData>(rt, this)
}

impl g::request::RequestImpl for WebState {
    fn constructor(cx: &mut Cx<'_>, input: Value, init: RequestInit) -> b::Result<Gc<Object>> {
        let realm = cx.realm();
        let request = build(cx.rt, realm, input, init, true)?;
        let handle = cx.rt.alloc_host_data(request)?;
        Ok(cx.new_instance::<WebState>(handle)?)
    }

    fn method(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(data(cx.rt, this)?.method.clone())
    }

    fn url(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(data(cx.rt, this)?.url.clone())
    }

    fn headers(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        Ok(data(cx.rt, this)?.headers)
    }

    fn destination(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<RequestDestination> {
        Ok(RequestDestination::Empty)
    }

    fn referrer(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(data(cx.rt, this)?.referrer.clone())
    }

    fn referrer_policy(cx: &mut Cx<'_>, this: Platform) -> b::Result<ReferrerPolicy> {
        Ok(data(cx.rt, this)?.referrer_policy)
    }

    fn mode(cx: &mut Cx<'_>, this: Platform) -> b::Result<RequestMode> {
        Ok(data(cx.rt, this)?.mode)
    }

    fn credentials(cx: &mut Cx<'_>, this: Platform) -> b::Result<RequestCredentials> {
        Ok(data(cx.rt, this)?.credentials)
    }

    fn cache(cx: &mut Cx<'_>, this: Platform) -> b::Result<RequestCache> {
        Ok(data(cx.rt, this)?.cache)
    }

    fn redirect(cx: &mut Cx<'_>, this: Platform) -> b::Result<RequestRedirect> {
        Ok(data(cx.rt, this)?.redirect)
    }

    fn integrity(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(data(cx.rt, this)?.integrity.clone())
    }

    fn keepalive(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(data(cx.rt, this)?.keepalive)
    }

    fn is_history_navigation(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<bool> {
        Ok(false)
    }

    fn signal(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        Ok(data(cx.rt, this)?.signal)
    }

    fn duplex(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<RequestDuplex> {
        Ok(RequestDuplex::Half)
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
            return Err(Exception::type_error("Request body is already used"));
        }
        let mut copy = RequestData {
            url: source.url.clone(),
            method: source.method.clone(),
            headers: source.headers,
            body: source
                .body
                .as_ref()
                .map(|b| BodyState::new(b.bytes.clone())),
            signal: source.signal,
            mode: source.mode,
            credentials: source.credentials,
            cache: source.cache,
            redirect: source.redirect,
            referrer: source.referrer.clone(),
            referrer_policy: source.referrer_policy,
            integrity: source.integrity.clone(),
            keepalive: source.keepalive,
        };
        copy.headers = headers::copy_of(cx.rt, realm, copy.headers, guard_of(copy.mode))?;
        copy.signal = crate::abort::follow(cx.rt, realm, Some(copy.signal))?;
        let handle = cx.rt.alloc_host_data(copy)?;
        Ok(cx.rt.new_host_object(realm, g::REQUEST, handle)?)
    }

    fn array_buffer(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        body::read::<RequestData>(cx, this, Read::ArrayBuffer)
    }

    fn blob(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        body::read::<RequestData>(cx, this, Read::Blob)
    }

    fn bytes(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        body::read::<RequestData>(cx, this, Read::Bytes)
    }

    fn form_data(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        body::read::<RequestData>(cx, this, Read::FormData)
    }

    fn json(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        body::read::<RequestData>(cx, this, Read::Json)
    }

    fn text(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        body::read::<RequestData>(cx, this, Read::Text)
    }
}
