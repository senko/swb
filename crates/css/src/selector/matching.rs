//! Selector matching.
//!
//! <https://www.w3.org/TR/selectors-4/#match-against-element>
//!
//! Matching runs right to left: first the subject compound against the
//! element, then each combinator moves to a parent or a previous sibling.
//! Recursion depth is bounded by the number of compounds in the selector,
//! not by the depth of the tree: ancestors and siblings are visited in a
//! loop.
//!
//! To avoid exponential backtracking with descendant and sibling
//! combinators, a failed match reports how far back the search must go
//! (see [`MatchResult`]). Browser engines use the same approach; the code
//! here is our own.
//!
//! Case rules:
//!
//! - Type selectors and attribute names match ASCII case-insensitively on
//!   HTML elements (the selector side is lowercased), and case-sensitively
//!   on other elements.
//! - Attribute values match case-sensitively, except with the `i` flag and
//!   for the attributes that the HTML specification lists, on HTML
//!   elements, unless the `s` flag is given.
//! - In quirks mode, class and ID selectors match ASCII case-insensitively,
//!   and the `:hover`/`:active` quirk applies
//!   (<https://quirks.spec.whatwg.org/#the-active-and-hover-quirk>).
//!
//! Hostile selectors: the parser limits how deep matching can recurse (see
//! `MAX_MATCH_DEPTH` in the parse module), and each call to [`matches()`]
//! visits at most [`MATCH_BUDGET`] elements. Nested arguments such as
//! `:is(:is(:is(.x *) *) *) *` can otherwise take exponential time. When
//! the budget runs out, the selector does not match.

use std::collections::HashMap;

use bitflags::bitflags;
use log::debug;

use super::{
    AttrCase, AttrOperator, AttributeSelector, Combinator, Component, Direction, Nth, PseudoClass,
    RelativeSelector, Selector, SelectorList,
};

bitflags! {
    /// Dynamic state of an element, for state pseudo-classes. The style
    /// crate computes it from the DOM and the user interaction state.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
    pub struct ElementState: u32 {
        /// `:hover`.
        const HOVER = 1 << 0;
        /// `:active`.
        const ACTIVE = 1 << 1;
        /// `:focus`.
        const FOCUS = 1 << 2;
        /// `:focus-visible`.
        const FOCUS_VISIBLE = 1 << 3;
        /// `:focus-within`.
        const FOCUS_WITHIN = 1 << 4;
        /// The link is visited (`:visited` instead of `:link`).
        const VISITED = 1 << 5;
        /// `:checked`.
        const CHECKED = 1 << 6;
        /// `:disabled`.
        const DISABLED = 1 << 7;
        /// `:enabled`.
        const ENABLED = 1 << 8;
        /// `:indeterminate`.
        const INDETERMINATE = 1 << 9;
        /// `:required`.
        const REQUIRED = 1 << 10;
        /// `:optional`.
        const OPTIONAL = 1 << 11;
        /// `:read-only`.
        const READ_ONLY = 1 << 12;
        /// `:read-write`.
        const READ_WRITE = 1 << 13;
        /// `:placeholder-shown`.
        const PLACEHOLDER_SHOWN = 1 << 14;
        /// `:default`.
        const DEFAULT = 1 << 15;
        /// `:target`.
        const TARGET = 1 << 16;
        /// `:defined`.
        const DEFINED = 1 << 17;
        /// `:valid`.
        const VALID = 1 << 18;
        /// `:invalid`.
        const INVALID = 1 << 19;
        /// `:user-valid`.
        const USER_VALID = 1 << 20;
        /// `:user-invalid`.
        const USER_INVALID = 1 << 21;
        /// `:in-range`.
        const IN_RANGE = 1 << 22;
        /// `:out-of-range`.
        const OUT_OF_RANGE = 1 << 23;
        /// `:autofill`.
        const AUTOFILL = 1 << 24;
        /// `:open` (open `<details>`, `<dialog>`, `<select>`).
        const OPEN = 1 << 25;
        /// `:modal`.
        const MODAL = 1 << 26;
        /// `:fullscreen`.
        const FULLSCREEN = 1 << 27;
        /// `:popover-open`.
        const POPOVER_OPEN = 1 << 28;
    }
}

