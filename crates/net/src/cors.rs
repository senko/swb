//! CORS: the CORS check, the CORS-preflight fetch, the preflight cache and
//! the filtered responses that scripts see (ADR 0028 section 12).
//!
//! The functions work on a [`Request`] and a [`Response`], so that
//! [`fetch_request`](crate::fetch_request) applies the protocol in one
//! place for every fetch of the page:
//!
//! - [`cors_check`]: <https://fetch.spec.whatwg.org/#concept-cors-check>.
//! - [`needs_preflight`], [`preflight_request`], [`check_preflight`]:
//!   <https://fetch.spec.whatwg.org/#cors-preflight-fetch-0>.
//! - [`unsafe_request_header_names`]:
//!   <https://fetch.spec.whatwg.org/#cors-unsafe-request-header-names>.
//! - [`filter_response`]: the basic, CORS and opaque filtered responses
//!   (<https://fetch.spec.whatwg.org/#concept-filtered-response>).
//! - [`PreflightCache`]: <https://fetch.spec.whatwg.org/#concept-cache>,
//!   bounded.
//!
//! The failure texts are those of Chromium's console (measured with the
//! probe tool, `tools/probes/web-fetch.json`).

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use url::Url;

use crate::headers::Headers;
use crate::mime::MimeType;
use crate::request::{CredentialsMode, Method, RedirectMode, Request, RequestMode, Tainting};
use crate::response::Response;

/// The most entries of the [`PreflightCache`].
pub const PREFLIGHT_CACHE_ENTRIES: usize = 256;

/// The largest `Access-Control-Max-Age` in seconds that the cache keeps (a
/// larger value is cut; Chromium caps it at two hours).
const MAX_AGE_LIMIT: u64 = 7200;

/// The age of an entry without `Access-Control-Max-Age`, in seconds.
const DEFAULT_MAX_AGE: u64 = 5;

/// The kind of a response, as `Response.type` names it.
///
/// <https://fetch.spec.whatwg.org/#concept-response-type>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResponseKind {
    /// Same-origin, or a URL that has no origin (`data:`).
    Basic,
    /// Cross-origin that passed the CORS check.
    Cors,
    /// Cross-origin without CORS (`no-cors` mode).
    Opaque,
    /// A redirect with the redirect mode `manual`.
    OpaqueRedirect,
}

impl ResponseKind {
    /// The name that `Response.type` has.
    pub fn as_str(self) -> &'static str {
        match self {
            ResponseKind::Basic => "basic",
            ResponseKind::Cors => "cors",
            ResponseKind::Opaque => "opaque",
            ResponseKind::OpaqueRedirect => "opaqueredirect",
        }
    }
}

/// Why a CORS check failed. [`CorsFailure::reason`] is the text that
/// Chromium prints after "has been blocked by CORS policy: ".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorsFailure {
    /// The response has no `Access-Control-Allow-Origin` header.
    MissingAllowOrigin,
    /// The header has several values (joined with `, `).
    MultipleAllowOrigin(String),
    /// The header is `*` and the credentials mode is `include`.
    WildcardWithCredentials,
    /// The header names another origin than the request's.
    OriginMismatch(String),
    /// A credentialed request, and `Access-Control-Allow-Credentials` is not
    /// `true` (the value, empty if missing).
    CredentialsNotAllowed(String),
    /// The preflight response has a status outside 200 to 299.
    PreflightStatus,
    /// The preflight answered with a redirect.
    PreflightRedirect,
    /// The method is not in `Access-Control-Allow-Methods`.
    MethodNotAllowed(String),
    /// A request header is not in `Access-Control-Allow-Headers`.
    HeaderNotAllowed(String),
    /// The redirect target has credentials in its URL.
    RedirectCredentials,
}

