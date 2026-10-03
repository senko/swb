//! The DOM element handle that selectors match against, and the dynamic
//! element states (hover, focus, ...).
//!
//! [`DomElement`] implements [`swb_css::Element`] for elements of a
//! [`swb_dom::Document`]. Form-control pseudo-classes come from attributes,
//! except the states in [`CONTROL_STATES`], which the page passes in
//! [`ElementStates::controls`] for the controls whose state it knows.

use std::collections::HashMap;
use std::sync::Arc;

use swb_css::{CaseSensitivity, Element, ElementState};
use swb_dom::{Document, ElementData, NodeData, NodeId, local_name};

/// The form-control states that depend on the current state of a control
/// (its checkedness, its value) and not only on its attributes: `:checked`,
/// `:placeholder-shown`, `:valid` and `:invalid`.
pub const CONTROL_STATES: ElementState = ElementState::CHECKED
    .union(ElementState::PLACEHOLDER_SHOWN)
    .union(ElementState::VALID)
    .union(ElementState::INVALID);

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
    /// True if the focused element matches `:focus-visible` (the focus
    /// moved with the keyboard).
    pub focus_visible: bool,
    /// The target of the URL fragment (`:target`).
    pub target: Option<NodeId>,
    /// The [`CONTROL_STATES`] of form controls (and `option` elements),
    /// from their current state. Elements without an entry get these
    /// states from their attributes. Shared, so that a copy is cheap.
    pub controls: Arc<HashMap<NodeId, ElementState>>,
}

impl ElementStates {
    /// The state flags that differ between `self` and `other`, for
    /// [`crate::Stylist::state_dependencies`].
    pub fn changes(&self, other: &ElementStates) -> ElementState {
        let mut changed = ElementState::empty();
        if self.hover != other.hover {
            changed |= ElementState::HOVER;
        }
        if self.active != other.active {
            changed |= ElementState::ACTIVE;
        }
        if self.focus != other.focus {
            changed |= ElementState::FOCUS | ElementState::FOCUS_WITHIN;
        }
        if self.focus != other.focus || self.focus_visible != other.focus_visible {
            changed |= ElementState::FOCUS_VISIBLE;
        }
        if self.target != other.target {
            changed |= ElementState::TARGET;
        }
        if !Arc::ptr_eq(&self.controls, &other.controls) {
            changed |= control_changes(&self.controls, &other.controls);
        }
        changed
    }
}

/// The control states that differ between two maps. An element with an
/// entry in only one map may change all of [`CONTROL_STATES`].
fn control_changes(
    a: &HashMap<NodeId, ElementState>,
    b: &HashMap<NodeId, ElementState>,
) -> ElementState {
    let mut changed = ElementState::empty();
    for (node, state) in a {
        changed |= match b.get(node) {
            Some(other) => (*state ^ *other) & CONTROL_STATES,
            None => CONTROL_STATES,
        };
    }
    if b.keys().any(|node| !a.contains_key(node)) {
        changed |= CONTROL_STATES;
    }
    changed
}

/// True if the element is a disabled form control, a disabled `fieldset`,
/// `optgroup` or `option`: its `disabled` attribute, or for controls and
/// fieldsets a disabled `fieldset` ancestor (outside that fieldset's first
/// `legend`), or for an `option` a disabled parent `optgroup`. For many
/// elements, [`DisabledElements`] is faster.
/// <https://html.spec.whatwg.org/multipage/semantics-other.html#concept-element-disabled>
pub fn is_actually_disabled(doc: &Document, node: NodeId) -> bool {
    doc.element(node)
        .is_some_and(|e| is_disabled_with(doc, node, e, || in_disabled_fieldset(doc, node)))
}

