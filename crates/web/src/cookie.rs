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
//!   (`file:`, `about:`), does the same.
//! - A `data:` document throws `SecurityError` ("Cookies are disabled
//!   inside 'data:' URLs."), as Chromium does for the opaque origin of a
//!   `data:` URL. The spec lets every opaque origin throw; swb's other
//!   documents with a non-HTTP URL are cookie-averse and cannot have
//!   another opaque origin (no sandbox, no frames).

use crate::bindings::{self as b, Cx, Exception};
use crate::state::{engine, engine_mut};

/// What a document may do with cookies, from its URL.
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

/// The getter. `is_main` is true for the document of the window.
pub(crate) fn read(cx: &mut Cx<'_>, is_main: bool) -> b::Result<String> {
    if !is_main {
        return Ok(String::new());
    }
    let host = engine(cx.rt)?;
    match access(&host.url()) {
        Access::Jar => Ok(host.cookie_string()),
        Access::Averse => Ok(String::new()),
        Access::Denied(message) => Err(Exception::dom("SecurityError", message)),
    }
}

/// The setter.
pub(crate) fn write(cx: &mut Cx<'_>, is_main: bool, value: &str) -> b::Result<()> {
    if !is_main {
        return Ok(());
    }
    let host = engine_mut(cx.rt)?;
    match access(&host.url()) {
        Access::Jar => {
            host.set_cookie(value);
            Ok(())
        }
        Access::Averse => Ok(()),
        Access::Denied(message) => Err(Exception::dom("SecurityError", message)),
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
