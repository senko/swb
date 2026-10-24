//! Selector queries for scripts (`querySelector`, `querySelectorAll`,
//! `matches`, `closest`): the selector parser of `swb_css` and the element
//! adapter of this crate, with the states the engine passes.
//! <https://dom.spec.whatwg.org/#scope-match-a-selectors-string>
//!
//! A [`Matcher`] costs no pass over the document: it reads the class names
//! from the elements and computes the disabled state of an element when a
//! selector asks for it, so a script can call `matches` thousands of times
//! on a large document. Its caches (sibling indices for `:nth-child()`)
//! hold for one query; make a new matcher after a change.
//!
//! `:scope` is the element that the query started from; a query on a
//! document or a fragment has no scope element, and `:scope` then matches
//! the root element of a document (and nothing in a fragment). Selectors
//! with a pseudo-element match no element. The pseudo-classes of the
//! dynamic states follow `ElementStates`: `:hover` and `:active` of the
//! engine's current state, `:visited` never.

use swb_css::{MatchingContext, QuirksMode, SelectorList};
use swb_dom::{Document, NodeId, QuirksMode as DomQuirks};

use crate::element::{DomElement, ElementStates, StateSet};

/// The most steps that the matches of one query take in all: elements
/// visited and selectors tried (the
/// right-to-left match of a descendant selector walks the ancestors of
/// each element, so a selector that fails high in a chain of 100,000
/// levels costs the square of the depth). A query that reaches it stops
/// and returns what it has found, with a warning.
const MAX_QUERY_STEPS: u64 = 100_000_000;

/// Parses the selectors string of a query. `None` if it is not a valid
/// selector list (the caller throws `SyntaxError`).
pub fn parse_selectors(text: &str) -> Option<SelectorList> {
    SelectorList::parse_str(text).ok()
}

/// Matches selector lists against the elements of one document state.
pub struct Matcher<'a> {
    doc: &'a Document,
    set: StateSet<'a>,
    context: MatchingContext,
}

impl<'a> Matcher<'a> {
    /// A matcher for elements of the tree of `node`: the quirks mode is
    /// that of the node's document.
    pub fn new(doc: &'a Document, states: &ElementStates, node: NodeId) -> Self {
        let quirks = match doc.quirks_mode_of(doc.node_document(node)) {
            DomQuirks::Quirks => QuirksMode::Quirks,
            _ => QuirksMode::NoQuirks,
        };
        Matcher {
            doc,
            set: StateSet::lazy(doc, states),
            context: MatchingContext::new(quirks),
        }
    }

    /// True if `node` is an element that one selector of `list` matches.
    /// `scope` is the element for `:scope`.
    pub fn matches(&mut self, list: &SelectorList, node: NodeId, scope: Option<NodeId>) -> bool {
        let Some(element) = DomElement::new(self.doc, node, &self.set) else {
            return false;
        };
        let scope = scope.and_then(|s| DomElement::new(self.doc, s, &self.set));
        for selector in list {
            // Each selector tried counts as one step, so a long selector
            // list on many elements is bounded too.
            self.context.steps += 1;
            if self.context.steps > MAX_QUERY_STEPS {
                return false;
            }
            if selector.pseudo_element().is_none()
                && swb_css::matches_with_scope(
                    selector,
                    &element,
                    scope.as_ref(),
                    &mut self.context,
                )
            {
                return true;
            }
        }
        false
    }

    /// The first inclusive ancestor element of `node` that `list` matches
    /// (`closest`).
    pub fn closest(
        &mut self,
        list: &SelectorList,
        node: NodeId,
        scope: Option<NodeId>,
    ) -> Option<NodeId> {
        let mut current = Some(node);
        while let Some(n) = current {
            if self.matches(list, n, scope) {
                return Some(n);
            }
            if self.over_budget() {
                return None;
            }
            current = self.doc.parent(n);
        }
        None
    }

    /// The first descendant element of `root` (not `root` itself) that
    /// `list` matches, in tree order.
    pub fn first(
        &mut self,
        list: &SelectorList,
        root: NodeId,
        scope: Option<NodeId>,
    ) -> Option<NodeId> {
        let doc = self.doc;
        for n in doc.descendants(root).skip(1) {
            if self.matches(list, n, scope) {
                return Some(n);
            }
            if self.over_budget() {
                return None;
            }
        }
        None
    }

