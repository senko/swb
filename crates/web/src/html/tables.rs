//! The table interfaces: `caption`, `tHead`, `tFoot`, `tBodies`, `rows`,
//! `insertRow`, `deleteRow`, `cells`, `insertCell`, `deleteCell`,
//! `rowIndex`, `sectionRowIndex` and `cellIndex` (HTML §4.9).
//!
//! <https://html.spec.whatwg.org/multipage/tables.html>

use swb_dom::{ElementData, NodeData, NodeId, QualName, ns};
use swb_js::{Gc, Object};

use crate::bindings::{self as b, Cx, Exception};
use crate::dom::lists;
use crate::dom::tree_ops::{hierarchy, pre_insert, remove};
use crate::dom::{ElementList, element_lists::is_named};
use crate::generated as g;
use crate::state::{WebState, dom, dom_mut};

/// A new HTML element `name` in the document of `near`.
fn create(cx: &mut Cx<'_>, near: NodeId, name: &str) -> b::Result<NodeId> {
    let document = dom(cx.rt)?.node_document(near);
    let name = QualName::new(None, ns!(html), swb_dom::LocalName::from(name));
    let data = NodeData::Element(ElementData::new(name, Vec::new()));
    let result = dom_mut(cx.rt)?.create_node_in(document, data);
    crate::dom::dom_result(cx.rt, result)
}

/// The first child of `table` that is an HTML element named `name`.
fn child_named(cx: &Cx<'_>, parent: NodeId, name: &str) -> b::Result<Option<NodeId>> {
    let dom = dom(cx.rt)?;
    Ok(dom
        .element_children(parent)
        .find(|&c| dom.element(c).is_some_and(|e| is_named(e, name))))
}

/// The nodes of a list of `root`.
fn nodes(cx: &mut Cx<'_>, root: NodeId, kind: ElementList) -> b::Result<Vec<NodeId>> {
    let object = lists::element_list(cx, root, kind)?;
    let list = lists::platform(cx.rt, object)?;
    lists::nodes_of(cx, list)
}

/// How Chromium words the `IndexSizeError` of each member (measured in
/// `tools/probes/web-html-elements.json`).
#[derive(Clone, Copy)]
enum Kind {
    /// `table.insertRow` and `table.deleteRow`.
    TableRow,
    /// `insertRow` and `deleteRow` of a section.
    SectionRow,
    /// `tr.insertCell`.
    InsertCell,
    /// `tr.deleteCell`.
    DeleteCell,
}

fn index_error(kind: Kind, index: i32, count: i32) -> Exception {
    let message = match kind {
        Kind::TableRow if index < -1 => format!("The index provided ({index}) is less than -1."),
        Kind::TableRow => format!(
            "The index provided ({index}) is greater than the number of rows in the table ({count})."
        ),
        Kind::SectionRow => {
            format!("The provided index ({index} is outside the range [-1, {count}].")
        }
        Kind::InsertCell => {
            format!("The value provided ({index}) is outside the range [-1, {count}].")
        }
        Kind::DeleteCell => {
            format!("The value provided ({index}) is outside the range [0, {count}).")
        }
    };
    Exception::dom("IndexSizeError", message)
}

/// The first child of `table` that comes after the captions and column
/// groups (where `thead` goes).
fn after_captions_and_cols(cx: &Cx<'_>, table: NodeId) -> b::Result<Option<NodeId>> {
    let dom = dom(cx.rt)?;
    Ok(dom.element_children(table).find(|&c| {
        !dom.element(c)
            .is_some_and(|e| is_named(e, "caption") || is_named(e, "colgroup"))
    }))
}

