//! The DOM element handle that selectors match against, and the dynamic
//! element states (hover, focus, ...).
//!
//! [`DomElement`] implements [`swb_css::Element`] for elements of a
//! [`swb_dom::Document`]. Form-control pseudo-classes come from attributes:
//! swb has no form state yet.

use swb_css::{CaseSensitivity, Element, ElementState};
use swb_dom::{Document, ElementData, NodeData, NodeId};

/// The interaction state of a document: which elements are hovered,
/// active, focused, or the target of the URL fragment.
///
/// `:hover` and `:active` also match the ancestors of the node. Links
/// never match `:visited` (swb keeps no history for styling).
#[derive(Default, Clone, Debug)]
pub struct ElementStates {
    /// The element under the pointer.
    pub hover: Option<NodeId>,
    /// The element being activated (mouse button down).
    pub active: Option<NodeId>,
    /// The focused element.
    pub focus: Option<NodeId>,
    /// The target of the URL fragment (`:target`).
    pub target: Option<NodeId>,
}

/// Per-document data for matching: the hover, active and focus chains,
/// and the class lists of all elements (split once, because descendant
/// selectors test the classes of ancestors many times).
#[derive(Clone, Debug, Default)]
pub(crate) struct StateSet<'a> {
    hover_chain: Vec<NodeId>,
    active_chain: Vec<NodeId>,
    focus_chain: Vec<NodeId>,
    focus: Option<NodeId>,
    target: Option<NodeId>,
    /// The class names of all elements, concatenated.
    classes: Vec<&'a str>,
    /// For each node index, the range of its classes in `classes`.
    class_ranges: Vec<(u32, u32)>,
}

impl<'a> StateSet<'a> {
    /// Prepares the states of `doc`.
    pub(crate) fn new(doc: &'a Document, states: &ElementStates) -> Self {
        let chain = |node: Option<NodeId>| -> Vec<NodeId> {
            let Some(node) = node.filter(|&n| doc.get(n).is_some()) else {
                return Vec::new();
            };
            std::iter::once(node)
                .chain(doc.ancestors(node))
                .filter(|&n| doc.node(n).is_element())
                .collect()
        };
        let mut classes = Vec::new();
        let mut class_ranges = vec![(0, 0); doc.len()];
        for (index, range) in class_ranges.iter_mut().enumerate() {
            let Some(e) = NodeId::from_index(index).and_then(|id| doc.get(id)?.as_element()) else {
                continue;
            };
            let start = classes.len() as u32;
            classes.extend(e.classes());
            *range = (start, classes.len() as u32);
        }
        StateSet {
            hover_chain: chain(states.hover),
            active_chain: chain(states.active),
            focus_chain: chain(states.focus),
            focus: states.focus,
            target: states.target,
            classes,
            class_ranges,
        }
    }

    /// The class names of an element, if precomputed.
    fn classes_of(&self, id: NodeId) -> Option<&[&'a str]> {
        let &(start, end) = self.class_ranges.get(id.index())?;
        self.classes.get(start as usize..end as usize)
    }

    /// The states of an element that do not depend on its attributes.
    fn interaction_state(&self, id: NodeId) -> ElementState {
        let mut state = ElementState::DEFINED;
        if self.hover_chain.contains(&id) {
            state |= ElementState::HOVER;
        }
        if self.active_chain.contains(&id) {
            state |= ElementState::ACTIVE;
        }
        if self.focus_chain.contains(&id) {
            state |= ElementState::FOCUS_WITHIN;
        }
        if self.focus == Some(id) {
            state |= ElementState::FOCUS;
        }
        if self.target == Some(id) {
            state |= ElementState::TARGET;
        }
        state
    }
}

/// A handle to an element of a document, for selector matching.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DomElement<'a> {
    doc: &'a Document,
    id: NodeId,
    data: &'a ElementData,
    states: &'a StateSet<'a>,
}

impl<'a> DomElement<'a> {
    /// A handle for `id`, if it is an element.
    pub(crate) fn new(doc: &'a Document, id: NodeId, states: &'a StateSet<'a>) -> Option<Self> {
        let data = doc.element(id)?;
        Some(DomElement {
            doc,
            id,
            data,
            states,
        })
    }

    /// The node ID.
    pub(crate) fn node_id(&self) -> NodeId {
        self.id
    }

