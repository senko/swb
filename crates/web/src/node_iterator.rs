//! `NodeIterator`, `NodeFilter` and `Document.createNodeIterator` (DOM section 6,
//! <https://dom.spec.whatwg.org/#interface-nodeiterator>;
//! ADR 0028 section 17).
//!
//! The position of an iterator (root, reference node, pointer flag, and the
//! candidate of a running traversal) is Rust data in the arena of its root
//! (`swb_dom::Document::add_node_iterator`): the change functions of the arena
//! update it before a node leaves the tree (the pre-removing steps), and no
//! script object takes part. This module makes the JavaScript objects and runs
//! the filters.
//!
//! **Lifetime.** The object holds the id of its entry, its `whatToShow` value
//! and its filter (traced). The wrappers of the nodes live as long as their
//! document (ADR 0031 section 5), so the root and the reference need no
//! tracing, and a document keeps its arena for the life of the runtime. The
//! object's data ends the entry when the collector frees the object: it puts
//! the id on a list that the arenas share ([`swb_dom::DeadIterators`]; the
//! collector cannot reach an arena), and the next removal in an arena drops the
//! entry. The next `createNodeIterator` clears the list and the table's side. The
//! runtime has no limit on live iterators.
//!
//! **Documents.** The entry is in the arena of the root's document. The object is
//! in the realm of the document that `createNodeIterator` was called on, and
//! names the root's document in its host word, so a call on the iterator runs
//! in the root's document, and a root that moves to another document takes the
//! entry along ([`moved`]). The reference and the candidate are nodes, not
//! places in the root's tree: a node that script moves to another document stays
//! the reference or the candidate, as a node of that document
//! (`swb_dom::Spot::Away`). The table keeps an index of such nodes, so that a
//! move finds the iterators that point at them ([`IteratorTable::away_at`]).
//! Every node that this module turns into an object goes through [`wrapper_in`],
//! which follows a node that moved ([`state::settled`]).
//!
//! **Time.** A traversal over many nodes with no filter is native work: the loop
//! checks the time limit every [`WORK_PER_CHECK`] steps; the work of the
//! pre-removing steps is counted in the arena and charged by `after_request`
//! ([`take_overdue`]).

use std::cell::Cell;
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use swb_dom::{DeadIterators, IteratorWork, NodeData, NodeId, Spot, WORK_PER_CHECK};
use swb_js::{Gc, GenericData, HostData, Object, Runtime, Tracer, Value, VmResult};

use crate::bindings::convert::{self, IntMode};
use crate::bindings::{self as b, Cx, Exception, NodeRef, Platform};
use crate::generated as g;
use crate::generated::types::NodeFilter;
use crate::state::{self, DocKey, DocState, WebState, state_mut};

/// The nodes of other documents that an iterator points at: the reference and
/// the candidate.
type AwayPointers = [Option<(DocKey, NodeId)>; 2];

/// The iterators of the runtime.
#[derive(Default)]
pub(crate) struct IteratorTable {
    next_id: u32,
    /// The document whose arena holds the entry of each live iterator.
    home: HashMap<u32, DocKey>,
    /// The ids of iterators that the collector freed.
    dead: Rc<DeadIterators>,
    /// The work of the pre-removing steps since the host last charged it.
    steps: Rc<Cell<usize>>,
    /// The nodes in other documents that each iterator points at.
    away: HashMap<u32, AwayPointers>,
    /// `(document, node, id)` for each entry of `away`.
    away_at: BTreeSet<(DocKey, NodeId, u32)>,
}

impl IteratorTable {
    /// True if no iterator exists: the paths of the host skip the table.
    pub(crate) fn is_empty(&self) -> bool {
        self.home.is_empty()
    }

    /// Records where iterator `id` points in other documents.
    fn track(&mut self, id: u32, now: AwayPointers) {
        let old = self.away.get(&id).copied().unwrap_or([None, None]);
        if old == now {
            return;
        }
        for (key, node) in old.into_iter().flatten() {
            self.away_at.remove(&(key, node, id));
        }
        for (key, node) in now.into_iter().flatten() {
            self.away_at.insert((key, node, id));
        }
        if now == [None, None] {
            self.away.remove(&id);
        } else {
            self.away.insert(id, now);
        }
    }

    /// Drops what the table knows about iterator `id`.
    fn forget(&mut self, id: u32) -> Option<DocKey> {
        self.track(id, [None, None]);
        self.home.remove(&id)
    }
}