    /// Every descendant element of `root` (not `root` itself) that `list`
    /// matches, in tree order.
    pub fn all(&mut self, list: &SelectorList, root: NodeId, scope: Option<NodeId>) -> Vec<NodeId> {
        let doc = self.doc;
        let mut found = Vec::new();
        for n in doc.descendants(root).skip(1) {
            if self.matches(list, n, scope) {
                found.push(n);
            }
            if self.over_budget() {
                break;
            }
        }
        found
    }

    /// True (with a warning) if the query has used its work budget.
    fn over_budget(&self) -> bool {
        let over = self.context.steps > MAX_QUERY_STEPS;
        if over {
            log::warn!(
                "a selector query is too expensive; it stops with the elements found so far"
            );
        }
        over
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swb_dom::parse_html;

    fn ids(doc: &Document, nodes: &[NodeId]) -> Vec<String> {
        nodes
            .iter()
            .map(|&n| {
                doc.element(n)
                    .and_then(|e| e.id())
                    .unwrap_or("-")
                    .to_owned()
            })
            .collect()
    }

    #[test]
    fn queries_find_descendants_in_tree_order() {
        let doc = parse_html(
            "<div id=a><p id=p1 class=k></p><span><p id=p2 class=k></p></span></div><p id=p3 class=k></p>",
        );
        let a = doc.element_by_id("a").expect("a");
        let states = ElementStates::default();
        let mut m = Matcher::new(&doc, &states, a);
        let list = parse_selectors("p.k, span").expect("valid");
        assert_eq!(ids(&doc, &m.all(&list, a, None)), ["p1", "-", "p2"]);
        assert_eq!(
            m.first(&list, a, None).map(|n| ids(&doc, &[n])),
            Some(vec!["p1".into()])
        );
        // The root is not a candidate.
        let div = parse_selectors("div").expect("valid");
        assert_eq!(m.all(&div, a, None).len(), 0);
        assert!(m.matches(&div, a, None));
    }

    #[test]
    fn scope_is_the_start_element() {
        let doc = parse_html("<div id=a><p id=p1></p></div>");
        let a = doc.element_by_id("a").expect("a");
        let p = doc.element_by_id("p1").expect("p1");
        let states = ElementStates::default();
        let mut m = Matcher::new(&doc, &states, a);
        let list = parse_selectors(":scope > p").expect("valid");
        assert_eq!(m.first(&list, a, Some(a)), Some(p));
        assert_eq!(m.first(&list, NodeId::DOCUMENT, None), None);
        let root = parse_selectors(":scope").expect("valid");
        assert_eq!(
            m.first(&root, NodeId::DOCUMENT, None),
            doc.document_element()
        );
        assert_eq!(m.closest(&root, p, Some(a)), Some(a));
    }

    #[test]
    fn pseudo_elements_match_nothing_and_bad_selectors_do_not_parse() {
        let doc = parse_html("<p id=p></p>");
        let states = ElementStates::default();
        let mut m = Matcher::new(&doc, &states, NodeId::DOCUMENT);
        let list = parse_selectors("p::before").expect("valid");
        assert_eq!(m.all(&list, NodeId::DOCUMENT, None).len(), 0);
        for bad in ["", "  ", "p[", "a:foo", "1a", "p >", ":not()"] {
            assert!(parse_selectors(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn many_descendant_combinators_do_not_backtrack_exponentially() {
        // The parser limits the depth to 512; a failing selector with 40
        // descendant combinators on 400 levels would take 400^40 steps
        // without the matcher's shortcuts and its budget.
        let depth = 400;
        let html = format!(
            "{}<i id=leaf></i>{}",
            "<div>".repeat(depth),
            "</div>".repeat(depth)
        );
        let doc = parse_html(&html);
        let states = ElementStates::default();
        let mut m = Matcher::new(&doc, &states, NodeId::DOCUMENT);
        let start = std::time::Instant::now();
        for (selector, expected) in [
            (format!("{}span", "div ".repeat(40)), 0),
            (format!("{}i", "div > p ".repeat(20)), 0),
            (format!("{}i", "div ".repeat(40)), 1),
        ] {
            let list = parse_selectors(&selector).expect("valid");
            assert_eq!(
                m.all(&list, NodeId::DOCUMENT, None).len(),
                expected,
                "{selector}"
            );
        }
        assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
    }
}
