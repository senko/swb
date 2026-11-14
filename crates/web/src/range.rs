//! `AbstractRange`, `StaticRange`, `Range` and `Document.createRange` (DOM
//! section 5, <https://dom.spec.whatwg.org/#ranges>; M10 sub-features 9b-1 and
//! 9b-3, ADR 0039). The contents operations and `createContextualFragment`
//! are in `range/contents.rs`, the geometry in `range/geometry.rs`.
//!
//! The boundary points of a live range are Rust data in the arena that holds
//! its nodes (`swb_dom::Document::set_live_range`): the change functions of the
//! arena keep them live, and no script object takes part. This module makes the
//! JavaScript objects, converts the offsets of character data between the
//! UTF-16 code units of the API and the bytes of the arena, and gives
//! Chromium's exceptions (measured, `tools/probes/web-range.json`).
//!
//! **Lifetime.** The object holds the id of its entry. The wrappers of the nodes
//! live as long as their document (ADR 0031 section 5), so the range traces
//! nothing. When the collector frees the object, its data puts the id on a list
//! that the arenas share ([`swb_dom::DeadIds`]); an arena drops the entry when
//! it next looks, and the next new range clears the list and the table.
//!
//! **Documents.** The table knows the document whose arena holds each entry. A
//! boundary point in another arena collapses the range there (another root),
//! so the entry moves; a range whose root moves to another arena goes with it
//! ([`moved`]).
//!
//! **Offsets.** An offset in a text, comment or processing instruction node is
//! a byte offset in the arena; the API's UTF-16 offset that falls inside a
//! surrogate pair rounds up to the end of the character, as in
//! `CharacterData` (departure: Chromium keeps it).

use std::cell::Cell;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::rc::Rc;

use swb_dom::{ArenaWork, Boundary, DeadIds, LiveRange, NodeData, NodeId, WORK_PER_CHECK};
use swb_js::{Gc, GenericData, HostData, Object, Runtime, Tracer, VmResult};

use crate::bindings::{self as b, Cx, Exception, NodeRef, Platform};
use crate::generated as g;
use crate::node_iterator::wrapper_in;
use crate::state::{self, DocKey, DocState, WebState, state_mut};

pub(crate) mod contents;
mod geometry;

/// The live ranges of the runtime.
#[derive(Default)]
pub(crate) struct RangeTable {
    next_id: u32,
    /// The document whose arena holds the entry of each live range.
    home: HashMap<u32, DocKey>,
    /// The ids of ranges that the collector freed.
    dead: Rc<DeadIds>,
    /// The work of the live range steps since the host last charged it.
    steps: Rc<Cell<usize>>,
}

impl RangeTable {
    /// True if no range exists.
    pub(crate) fn is_empty(&self) -> bool {
        self.home.is_empty()
    }
}

/// The host data of a `Range`.
struct RangeData {
    id: u32,
    dead: Rc<DeadIds>,
}

impl GenericData for RangeData {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        // The entry in the arena, its index and the table.
        80
    }
}

impl Drop for RangeData {
    fn drop(&mut self) {
        self.dead.push(self.id);
    }
}

/// A boundary point of a `StaticRange`: the node and the offset as given.
#[derive(Clone, Copy)]
struct StaticPoint {
    key: DocKey,
    node: NodeId,
    offset: u32,
}

/// The host data of a `StaticRange`.
struct StaticRangeData {
    start: StaticPoint,
    end: StaticPoint,
}

impl GenericData for StaticRangeData {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        32
    }
}

fn illegal() -> Exception {
    Exception::type_error("Illegal invocation")
}

fn range_data(rt: &Runtime, this: Platform) -> Option<&RangeData> {
    match this.data {
        HostData::Generic(handle) => rt.heap().generic::<RangeData>(handle).ok(),
        HostData::Word(_) => None,
    }
}