/// Replaces the child of `table` named `name` with `new` (the setters of
/// `caption`, `tHead` and `tFoot`), which goes before `reference`.
fn replace_child_named(
    cx: &mut Cx<'_>,
    table: NodeId,
    name: &str,
    new: Option<NodeId>,
    place: impl FnOnce(&mut Cx<'_>, NodeId) -> b::Result<Option<NodeId>>,
) -> b::Result<()> {
    if let Some(node) = new
        && !dom(cx.rt)?.element(node).is_some_and(|e| is_named(e, name))
    {
        return Err(hierarchy(&format!("Not a {name} element.")));
    }
    if let Some(old) = child_named(cx, table, name)? {
        remove(cx, old)?;
    }
    if let Some(node) = new {
        let reference = place(cx, table)?;
        pre_insert(cx, table, node, reference)?;
    }
    Ok(())
}

/// The child of `table` before which a new `caption` goes: the first
/// child.
fn first_child_of(cx: &mut Cx<'_>, table: NodeId) -> b::Result<Option<NodeId>> {
    Ok(dom(cx.rt)?.node(table).first_child())
}

impl g::html_table_element::HTMLTableElementImpl for WebState {
    fn caption(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        child_named(cx, this, "caption")
    }

    fn set_caption(cx: &mut Cx<'_>, this: NodeId, value: Option<NodeId>) -> b::Result<()> {
        replace_child_named(cx, this, "caption", value, first_child_of)
    }

    fn t_head(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        child_named(cx, this, "thead")
    }

    fn set_t_head(cx: &mut Cx<'_>, this: NodeId, value: Option<NodeId>) -> b::Result<()> {
        replace_child_named(cx, this, "thead", value, |cx, t| {
            after_captions_and_cols(cx, t)
        })
    }

    fn t_foot(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        child_named(cx, this, "tfoot")
    }

    fn set_t_foot(cx: &mut Cx<'_>, this: NodeId, value: Option<NodeId>) -> b::Result<()> {
        replace_child_named(cx, this, "tfoot", value, |_, _| Ok(None))
    }

    fn t_bodies(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Gc<Object>> {
        lists::element_list(cx, this, ElementList::TBodies)
    }

    fn rows(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Gc<Object>> {
        lists::element_list(cx, this, ElementList::TableRows)
    }

    fn create_caption(cx: &mut Cx<'_>, this: NodeId) -> b::Result<NodeId> {
        if let Some(caption) = child_named(cx, this, "caption")? {
            return Ok(caption);
        }
        let caption = create(cx, this, "caption")?;
        let first = first_child_of(cx, this)?;
        pre_insert(cx, this, caption, first)?;
        Ok(caption)
    }

    fn delete_caption(cx: &mut Cx<'_>, this: NodeId) -> b::Result<()> {
        match child_named(cx, this, "caption")? {
            Some(caption) => remove(cx, caption),
            None => Ok(()),
        }
    }

    fn create_t_head(cx: &mut Cx<'_>, this: NodeId) -> b::Result<NodeId> {
        if let Some(head) = child_named(cx, this, "thead")? {
            return Ok(head);
        }
        let head = create(cx, this, "thead")?;
        let reference = after_captions_and_cols(cx, this)?;
        pre_insert(cx, this, head, reference)?;
        Ok(head)
    }

    fn delete_t_head(cx: &mut Cx<'_>, this: NodeId) -> b::Result<()> {
        match child_named(cx, this, "thead")? {
            Some(head) => remove(cx, head),
            None => Ok(()),
        }
    }

    fn create_t_foot(cx: &mut Cx<'_>, this: NodeId) -> b::Result<NodeId> {
        if let Some(foot) = child_named(cx, this, "tfoot")? {
            return Ok(foot);
        }
        let foot = create(cx, this, "tfoot")?;
        pre_insert(cx, this, foot, None)?;
        Ok(foot)
    }

    fn delete_t_foot(cx: &mut Cx<'_>, this: NodeId) -> b::Result<()> {
        match child_named(cx, this, "tfoot")? {
            Some(foot) => remove(cx, foot),
            None => Ok(()),
        }
    }

    /// A new `tbody` after the last `tbody` child, or at the end.
    fn create_t_body(cx: &mut Cx<'_>, this: NodeId) -> b::Result<NodeId> {
        let body = create(cx, this, "tbody")?;
        let reference = {
            let dom = dom(cx.rt)?;
            let last = dom
                .element_children(this)
                .filter(|&c| dom.element(c).is_some_and(|e| is_named(e, "tbody")))
                .last();
            last.and_then(|l| dom.node(l).next_sibling())
        };
        pre_insert(cx, this, body, reference)?;
        Ok(body)
    }

    /// <https://html.spec.whatwg.org/multipage/tables.html#dom-table-insertrow>
    fn insert_row(cx: &mut Cx<'_>, this: NodeId, index: i32) -> b::Result<NodeId> {
        let rows = nodes(cx, this, ElementList::TableRows)?;
        let count = i32::try_from(rows.len()).unwrap_or(i32::MAX);
        if index < -1 || index > count {
            return Err(index_error(Kind::TableRow, index, count));
        }
        let row = create(cx, this, "tr")?;
        if rows.is_empty() {
            let last_body = {
                let dom = dom(cx.rt)?;
                dom.element_children(this)
                    .filter(|&c| dom.element(c).is_some_and(|e| is_named(e, "tbody")))
                    .last()
            };
            let body = if let Some(body) = last_body {
                body
            } else {
                let body = create(cx, this, "tbody")?;
                pre_insert(cx, this, body, None)?;
                body
            };
            pre_insert(cx, body, row, None)?;
        } else if index == -1 || index == count {
            let last = rows.last().copied().unwrap_or(this);
            let parent = dom(cx.rt)?.parent(last).unwrap_or(this);
            pre_insert(cx, parent, row, None)?;
        } else {
            let at = usize::try_from(index)
                .ok()
                .and_then(|i| rows.get(i).copied());
            let parent = at.and_then(|r| dom(cx.rt).ok()?.parent(r)).unwrap_or(this);
            pre_insert(cx, parent, row, at)?;
        }
        Ok(row)
    }

    fn delete_row(cx: &mut Cx<'_>, this: NodeId, index: i32) -> b::Result<()> {
        let rows = nodes(cx, this, ElementList::TableRows)?;
        delete_at(cx, &rows, index, Kind::TableRow)
    }
}

/// Deletes item `index` (-1: the last) of `items`; an index out of range
/// is an `IndexSizeError`.
fn delete_at(cx: &mut Cx<'_>, items: &[NodeId], index: i32, kind: Kind) -> b::Result<()> {
    let count = i32::try_from(items.len()).unwrap_or(i32::MAX);
    if index < -1 || index >= count {
        return Err(index_error(kind, index, count));
    }
    let target = if index == -1 {
        items.last().copied()
    } else {
        usize::try_from(index)
            .ok()
            .and_then(|i| items.get(i).copied())
    };
    match target {
        Some(node) => remove(cx, node),
        None => Ok(()),
    }
}

/// Inserts a new `name` child at `index` of the children `items` of
/// `parent` (`insertRow` of a section, `insertCell`).
fn insert_at(
    cx: &mut Cx<'_>,
    parent: NodeId,
    items: &[NodeId],
    index: i32,
    name: &str,
    kind: Kind,
) -> b::Result<NodeId> {
    let count = i32::try_from(items.len()).unwrap_or(i32::MAX);
    if index < -1 || index > count {
        return Err(index_error(kind, index, count));
    }
    let node = create(cx, parent, name)?;
    let reference = if index == -1 || index == count {
        None
    } else {
        usize::try_from(index)
            .ok()
            .and_then(|i| items.get(i).copied())
    };
    pre_insert(cx, parent, node, reference)?;
    Ok(node)
}

impl g::html_table_section_element::HTMLTableSectionElementImpl for WebState {
    fn rows(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Gc<Object>> {
        lists::element_list(cx, this, ElementList::SectionRows)
    }

    fn insert_row(cx: &mut Cx<'_>, this: NodeId, index: i32) -> b::Result<NodeId> {
        let rows = nodes(cx, this, ElementList::SectionRows)?;
        insert_at(cx, this, &rows, index, "tr", Kind::SectionRow)
    }

    fn delete_row(cx: &mut Cx<'_>, this: NodeId, index: i32) -> b::Result<()> {
        let rows = nodes(cx, this, ElementList::SectionRows)?;
        delete_at(cx, &rows, index, Kind::SectionRow)
    }
}

impl g::html_table_row_element::HTMLTableRowElementImpl for WebState {
    /// The position in `rows` of the table that contains the row, else -1.
    fn row_index(cx: &mut Cx<'_>, this: NodeId) -> b::Result<i32> {
        let table = {
            let dom = dom(cx.rt)?;
            dom.ancestors(this)
                .find(|&a| dom.element(a).is_some_and(|e| is_named(e, "table")))
        };
        let Some(table) = table else {
            return Ok(-1);
        };
        let rows = nodes(cx, table, ElementList::TableRows)?;
        Ok(position(&rows, this))
    }

    /// The position in the `rows` of the parent table or section, else -1.
    fn section_row_index(cx: &mut Cx<'_>, this: NodeId) -> b::Result<i32> {
        let parent = dom(cx.rt)?.parent(this);
        let Some(parent) = parent else {
            return Ok(-1);
        };
        let kind = {
            let dom = dom(cx.rt)?;
            match dom.element(parent) {
                Some(e) if is_named(e, "table") => ElementList::TableRows,
                Some(e) if is_named(e, "tbody") || is_named(e, "thead") || is_named(e, "tfoot") => {
                    ElementList::SectionRows
                }
                _ => return Ok(-1),
            }
        };
        let rows = nodes(cx, parent, kind)?;
        Ok(position(&rows, this))
    }

    fn cells(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Gc<Object>> {
        lists::element_list(cx, this, ElementList::Cells)
    }

    fn insert_cell(cx: &mut Cx<'_>, this: NodeId, index: i32) -> b::Result<NodeId> {
        let cells = nodes(cx, this, ElementList::Cells)?;
        insert_at(cx, this, &cells, index, "td", Kind::InsertCell)
    }

    fn delete_cell(cx: &mut Cx<'_>, this: NodeId, index: i32) -> b::Result<()> {
        let cells = nodes(cx, this, ElementList::Cells)?;
        delete_at(cx, &cells, index, Kind::DeleteCell)
    }
}

/// The position of `node` in `items`, else -1.
fn position(items: &[NodeId], node: NodeId) -> i32 {
    items
        .iter()
        .position(|&n| n == node)
        .and_then(|i| i32::try_from(i).ok())
        .unwrap_or(-1)
}

impl g::html_table_cell_element::HTMLTableCellElementImpl for WebState {
    /// The position among the cells of the parent `tr`, else -1.
    fn cell_index(cx: &mut Cx<'_>, this: NodeId) -> b::Result<i32> {
        let parent = {
            let dom = dom(cx.rt)?;
            dom.parent(this)
                .filter(|&p| dom.element(p).is_some_and(|e| is_named(e, "tr")))
        };
        let Some(row) = parent else {
            return Ok(-1);
        };
        let cells = nodes(cx, row, ElementList::Cells)?;
        Ok(position(&cells, this))
    }
}
