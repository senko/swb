//! The [`Fetcher`] trait and redirect handling.
//!
//! Redirects follow <https://fetch.spec.whatwg.org/#http-redirect-fetch>.

use std::sync::Arc;

use log::debug;
use url::Url;

use crate::cookies::CookieJar;
use crate::error::NetError;
use crate::request::{Method, Request};
use crate::response::Response;

/// The maximum number of redirects for one request.
///
/// <https://fetch.spec.whatwg.org/#http-redirect-fetch>, step 8.
pub const MAX_REDIRECTS: usize = 20;

/// Request headers that describe the body. They are removed when a redirect
/// changes the method to `GET`.
///
/// <https://fetch.spec.whatwg.org/#request-body-header-name>
const REQUEST_BODY_HEADER_NAMES: [&str; 4] = [
    "content-encoding",
    "content-language",
    "content-location",
    "content-type",
];

/// Performs one request/response exchange.
///
/// A fetcher does not follow redirects: it returns 3xx responses as they
/// are. Use [`fetch_following_redirects`] for that. HTTP error statuses are
/// responses, not errors. The returned [`Response::url`] is the request URL.
pub trait Fetcher: Send + Sync {
    /// Fetches the resource for `request`.
    fn fetch(&self, request: &Request) -> Result<Response, NetError>;

    /// Returns the cookie jar that the fetcher uses, if it has one. A
    /// fetcher that wraps another one returns the jar of the inner fetcher.
    fn cookie_jar(&self) -> Option<&CookieJar> {
        None
    }
}

impl<F: Fetcher + ?Sized> Fetcher for Arc<F> {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        (**self).fetch(request)
    }

    fn cookie_jar(&self) -> Option<&CookieJar> {
        (**self).cookie_jar()
    }
}

impl<F: Fetcher + ?Sized> Fetcher for Box<F> {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        (**self).fetch(request)
    }

    fn cookie_jar(&self) -> Option<&CookieJar> {
        (**self).cookie_jar()
    }
}

/// Fetches `request` and follows redirects.
///
/// - 301 and 302 change a `POST` to a `GET` without a body. 303 changes any
///   method to `GET`. 307 and 308 keep the method and the body.
/// - The `Location` header is resolved against the current URL. If it has
///   no fragment, it gets the fragment of the current URL.
/// - A redirect to a scheme other than `http:` or `https:` is an error.
/// - The `Authorization` header is removed on a cross-origin redirect.
/// - At most [`MAX_REDIRECTS`] redirects are followed.
/// - A redirect status without a `Location` header is returned as the
///   response.
///
/// [`Response::url`] of the result is the final URL.
///
/// <https://fetch.spec.whatwg.org/#http-redirect-fetch>
pub fn fetch_following_redirects(
    fetcher: &dyn Fetcher,
    mut request: Request,
) -> Result<Response, NetError> {
    let start_url = request.url.clone();
    let mut redirect_count = 0;
    loop {
        let response = fetcher.fetch(&request)?;
        let Some(location) = location_url(&request.url, &response)? else {
            return Ok(response);
        };
        if redirect_count == MAX_REDIRECTS {
            return Err(NetError::TooManyRedirects(start_url));
        }
        redirect_count += 1;
        debug!("redirect {} {} -> {location}", response.status, request.url);
        redirect_request(&mut request, response.status, location);
    }
}

/// Returns the redirect target of `response`, or `None` if it is not a
/// redirect.
///
/// <https://fetch.spec.whatwg.org/#concept-response-location-url>
fn location_url(current_url: &Url, response: &Response) -> Result<Option<Url>, NetError> {
    if !response.is_redirect() {
        return Ok(None);
    }
    let Some(location) = response.headers.get("location") else {
        return Ok(None);
    };
    let invalid = || NetError::InvalidRedirect {
        url: current_url.clone(),
        location: location.to_string(),
    };
    // Fetch's "extract header list values" fails for a repeated
    // single-value header; Chromium rejects differing Location values.
    if response.headers.get_all("location").any(|l| l != location) {
        return Err(invalid());
    }
    let mut url = current_url.join(location).map_err(|_| invalid())?;
    if url.fragment().is_none() {
        url.set_fragment(current_url.fragment());
    }
    if !matches!(url.scheme(), "http" | "https") {
        return Err(invalid());
    }
    Ok(Some(url))
}