fn static_data(rt: &Runtime, this: Platform) -> Option<&StaticRangeData> {
    match this.data {
        HostData::Generic(handle) => rt.heap().generic::<StaticRangeData>(handle).ok(),
        HostData::Word(_) => None,
    }
}

/// The arena of document `key`, if the runtime has it now.
fn arena(rt: &mut Runtime, key: DocKey) -> Option<&mut swb_dom::Document> {
    state::ensure_lent(rt, key).ok()?;
    let state = rt.host_data_mut::<WebState>()?;
    let record = state::document_mut(state, key)?;
    Some(record.host.as_deref_mut()?.dom_mut())
}

/// Removes the entries of the ranges that the collector freed and clears the
/// list.
fn reap(rt: &mut Runtime) -> VmResult<()> {
    let dead = Rc::clone(&state::state(rt)?.ranges.dead);
    for id in dead.take() {
        let Some(key) = state_mut(rt)?.ranges.home.remove(&id) else {
            continue;
        };
        if let Some(dom) = arena(rt, key) {
            // The selection keeps its entry (ADR 0039 section 4).
            dom.release_live_range(id);
        }
    }
    Ok(())
}

/// Gives the arena of the current document the counters of the ranges, so
/// that a change of the selection's range by the live range steps asks the
/// host to look (`selectionchange` after a data change).
pub(crate) fn watch_selection(rt: &mut Runtime) -> VmResult<()> {
    let work = work(rt)?;
    if let Ok(dom) = state::dom_mut(rt) {
        dom.set_range_work(work);
    }
    Ok(())
}

/// The next id of a `Range` object.
fn next_id(rt: &mut Runtime) -> b::Result<u32> {
    let table = &mut state_mut(rt)?.ranges;
    let id = table
        .next_id
        .checked_add(1)
        .filter(|&id| id < swb_dom::HOST_ID_LIMIT)
        .ok_or_else(|| Exception::range_error("Too many Range objects were made."))?;
    table.next_id = id;
    Ok(id)
}

/// The `Range` object of the selection's range entry of the current document,
/// made in `realm`: the entry gets the id of the new object (ADR 0039 section
/// 4). `None` without a selection.
pub(crate) fn wrap_selection(rt: &mut Runtime, realm: u32) -> b::Result<Option<Gc<Object>>> {
    reap(rt)?;
    let key = state::state(rt)?.doc.key;
    let id = next_id(rt)?;
    let work = work(rt)?;
    let dom = arena_or_error(rt, key)?;
    dom.set_range_work(work);
    let Some(held) = dom.wrap_selection_entry(id) else {
        return Ok(None);
    };
    if held != id {
        // An object that the caller does not know holds the entry (not
        // reached: the `Selection` keeps the object it hands out): a copy.
        let Some(range) = dom.live_range(held) else {
            return Ok(None);
        };
        return create(rt, realm, key, range).map(Some);
    }
    state_mut(rt)?.ranges.home.insert(id, key);
    let dead = Rc::clone(&state::state(rt)?.ranges.dead);
    let data = RangeData { id, dead };
    let object = rt
        .alloc_host_data(data)
        .and_then(|data| rt.new_host_object(realm, g::RANGE, data))?;
    Ok(Some(object))
}

/// The id of the `Range` object `this` and the document of its arena.
pub(crate) fn range_entry(rt: &Runtime, this: Platform) -> b::Result<(u32, DocKey)> {
    let id = range_data(rt, this).ok_or_else(illegal)?.id;
    let key = *state::state(rt)?.ranges.home.get(&id).ok_or_else(illegal)?;
    Ok((id, key))
}

/// True once the work of the live range steps passed the unit that the host
/// charges to the time limit; the count starts again.
pub(crate) fn take_overdue(rt: &Runtime) -> bool {
    let Ok(state) = state::state(rt) else {
        return false;
    };
    if state.ranges.steps.get() < WORK_PER_CHECK {
        return false;
    }
    state.ranges.steps.set(0);
    true
}

