//! Which element a `click` activates when interactive elements are nested
//! (M9, the BBC side menu; ADR 0028 "Nested activation").
//!
//! The DOM standard's "dispatch" makes the nearest inclusive ancestor of the
//! target that has an activation behavior the activation target
//! (<https://dom.spec.whatwg.org/#concept-event-dispatch>, steps 5.9 to 5.14).
//! Chromium agrees in most nestings and differs in the ones below. The rule
//! is measured with Chromium 148
//! (`tools/probes/web-nested-activation.json`, 548 cases, the click of a
//! pointer, `click()`, `dispatchEvent` and keys). Walking up from the target:
//!
//! - A checkbox, a radio button and a submit or reset button that acts (it has
//!   a form owner and is enabled) are the target. A button that does not act
//!   (`type=button`, no form owner, disabled) is skipped, and a link around it
//!   is followed: the click on BBC's `a > button > span` follows the `a`.
//! - A link is the target, and so is a `label` with a control. A `label`
//!   or a `summary` that has interactive content (a field, a button that does
//!   not act, a `select`, ...) between itself and the target does nothing; a
//!   link is not stopped by it.
//! - Departure from the standard: a link or a label that is not the target
//!   itself, inside a button that acts, does nothing and the button acts
//!   (`C > a > span`: the form is submitted; `C > a`, the target being the
//!   link: the link is followed). A `summary` is not stopped by such a
//!   button.
//! - Departure from the standard: when a label acts and a button that acts
//!   is above it, the button acts after the label.
//!
//! The roles of the elements come from the caller, which knows its form
//! controls (the scripts' DOM and the engine's form state differ in how they
//! tell the form owner and the disabled state).

use crate::{Document, NodeId};

/// What an element does with a click that reaches it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickRole {
    /// Nothing, and the element is not interactive content.
    Plain,
    /// Interactive content without an activation behavior that a click
    /// reaches: a text field, a `select`, a button that does not act.
    Interactive,
    /// A link: `a`, `area` or SVG `a` with `href`.
    Link,
    /// A `label` that has a labeled control.
    Label,
    /// A `summary` of a `details` element, also the default summary.
    Summary,
    /// A checkbox or a radio button.
    Toggle,
    /// A submit or reset button that acts.
    Button,
    /// A disabled control: the click goes no further (a real click on it
    /// fires no events, so nothing around it activates).
    Blocked,
}

/// The element that a click activates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClickTarget {
    /// The element.
    pub node: NodeId,
    /// Its role.
    pub role: ClickRole,
    /// For a label: the button that acts after the label, if one is above it.
    pub also: Option<NodeId>,
}

/// The roles of the labels on one walk, found without a search of the
/// subtree for each label (a page can nest thousands of labels).
///
/// A label without a `for` attribute has a control if a labelable element is
/// among its descendants. If such a label has one, so has every label
/// without `for` above it on the path. The memo keeps that fact; the caller
/// makes one memo for each walk.
#[derive(Debug, Default)]
pub struct LabelMemo {
    control_inside: bool,
}

impl LabelMemo {
    /// The role of a `label`: `Label` if `has_control()` is true, else
    /// `Interactive`. `has_for` tells if the label has a `for` attribute
    /// (its control is then not among its descendants, and `has_control` is
    /// cheap).
    pub fn role(&mut self, has_for: bool, has_control: impl FnOnce() -> bool) -> ClickRole {
        if !has_for && self.control_inside {
            return ClickRole::Label;
        }
        if has_control() {
            self.control_inside |= !has_for;
            ClickRole::Label
        } else {
            ClickRole::Interactive
        }
    }
}

/// The nearest button that acts above the walk, found once.
enum Above {
    Unknown,
    Found(Option<NodeId>),
}

