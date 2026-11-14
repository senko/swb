//! `Selection`, `Window.getSelection` and `Document.getSelection` (Selection
//! API, <https://w3c.github.io/selection-api/>; M10 sub-feature 9b-2, ADR 0039
//! section 3).
//!
//! The selection's state belongs to the arena (`swb_dom::SelectionState`: the
//! id of a live range entry, the direction, the text control that owns it),
//! so the engine paints and copies a script's selection and scripts see the
//! user's. This module is the script's side, with Chromium's rules (measured,
//! `tools/probes/web-selection.json`):
//!
//! - one `Selection` object per document with a browsing context, `null` for
//!   other documents; it keeps the `Range` object of its range entry, so
//!   `getRangeAt(0)` returns the same object until the range changes;
//! - `addRange` makes the given range the selection's (a second range, a
//!   range of another document or tree is ignored); every other method makes
//!   a new range entry, and the old range object stays where it was;
//! - a script's change of the document's selection while a text control owns
//!   it (it has the focus) collapses the control's selection at its start and
//!   ends the ownership; the focus stays (measured, `control-script-selection`;
//!   decision 5 of the ADR). `addRange` is ignored then (the selection has a
//!   range);
//! - `toString` is the copied text (the engine's, `EngineHost::selection_text`);
//! - `containsNode` answers from the positions that show the selection
//!   (`swb_dom::Document::visual_range`; decision 4, a measured departure from
//!   the standard);
//! - a change queues `selectionchange` at the document
//!   (`html/selection_events.rs`);
//! - `deleteFromDocument` deletes the contents of the range (9b-3); after it,
//!   and after a contents operation of the range's `Range` object that moved
//!   it, the selection reports the type `Range` and the direction `forward`
//!   until its next change, also when collapsed (Chromium, measured:
//!   `SelectionState::stale_type`).
//!
//! Departures: anchor and focus in different trees (a shadow tree) collapse at
//! the focus; `modify`, `getComposedRanges` and `ShadowRoot.getSelection` are
//! absent.

use std::cmp::Ordering;

use swb_dom::{Boundary, LiveRange, NodeData, NodeId, SelectionState};
use swb_js::{Gc, Object};

use crate::bindings::{self as b, Cx, Exception, NodeRef, Platform};
use crate::generated as g;
use crate::html::selection_events;
use crate::range;
use crate::state::{self, WebState, dom, dom_mut, engine_mut, state, state_mut};

/// The `Selection` of the current document, made at the first access; `None`
/// for a document that left its frame.
pub(crate) fn selection_object(cx: &mut Cx<'_>) -> b::Result<Option<Gc<Object>>> {
    if state(cx.rt)?.doc.retired {
        return Ok(None);
    }
    let made = state(cx.rt)?.doc.selection.is_none();
    let object = state::window_object(cx, |d| &mut d.selection, g::SELECTION, state::no_data)?;
    if made {
        // A data change that moves the selection's boundary points reports
        // a change from now on.
        range::watch_selection(cx.rt)?;
    }
    Ok(Some(object))
}

/// The selection of the current document.
fn current(cx: &Cx<'_>) -> b::Result<Option<SelectionState>> {
    Ok(dom(cx.rt)?.selection())
}

/// The selection of the text control that owns the document's selection.
fn control_range(cx: &mut Cx<'_>) -> b::Result<Option<crate::host::SelectionRange>> {
    let Some(control) = selection_events::owner(cx.rt)? else {
        return Ok(None);
    };
    Ok(engine_mut(cx.rt)?.control_selection(control))
}

/// The API offset of a boundary point.
fn api_offset(cx: &Cx<'_>, point: Boundary) -> b::Result<u32> {
    Ok(range::api_offset(dom(cx.rt)?, point.node, point.offset))
}

/// True if `node` is in the current document's tree (a shadow tree of it
/// counts), where the selection can be.
fn in_tree(cx: &Cx<'_>, node: NodeRef) -> b::Result<bool> {
    let key = state(cx.rt)?.doc.key;
    Ok(node.document == key && dom(cx.rt)?.shadow_including_root(node.node) == NodeId::DOCUMENT)
}

