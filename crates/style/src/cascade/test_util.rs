//! Helpers that the unit tests of the cascade modules share: the base URL,
//! element lookup by id, a stylist of one author sheet, the size of a
//! targeted result and a time bound.

use std::time::{Duration, Instant};

use swb_dom::{Document, NodeId};
use url::Url;

use super::targeted::Restyle;
use crate::stylist::Stylist;

/// The base URL of the test documents.
pub(super) fn base() -> Url {
    Url::parse("https://example.com/").expect("valid URL")
}

/// The element with id `id`; panics if there is none.
pub(super) fn node(doc: &Document, id: &str) -> NodeId {
    doc.element_by_id(id)
        .unwrap_or_else(|| panic!("no element #{id}"))
}

/// A stylist with the user-agent sheet and the author sheet `css`.
pub(super) fn stylist(doc: &Document, css: &str) -> Stylist {
    let mut stylist = Stylist::new(doc.quirks_mode);
    stylist.add_author_sheet(&swb_css::parse_stylesheet(css), &base());
    stylist
}

/// The number of restyled elements of a targeted result.
pub(super) fn restyled(result: &Restyle) -> usize {
    match result {
        Restyle::Done { restyled, .. } => *restyled,
        Restyle::Full(reason) => panic!("a full restyle: {reason:?}"),
    }
}

/// Runs `f` and fails if it took longer than `limit`.
pub(super) fn within(limit: Duration, what: &str, f: impl FnOnce()) {
    let started = Instant::now();
    f();
    let took = started.elapsed();
    assert!(took < limit, "{what}: {took:?}");
}