    /// The element data.
    pub(crate) fn data(&self) -> &'a ElementData {
        self.data
    }

    fn handle(&self, id: Option<NodeId>) -> Option<Self> {
        DomElement::new(self.doc, id?, self.states)
    }

    fn is_html(&self, name: &str) -> bool {
        self.data.is_html() && &**self.data.local_name() == name
    }

    /// Form-control states from attributes.
    /// <https://html.spec.whatwg.org/multipage/semantics-other.html#pseudo-classes>
    fn form_state(&self) -> ElementState {
        if !self.data.is_html() {
            return ElementState::empty();
        }
        let e = self.data;
        let name = &**e.local_name();
        let mut state = ElementState::empty();
        let disableable = matches!(
            name,
            "button" | "input" | "select" | "textarea" | "optgroup" | "option" | "fieldset"
        );
        if disableable {
            state |= if e.has_attr("disabled") {
                ElementState::DISABLED
            } else {
                ElementState::ENABLED
            };
        }
        match name {
            "input" => state |= input_state(e),
            "textarea" => {
                state |= required_state(e, true);
                if e.has_attr("readonly") || e.has_attr("disabled") {
                    state |= ElementState::READ_ONLY;
                } else {
                    state |= ElementState::READ_WRITE;
                }
                let empty = self.doc.text_content(self.id).is_empty();
                if e.has_attr("placeholder") && empty {
                    state |= ElementState::PLACEHOLDER_SHOWN;
                }
                state |= if e.has_attr("required") && empty {
                    ElementState::INVALID
                } else {
                    ElementState::VALID
                };
            }
            "select" => {
                state |= required_state(e, true) | ElementState::READ_ONLY | ElementState::VALID;
            }
            "option" => {
                if e.has_attr("selected") {
                    state |= ElementState::CHECKED | ElementState::DEFAULT;
                }
                state |= ElementState::READ_ONLY;
            }
            "details" | "dialog" => {
                if e.has_attr("open") {
                    state |= ElementState::OPEN;
                }
                state |= ElementState::READ_ONLY;
            }
            _ => {
                if e.has_attr("contenteditable")
                    && !e
                        .attr("contenteditable")
                        .is_some_and(|v| v.eq_ignore_ascii_case("false"))
                {
                    state |= ElementState::READ_WRITE;
                } else {
                    state |= ElementState::READ_ONLY;
                }
            }
        }
        state
    }
}

/// The states of an `input` element from its attributes.
fn input_state(e: &ElementData) -> ElementState {
    let mut state = ElementState::empty();
    let t = e.attr("type").unwrap_or("text").to_ascii_lowercase();

    let checkable = t == "checkbox" || t == "radio";
    if checkable && e.has_attr("checked") {
        state |= ElementState::CHECKED | ElementState::DEFAULT;
    }
    let text_like = matches!(
        t.as_str(),
        "text"
            | "search"
            | "url"
            | "tel"
            | "email"
            | "password"
            | "number"
            | "date"
            | "month"
            | "week"
            | "time"
            | "datetime-local"
    );
    state |= required_state(
        e,
        !matches!(
            t.as_str(),
            "hidden" | "button" | "submit" | "reset" | "image"
        ),
    );
    if text_like && !e.has_attr("readonly") && !e.has_attr("disabled") {
        state |= ElementState::READ_WRITE;
    } else {
        state |= ElementState::READ_ONLY;
    }
    if text_like && e.has_attr("placeholder") && e.attr("value").is_none_or(str::is_empty) {
        state |= ElementState::PLACEHOLDER_SHOWN;
    }
    if text_like || checkable {
        let empty = if checkable {
            !e.has_attr("checked")
        } else {
            e.attr("value").is_none_or(str::is_empty)
        };
        state |= if e.has_attr("required") && empty {
            ElementState::INVALID
        } else {
            ElementState::VALID
        };
    }
    state
}

fn required_state(e: &ElementData, applies: bool) -> ElementState {
    if !applies {
        return ElementState::empty();
    }
    if e.has_attr("required") {
        ElementState::REQUIRED
    } else {
        ElementState::OPTIONAL
    }
}