impl CorsFailure {
    /// The text after "has been blocked by CORS policy: ".
    pub fn reason(&self) -> String {
        match self {
            CorsFailure::MissingAllowOrigin => {
                "No 'Access-Control-Allow-Origin' header is present on the requested resource."
                    .to_owned()
            }
            CorsFailure::MultipleAllowOrigin(value) => format!(
                "The 'Access-Control-Allow-Origin' header contains multiple values '{value}', but only one is allowed. Have the server send the header with a valid value."
            ),
            CorsFailure::WildcardWithCredentials => "The value of the 'Access-Control-Allow-Origin' header in the response must not be the wildcard '*' when the request's credentials mode is 'include'.".to_owned(),
            CorsFailure::OriginMismatch(value) => format!(
                "The 'Access-Control-Allow-Origin' header has a value '{value}' that is not equal to the supplied origin. Have the server send the header with a valid value."
            ),
            CorsFailure::CredentialsNotAllowed(value) => format!(
                "The value of the 'Access-Control-Allow-Credentials' header in the response is '{value}' which must be 'true' when the request's credentials mode is 'include'."
            ),
            CorsFailure::PreflightStatus => {
                "Response to preflight request doesn't pass access control check: It does not have HTTP ok status.".to_owned()
            }
            CorsFailure::PreflightRedirect => {
                "Response to preflight request doesn't pass access control check: Redirect is not allowed for a preflight request.".to_owned()
            }
            CorsFailure::MethodNotAllowed(method) => format!(
                "Method {method} is not allowed by Access-Control-Allow-Methods in preflight response."
            ),
            CorsFailure::HeaderNotAllowed(name) => format!(
                "Request header field {name} is not allowed by Access-Control-Allow-Headers in preflight response."
            ),
            CorsFailure::RedirectCredentials => {
                "Redirect location contains a username and password, which is disallowed for cross-origin requests.".to_owned()
            }
        }
    }

    /// True for a failure of the preflight request (Chromium prefixes the
    /// reason with "Response to preflight request doesn't pass access
    /// control check: ").
    fn is_preflight_check(&self) -> bool {
        matches!(
            self,
            CorsFailure::MissingAllowOrigin
                | CorsFailure::MultipleAllowOrigin(_)
                | CorsFailure::WildcardWithCredentials
                | CorsFailure::OriginMismatch(_)
                | CorsFailure::CredentialsNotAllowed(_)
        )
    }
}

/// The value of `Access-Control-Allow-Origin` etc. as Fetch "gets" a
/// header: all values joined with `, `.
fn joined(headers: &Headers, name: &str) -> Option<String> {
    let mut values = headers.get_all(name).peekable();
    values.peek()?;
    Some(values.collect::<Vec<_>>().join(", "))
}

/// The serialization of the request's origin as the CORS check compares
/// it: `null` after a redirect-tainted chain.
pub(crate) fn serialized_origin(request: &Request) -> String {
    if request.tainted_origin {
        return "null".to_owned();
    }
    request
        .initiator
        .as_ref()
        .map_or_else(|| "null".to_owned(), url::Origin::ascii_serialization)
}

/// Returns true if `credentials` is `include`.
fn includes_credentials(request: &Request) -> bool {
    request.credentials == CredentialsMode::Include
}

/// The CORS check.
///
/// <https://fetch.spec.whatwg.org/#concept-cors-check>
pub fn cors_check(request: &Request, response: &Response) -> Result<(), CorsFailure> {
    let Some(origin) = joined(&response.headers, "access-control-allow-origin") else {
        return Err(CorsFailure::MissingAllowOrigin);
    };
    if !includes_credentials(request) && origin == "*" {
        return Ok(());
    }
    if includes_credentials(request) && origin == "*" {
        return Err(CorsFailure::WildcardWithCredentials);
    }
    if response
        .headers
        .get_all("access-control-allow-origin")
        .count()
        > 1
    {
        return Err(CorsFailure::MultipleAllowOrigin(origin));
    }
    if origin != serialized_origin(request) {
        return Err(CorsFailure::OriginMismatch(origin));
    }
    if !includes_credentials(request) {
        return Ok(());
    }
    let credentials = joined(&response.headers, "access-control-allow-credentials");
    if credentials.as_deref() == Some("true") {
        Ok(())
    } else {
        Err(CorsFailure::CredentialsNotAllowed(
            credentials.unwrap_or_default(),
        ))
    }
}

// ----- CORS-safelisted request headers -----

/// Bytes that make a value CORS-unsafe.
///
/// <https://fetch.spec.whatwg.org/#cors-unsafe-request-header-byte>
fn is_unsafe_byte(b: u8) -> bool {
    (b < 0x20 && b != b'\t')
        || matches!(
            b,
            b'"' | b'('
                | b')'
                | b':'
                | b'<'
                | b'>'
                | b'?'
                | b'@'
                | b'['
                | b'\\'
                | b']'
                | b'{'
                | b'}'
                | 0x7f
        )
}

