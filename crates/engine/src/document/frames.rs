//! What a document tells the page about its frames (ADR 0031 sections 2
//! and 4): the `iframe` elements that the journal drain saw inserted,
//! removed or changed ([`FrameChanges`]), the content boxes of the
//! `iframe` elements after a layout (the frames' viewports), and the
//! declarative refresh of a frame document (`<meta http-equiv=refresh>`).
//! The page keeps the frame tree (`crate::frames`) and acts on these.

use std::collections::HashMap;

use swb_dom::{Document, NodeId, is_html_whitespace, local_name};
use swb_layout::{BoxContent, FragmentRef, Size};
use swb_net::{Request, RequestId, Reservation, Url};

use super::DocumentState;

/// The changes of `iframe` elements since the page last took them, in the
/// order of the journal. A moved `iframe` is in both lists: its frame is
/// destroyed and made again (HTML "removing steps" and "insertion steps").
#[derive(Debug, Default)]
pub(crate) struct FrameChanges {
    /// `iframe` elements in inserted subtrees.
    pub(crate) inserted: Vec<NodeId>,
    /// `iframe` elements in removed subtrees.
    pub(crate) removed: Vec<NodeId>,
    /// `iframe` elements whose `src` or `srcdoc` was set (HTML "process
    /// the iframe attributes"), once per set. (`sandbox` applies at the
    /// next navigation; the page reads it then.)
    pub(crate) attributes: Vec<NodeId>,
    /// `iframe` elements whose `name` changed.
    pub(crate) names: Vec<NodeId>,
    /// True after a walk of the whole document (an incomplete journal):
    /// the page compares its frames with the `iframe` elements of the
    /// document.
    pub(crate) rescan: bool,
}

impl FrameChanges {
    /// True if nothing changed.
    pub(crate) fn is_empty(&self) -> bool {
        !self.rescan
            && self.inserted.is_empty()
            && self.removed.is_empty()
            && self.attributes.is_empty()
            && self.names.is_empty()
    }
}

/// A declarative refresh of a frame document (HTML "shared declarative
/// refresh steps"): the delay in seconds and the URL to load.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Refresh {
    pub(crate) delay_s: f64,
    pub(crate) url: Url,
}

/// The longest delay of a refresh that swb keeps (a day): a larger value
/// never acts in a session.
const MAX_REFRESH_DELAY_S: f64 = 86_400.0;

impl DocumentState {
    /// Takes the changes of `iframe` elements since the last call.
    pub(crate) fn take_frame_changes(&mut self) -> FrameChanges {
        std::mem::take(&mut self.frame_changes)
    }

    /// True if the page has `iframe` changes to take.
    pub(crate) fn has_frame_changes(&self) -> bool {
        !self.frame_changes.is_empty()
    }

    /// Starts the request of a subresource of the document: the initiator
    /// is the document's origin, the site for cookies the document's
    /// (frames), and the completion carries the document's client.
    pub(crate) fn start_subresource(&self, request: Request) -> RequestId {
        let mut request = request
            .with_initiator(Some(self.origin()))
            .with_site_for_cookies(self.site_for_cookies.clone());
        if let Some(budget) = &self.body_budget {
            // A frame document's bodies that the page has not taken yet
            // count against the page (a reservation of 0 bytes always fits;
            // the worker grows it by the body).
            request.reservation = Reservation::new(budget, 0);
        }
        self.loader.start_for(self.client, request)
    }

    /// The content box sizes of the `iframe` elements of the current
    /// layout (the viewports of their frames; ADR 0031 section 4). Empty
    /// without a layout.
    pub(crate) fn frame_boxes(&self) -> HashMap<NodeId, Size> {
        let mut boxes = HashMap::new();
        let Some(fragments) = &self.fragments else {
            return boxes;
        };
        fragments.walk(|fragment, _| {
            if let FragmentRef::Box(b) = fragment
                && let BoxContent::Frame(node) = b.content
            {
                let r = b.border_rect;
                let width =
                    r.width - b.border.left - b.border.right - b.padding.left - b.padding.right;
                let height =
                    r.height - b.border.top - b.border.bottom - b.padding.top - b.padding.bottom;
                // An element with several boxes (it cannot have them) keeps
                // the first.
                boxes
                    .entry(node)
                    .or_insert_with(|| Size::new(width.max(0.0), height.max(0.0)));
            }
        });
        boxes
    }

    /// The serial number of the current layout (it grows with each).
    pub(crate) fn layout_serial(&self) -> u64 {
        self.layout_serial
    }

    /// The declarative refresh of the document: the first `meta` element
    /// in tree order whose `http-equiv` is `refresh` (ASCII
    /// case-insensitive) with a `content` that parses. Only frame
    /// documents use it in swb (ADR 0031 section 2).
    pub(crate) fn declarative_refresh(&self) -> Option<Refresh> {
        let dom = &self.dom;
        dom.descendants(NodeId::DOCUMENT)
            .filter(|&n| dom.is_html_element(n, &local_name!("meta")))
            .filter_map(|n| dom.element(n))
            .filter(|e| {
                e.attr("http-equiv")
                    .is_some_and(|v| v.eq_ignore_ascii_case("refresh"))
            })
            .find_map(|e| e.attr("content"))
            .and_then(|content| parse_refresh(content, &self.url, self.base_url()))
    }
}