/// The counters that the arena of a range reports its work to.
fn work(rt: &mut Runtime) -> VmResult<ArenaWork> {
    let request = rt.host_signal_cell();
    let table = &state::state(rt)?.ranges;
    Ok(ArenaWork::new(
        Rc::clone(&table.steps),
        request,
        Rc::clone(&table.dead),
    ))
}

/// Puts `range` into the arena of document `key` as entry `id`.
fn store(rt: &mut Runtime, id: u32, key: DocKey, range: LiveRange) -> b::Result<()> {
    let work = work(rt)?;
    let dom = arena_or_error(rt, key)?;
    dom.set_range_work(work);
    dom.set_live_range(id, range);
    let old = state_mut(rt)?.ranges.home.insert(id, key);
    if let Some(old) = old
        && old != key
        && let Some(dom) = arena(rt, old)
    {
        dom.remove_live_range(id);
    }
    Ok(())
}

/// Makes a `Range` object in `realm` with the boundary points `range` in the
/// arena of document `key`.
fn create(rt: &mut Runtime, realm: u32, key: DocKey, range: LiveRange) -> b::Result<Gc<Object>> {
    reap(rt)?;
    let id = next_id(rt)?;
    let dead = Rc::clone(&state::state(rt)?.ranges.dead);
    store(rt, id, key, range)?;
    // The data frees the entry if anything below fails.
    let data = RangeData { id, dead };
    let object = rt
        .alloc_host_data(data)
        .and_then(|data| rt.new_host_object(realm, g::RANGE, data));
    Ok(object?)
}

/// `Document.createRange`: a range at `(document, 0)` in the realm of the
/// method.
pub(crate) fn create_for_document(cx: &mut Cx<'_>, document: NodeId) -> b::Result<Gc<Object>> {
    let realm = cx.realm();
    let key = state::state(cx.rt)?.doc.key;
    let point = Boundary::new(document, 0);
    create(cx.rt, realm, key, LiveRange::collapsed_at(point))
}

/// The id of the range `this`, the document of its arena and its boundary
/// points.
fn current(rt: &mut Runtime, this: Platform) -> b::Result<(u32, DocKey, LiveRange)> {
    let id = range_data(rt, this).ok_or_else(illegal)?.id;
    let key = *state::state(rt)?.ranges.home.get(&id).ok_or_else(illegal)?;
    let range = arena(rt, key)
        .and_then(|dom| dom.live_range(id))
        .ok_or_else(illegal)?;
    Ok((id, key, range))
}

/// The arena of document `key`, or the error of a document that is gone.
pub(crate) fn arena_or_error(rt: &mut Runtime, key: DocKey) -> b::Result<&mut swb_dom::Document> {
    arena(rt, key).ok_or_else(|| Exception::type_error("the document is not available"))
}

/// The number of UTF-16 code units of `text`.
fn utf16_len(text: &str) -> u32 {
    u32::try_from(text.encode_utf16().count()).unwrap_or(u32::MAX)
}

/// The API offset of the byte offset `offset` in `node`.
pub(crate) fn api_offset(dom: &swb_dom::Document, node: NodeId, offset: u32) -> u32 {
    match dom.char_data(node) {
        Some(text) => {
            let end = (offset as usize).min(text.len());
            utf16_len(text.get(..end).unwrap_or(text))
        }
        None => offset,
    }
}

/// The byte offset of the UTF-16 `offset` in `text` (rounded up to the end of a
/// character that it falls inside), or `None` past the end.
fn byte_offset(text: &str, offset: u32) -> Option<u32> {
    let mut units = 0u32;
    for (i, c) in text.char_indices() {
        if units >= offset {
            return u32::try_from(i).ok();
        }
        units += u32::try_from(c.len_utf16()).unwrap_or(2);
    }
    (units >= offset).then(|| u32::try_from(text.len()).unwrap_or(u32::MAX))
}