/// A CORS-safelisted request header: `(name, value)` is safe for a
/// cross-origin request without a preflight.
///
/// <https://fetch.spec.whatwg.org/#cors-safelisted-request-header>
pub fn is_cors_safelisted_header(name: &str, value: &str) -> bool {
    if value.len() > 128 {
        return false;
    }
    let name = name.to_ascii_lowercase();
    match name.as_str() {
        "accept" => !value.bytes().any(is_unsafe_byte),
        "accept-language" | "content-language" => value.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b' ' | b'*' | b',' | b'-' | b'.' | b';' | b'=')
        }),
        "content-type" => {
            if value.bytes().any(is_unsafe_byte) {
                return false;
            }
            MimeType::parse(value).is_some_and(|m| {
                matches!(
                    m.essence().as_str(),
                    "application/x-www-form-urlencoded" | "multipart/form-data" | "text/plain"
                )
            })
        }
        "range" => is_simple_range(value),
        _ => false,
    }
}

/// `bytes=` digits `-` optional digits, with the start not above the end.
fn is_simple_range(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("bytes=") else {
        return false;
    };
    let Some((start, end)) = rest.split_once('-') else {
        return false;
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(start) || !(end.is_empty() || digits(end)) {
        return false;
    }
    match (start.parse::<u64>(), end.parse::<u64>()) {
        (Ok(s), Ok(e)) => s <= e,
        (Ok(_), Err(_)) => end.is_empty(),
        _ => false,
    }
}

/// The names of the request headers that are not CORS-safelisted, in
/// lowercase, sorted, without duplicates.
///
/// <https://fetch.spec.whatwg.org/#cors-unsafe-request-header-names>
pub fn unsafe_request_header_names(headers: &Headers) -> Vec<String> {
    let mut unsafe_names = Vec::new();
    let mut potentially = Vec::new();
    let mut size = 0usize;
    for (name, value) in headers.iter() {
        if is_cors_safelisted_header(name, value) {
            potentially.push(name.to_owned());
            size += value.len();
        } else {
            unsafe_names.push(name.to_owned());
        }
    }
    if size > 1024 {
        unsafe_names.append(&mut potentially);
    }
    unsafe_names.sort();
    unsafe_names.dedup();
    unsafe_names
}

/// True if the request needs a preflight: the use-CORS-preflight flag, a method that is not
/// CORS-safelisted, or a CORS-unsafe request header.
pub fn needs_preflight(request: &Request) -> bool {
    request.use_cors_preflight
        || !request.method.is_cors_safelisted()
        || !unsafe_request_header_names(&request.headers).is_empty()
}

/// The preflight request for `request`: `OPTIONS`, no credentials, and the
/// `Access-Control-Request-Method` and `-Headers` headers.
///
/// <https://fetch.spec.whatwg.org/#cors-preflight-fetch-0>
pub fn preflight_request(request: &Request) -> Request {
    let mut headers = Headers::new();
    headers.append("accept", "*/*");
    headers.append("access-control-request-method", request.method.as_str());
    let names = unsafe_request_header_names(&request.headers);
    if !names.is_empty() {
        headers.append("access-control-request-headers", names.join(","));
    }
    Request {
        url: request.url.clone(),
        method: Method::Options,
        headers,
        body: None,
        destination: request.destination,
        initiator: request.initiator.clone(),
        mode: RequestMode::Cors,
        credentials: CredentialsMode::Omit,
        redirect: RedirectMode::Manual,
        tainting: Tainting::Cors,
        tainted_origin: request.tainted_origin,
        exposed_to_script: false,
        reservation: request.reservation.clone(),
        use_cors_preflight: false,
    }
}

/// What a successful preflight allows, for the cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightAnswer {
    /// The methods in `Access-Control-Allow-Methods`.
    pub methods: Vec<String>,
    /// The header names in `Access-Control-Allow-Headers`, in lowercase.
    pub headers: Vec<String>,
    /// `Access-Control-Max-Age` in seconds (cut to a limit).
    pub max_age: u64,
}