/// The document's quirks mode.
/// <https://dom.spec.whatwg.org/#concept-document-quirks>
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum QuirksMode {
    /// Standards mode.
    #[default]
    NoQuirks,
    /// Limited-quirks mode (no effect on selectors).
    LimitedQuirks,
    /// Quirks mode.
    Quirks,
}

/// How to compare names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CaseSensitivity {
    /// Exact comparison.
    CaseSensitive,
    /// ASCII case-insensitive comparison.
    AsciiCaseInsensitive,
}

impl CaseSensitivity {
    /// Compares `a` and `b`.
    pub fn eq(self, a: &str, b: &str) -> bool {
        match self {
            CaseSensitivity::CaseSensitive => a == b,
            CaseSensitivity::AsciiCaseInsensitive => a.eq_ignore_ascii_case(b),
        }
    }
}

/// An element that selectors can match. The style crate implements it for
/// DOM elements; it should be a cheap handle (for example a tree reference
/// and a node ID).
pub trait Element: Sized + Clone {
    /// The parent element, if the parent is an element.
    fn parent_element(&self) -> Option<Self>;
    /// The previous sibling that is an element.
    fn prev_sibling_element(&self) -> Option<Self>;
    /// The next sibling that is an element.
    fn next_sibling_element(&self) -> Option<Self>;
    /// The first child that is an element.
    fn first_child_element(&self) -> Option<Self>;
    /// The local name. Already lowercase for HTML elements.
    fn local_name(&self) -> &str;
    /// True for an element in the HTML namespace in an HTML document.
    fn is_html_element(&self) -> bool;
    /// The `id` attribute.
    fn id(&self) -> Option<&str>;
    /// True if the element's class list contains `name`.
    fn has_class(&self, name: &str, case: CaseSensitivity) -> bool;
    /// The value of the attribute with this local name and no namespace.
    /// The name is lowercase for HTML elements.
    fn attribute(&self, local_name: &str) -> Option<&str>;
    /// True for the document element.
    fn is_root(&self) -> bool;
    /// True if the element is a link (`:any-link`). In HTML: `a` and
    /// `area` elements with an `href` attribute.
    fn is_link(&self) -> bool;
    /// True if the element has an element child or a non-empty text child
    /// (`:empty` is the opposite).
    fn has_children(&self) -> bool;
    /// The dynamic state.
    fn state(&self) -> ElementState;
    /// True if `self` and `other` are the same element.
    fn same_element(&self, other: &Self) -> bool;
    /// A key that identifies the element in its document (for example its
    /// node ID), for the caches in [`MatchingContext`]. The default, `None`,
    /// disables caching; then `:nth-child()` and related pseudo-classes
    /// cost O(number of siblings) per element.
    fn cache_key(&self) -> Option<usize> {
        None
    }
}

/// The maximum number of elements that one call to [`matches()`] visits.
pub const MATCH_BUDGET: u32 = 200_000;

/// Settings and caches for matching.
///
/// The caches assume that the document does not change. Create a new
/// context after a DOM change.
#[derive(Clone, Debug, Default)]
pub struct MatchingContext {
    /// The document's quirks mode.
    pub quirks_mode: QuirksMode,
    /// Sibling indices for `:nth-*()`, by kind (see [`NthKind`]) and
    /// [`Element::cache_key`].
    nth_indices: [HashMap<usize, i32>; 4],
}

impl MatchingContext {
    /// Creates a context.
    pub fn new(quirks_mode: QuirksMode) -> Self {
        MatchingContext {
            quirks_mode,
            ..MatchingContext::default()
        }
    }
}

/// Which sibling index to compute.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NthKind {
    /// Count from the end (`:nth-last-*`).
    from_end: bool,
    /// Count only siblings of the same type (`:nth-*-of-type`).
    of_type: bool,
}

