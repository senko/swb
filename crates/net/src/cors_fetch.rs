//! [`fetch_request`]: Fetch's "main fetch" and "HTTP fetch" over a
//! [`Fetcher`]: the response tainting, the CORS-preflight fetch, the CORS
//! check, and the redirects with the request, credentials and redirect
//! modes.
//!
//! - <https://fetch.spec.whatwg.org/#main-fetch>
//! - <https://fetch.spec.whatwg.org/#http-fetch>
//! - <https://fetch.spec.whatwg.org/#http-redirect-fetch>
//!
//! A request without an initiator, and a navigation, are same-origin
//! (tainting `Basic`): the function then follows redirects as
//! [`fetch_following_redirects`](crate::fetch_following_redirects) always
//! did.

use log::debug;

use crate::cors::{
    self, CorsFailure, PreflightCache, ResponseKind, check_preflight, cors_check, has_credentials,
    needs_preflight, preflight_request,
};
use crate::error::{CorsError, NetError};
use crate::fetch::{Fetcher, MAX_REDIRECTS, check_redirect_scheme, location_url, redirect_request};
use crate::request::{RedirectMode, Request, RequestMode, Tainting};
use crate::response::Response;

/// The result of [`fetch_request`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    /// The final response after redirects. With
    /// [`Request::exposed_to_script`] it is the filtered response of
    /// `kind`; otherwise the full response.
    pub response: Response,
    /// The kind of the response, as `Response.type` names it.
    pub kind: ResponseKind,
}

/// Fetches `request` with `fetcher`: tainting, preflight, CORS check and
/// redirects. HTTP error statuses are responses.
///
/// - A mode `same-origin` request to another origin is an error
///   ([`NetError::Blocked`]).
/// - A `cors` request to another origin sends a preflight first if the
///   method or a header is not CORS-safelisted and `cache` does not allow
///   it; the response must pass [`cors_check`] ([`NetError::Cors`]).
/// - A `no-cors` request to another origin gets an opaque response.
/// - Redirects: [`RedirectMode`]; at most [`MAX_REDIRECTS`]; the redirect
///   sets the tainted-origin flag as Fetch says.
///
/// <https://fetch.spec.whatwg.org/#main-fetch>
pub fn fetch_request(
    fetcher: &dyn Fetcher,
    mut request: Request,
    cache: &PreflightCache,
) -> Result<Fetched, NetError> {
    let start_url = request.url.clone();
    let mut redirect_count = 0;
    loop {
        set_tainting(&mut request)?;
        let redirected_from = (redirect_count > 0).then(|| start_url.clone());
        let cors_error = |failure, preflight| {
            NetError::Cors(Box::new(CorsError {
                url: request.url.clone(),
                redirected_from: redirected_from.clone(),
                origin: cors::serialized_origin(&request),
                preflight,
                failure,
            }))
        };
        if request.tainting == Tainting::Cors
            && needs_preflight(&request)
            && !cache.allows(&request)
        {
            let preflight = preflight_request(&request);
            debug!("preflight {} {}", request.method, request.url);
            let response = fetcher.fetch(&preflight)?;
            let answer = check_preflight(&request, &response)
                .map_err(|failure| cors_error(failure, true))?;
            cache.store(&request, answer);
        }
        let mut response = fetcher.fetch(&request)?;
        if request.tainting == Tainting::Cors {
            cors_check(&request, &response).map_err(|failure| cors_error(failure, false))?;
        }
        let Some(location) = location_url(&request.url, &response)? else {
            response.redirected = redirect_count > 0;
            return Ok(finish(&request, response, kind_of(request.tainting)));
        };
        match request.redirect {
            RedirectMode::Follow => check_redirect_scheme(&request.url, &response, &location)?,
            RedirectMode::Manual => {
                return Ok(finish(&request, response, ResponseKind::OpaqueRedirect));
            }
            RedirectMode::Error => {
                return Err(NetError::Blocked(format!(
                    "Fetch API cannot load {}. Redirects are not allowed when redirect mode is set to \"error\".",
                    request.url
                )));
            }
        }
        // Fetch compares the request's origin with the `Location`'s, not
        // the response tainting: a chain A -> B -> A with credentials in
        // the URL is allowed.
        if has_credentials(&location)
            && request.mode == RequestMode::Cors
            && request
                .initiator
                .as_ref()
                .is_none_or(|o| *o != location.origin() || !o.is_tuple())
        {
            return Err(cors_error(CorsFailure::RedirectCredentials, false));
        }
        if redirect_count == MAX_REDIRECTS {
            return Err(NetError::TooManyRedirects(start_url));
        }
        redirect_count += 1;
        debug!("redirect {} {} -> {location}", response.status, request.url);
        taint_origin(&mut request, &location);
        redirect_request(&mut request, response.status, location);
    }
}