/// [`is_actually_disabled`] for element `e` (`node`), where
/// `in_disabled_fieldset` tells whether the element is inside a disabled
/// fieldset, outside its first legend.
fn is_disabled_with(
    doc: &Document,
    node: NodeId,
    e: &ElementData,
    in_disabled_fieldset: impl FnOnce() -> bool,
) -> bool {
    if !e.is_html() {
        return false;
    }
    match &**e.local_name() {
        "optgroup" => e.has_attr("disabled"),
        "option" => {
            e.has_attr("disabled")
                || doc.parent(node).is_some_and(|p| {
                    doc.element(p).is_some_and(|e| {
                        e.is_html_named(&local_name!("optgroup")) && e.has_attr("disabled")
                    })
                })
        }
        "button" | "input" | "select" | "textarea" | "fieldset" => {
            e.has_attr("disabled") || in_disabled_fieldset()
        }
        _ => false,
    }
}

/// True if `node` is a descendant of a `fieldset` with a `disabled`
/// attribute and not inside that fieldset's first `legend` child. Only a
/// `legend` child can be the first legend, so the siblings are searched
/// only for those.
fn in_disabled_fieldset(doc: &Document, node: NodeId) -> bool {
    // The child of each ancestor on the path from the node.
    let mut child = node;
    for ancestor in doc.ancestors(node) {
        if is_disabled_fieldset(doc, ancestor) {
            let is_legend = |n: NodeId| doc.is_html_element(n, &local_name!("legend"));
            let first_legend = is_legend(child)
                && std::iter::successors(doc.prev_sibling_element(child), |&n| {
                    doc.prev_sibling_element(n)
                })
                .all(|n| !is_legend(n));
            if !first_legend {
                return true;
            }
        }
        child = ancestor;
    }
    false
}

/// True for a `fieldset` element with a `disabled` attribute.
fn is_disabled_fieldset(doc: &Document, node: NodeId) -> bool {
    doc.element(node)
        .is_some_and(|e| e.is_html_named(&local_name!("fieldset")) && e.has_attr("disabled"))
}

/// The disabled elements of a document ([`is_actually_disabled`] for every
/// element), computed in one pass over the tree.
#[derive(Clone, Debug, Default)]
pub struct DisabledElements {
    /// Indexed by `NodeId::index()`.
    disabled: Vec<bool>,
}

impl DisabledElements {
    /// Computes the disabled elements of `doc`.
    pub fn new(doc: &Document) -> Self {
        // True for the nodes inside a fieldset with a `disabled` attribute
        // and outside that fieldset's first legend.
        let mut in_fieldset = vec![false; doc.len()];
        let mut disabled = vec![false; doc.len()];
        // Parents come before their children in tree order.
        for node in doc.descendants(NodeId::DOCUMENT) {
            let Some(e) = doc.element(node) else {
                continue;
            };
            let inside = in_fieldset.get(node.index()).copied().unwrap_or(false);
            if let Some(slot) = disabled.get_mut(node.index()) {
                *slot = is_disabled_with(doc, node, e, || inside);
            }
            let disables = is_disabled_fieldset(doc, node);
            let first_legend = disables
                .then(|| {
                    doc.element_children(node)
                        .find(|&c| doc.is_html_element(c, &local_name!("legend")))
                })
                .flatten();
            for child in doc.children(node) {
                if let Some(slot) = in_fieldset.get_mut(child.index()) {
                    *slot = if disables && Some(child) != first_legend {
                        true
                    } else {
                        inside
                    };
                }
            }
        }
        DisabledElements { disabled }
    }

    /// True if the element is disabled.
    pub fn contains(&self, node: NodeId) -> bool {
        self.disabled.get(node.index()).copied().unwrap_or(false)
    }
}

/// The elements of `doc` that match the selector list `selectors`, in
/// tree order (as `querySelectorAll`), or `None` if the selector list is
/// invalid. Template contents are not searched. Selectors with a
/// pseudo-element match nothing (they select pseudo-elements, not
/// elements).
pub fn query_selector_all(
    doc: &Document,
    selectors: &str,
    states: &ElementStates,
) -> Option<Vec<NodeId>> {
    let list = swb_css::SelectorList::parse_str(selectors).ok()?;
    let states = StateSet::new(doc, states);
    let quirks = match doc.quirks_mode {
        swb_dom::QuirksMode::Quirks => swb_css::QuirksMode::Quirks,
        _ => swb_css::QuirksMode::NoQuirks,
    };
    let mut context = swb_css::MatchingContext::new(quirks);
    let selectors: Vec<_> = list
        .iter()
        .filter(|s| s.pseudo_element().is_none())
        .collect();
    Some(
        doc.descendants(NodeId::DOCUMENT)
            .filter(|&id| {
                DomElement::new(doc, id, &states)
                    .is_some_and(|e| selectors.iter().any(|s| s.matches(&e, &mut context)))
            })
            .collect(),
    )
}

