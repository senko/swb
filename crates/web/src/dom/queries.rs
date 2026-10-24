//! The selector queries of `Element` and `ParentNode` (DOM §4.2.6,
//! <https://dom.spec.whatwg.org/#scope-match-a-selectors-string>):
//! `querySelector`, `querySelectorAll`, `matches`, `closest` and
//! `webkitMatchesSelector`.
//!
//! The selector parser is `swb_css`'s and the matcher `swb_style`'s
//! ([`swb_style::Matcher`]): the pseudo-classes that depend on state follow
//! the engine's element states ([`crate::EngineHost::element_states`]).
//! A string that is not a selector list throws a `SyntaxError` with
//! Chromium's message. Parsed lists are cached by their text, so a loop
//! that calls `querySelector('.item')` parses it once.

use std::rc::Rc;

use swb_css::SelectorList;
use swb_dom::NodeId;
use swb_style::{CONTROL_STATES, Matcher, parse_selectors};

use super::lists;
use crate::bindings::{self as b, Cx, Exception};
use crate::state::{dom, engine_mut, state, state_mut};

/// The most parsed selector lists that the binding state keeps.
const CACHE_ENTRIES: usize = 256;

/// The longest selector text that is kept.
const CACHE_TEXT: usize = 1024;

/// Parses a selectors string, or throws Chromium's `SyntaxError`.
fn parse(cx: &mut Cx<'_>, selectors: &str) -> b::Result<Rc<SelectorList>> {
    if let Some(list) = state(cx.rt)?.selectors.get(selectors) {
        return Ok(Rc::clone(list));
    }
    if selectors.is_empty() {
        return Err(Exception::dom(
            "SyntaxError",
            "The provided selector is empty.",
        ));
    }
    let Some(list) = parse_selectors(selectors) else {
        return Err(Exception::dom(
            "SyntaxError",
            format!("'{selectors}' is not a valid selector."),
        ));
    };
    let list = Rc::new(list);
    if selectors.len() <= CACHE_TEXT {
        let cache = &mut state_mut(cx.rt)?.selectors;
        if cache.len() >= CACHE_ENTRIES {
            cache.clear();
        }
        cache.insert(selectors.to_owned(), Rc::clone(&list));
    }
    Ok(list)
}

/// Runs `f` with a matcher for the tree of `node` and the parsed list.
fn with_matcher<T>(
    cx: &mut Cx<'_>,
    node: NodeId,
    selectors: &str,
    f: impl FnOnce(&mut Matcher<'_>, &SelectorList) -> T,
) -> b::Result<T> {
    let list = parse(cx, selectors)?;
    let controls = list
        .iter()
        .any(|s| s.state_dependencies().intersects(CONTROL_STATES));
    let states = engine_mut(cx.rt)?.element_states(controls);
    let mut matcher = Matcher::new(dom(cx.rt)?, &states, node);
    Ok(f(&mut matcher, &list))
}

/// The element that `:scope` means for a query on `node`: the element
/// itself (a document or a fragment has none).
fn scope_of(cx: &Cx<'_>, node: NodeId) -> b::Result<Option<NodeId>> {
    Ok(dom(cx.rt)?.element(node).map(|_| node))
}

/// `querySelector`: the first descendant element that matches.
pub(super) fn query_selector(
    cx: &mut Cx<'_>,
    this: NodeId,
    selectors: &str,
) -> b::Result<Option<NodeId>> {
    let scope = scope_of(cx, this)?;
    with_matcher(cx, this, selectors, |m, list| m.first(list, this, scope))
}

/// `querySelectorAll`: a static `NodeList` of the descendant elements that
/// match.
pub(super) fn query_selector_all(
    cx: &mut Cx<'_>,
    this: NodeId,
    selectors: &str,
) -> b::Result<swb_js::Gc<swb_js::Object>> {
    let scope = scope_of(cx, this)?;
    let nodes = with_matcher(cx, this, selectors, |m, list| m.all(list, this, scope))?;
    let realm = cx.realm();
    Ok(lists::new_static_list(cx.rt, realm, nodes)?)
}

/// `Element.matches`.
pub(super) fn matches(cx: &mut Cx<'_>, this: NodeId, selectors: &str) -> b::Result<bool> {
    with_matcher(cx, this, selectors, |m, list| {
        m.matches(list, this, Some(this))
    })
}

/// `Element.closest`.
pub(super) fn closest(cx: &mut Cx<'_>, this: NodeId, selectors: &str) -> b::Result<Option<NodeId>> {
    with_matcher(cx, this, selectors, |m, list| {
        m.closest(list, this, Some(this))
    })
}
