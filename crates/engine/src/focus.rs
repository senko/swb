//! Focus: which elements can be focused, and the sequential focus order
//! (the Tab key).
//!
//! <https://html.spec.whatwg.org/multipage/interaction.html#focusable-area>
//! and
//! <https://html.spec.whatwg.org/multipage/interaction.html#sequential-focus-navigation>.
//! Deliberate simplifications: no focus navigation scopes (shadow trees,
//! iframes), and `contenteditable` elements are not focusable (swb edits
//! only form controls).

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
        return false;
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
        local_name!("summary") => is_details_summary(doc, node),
        _ => false,
    }
}

/// True if `node` is the first `summary` child of a `details` element.
fn is_details_summary(doc: &Document, node: NodeId) -> bool {
    doc.parent(node).is_some_and(|parent| {
        doc.is_html_element(parent, &local_name!("details"))
            && doc
                .element_children(parent)
                .find(|&c| doc.is_html_element(c, &local_name!("summary")))
                == Some(node)
    })
}

/// The element that a click on `node` focuses: the nearest inclusive
/// ancestor that is focusable and rendered.
pub(crate) fn click_target(
    doc: &Document,
    node: NodeId,
    rendered: impl Fn(NodeId) -> bool,
) -> Option<NodeId> {
    std::iter::once(node)
        .chain(doc.ancestors(node))
        .find(|&n| is_focusable(doc, n) && rendered(n))
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
/// element from the order.
pub(crate) fn next_in_order(
    doc: &Document,
    from: Option<NodeId>,
    backward: bool,
    rendered: impl Fn(NodeId) -> bool,
) -> Option<NodeId> {
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