/// The host data of a `NodeIterator`.
struct IteratorData {
    id: u32,
    what_to_show: u32,
    filter: Option<Gc<Object>>,
    /// The "active" flag of DOM "filter".
    active: Cell<bool>,
    /// The root's document and node.
    word: u64,
    dead: Rc<DeadIterators>,
}

impl GenericData for IteratorData {
    fn host_word(&self) -> Option<u64> {
        Some(self.word)
    }

    fn trace(&self, tracer: &mut Tracer<'_>) {
        if let Some(filter) = self.filter {
            tracer.object(filter);
        }
    }

    fn heap_size(&self) -> usize {
        // The entry in the arena and in the table.
        96
    }
}

impl Drop for IteratorData {
    fn drop(&mut self) {
        self.dead.push(self.id);
    }
}

fn illegal() -> Exception {
    Exception::type_error("Illegal invocation")
}

fn iterator_data(rt: &Runtime, this: Platform) -> b::Result<&IteratorData> {
    match this.data {
        HostData::Generic(handle) => rt
            .heap()
            .generic::<IteratorData>(handle)
            .map_err(|_| illegal()),
        HostData::Word(_) => Err(illegal()),
    }
}

/// The arena of document `key`, if the runtime has it now.
fn arena(rt: &mut Runtime, key: DocKey) -> Option<&mut swb_dom::Document> {
    state::ensure_lent(rt, key).ok()?;
    let state = rt.host_data_mut::<WebState>()?;
    let record = state::document_mut(state, key)?;
    Some(record.host.as_deref_mut()?.dom_mut())
}

/// The key of the document whose arena holds the entry of iterator `id`.
fn home_of(rt: &Runtime, id: u32) -> Option<DocKey> {
    state::state(rt).ok()?.iterators.home.get(&id).copied()
}

/// The arena that holds the entry of iterator `id`.
fn arena_of(rt: &mut Runtime, id: u32) -> Option<&mut swb_dom::Document> {
    let key = home_of(rt, id)?;
    arena(rt, key)
}

/// Removes the entries of the iterators that the collector freed and clears
/// the list.
fn reap(rt: &mut Runtime) -> VmResult<()> {
    let dead = Rc::clone(&state::state(rt)?.iterators.dead);
    for id in dead.take() {
        let Some(key) = state_mut(rt)?.iterators.forget(id) else {
            continue;
        };
        // A document that is not available has no script that could run in it.
        if let Some(dom) = arena(rt, key) {
            dom.remove_node_iterator(id);
        }
    }
    Ok(())
}

/// True once the work of the pre-removing steps passed the unit that the host
/// charges to the time limit; the count starts again.
pub(crate) fn take_overdue(rt: &Runtime) -> bool {
    let Ok(state) = state::state(rt) else {
        return false;
    };
    if state.iterators.steps.get() < WORK_PER_CHECK {
        return false;
    }
    state.iterators.steps.set(0);
    true
}

/// What a move of the nodes of `pairs` (old id in the document `from`, new id in
/// the document `to`) does to the iterators (`state::move_node_state`): the
/// iterators with their root among the old nodes take their entry along; a
/// pointer of another iterator at an old node follows the node, as a node of
/// the document `to`; and a pointer that was in another document and comes back
/// to the arena of its iterator is a node there again. `others` are the other
/// documents of the runtime.
pub(crate) fn moved(
    table: &mut IteratorTable,
    request: &Rc<Cell<bool>>,
    others: &mut [Option<Box<DocState>>],
    from: &mut DocState,
    to: &mut DocState,
    pairs: &[(NodeId, NodeId)],
) {
    if table.is_empty() {
        return;
    }
    let work = IteratorWork::new(
        Rc::clone(&table.steps),
        Rc::clone(request),
        Rc::clone(&table.dead),
    );
    if let Some(source) = from.host.as_deref_mut() {
        let dom = source.dom_mut();
        let found = dom.take_node_iterators(pairs, from.key);
        let sent = dom.send_node_pointers(pairs, to.key);
        put_entries(table, work, to, found);
        for id in sent {
            track_in(table, from, id);
        }
    }
    // Pointers that were in `from`, at nodes of an iterator of another arena.
    for &(old, new) in pairs {
        let ids: Vec<u32> = table
            .away_at
            .range((from.key, old, 0)..=(from.key, old, u32::MAX))
            .map(|&(_, _, id)| id)
            .collect();
        for id in ids {
            let Some(&home) = table.home.get(&id) else {
                continue;
            };
            let spot = if home == to.key {
                Spot::Here(new)
            } else {
                Spot::Away(to.key, new)
            };
            let record = if home == to.key {
                Some(&mut *to)
            } else {
                others
                    .get_mut(home as usize)
                    .and_then(|slot| slot.as_deref_mut())
            };
            let Some(record) = record else {
                continue;
            };
            let Some(host) = record.host.as_deref_mut() else {
                continue;
            };
            host.dom_mut().receive_node_pointer(id, from.key, old, spot);
            track_in(table, record, id);
        }
    }
}

