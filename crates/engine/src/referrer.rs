//! `document.referrer` of frame documents (Referrer Policy,
//! <https://w3c.github.io/webappsec-referrer-policy/>, default policy
//! `strict-origin-when-cross-origin`; ADR 0031 section 3 and "Done (feature 14b)").
//!
//! The initial `about:blank` document of a frame is no navigation: it has the URL of the
//! document that made it as that document has it now, with its fragment, also `file:`,
//! `about:blank` and `about:srcdoc` (measured, cases `referrer-about-blank*`).
//!
//! For a navigation, the source is the document that started it: the parent for `srcdoc`
//! and for a change of the `src` attribute (the container's node document, also when a
//! script of another frame sets it), the document itself for a link, a form, a write to
//! its own `location`, and a reload, and the document of the script for a write to the
//! `location` of another frame (measured, cases `referrer-*`). The value follows the target:
//!
//! - a target of the source's origin gets the URL of the source without fragment and
//!   credentials;
//! - an `http:` target that is not potentially trustworthy gets nothing from a source that
//!   is (the spec; the probe has no `https:` page);
//! - another target (a `data:` document too) gets the origin of the source and `/`;
//! - `srcdoc` and `about:blank` get the origin and `/` also when they are of the same origin;
//! - a source with an opaque origin (`data:`, `file:`, `about:blank` in a reload, a sandbox)
//!   gets nothing.
//!
//! swb sends no `Referer` header; only the property is set. The `referrerpolicy` attribute
//! and the `Referrer-Policy` header are not read (ADR 0031 section 13).

use swb_net::Url;

/// What the document that a navigation commits, and that gets the referrer, is.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Target<'a> {
    /// A document of a URL from the network (or `data:`).
    Url(&'a Url),
    /// An `about:blank` document.
    Blank,
    /// An `about:srcdoc` document.
    Srcdoc,
}

/// The `document.referrer` of a document of `target` navigated by the document at `source`
/// (the URL that gives it its origin; ADR 0031 section 3).
pub(crate) fn referrer(source: &Url, target: Target<'_>) -> String {
    let origin = source.origin();
    if !origin.is_tuple() {
        return String::new();
    }
    match target {
        Target::Url(url) if url.origin() == origin => {
            let mut url = without_credentials(source);
            url.set_fragment(None);
            url.to_string()
        }
        Target::Url(url) if swb_net::is_downgrade(source, url) => String::new(),
        _ => format!("{}/", origin.ascii_serialization()),
    }
}

/// The `document.referrer` of the initial `about:blank` document that the document at
/// `creator` makes for a frame of its own origin. A `data:` URL holds a document, and it
/// gives nothing (Chromium gives it: a departure, ADR 0031 "Done (feature 14b)").
pub(crate) fn initial_referrer(creator: &Url) -> String {
    match creator.scheme() {
        "http" | "https" | "file" | "about" => without_credentials(creator).to_string(),
        _ => String::new(),
    }
}

/// `url` without its credentials (Referrer Policy "strip url for use as a referrer").
fn without_credentials(url: &Url) -> Url {
    let mut url = url.clone();
    // A URL that cannot have credentials has none to remove.
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    #[test]
    fn same_origin_gets_the_stripped_url() {
        let source = url("http://user:pw@a.test:8000/p/q.html?x=1#frag");
        let target = url("http://a.test:8000/other");
        assert_eq!(
            referrer(&source, Target::Url(&target)),
            "http://a.test:8000/p/q.html?x=1"
        );
    }

    #[test]
    fn the_initial_document_gets_the_url_of_its_creator_with_the_fragment() {
        for (creator, want) in [
            (
                "http://user:pw@a.test:8000/p/q.html?x=1#frag",
                "http://a.test:8000/p/q.html?x=1#frag",
            ),
            ("https://a.test/#f", "https://a.test/#f"),
            ("file:///tmp/a.html#f", "file:///tmp/a.html#f"),
            ("about:blank", "about:blank"),
            ("about:srcdoc", "about:srcdoc"),
            ("data:text/html,secret", ""),
            ("blob:http://a.test/1", ""),
        ] {
            assert_eq!(initial_referrer(&url(creator)), want, "{creator}");
        }
    }

    #[test]
    fn a_navigation_to_about_blank_gets_the_origin_of_any_source() {
        for source in [
            "http://a.test:8000/p/q.html?secret=1#frag",
            "http://user:pw@a.test:8000/p/q.html",
        ] {
            assert_eq!(referrer(&url(source), Target::Blank), "http://a.test:8000/");
        }
        assert_eq!(referrer(&url("about:blank"), Target::Blank), "");
    }

    /// Referrer Policy, `strict-origin-when-cross-origin`: nothing for a downgrade.
    #[test]
    fn a_downgrade_gets_nothing() {
        let source = url("https://a.test/p?x=1");
        for (target, want) in [
            ("http://b.test/", ""),
            ("http://a.test/", ""),
            ("https://b.test/", "https://a.test/"),
            ("https://a.test/other", "https://a.test/p?x=1"),
            ("http://localhost:8000/", "https://a.test/"),
            ("http://b.localhost:8000/", "https://a.test/"),
            ("http://127.0.0.1:8000/", "https://a.test/"),
            ("data:text/html,x", "https://a.test/"),
        ] {
            assert_eq!(
                referrer(&source, Target::Url(&url(target))),
                want,
                "{target}"
            );
        }
        assert_eq!(referrer(&source, Target::Blank), "https://a.test/");
        assert_eq!(referrer(&source, Target::Srcdoc), "https://a.test/");
        assert_eq!(
            referrer(&url("http://a.test/p"), Target::Url(&url("http://b.test/"))),
            "http://a.test/"
        );
    }

    #[test]
    fn another_origin_gets_the_origin() {
        let source = url("http://a.test:8000/p/q.html?x=1#frag");
        for target in [
            "http://a.test:8001/",
            "https://a.test:8000/",
            "http://b.test:8000/",
        ] {
            assert_eq!(
                referrer(&source, Target::Url(&url(target))),
                "http://a.test:8000/"
            );
        }
        assert_eq!(
            referrer(&source, Target::Url(&url("data:text/html,x"))),
            "http://a.test:8000/"
        );
        assert_eq!(referrer(&source, Target::Srcdoc), "http://a.test:8000/");
    }

    #[test]
    fn an_opaque_source_gets_nothing() {
        let file = url("file:///tmp/a.html");
        assert_eq!(referrer(&file, Target::Srcdoc), "");
        assert_eq!(referrer(&file, Target::Blank), "");
        assert_eq!(referrer(&file, Target::Url(&url("data:text/html,x"))), "");
        let data = url("data:text/html,x");
        assert_eq!(referrer(&data, Target::Srcdoc), "");
        assert_eq!(referrer(&data, Target::Blank), "");
    }
}
