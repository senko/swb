//! The lists that elements have (`HTMLCollection` and subclasses): the
//! rows, bodies and cells of tables, the options of a `select` and a
//! `datalist`, the selected options, the controls of a form or a
//! `fieldset`, and the areas of a `map`. This file says which nodes each
//! list holds; `lists.rs` caches them and runs the walks.
//!
//! HTML §4.9 (tables), §4.10.3 (`form.elements`), §4.10.7 (`select`),
//! <https://html.spec.whatwg.org/multipage/tables.html#dom-table-rows>.

use swb_dom::{Document, ElementData, NodeId};

use crate::bindings::{self as b, Cx};
use crate::html;
use crate::state::{dom, engine_mut};

/// A list of an element.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ElementList {
    /// `table.rows`: the `tr` elements of the head sections, then those of
    /// the table and its bodies, then those of the foot sections.
    TableRows,
    /// `table.tBodies`: the `tbody` children.
    TBodies,
    /// `tbody.rows`: the `tr` children.
    SectionRows,
    /// `tr.cells`: the `td` and `th` children.
    Cells,
    /// `select.options`: the `option` children and the `option` children
    /// of the `optgroup` children.
    SelectOptions,
    /// `select.selectedOptions`: the options that are selected.
    SelectedOptions,
    /// `datalist.options`: the `option` descendants.
    DatalistOptions,
    /// `form.elements`: the listed elements whose form owner is the form,
    /// except image buttons.
    FormControls,
    /// `fieldset.elements`: the listed elements below the fieldset.
    FieldsetElements,
    /// `map.areas`: the `area` descendants.
    Areas,
}

impl ElementList {
    /// True if the list is built in one pass.
    pub(super) fn is_eager(self) -> bool {
        matches!(
            self,
            ElementList::TableRows | ElementList::SelectedOptions | ElementList::FormControls
        )
    }

    /// True if the list holds children of its root.
    pub(super) fn is_child_list(self) -> bool {
        matches!(
            self,
            ElementList::TBodies | ElementList::SectionRows | ElementList::Cells
        )
    }
}

/// True if `e` is an HTML element with this local name.
pub(crate) fn is_named(e: &ElementData, name: &str) -> bool {
    e.is_html() && &**e.local_name() == name
}

/// True for the listed elements of HTML: the ones that have a form owner
/// and appear in `form.elements`.
/// <https://html.spec.whatwg.org/multipage/forms.html#category-listed>
pub(crate) fn is_listed(e: &ElementData) -> bool {
    e.is_html()
        && matches!(
            &**e.local_name(),
            "button" | "fieldset" | "input" | "object" | "output" | "select" | "textarea"
        )
}

/// True for an `input` of type `image`.
fn is_image_button(e: &ElementData) -> bool {
    is_named(e, "input")
        && e.attr("type")
            .is_some_and(|t| t.eq_ignore_ascii_case("image"))
}

/// True if the node `n` is in the lazy list `list` of `root`.
pub(super) fn matches(dom: &Document, root: NodeId, n: NodeId, list: ElementList) -> bool {
    let Some(e) = dom.element(n) else {
        return false;
    };
    match list {
        ElementList::TBodies => is_named(e, "tbody"),
        ElementList::SectionRows => is_named(e, "tr"),
        ElementList::Cells => is_named(e, "td") || is_named(e, "th"),
        ElementList::SelectOptions => {
            is_named(e, "option")
                && dom.parent(n).is_some_and(|p| {
                    p == root
                        || (dom.element(p).is_some_and(|pe| is_named(pe, "optgroup"))
                            && dom.parent(p) == Some(root))
                })
        }
        ElementList::DatalistOptions => is_named(e, "option"),
        ElementList::FieldsetElements => is_listed(e),
        ElementList::Areas => is_named(e, "area"),
        ElementList::TableRows | ElementList::SelectedOptions | ElementList::FormControls => false,
    }
}

/// The nodes of an eager list, in order.
pub(super) fn collect(
    cx: &mut Cx<'_>,
    root: NodeId,
    kind: &super::lists::ListKind,
) -> b::Result<Vec<NodeId>> {
    use super::lists::ListKind;
    match kind {
        ListKind::Element(ElementList::TableRows) => Ok(table_rows(dom(cx.rt)?, root)),
        ListKind::Element(ElementList::FormControls) => Ok(form_controls(dom(cx.rt)?, root)),
        ListKind::Element(ElementList::SelectedOptions) => selected_options(cx, root),
        ListKind::ControlsNamed(name) => {
            let controls = form_controls(dom(cx.rt)?, root);
            let dom = dom(cx.rt)?;
            Ok(controls
                .into_iter()
                .filter(|&n| {
                    dom.element(n)
                        .is_some_and(|e| e.id() == Some(name) || e.attr("name") == Some(name))
                })
                .collect())
        }
        _ => Ok(Vec::new()),
    }
}

/// The rows of a table in the order of `table.rows`.
fn table_rows(dom: &Document, table: NodeId) -> Vec<NodeId> {
    let rows_of = |section: NodeId| -> Vec<NodeId> {
        dom.element_children(section)
            .filter(|&r| dom.element(r).is_some_and(|e| is_named(e, "tr")))
            .collect()
    };
    let (mut head, mut body, mut foot) = (Vec::new(), Vec::new(), Vec::new());
    for child in dom.element_children(table) {
        let Some(e) = dom.element(child) else {
            continue;
        };
        if is_named(e, "tr") {
            body.push(child);
        } else if is_named(e, "thead") {
            head.extend(rows_of(child));
        } else if is_named(e, "tbody") {
            body.extend(rows_of(child));
        } else if is_named(e, "tfoot") {
            foot.extend(rows_of(child));
        }
    }
    head.extend(body);
    head.extend(foot);
    head
}

/// The controls of a form, in tree order: the listed elements below the
/// root of the form's tree whose form owner is the form, except image
/// buttons. One pass, with the nearest form ancestor of each node kept on
/// the way, so that a form of 100,000 controls costs O(n).
fn form_controls(dom: &Document, form: NodeId) -> Vec<NodeId> {
    let is_form = |n: NodeId| dom.is_html_element(n, &swb_dom::local_name!("form"));
    let mut nearest: Vec<Option<NodeId>> = vec![None; dom.len()];
    let mut out = Vec::new();
    for n in dom.descendants(dom.root(form)) {
        let inherited = dom.parent(n).and_then(|p| {
            if is_form(p) {
                Some(p)
            } else {
                nearest.get(p.index()).copied().flatten()
            }
        });
        if let Some(slot) = nearest.get_mut(n.index()) {
            *slot = inherited;
        }
        let Some(e) = dom.element(n) else {
            continue;
        };
        if is_listed(e)
            && !is_image_button(e)
            && html::owner_with(dom, n, || inherited) == Some(form)
        {
            out.push(n);
        }
    }
    out
}

/// The selected options of a select, in order.
fn selected_options(cx: &mut Cx<'_>, select: NodeId) -> b::Result<Vec<NodeId>> {
    let options: Vec<NodeId> = {
        let dom = dom(cx.rt)?;
        dom.descendants(select)
            .filter(|&n| matches(dom, select, n, ElementList::SelectOptions))
            .collect()
    };
    let host = engine_mut(cx.rt)?;
    Ok(options
        .into_iter()
        .filter(|&o| host.option_selected(o))
        .collect())
}