/// Chromium's exception for the API `offset` past the length of `node`.
fn offset_error(dom: &swb_dom::Document, node: NodeId, offset: u32) -> Exception {
    match dom.char_data(node) {
        Some(text) => Exception::dom(
            "IndexSizeError",
            format!(
                "The offset {offset} is larger than the node's length ({}).",
                utf16_len(text)
            ),
        ),
        // An offset of 2^31 or more is a negative `int` to Chromium.
        None if offset > i32::MAX as u32 => {
            Exception::dom("IndexSizeError", format!("The offset {offset} is invalid."))
        }
        None => Exception::dom(
            "IndexSizeError",
            format!("There is no child at offset {offset}."),
        ),
    }
}

/// Chromium's exception for a node without a parent where a range needs one.
fn no_parent_error() -> Exception {
    Exception::dom("InvalidNodeTypeError", "the given Node has no parent.")
}

/// The exception for a doctype where a range refuses one: Chromium names the
/// doctype's name as its type.
pub(crate) fn doctype_error(dom: &swb_dom::Document, node: NodeId) -> Exception {
    let name = match &dom.node(node).data {
        NodeData::Doctype { name, .. } => name.clone(),
        _ => String::new(),
    };
    Exception::dom(
        "InvalidNodeTypeError",
        format!("The node provided is of type '{name}'."),
    )
}

/// The checked boundary point of the API point `(node, offset)`, in bytes.
pub(crate) fn checked_point(rt: &mut Runtime, node: NodeRef, offset: u32) -> b::Result<Boundary> {
    let dom = arena_or_error(rt, node.document)?;
    if matches!(dom.node(node.node).data, NodeData::Doctype { .. }) {
        return Err(doctype_error(dom, node.node));
    }
    let bytes = match dom.char_data(node.node) {
        Some(text) => byte_offset(text, offset),
        None => (offset <= dom.node_length(node.node)).then_some(offset),
    };
    match bytes {
        Some(bytes) => Ok(Boundary::new(node.node, bytes)),
        None => Err(offset_error(dom, node.node, offset)),
    }
}

/// DOM "set the start or end" of the range `this` to the checked `point` of the
/// document `key`.
fn set_point(
    rt: &mut Runtime,
    this: Platform,
    key: DocKey,
    point: Boundary,
    start: bool,
) -> b::Result<()> {
    let (id, home, range) = current(rt, this)?;
    let new = if home == key {
        arena_or_error(rt, key)?.range_with(range, point, start)
    } else {
        // A node of another arena has another root.
        LiveRange::collapsed_at(point)
    };
    store(rt, id, key, new)
}

/// The point before (`after` false) or after a node, which must have a parent.
fn point_beside(rt: &mut Runtime, node: NodeRef, after: bool) -> b::Result<Boundary> {
    let dom = arena_or_error(rt, node.document)?;
    let Some(parent) = dom.parent(node.node) else {
        return Err(no_parent_error());
    };
    let index = dom.index_in_parent(node.node);
    Ok(Boundary::new(parent, index + u32::from(after)))
}

/// The wrapper of a boundary point's node.
fn container(rt: &mut Runtime, key: DocKey, node: NodeId) -> b::Result<Gc<Object>> {
    Ok(wrapper_in(rt, key, node)?)
}

/// The order of `point` (in the arena of `key`) against the range `range` of
/// the arena of `home`, with DOM's checks of `comparePoint` and
/// `isPointInRange`: `None` for a node in another root.
fn point_order(
    rt: &mut Runtime,
    home: DocKey,
    range: LiveRange,
    node: NodeRef,
    offset: u32,
) -> b::Result<Option<Ordering>> {
    if node.document != home {
        return Ok(None);
    }
    let dom = arena_or_error(rt, home)?;
    if dom.root(node.node) != dom.root(range.start.node) {
        return Ok(None);
    }
    let point = checked_point(rt, node, offset)?;
    let dom = arena_or_error(rt, home)?;
    Ok(Some(
        if dom.compare_boundaries(point, range.start) == Ordering::Less {
            Ordering::Less
        } else if dom.compare_boundaries(point, range.end) == Ordering::Greater {
            Ordering::Greater
        } else {
            Ordering::Equal
        },
    ))
}

