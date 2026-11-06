//! `document.domain` and `window.originAgentCluster` (HTML "relaxing the
//! same-origin restriction",
//! <https://html.spec.whatwg.org/multipage/browsers.html#dom-document-domain>;
//! ADR 0031 section 5, M9 feature 1).
//!
//! The getter gives the host of the document's origin (the effective
//! domain), `""` for an opaque origin and for `file:` URLs. The setter runs
//! the checks of the standard and then does nothing: Chromium keys its agent
//! clusters by origin, so `originAgentCluster` is true for a document with a
//! tuple origin and the setter returns at step 6 ("if the surrounding
//! agent's agent cluster's is origin-keyed is true, then return"). Measured
//! in Chromium 148 (`tools/probes/web-small-gaps.json`, `web-frames.json`):
//!
//! | Value | Result |
//! |---|---|
//! | `""` | `SecurityError`: `'' is an empty domain.` |
//! | does not parse as a host (`a.b:80`, `http://a.b`) | `'v' could not be parsed properly.` |
//! | the effective domain (also in other case or with an IPv4 form) | returns |
//! | not a dot-boundary suffix of the effective domain, or any value for an IP address or an opaque origin | `'v' is not a suffix of 'host'.` (the value in the form of the URL host parser: `0.0.1` is `0.0.0.1`) |
//! | a suffix that is a top-level domain (`localhost`, `co.uk`) | `'localhost' is a top-level domain.` |
//! | another suffix | returns |
//!
//! A document without a browsing context (`createHTMLDocument`,
//! `DOMParser`) reads the window's domain and throws `A browsing context is
//! required to set a domain.` Departure: a value with a character that a
//! host cannot have (a space) gives the "could not be parsed" error;
//! Chromium percent-encodes it and gives the "not a suffix" error. Both are
//! a `SecurityError`.

use swb_net::Url;
use url::{Host, Origin};

use crate::bindings::{self as b, Cx, Exception};
use crate::state::engine;

/// The effective domain of the window's document (HTML "effective domain"
/// of the origin): the host of a tuple origin, else `None`. `file:` URLs
/// have an opaque origin here, as in Chromium's `document.domain`.
fn effective_domain(url: &str) -> Option<Host<String>> {
    match Url::parse(url).ok()?.origin() {
        Origin::Tuple(_, host, _) => Some(host),
        Origin::Opaque(_) => None,
    }
}

/// `document.domain`: the host of the document's origin, or `""`.
pub(crate) fn get(cx: &mut Cx<'_>) -> b::Result<String> {
    let host = engine(cx.rt)?;
    if host.sandboxed_origin() {
        return Ok(String::new());
    }
    let url = host.url();
    Ok(effective_domain(&url).map_or_else(String::new, |host| host.to_string()))
}

/// `window.originAgentCluster`: true for a document with a tuple origin, a `data:`
/// document and a sandboxed frame document; false for `file:` (Chromium's
/// origin-keyed agent clusters; measured, cases `agents-opaque-state`,
/// `agents-data-state`, `agents-file-state`).
pub(crate) fn origin_agent_cluster(cx: &mut Cx<'_>) -> b::Result<bool> {
    let host = engine(cx.rt)?;
    if host.sandboxed_origin() {
        return Ok(true);
    }
    let url = host.url();
    Ok(url.starts_with("data:") || effective_domain(&url).is_some())
}

fn security(message: String) -> Exception {
    Exception::dom("SecurityError", message)
}

/// The setter. `has_context` is false for a document without a browsing
/// context.
pub(crate) fn set(cx: &mut Cx<'_>, has_context: bool, value: &str) -> b::Result<()> {
    if !has_context {
        return Err(security(
            "A browsing context is required to set a domain.".to_owned(),
        ));
    }
    let host = engine(cx.rt)?;
    let domain = if host.sandboxed_origin() {
        None
    } else {
        effective_domain(&host.url())
    };
    check(domain.as_ref(), value)
}