/// Splits a list header into its elements.
fn list_values(headers: &Headers, name: &str) -> Vec<String> {
    headers
        .get_all(name)
        .flat_map(|v| v.split(','))
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
        .collect()
}

/// Checks the response to a preflight for `request` (steps 7 to 14 of the
/// CORS-preflight fetch).
///
/// <https://fetch.spec.whatwg.org/#cors-preflight-fetch-0>
pub fn check_preflight(
    request: &Request,
    response: &Response,
) -> Result<PreflightAnswer, CorsFailure> {
    if response.is_redirect() {
        return Err(CorsFailure::PreflightRedirect);
    }
    cors_check(request, response)?;
    if !response.is_success() {
        return Err(CorsFailure::PreflightStatus);
    }
    let methods = list_values(&response.headers, "access-control-allow-methods");
    let headers: Vec<String> = list_values(&response.headers, "access-control-allow-headers")
        .into_iter()
        .map(|h| h.to_ascii_lowercase())
        .collect();
    let credentials = includes_credentials(request);
    let wildcard_methods = !credentials && methods.iter().any(|m| m == "*");
    let method = request.method.as_str();
    if !methods.iter().any(|m| m == method)
        && !request.method.is_cors_safelisted()
        && !wildcard_methods
    {
        return Err(CorsFailure::MethodNotAllowed(method.to_owned()));
    }
    let wildcard_headers = !credentials && headers.iter().any(|h| h == "*");
    // Deviation from Fetch, as in Chromium 148 (measured): the wildcard also
    // covers `authorization`, which the standard excludes ("CORS
    // non-wildcard request-header name").
    for name in unsafe_request_header_names(&request.headers) {
        if !headers.contains(&name) && !wildcard_headers {
            return Err(CorsFailure::HeaderNotAllowed(name));
        }
    }
    let max_age = joined(&response.headers, "access-control-max-age")
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_MAX_AGE)
        .min(MAX_AGE_LIMIT);
    Ok(PreflightAnswer {
        methods,
        headers,
        max_age,
    })
}

// ----- The preflight cache -----

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    origin: String,
    url: String,
    credentials: bool,
}

#[derive(Debug)]
struct CacheEntry {
    answer: PreflightAnswer,
    expires: Instant,
}

/// The preflight cache: what earlier preflights allowed, for their
/// `Access-Control-Max-Age`. At most [`PREFLIGHT_CACHE_ENTRIES`] entries;
/// when it is full the expired entries go, then the soonest to expire. One
/// cache is shared by the workers of a loader.
#[derive(Debug, Default)]
pub struct PreflightCache {
    entries: Mutex<HashMap<CacheKey, CacheEntry>>,
}

