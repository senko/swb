//! Helpers that the unit tests of the cascade modules share: the base URL,
//! element lookup by id, a stylist of one author sheet, the size of a
//! targeted result, a time bound and the style entry points without
//! animations (the engine calls the `_animated` forms).

use std::sync::Arc;
use std::time::{Duration, Instant};

use swb_css::MediaEnvironment;
use swb_dom::{Document, NodeId};
use url::Url;

use super::targeted::Restyle;
use super::{compute_element_styles_animated, compute_pseudo_element_style_animated};
use crate::ComputedStyle;
use crate::container::ContainerSizes;
use crate::element::ElementStates;
use crate::style_map::PseudoKind;
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

/// [`compute_element_styles_animated`] without animations and with no
/// query container sizes.
pub(super) fn compute_element_styles(
    doc: &Document,
    stylist: &Stylist,
    env: &MediaEnvironment,
    states: &ElementStates,
    base_url: &Url,
    node: NodeId,
) -> Vec<(NodeId, Arc<ComputedStyle>)> {
    let sizes = ContainerSizes::new();
    compute_element_styles_with_containers(doc, stylist, env, states, base_url, node, &sizes)
}

/// [`compute_element_styles_animated`] without animations.
#[allow(clippy::too_many_arguments)]
pub(super) fn compute_element_styles_with_containers(
    doc: &Document,
    stylist: &Stylist,
    env: &MediaEnvironment,
    states: &ElementStates,
    base_url: &Url,
    node: NodeId,
    sizes: &ContainerSizes,
) -> Vec<(NodeId, Arc<ComputedStyle>)> {
    compute_element_styles_animated(doc, stylist, env, states, base_url, node, sizes, None)
}

/// [`compute_pseudo_element_style_animated`] without animations and with
/// no query container sizes.
pub(super) fn compute_pseudo_element_style(
    doc: &Document,
    stylist: &Stylist,
    env: &MediaEnvironment,
    states: &ElementStates,
    base_url: &Url,
    chain: &[(NodeId, Arc<ComputedStyle>)],
    kind: PseudoKind,
) -> Option<ComputedStyle> {
    let sizes = ContainerSizes::new();
    compute_pseudo_element_style_with_containers(
        doc, stylist, env, states, base_url, chain, kind, &sizes,
    )
}

/// [`compute_pseudo_element_style_animated`] without animations.
#[allow(clippy::too_many_arguments)]
pub(super) fn compute_pseudo_element_style_with_containers(
    doc: &Document,
    stylist: &Stylist,
    env: &MediaEnvironment,
    states: &ElementStates,
    base_url: &Url,
    chain: &[(NodeId, Arc<ComputedStyle>)],
    kind: PseudoKind,
    sizes: &ContainerSizes,
) -> Option<ComputedStyle> {
    compute_pseudo_element_style_animated(
        doc, stylist, env, states, base_url, chain, kind, sizes, None,
    )
}