impl NthKind {
    fn slot(self) -> usize {
        usize::from(self.from_end) | (usize::from(self.of_type) << 1)
    }
}

/// True if `selector` matches `element`. A pseudo-element in the selector
/// is ignored (the originating element is matched). `:scope` matches the
/// root element.
pub fn matches<E: Element>(
    selector: &Selector,
    element: &E,
    context: &mut MatchingContext,
) -> bool {
    matches_with_scope(selector, element, None, context)
}

/// Like [`matches()`], with a scoping root for `:scope` (as in
/// `element.querySelector()`). Without a scope, `:scope` matches the root
/// element.
pub fn matches_with_scope<E: Element>(
    selector: &Selector,
    element: &E,
    scope: Option<&E>,
    context: &mut MatchingContext,
) -> bool {
    let mut matcher = Matcher {
        context,
        scope,
        nesting: 0,
        budget: MATCH_BUDGET,
        exhausted: false,
        at_subject: true,
        has_pseudo_element: selector.pseudo_element.is_some(),
    };
    let result = matcher.match_from(&selector.components, element, None);
    if matcher.exhausted {
        debug!("selector `{selector}` is too expensive to match; treated as no match");
        return false;
    }
    result == MatchResult::Matched
}

/// True if any selector in `list` matches `element`.
pub fn matches_any<E: Element>(
    list: &SelectorList,
    element: &E,
    context: &mut MatchingContext,
) -> bool {
    list.iter().any(|s| matches(s, element, context))
}

/// Splits off the first compound of right-to-left components. Returns the
/// compound and the rest (starting with a combinator, or empty).
pub(crate) fn split_compound(components: &[Component]) -> (&[Component], &[Component]) {
    let end = components
        .iter()
        .position(|c| matches!(c, Component::Combinator(_)))
        .unwrap_or(components.len());
    components.split_at(end)
}

/// The result of matching part of a selector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MatchResult {
    Matched,
    /// No match here; another candidate for the combinator to the right may
    /// still match.
    Unmatched,
    /// No match, and no other candidate for a sibling combinator to the
    /// right can match; only another ancestor for a descendant combinator
    /// can.
    UnmatchedRetryAncestor,
    /// No match, and no other choice anywhere can match.
    UnmatchedGlobally,
}

/// The element that a `:has()` argument is relative to.
struct Anchor<'x, E> {
    combinator: Combinator,
    element: &'x E,
}

impl<E> Clone for Anchor<'_, E> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<E> Copy for Anchor<'_, E> {}