fn kind_of(tainting: Tainting) -> ResponseKind {
    match tainting {
        Tainting::Basic => ResponseKind::Basic,
        Tainting::Cors => ResponseKind::Cors,
        Tainting::Opaque => ResponseKind::Opaque,
    }
}

/// Filters the response for a script, or returns it as it is.
fn finish(request: &Request, response: Response, kind: ResponseKind) -> Fetched {
    let response = if request.exposed_to_script {
        cors::filter_response(request, response, kind)
    } else {
        response
    };
    Fetched { response, kind }
}

/// Sets `request.tainting` for the current URL (main fetch, step "set
/// request's response tainting"), or fails for mode `same-origin`.
///
/// <https://fetch.spec.whatwg.org/#main-fetch>
fn set_tainting(request: &mut Request) -> Result<(), NetError> {
    let scheme = request.url.scheme();
    if request.initiator.is_none()
        || matches!(scheme, "data" | "about")
        || request.mode == RequestMode::Navigate
        || (request.is_same_origin() && request.tainting == Tainting::Basic)
    {
        request.tainting = Tainting::Basic;
        return Ok(());
    }
    request.tainting = match request.mode {
        RequestMode::SameOrigin => {
            let origin = request
                .initiator
                .as_ref()
                .map_or_else(|| "null".to_owned(), url::Origin::ascii_serialization);
            return Err(NetError::Blocked(format!(
                "Fetch API cannot load {}. Request mode is \"same-origin\" but the URL's origin is not same as the request origin {origin}.",
                request.url
            )));
        }
        RequestMode::NoCors => {
            if request.redirect != RedirectMode::Follow {
                return Err(NetError::Blocked(format!(
                    "Fetch API cannot load {}. Request mode is \"no-cors\" but the redirect mode is not \"follow\".",
                    request.url
                )));
            }
            Tainting::Opaque
        }
        RequestMode::Cors => Tainting::Cors,
        RequestMode::Navigate => Tainting::Basic,
    };
    Ok(())
}