/// The element that a click at `target` activates, or `None`. With
/// `bubbles` false the target itself is the only candidate. `role_of` tells
/// the role of each element on the path; its second argument is true when the
/// answer for a `label` does not matter (interactive content is below it, or
/// only a button is looked for): it may then skip the search for the control
/// of a label and return `Plain`. See [`LabelMemo`].
pub fn click_target(
    dom: &Document,
    target: NodeId,
    bubbles: bool,
    mut role_of: impl FnMut(NodeId, bool) -> ClickRole,
) -> Option<ClickTarget> {
    let mut interactive = false;
    let mut above = Above::Unknown;
    let path = std::iter::once(target).chain(dom.ancestors(target));
    for node in path.take(if bubbles { usize::MAX } else { 1 }) {
        let role = role_of(node, interactive);
        let plain = |role| {
            Some(ClickTarget {
                node,
                role,
                also: None,
            })
        };
        match role {
            ClickRole::Blocked => return None,
            ClickRole::Toggle | ClickRole::Button => return plain(role),
            ClickRole::Summary if !interactive => return plain(role),
            ClickRole::Link | ClickRole::Label => {
                let inert = role == ClickRole::Label && interactive;
                let button = if inert {
                    None
                } else {
                    button_above(dom, node, &mut role_of, &mut above)
                };
                if !inert && (node == target || button.is_none()) {
                    let also = button.filter(|_| role == ClickRole::Label);
                    return Some(ClickTarget { node, role, also });
                }
            }
            _ => {}
        }
        interactive |= role == ClickRole::Interactive;
    }
    None
}