/// Per-document data for matching: the hover, active and focus chains,
/// and the class lists of all elements (split once, because descendant
/// selectors test the classes of ancestors many times).
#[derive(Clone, Debug)]
pub(crate) struct StateSet<'a> {
    hover_chain: Vec<NodeId>,
    active_chain: Vec<NodeId>,
    focus_chain: Vec<NodeId>,
    focus: Option<NodeId>,
    focus_visible: bool,
    target: Option<NodeId>,
    controls: Arc<HashMap<NodeId, ElementState>>,
    disabled: DisabledElements,
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
            focus_visible: states.focus_visible,
            target: states.target,
            controls: Arc::clone(&states.controls),
            disabled: DisabledElements::new(doc),
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
            if self.focus_visible {
                state |= ElementState::FOCUS_VISIBLE;
            }
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

    /// Form-control states from attributes, and the [`CONTROL_STATES`]
    /// from the page if it knows them.
    /// <https://html.spec.whatwg.org/multipage/semantics-other.html#pseudo-classes>
    fn form_state(&self) -> ElementState {
        let state = self.attribute_form_state();
        match self.states.controls.get(&self.id) {
            Some(&dynamic) => (state - CONTROL_STATES) | (dynamic & CONTROL_STATES),
            None => state,
        }
    }

    /// Form-control states from attributes.
    fn attribute_form_state(&self) -> ElementState {
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
        let disabled = disableable && self.states.disabled.contains(self.id);
        if disableable {
            state |= if disabled {
                ElementState::DISABLED
            } else {
                ElementState::ENABLED
            };
        }
        match name {
            "input" => state |= input_state(e, disabled),
            "textarea" => {
                state |= required_state(e, true);
                // A control in a disabled fieldset is disabled too.
                if e.has_attr("readonly") || disabled {
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

/// The states of an `input` element from its attributes; `disabled` tells
/// whether it is disabled (also through a fieldset).
fn input_state(e: &ElementData, disabled: bool) -> ElementState {
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
    if text_like && !e.has_attr("readonly") && !disabled {
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
            Some(classes) => classes.iter().any(|c| case.eq(c, name)),
            None => self.data.classes().any(|c| case.eq(c, name)),
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
        let s = StateSet::new(&doc, &ElementStates::default());
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
    fn query_selector_all_in_tree_order() {
        let doc = parse_html("<p id=a class=x></p><div><p id=b></p></div><p id=c class=x></p>");
        let ids = |selector: &str| {
            query_selector_all(&doc, selector, &ElementStates::default()).map(|nodes| {
                nodes
                    .into_iter()
                    .filter_map(|n| doc.element(n)?.attr("id").map(str::to_owned))
                    .collect::<Vec<_>>()
            })
        };
        assert_eq!(ids("p"), Some(vec!["a".into(), "b".into(), "c".into()]));
        assert_eq!(
            ids(".x, div p"),
            Some(vec!["a".into(), "b".into(), "c".into()])
        );
        assert_eq!(ids("div > p"), Some(vec!["b".into()]));
        assert_eq!(ids("p::before"), Some(vec![]));
        assert_eq!(ids("p::before, div > p"), Some(vec!["b".into()]));
        assert_eq!(ids("p["), None);
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
        assert!(!matches(&doc, &s, "inner", ":focus-visible"));
        let states = ElementStates {
            focus: Some(inner),
            focus_visible: true,
            ..ElementStates::default()
        };
        let s = StateSet::new(&doc, &states);
        assert!(matches(&doc, &s, "inner", ":focus-visible"));
        assert!(!matches(&doc, &s, "outer", ":focus-visible"));
    }

    #[test]
    fn form_controls() {
        let doc = parse_html(
            "<input id=t placeholder=p required><input id=c type=checkbox checked>\
             <input id=d disabled><textarea id=ta readonly></textarea>\
             <select id=s><option id=o selected>a</option></select>",
        );
        let s = StateSet::new(&doc, &ElementStates::default());
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

    #[test]
    fn control_states_from_the_page_replace_attributes() {
        let doc = parse_html("<input id=c type=checkbox checked><input id=t placeholder=p>");
        let (c, t) = (find(&doc, "c"), find(&doc, "t"));
        let controls = HashMap::from([(c, ElementState::empty()), (t, ElementState::VALID)]);
        let states = ElementStates {
            controls: Arc::new(controls),
            ..ElementStates::default()
        };
        let s = StateSet::new(&doc, &states);
        assert!(!matches(&doc, &s, "c", ":checked"));
        assert!(matches(&doc, &s, "c", ":default"));
        assert!(!matches(&doc, &s, "t", ":placeholder-shown"));
        let changed = states.changes(&ElementStates::default());
        assert_eq!(changed, CONTROL_STATES);
        assert!(states.changes(&states.clone()).is_empty());
    }

    #[test]
    fn disabled_fieldsets_and_optgroups() {
        let doc = parse_html(
            "<fieldset disabled><legend><input id=l></legend><input id=f>\
             <textarea id=t></textarea>\
             <select id=s><option id=o1>a</option></select></fieldset>\
             <select><optgroup disabled><option id=o2>b</option></optgroup></select>",
        );
        let s = StateSet::new(&doc, &ElementStates::default());
        assert!(matches(&doc, &s, "l", ":enabled"));
        assert!(matches(&doc, &s, "f", ":disabled"));
        // Fields in a disabled fieldset are read-only (as in Chromium).
        assert!(matches(&doc, &s, "l", ":read-write"));
        assert!(matches(&doc, &s, "f", ":read-only"));
        assert!(matches(&doc, &s, "t", ":read-only"));
        assert!(matches(&doc, &s, "s", ":disabled"));
        // The fieldset disables the select, not its options.
        assert!(matches(&doc, &s, "o1", ":enabled"));
        assert!(matches(&doc, &s, "o2", ":disabled"));
    }

    #[test]
    fn disabled_elements_agree_with_the_single_element_test() {
        let doc = parse_html(
            "<fieldset disabled><legend><input><fieldset><input></fieldset></legend>\
             <legend><input></legend><input><button></button></fieldset>\
             <fieldset disabled><div><legend><input></legend></div></fieldset>\
             <select><optgroup disabled><option>a</option></optgroup><option disabled>b</option>\
             <option>c</option></select><input disabled><textarea></textarea>",
        );
        let set = DisabledElements::new(&doc);
        let mut disabled = 0;
        for node in doc.descendants(NodeId::DOCUMENT) {
            assert_eq!(
                set.contains(node),
                is_actually_disabled(&doc, node),
                "{node:?}"
            );
            disabled += usize::from(set.contains(node));
        }
        assert_eq!(disabled, 10);
    }

    #[test]
    fn many_controls_in_a_disabled_fieldset_cost_linear_time() {
        let mut html = String::from("<fieldset disabled>");
        for _ in 0..20_000 {
            html.push_str("<input>");
        }
        let doc = parse_html(&html);
        let started = std::time::Instant::now();
        let set = DisabledElements::new(&doc);
        let all = doc
            .descendants(NodeId::DOCUMENT)
            .filter(|&n| is_actually_disabled(&doc, n))
            .count();
        assert_eq!(all, 20_001);
        let in_set = doc
            .descendants(NodeId::DOCUMENT)
            .filter(|&n| set.contains(n))
            .count();
        assert_eq!(in_set, all);
        // A check that is quadratic in the number of controls takes much
        // longer.
        assert!(
            started.elapsed() < std::time::Duration::from_millis(500),
            "{:?}",
            started.elapsed()
        );
    }
}