/// What the left side of a combinator must match.
enum Next<'s, 'x, E> {
    Components(&'s [Component]),
    Anchor(&'x E),
}

struct Matcher<'c, 's, E> {
    context: &'c mut MatchingContext,
    scope: Option<&'s E>,
    /// The nesting depth of selector arguments (`:is()`, `:not()`, ...).
    nesting: u32,
    /// How many more elements this match may visit.
    budget: u32,
    /// True when the budget ran out.
    exhausted: bool,
    /// True until the subject compound of the top-level selector is matched.
    at_subject: bool,
    /// True if the top-level selector ends with a pseudo-element.
    has_pseudo_element: bool,
}

impl<E: Element> Matcher<'_, '_, E> {
    /// Uses one step of the budget. Returns false if none is left.
    fn spend(&mut self) -> bool {
        if self.budget == 0 {
            self.exhausted = true;
            return false;
        }
        self.budget -= 1;
        true
    }

    fn quirks(&self) -> bool {
        self.context.quirks_mode == QuirksMode::Quirks
    }

    fn id_and_class_case(&self) -> CaseSensitivity {
        if self.quirks() {
            CaseSensitivity::AsciiCaseInsensitive
        } else {
            CaseSensitivity::CaseSensitive
        }
    }

    /// Matches right-to-left `components` against `element`, which must
    /// match the first compound.
    fn match_from(
        &mut self,
        components: &[Component],
        element: &E,
        anchor: Option<Anchor<'_, E>>,
    ) -> MatchResult {
        if self.exhausted {
            return MatchResult::UnmatchedGlobally;
        }
        let (compound, rest) = split_compound(components);
        if !self.match_compound(compound, element) {
            return MatchResult::Unmatched;
        }
        let (combinator, next) = match rest.split_first() {
            Some((Component::Combinator(c), rest)) => (*c, Next::Components(rest)),
            _ => match anchor {
                Some(a) => (a.combinator, Next::Anchor(a.element)),
                None => return MatchResult::Matched,
            },
        };
        let not_found = match combinator {
            Combinator::Descendant | Combinator::Child => MatchResult::UnmatchedGlobally,
            Combinator::NextSibling | Combinator::SubsequentSibling => {
                MatchResult::UnmatchedRetryAncestor
            }
        };
        let mut current = element.clone();
        loop {
            let candidate = match combinator {
                Combinator::Descendant | Combinator::Child => current.parent_element(),
                Combinator::NextSibling | Combinator::SubsequentSibling => {
                    current.prev_sibling_element()
                }
            };
            let Some(candidate) = candidate else {
                return not_found;
            };
            if !self.spend() {
                return MatchResult::UnmatchedGlobally;
            }
            let result = match next {
                Next::Components(rest) => self.match_from(rest, &candidate, anchor),
                Next::Anchor(a) if candidate.same_element(a) => MatchResult::Matched,
                Next::Anchor(_) => MatchResult::Unmatched,
            };
            match (result, combinator) {
                (MatchResult::Matched | MatchResult::UnmatchedGlobally, _)
                | (_, Combinator::NextSibling) => return result,
                // All siblings share the parent: a sibling combinator to the
                // right cannot help.
                (_, Combinator::Child) => return MatchResult::UnmatchedRetryAncestor,
                (MatchResult::UnmatchedRetryAncestor, Combinator::SubsequentSibling) => {
                    return result;
                }
                // Descendant: try the next ancestor. Subsequent sibling: try
                // the next previous sibling.
                _ => {}
            }
            current = candidate;
        }
    }

    fn match_compound(&mut self, compound: &[Component], element: &E) -> bool {
        let is_subject = std::mem::replace(&mut self.at_subject, false);
        // The quirk does not apply to a compound with a pseudo-element.
        if self.quirks()
            && self.nesting == 0
            && !(is_subject && self.has_pseudo_element)
            && hover_active_quirk_applies(compound)
            && !element.is_link()
        {
            return false;
        }
        compound.iter().all(|c| self.match_simple(c, element))
    }

    fn match_simple(&mut self, component: &Component, element: &E) -> bool {
        match component {
            Component::Combinator(_) | Component::Universal => true,
            Component::LocalName(name) => {
                let expected = if element.is_html_element() {
                    name.lower()
                } else {
                    &name.name
                };
                element.local_name() == expected
            }
            Component::Id(id) => element
                .id()
                .is_some_and(|value| self.id_and_class_case().eq(value, id)),
            Component::Class(class) => element.has_class(class, self.id_and_class_case()),
            Component::Attribute(attr) => match_attribute(attr, element),
            Component::PseudoClass(pc) => self.match_pseudo_class(pc, element),
        }
    }

    fn match_pseudo_class(&mut self, pc: &PseudoClass, element: &E) -> bool {
        match pc {
            PseudoClass::Link => {
                element.is_link() && !element.state().contains(ElementState::VISITED)
            }
            PseudoClass::Visited => {
                element.is_link() && element.state().contains(ElementState::VISITED)
            }
            PseudoClass::AnyLink => element.is_link(),
            PseudoClass::Root => element.is_root(),
            PseudoClass::Empty => !element.has_children(),
            PseudoClass::Scope => match self.scope {
                Some(scope) => scope.same_element(element),
                None => element.is_root(),
            },
            PseudoClass::FirstChild => element.prev_sibling_element().is_none(),
            PseudoClass::LastChild => element.next_sibling_element().is_none(),
            PseudoClass::OnlyChild => {
                element.prev_sibling_element().is_none() && element.next_sibling_element().is_none()
            }
            PseudoClass::FirstOfType => !self.has_sibling_of_type(element, false),
            PseudoClass::LastOfType => !self.has_sibling_of_type(element, true),
            PseudoClass::OnlyOfType => {
                !self.has_sibling_of_type(element, false)
                    && !self.has_sibling_of_type(element, true)
            }
            PseudoClass::State(flags) => element.state().contains(*flags),
            PseudoClass::NthChild(nth) => self.match_nth_child(element, nth, false),
            PseudoClass::NthLastChild(nth) => self.match_nth_child(element, nth, true),
            PseudoClass::NthOfType(anb) => {
                let kind = NthKind {
                    from_end: false,
                    of_type: true,
                };
                anb.matches(self.sibling_index(element, kind))
            }
            PseudoClass::NthLastOfType(anb) => {
                let kind = NthKind {
                    from_end: true,
                    of_type: true,
                };
                anb.matches(self.sibling_index(element, kind))
            }
            PseudoClass::Not(list) => !self.match_nested_list(list, element),
            PseudoClass::Is(list) | PseudoClass::Where(list) => {
                self.match_nested_list(list, element)
            }
            PseudoClass::Has(relative) => relative.iter().any(|r| self.match_relative(r, element)),
            PseudoClass::Lang(ranges) => match_lang(element, ranges),
            PseudoClass::Dir(dir) => match_dir(element, *dir),
            // No shadow trees: no element is a shadow host.
            PseudoClass::Host(_) | PseudoClass::HostContext(_) => false,
        }
    }

    fn match_nested_list(&mut self, list: &SelectorList, element: &E) -> bool {
        self.nesting += 1;
        let result = list
            .iter()
            .any(|s| self.match_from(&s.components, element, None) == MatchResult::Matched);
        self.nesting -= 1;
        result
    }

    /// `:nth-child()` and `:nth-last-child()`, with an optional `of S`.
    fn match_nth_child(&mut self, element: &E, nth: &Nth, from_end: bool) -> bool {
        let Some(of) = &nth.of else {
            let kind = NthKind {
                from_end,
                of_type: false,
            };
            return nth.anb.matches(self.sibling_index(element, kind));
        };
        if !self.match_nested_list(of, element) {
            return false;
        }
        // Not cached: the index depends on the selector list.
        let mut index: i32 = 1;
        let mut sibling = step_sibling(element, from_end);
        while let Some(s) = sibling {
            if !self.spend() {
                return false;
            }
            if self.match_nested_list(of, &s) {
                index = index.saturating_add(1);
            }
            sibling = step_sibling(&s, from_end);
        }
        nth.anb.matches(index)
    }

    /// The 1-based index of `element` among its siblings (of the same type,
    /// if `kind.of_type`), counted from the start or the end. Uses and fills
    /// the cache in the context when the element has a cache key.
    fn sibling_index(&mut self, element: &E, kind: NthKind) -> i32 {
        let Some(key) = element.cache_key() else {
            return self.uncached_sibling_index(element, kind);
        };
        let slot = kind.slot();
        if let Some(&index) = self.context.nth_indices[slot].get(&key) {
            return index;
        }
        // Walk to the nearest counted sibling with a known index (or to the
        // end of the list), then fill in the indices of all counted siblings
        // on the way.
        let mut pending = vec![key];
        let mut base = 0;
        let mut sibling = step_sibling(element, kind.from_end);
        while let Some(s) = sibling {
            if !kind.of_type || same_type(&s, element) {
                let Some(sibling_key) = s.cache_key() else {
                    return self.uncached_sibling_index(element, kind);
                };
                if let Some(&index) = self.context.nth_indices[slot].get(&sibling_key) {
                    base = index;
                    break;
                }
                pending.push(sibling_key);
            }
            sibling = step_sibling(&s, kind.from_end);
        }
        let map = &mut self.context.nth_indices[slot];
        for (offset, key) in pending.iter().rev().enumerate() {
            map.insert(*key, base.saturating_add(offset as i32 + 1));
        }
        base.saturating_add(pending.len() as i32)
    }

    fn uncached_sibling_index(&mut self, element: &E, kind: NthKind) -> i32 {
        let mut index: i32 = 1;
        let mut sibling = step_sibling(element, kind.from_end);
        while let Some(s) = sibling {
            if !self.spend() {
                return 0;
            }
            if !kind.of_type || same_type(&s, element) {
                index = index.saturating_add(1);
            }
            sibling = step_sibling(&s, kind.from_end);
        }
        index
    }

    fn has_sibling_of_type(&mut self, element: &E, forward: bool) -> bool {
        let mut sibling = step_sibling(element, forward);
        while let Some(s) = sibling {
            if same_type(&s, element) {
                return true;
            }
            if !self.spend() {
                return false;
            }
            sibling = step_sibling(&s, forward);
        }
        false
    }

    /// `:has()`: true if some element in the search scope of `anchor`
    /// matches the relative selector.
    fn match_relative(&mut self, relative: &RelativeSelector, anchor: &E) -> bool {
        let components = &relative.selector.components;
        let single_compound = split_compound(components).1.is_empty();
        let anchor_info = Anchor {
            combinator: relative.combinator,
            element: anchor,
        };
        self.nesting += 1;
        let mut found = false;
        let mut check = |matcher: &mut Self, candidate: &E| {
            matcher.spend()
                && matcher.match_from(components, candidate, Some(anchor_info))
                    == MatchResult::Matched
        };
        match relative.combinator {
            Combinator::Child if single_compound => {
                let mut child = anchor.first_child_element();
                while let Some(c) = child {
                    if check(self, &c) {
                        found = true;
                        break;
                    }
                    if self.exhausted {
                        break;
                    }
                    child = c.next_sibling_element();
                }
            }
            Combinator::Child | Combinator::Descendant => {
                found = self.any_descendant(anchor, &mut check);
            }
            Combinator::NextSibling | Combinator::SubsequentSibling => {
                let mut sibling = anchor.next_sibling_element();
                while let Some(s) = sibling {
                    if check(self, &s) || (!single_compound && self.any_descendant(&s, &mut check))
                    {
                        found = true;
                        break;
                    }
                    if self.exhausted
                        || (relative.combinator == Combinator::NextSibling && single_compound)
                    {
                        break;
                    }
                    sibling = s.next_sibling_element();
                }
            }
        }
        self.nesting -= 1;
        found
    }

    /// Visits the descendants of `root` in tree order without recursion.
    fn any_descendant(&mut self, root: &E, check: &mut impl FnMut(&mut Self, &E) -> bool) -> bool {
        let mut current = root.first_child_element();
        while let Some(node) = current {
            if check(self, &node) {
                return true;
            }
            if self.exhausted {
                return false;
            }
            current = next_in_subtree(&node, root);
        }
        false
    }
}

/// The next element after `current` in tree order, staying inside `root`.
fn next_in_subtree<E: Element>(current: &E, root: &E) -> Option<E> {
    if let Some(child) = current.first_child_element() {
        return Some(child);
    }
    let mut node = current.clone();
    loop {
        if node.same_element(root) {
            return None;
        }
        if let Some(sibling) = node.next_sibling_element() {
            return Some(sibling);
        }
        node = node.parent_element()?;
    }
}

fn step_sibling<E: Element>(element: &E, forward: bool) -> Option<E> {
    if forward {
        element.next_sibling_element()
    } else {
        element.prev_sibling_element()
    }
}

fn same_type<E: Element>(a: &E, b: &E) -> bool {
    a.is_html_element() == b.is_html_element() && a.local_name() == b.local_name()
}

/// The `:hover`/`:active` quirk applies to a compound that uses one of them
/// and has no type, ID, class or attribute selector and no other
/// pseudo-class.
fn hover_active_quirk_applies(compound: &[Component]) -> bool {
    let is_hover_or_active = |c: &Component| {
        matches!(c, Component::PseudoClass(PseudoClass::State(s))
            if *s == ElementState::HOVER || *s == ElementState::ACTIVE)
    };
    compound.iter().any(is_hover_or_active)
        && compound
            .iter()
            .all(|c| matches!(c, Component::Universal) || is_hover_or_active(c))
}

fn is_html_whitespace(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\x0c' | b'\r')
}

fn match_attribute<E: Element>(attr: &AttributeSelector, element: &E) -> bool {
    let is_html = element.is_html_element();
    let name = if is_html {
        attr.name.lower()
    } else {
        &attr.name.name
    };
    let Some(value) = element.attribute(name) else {
        return false;
    };
    let Some((operator, expected)) = &attr.operation else {
        return true;
    };
    let insensitive = match attr.case {
        AttrCase::Insensitive => true,
        AttrCase::Sensitive => false,
        AttrCase::Default => is_html && attr.html_case_insensitive,
    };
    let eq = |a: Option<&[u8]>, b: &[u8]| {
        a.is_some_and(|a| {
            if insensitive {
                a.eq_ignore_ascii_case(b)
            } else {
                a == b
            }
        })
    };
    let (v, e) = (value.as_bytes(), expected.as_bytes());
    let n = e.len();
    match operator {
        AttrOperator::Equals => eq(Some(v), e),
        AttrOperator::Includes => {
            !e.is_empty()
                && !e.iter().copied().any(is_html_whitespace)
                && v.split(|&b| is_html_whitespace(b))
                    .any(|word| eq(Some(word), e))
        }
        AttrOperator::DashMatch => eq(Some(v), e) || (v.get(n) == Some(&b'-') && eq(v.get(..n), e)),
        AttrOperator::Prefix => n > 0 && eq(v.get(..n), e),
        AttrOperator::Suffix => n > 0 && eq(v.len().checked_sub(n).and_then(|s| v.get(s..)), e),
        AttrOperator::Substring => n > 0 && v.windows(n).any(|w| eq(Some(w), e)),
    }
}

/// `:lang()`: the language is the `lang` attribute of the nearest
/// ancestor-or-self that has one. Ranges match by prefix up to a `-`
/// (RFC 4647 basic filtering); `*` matches any non-empty language.
fn match_lang<E: Element>(element: &E, ranges: &[Box<str>]) -> bool {
    let mut current = element.clone();
    loop {
        if let Some(lang) = current.attribute("lang") {
            return ranges.iter().any(|range| lang_matches(lang, range));
        }
        match current.parent_element() {
            Some(parent) => current = parent,
            None => return false,
        }
    }
}

fn lang_matches(lang: &str, range: &str) -> bool {
    if range == "*" {
        return !lang.is_empty();
    }
    if range.is_empty() {
        return lang.is_empty();
    }
    let (lang, range) = (lang.as_bytes(), range.as_bytes());
    lang.get(..range.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(range))
        && matches!(lang.get(range.len()), None | Some(b'-'))
}

/// `:dir()`, approximated: the direction is the `dir` attribute (`ltr` or
/// `rtl`) of the nearest ancestor-or-self that has a valid one; `auto`
/// counts as `ltr`, and the default is `ltr`.
fn match_dir<E: Element>(element: &E, dir: Direction) -> bool {
    let mut current = element.clone();
    let rtl = loop {
        if let Some(value) = current.attribute("dir") {
            if value.eq_ignore_ascii_case("rtl") {
                break true;
            }
            if value.eq_ignore_ascii_case("ltr") || value.eq_ignore_ascii_case("auto") {
                break false;
            }
        }
        match current.parent_element() {
            Some(parent) => current = parent,
            None => break false,
        }
    };
    rtl == (dir == Direction::Rtl)
}