impl PreflightCache {
    /// Creates an empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    fn key(request: &Request) -> CacheKey {
        let mut url = request.url.clone();
        url.set_fragment(None);
        CacheKey {
            origin: serialized_origin(request),
            url: url.to_string(),
            credentials: includes_credentials(request),
        }
    }

    /// True if a cached preflight allows `request` (its method, and all its
    /// CORS-unsafe headers).
    pub fn allows(&self, request: &Request) -> bool {
        let credentials = includes_credentials(request);
        let guard = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(entry) = guard.get(&Self::key(request)) else {
            return false;
        };
        if entry.expires <= Instant::now() {
            return false;
        }
        let answer = &entry.answer;
        let method = request.method.as_str();
        let method_ok = request.method.is_cors_safelisted()
            || answer.methods.iter().any(|m| m == method)
            || (!credentials && answer.methods.iter().any(|m| m == "*"));
        let headers_ok = unsafe_request_header_names(&request.headers)
            .iter()
            .all(|n| {
                answer.headers.contains(n)
                    || (!credentials && answer.headers.iter().any(|h| h == "*"))
            });
        method_ok && headers_ok
    }

    /// Stores what a preflight for `request` allowed.
    pub fn store(&self, request: &Request, answer: PreflightAnswer) {
        if answer.max_age == 0 {
            return;
        }
        let now = Instant::now();
        let mut guard = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        let key = Self::key(request);
        if guard.len() >= PREFLIGHT_CACHE_ENTRIES && !guard.contains_key(&key) {
            guard.retain(|_, e| e.expires > now);
            if guard.len() >= PREFLIGHT_CACHE_ENTRIES
                && let Some(oldest) = guard
                    .iter()
                    .min_by_key(|(_, e)| e.expires)
                    .map(|(k, _)| k.clone())
            {
                guard.remove(&oldest);
            }
        }
        let expires = now + Duration::from_secs(answer.max_age);
        guard.insert(key, CacheEntry { answer, expires });
    }

    /// The number of entries.
    pub fn len(&self) -> usize {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    /// True if the cache has no entry.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ----- Filtered responses -----

/// The response headers that a CORS response exposes without
/// `Access-Control-Expose-Headers`.
///
/// <https://fetch.spec.whatwg.org/#cors-safelisted-response-header-name>
const SAFELISTED_RESPONSE_HEADERS: [&str; 7] = [
    "cache-control",
    "content-language",
    "content-length",
    "content-type",
    "expires",
    "last-modified",
    "pragma",
];

/// The forbidden response-header names: scripts never see them.
///
/// <https://fetch.spec.whatwg.org/#forbidden-response-header-name>
const FORBIDDEN_RESPONSE_HEADERS: [&str; 2] = ["set-cookie", "set-cookie2"];

/// Returns the response that a script gets for `kind`.
///
/// - `Basic`: without the forbidden response headers.
/// - `Cors`: only the CORS-safelisted response headers and those named by
///   `Access-Control-Expose-Headers` (`*` exposes all if the request's
///   credentials mode is not `include`), without the forbidden ones.
/// - `Opaque` and `OpaqueRedirect`: status 0, no headers, no body.
///
/// <https://fetch.spec.whatwg.org/#concept-filtered-response>
pub fn filter_response(request: &Request, mut response: Response, kind: ResponseKind) -> Response {
    match kind {
        ResponseKind::Basic => {
            response.headers = keep_headers(&response.headers, |_| true);
        }
        ResponseKind::Cors => {
            let exposed = list_values(&response.headers, "access-control-expose-headers");
            let all = !includes_credentials(request) && exposed.iter().any(|h| h == "*");
            let exposed: Vec<String> = exposed.iter().map(|h| h.to_ascii_lowercase()).collect();
            response.headers = keep_headers(&response.headers, |name| {
                all || SAFELISTED_RESPONSE_HEADERS.contains(&name)
                    || exposed.iter().any(|e| e == name)
            });
        }
        ResponseKind::Opaque | ResponseKind::OpaqueRedirect => {
            response.status = 0;
            response.redirected = false;
            response.headers = Headers::new();
            response.body = Vec::new();
        }
    }
    response
}

fn keep_headers(headers: &Headers, keep: impl Fn(&str) -> bool) -> Headers {
    headers
        .iter()
        .filter(|(name, _)| !FORBIDDEN_RESPONSE_HEADERS.contains(name) && keep(name))
        .collect()
}

/// Returns true if `url` has a username or a password.
pub(crate) fn has_credentials(url: &Url) -> bool {
    !url.username().is_empty() || url.password().is_some()
}

/// The console message of a CORS failure, in Chromium's words. `api` is
/// `fetch` or `XMLHttpRequest`; `redirected_from` is the first URL of a
/// redirect chain; `preflight` is true if the failed check was that of the
/// preflight response.
pub fn console_message(
    api: &str,
    url: &Url,
    redirected_from: Option<&Url>,
    origin: &str,
    preflight: bool,
    failure: &CorsFailure,
) -> String {
    let from = redirected_from.map_or(String::new(), |u| format!(" (redirected from '{u}')"));
    let prefix = if preflight && failure.is_preflight_check() {
        "Response to preflight request doesn't pass access control check: "
    } else {
        ""
    };
    format!(
        "Access to {api} at '{url}'{from} from origin '{origin}' has been blocked by CORS policy: {prefix}{}",
        failure.reason()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::Destination;

    fn request(method: Method, credentials: CredentialsMode) -> Request {
        let url = Url::parse("https://api.test/data").unwrap();
        let mut request = Request::get(url, Destination::Other)
            .with_initiator(Some(Url::parse("https://app.test/").unwrap().origin()));
        request.method = method;
        request.mode = RequestMode::Cors;
        request.tainting = Tainting::Cors;
        request.credentials = credentials;
        request
    }

    fn response(status: u16, headers: &[(&str, &str)]) -> Response {
        Response {
            url: Url::parse("https://api.test/data").unwrap(),
            status,
            headers: headers.iter().copied().collect(),
            body: b"x".to_vec(),
            redirected: false,
        }
    }

    #[test]
    fn cors_check_allow_origin() {
        let r = request(Method::Get, CredentialsMode::SameOrigin);
        assert_eq!(
            cors_check(&r, &response(200, &[])),
            Err(CorsFailure::MissingAllowOrigin)
        );
        assert!(cors_check(&r, &response(200, &[("Access-Control-Allow-Origin", "*")])).is_ok());
        assert!(
            cors_check(
                &r,
                &response(200, &[("Access-Control-Allow-Origin", "https://app.test")])
            )
            .is_ok()
        );
        assert_eq!(
            cors_check(
                &r,
                &response(
                    200,
                    &[("Access-Control-Allow-Origin", "https://other.test")]
                )
            ),
            Err(CorsFailure::OriginMismatch("https://other.test".into()))
        );
        // A trailing slash or a different case is not the origin.
        assert!(
            cors_check(
                &r,
                &response(200, &[("Access-Control-Allow-Origin", "https://app.test/")])
            )
            .is_err()
        );
        // Two values never pass.
        let two = response(
            200,
            &[
                ("Access-Control-Allow-Origin", "https://app.test"),
                ("Access-Control-Allow-Origin", "https://app.test"),
            ],
        );
        assert!(matches!(
            cors_check(&r, &two),
            Err(CorsFailure::MultipleAllowOrigin(_))
        ));
    }

    #[test]
    fn cors_check_credentials() {
        let r = request(Method::Get, CredentialsMode::Include);
        let star = response(200, &[("Access-Control-Allow-Origin", "*")]);
        assert_eq!(
            cors_check(&r, &star),
            Err(CorsFailure::WildcardWithCredentials)
        );
        let origin = [("Access-Control-Allow-Origin", "https://app.test")];
        assert_eq!(
            cors_check(&r, &response(200, &origin)),
            Err(CorsFailure::CredentialsNotAllowed(String::new()))
        );
        let yes = [origin[0], ("Access-Control-Allow-Credentials", "true")];
        assert!(cors_check(&r, &response(200, &yes)).is_ok());
        let no = [origin[0], ("Access-Control-Allow-Credentials", "True")];
        assert!(cors_check(&r, &response(200, &no)).is_err());
    }

    #[test]
    fn tainted_origin_is_null() {
        let mut r = request(Method::Get, CredentialsMode::SameOrigin);
        r.tainted_origin = true;
        let app = response(200, &[("Access-Control-Allow-Origin", "https://app.test")]);
        assert!(cors_check(&r, &app).is_err());
        let null = response(200, &[("Access-Control-Allow-Origin", "null")]);
        assert!(cors_check(&r, &null).is_ok());
    }

    #[test]
    fn safelisted_headers() {
        assert!(is_cors_safelisted_header("Accept", "text/html"));
        assert!(!is_cors_safelisted_header("Accept", "a\"b"));
        assert!(is_cors_safelisted_header(
            "Content-Type",
            "text/plain;charset=UTF-8"
        ));
        assert!(is_cors_safelisted_header(
            "content-type",
            "multipart/form-data; boundary=x"
        ));
        assert!(!is_cors_safelisted_header(
            "Content-Type",
            "application/json"
        ));
        assert!(!is_cors_safelisted_header("Content-Type", "nonsense"));
        assert!(is_cors_safelisted_header(
            "Accept-Language",
            "en-US,en;q=0.9"
        ));
        assert!(!is_cors_safelisted_header("Accept-Language", "en_US"));
        assert!(is_cors_safelisted_header("Range", "bytes=0-"));
        assert!(is_cors_safelisted_header("Range", "bytes=5-10"));
        assert!(!is_cors_safelisted_header("Range", "bytes=10-5"));
        assert!(!is_cors_safelisted_header("Range", "bytes=-5"));
        assert!(!is_cors_safelisted_header("X-Custom", "1"));
        assert!(!is_cors_safelisted_header("Accept", &"a".repeat(129)));
    }

    #[test]
    fn unsafe_names_are_sorted_lowercase_and_limited() {
        let headers: Headers = [
            ("X-B", "1"),
            ("Content-Type", "application/json"),
            ("x-a", "2"),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            unsafe_request_header_names(&headers),
            ["content-type", "x-a", "x-b"]
        );
        // Safelisted headers with more than 1024 bytes of values together
        // are unsafe.
        let long = "a".repeat(120);
        let many: Headers = (0..10).map(|_| ("accept", long.clone())).collect();
        assert_eq!(unsafe_request_header_names(&many), ["accept"]);
        let few: Headers = (0..3).map(|_| ("accept", long.clone())).collect();
        assert_eq!(unsafe_request_header_names(&few).len(), 0);
    }

    #[test]
    fn preflight_is_needed_for_methods_and_headers() {
        assert!(!needs_preflight(&request(
            Method::Get,
            CredentialsMode::Omit
        )));
        assert!(!needs_preflight(&request(
            Method::Post,
            CredentialsMode::Omit
        )));
        assert!(needs_preflight(&request(
            Method::Put,
            CredentialsMode::Omit
        )));
        assert!(needs_preflight(&request(
            Method::Other("patch".into()),
            CredentialsMode::Omit
        )));
        let mut r = request(Method::Get, CredentialsMode::Omit);
        r.headers.append("X-Token", "1");
        assert!(needs_preflight(&r));
    }

    #[test]
    fn preflight_request_shape() {
        let mut r = request(Method::Put, CredentialsMode::Include);
        r.headers.append("X-B", "1");
        r.headers.append("Content-Type", "application/json");
        let p = preflight_request(&r);
        assert_eq!(p.method, Method::Options);
        assert_eq!(p.credentials, CredentialsMode::Omit);
        assert_eq!(p.headers.get("access-control-request-method"), Some("PUT"));
        assert_eq!(
            p.headers.get("access-control-request-headers"),
            Some("content-type,x-b")
        );
        assert_eq!(p.origin_header().as_deref(), Some("https://app.test"));
        assert!(p.body.is_none());
    }

    #[test]
    fn preflight_answers() {
        let mut r = request(Method::Put, CredentialsMode::SameOrigin);
        r.headers.append("X-Token", "1");
        let allow = [("Access-Control-Allow-Origin", "*")];
        let answer = |extra: &[(&str, &str)], status| {
            let mut h = allow.to_vec();
            h.extend_from_slice(extra);
            check_preflight(&r, &response(status, &h))
        };
        assert_eq!(
            answer(&[], 200),
            Err(CorsFailure::MethodNotAllowed("PUT".into()))
        );
        let methods = ("Access-Control-Allow-Methods", "GET, PUT");
        assert_eq!(
            answer(&[methods], 200),
            Err(CorsFailure::HeaderNotAllowed("x-token".into()))
        );
        let headers = ("Access-Control-Allow-Headers", "X-Token");
        let ok = answer(
            &[methods, headers, ("Access-Control-Max-Age", "99999")],
            204,
        )
        .unwrap();
        assert_eq!(ok.max_age, MAX_AGE_LIMIT);
        assert_eq!(ok.methods, ["GET", "PUT"]);
        assert_eq!(ok.headers, ["x-token"]);
        assert_eq!(
            answer(&[methods, headers], 404),
            Err(CorsFailure::PreflightStatus)
        );
        assert_eq!(
            answer(&[methods, headers], 302),
            Err(CorsFailure::PreflightRedirect)
        );
        // Wildcards work without credentials only.
        let wild = [
            ("Access-Control-Allow-Methods", "*"),
            ("Access-Control-Allow-Headers", "*"),
        ];
        assert!(answer(&wild, 200).is_ok());
        let mut with = r.clone();
        with.credentials = CredentialsMode::Include;
        let h = [
            ("Access-Control-Allow-Origin", "https://app.test"),
            ("Access-Control-Allow-Credentials", "true"),
            wild[0],
            wild[1],
        ];
        assert!(check_preflight(&with, &response(200, &h)).is_err());
    }

    #[test]
    fn the_wildcard_covers_authorization_as_in_chromium() {
        let mut r = request(Method::Get, CredentialsMode::Omit);
        r.headers.append("Authorization", "x");
        let h = [
            ("Access-Control-Allow-Origin", "*"),
            ("Access-Control-Allow-Headers", "*"),
        ];
        assert!(check_preflight(&r, &response(200, &h)).is_ok());
    }

    #[test]
    fn cache_allows_what_was_preflighted() {
        let cache = PreflightCache::new();
        let mut r = request(Method::Put, CredentialsMode::Omit);
        r.headers.append("X-Token", "1");
        assert!(!cache.allows(&r));
        cache.store(
            &r,
            PreflightAnswer {
                methods: vec!["PUT".into()],
                headers: vec!["x-token".into()],
                max_age: 60,
            },
        );
        assert!(cache.allows(&r));
        let mut other = r.clone();
        other.headers.append("X-Other", "1");
        assert!(!cache.allows(&other));
        let mut credentialed = r.clone();
        credentialed.credentials = CredentialsMode::Include;
        assert!(!cache.allows(&credentialed));
        let mut delete = r.clone();
        delete.method = Method::Delete;
        assert!(!cache.allows(&delete));
        // Max-Age 0 stores nothing.
        let empty = PreflightCache::new();
        empty.store(
            &r,
            PreflightAnswer {
                methods: vec!["PUT".into()],
                headers: vec!["x-token".into()],
                max_age: 0,
            },
        );
        assert!(empty.is_empty());
    }

    #[test]
    fn cache_is_bounded() {
        let cache = PreflightCache::new();
        for i in 0..PREFLIGHT_CACHE_ENTRIES + 50 {
            let mut r = request(Method::Put, CredentialsMode::Omit);
            r.url = Url::parse(&format!("https://api.test/{i}")).unwrap();
            cache.store(
                &r,
                PreflightAnswer {
                    methods: vec!["PUT".into()],
                    headers: vec![],
                    max_age: 60,
                },
            );
        }
        assert_eq!(cache.len(), PREFLIGHT_CACHE_ENTRIES);
    }

    #[test]
    fn filtering() {
        let r = request(Method::Get, CredentialsMode::SameOrigin);
        let full = response(
            200,
            &[
                ("Content-Type", "text/plain"),
                ("X-Secret", "1"),
                ("X-Shown", "2"),
                ("Set-Cookie", "a=1"),
                ("Access-Control-Expose-Headers", "x-shown"),
            ],
        );
        let cors = filter_response(&r, full.clone(), ResponseKind::Cors);
        assert_eq!(
            cors.headers.iter().map(|(n, _)| n).collect::<Vec<_>>(),
            ["content-type", "x-shown"]
        );
        let basic = filter_response(&r, full.clone(), ResponseKind::Basic);
        assert!(!basic.headers.contains("set-cookie"));
        assert!(basic.headers.contains("x-secret"));
        let opaque = filter_response(&r, full.clone(), ResponseKind::Opaque);
        assert_eq!(
            (opaque.status, opaque.headers.len(), opaque.body.len()),
            (0, 0, 0)
        );
        // `*` exposes all except with credentials.
        let star = response(200, &[("X-A", "1"), ("Access-Control-Expose-Headers", "*")]);
        let all = filter_response(&r, star.clone(), ResponseKind::Cors);
        assert!(all.headers.contains("x-a"));
        let with = request(Method::Get, CredentialsMode::Include);
        let none = filter_response(&with, star, ResponseKind::Cors);
        assert!(!none.headers.contains("x-a"));
    }

    #[test]
    fn console_texts() {
        let r = request(Method::Get, CredentialsMode::SameOrigin);
        let text = console_message(
            "fetch",
            &r.url,
            None,
            "https://app.test",
            false,
            &CorsFailure::MissingAllowOrigin,
        );
        assert_eq!(
            text,
            "Access to fetch at 'https://api.test/data' from origin 'https://app.test' has been blocked by CORS policy: No 'Access-Control-Allow-Origin' header is present on the requested resource."
        );
        let preflight = console_message(
            "fetch",
            &r.url,
            None,
            "https://app.test",
            true,
            &CorsFailure::MissingAllowOrigin,
        );
        assert!(preflight.ends_with(
            "blocked by CORS policy: Response to preflight request doesn't pass access control check: No 'Access-Control-Allow-Origin' header is present on the requested resource."
        ));
    }
}