/// Changes `request` so that it fetches `location` after a redirect with
/// `status`.
fn redirect_request(request: &mut Request, status: u16, location: Url) {
    let to_get = (matches!(status, 301 | 302) && request.method == Method::Post)
        || (status == 303 && request.method != Method::Get);
    if to_get {
        request.method = Method::Get;
        request.body = None;
        for name in REQUEST_BODY_HEADER_NAMES {
            request.headers.remove(name);
        }
    }
    if request.url.origin() != location.origin() {
        request.headers.remove("authorization");
    }
    request.url = location;
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;
    use crate::headers::Headers;
    use crate::request::Destination;

    /// Answers from a map of URL to (status, location). Records requests.
    #[derive(Default)]
    struct MockFetcher {
        routes: HashMap<String, (u16, Option<String>)>,
        log: Mutex<Vec<Request>>,
    }

    impl MockFetcher {
        fn route(mut self, url: &str, status: u16, location: Option<&str>) -> Self {
            self.routes
                .insert(url.to_string(), (status, location.map(str::to_string)));
            self
        }

        fn log(&self) -> Vec<Request> {
            self.log.lock().unwrap().clone()
        }
    }

    impl Fetcher for MockFetcher {
        fn fetch(&self, request: &Request) -> Result<Response, NetError> {
            self.log.lock().unwrap().push(request.clone());
            let mut key = request.url.clone();
            key.set_fragment(None);
            let (status, location) = self
                .routes
                .get(key.as_str())
                .cloned()
                .unwrap_or((404, None));
            let mut headers = Headers::new();
            if let Some(location) = location {
                headers.append("Location", location);
            }
            Ok(Response {
                url: request.url.clone(),
                status,
                headers,
                body: Vec::new(),
            })
        }
    }

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    fn post(target: &str) -> Request {
        let mut request = Request::post(
            url(target),
            b"a=1".to_vec(),
            "application/x-www-form-urlencoded",
            Destination::Document,
        );
        request.headers.append("Authorization", "Basic xyz");
        request
    }

    #[test]
    fn differing_location_headers_are_an_error() {
        let response = |locations: &[&str]| {
            let mut headers = Headers::new();
            for location in locations {
                headers.append("Location", *location);
            }
            Response {
                url: url("https://a.test/"),
                status: 302,
                headers,
                body: Vec::new(),
            }
        };
        let current = url("https://a.test/");
        assert!(location_url(&current, &response(&["/a", "/b"])).is_err());
        let same = location_url(&current, &response(&["/a", "/a"])).unwrap();
        assert_eq!(same.unwrap().as_str(), "https://a.test/a");
    }

    #[test]
    fn no_redirect() {
        let fetcher = MockFetcher::default().route("https://a.test/", 200, None);
        let response = fetch_following_redirects(
            &fetcher,
            Request::get(url("https://a.test/"), Destination::Document),
        )
        .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(fetcher.log().len(), 1);
    }

    #[test]
    fn relative_location_is_resolved() {
        let fetcher = MockFetcher::default()
            .route("https://a.test/dir/page", 301, Some("../other?q=1"))
            .route("https://a.test/other?q=1", 200, None);
        let response = fetch_following_redirects(
            &fetcher,
            Request::get(url("https://a.test/dir/page"), Destination::Document),
        )
        .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.url.as_str(), "https://a.test/other?q=1");
    }

    #[test]
    fn fragment_is_kept_unless_location_has_one() {
        let fetcher = MockFetcher::default()
            .route("https://a.test/1", 302, Some("/2"))
            .route("https://a.test/2", 302, Some("/3#new"))
            .route("https://a.test/3", 200, None);
        let response = fetch_following_redirects(
            &fetcher,
            Request::get(url("https://a.test/1#old"), Destination::Document),
        )
        .unwrap();
        let urls: Vec<String> = fetcher.log().iter().map(|r| r.url.to_string()).collect();
        assert_eq!(
            urls,
            [
                "https://a.test/1#old",
                "https://a.test/2#old",
                "https://a.test/3#new"
            ]
        );
        assert_eq!(response.url.as_str(), "https://a.test/3#new");
    }

    #[test]
    fn post_becomes_get_on_301_302_303() {
        for status in [301, 302, 303] {
            let fetcher = MockFetcher::default()
                .route("https://a.test/form", status, Some("/done"))
                .route("https://a.test/done", 200, None);
            fetch_following_redirects(&fetcher, post("https://a.test/form")).unwrap();
            let log = fetcher.log();
            let second = &log[1];
            assert_eq!(second.method, Method::Get, "status {status}");
            assert_eq!(second.body, None);
            assert!(!second.headers.contains("content-type"));
            // Same origin: the Authorization header stays.
            assert!(second.headers.contains("authorization"));
        }
    }

    #[test]
    fn get_stays_get_on_301_302() {
        let fetcher = MockFetcher::default()
            .route("https://a.test/1", 301, Some("/2"))
            .route("https://a.test/2", 200, None);
        let mut request = Request::get(url("https://a.test/1"), Destination::Style);
        request.headers.append("Content-Type", "text/plain");
        fetch_following_redirects(&fetcher, request).unwrap();
        let second = &fetcher.log()[1];
        assert_eq!(second.method, Method::Get);
        // The method did not change, so the headers stay.
        assert!(second.headers.contains("content-type"));
        assert_eq!(second.destination, Destination::Style);
    }

    #[test]
    fn post_stays_post_on_307_308() {
        for status in [307, 308] {
            let fetcher = MockFetcher::default()
                .route("https://a.test/form", status, Some("https://b.test/form"))
                .route("https://b.test/form", 200, None);
            fetch_following_redirects(&fetcher, post("https://a.test/form")).unwrap();
            let second = &fetcher.log()[1];
            assert_eq!(second.method, Method::Post);
            assert_eq!(second.body.as_deref(), Some(&b"a=1"[..]));
            assert!(second.headers.contains("content-type"));
            // Cross origin: the Authorization header is removed.
            assert!(!second.headers.contains("authorization"));
        }
    }

    #[test]
    fn redirects_keep_the_initiator_and_destination() {
        let fetcher = MockFetcher::default()
            .route("https://a.test/1", 302, Some("https://b.test/2"))
            .route("https://b.test/2", 200, None);
        let initiator = Some(url("https://a.test/").origin());
        let request = Request::get(url("https://a.test/1"), Destination::Image)
            .with_initiator(initiator.clone());
        fetch_following_redirects(&fetcher, request).unwrap();
        let second = &fetcher.log()[1];
        assert_eq!(second.url.as_str(), "https://b.test/2");
        assert_eq!(second.initiator, initiator);
        assert_eq!(second.destination, Destination::Image);
    }

    #[test]
    fn redirect_without_location_is_returned() {
        let fetcher = MockFetcher::default().route("https://a.test/", 302, None);
        let response = fetch_following_redirects(
            &fetcher,
            Request::get(url("https://a.test/"), Destination::Document),
        )
        .unwrap();
        assert_eq!(response.status, 302);
    }

    #[test]
    fn redirect_to_other_scheme_fails() {
        let fetcher = MockFetcher::default().route("https://a.test/", 302, Some("data:,x"));
        let result = fetch_following_redirects(
            &fetcher,
            Request::get(url("https://a.test/"), Destination::Document),
        );
        assert!(matches!(result, Err(NetError::InvalidRedirect { .. })));
    }

    #[test]
    fn redirect_to_invalid_url_fails() {
        let fetcher = MockFetcher::default().route("https://a.test/", 302, Some("http://[::1"));
        let result = fetch_following_redirects(
            &fetcher,
            Request::get(url("https://a.test/"), Destination::Document),
        );
        assert!(matches!(result, Err(NetError::InvalidRedirect { .. })));
    }

    #[test]
    fn redirect_loop_stops() {
        let fetcher = MockFetcher::default()
            .route("https://a.test/a", 302, Some("/b"))
            .route("https://a.test/b", 302, Some("/a"));
        let result = fetch_following_redirects(
            &fetcher,
            Request::get(url("https://a.test/a"), Destination::Document),
        );
        match result {
            Err(NetError::TooManyRedirects(start)) => {
                assert_eq!(start.as_str(), "https://a.test/a");
            }
            other => panic!("unexpected result: {other:?}"),
        }
        // The first request and MAX_REDIRECTS redirects.
        assert_eq!(fetcher.log().len(), MAX_REDIRECTS + 1);
    }

    #[test]
    fn exactly_max_redirects_is_allowed() {
        let mut fetcher = MockFetcher::default();
        for i in 0..MAX_REDIRECTS {
            let from = format!("https://a.test/{i}");
            let to = format!("/{}", i + 1);
            fetcher = fetcher.route(&from, 301, Some(&to));
        }
        let last = format!("https://a.test/{MAX_REDIRECTS}");
        fetcher = fetcher.route(&last, 200, None);
        let response = fetch_following_redirects(
            &fetcher,
            Request::get(url("https://a.test/0"), Destination::Document),
        )
        .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.url.as_str(), last);
    }
}
