//! Form owners (HTML "form owner",
//! <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#form-owner>):
//! the form that a listed element belongs to, and the owners of the
//! elements that borrow their form (`option`, `legend`, `label`).
//!
//! The engine keeps the same relation for the controls that are in the
//! document (`crates/engine/src/forms/mod.rs`), for rendering and
//! submission; scripts also ask about controls in other trees, so this
//! function reads the DOM.

use swb_dom::{Document, NodeId, local_name};

use crate::dom::element_lists::{is_listed, is_named};

/// True if `n` is a `form` element.
fn is_form(dom: &Document, n: NodeId) -> bool {
    dom.is_html_element(n, &local_name!("form"))
}

/// The form owner of a listed element: the form that its `form` attribute
/// names (in the same tree), else the form that the parser associated
/// with it, else its nearest `form` ancestor (given by `ancestor`, so that
/// a caller that walks a whole tree can track it).
pub(crate) fn owner_with(
    dom: &Document,
    node: NodeId,
    ancestor: impl FnOnce() -> Option<NodeId>,
) -> Option<NodeId> {
    let e = dom.element(node)?;
    if is_listed(e)
        && let Some(id) = e.attr("form")
    {
        return dom
            .get_element_by_id(dom.root(node), id)
            .filter(|&f| is_form(dom, f));
    }
    if let Some(form) = e
        .parser_form
        .filter(|&f| is_form(dom, f) && dom.is_connected(f))
    {
        return Some(form);
    }
    ancestor()
}

/// The form owner of a listed element.
pub(crate) fn form_owner(dom: &Document, node: NodeId) -> Option<NodeId> {
    // Only form-associated elements have a form owner.
    let e = dom.element(node)?;
    if !(is_listed(e) || (e.is_html() && &**e.local_name() == "img")) {
        return None;
    }
    owner_with(dom, node, || dom.ancestors(node).find(|&n| is_form(dom, n)))
}

/// The select that an option belongs to: its parent, or the parent of its
/// `optgroup` parent.
pub(crate) fn select_of_option(dom: &Document, option: NodeId) -> Option<NodeId> {
    let parent = dom.parent(option)?;
    let named = |n: NodeId, name: &str| dom.element(n).is_some_and(|e| is_named(e, name));
    if named(parent, "select") {
        return Some(parent);
    }
    let grand = dom.parent(parent)?;
    (named(parent, "optgroup") && named(grand, "select")).then_some(grand)
}

/// The form owner of an `option`: the owner of its select.
pub(crate) fn option_form(dom: &Document, option: NodeId) -> Option<NodeId> {
    form_owner(dom, select_of_option(dom, option)?)
}

/// The form owner of a `legend`: the owner of its parent `fieldset`.
pub(crate) fn legend_form(dom: &Document, legend: NodeId) -> Option<NodeId> {
    let parent = dom.parent(legend)?;
    dom.element(parent)
        .is_some_and(|e| is_named(e, "fieldset"))
        .then(|| form_owner(dom, parent))
        .flatten()
}

/// The labeled control of a `label`: the element its `for` attribute
/// names if that is labelable, else its first labelable descendant.
/// <https://html.spec.whatwg.org/multipage/forms.html#labeled-control>
pub(crate) fn labeled_control(dom: &Document, label: NodeId) -> Option<NodeId> {
    let e = dom.element(label)?;
    let labelable = |n: NodeId| {
        dom.element(n).is_some_and(|e| {
            e.is_html()
                && match &**e.local_name() {
                    "input" => !e
                        .attr("type")
                        .is_some_and(|t| t.eq_ignore_ascii_case("hidden")),
                    "button" | "meter" | "output" | "progress" | "select" | "textarea" => true,
                    _ => false,
                }
        })
    };
    if let Some(id) = e.attr("for") {
        return dom
            .get_element_by_id(dom.root(label), id)
            .filter(|&n| labelable(n));
    }
    dom.descendants(label).skip(1).find(|&n| labelable(n))
}