impl g::abstract_range::AbstractRangeImpl for WebState {
    fn start_container(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        if let Some(data) = static_data(cx.rt, this) {
            let start = data.start;
            return container(cx.rt, start.key, start.node);
        }
        let (_, key, range) = current(cx.rt, this)?;
        container(cx.rt, key, range.start.node)
    }

    fn start_offset(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        if let Some(data) = static_data(cx.rt, this) {
            return Ok(data.start.offset);
        }
        let (_, key, range) = current(cx.rt, this)?;
        let dom = arena_or_error(cx.rt, key)?;
        Ok(api_offset(dom, range.start.node, range.start.offset))
    }

    fn end_container(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        if let Some(data) = static_data(cx.rt, this) {
            let end = data.end;
            return container(cx.rt, end.key, end.node);
        }
        let (_, key, range) = current(cx.rt, this)?;
        container(cx.rt, key, range.end.node)
    }

    fn end_offset(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        if let Some(data) = static_data(cx.rt, this) {
            return Ok(data.end.offset);
        }
        let (_, key, range) = current(cx.rt, this)?;
        let dom = arena_or_error(cx.rt, key)?;
        Ok(api_offset(dom, range.end.node, range.end.offset))
    }

    fn collapsed(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        if let Some(data) = static_data(cx.rt, this) {
            let (s, e) = (data.start, data.end);
            return Ok(s.key == e.key && s.node == e.node && s.offset == e.offset);
        }
        Ok(current(cx.rt, this)?.2.is_collapsed())
    }
}

impl g::static_range::StaticRangeImpl for WebState {
    /// `new StaticRange(init)`: a doctype container is refused (an `Attr` is no
    /// node in swb, so its conversion fails first); offsets are not checked.
    fn constructor(cx: &mut Cx<'_>, init: g::types::StaticRangeInit) -> b::Result<Gc<Object>> {
        for node in [&init.start_container, &init.end_container] {
            let dom = arena_or_error(cx.rt, node.document)?;
            if matches!(dom.node(node.node).data, NodeData::Doctype { .. }) {
                return Err(Exception::dom(
                    "InvalidNodeTypeError",
                    "Neither startContainer nor endContainer can be a DocumentType or Attribute node.",
                ));
            }
        }
        let point = |node: &NodeRef, offset| StaticPoint {
            key: node.document,
            node: node.node,
            offset,
        };
        let data = StaticRangeData {
            start: point(&init.start_container, init.start_offset),
            end: point(&init.end_container, init.end_offset),
        };
        let realm = cx.realm();
        let rt = &mut *cx.rt;
        let object = rt
            .alloc_host_data(data)
            .and_then(|data| rt.new_host_object(realm, g::STATIC_RANGE, data))?;
        Ok(object)
    }
}

impl g::range::RangeImpl for WebState {
    fn common_ancestor_container(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        let (_, key, range) = current(cx.rt, this)?;
        let node = arena_or_error(cx.rt, key)?.range_common_ancestor(range);
        container(cx.rt, key, node)
    }

    fn clone_contents(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        contents::clone_contents(cx, this)
    }

    fn create_contextual_fragment(
        cx: &mut Cx<'_>,
        this: Platform,
        string: String,
    ) -> b::Result<Gc<Object>> {
        contents::create_contextual_fragment(cx, this, &string)
    }

    fn delete_contents(cx: &mut Cx<'_>, this: Platform) -> b::Result<()> {
        contents::delete_contents(cx, this)
    }

    fn extract_contents(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        contents::extract_contents(cx, this)
    }

    fn get_bounding_client_rect(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        geometry::get_bounding_client_rect(cx, this)
    }

    fn get_client_rects(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        geometry::get_client_rects(cx, this)
    }