/// The nearest button that acts above `node`, before a disabled control.
/// `above` keeps the answer for the elements further up the same walk.
fn button_above(
    dom: &Document,
    node: NodeId,
    role_of: &mut impl FnMut(NodeId, bool) -> ClickRole,
    above: &mut Above,
) -> Option<NodeId> {
    if let Above::Found(found) = above {
        return *found;
    }
    let found = dom
        .ancestors(node)
        .map(|a| (a, role_of(a, true)))
        .find(|(_, role)| matches!(role, ClickRole::Button | ClickRole::Blocked))
        .and_then(|(a, role)| (role == ClickRole::Button).then_some(a));
    *above = Above::Found(found);
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{QualName, ns, parse_html};

    /// Roles by tag name: `a` link, `label` label, `summary` summary,
    /// `b` a button that acts, `i` interactive, `u` a disabled control, `c` a
    /// checkbox.
    fn role(dom: &Document, node: NodeId) -> ClickRole {
        match dom.element(node).map(|e| &**e.local_name()) {
            Some("a") => ClickRole::Link,
            Some("label") => ClickRole::Label,
            Some("summary") => ClickRole::Summary,
            Some("button") => ClickRole::Button,
            Some("select") => ClickRole::Interactive,
            Some("u") => ClickRole::Blocked,
            Some("input") => ClickRole::Toggle,
            _ => ClickRole::Plain,
        }
    }

    fn pick(html: &str, bubbles: bool) -> Option<(String, ClickRole, Option<String>)> {
        let dom = parse_html(html);
        let by_id = |id: &str| dom.get_element_by_id(NodeId::DOCUMENT, id);
        let name = |n: NodeId| dom.element(n).unwrap().local_name().to_string();
        let target = by_id("t")?;
        click_target(&dom, target, bubbles, |n, _| role(&dom, n))
            .map(|t| (name(t.node), t.role, t.also.map(name)))
    }

    #[test]
    fn nearest_with_a_behavior() {
        let got = pick("<a id=o href=x><span id=t>x</span></a>", true);
        assert_eq!(got, Some(("a".into(), ClickRole::Link, None)));
        assert_eq!(pick("<a href=x><span id=t>x</span></a>", false), None);
    }

    #[test]
    fn button_that_does_not_act_lets_the_link_win() {
        // The role of such a button is `Plain` or `Interactive`.
        let got = pick("<a href=x><select id=t></select></a>", true);
        assert_eq!(got, Some(("a".into(), ClickRole::Link, None)));
    }

    #[test]
    fn link_inside_an_acting_button_yields_unless_it_is_the_target() {
        let html = "<button><a id=t href=x>x</a></button>";
        assert_eq!(
            pick(html, true),
            Some(("a".into(), ClickRole::Link, None)),
            "the link itself"
        );
        let html = "<button><a href=x><span id=t>x</span></a></button>";
        assert_eq!(
            pick(html, true),
            Some(("button".into(), ClickRole::Button, None))
        );
    }

    #[test]
    fn label_blocked_by_interactive_content_and_acting_label_adds_the_button() {
        let html = "<label><select id=t></select></label>";
        assert_eq!(pick(html, true), None);
        let html = "<button><label id=t>x</label></button>";
        assert_eq!(
            pick(html, true),
            Some(("label".into(), ClickRole::Label, Some("button".into())))
        );
        let html = "<button><label><span id=t>x</span></label></button>";
        assert_eq!(
            pick(html, true),
            Some(("button".into(), ClickRole::Button, None))
        );
    }

    #[test]
    fn disabled_control_stops_the_walk() {
        assert_eq!(pick("<a href=x><u><span id=t>x</span></u></a>", true), None);
    }

    /// A chain of alternating links and interactive elements below a button
    /// that acts, as a script can build it: the walk does not scan the
    /// ancestors again for each element.
    #[test]
    fn deep_nesting_is_linear() {
        let mut dom = Document::new();
        let mut roles = std::collections::HashMap::new();
        let mut make = |dom: &mut Document, name: &str, role: ClickRole, parent: NodeId| {
            let qual = QualName::new(None, ns!(html), name.into());
            let node = dom.create_element(qual, Vec::new()).unwrap();
            dom.append_child(parent, node).unwrap();
            roles.insert(node, role);
            node
        };
        let button = make(&mut dom, "button", ClickRole::Button, NodeId::DOCUMENT);
        let mut parent = button;
        let mut innermost_link = button;
        for depth in 0..200_000 {
            parent = if depth % 2 == 0 {
                innermost_link = make(&mut dom, "a", ClickRole::Link, parent);
                innermost_link
            } else {
                make(&mut dom, "select", ClickRole::Interactive, parent)
            };
        }
        let role_of = |n: NodeId, _| roles.get(&n).copied().unwrap_or(ClickRole::Plain);
        // The target is a link: it takes the click itself.
        let got = click_target(&dom, innermost_link, true, role_of);
        assert_eq!(got.map(|t| t.node), Some(innermost_link));
        // The target is the select below it: every link above yields to the
        // button.
        let got = click_target(&dom, parent, true, role_of);
        assert_eq!(got.map(|t| t.node), Some(button));
    }

    /// A button with thousands of labels nested in it and a target below
    /// them: the search for the control of a label runs a bounded number of
    /// times, not once for each label.
    #[test]
    fn nested_labels_search_for_a_control_once() {
        for control_at_bottom in [false, true] {
            let mut dom = Document::new();
            let make = |dom: &mut Document, name: &str, parent: NodeId| {
                let qual = QualName::new(None, ns!(html), name.into());
                let node = dom.create_element(qual, Vec::new()).unwrap();
                dom.append_child(parent, node).unwrap();
                node
            };
            let button = make(&mut dom, "button", NodeId::DOCUMENT);
            let mut parent = button;
            let mut labels = std::collections::HashSet::new();
            for _ in 0..20_000 {
                parent = make(&mut dom, "label", parent);
                labels.insert(parent);
            }
            if control_at_bottom {
                make(&mut dom, "select", parent);
            }
            let target = make(&mut dom, "span", parent);
            let searched = std::cell::Cell::new(0);
            let mut memo = LabelMemo::default();
            let got = click_target(&dom, target, true, |n, skip| {
                if n == button {
                    ClickRole::Button
                } else if labels.contains(&n) && !skip {
                    memo.role(false, || {
                        searched.set(searched.get() + 1);
                        control_at_bottom
                    })
                } else {
                    ClickRole::Plain
                }
            });
            // The labels yield to the button, whether they have a control or
            // not.
            assert_eq!(got.map(|t| t.node), Some(button));
            assert!(
                searched.get() <= 1,
                "{} searches (control at the bottom: {control_at_bottom})",
                searched.get()
            );
        }
    }
}
