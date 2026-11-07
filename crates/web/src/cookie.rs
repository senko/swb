//! `document.cookie` (HTML §3.1.5,
//! <https://html.spec.whatwg.org/multipage/dom.html#dom-document-cookie>;
//! ADR 0028 section 12): the getter and the setter over the page's cookie
//! jar, which the engine lends through [`crate::EngineHost::cookie_string`]
//! and [`crate::EngineHost::set_cookie`].
//!
//! - A document without a browsing context (`createHTMLDocument`,
//!   `DOMParser`, `new Document()`, template contents) returns the empty
//!   string and ignores writes (measured in Chromium).
//! - A cookie-averse document, one whose URL is not `http:` or `https:`
//!   (`file:`, `about:`), does the same. An `about:blank` or `about:srcdoc`
//!   frame document with its creator's origin has its creator's cookies
//!   (measured, case `scripted-about-frame-state`).
//! - A `data:` document throws `SecurityError` ("Cookies are disabled
//!   inside 'data:' URLs."), as Chromium does for the opaque origin of a
//!   `data:` URL. A frame document with an opaque origin (a sandbox without
//!   `allow-same-origin`) throws with the text of the sandbox (measured, case
//!   `agents-opaque-state`). The spec lets every opaque origin throw; a top-level
//!   `about:blank` does not in Chromium.

use crate::bindings::{self as b, Cx, Exception};
use crate::state::{engine, engine_mut};

/// The text of a `SecurityError` of a document in a sandbox without
/// `allow-same-origin` (measured).
pub(crate) const SANDBOXED: &str =
    "The document is sandboxed and lacks the 'allow-same-origin' flag.";

/// What a document may do with cookies.
enum Access {
    /// An `http:` or `https:` URL: the jar.
    Jar,
    /// A URL without cookies: reads give "" and writes do nothing.
    Averse,
    /// A URL whose origin is opaque in a way that throws.
    Denied(&'static str),
}

fn access(url: &str) -> Access {
    let scheme = url.split_once(':').map_or("", |(scheme, _)| scheme);
    if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") {
        Access::Jar
    } else if scheme.eq_ignore_ascii_case("data") {
        Access::Denied("Cookies are disabled inside 'data:' URLs.")
    } else {
        Access::Averse
    }
}

/// What the document may do with cookies: from its sandbox, then its URL (measured,
/// case `agents-data-state`).
fn access_of(host: &dyn crate::EngineHost) -> Access {
    if host.sandboxed_origin() {
        return Access::Denied(SANDBOXED);
    }
    match access(&host.storage_url()) {
        // An `about:` document of a `data:` document's origin: the text of a document
        // that is not itself a `data:` one (measured, case `domain-about-frames`).
        Access::Denied(_) if !host.url().starts_with("data:") => {
            Access::Denied(crate::storage::DENIED)
        }
        other => other,
    }
}

/// The getter. `is_main` is true for the document of the window.
pub(crate) fn read(cx: &mut Cx<'_>, is_main: bool) -> b::Result<String> {
    if !is_main {
        return Ok(String::new());
    }
    let host = engine(cx.rt)?;
    match access_of(host) {
        Access::Jar => Ok(host.cookie_string()),
        Access::Averse => Ok(String::new()),
        Access::Denied(message) => Err(Exception::security(message)),
    }
}

/// The setter.
pub(crate) fn write(cx: &mut Cx<'_>, is_main: bool, value: &str) -> b::Result<()> {
    if !is_main {
        return Ok(());
    }
    let host = engine_mut(cx.rt)?;
    match access_of(host) {
        Access::Jar => {
            host.set_cookie(value);
            Ok(())
        }
        Access::Averse => Ok(()),
        Access::Denied(message) => Err(Exception::security(message)),
    }
}

#[cfg(test)]
mod tests {
    use super::{Access, access};

    #[test]
    fn access_by_scheme() {
        assert!(matches!(access("http://a.test/"), Access::Jar));
        assert!(matches!(access("HTTPS://a.test/"), Access::Jar));
        assert!(matches!(access("file:///tmp/a.html"), Access::Averse));
        assert!(matches!(access("about:blank"), Access::Averse));
        assert!(matches!(access("data:text/html,x"), Access::Denied(_)));
        assert!(matches!(access(""), Access::Averse));
    }
}