/// Puts the entries that came with their root into the arena of `to`.
fn put_entries(
    table: &mut IteratorTable,
    work: IteratorWork,
    to: &mut DocState,
    found: Vec<(u32, swb_dom::IteratorPosition)>,
) {
    if found.is_empty() {
        return;
    }
    let Some(target) = to.host.as_deref_mut() else {
        // Nothing can run in a document without an arena; the entries go.
        for (id, _) in &found {
            table.forget(*id);
        }
        return;
    };
    let dom = target.dom_mut();
    dom.set_iterator_work(work);
    let ids: Vec<u32> = found.iter().map(|(id, _)| *id).collect();
    for id in &ids {
        table.home.insert(*id, to.key);
    }
    dom.put_node_iterators(found, to.key);
    for id in ids {
        let away = dom.away_pointers(id);
        table.track(id, away);
    }
}

/// Reads where iterator `id` points in other documents from the arena of
/// `record`, which holds its entry.
fn track_in(table: &mut IteratorTable, record: &mut DocState, id: u32) {
    if let Some(host) = record.host.as_deref() {
        table.track(id, host.dom().away_pointers(id));
    }
}

/// `Document.createNodeIterator` (DOM "create a NodeIterator"): the entry goes
/// into the arena of `root`'s document, the object into the realm of the
/// document that the method was called on.
pub(crate) fn create(
    cx: &mut Cx<'_>,
    root: NodeRef,
    what_to_show: u32,
    filter: Option<NodeFilter>,
) -> b::Result<Gc<Object>> {
    reap(cx.rt)?;
    let realm = cx.realm();
    let rt = &mut *cx.rt;
    state::ensure_lent(rt, root.document)?;
    let request = rt.host_signal_cell();
    let table = &mut state_mut(rt)?.iterators;
    let id = table
        .next_id
        .checked_add(1)
        .ok_or_else(|| Exception::range_error("Too many NodeIterator objects were made."))?;
    table.next_id = id;
    let dead = Rc::clone(&table.dead);
    let work = IteratorWork::new(Rc::clone(&table.steps), request, Rc::clone(&dead));
    let Some(dom) = arena(rt, root.document) else {
        return Err(Exception::type_error("the document is not available"));
    };
    dom.set_iterator_work(work);
    dom.add_node_iterator(id, root.node);
    state_mut(rt)?.iterators.home.insert(id, root.document);
    let data = IteratorData {
        id,
        what_to_show,
        filter: filter.map(|f| f.0),
        active: Cell::new(false),
        word: b::word_of(root.document, root.node),
        dead,
    };
    // The data frees the entry if anything below fails.
    let object = rt
        .alloc_host_data(data)
        .and_then(|data| rt.new_host_object(realm, g::NODE_ITERATOR, data));
    Ok(object?)
}

/// The id of the entry of `this`.
fn entry(rt: &Runtime, this: Platform) -> b::Result<u32> {
    Ok(iterator_data(rt, this)?.id)
}

/// The position of the iterator `this`.
fn position(rt: &mut Runtime, this: Platform) -> b::Result<swb_dom::IteratorPosition> {
    let id = entry(rt, this)?;
    arena_of(rt, id)
        .and_then(|dom| dom.node_iterator(id))
        .ok_or_else(illegal)
}

/// The document and node that a spot of iterator `id` names now. A node that
/// moved since the spot was made is at its new place ([`state::settled`]).
fn place_of(rt: &mut Runtime, id: u32, spot: Spot) -> VmResult<(DocKey, NodeId)> {
    let (key, node) = match spot {
        Spot::Here(node) => match home_of(rt, id) {
            Some(home) => (home, node),
            None => return Err(swb_js::VmError::type_error("the iterator is gone")),
        },
        Spot::Away(key, node) => (key, node),
    };
    Ok(state::settled(state_mut(rt)?, key, node))
}