/// The `iframe` elements of the document tree, in tree order.
pub(crate) fn iframes_in(dom: &Document) -> Vec<NodeId> {
    dom.descendants(NodeId::DOCUMENT)
        .filter(|&n| dom.is_html_element(n, &local_name!("iframe")))
        .collect()
}

/// Parses the `content` of a refresh (HTML "shared declarative refresh
/// steps", steps 1 to 11): a non-negative number of seconds (digits and an
/// optional fraction, which is ignored), then optionally `;` or `,` and a
/// URL, which may start with `url=` (any case) and may be quoted. Without
/// a URL the document refreshes itself (`document_url`); the URL is parsed
/// against `base`.
/// <https://html.spec.whatwg.org/multipage/semantics.html#shared-declarative-refresh-steps>
pub(crate) fn parse_refresh(content: &str, document_url: &Url, base: &Url) -> Option<Refresh> {
    let rest = content.trim_start_matches(is_html_whitespace);
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    let after_digits = &rest[digits..];
    // Step 6: the integer part; a value with no digits is allowed only if a
    // fraction follows (".5" is 0 seconds).
    if digits == 0 && !after_digits.starts_with('.') {
        return None;
    }
    let delay_s = if digits == 0 {
        0.0
    } else {
        rest[..digits].parse::<f64>().unwrap_or(f64::MAX)
    };
    // Step 7: skip digits and dots.
    let after_number = after_digits.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.');
    let delay_s = delay_s.min(MAX_REFRESH_DELAY_S);
    let mut tail = after_number.trim_start_matches(is_html_whitespace);
    if tail.is_empty() {
        return Some(Refresh {
            delay_s,
            url: document_url.clone(),
        });
    }
    // Step 11: the separator is `;`, `,` or white space (measured: `0 s2.html`
    // navigates); anything else ends the parse.
    if !after_number.starts_with(|c: char| c == ';' || c == ',' || is_html_whitespace(c)) {
        return None;
    }
    tail = tail
        .strip_prefix([';', ','])
        .unwrap_or(tail)
        .trim_start_matches(is_html_whitespace);
    if tail.is_empty() {
        return Some(Refresh {
            delay_s,
            url: document_url.clone(),
        });
    }
    // `url=` (any case), with optional white space around `=`.
    if tail.get(..3).is_some_and(|p| p.eq_ignore_ascii_case("url"))
        && let Some(after) = tail.get(3..)
        && let Some(value) = after
            .trim_start_matches(is_html_whitespace)
            .strip_prefix('=')
    {
        tail = value.trim_start_matches(is_html_whitespace);
    }
    // A quoted URL ends at the matching quote.
    let url_text = match tail.chars().next() {
        Some(quote @ ('"' | '\'')) => {
            let inner = &tail[1..];
            inner.split(quote).next().unwrap_or(inner)
        }
        _ => tail.trim_end_matches(is_html_whitespace),
    };
    let url = base.join(url_text).ok()?;
    Some(Refresh { delay_s, url })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).expect("a valid URL")
    }

    #[test]
    fn refresh_contents() {
        let doc = url("http://a.test/dir/page.html");
        let parse = |s: &str| parse_refresh(s, &doc, &doc);
        let at = |delay_s: f64, u: &str| {
            Some(Refresh {
                delay_s,
                url: url(u),
            })
        };
        assert_eq!(parse("0"), at(0.0, "http://a.test/dir/page.html"));
        assert_eq!(parse(" 5 "), at(5.0, "http://a.test/dir/page.html"));
        assert_eq!(parse("3.7"), at(3.0, "http://a.test/dir/page.html"));
        assert_eq!(parse(".5"), at(0.0, "http://a.test/dir/page.html"));
        assert_eq!(
            parse("1; url=next.html"),
            at(1.0, "http://a.test/dir/next.html")
        );
        assert_eq!(
            parse("1,URL = 'q.html'x"),
            at(1.0, "http://a.test/dir/q.html")
        );
        assert_eq!(parse("0;\"/top\""), at(0.0, "http://a.test/top"));
        assert_eq!(parse("2;"), at(2.0, "http://a.test/dir/page.html"));
        assert_eq!(parse("x"), None);
        assert_eq!(parse(""), None);
        assert_eq!(parse("1 x"), at(1.0, "http://a.test/dir/x"));
        assert_eq!(parse("0 url=s2.html"), at(0.0, "http://a.test/dir/s2.html"));
        assert_eq!(parse("1x"), None);
        assert_eq!(
            parse("99999999999999999999999"),
            at(MAX_REFRESH_DELAY_S, "http://a.test/dir/page.html")
        );
    }
}