impl Element for DomElement<'_> {
    fn parent_element(&self) -> Option<Self> {
        self.handle(self.doc.parent_element(self.id))
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        self.handle(self.doc.prev_sibling_element(self.id))
    }

    fn next_sibling_element(&self) -> Option<Self> {
        self.handle(self.doc.next_sibling_element(self.id))
    }

    fn first_child_element(&self) -> Option<Self> {
        self.handle(self.doc.first_element_child(self.id))
    }

    fn local_name(&self) -> &str {
        self.data.local_name()
    }

    fn is_html_element(&self) -> bool {
        self.data.is_html()
    }

    fn id(&self) -> Option<&str> {
        self.data.id()
    }

    fn has_class(&self, name: &str, case: CaseSensitivity) -> bool {
        match self.states.classes_of(self.id) {
            Some(classes) if !self.states.class_ranges.is_empty() => {
                classes.iter().any(|c| case.eq(c, name))
            }
            _ => self.data.classes().any(|c| case.eq(c, name)),
        }
    }

    fn attribute(&self, local_name: &str) -> Option<&str> {
        self.data.attr(local_name)
    }

    fn is_root(&self) -> bool {
        self.doc.document_element() == Some(self.id)
    }

    /// `a` and `area` elements with `href` (HTML: "all a elements that have
    /// an href attribute, and all area elements that have an href
    /// attribute").
    fn is_link(&self) -> bool {
        (self.is_html("a") || self.is_html("area")) && self.data.has_attr("href")
    }

    fn has_children(&self) -> bool {
        self.doc
            .children(self.id)
            .any(|c| match &self.doc.node(c).data {
                NodeData::Element(_) => true,
                NodeData::Text(t) => !t.is_empty(),
                _ => false,
            })
    }

    fn state(&self) -> ElementState {
        let mut state = self.states.interaction_state(self.id) | self.form_state();
        if state.contains(ElementState::FOCUS)
            && matches!(&**self.data.local_name(), "input" | "textarea" | "select")
        {
            state |= ElementState::FOCUS_VISIBLE;
        }
        state
    }

    fn same_element(&self, other: &Self) -> bool {
        self.id == other.id
    }

    fn cache_key(&self) -> Option<usize> {
        Some(self.id.index())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swb_css::{MatchingContext, QuirksMode, SelectorList, matches_any};
    use swb_dom::parse_html;

    fn find(doc: &Document, id: &str) -> NodeId {
        doc.element_by_id(id).expect("element exists")
    }

    fn matches(doc: &Document, states: &StateSet<'_>, id: &str, selector: &str) -> bool {
        let el = DomElement::new(doc, find(doc, id), states).expect("element");
        let list = SelectorList::parse_str(selector).expect("valid selector");
        matches_any(&list, &el, &mut MatchingContext::new(QuirksMode::NoQuirks))
    }

    #[test]
    fn structure_and_links() {
        let doc = parse_html(
            "<div id=d class='a b'><a id=l href=x>x</a><a id=n>y</a><p id=e></p><p id=t>t</p></div>",
        );
        let s = StateSet::default();
        assert!(matches(&doc, &s, "d", "div.a.b"));
        assert!(matches(&doc, &s, "l", ":link"));
        assert!(matches(&doc, &s, "l", "a:any-link"));
        assert!(!matches(&doc, &s, "l", ":visited"));
        assert!(!matches(&doc, &s, "n", ":link"));
        assert!(matches(&doc, &s, "e", "p:empty"));
        assert!(!matches(&doc, &s, "t", "p:empty"));
        assert!(matches(&doc, &s, "t", "div > p:last-child"));
        assert!(matches(&doc, &s, "l", ":defined"));
        assert!(matches(&doc, &s, "d", "div:has(> a:link)"));
    }

    #[test]
    fn hover_chain_and_focus() {
        let doc = parse_html("<div id=outer><span id=inner>x</span></div><p id=other>");
        let inner = find(&doc, "inner");
        let states = ElementStates {
            hover: Some(inner),
            focus: Some(inner),
            ..ElementStates::default()
        };
        let s = StateSet::new(&doc, &states);
        assert!(matches(&doc, &s, "inner", ":hover"));
        assert!(matches(&doc, &s, "outer", "div:hover"));
        assert!(!matches(&doc, &s, "other", "p:hover"));
        assert!(matches(&doc, &s, "inner", ":focus"));
        assert!(matches(&doc, &s, "outer", ":focus-within"));
        assert!(!matches(&doc, &s, "outer", ":focus"));
    }

    #[test]
    fn form_controls() {
        let doc = parse_html(
            "<input id=t placeholder=p required><input id=c type=checkbox checked>\
             <input id=d disabled><textarea id=ta readonly></textarea>\
             <select id=s><option id=o selected>a</option></select>",
        );
        let s = StateSet::default();
        assert!(matches(&doc, &s, "t", ":placeholder-shown"));
        assert!(matches(&doc, &s, "t", ":required"));
        assert!(matches(&doc, &s, "t", ":invalid"));
        assert!(matches(&doc, &s, "t", ":read-write"));
        assert!(matches(&doc, &s, "t", ":enabled"));
        assert!(matches(&doc, &s, "c", ":checked"));
        assert!(matches(&doc, &s, "c", ":optional"));
        assert!(matches(&doc, &s, "d", ":disabled"));
        assert!(matches(&doc, &s, "d", ":read-only"));
        assert!(matches(&doc, &s, "ta", ":read-only"));
        assert!(matches(&doc, &s, "o", ":checked"));
        assert!(matches(&doc, &s, "o", ":default"));
        assert!(matches(&doc, &s, "s", ":enabled"));
    }
}
