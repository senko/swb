//! Nodes that move to another document (ADR 0031 section 5): the wrapper
//! slots a node can have, and the move of the state of a node (its wrappers,
//! `[SameObject]` objects, listeners and `Attr` objects) from the record of
//! one document to the record of another. The DOM side is `dom/adopt.rs`.

use swb_dom::{Document, NodeId};
use swb_js::{Gc, Object, Runtime, VmError, VmResult};

use super::{
    DocKey, DocState, NO_DOCUMENT, NO_DOCUMENT_FOR_KEY, Slot, borrow_document, document_ref, state,
    state_mut,
};
use crate::bindings as b;
use crate::host::EngineHost;

/// The `[SameObject]` objects that a node other than a document can have.
fn node_slots() -> impl Iterator<Item = Slot> {
    use crate::dom::ElementList as L;
    [
        Slot::ChildNodes,
        Slot::Children,
        Slot::ClassList,
        Slot::Attributes,
        Slot::Dataset,
        Slot::ClassName,
        Slot::Style,
        Slot::Validity,
        Slot::TokenList("rel"),
        Slot::TokenList("sizes"),
        Slot::TokenList("blocking"),
        Slot::TokenList("sandbox"),
        Slot::TokenList("for"),
        Slot::ElementList(L::TableRows),
        Slot::ElementList(L::TBodies),
        Slot::ElementList(L::SectionRows),
        Slot::ElementList(L::Cells),
        Slot::ElementList(L::SelectOptions),
        Slot::ElementList(L::SelectedOptions),
        Slot::ElementList(L::DatalistOptions),
        Slot::ElementList(L::FormControls),
        Slot::ElementList(L::FieldsetElements),
        Slot::ElementList(L::Areas),
        Slot::ElementList(L::Labels),
    ]
    .into_iter()
}

/// True if `node` of the current document went to another document.
pub(crate) fn has_moved(rt: &Runtime, node: NodeId) -> VmResult<bool> {
    let doc = &state(rt)?.doc;
    Ok(!doc.moved.is_empty() && doc.moved.contains_key(&node))
}

/// Gives the record of the document `key` its document, from the page's
/// registry if the task has not touched it yet.
pub(crate) fn ensure_lent(rt: &mut Runtime, key: DocKey) -> VmResult<()> {
    let state = state_mut(rt)?;
    if state.doc.key == key {
        return Ok(());
    }
    let Some(mut record) = state.docs.get_mut(key as usize).and_then(Option::take) else {
        return Err(VmError::type_error(NO_DOCUMENT_FOR_KEY));
    };
    if record.host.is_none() && !record.retired {
        borrow_document(state, &mut record);
    }
    if let Some(slot) = state.docs.get_mut(key as usize) {
        *slot = Some(record);
    }
    Ok(())
}

/// The DOM of the document `key`; [`ensure_lent`] came first.
pub(crate) fn dom_of(rt: &Runtime, key: DocKey) -> VmResult<&Document> {
    document_ref(state(rt)?, key)
        .and_then(|record| record.host.as_deref())
        .map(EngineHost::dom)
        .ok_or(VmError::type_error(NO_DOCUMENT))
}

/// What belongs to the nodes of `pairs` (old id, new id) goes from the document
/// `source` to the current one (ADR 0031 "Done (feature 12b2)"): each wrapper
/// gets its new word and stays the same object, with its expandos; its
/// listeners, handlers and `[SameObject]` objects go with it. A list or another
/// object with generic data finds its new place at its next use ([`settled`]).
/// The state of a node without a wrapper is only the script element's and the
/// mutation registry's.
pub(crate) fn move_node_state(
    rt: &mut Runtime,
    source: DocKey,
    pairs: &[(NodeId, NodeId)],
) -> VmResult<()> {
    let request = rt.host_signal_cell();
    let state_now = state_mut(rt)?;
    let target = state_now.doc.key;
    let Some(mut from) = state_now
        .docs
        .get_mut(source as usize)
        .and_then(Option::take)
    else {
        return Err(VmError::type_error(NO_DOCUMENT_FOR_KEY));
    };
    let to = &mut state_now.doc;
    let mut words: Vec<(Gc<Object>, u64)> = Vec::new();
    let mut with_wrapper: Vec<(NodeId, NodeId)> = Vec::new();
    for &(old, new) in pairs {
        from.moved.insert(old, (target, new));
        if !from.scripts.elements.is_empty() {
            crate::scripts::node_moved(&from.scripts, &mut to.scripts, old, new);
        }
        let Some(wrapper) = from.wrappers.get_mut(old.index()).and_then(Option::take) else {
            continue;
        };
        if to.wrappers.len() <= new.index() {
            to.wrappers.resize(new.index() + 1, None);
        }
        if let Some(slot) = to.wrappers.get_mut(new.index()) {
            *slot = Some(wrapper);
        }
        words.push((wrapper, b::word_of(target, new)));
        with_wrapper.push((old, new));
    }
    for &(old, new) in &with_wrapper {
        for slot in node_slots() {
            if let Some(object) = from.side.remove(&(old, slot)) {
                to.side.insert((new, slot), object);
                words.push((object, b::word_of(target, new)));
            }
        }
        if let Some(list) = from.listeners.remove(&old) {
            let count = list.count();
            from.listener_count = from.listener_count.saturating_sub(count);
            to.listener_count += count;
            to.listeners.insert(new, list);
        }
        if from.indeterminate.remove(&old) {
            to.indeterminate.insert(new);
        }
        if let Some(value) = from.output_defaults.remove(&old) {
            to.output_defaults.insert(new, value);
        }
        if let Some(promises) = from.decodes.remove(&old) {
            to.decodes.insert(new, promises);
        }
        from.style_blocks.forget(old);
        from.io.forget_target(old);
        from.ro.forget_target(old);
        move_attr_objects(&mut from, to, old, new);
    }
    if !with_wrapper.is_empty() {
        from.handlers.move_nodes(&mut to.handlers, &with_wrapper);
        from.selection_events.forget(&with_wrapper);
    }
    crate::node_iterator::moved(
        &mut state_now.iterators,
        &request,
        &mut state_now.docs,
        &mut from,
        to,
        pairs,
    );
    if let Some(slot) = state_now.docs.get_mut(source as usize) {
        *slot = Some(from);
    }
    let registered = state_now.mutation.move_nodes(source, target, pairs);
    for (object, word) in words {
        rt.set_host_word(object, word)?;
    }
    if registered {
        crate::mutation_observer::install_sink(rt)?;
    }
    Ok(())
}

/// The `Attr` objects of a moved element, which is in the arena of `to` now.
/// The data of each finds the new place of its element at its next use.
fn move_attr_objects(from: &mut DocState, to: &mut DocState, old: NodeId, new: NodeId) {
    let Some(host) = to.host.as_deref() else {
        return;
    };
    let Some(element) = host.dom().element(new) else {
        return;
    };
    for attribute in element.attributes() {
        let name = &attribute.name;
        let key = (old, name.ns.clone(), name.local.clone());
        if let Some(object) = from.attr_objects.remove(&key) {
            to.attr_objects
                .insert((new, name.ns.clone(), name.local.clone()), object);
        }
    }
}