    fn insert_node(cx: &mut Cx<'_>, this: Platform, node: NodeRef) -> b::Result<()> {
        contents::insert_node(cx, this, node)
    }

    fn surround_contents(cx: &mut Cx<'_>, this: Platform, new_parent: NodeRef) -> b::Result<()> {
        contents::surround_contents(cx, this, new_parent)
    }

    /// `new Range()`: a range at `(document, 0)` of the current global's
    /// document.
    fn constructor(cx: &mut Cx<'_>) -> b::Result<Gc<Object>> {
        let realm = cx.realm();
        let key = state::document_of_realm(state::state(cx.rt)?, realm);
        let point = Boundary::new(NodeId::DOCUMENT, 0);
        create(cx.rt, realm, key, LiveRange::collapsed_at(point))
    }

    fn clone_range(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        let (_, key, range) = current(cx.rt, this)?;
        let realm = cx
            .rt
            .host_object_realm(swb_js::Value::Object(this.object))
            .unwrap_or_else(|| cx.realm());
        create(cx.rt, realm, key, range)
    }

    fn collapse(cx: &mut Cx<'_>, this: Platform, to_start: bool) -> b::Result<()> {
        let (id, key, range) = current(cx.rt, this)?;
        let point = if to_start { range.start } else { range.end };
        store(cx.rt, id, key, LiveRange::collapsed_at(point))
    }

    fn compare_boundary_points(
        cx: &mut Cx<'_>,
        this: Platform,
        how: u16,
        source_range: Platform,
    ) -> b::Result<i16> {
        if how > 3 {
            return Err(Exception::dom(
                "NotSupportedError",
                "The comparison method provided must be one of 'START_TO_START', 'START_TO_END', 'END_TO_END', or 'END_TO_START'.",
            ));
        }
        let (_, key, range) = current(cx.rt, this)?;
        let (_, source_key, source) = current(cx.rt, source_range)?;
        let different = || {
            Exception::dom(
                "WrongDocumentError",
                "The source range is in a different document than this range.",
            )
        };
        if key != source_key {
            return Err(different());
        }
        let dom = arena_or_error(cx.rt, key)?;
        if dom.root(range.start.node) != dom.root(source.start.node) {
            return Err(different());
        }
        let (this_point, other) = match how {
            0 => (range.start, source.start),
            1 => (range.end, source.start),
            2 => (range.end, source.end),
            _ => (range.start, source.end),
        };
        Ok(match dom.compare_boundaries(this_point, other) {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        })
    }

    fn compare_point(
        cx: &mut Cx<'_>,
        this: Platform,
        node: NodeRef,
        offset: u32,
    ) -> b::Result<i16> {
        let (_, home, range) = current(cx.rt, this)?;
        match point_order(cx.rt, home, range, node, offset)? {
            None => Err(Exception::dom(
                "WrongDocumentError",
                "The node provided and the Range are not in the same tree.",
            )),
            Some(Ordering::Less) => Ok(-1),
            Some(Ordering::Equal) => Ok(0),
            Some(Ordering::Greater) => Ok(1),
        }
    }

    fn detach(cx: &mut Cx<'_>, this: Platform) -> b::Result<()> {
        range_data(cx.rt, this).ok_or_else(illegal)?;
        Ok(())
    }

    fn intersects_node(cx: &mut Cx<'_>, this: Platform, node: NodeRef) -> b::Result<bool> {
        let (_, home, range) = current(cx.rt, this)?;
        if node.document != home {
            return Ok(false);
        }
        let dom = arena_or_error(cx.rt, home)?;
        if dom.root(node.node) != dom.root(range.start.node) {
            return Ok(false);
        }
        Ok(dom.range_intersects(range, node.node))
    }

    fn is_point_in_range(
        cx: &mut Cx<'_>,
        this: Platform,
        node: NodeRef,
        offset: u32,
    ) -> b::Result<bool> {
        let (_, home, range) = current(cx.rt, this)?;
        Ok(point_order(cx.rt, home, range, node, offset)? == Some(Ordering::Equal))
    }