/// Sets the tainted-origin flag when the redirect leaves a URL that is
/// cross-origin to the initiator for a third origin.
///
/// <https://fetch.spec.whatwg.org/#http-redirect-fetch>, step 14.
fn taint_origin(request: &mut Request, location: &url::Url) {
    if request.initiator.is_some()
        && !request.is_same_origin()
        && request.url.origin() != location.origin()
    {
        request.tainted_origin = true;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use url::Url;

    use super::*;
    use crate::headers::Headers;
    use crate::request::{CredentialsMode, Destination, Method};

    /// A tiny server: routes by URL (without fragment) and method
    /// (`OPTIONS` routes have the key `OPTIONS url`). Logs the requests.
    #[derive(Default)]
    struct Server {
        routes: HashMap<String, (u16, Vec<(String, String)>)>,
        log: Mutex<Vec<Request>>,
    }

    impl Server {
        fn route(mut self, key: &str, status: u16, headers: &[(&str, &str)]) -> Self {
            let headers = headers
                .iter()
                .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
                .collect();
            self.routes.insert(key.to_owned(), (status, headers));
            self
        }

        fn methods(&self) -> Vec<String> {
            self.log
                .lock()
                .unwrap()
                .iter()
                .map(|r| format!("{} {}", r.method, r.url))
                .collect()
        }
    }

    impl Fetcher for Server {
        fn fetch(&self, request: &Request) -> Result<Response, NetError> {
            self.log.lock().unwrap().push(request.clone());
            let key = if request.method == Method::Options {
                format!("OPTIONS {}", request.url)
            } else {
                request.url.to_string()
            };
            let (status, headers) = self.routes.get(&key).cloned().unwrap_or((404, vec![]));
            Ok(Response {
                url: request.url.clone(),
                status,
                headers: headers.into_iter().collect::<Headers>(),
                body: b"body".to_vec(),
                redirected: false,
            })
        }
    }

    fn script_request(url: &str) -> Request {
        let mut request = Request::get(Url::parse(url).unwrap(), Destination::Other)
            .with_initiator(Some(Url::parse("https://app.test/").unwrap().origin()));
        request.mode = RequestMode::Cors;
        request.credentials = CredentialsMode::SameOrigin;
        request.exposed_to_script = true;
        request
    }

    const ALLOW: (&str, &str) = ("access-control-allow-origin", "https://app.test");

    #[test]
    fn same_origin_is_basic_without_cors() {
        let server = Server::default().route("https://app.test/x", 200, &[("x-a", "1")]);
        let fetched = fetch_request(
            &server,
            script_request("https://app.test/x"),
            &PreflightCache::new(),
        )
        .unwrap();
        assert_eq!(fetched.kind, ResponseKind::Basic);
        assert_eq!(fetched.response.headers.get("x-a"), Some("1"));
        assert_eq!(server.methods(), ["GET https://app.test/x"]);
    }

    #[test]
    fn cross_origin_needs_the_check() {
        let server = Server::default()
            .route(
                "https://api.test/ok",
                200,
                &[ALLOW, ("x-hidden", "1"), ("content-type", "text/plain")],
            )
            .route("https://api.test/denied", 200, &[]);
        let cache = PreflightCache::new();
        let fetched =
            fetch_request(&server, script_request("https://api.test/ok"), &cache).unwrap();
        assert_eq!(fetched.kind, ResponseKind::Cors);
        assert!(fetched.response.headers.contains("content-type"));
        assert!(!fetched.response.headers.contains("x-hidden"));
        let error =
            fetch_request(&server, script_request("https://api.test/denied"), &cache).unwrap_err();
        assert!(
            matches!(&error, NetError::Cors(c) if !c.preflight),
            "{error:?}"
        );
        let origin = server.log.lock().unwrap()[0].origin_header();
        assert_eq!(origin.as_deref(), Some("https://app.test"));
    }

    #[test]
    fn no_cors_is_opaque() {
        let server = Server::default().route("https://api.test/x", 200, &[("x-a", "1")]);
        let mut request = script_request("https://api.test/x");
        request.mode = RequestMode::NoCors;
        let fetched = fetch_request(&server, request, &PreflightCache::new()).unwrap();
        assert_eq!(fetched.kind, ResponseKind::Opaque);
        assert_eq!(fetched.response.status, 0);
        assert_eq!(fetched.response.body.len(), 0);
        // The request had no Origin header (a simple GET without CORS).
        assert_eq!(server.log.lock().unwrap()[0].origin_header(), None);
    }

    #[test]
    fn same_origin_mode_refuses_other_origins() {
        let server = Server::default();
        let mut request = script_request("https://api.test/x");
        request.mode = RequestMode::SameOrigin;
        let error = fetch_request(&server, request, &PreflightCache::new()).unwrap_err();
        assert!(matches!(error, NetError::Blocked(_)));
        assert_eq!(server.methods().len(), 0);
    }

    #[test]
    fn data_urls_are_basic() {
        struct Data;
        impl Fetcher for Data {
            fn fetch(&self, request: &Request) -> Result<Response, NetError> {
                Ok(Response {
                    url: request.url.clone(),
                    status: 200,
                    headers: Headers::new(),
                    body: Vec::new(),
                    redirected: false,
                })
            }
        }
        let mut request = script_request("data:,x");
        request.mode = RequestMode::SameOrigin;
        let fetched = fetch_request(&Data, request, &PreflightCache::new()).unwrap();
        assert_eq!(fetched.kind, ResponseKind::Basic);
    }

    #[test]
    fn preflight_runs_once_and_is_cached() {
        let server = Server::default()
            .route(
                "OPTIONS https://api.test/p",
                204,
                &[
                    ALLOW,
                    ("access-control-allow-methods", "PUT"),
                    ("access-control-max-age", "60"),
                ],
            )
            .route("https://api.test/p", 200, &[ALLOW]);
        let cache = PreflightCache::new();
        for _ in 0..2 {
            let mut request = script_request("https://api.test/p");
            request.method = Method::Put;
            fetch_request(&server, request, &cache).unwrap();
        }
        assert_eq!(
            server.methods(),
            [
                "OPTIONS https://api.test/p",
                "PUT https://api.test/p",
                "PUT https://api.test/p"
            ]
        );
        let log = server.log.lock().unwrap();
        let preflight = &log[0];
        assert_eq!(preflight.credentials, CredentialsMode::Omit);
        assert_eq!(
            preflight.headers.get("access-control-request-method"),
            Some("PUT")
        );
    }

    #[test]
    fn failed_preflight_sends_no_request() {
        let server = Server::default().route("OPTIONS https://api.test/p", 200, &[ALLOW]);
        let mut request = script_request("https://api.test/p");
        request.method = Method::Delete;
        let error = fetch_request(&server, request, &PreflightCache::new()).unwrap_err();
        assert!(matches!(
            &error,
            NetError::Cors(c) if c.preflight && matches!(c.failure, CorsFailure::MethodNotAllowed(_))
        ));
        assert_eq!(server.methods(), ["OPTIONS https://api.test/p"]);
    }

    #[test]
    fn redirects_are_checked_and_taint_the_origin() {
        let server = Server::default()
            .route(
                "https://app.test/r",
                302,
                &[("location", "https://api.test/a")],
            )
            .route(
                "https://api.test/a",
                302,
                &[ALLOW, ("location", "https://third.test/b")],
            )
            .route(
                "https://third.test/b",
                200,
                &[("access-control-allow-origin", "null")],
            );
        let fetched = fetch_request(
            &server,
            script_request("https://app.test/r"),
            &PreflightCache::new(),
        )
        .unwrap();
        assert_eq!(fetched.kind, ResponseKind::Cors);
        assert!(fetched.response.redirected);
        let log = server.log.lock().unwrap();
        assert_eq!(log[0].origin_header(), None);
        assert_eq!(log[1].origin_header().as_deref(), Some("https://app.test"));
        // The second hop leaves a cross-origin URL for a third origin.
        assert_eq!(log[2].origin_header().as_deref(), Some("null"));
    }

    #[test]
    fn redirect_without_allow_origin_fails() {
        let server = Server::default()
            .route(
                "https://api.test/a",
                302,
                &[("location", "https://api.test/b")],
            )
            .route("https://api.test/b", 200, &[ALLOW]);
        let error = fetch_request(
            &server,
            script_request("https://api.test/a"),
            &PreflightCache::new(),
        )
        .unwrap_err();
        assert!(matches!(error, NetError::Cors(_)));
        // The redirect target was not requested.
        assert_eq!(server.methods(), ["GET https://api.test/a"]);
    }

    #[test]
    fn redirect_modes() {
        let server = Server::default()
            .route("https://app.test/a", 302, &[("location", "/b")])
            .route("https://app.test/b", 200, &[]);
        let mut request = script_request("https://app.test/a");
        request.redirect = RedirectMode::Error;
        let error = fetch_request(&server, request, &PreflightCache::new()).unwrap_err();
        assert!(matches!(error, NetError::Blocked(_)));
        let mut request = script_request("https://app.test/a");
        request.redirect = RedirectMode::Manual;
        let fetched = fetch_request(&server, request, &PreflightCache::new()).unwrap();
        assert_eq!(fetched.kind, ResponseKind::OpaqueRedirect);
        assert_eq!(fetched.response.status, 0);
        assert_eq!(server.methods().len(), 2);
    }

    #[test]
    fn manual_redirect_to_another_scheme_is_an_opaque_redirect() {
        // Review fix 4: the scheme check belongs to following the redirect.
        for target in ["ftp://x.test/f", "file:///etc/hostname"] {
            let server =
                Server::default().route("https://app.test/a", 302, &[("location", target)]);
            let mut request = script_request("https://app.test/a");
            request.redirect = RedirectMode::Manual;
            let fetched = fetch_request(&server, request, &PreflightCache::new()).unwrap();
            assert_eq!(fetched.kind, ResponseKind::OpaqueRedirect, "{target}");
            let request = script_request("https://app.test/a");
            let error = fetch_request(&server, request, &PreflightCache::new()).unwrap_err();
            assert!(
                matches!(error, NetError::InvalidRedirect { .. }),
                "{target}"
            );
        }
    }

    #[test]
    fn credentials_in_a_location_of_the_initiators_origin_are_allowed() {
        // Review fix 6: A -> B -> A@credentials compares the origins only.
        let server = Server::default()
            .route("https://app.test/a", 302, &[("location", "https://api.test/b")])
            .route(
                "https://api.test/b",
                302,
                &[ALLOW, ("location", "https://u:p@app.test/c")],
            )
            // The chain is tainted: the origin of the request is `null`.
            .route(
                "https://u:p@app.test/c",
                200,
                &[("access-control-allow-origin", "null")],
            );
        let fetched = fetch_request(
            &server,
            script_request("https://app.test/a"),
            &PreflightCache::new(),
        )
        .unwrap();
        assert_eq!(fetched.response.status, 200);
    }

    #[test]
    fn credentials_in_a_redirect_location_are_refused() {
        let server = Server::default().route(
            "https://api.test/a",
            302,
            &[ALLOW, ("location", "https://u:p@api.test/b")],
        );
        let error = fetch_request(
            &server,
            script_request("https://api.test/a"),
            &PreflightCache::new(),
        )
        .unwrap_err();
        assert!(matches!(
            &error,
            NetError::Cors(c) if c.failure == CorsFailure::RedirectCredentials
        ));
    }
}
