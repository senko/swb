//! The resource selection algorithm (HTML,
//! <https://html.spec.whatwg.org/multipage/media.html#concept-media-load-algorithm>
//! and "resource selection algorithm"): which resource a media element
//! loads. The `src` attribute wins (children are never used when `src` is
//! there, also when it fails); else the `source` children, in order, whose
//! `type` the build can play (`canPlayType`) and whose `media` matches the
//! document's environment. A `source` that is skipped or fails gets an
//! `error` event; one without `src`, or with an empty one, is skipped
//! without an event (Chromium, measured: the specification fires one).

use swb_css::{MediaEnvironment, MediaQueryList};
use swb_dom::{Document, NodeId, is_html_whitespace, local_name};
use swb_net::{Origin, Url};

/// What the algorithm needs of the document.
pub(crate) struct Cx<'a> {
    pub(crate) dom: &'a Document,
    /// The document base URL: `src` resolves against it.
    pub(crate) base: &'a Url,
    /// The media queries' environment.
    pub(crate) env: MediaEnvironment,
    pub(crate) loader: &'a swb_net::Loader,
    /// The origin of the requests.
    pub(crate) initiator: Option<Origin>,
    pub(crate) site_for_cookies: Option<Origin>,
    /// The document is a `file:` document: it may load `file:` URLs.
    pub(crate) file_document: bool,
    /// The page time in milliseconds.
    pub(crate) now_ms: f64,
}

impl Cx<'_> {
    /// True if the document may load `url` (as for images).
    pub(crate) fn may_load(&self, url: &Url) -> bool {
        crate::page::is_loadable(url) && (url.scheme() != "file" || self.file_document)
    }
}

/// What the algorithm makes of a `source` element.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Candidate {
    /// Not a candidate; no event.
    Skip,
    /// Fails: an `error` event at the `source`.
    Fail,
    /// Load this resource.
    Use(Url),
}

/// The *pointer* of the resource selection algorithm in the children of a
/// media element: a position between two children, kept as the child
/// before it. Insertions at the pointer go after it; when the child before
/// it is removed, it moves to the child before that one, so it does not
/// move relative to the children that stay
/// (<https://html.spec.whatwg.org/multipage/media.html#concept-media-load-algorithm>,
/// "pointer"; [`super::Media::children_removed`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pointer {
    /// Before the first child.
    Start,
    /// After this child.
    After(NodeId),
}

/// The next `source` child of `parent` after `pointer` (from the first
/// child when the node before the pointer is not a child of `parent`).
/// Each call costs the children between the pointer and the result.
pub(super) fn next_source(dom: &Document, parent: NodeId, pointer: Pointer) -> Option<NodeId> {
    let first = match pointer {
        Pointer::After(node) if dom.parent(node) == Some(parent) => dom.next_sibling_element(node),
        _ => dom.first_element_child(parent),
    };
    std::iter::successors(first, |&child| dom.next_sibling_element(child))
        .find(|&child| dom.is_html_element(child, &local_name!("source")))
}

/// The value of the `src` attribute of `node`, stripped of white space.
pub(super) fn src_attribute(dom: &Document, node: NodeId) -> Option<String> {
    dom.element(node)
        .and_then(|e| e.attr("src"))
        .map(|s| s.trim_matches(is_html_whitespace).to_owned())
}

/// What the candidate `source` makes of itself.
/// <https://html.spec.whatwg.org/multipage/media.html#resource-selection-algorithm>,
/// step "children": no `src`, a `media` that does not match, a `type` that
/// `canPlayType` answers `""` for, a URL that does not resolve.
pub(super) fn candidate(cx: &Cx<'_>, source: NodeId) -> Candidate {
    let Some(element) = cx.dom.element(source) else {
        return Candidate::Skip;
    };
    let Some(src) = element
        .attr("src")
        .map(|s| s.trim_matches(is_html_whitespace))
        .filter(|s| !s.is_empty())
    else {
        return Candidate::Skip;
    };
    if let Some(media) = element.attr("media")
        && !MediaQueryList::parse_str(media).matches(&cx.env)
    {
        return Candidate::Fail;
    }
    if let Some(kind) = element.attr("type").filter(|t| !t.is_empty())
        && swb_media::can_play_type(kind) == swb_media::CanPlay::No
    {
        return Candidate::Fail;
    }
    match cx.base.join(src) {
        Ok(url) if cx.may_load(&url) => Candidate::Use(url),
        Ok(url) => {
            log::warn!("not allowed to load the media {url}");
            Candidate::Fail
        }
        Err(error) => {
            log::warn!("media URL {src:?}: {error}");
            Candidate::Fail
        }
    }
}