/// The wrapper of `node` of the document `key`, made in that document's realm.
fn wrapper_in(rt: &mut Runtime, key: DocKey, node: NodeId) -> VmResult<Gc<Object>> {
    state::ensure_lent(rt, key)?;
    let entered = state::switch_if_other(rt, key)?;
    let wrapper = state::wrap_node_with_proto(rt, node, None);
    state::leave(rt, entered);
    wrapper
}

impl g::node_iterator::NodeIteratorImpl for WebState {
    fn root(cx: &mut Cx<'_>, this: Platform) -> b::Result<NodeId> {
        Ok(position(cx.rt, this)?.root)
    }

    fn reference_node(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        let id = entry(cx.rt, this)?;
        let reference = position(cx.rt, this)?.reference;
        let (key, node) = place_of(cx.rt, id, reference)?;
        Ok(wrapper_in(cx.rt, key, node)?)
    }

    fn pointer_before_reference_node(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(position(cx.rt, this)?.pointer_before)
    }

    fn what_to_show(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        Ok(iterator_data(cx.rt, this)?.what_to_show)
    }

    fn filter(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<NodeFilter>> {
        Ok(iterator_data(cx.rt, this)?.filter.map(NodeFilter))
    }

    fn next_node(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<Gc<Object>>> {
        traverse(cx, this, true)
    }

    fn previous_node(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<Gc<Object>>> {
        traverse(cx, this, false)
    }

    fn detach(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<()> {
        Ok(())
    }
}

/// One step of "traverse" from `spot` with the flag `before`: the spot of the
/// next node and the flag after the step. The tree of the root bounds a step
/// from a node of the iterator's arena; a node of another document moves
/// inside its own tree (Chromium; measured).
fn step_from(
    rt: &mut Runtime,
    id: u32,
    root: NodeId,
    spot: Spot,
    before: bool,
    forward: bool,
) -> Option<(Spot, bool)> {
    match spot {
        Spot::Here(node) => {
            let dom = arena_of(rt, id)?;
            let (next, flag) = dom.iterator_step(Some(root), node, before, forward)?;
            Some((Spot::Here(next), flag))
        }
        Spot::Away(key, node) => {
            let (key, node) = state::settled(state_mut(rt).ok()?, key, node);
            let dom = arena(rt, key)?;
            let (next, flag) = dom.iterator_step(None, node, before, forward)?;
            Some((Spot::Away(key, next), flag))
        }
    }
}

/// DOM "traverse" (<https://dom.spec.whatwg.org/#concept-nodeiterator-traverse>).
/// The candidate that the steps walk is in the arena, so a filter that changes
/// the tree moves it with the tree (Chromium; measured).
fn traverse(cx: &mut Cx<'_>, this: Platform, forward: bool) -> b::Result<Option<Gc<Object>>> {
    let rt = &mut *cx.rt;
    let (id, what_to_show, filter, active) = {
        let data = iterator_data(rt, this)?;
        (data.id, data.what_to_show, data.filter, data.active.get())
    };
    if active {
        // A call from the iterator's own filter: the filter step of its first node
        // throws, and nothing else changes, in particular not the candidate of the
        // traversal that runs.
        let first = position(rt, this)
            .ok()
            .and_then(|p| step_from(rt, id, p.root, p.reference, p.pointer_before, forward));
        return match first {
            None => Ok(None),
            Some(_) => Err(Exception::dom(
                "InvalidStateError",
                "Filter function can't be recursive",
            )),
        };
    }
    let started = arena_of(rt, id).is_some_and(|dom| dom.node_iterator_begin(id));
    if !started {
        return Ok(None);
    }
    let result = walk(rt, this, id, what_to_show, filter, forward);
    if let Some(home) = home_of(rt, id)
        && let Some(dom) = arena(rt, home)
    {
        match &result {
            Ok(Some(_)) => dom.node_iterator_accept(id),
            _ => dom.node_iterator_end(id),
        }
        let away = dom.away_pointers(id);
        if let Ok(state) = state_mut(rt) {
            state.iterators.track(id, away);
        }
    }
    match result? {
        Some((key, node)) => Ok(Some(wrapper_in(rt, key, node)?)),
        None => Ok(None),
    }
}

/// The loop of "traverse": the node that the filter accepted, or `None` at the
/// end of the collection.
fn walk(
    rt: &mut Runtime,
    this: Platform,
    id: u32,
    what_to_show: u32,
    filter: Option<Gc<Object>>,
    forward: bool,
) -> b::Result<Option<(DocKey, NodeId)>> {
    let mut steps = 0usize;
    loop {
        steps += 1;
        if steps.is_multiple_of(WORK_PER_CHECK) {
            rt.check_time()?;
        }
        // The arena and the candidate are read again at each step: a filter can
        // move the root, and so the entry, to another document.
        let Some(position) = arena_of(rt, id).and_then(|dom| dom.node_iterator(id)) else {
            return Ok(None);
        };
        let Some((candidate, before)) = position.candidate else {
            return Ok(None);
        };
        let Some((next, flag)) = step_from(rt, id, position.root, candidate, before, forward)
        else {
            return Ok(None);
        };
        if let Some(dom) = arena_of(rt, id) {
            dom.node_iterator_set_candidate(id, next, flag);
            dom.charge_iterator_work(1);
        }
        let place = place_of(rt, id, next)?;
        let method = if forward { "nextNode" } else { "previousNode" };
        if accepts(rt, this, place, what_to_show, filter, method)? {
            return Ok(Some(place));
        }
    }
}

/// DOM "filter" (<https://dom.spec.whatwg.org/#concept-node-filter>) of the node
/// `place`: true if the result is `FILTER_ACCEPT`.
fn accepts(
    rt: &mut Runtime,
    this: Platform,
    place: (DocKey, NodeId),
    what_to_show: u32,
    filter: Option<Gc<Object>>,
    method: &str,
) -> b::Result<bool> {
    let data = iterator_data(rt, this)?;
    if data.active.get() {
        return Err(Exception::dom(
            "InvalidStateError",
            "Filter function can't be recursive",
        ));
    }
    let (key, node) = place;
    let Some(kind) = node_type(rt, key, node) else {
        return Ok(false);
    };
    // Bit `nodeType - 1` of `whatToShow` (a type of 32 or more has no bit).
    if what_to_show & (1u32 << (kind - 1)) == 0 {
        return Ok(false);
    }
    let Some(filter) = filter else {
        return Ok(true);
    };
    let argument = wrapper_in(rt, key, node)?;
    b::keep(rt, Value::Object(argument));
    let data = iterator_data(rt, this)?;
    data.active.set(true);
    let called = call_filter(rt, filter, argument, method);
    if let Ok(data) = iterator_data(rt, this) {
        data.active.set(false);
    }
    Ok(called? == 1)
}

/// The `nodeType` of `node` in the arena of the document `key`.
fn node_type(rt: &mut Runtime, key: DocKey, node: NodeId) -> Option<u32> {
    state::ensure_lent(rt, key).ok()?;
    let dom = state::dom_of(rt, key).ok()?;
    Some(match dom.get(node)?.data {
        NodeData::Element(_) => 1,
        NodeData::Text(_) => 3,
        NodeData::ProcessingInstruction { .. } => 7,
        NodeData::Comment(_) => 8,
        NodeData::Document => 9,
        NodeData::Doctype { .. } => 10,
        NodeData::DocumentFragment => 11,
    })
}

/// Web IDL "call a user object's operation" `acceptNode` of `filter`, and the
/// conversion of the result to `unsigned short`. A callable filter is called
/// with `this` undefined; `acceptNode` is read at each call.
fn call_filter(
    rt: &mut Runtime,
    filter: Gc<Object>,
    argument: Gc<Object>,
    method: &str,
) -> b::Result<u16> {
    let value = Value::Object(filter);
    let (function, this) = if rt.callable(value)? {
        (value, Value::Undefined)
    } else {
        let key = rt.key_from_str("acceptNode")?;
        let method = rt.get_key(value, key)?;
        if !rt.callable(method)? {
            return Err(Exception::type_error(
                "Failed to execute 'acceptNode' on 'NodeFilter': The provided callback is not callable.",
            ));
        }
        (method, value)
    };
    let result = rt.call(function, this, &[Value::Object(argument)])?;
    convert::unsigned_short(rt, result, IntMode::Plain).map_err(|e| match e {
        // Chromium names the callback first and the method of the iterator
        // after it, the other way round than the glue does.
        Exception::Type(message) => Exception::Js(swb_js::VmError::type_error(format!(
            "Failed to execute 'acceptNode' on 'NodeFilter': Failed to execute '{method}' on 'NodeIterator': {message}"
        ))),
        other => other,
    })
}