/// The range from `anchor` to `focus` and whether it is backward; points in
/// different trees collapse at the focus (departure: Chromium keeps a
/// composed selection).
fn ordered(cx: &Cx<'_>, anchor: Boundary, focus: Boundary) -> b::Result<(LiveRange, bool)> {
    let dom = dom(cx.rt)?;
    if dom.root(anchor.node) != dom.root(focus.node) {
        return Ok((LiveRange::collapsed_at(focus), false));
    }
    Ok(if dom.compare_boundaries(focus, anchor) == Ordering::Less {
        (
            LiveRange {
                start: focus,
                end: anchor,
            },
            true,
        )
    } else {
        (
            LiveRange {
                start: anchor,
                end: focus,
            },
            false,
        )
    })
}

/// After a method: queues `selectionchange` if the selection changed, and
/// lets go of the `Range` object of a range that left the selection.
fn finish(cx: &mut Cx<'_>) -> b::Result<()> {
    selection_events::note_document_change(cx.rt)?;
    let id = dom(cx.rt)?.selection().map(|s| s.id);
    let doc = &mut state_mut(cx.rt)?.doc;
    if doc
        .selection_range
        .is_some_and(|(cached, _)| Some(cached) != id)
    {
        doc.selection_range = None;
    }
    Ok(cx.rt.check_time()?)
}

/// Replaces the selection with `range` in a new entry (`None`: no range).
/// A text control that owned the selection loses it, and its selection
/// collapses at its start.
fn replace(cx: &mut Cx<'_>, range: Option<LiveRange>, backward: bool) -> b::Result<()> {
    if let Some(control) = selection_events::owner(cx.rt)? {
        engine_mut(cx.rt)?.set_control_selection(control, 0, 0, false);
    }
    dom_mut(cx.rt)?.set_selection(range, backward, true);
    finish(cx)
}

/// `removeAllRanges` and `empty`.
fn remove_all(cx: &mut Cx<'_>) -> b::Result<()> {
    if current(cx)?.is_none() {
        return Ok(());
    }
    replace(cx, None, false)
}

/// `collapse` and `setPosition`.
fn collapse_to(cx: &mut Cx<'_>, node: Option<NodeRef>, offset: u32) -> b::Result<()> {
    let Some(node) = node else {
        return remove_all(cx);
    };
    let point = range::checked_point(cx.rt, node, offset)?;
    if !in_tree(cx, node)? {
        return Ok(());
    }
    replace(cx, Some(LiveRange::collapsed_at(point)), false)
}

impl g::selection::SelectionImpl for WebState {
    fn anchor_node(cx: &mut Cx<'_>, _this: Platform) -> b::Result<Option<NodeId>> {
        Ok(current(cx)?.map(|s| s.anchor().node))
    }

    fn anchor_offset(cx: &mut Cx<'_>, _this: Platform) -> b::Result<u32> {
        match current(cx)? {
            Some(s) => api_offset(cx, s.anchor()),
            None => Ok(0),
        }
    }

    fn focus_node(cx: &mut Cx<'_>, _this: Platform) -> b::Result<Option<NodeId>> {
        Ok(current(cx)?.map(|s| s.focus().node))
    }

    fn focus_offset(cx: &mut Cx<'_>, _this: Platform) -> b::Result<u32> {
        match current(cx)? {
            Some(s) => api_offset(cx, s.focus()),
            None => Ok(0),
        }
    }

    fn is_collapsed(cx: &mut Cx<'_>, _this: Platform) -> b::Result<bool> {
        Ok(current(cx)?.is_none_or(|s| s.range.is_collapsed()))
    }

    fn range_count(cx: &mut Cx<'_>, _this: Platform) -> b::Result<u32> {
        Ok(u32::from(current(cx)?.is_some()))
    }

    /// `None`, `Caret` or `Range`; the type of the control's selection while
    /// a text control owns the selection.
    fn type_(cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        let Some(s) = current(cx)? else {
            return Ok("None".to_owned());
        };
        let collapsed = match control_range(cx)? {
            Some(c) => c.start == c.end,
            None => s.range.is_collapsed() && !s.stale_type,
        };
        Ok((if collapsed { "Caret" } else { "Range" }).to_owned())
    }

