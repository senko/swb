//! Focus: which elements can be focused, and the sequential focus order
//! (the Tab key).
//!
//! <https://html.spec.whatwg.org/multipage/interaction.html#focusable-area>
//! and
//! <https://html.spec.whatwg.org/multipage/interaction.html#sequential-focus-navigation>.
//! Shadow hosts and the slots of shadow trees own focus navigation scopes
//! (ADR 0036 section 8). Deliberate simplifications: iframes are not scope
//! owners (ADR 0031), and `contenteditable` elements are not focusable
//! (swb edits only form controls).

use std::collections::{HashMap, HashSet};

use swb_dom::{Document, ElementData, NodeId, local_name, parse_integer};
use swb_style::{DisabledElements, is_actually_disabled};

/// The `tabindex` of an element: the attribute if it is a valid integer,
/// otherwise 0 for elements that are focusable by default, otherwise
/// `None` (not focusable). Disabled form controls are not focusable, also
/// with a `tabindex`; `disabled` tells whether `node` is disabled.
fn tab_index(doc: &Document, node: NodeId, disabled: impl FnOnce() -> bool) -> Option<i32> {
    let element = doc.element(node)?;
    if disabled() {
        return None;
    }
    if let Some(value) = element.attr("tabindex").and_then(parse_tab_index) {
        return Some(value);
    }
    focusable_by_default(doc, node, element).then_some(0)
}

/// Parses a `tabindex` value with the rules for parsing integers; values
/// outside the range of an `i32` saturate.
fn parse_tab_index(value: &str) -> Option<i32> {
    let number = parse_integer(value)?;
    Some(number.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32)
}

/// HTML elements that are focusable without `tabindex`.
fn focusable_by_default(doc: &Document, node: NodeId, element: &ElementData) -> bool {
    if !element.is_html() {
        // An SVG `a` with a link is focusable (SVG 2, "Links").
        return element.name.ns == swb_dom::ns!(svg)
            && *element.local_name() == local_name!("a")
            && element.svg_href().is_some();
    }
    match *element.local_name() {
        local_name!("a") | local_name!("area") => element.has_attr("href"),
        local_name!("input") => !element
            .attr("type")
            .is_some_and(|t| t.eq_ignore_ascii_case("hidden")),
        local_name!("button")
        | local_name!("select")
        | local_name!("textarea")
        | local_name!("iframe") => true,
        local_name!("audio") | local_name!("video") => element.has_attr("controls"),
        local_name!("summary") => doc.details_of_summary(node).is_some(),
        _ => false,
    }
}

/// The element that a click on `node` focuses: the nearest element of the
/// click's (composed) event path that is focusable and rendered, or a shown
/// `dialog` (open, or showing as a popover; Chromium 148, measured, cases
/// `e-click-*` and `x-popover-dialog` of
/// `tools/probes/web-top-layer.json`: a click on a dialog's text, padding or
/// backdrop focuses the dialog, also when it is not modal, unless a
/// focusable element is nearer); a shadow host that delegates focus,
/// reached without one, gives its focus delegate ([`focus_delegate`], ADR
/// 0036 section 8).
pub(crate) fn click_target(
    doc: &Document,
    node: NodeId,
    rendered: impl Fn(NodeId) -> bool,
) -> Option<NodeId> {
    std::iter::once(node)
        .chain(doc.event_ancestors(node, true))
        .find_map(|n| {
            if doc.delegates_focus(n) {
                focus_delegate(doc, n, &rendered)
            } else {
                ((is_focusable(doc, n) || doc.is_shown_dialog(n)) && rendered(n)).then_some(n)
            }
        })
}

/// HTML "focus delegate" of the shadow host `host` whose root delegates
/// focus: the first element with `autofocus` that is focusable and
/// rendered, else the first such element, in flat tree order. A host that
/// delegates focus is not a candidate; its tree is searched in its place.
pub(crate) fn focus_delegate(
    doc: &Document,
    host: NodeId,
    rendered: &impl Fn(NodeId) -> bool,
) -> Option<NodeId> {
    for autofocus_only in [true, false] {
        let mut stack: Vec<NodeId> = doc.flat_children(host).collect();
        stack.reverse();
        while let Some(node) = stack.pop() {
            let Some(element) = doc.element(node) else {
                continue;
            };
            let wanted = !autofocus_only || element.has_attr("autofocus");
            if wanted && !doc.delegates_focus(node) && is_focusable(doc, node) && rendered(node) {
                return Some(node);
            }
            let mark = stack.len();
            stack.extend(doc.flat_children(node));
            stack[mark..].reverse();
        }
    }
    None
}

/// True if `node` can be focused (with a click or a script) when it is
/// rendered. An inert node (not in the modal dialog that blocks the
/// document: HTML "blocked by a modal dialog", ADR 0037 section 7) is not
/// focusable.
pub(crate) fn is_focusable(doc: &Document, node: NodeId) -> bool {
    tab_index(doc, node, || is_actually_disabled(doc, node)).is_some() && !doc.is_modal_inert(node)
}

/// The next element in sequential focus order after `from` (or before it,
/// if `backward`), among the rendered elements. Without `from`, the first
/// (or last) element. `from` need not be focusable: navigation then starts
/// at its position in tree order (the sequential focus navigation starting
/// point). Elements with a positive `tabindex` come first, in increasing
/// order, then the others in tree order; a negative `tabindex` removes an
/// element from the order. A document with shadow trees has the order of
/// its focus navigation scopes ([`flattened_order`]).
pub(crate) fn next_in_order(
    doc: &Document,
    from: Option<NodeId>,
    backward: bool,
    rendered: impl Fn(NodeId) -> bool,
) -> Option<NodeId> {
    let popovers = Popovers::of(doc);
    // Under a modal dialog the order is the dialog's: the walk starts at the
    // dialog (the inert rest of the tree is skipped at its root), and a start
    // outside it counts as no start: the order is entered at its edge.
    let from = match popovers.modal {
        Some(dialog) => from.filter(|&f| !doc.is_inert_under(f, dialog)),
        None => from,
    };
    if doc.has_shadow_trees() || !popovers.moved.is_empty() {
        return next_in_scopes(doc, from, backward, &rendered, &popovers);
    }
    let disabled = DisabledElements::new(doc);
    // (tabindex group, tree position, node); group 0 is tabindex 0.
    let mut order: Vec<(i32, usize, NodeId)> = Vec::new();
    let mut from_key = None;
    let root = popovers.modal.unwrap_or(NodeId::DOCUMENT);
    for (position, node) in doc.descendants(root).enumerate() {
        if Some(node) == from {
            from_key = Some(match tab_index(doc, node, || disabled.contains(node)) {
                Some(index) if index > 0 => (index, position),
                // Not in the order: it sorts after the positive group, at
                // its tree position.
                _ => (i32::MAX, position),
            });
        }
        let Some(index) = tab_index(doc, node, || disabled.contains(node)).filter(|&i| i >= 0)
        else {
            continue;
        };
        if rendered(node) {
            let group = if index == 0 { i32::MAX } else { index };
            order.push((group, position, node));
        }
    }
    order.sort_unstable();
    let candidates = order
        .iter()
        .map(|&(group, position, node)| ((group, position), node));
    match (from_key, backward) {
        (None, false) => order.first().map(|e| e.2),
        (None, true) => order.last().map(|e| e.2),
        (Some(key), false) => candidates.clone().find(|(k, _)| *k > key).map(|e| e.1),
        (Some(key), true) => candidates.rev().find(|(k, _)| *k < key).map(|e| e.1),
    }
}