    fn select_node(cx: &mut Cx<'_>, this: Platform, node: NodeRef) -> b::Result<()> {
        let (id, _, _) = current(cx.rt, this)?;
        let start = point_beside(cx.rt, node, false)?;
        let end = Boundary::new(start.node, start.offset + 1);
        store(cx.rt, id, node.document, LiveRange { start, end })
    }

    fn select_node_contents(cx: &mut Cx<'_>, this: Platform, node: NodeRef) -> b::Result<()> {
        let (id, _, _) = current(cx.rt, this)?;
        let dom = arena_or_error(cx.rt, node.document)?;
        if matches!(dom.node(node.node).data, NodeData::Doctype { .. }) {
            return Err(doctype_error(dom, node.node));
        }
        let length = dom.node_length(node.node);
        let range = LiveRange {
            start: Boundary::new(node.node, 0),
            end: Boundary::new(node.node, length),
        };
        store(cx.rt, id, node.document, range)
    }

    fn set_end(cx: &mut Cx<'_>, this: Platform, node: NodeRef, offset: u32) -> b::Result<()> {
        let point = checked_point(cx.rt, node, offset)?;
        set_point(cx.rt, this, node.document, point, false)
    }

    fn set_end_after(cx: &mut Cx<'_>, this: Platform, node: NodeRef) -> b::Result<()> {
        let point = point_beside(cx.rt, node, true)?;
        set_point(cx.rt, this, node.document, point, false)
    }

    fn set_end_before(cx: &mut Cx<'_>, this: Platform, node: NodeRef) -> b::Result<()> {
        let point = point_beside(cx.rt, node, false)?;
        set_point(cx.rt, this, node.document, point, false)
    }

    fn set_start(cx: &mut Cx<'_>, this: Platform, node: NodeRef, offset: u32) -> b::Result<()> {
        let point = checked_point(cx.rt, node, offset)?;
        set_point(cx.rt, this, node.document, point, true)
    }

    fn set_start_after(cx: &mut Cx<'_>, this: Platform, node: NodeRef) -> b::Result<()> {
        let point = point_beside(cx.rt, node, true)?;
        set_point(cx.rt, this, node.document, point, true)
    }

    fn set_start_before(cx: &mut Cx<'_>, this: Platform, node: NodeRef) -> b::Result<()> {
        let point = point_beside(cx.rt, node, false)?;
        set_point(cx.rt, this, node.document, point, true)
    }

    fn stringify(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        let (_, key, range) = current(cx.rt, this)?;
        Ok(arena_or_error(cx.rt, key)?.range_text(range))
    }
}

/// What a move of the nodes of `pairs` (old id in the document `from`, new id in
/// the document `to`) does to the live ranges (`state::move_node_state`): a
/// range with its boundary points in the moved subtree goes with it.
pub(crate) fn moved(
    table: &mut RangeTable,
    request: &Rc<Cell<bool>>,
    from: &mut DocState,
    to: &mut DocState,
    pairs: &[(NodeId, NodeId)],
) {
    if table.is_empty() {
        return;
    }
    let Some(source) = from.host.as_deref_mut() else {
        return;
    };
    let found = source.dom_mut().take_live_ranges(pairs);
    if found.is_empty() {
        return;
    }
    let Some(target) = to.host.as_deref_mut() else {
        // Nothing can run in a document without an arena; the entries go.
        for (id, _) in &found {
            table.home.remove(id);
        }
        return;
    };
    let dom = target.dom_mut();
    dom.set_range_work(ArenaWork::new(
        Rc::clone(&table.steps),
        Rc::clone(request),
        Rc::clone(&table.dead),
    ));
    for (id, _) in &found {
        table.home.insert(*id, to.key);
    }
    dom.put_live_ranges(found);
}