    /// `none` for no or a collapsed selection, else `forward` or
    /// `backward`; the control's while a text control owns the selection.
    fn direction(cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        let Some(s) = current(cx)? else {
            return Ok("none".to_owned());
        };
        let (collapsed, backward) = match control_range(cx)? {
            Some(c) => (c.start == c.end, c.backward),
            None => (s.range.is_collapsed() && !s.stale_type, s.backward),
        };
        Ok(match (collapsed, backward) {
            (true, _) => "none",
            (false, true) => "backward",
            (false, false) => "forward",
        }
        .to_owned())
    }

    fn base_node(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<NodeId>> {
        Self::anchor_node(cx, this)
    }

    fn base_offset(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        Self::anchor_offset(cx, this)
    }

    fn extent_node(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<NodeId>> {
        Self::focus_node(cx, this)
    }

    fn extent_offset(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        Self::focus_offset(cx, this)
    }

    /// `addRange`: the range becomes the selection's, unless the selection
    /// has one or the range is in another document or tree (Chromium ignores
    /// it then).
    fn add_range(cx: &mut Cx<'_>, _this: Platform, range: Platform) -> b::Result<()> {
        let (id, key) = range::range_entry(cx.rt, range)?;
        if current(cx)?.is_some() || key != state(cx.rt)?.doc.key {
            return Ok(());
        }
        let dom = dom_mut(cx.rt)?;
        let in_document = dom
            .live_range(id)
            .is_some_and(|r| dom.is_in_document_tree(r));
        if !in_document || !dom.set_selection_entry(id) {
            return Ok(());
        }
        state_mut(cx.rt)?.doc.selection_range = Some((id, range.object));
        finish(cx)
    }

    fn collapse(
        cx: &mut Cx<'_>,
        _this: Platform,
        node: Option<NodeRef>,
        offset: u32,
    ) -> b::Result<()> {
        collapse_to(cx, node, offset)
    }

    fn collapse_to_end(cx: &mut Cx<'_>, _this: Platform) -> b::Result<()> {
        let s = current(cx)?.ok_or_else(no_selection)?;
        replace(cx, Some(LiveRange::collapsed_at(s.range.end)), false)
    }

    fn collapse_to_start(cx: &mut Cx<'_>, _this: Platform) -> b::Result<()> {
        let s = current(cx)?.ok_or_else(no_selection)?;
        replace(cx, Some(LiveRange::collapsed_at(s.range.start)), false)
    }

    fn contains_node(
        cx: &mut Cx<'_>,
        _this: Platform,
        node: NodeRef,
        allow_partial_containment: bool,
    ) -> b::Result<bool> {
        let Some(s) = current(cx)? else {
            return Ok(false);
        };
        if node.document != state(cx.rt)?.doc.key {
            return Ok(false);
        }
        let dom = dom(cx.rt)?;
        if dom.root(node.node) != dom.root(s.range.start.node) {
            return Ok(false);
        }
        let text = matches!(&dom.node(node.node).data, NodeData::Text(t) if !t.is_empty());
        let shown = engine_mut(cx.rt)?.visual_range(s.range);
        let contains =
            state::dom(cx.rt)?.visually_contains(shown, node.node, text, allow_partial_containment);
        cx.rt.check_time()?;
        Ok(contains)
    }

    /// `deleteFromDocument`: `deleteContents` of the selection's range.
    fn delete_from_document(cx: &mut Cx<'_>, _this: Platform) -> b::Result<()> {
        let Some(s) = current(cx)? else {
            return Ok(());
        };
        range::contents::delete(cx.rt, s.id, s.range)?;
        finish(cx)
    }

    fn empty(cx: &mut Cx<'_>, _this: Platform) -> b::Result<()> {
        remove_all(cx)
    }

    /// `extend`: the focus moves to the point, the anchor stays (a new range
    /// entry). While a text control owns the selection, the selection
    /// collapses at the point (measured).
    fn extend(cx: &mut Cx<'_>, _this: Platform, node: NodeRef, offset: u32) -> b::Result<()> {
        if !in_tree(cx, node)? {
            return Ok(());
        }
        let s = current(cx)?.ok_or_else(|| {
            Exception::dom(
                "InvalidStateError",
                "This Selection object doesn't have any Ranges.",
            )
        })?;
        let point = range::checked_point(cx.rt, node, offset)?;
        let (range, backward) = if selection_events::owner(cx.rt)?.is_some() {
            (LiveRange::collapsed_at(point), false)
        } else {
            ordered(cx, s.anchor(), point)?
        };
        replace(cx, Some(range), backward)
    }

    fn get_range_at(cx: &mut Cx<'_>, _this: Platform, index: u32) -> b::Result<Gc<Object>> {
        let invalid = || Exception::dom("IndexSizeError", format!("{index} is not a valid index."));
        let s = current(cx)?.filter(|_| index == 0).ok_or_else(invalid)?;
        if let Some((id, object)) = state(cx.rt)?.doc.selection_range
            && id == s.id
        {
            return Ok(object);
        }
        let realm = state::doc_realm(cx.rt)?;
        let object = range::wrap_selection(cx.rt, realm)?.ok_or_else(invalid)?;
        if let Some(s) = current(cx)? {
            state_mut(cx.rt)?.doc.selection_range = Some((s.id, object));
        }
        Ok(object)
    }

    fn remove_all_ranges(cx: &mut Cx<'_>, _this: Platform) -> b::Result<()> {
        remove_all(cx)
    }

    fn remove_range(cx: &mut Cx<'_>, _this: Platform, range: Platform) -> b::Result<()> {
        let (id, _) = range::range_entry(cx.rt, range)?;
        if current(cx)?.is_none_or(|s| s.id != id) {
            return Err(Exception::dom("NotFoundError", "Range not found."));
        }
        replace(cx, None, false)
    }

    /// `selectAllChildren`: from `(node, 0)` to `(node, child count)` (a text
    /// node has no children: a caret at its offset 0, measured).
    fn select_all_children(cx: &mut Cx<'_>, _this: Platform, node: NodeRef) -> b::Result<()> {
        let dom = range::arena_or_error(cx.rt, node.document)?;
        if matches!(dom.node(node.node).data, NodeData::Doctype { .. }) {
            return Err(range::doctype_error(dom, node.node));
        }
        if !in_tree(cx, node)? {
            return Ok(());
        }
        let count = u32::try_from(state::dom(cx.rt)?.child_count(node.node)).unwrap_or(u32::MAX);
        let range = LiveRange {
            start: Boundary::new(node.node, 0),
            end: Boundary::new(node.node, count),
        };
        replace(cx, Some(range), false)
    }

    /// `setBaseAndExtent`: a `null` anchor empties the selection, a `null`
    /// focus collapses it at the anchor; nodes of another document or tree
    /// change nothing (Chromium, measured).
    fn set_base_and_extent(
        cx: &mut Cx<'_>,
        _this: Platform,
        anchor_node: Option<NodeRef>,
        anchor_offset: u32,
        focus_node: Option<NodeRef>,
        focus_offset: u32,
    ) -> b::Result<()> {
        let Some(anchor_node) = anchor_node else {
            return remove_all(cx);
        };
        let Some(focus_node) = focus_node else {
            return collapse_to(cx, Some(anchor_node), anchor_offset);
        };
        let anchor = range::checked_point(cx.rt, anchor_node, anchor_offset)?;
        let focus = range::checked_point(cx.rt, focus_node, focus_offset)?;
        if !in_tree(cx, anchor_node)? || !in_tree(cx, focus_node)? {
            return Ok(());
        }
        let (range, backward) = ordered(cx, anchor, focus)?;
        replace(cx, Some(range), backward)
    }

    fn set_position(
        cx: &mut Cx<'_>,
        _this: Platform,
        node: Option<NodeRef>,
        offset: u32,
    ) -> b::Result<()> {
        collapse_to(cx, node, offset)
    }

    /// The copied text of the selection (the engine's rendered text; the
    /// selected text of the text control that owns it).
    fn stringify(cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        if current(cx)?.is_none() {
            return Ok(String::new());
        }
        let text = engine_mut(cx.rt)?.selection_text();
        cx.rt.check_time()?;
        Ok(text)
    }
}

/// The error of `collapseToStart` and `collapseToEnd` without a selection.
fn no_selection() -> Exception {
    Exception::dom("InvalidStateError", "there is no selection.")
}