/// An entry of a focus navigation scope.
struct Entry {
    /// The tabindex group: a positive `tabindex`, or `i32::MAX` for the
    /// elements with `tabindex` 0 and the owners without a `tabindex`.
    group: i32,
    node: NodeId,
    /// True if the node is a scope owner: its scope follows it.
    owner: bool,
    /// True if the node is in the order itself (a focusable owner comes
    /// right before its scope; one that is not has its scope in its place).
    emit: bool,
    /// For an owner that is the invoker of showing popovers: the popovers
    /// that follow its scope.
    popovers: Vec<NodeId>,
}

/// True if `node` owns a focus navigation scope: a shadow host, or a slot in
/// a shadow tree (HTML "focus navigation scope owner",
/// <https://html.spec.whatwg.org/multipage/interaction.html#focus-navigation-scope-owner>).
fn is_scope_owner(doc: &Document, node: NodeId) -> bool {
    doc.shadow_root(node).is_some()
        || (doc.is_html_element(node, &local_name!("slot")) && doc.in_shadow_tree(node))
}

/// The showing popovers that follow their invokers in the Tab order (ADR 0037
/// section 5; Chromium 148, measured, `popover-tab-order-*`): a showing
/// popover that has an invoker (the button of a click, or the `source` of
/// `showPopover`) is not visited at its place in the tree; its content is
/// visited as a scope right after the invoker's subtree, whether or not the
/// invoker can be focused. The nested popovers follow in the same way within
/// the scope of their popover. A popover whose invoker is gone or is inside
/// it stays where it is; two popovers that follow each other's invokers
/// would be lost, so they go to the end.
#[derive(Default)]
struct Popovers {
    /// The popovers that follow an invoker, by invoker, in top layer order.
    after: HashMap<NodeId, Vec<NodeId>>,
    /// The popovers that are not visited at their place.
    moved: HashSet<NodeId>,
    /// The same in top layer order (a fixed order for the walk).
    layer_order: Vec<NodeId>,
    /// The modal dialog that blocks the document, if any: the order is the
    /// order of its subtree (ADR 0037 section 7).
    modal: Option<NodeId>,
}

impl Popovers {
    /// The popovers of the document that follow their invokers. One pass over
    /// the showing popovers; the questions about their invokers (is it in the
    /// document, is it inside its popover) share the climbs ([`Climbs`]), so
    /// a chain of nested popovers costs its length, not its square.
    fn of(doc: &Document) -> Popovers {
        let mut popovers = Popovers::default();
        let Some(state) = doc.top_layer().filter(|s| !s.layer().is_empty()) else {
            return popovers;
        };
        popovers.modal = state.blocking();
        let candidates: Vec<(NodeId, NodeId)> = state
            .layer()
            .iter()
            .filter(|&&popover| doc.is_popover_showing(popover))
            .filter_map(|&popover| Some((popover, doc.popover_invoker(popover)?)))
            .filter(|&(popover, invoker)| invoker != popover && doc.element(invoker).is_some())
            .collect();
        if candidates.is_empty() {
            return popovers;
        }
        // Under a modal dialog only the popovers inside it and the invokers
        // inside it matter: the rest of the tree is not in the order, and a
        // popover that follows an inert invoker would be lost (it stays at
        // its place in the dialog instead).
        let modal = popovers.modal;
        let mut climbs = Climbs::new(
            doc,
            candidates.iter().map(|&(popover, _)| popover).chain(modal),
        );
        for (popover, invoker) in candidates {
            if !climbs.connected(invoker) || climbs.inside(invoker, popover) {
                continue;
            }
            if let Some(dialog) = modal
                && !(climbs.inside(popover, dialog) && climbs.inside(invoker, dialog))
            {
                continue;
            }
            popovers.after.entry(invoker).or_default().push(popover);
            popovers.moved.insert(popover);
            popovers.layer_order.push(popover);
        }
        popovers
    }
}

/// The answers about the invokers of many popovers, each node climbed once:
/// "is it connected" (the DOM chain) and "is it a flat descendant of this
/// popover" (the flat tree chain). The second one uses the nearest popover
/// above each node: a popover is above a node if it is that nearest one or
/// above it, which the intervals of the tree of popovers tell.
struct Climbs<'a> {
    doc: &'a Document,
    /// The popovers that are asked about.
    popovers: HashSet<NodeId>,
    /// The nearest popover that is a flat ancestor of a node, for the nodes
    /// that were climbed.
    near: HashMap<NodeId, Option<NodeId>>,
    /// Whether a node is connected, for the nodes that were climbed.
    connected: HashMap<NodeId, bool>,
    /// The interval of each popover in the tree of popovers (a popover
    /// contains the popovers whose interval is inside its own); built when
    /// the first question about a popover comes.
    intervals: Option<HashMap<NodeId, (usize, usize)>>,
}