/// The checks of the setter against the effective domain; `Ok` means that
/// the setter returns (it never changes the domain).
fn check(effective: Option<&Host<String>>, value: &str) -> b::Result<()> {
    if value.is_empty() {
        return Err(security("'' is an empty domain.".to_owned()));
    }
    let Ok(wanted) = Host::parse(value) else {
        return Err(security(format!("'{value}' could not be parsed properly.")));
    };
    let current = effective.map_or_else(String::new, ToString::to_string);
    let wanted_text = wanted.to_string();
    if effective == Some(&wanted) {
        return Ok(());
    }
    let is_suffix = matches!(effective, Some(Host::Domain(_)))
        && matches!(wanted, Host::Domain(_))
        && current.ends_with(&format!(".{wanted_text}"));
    if !is_suffix {
        return Err(security(format!(
            "'{wanted_text}' is not a suffix of '{current}'."
        )));
    }
    if swb_net::is_top_level_domain(&wanted_text) {
        return Err(security(format!("'{wanted_text}' is a top-level domain.")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(effective: &str, value: &str) -> String {
        let host = effective_domain(effective);
        match check(host.as_ref(), value) {
            Ok(()) => "ok".to_owned(),
            Err(Exception::Dom { message, .. }) => message.into_owned(),
            Err(other) => format!("{other:?}"),
        }
    }

    #[test]
    fn effective_domains() {
        assert_eq!(
            effective_domain("http://127.0.0.1:8000/x").map(|h| h.to_string()),
            Some("127.0.0.1".to_owned())
        );
        assert_eq!(
            effective_domain("https://www.Example.com/").map(|h| h.to_string()),
            Some("www.example.com".to_owned())
        );
        assert!(effective_domain("file:///probe/page.html").is_none());
        assert!(effective_domain("data:text/html,x").is_none());
        assert!(effective_domain("about:blank").is_none());
    }

    #[test]
    fn ip_address_values_from_chromium() {
        let page = "http://127.0.0.1:8000/";
        assert_eq!(message(page, "127.0.0.1"), "ok");
        assert_eq!(message(page, "127.0.0.1."), "ok");
        assert_eq!(
            message(page, "0.0.1"),
            "'0.0.0.1' is not a suffix of '127.0.0.1'."
        );
        assert_eq!(
            message(page, "localhost"),
            "'localhost' is not a suffix of '127.0.0.1'."
        );
        assert_eq!(message(page, ""), "'' is an empty domain.");
    }

    #[test]
    fn file_urls_have_an_empty_domain() {
        let page = "file:///probe/page.html";
        assert_eq!(message(page, ""), "'' is an empty domain.");
        assert_eq!(
            message(page, "example.com"),
            "'example.com' is not a suffix of ''."
        );
        assert_eq!(message(page, "."), "'.' is not a suffix of ''.");
    }

    #[test]
    fn suffixes_from_chromium() {
        let page = "http://x.a.localhost:8000/";
        assert_eq!(message(page, "a.localhost"), "ok");
        assert_eq!(message(page, "x.a.localhost"), "ok");
        assert_eq!(
            message(page, "localhost"),
            "'localhost' is a top-level domain."
        );
        assert_eq!(
            message(page, "b.localhost"),
            "'b.localhost' is not a suffix of 'x.a.localhost'."
        );
        assert_eq!(
            message(page, "xa.localhost"),
            "'xa.localhost' is not a suffix of 'x.a.localhost'."
        );
        assert_eq!(
            message(page, "X.A.LOCALHOST."),
            "'x.a.localhost.' is not a suffix of 'x.a.localhost'."
        );
        assert_eq!(
            message(page, "a.localhost:80"),
            "'a.localhost:80' could not be parsed properly."
        );
        assert_eq!(
            message(page, "http://a.localhost"),
            "'http://a.localhost' could not be parsed properly."
        );
        assert_eq!(
            message("https://www.example.co.uk/", "co.uk"),
            "'co.uk' is a top-level domain."
        );
        assert_eq!(message("https://www.example.co.uk/", "example.co.uk"), "ok");
    }
}
