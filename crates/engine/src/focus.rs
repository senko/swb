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
/// click's (composed) event path that is focusable and rendered; a shadow
/// host that delegates focus, reached without one, gives its focus delegate
/// ([`focus_delegate`], ADR 0036 section 8).
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
                (is_focusable(doc, n) && rendered(n)).then_some(n)
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
/// rendered.
pub(crate) fn is_focusable(doc: &Document, node: NodeId) -> bool {
    tab_index(doc, node, || is_actually_disabled(doc, node)).is_some()
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
    if doc.has_shadow_trees() {
        return next_in_scopes(doc, from, backward, &rendered);
    }
    let disabled = DisabledElements::new(doc);
    // (tabindex group, tree position, node); group 0 is tabindex 0.
    let mut order: Vec<(i32, usize, NodeId)> = Vec::new();
    let mut from_key = None;
    for (position, node) in doc.descendants(NodeId::DOCUMENT).enumerate() {
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
}

/// True if `node` owns a focus navigation scope: a shadow host, or a slot in
/// a shadow tree (HTML "focus navigation scope owner",
/// <https://html.spec.whatwg.org/multipage/interaction.html#focus-navigation-scope-owner>).
fn is_scope_owner(doc: &Document, node: NodeId) -> bool {
    doc.shadow_root(node).is_some()
        || (doc.is_html_element(node, &local_name!("slot")) && doc.in_shadow_tree(node))
}

/// The entries of the scope of `owner` (the document for
/// [`NodeId::DOCUMENT`]) in the order of the sequence: the elements of the
/// flat tree below it that no inner owner takes, with their owners, the
/// positive `tabindex` groups first in increasing order, then the zero
/// group, each in flat tree order. A negative `tabindex` removes an
/// element from the order, and an owner with one drops its scope. Iterative:
/// a script can nest 100,000 scopes.
fn scope_entries(doc: &Document, disabled: &DisabledElements, owner: NodeId) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut stack: Vec<NodeId> = doc.flat_children(owner).collect();
    stack.reverse();
    while let Some(node) = stack.pop() {
        if doc.element(node).is_none() {
            continue;
        }
        let index = tab_index(doc, node, || disabled.contains(node));
        let group = |i: i32| if i > 0 { i } else { i32::MAX };
        if is_scope_owner(doc, node) {
            match index {
                Some(i) if i < 0 => {}
                // A host that delegates focus is not a focusable area, with a
                // `tabindex` or without (measured in Chromium 148, case
                // `tab-delegating-tabindex-back`): only its scope is in the
                // order, in the group of the `tabindex`.
                Some(i) => entries.push(Entry {
                    group: group(i),
                    node,
                    owner: true,
                    emit: !doc.delegates_focus(node),
                }),
                None => entries.push(Entry {
                    group: i32::MAX,
                    node,
                    owner: true,
                    emit: false,
                }),
            }
            continue;
        }
        if let Some(i) = index.filter(|&i| i >= 0) {
            entries.push(Entry {
                group: group(i),
                node,
                owner: false,
                emit: true,
            });
        }
        let mark = stack.len();
        stack.extend(doc.flat_children(node));
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
fn flattened_order(doc: &Document, rendered: &impl Fn(NodeId) -> bool) -> Vec<NodeId> {
    let disabled = DisabledElements::new(doc);
    let mut order = Vec::new();
    let mut stack = vec![scope_entries(doc, &disabled, NodeId::DOCUMENT).into_iter()];
    while let Some(frame) = stack.last_mut() {
        let Some(entry) = frame.next() else {
            stack.pop();
            continue;
        };
        if entry.emit && rendered(entry.node) {
            order.push(entry.node);
        }
        if entry.owner {
            stack.push(scope_entries(doc, &disabled, entry.node).into_iter());
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
) -> Option<NodeId> {
    let order = flattened_order(doc, rendered);
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
    let mut position = std::collections::HashMap::new();
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
}