impl<'a> Climbs<'a> {
    fn new(doc: &'a Document, popovers: impl Iterator<Item = NodeId>) -> Climbs<'a> {
        Climbs {
            doc,
            popovers: popovers.collect(),
            near: HashMap::new(),
            connected: HashMap::new(),
            intervals: None,
        }
    }

    /// The nearest popover among the flat ancestors of `node`.
    fn nearest(&mut self, node: NodeId) -> Option<NodeId> {
        let mut path = Vec::new();
        let mut current = node;
        let found = loop {
            if let Some(&found) = self.near.get(&current) {
                break found;
            }
            path.push(current);
            match self.doc.flat_parent(current) {
                None => break None,
                Some(parent) if self.popovers.contains(&parent) => break Some(parent),
                Some(parent) => current = parent,
            }
        };
        // Every node of the path has the same nearest popover: the ones above
        // the first are not popovers themselves.
        for passed in path {
            self.near.insert(passed, found);
        }
        found
    }

    /// DOM "connected" for `node`.
    fn connected(&mut self, node: NodeId) -> bool {
        let mut path = Vec::new();
        let mut current = node;
        let found = loop {
            if let Some(&found) = self.connected.get(&current) {
                break found;
            }
            path.push(current);
            match self.doc.parent(current) {
                Some(parent) => current = parent,
                // The root of a tree: a document node, or a shadow root
                // whose host decides.
                None => break self.doc.is_connected(current),
            }
        };
        for passed in path {
            self.connected.insert(passed, found);
        }
        found
    }

    /// True if `popover` is a flat ancestor of `node` (not `node` itself).
    fn inside(&mut self, node: NodeId, popover: NodeId) -> bool {
        let Some(nearest) = self.nearest(node) else {
            return false;
        };
        let intervals = self.intervals();
        match (intervals.get(&popover), intervals.get(&nearest)) {
            (Some(outer), Some(inner)) => outer.0 <= inner.0 && inner.1 <= outer.1,
            _ => false,
        }
    }

    /// The intervals of the popovers in the tree that the nearest-popover
    /// links make (a depth-first numbering).
    fn intervals(&mut self) -> &HashMap<NodeId, (usize, usize)> {
        if self.intervals.is_none() {
            let popovers: Vec<NodeId> = self.popovers.iter().copied().collect();
            let mut children: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
            let mut roots = Vec::new();
            for popover in popovers {
                match self.nearest(popover) {
                    Some(parent) => children.entry(parent).or_default().push(popover),
                    None => roots.push(popover),
                }
            }
            self.intervals = Some(number(&roots, &children));
        }
        self.intervals.get_or_insert_with(HashMap::new)
    }
}

/// A depth-first numbering of a forest: `(first, last)` of each node, where a
/// node's interval contains those of its descendants. Iterative.
fn number(
    roots: &[NodeId],
    children: &HashMap<NodeId, Vec<NodeId>>,
) -> HashMap<NodeId, (usize, usize)> {
    let mut intervals = HashMap::new();
    let mut first = HashMap::new();
    let mut counter = 0;
    // (node, true) enters it; (node, false) leaves it.
    let mut stack: Vec<(NodeId, bool)> = roots.iter().rev().map(|&r| (r, true)).collect();
    while let Some((node, enter)) = stack.pop() {
        if enter {
            first.insert(node, counter);
            counter += 1;
            stack.push((node, false));
            let below = children.get(&node).map_or(&[][..], Vec::as_slice);
            stack.extend(below.iter().rev().map(|&c| (c, true)));
        } else if let Some(&start) = first.get(&node) {
            intervals.insert(node, (start, counter));
            counter += 1;
        }
    }
    intervals
}

/// The entry of the owner `node` of a scope; `index` is its `tabindex` (none
/// if it is not in the order). `None` for an element with a negative
/// `tabindex`, which drops its scope.
fn owner_entry(
    doc: &Document,
    node: NodeId,
    index: Option<i32>,
    popovers: &Popovers,
) -> Option<Entry> {
    let group = |i: i32| if i > 0 { i } else { i32::MAX };
    let after = popovers.after.get(&node).cloned().unwrap_or_default();
    match index {
        Some(i) if i < 0 => None,
        // A host that delegates focus is not a focusable area, with a
        // `tabindex` or without (measured in Chromium 148, case
        // `tab-delegating-tabindex-back`): only its scope is in the
        // order, in the group of the `tabindex`.
        Some(i) => Some(Entry {
            group: group(i),
            node,
            owner: true,
            emit: !doc.delegates_focus(node),
            popovers: after,
        }),
        None => Some(Entry {
            group: i32::MAX,
            node,
            owner: true,
            emit: false,
            popovers: after,
        }),
    }
}

/// A step of the walk of a scope.
enum Visit {
    /// The node, before its children.
    Enter(NodeId),
    /// The node after its children: the popovers that follow it come now.
    After(NodeId),
}

/// The entries of the scope of `owner` (the document for
/// [`NodeId::DOCUMENT`]) in the order of the sequence: the elements of the
/// flat tree below it that no inner owner takes, with their owners, the
/// positive `tabindex` groups first in increasing order, then the zero
/// group, each in flat tree order. A negative `tabindex` removes an
/// element from the order, and an owner with one drops its scope. A showing
/// popover that follows its invoker is an owner after the invoker's
/// subtree. Iterative: a script can nest 100,000 scopes.
fn scope_entries(
    doc: &Document,
    disabled: &DisabledElements,
    popovers: &Popovers,
    owner: NodeId,
) -> Vec<Entry> {
    let mut entries = Vec::new();
    // Under a modal dialog the document's scope is the dialog and what it
    // holds: the inert rest of the tree is skipped at its root.
    let mut stack: Vec<Visit> = match popovers.modal.filter(|_| owner == NodeId::DOCUMENT) {
        Some(dialog) => vec![Visit::Enter(dialog)],
        None => doc.flat_children(owner).map(Visit::Enter).collect(),
    };
    stack.reverse();
    while let Some(visit) = stack.pop() {
        let node = match visit {
            Visit::After(node) => {
                for &popover in popovers.after.get(&node).map_or(&[][..], Vec::as_slice) {
                    let index = tab_index(doc, popover, || disabled.contains(popover));
                    entries.extend(owner_entry(doc, popover, index, popovers));
                }
                continue;
            }
            Visit::Enter(node) => node,
        };
        if doc.element(node).is_none() || popovers.moved.contains(&node) {
            continue;
        }
        let index = tab_index(doc, node, || disabled.contains(node));
        let group = |i: i32| if i > 0 { i } else { i32::MAX };
        if is_scope_owner(doc, node) {
            // The popovers that follow this owner come after its scope
            // ([`flattened_order`]).
            entries.extend(owner_entry(doc, node, index, popovers));
            continue;
        }
        if let Some(i) = index.filter(|&i| i >= 0) {
            entries.push(Entry {
                group: group(i),
                node,
                owner: false,
                emit: true,
                popovers: Vec::new(),
            });
        }
        if popovers.after.contains_key(&node) {
            stack.push(Visit::After(node));
        }
        let mark = stack.len();
        stack.extend(doc.flat_children(node).map(Visit::Enter));
        stack[mark..].reverse();
    }
    entries.sort_by_key(|e| e.group);
    entries
}

/// HTML "flattened tabindex-ordered focus navigation scope" of the
/// document: the rendered elements in the order of Tab, with the scope of
/// each owner in place of the owner (after it, if it is focusable).
/// <https://html.spec.whatwg.org/multipage/interaction.html#flattened-tabindex-ordered-focus-navigation-scope>
/// The scopes are expanded with a stack of iterators, not by recursion.
fn flattened_order(
    doc: &Document,
    rendered: &impl Fn(NodeId) -> bool,
    popovers: &Popovers,
) -> Vec<NodeId> {
    let disabled = DisabledElements::new(doc);
    let mut order = Vec::new();
    let mut reached = HashSet::new();
    // Popovers that the last pass pushed (one that its entry drops, a negative
    // `tabindex`, must not be pushed again).
    let mut tried = HashSet::new();
    let mut stack = vec![scope_entries(doc, &disabled, popovers, NodeId::DOCUMENT).into_iter()];
    loop {
        while let Some(frame) = stack.last_mut() {
            let Some(entry) = frame.next() else {
                stack.pop();
                continue;
            };
            // Each popover is visited once: invokers inside popovers that
            // follow each other would make a cycle (a hostile page).
            if popovers.moved.contains(&entry.node) && !reached.insert(entry.node) {
                continue;
            }
            if entry.emit && rendered(entry.node) {
                order.push(entry.node);
            }
            if entry.owner {
                // The popovers that follow the owner come after its scope:
                // their frames go below the scope's.
                for &popover in entry.popovers.iter().rev() {
                    if reached.contains(&popover) {
                        continue;
                    }
                    let index = tab_index(doc, popover, || disabled.contains(popover));
                    let owner = owner_entry(doc, popover, index, popovers);
                    stack.push(owner.into_iter().collect::<Vec<_>>().into_iter());
                }
                stack.push(scope_entries(doc, &disabled, popovers, entry.node).into_iter());
            }
        }
        // Popovers that nothing reached (their invokers follow each other)
        // come last, so that they stay in the order.
        let lost: Vec<NodeId> = popovers
            .layer_order
            .iter()
            .copied()
            .filter(|p| !reached.contains(p) && tried.insert(*p))
            .collect();
        if lost.is_empty() {
            break;
        }
        for popover in lost {
            let index = tab_index(doc, popover, || disabled.contains(popover));
            let owner = owner_entry(doc, popover, index, popovers);
            stack.push(owner.into_iter().collect::<Vec<_>>().into_iter());
        }
    }
    order
}

/// [`next_in_order`] for a document with shadow trees. An element that is
/// not in the order starts the navigation at its position in flat tree
/// order, as in a document without shadow trees: forward, the next element
/// of the zero group after it; backward, the last element of the order that
/// has a positive `tabindex` or comes before it.
fn next_in_scopes(
    doc: &Document,
    from: Option<NodeId>,
    backward: bool,
    rendered: &impl Fn(NodeId) -> bool,
    popovers: &Popovers,
) -> Option<NodeId> {
    let order = flattened_order(doc, rendered, popovers);
    let edge = || {
        if backward {
            order.last()
        } else {
            order.first()
        }
        .copied()
    };
    let Some(from) = from else {
        return edge();
    };
    if let Some(at) = order.iter().position(|&n| n == from) {
        return if backward {
            at.checked_sub(1).and_then(|i| order.get(i)).copied()
        } else {
            order.get(at + 1).copied()
        };
    }
    let mut position = HashMap::new();
    let mut stack = vec![NodeId::DOCUMENT];
    while let Some(node) = stack.pop() {
        position.insert(node, position.len());
        let mark = stack.len();
        stack.extend(doc.flat_children(node));
        stack[mark..].reverse();
    }
    let Some(&start) = position.get(&from) else {
        return edge();
    };
    let positive = |n: NodeId| tab_index(doc, n, || false).is_some_and(|i| i > 0);
    let after = |n: NodeId| position.get(&n).is_some_and(|&p| p > start);
    if backward {
        order
            .iter()
            .rev()
            .find(|&&n| positive(n) || !after(n))
            .copied()
    } else {
        order.iter().find(|&&n| !positive(n) && after(n)).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write as _;

    use swb_dom::parse_html;

    fn id(doc: &Document, id: &str) -> NodeId {
        doc.element_by_id(id).expect("element exists")
    }

    fn sequence(doc: &Document, backward: bool) -> Vec<String> {
        let mut out = Vec::new();
        let mut current = None;
        while let Some(next) = next_in_order(doc, current, backward, |_| true) {
            out.push(
                doc.element(next)
                    .and_then(|e| e.attr("id"))
                    .unwrap_or("?")
                    .to_owned(),
            );
            current = Some(next);
            assert!(out.len() < 100, "no end");
        }
        out
    }

    #[test]
    fn tab_order() {
        let doc = parse_html(
            "<a id=a href=x>a</a><a id=nohref>n</a><div id=t2 tabindex=2></div>\
             <button id=b></button><input id=h type=hidden><div id=neg tabindex=-1></div>\
             <div id=t1 tabindex=1></div><input id=i><button id=d disabled></button>\
             <details><summary id=s>s</summary><summary id=s2>s2</summary></details>",
        );
        assert_eq!(sequence(&doc, false), ["t1", "t2", "a", "b", "i", "s"]);
        assert_eq!(sequence(&doc, true), ["s", "i", "b", "a", "t2", "t1"]);
        assert!(is_focusable(&doc, id(&doc, "neg")));
        assert!(!is_focusable(&doc, id(&doc, "nohref")));
    }

    #[test]
    fn navigation_starts_at_a_node_that_is_not_focusable() {
        let doc = parse_html("<a id=a href=x>a</a><p id=p>text</p><a id=b href=x>b</a>");
        let p = id(&doc, "p");
        assert_eq!(
            next_in_order(&doc, Some(p), false, |_| true),
            Some(id(&doc, "b"))
        );
        assert_eq!(
            next_in_order(&doc, Some(p), true, |_| true),
            Some(id(&doc, "a"))
        );
    }

    /// A child of a shadow root of the tests: (tag, id, attributes).
    type Child<'a> = (&'a str, &'a str, &'a [(&'a str, &'a str)]);

    /// Attaches a shadow root to `host` and gives it `children`, in order,
    /// all children of the root.
    fn shadow(doc: &mut Document, host: &str, delegates: bool, children: &[Child<'_>]) {
        use swb_dom::{ShadowRootInit, ShadowRootMode, SlotAssignment};
        let host = id(doc, host);
        let init = ShadowRootInit {
            mode: ShadowRootMode::Open,
            slot_assignment: SlotAssignment::Named,
            delegates_focus: delegates,
            clonable: false,
            serializable: false,
        };
        let root = doc.attach_shadow(host, init, false).unwrap();
        for (tag, element_id, attrs) in children {
            let name = swb_dom::QualName::new(None, swb_dom::ns!(html), (*tag).into());
            let e = doc.create_element(name, Vec::new()).unwrap();
            if !element_id.is_empty() {
                doc.set_attr(e, "id", element_id).unwrap();
            }
            for (k, v) in *attrs {
                doc.set_attr(e, k, v).unwrap();
            }
            doc.append_child(root, e).unwrap();
        }
    }

    /// The case `tab-order` of `tools/probes/web-shadow-dom.json`, with
    /// Chromium 148's order: the scope of a host stands in its place (a
    /// focusable host comes before its scope), a negative `tabindex` of a
    /// host drops its scope, the slots are owners (the light children come
    /// in the scope of their slot), the positive groups come first within
    /// each scope, and the host that delegates focus is not in the order.
    fn tab_order_page() -> Document {
        let mut doc = parse_html(
            "<button id=b1>1</button><div id=h1></div><button id=b2>2</button>\
             <div id=h2 tabindex=0>h2</div><button id=b3 tabindex=1>3</button>\
             <div id=h3 tabindex=-1></div><button id=b4>4</button>\
             <div id=h4><button id=l1 slot=x>L1</button><button id=l2>L2</button></div>\
             <div id=h5></div><button id=b5>5</button>",
        );
        let b: &str = "button";
        shadow(
            &mut doc,
            "h1",
            false,
            &[
                (b, "s1", &[]),
                (b, "s2", &[("tabindex", "2")]),
                (b, "s3", &[("tabindex", "1")]),
            ],
        );
        shadow(&mut doc, "h2", false, &[(b, "s4", &[])]);
        shadow(&mut doc, "h3", false, &[(b, "s5", &[])]);
        shadow(
            &mut doc,
            "h4",
            false,
            &[
                (b, "s6", &[]),
                ("slot", "", &[("name", "x")]),
                (b, "s7", &[]),
                ("slot", "", &[]),
            ],
        );
        shadow(&mut doc, "h5", true, &[(b, "s8", &[])]);
        doc
    }

    /// The element with the ID `name` in the document or a shadow tree.
    fn in_any_tree(doc: &Document, name: &str) -> NodeId {
        doc.shadow_including_descendants(NodeId::DOCUMENT)
            .find(|&n| doc.element(n).and_then(|e| e.attr("id")) == Some(name))
            .expect("element exists")
    }

    fn ids_of(doc: &Document, nodes: &[NodeId]) -> Vec<String> {
        nodes
            .iter()
            .map(|&n| {
                doc.element(n)
                    .and_then(|e| e.attr("id"))
                    .unwrap_or("?")
                    .to_owned()
            })
            .collect()
    }

    #[test]
    fn scopes_order_the_tab_sequence() {
        let doc = tab_order_page();
        let forward = std::iter::successors(next_in_order(&doc, None, false, |_| true), |&n| {
            next_in_order(&doc, Some(n), false, |_| true)
        })
        .collect::<Vec<_>>();
        assert_eq!(
            ids_of(&doc, &forward),
            "b3 b1 s3 s2 s1 b2 h2 s4 b4 s6 l1 s7 l2 s8 b5"
                .split(' ')
                .collect::<Vec<_>>()
        );
        let mut backward = std::iter::successors(next_in_order(&doc, None, true, |_| true), |&n| {
            next_in_order(&doc, Some(n), true, |_| true)
        })
        .collect::<Vec<_>>();
        backward.reverse();
        assert_eq!(backward, forward);
    }

    /// A host that delegates focus is not in the order even with a
    /// `tabindex`: Shift+Tab from its delegate goes to the element before the
    /// host, not to the host (a trap in the first version). The order is that
    /// of the case `tab-delegating-tabindex-back` in Chromium 148.
    #[test]
    fn a_delegating_host_with_tabindex_is_not_in_the_order() {
        let mut doc = parse_html(
            "<button id=b1>1</button><div id=h tabindex=0></div><div id=g tabindex=-1></div>\
             <div id=j tabindex=3></div><button id=b2>2</button>",
        );
        let b: &str = "button";
        shadow(&mut doc, "h", true, &[(b, "s1", &[])]);
        shadow(&mut doc, "g", true, &[(b, "s2", &[])]);
        shadow(&mut doc, "j", true, &[(b, "s3", &[])]);
        let walk = |backward: bool| {
            let mut order = Vec::new();
            let mut at = None;
            while let Some(n) = next_in_order(&doc, at, backward, |_| true) {
                order.push(n);
                at = Some(n);
            }
            order
        };
        let forward = walk(false);
        assert_eq!(ids_of(&doc, &forward), ["s3", "b1", "s1", "b2"]);
        let mut backward = walk(true);
        backward.reverse();
        assert_eq!(backward, forward);
        // Backward from the delegate of the host: the button before it.
        assert_eq!(
            next_in_order(&doc, Some(in_any_tree(&doc, "s1")), true, |_| true),
            Some(id(&doc, "b1"))
        );
    }

    /// A node that is not in the order starts at its flat tree position; a
    /// node that is not rendered is left out; without shadow trees the
    /// plain order stays.
    #[test]
    fn scopes_start_at_the_flat_position_and_skip_unrendered() {
        let doc = tab_order_page();
        let s4 = in_any_tree(&doc, "s4");
        let h3 = id(&doc, "h3");
        // `h3` has `tabindex=-1`: not in the order; its place is between
        // `h2`'s scope and `b4`.
        assert_eq!(
            next_in_order(&doc, Some(h3), false, |_| true),
            Some(id(&doc, "b4"))
        );
        assert_eq!(next_in_order(&doc, Some(h3), true, |_| true), Some(s4));
        let skip = in_any_tree(&doc, "s1");
        let order = |from| next_in_order(&doc, from, false, |n| n != skip);
        assert_eq!(order(Some(in_any_tree(&doc, "s2"))), Some(id(&doc, "b2")));
        // A focusable element in a deep chain of scopes is in the order.
        let mut doc = parse_html("<div id=a></div>");
        shadow(
            &mut doc,
            "a",
            false,
            &[("div", "inner", &[]), ("button", "x", &[])],
        );
        let mut seen = Vec::new();
        let mut at = None;
        while let Some(n) = next_in_order(&doc, at, false, |_| true) {
            seen.push(n);
            at = Some(n);
        }
        assert_eq!(ids_of(&doc, &seen), ["x"]);
    }

    #[test]
    fn unrendered_elements_are_skipped() {
        let doc = parse_html("<a id=a href=x>a</a><a id=b href=x>b</a>");
        let b = id(&doc, "b");
        assert_eq!(next_in_order(&doc, None, false, |n| n == b), Some(b));
        assert_eq!(click_target(&doc, b, |n| n != b), None);
    }

    #[test]
    fn click_focuses_the_nearest_focusable_ancestor() {
        let doc = parse_html("<a id=a href=x><span id=s>text</span></a><p id=p>x</p>");
        let s = id(&doc, "s");
        assert_eq!(click_target(&doc, s, |_| true), Some(id(&doc, "a")));
        assert_eq!(click_target(&doc, id(&doc, "p"), |_| true), None);
    }

    #[test]
    fn disabled_controls_are_not_focusable() {
        let doc = parse_html(
            "<button id=b disabled tabindex=0></button>\
             <fieldset disabled><legend><input id=l></legend><input id=f>\
             <legend><input id=l2></legend></fieldset><div id=d disabled tabindex=0></div>",
        );
        assert!(!is_focusable(&doc, id(&doc, "b")));
        assert!(is_focusable(&doc, id(&doc, "l")));
        assert!(!is_focusable(&doc, id(&doc, "f")));
        assert!(!is_focusable(&doc, id(&doc, "l2")));
        // `disabled` means nothing on a div.
        assert!(is_focusable(&doc, id(&doc, "d")));
    }

    #[test]
    fn tab_index_parsing() {
        assert_eq!(parse_tab_index(" 3"), Some(3));
        assert_eq!(parse_tab_index("-1"), Some(-1));
        assert_eq!(parse_tab_index("+2x"), Some(2));
        assert_eq!(parse_tab_index("x"), None);
        assert_eq!(parse_tab_index(""), None);
        assert_eq!(parse_tab_index("99999999999"), Some(i32::MAX));
        assert_eq!(parse_tab_index("-99999999999999"), Some(i32::MIN));
        assert_eq!(parse_tab_index("0000000000001"), Some(1));
        assert_eq!(parse_tab_index("000"), Some(0));
    }

    /// A host that shows popovers without events (the tests of the order).
    struct Show(Document);

    impl swb_dom::TopLayerHost for Show {
        fn doc(&mut self) -> &mut Document {
            &mut self.0
        }

        fn fire_beforetoggle(
            &mut self,
            _: NodeId,
            _: swb_dom::ToggleState,
            _: swb_dom::ToggleState,
            _: bool,
            _: Option<NodeId>,
        ) -> Option<bool> {
            Some(true)
        }

        fn queue_toggle_task(&mut self, element: NodeId, serial: u64) {
            let _ = self.0.take_toggle(element, serial);
        }
    }

    /// Shows `popover` with `source` (an element id) as its invoker.
    fn show(doc: &mut Document, popover: &str, source: Option<&str>) {
        let (p, s) = (id(doc, popover), source.map(|s| id(doc, s)));
        let mut host = Show(std::mem::take(doc));
        swb_dom::show_popover(&mut host, p, true, s).unwrap();
        *doc = host.0;
    }

    /// The content of a showing popover follows its invoker in the Tab
    /// order, forward and backward (Chromium 148, `popover-tab-order`), and
    /// is not visited at its place in the tree.
    #[test]
    fn a_popover_follows_its_invoker() {
        let mut doc = parse_html(
            "<button id=x>x</button><button id=inv popovertarget=pp>inv</button>\
             <button id=y>y</button><div id=pp popover><button id=b1>1</button>\
             <button id=b2>2</button></div><button id=z>z</button>",
        );
        show(&mut doc, "pp", Some("inv"));
        assert_eq!(sequence(&doc, false), ["x", "inv", "b1", "b2", "y", "z"]);
        assert_eq!(sequence(&doc, true), ["z", "y", "b2", "b1", "inv", "x"]);
        // From inside the popover the order goes on after the invoker.
        let b2 = id(&doc, "b2");
        let next = next_in_order(&doc, Some(b2), false, |_| true);
        assert_eq!(next, Some(id(&doc, "y")));
        let back = next_in_order(&doc, Some(id(&doc, "b1")), true, |_| true);
        assert_eq!(back, Some(id(&doc, "inv")));
    }

    /// Without an invoker (a script showed it) or with a manual popover the
    /// rules differ only in the invoker: no invoker, the tree place.
    #[test]
    fn a_popover_without_an_invoker_stays_in_the_tree() {
        let mut doc = parse_html(
            "<button id=x>x</button><button id=inv popovertarget=pp>inv</button>\
             <div id=pp popover><button id=b1>1</button></div><button id=z>z</button>",
        );
        show(&mut doc, "pp", None);
        assert_eq!(sequence(&doc, false), ["x", "inv", "b1", "z"]);
        let mut doc = parse_html(
            "<button id=inv>inv</button><button id=y>y</button>\
             <div id=pp popover=manual><button id=b1>1</button></div>",
        );
        show(&mut doc, "pp", Some("inv"));
        assert_eq!(sequence(&doc, false), ["inv", "b1", "y"]);
    }

    /// The popover stands at the place of its invoker, after the invoker's
    /// subtree, also when the invoker cannot be focused.
    #[test]
    fn a_popover_stands_after_a_source_that_is_not_focusable() {
        let mut doc = parse_html(
            "<button id=x>x</button><div id=s><i>t</i></div><button id=y>y</button>\
             <div id=pp popover><button id=b1>1</button></div>",
        );
        show(&mut doc, "pp", Some("s"));
        assert_eq!(sequence(&doc, false), ["x", "b1", "y"]);
        let mut doc = parse_html(
            "<div id=s><button id=x>x</button><button id=y>y</button></div>\
             <button id=z>z</button><div id=pp popover><button id=b1>1</button></div>",
        );
        show(&mut doc, "pp", Some("s"));
        assert_eq!(sequence(&doc, false), ["x", "y", "b1", "z"]);
    }

    /// Opens the dialog `dialog` as a modal dialog, without events.
    fn show_modal(doc: &mut Document, dialog: &str) {
        let d = id(doc, dialog);
        let mut host = Show(std::mem::take(doc));
        swb_dom::show_modal(&mut host, d, None).unwrap();
        *doc = host.0;
    }

    /// Under a modal dialog the order is the dialog's, and a start outside
    /// it counts as no start (`inert-tab`, `e-tab-backward`; ADR 0037
    /// section 5).
    #[test]
    fn a_modal_dialog_has_its_own_tab_order() {
        let mut doc = parse_html(
            "<button id=x>x</button><dialog id=d><button id=a>a</button>\
             <button id=b>b</button></dialog><button id=y>y</button>",
        );
        // (Every element counts as rendered here.)
        assert_eq!(sequence(&doc, false), ["x", "a", "b", "y"]);
        show_modal(&mut doc, "d");
        assert_eq!(sequence(&doc, false), ["a", "b"]);
        assert_eq!(sequence(&doc, true), ["b", "a"]);
        // A start outside the dialog enters it at the edge; its own end ends
        // the order.
        let x = Some(id(&doc, "x"));
        assert_eq!(next_in_order(&doc, x, false, |_| true), Some(id(&doc, "a")));
        assert_eq!(next_in_order(&doc, x, true, |_| true), Some(id(&doc, "b")));
        let b = Some(id(&doc, "b"));
        assert_eq!(next_in_order(&doc, b, false, |_| true), None);
        // Nothing outside is focusable; inside, the dialog's controls are.
        assert!(!is_focusable(&doc, id(&doc, "x")));
        assert!(!is_focusable(&doc, id(&doc, "y")));
        assert!(is_focusable(&doc, id(&doc, "a")));
        // The same in a document with a shadow tree (the walk of scopes): a
        // start in the inert tree, in the light tree or the shadow tree,
        // enters at the edge in both directions.
        let mut doc = parse_html(
            "<button id=x>x</button><div id=h></div><dialog id=d><button id=a>a</button>\
             <button id=b>b</button></dialog>",
        );
        shadow(&mut doc, "h", false, &[("button", "sh", &[])]);
        show_modal(&mut doc, "d");
        assert_eq!(sequence(&doc, false), ["a", "b"]);
        assert_eq!(sequence(&doc, true), ["b", "a"]);
        for start in [
            id(&doc, "x"),
            doc.flat_children(id(&doc, "h")).next().unwrap(),
        ] {
            let from = Some(start);
            assert_eq!(
                next_in_order(&doc, from, false, |_| true),
                Some(id(&doc, "a"))
            );
            assert_eq!(
                next_in_order(&doc, from, true, |_| true),
                Some(id(&doc, "b"))
            );
        }
    }

    /// A popover in the modal dialog follows its invoker in the dialog; one
    /// whose invoker is inert stays at its place in the dialog; one outside
    /// the dialog is not visited, also with an invoker inside it. Also in a
    /// document with a shadow tree (the walk of scopes).
    #[test]
    fn popovers_under_a_modal_dialog() {
        let page = "<button id=x>x</button><div id=out popover=manual><button id=ob>ob</button></div>\
             <dialog id=d><button id=inv>inv</button><button id=a>a</button>\
             <div id=in popover=manual><button id=ib>ib</button></div></dialog>{shadow}";
        for shadow in ["", "<div id=h></div>"] {
            let mut doc = parse_html(&page.replace("{shadow}", shadow));
            if !shadow.is_empty() {
                self::shadow(&mut doc, "h", false, &[("button", "sh", &[])]);
            }
            show_modal(&mut doc, "d");
            // The popover inside, with its invoker inside: after the invoker.
            show(&mut doc, "in", Some("inv"));
            assert_eq!(sequence(&doc, false), ["inv", "ib", "a"], "{shadow}");
            // The popover outside, with an invoker inside: not visited.
            show(&mut doc, "out", Some("a"));
            assert_eq!(sequence(&doc, false), ["inv", "ib", "a"], "{shadow}");
        }
        // The popover inside with an invoker outside (inert): at its place.
        // It comes first in the dialog, so that a popover that is lost or
        // appended at the end gives another sequence (`x-popover-inert-
        // invoker-f` and `-b` of `tools/probes/web-top-layer.json`).
        let mut doc = parse_html(
            "<button id=x>x</button><dialog id=d><div id=in popover=manual>\
             <button id=ib>ib</button></div><button id=inv>inv</button>\
             <button id=a>a</button></dialog>",
        );
        show_modal(&mut doc, "d");
        show(&mut doc, "in", Some("x"));
        assert_eq!(sequence(&doc, false), ["ib", "inv", "a"]);
        assert_eq!(sequence(&doc, true), ["a", "inv", "ib"]);
    }

    /// Nested popovers follow their invokers inside the scope of the outer
    /// one; two invokers of the same popover: only the source counts.
    #[test]
    fn nested_popovers_follow_their_invokers_in_their_scopes() {
        let mut doc = parse_html(
            "<button id=x>x</button><button id=inv>inv</button><button id=y>y</button>\
             <div id=p1 popover><button id=a1>a1</button><button id=inv2>inv2</button>\
             <button id=a2>a2</button><div id=p2 popover><button id=c1>c1</button></div></div>\
             <button id=z>z</button>",
        );
        show(&mut doc, "p1", Some("inv"));
        show(&mut doc, "p2", Some("inv2"));
        assert_eq!(
            sequence(&doc, false),
            ["x", "inv", "a1", "inv2", "c1", "a2", "y", "z"]
        );
        assert_eq!(
            sequence(&doc, true),
            ["z", "y", "a2", "c1", "inv2", "a1", "inv", "x"]
        );
    }

    /// An invoker inside its own popover does not move it; two popovers that
    /// follow invokers inside each other stay in the order.
    #[test]
    fn no_popover_is_lost() {
        let mut doc =
            parse_html("<button id=x>x</button><div id=p1 popover><button id=i1>i1</button></div>");
        show(&mut doc, "p1", Some("i1"));
        assert_eq!(sequence(&doc, false), ["x", "i1"]);
        let mut doc = parse_html(
            "<div id=p1 popover=manual><button id=i1>i1</button></div>\
             <div id=p2 popover=manual><button id=i2>i2</button></div>",
        );
        show(&mut doc, "p1", Some("i2"));
        show(&mut doc, "p2", Some("i1"));
        let mut order = sequence(&doc, false);
        order.sort();
        assert_eq!(order, ["i1", "i2"]);
    }

    /// A popover that is its own invoker stays at its place in the tree.
    #[test]
    fn a_popover_that_is_its_own_invoker_stays_in_the_tree() {
        let mut doc = parse_html(
            "<div id=pp popover=manual><button id=b1>1</button></div><button id=z>z</button>",
        );
        show(&mut doc, "pp", Some("pp"));
        assert_eq!(sequence(&doc, false), ["b1", "z"]);
    }

    /// The walk ends for every tree that a page can build (each popover is
    /// visited once): a source that is an ancestor or a descendant, invokers
    /// in each other's popovers, a popover with a negative `tabindex`, a
    /// cycle of three.
    #[test]
    fn the_walk_ends_for_hostile_invokers() {
        let html = "<body id=bd><button id=i1>1</button><div id=p1 popover=manual tabindex=-1>\
            <button id=i2>2</button></div><div id=p2 popover=manual><button id=i3>3</button>\
            <div id=p3 popover=manual><button id=i4>4</button></div></div></body>";
        let cases: &[&[(&str, &str)]] = &[
            &[("p1", "bd"), ("p2", "bd")],
            &[("p2", "p3"), ("p3", "i3")],
            &[("p1", "i3"), ("p2", "i4"), ("p3", "i2")],
            &[("p1", "p2"), ("p2", "p1"), ("p3", "p3")],
            &[("p1", "i2"), ("p2", "i1"), ("p3", "bd")],
        ];
        for case in cases {
            let mut doc = parse_html(html);
            for &(popover, source) in *case {
                show(&mut doc, popover, Some(source));
            }
            let order = flattened_order(&doc, &|_| true, &Popovers::of(&doc));
            let mut sorted = order.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(sorted.len(), order.len(), "{case:?}: an element twice");
        }
    }

    /// The popovers that follow their invokers by the plain definition: one
    /// walk up the DOM and one up the flat tree for each popover (the
    /// implementation that [`Popovers::of`] replaces).
    fn moved_by_definition(doc: &Document) -> Vec<NodeId> {
        let Some(state) = doc.top_layer() else {
            return Vec::new();
        };
        state
            .layer()
            .iter()
            .copied()
            .filter(|&popover| doc.is_popover_showing(popover))
            .filter(|&popover| {
                let Some(invoker) = doc.popover_invoker(popover) else {
                    return false;
                };
                invoker != popover
                    && doc.element(invoker).is_some()
                    && doc.is_connected(invoker)
                    && !doc.flat_ancestors(invoker).any(|a| a == popover)
            })
            .collect()
    }

    /// `Popovers::of` answers as the plain definition does: nested popovers
    /// with sources above, below and beside them, invokers in a shadow tree
    /// and in content that no slot takes, a removed invoker, a chain.
    #[test]
    fn the_popovers_that_follow_agree_with_the_plain_definition() {
        let html = "<button id=i0>0</button><div id=p1 popover=manual><button id=i1>1</button>\
            <div id=p2 popover=manual><button id=i2>2</button><div id=p3 popover=manual>\
            <button id=i3>3</button></div></div></div><div id=p4 popover=manual>\
            <button id=i4>4</button></div><div id=host><template shadowrootmode=open>\
            <button id=si>s</button><slot name=s></slot></template><button id=li>l</button>\
            <div id=p5 popover=manual><button id=i5>5</button></div></div>";
        let cases: &[&[(&str, &str)]] = &[
            &[("p1", "i0"), ("p2", "i1"), ("p3", "i2")],
            &[("p1", "i3"), ("p2", "i3"), ("p3", "i0")],
            &[("p1", "i4"), ("p4", "i1"), ("p2", "i2"), ("p3", "i3")],
            &[("p1", "p2"), ("p2", "p1"), ("p3", "p3")],
            &[("p4", "i1"), ("p1", "i4")],
            &[("p5", "li"), ("p1", "li")],
            &[("p5", "i5"), ("p4", "i5"), ("p1", "i5")],
            &[("p1", "host"), ("p4", "host"), ("p5", "host")],
            &[("p1", "i5"), ("p5", "i1")],
        ];
        for case in cases {
            let mut doc = parse_html(html);
            for &(popover, source) in *case {
                show(&mut doc, popover, Some(source));
            }
            let want = moved_by_definition(&doc);
            assert_eq!(Popovers::of(&doc).layer_order, want, "{case:?}");
        }
        // A removed invoker, and a chain of nested popovers with sources.
        let mut doc = parse_html(html);
        show(&mut doc, "p4", Some("i0"));
        let i0 = id(&doc, "i0");
        doc.remove(i0);
        assert_eq!(Popovers::of(&doc).layer_order, moved_by_definition(&doc));
        let mut chain = String::new();
        for i in 0..300 {
            write!(
                chain,
                "<div id=q{i} popover=manual><button id=c{i}>c</button>"
            )
            .unwrap();
        }
        chain.push_str(&"</div>".repeat(300));
        let mut doc = parse_html(&chain);
        for i in 1..300 {
            show(&mut doc, &format!("q{i}"), Some(&format!("c{}", i - 1)));
        }
        let popovers = Popovers::of(&doc);
        assert_eq!(popovers.layer_order.len(), 299);
        assert_eq!(popovers.layer_order, moved_by_definition(&doc));
    }

    /// A popover whose invoker left the document stays at its place.
    #[test]
    fn a_popover_whose_invoker_is_gone_stays_in_the_tree() {
        let mut doc = parse_html(
            "<button id=x>x</button><button id=inv>inv</button>\
             <div id=pp popover><button id=b1>1</button></div><button id=z>z</button>",
        );
        show(&mut doc, "pp", Some("inv"));
        let inv = id(&doc, "inv");
        doc.remove(inv);
        assert_eq!(sequence(&doc, false), ["x", "b1", "z"]);
    }

    /// 10,000 popovers that follow one invoker each give an order in linear
    /// time.
    #[test]
    fn many_popovers_give_an_order_in_linear_time() {
        let mut html = String::new();
        for i in 0..2000 {
            write!(html, "<button id=i{i}>i</button>").unwrap();
        }
        for i in 0..2000 {
            write!(
                html,
                "<div id=p{i} popover=manual><button id=b{i}>b</button></div>"
            )
            .unwrap();
        }
        let mut doc = parse_html(&html);
        for i in 0..2000 {
            show(&mut doc, &format!("p{i}"), Some(&format!("i{i}")));
        }
        let started = std::time::Instant::now();
        let order = sequence_limit(&doc, 5000);
        assert_eq!(order.len(), 4000);
        assert!(started.elapsed() < std::time::Duration::from_secs(20));
    }

    /// The sequence of Tab, at most `limit` elements (for a long order).
    fn sequence_limit(doc: &Document, limit: usize) -> Vec<NodeId> {
        let order = flattened_order(doc, &|_| true, &Popovers::of(doc));
        assert!(order.len() <= limit);
        order
    }
}
