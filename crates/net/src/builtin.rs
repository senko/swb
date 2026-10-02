//! Schemes that need no I/O: `about:` and `data:`.
//!
//! Both [`NetworkFetcher`](crate::NetworkFetcher) and
//! [`ReplayFetcher`](crate::ReplayFetcher) use this module, so that `data:`
//! URLs and `about:blank` work the same way in tests.
//!
//! <https://fetch.spec.whatwg.org/#scheme-fetch>

use crate::data_url;
use crate::error::NetError;
use crate::headers::Headers;
use crate::request::Request;
use crate::response::Response;

/// Fetches `about:` and `data:` URLs. Returns `None` for other schemes.
pub(crate) fn fetch(request: &Request) -> Option<Result<Response, NetError>> {
    match request.url.scheme() {
        "about" => Some(fetch_about(request)),
        "data" => Some(fetch_data(request)),
        _ => None,
    }
}

/// Returns true if the URL is fetched by this module.
pub(crate) fn handles(url: &url::Url) -> bool {
    matches!(url.scheme(), "about" | "data")
}

/// `about:blank` is an empty HTML document. Other `about:` URLs are errors.
///
/// <https://url.spec.whatwg.org/#about-blank>
fn fetch_about(request: &Request) -> Result<Response, NetError> {
    if request.url.path() != "blank" {
        return Err(NetError::UnknownAboutUrl(request.url.clone()));
    }
    let mut headers = Headers::new();
    headers.append("content-type", "text/html;charset=utf-8");
    Ok(Response {
        url: request.url.clone(),
        status: 200,
        headers,
        body: Vec::new(),
    })
}

fn fetch_data(request: &Request) -> Result<Response, NetError> {
    let data = data_url::process(&request.url).ok_or(NetError::InvalidDataUrl)?;
    let mut headers = Headers::new();
    headers.append("content-type", data.mime_type.to_string());
    Ok(Response {
        url: request.url.clone(),
        status: 200,
        headers,
        body: data.body,
    })
}

#[cfg(test)]
mod tests {
    use url::Url;

    use super::*;
    use crate::request::Destination;

    fn get(url: &str) -> Option<Result<Response, NetError>> {
        fetch(&Request::get(
            Url::parse(url).unwrap(),
            Destination::Document,
        ))
    }

    #[test]
    fn about_blank() {
        for url in ["about:blank", "about:blank#x", "about:blank?x"] {
            let response = get(url).unwrap().unwrap();
            assert_eq!(response.status, 200);
            assert_eq!(response.body, b"");
            assert_eq!(response.content_type().unwrap().essence, "text/html");
            assert_eq!(response.url.as_str(), url);
        }
    }

    #[test]
    fn other_about_urls_fail() {
        assert!(matches!(
            get("about:config").unwrap(),
            Err(NetError::UnknownAboutUrl(_))
        ));
    }

    #[test]
    fn data_url() {
        let response = get("data:text/css;charset=utf-8,p%7Bcolor:red%7D")
            .unwrap()
            .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"p{color:red}");
        assert_eq!(
            response.headers.get("content-type"),
            Some("text/css;charset=utf-8")
        );
    }

    #[test]
    fn invalid_data_url_fails() {
        assert!(matches!(
            get("data:nocomma").unwrap(),
            Err(NetError::InvalidDataUrl)
        ));
    }

    #[test]
    fn other_schemes_are_not_handled() {
        assert!(get("https://example.com/").is_none());
        assert!(get("file:///tmp/").is_none());
    }
}
