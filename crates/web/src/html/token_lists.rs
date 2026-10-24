//! The `DOMTokenList` members of elements (`relList`, `sizes`,
//! `blocking`, `sandbox`, `htmlFor`) and the supported tokens of those
//! lists (`supports()`).
//!
//! <https://html.spec.whatwg.org/multipage/semantics.html#dom-link-rellist>

use swb_dom::{LocalName, NodeId};
use swb_js::{Gc, Object};

use crate::bindings::{self as b, Cx};
use crate::state::{Slot, same_object};

/// The one token list of the attribute `attribute` of `element`.
pub(crate) fn token_list(
    cx: &mut Cx<'_>,
    element: NodeId,
    attribute: &'static str,
) -> b::Result<Gc<Object>> {
    let realm = cx.realm();
    Ok(same_object(
        cx.rt,
        element,
        Slot::TokenList(attribute),
        |rt| crate::dom::token_list::new_token_list(rt, realm, element, LocalName::from(attribute)),
    )?)
}

/// The supported tokens of the list of `attribute` on an element named
/// `tag` (HTML "supported tokens"), or none if the list has none (then
/// `supports` throws).
pub(crate) fn supported_tokens(tag: &str, attribute: &str) -> Option<&'static [&'static str]> {
    match (tag, attribute) {
        ("link", "rel") => Some(LINK_REL),
        ("a" | "area" | "form", "rel") => Some(ANCHOR_REL),
        ("iframe", "sandbox") => Some(SANDBOX),
        (_, "blocking") => Some(BLOCKING),
        _ => None,
    }
}

const LINK_REL: &[&str] = &[
    "modulepreload",
    "preload",
    "preconnect",
    "dns-prefetch",
    "stylesheet",
    "icon",
    "alternate",
    "prefetch",
    "prerender",
    "next",
    "manifest",
    "canonical",
    "apple-touch-icon",
];

const ANCHOR_REL: &[&str] = &["noreferrer", "noopener", "opener"];

const SANDBOX: &[&str] = &[
    "allow-downloads",
    "allow-forms",
    "allow-modals",
    "allow-orientation-lock",
    "allow-pointer-lock",
    "allow-popups",
    "allow-popups-to-escape-sandbox",
    "allow-presentation",
    "allow-same-origin",
    "allow-scripts",
    "allow-top-navigation",
    "allow-top-navigation-by-user-activation",
    "allow-storage-access-by-user-activation",
];

const BLOCKING: &[&str] = &["render"];
