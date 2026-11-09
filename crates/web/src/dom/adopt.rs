//! Nodes between the documents of a runtime (DOM "adopt",
//! <https://dom.spec.whatwg.org/#concept-node-adopt>, and `importNode`; ADR 0031
//! "Done (feature 12b2)"). Each document of a frame has its own arena, so a move
//! takes the subtree out of one arena and puts it into the other
//! (`swb_dom::Document::take_subtree`); each wrapper of a moved node gets its new
//! word and stays the same object (`state::move_node_state`).
//!
//! The documents that a script makes (`createHTMLDocument`, `DOMParser`, the
//! inert documents of template contents) share the arena of their window's
//! document, so a move between them changes no id and does not come here.

use std::collections::HashMap;

use swb_dom::{NodeData, NodeId, local_name};
use swb_js::Runtime;

use super::dom_result;
use crate::bindings as b;
use crate::bindings::NodeRef;
use crate::state::{self, dom, dom_mut, engine_mut};

/// Moves the node `node` of another document, with its subtree, into the arena
/// of the current document, with the node document of `owner`. Returns its id
/// here. A script can run in the steps of the removal (`blur`, `unload` of a
/// frame in the subtree) and move the node again; the steps go on from where the
/// node is then. A subtree that does not fit ends the task, as a node limit does
/// everywhere, and stays whole in its document.
pub(crate) fn transplant(rt: &mut Runtime, node: NodeRef, owner: NodeId) -> b::Result<NodeId> {
    let target = state::document_key(rt)?;
    check_room(rt, node.document, node.node)?;
    leave_parent(rt, node)?;
    let node = node.refreshed(rt).unwrap_or(node);
    if node.document == target {
        // A script of the removal moved it here.
        let result = dom_mut(rt)?.adopt(node.node, owner);
        dom_result(rt, result)?;
        return Ok(node.node);
    }
    move_detached(rt, node.document, node.node, owner)
}

/// Takes the subtree of the node `node` of the document `document` and puts it
/// into the current document. What the engine holds for the removal is settled
/// before the nodes go (a `style` element leaves its document with its sheet).
/// No script runs from the check of the room to the put, so the put cannot fail
/// with the subtree out of its arena; a script before it can have filled the arena.
fn move_detached(
    rt: &mut Runtime,
    document: state::DocKey,
    node: NodeId,
    owner: NodeId,
) -> b::Result<NodeId> {
    let target = state::document_key(rt)?;
    state::ensure_lent(rt, document)?;
    let entered = state::switch_if_other(rt, document)?;
    let taken = (|| -> b::Result<_> {
        dom_mut(rt)?.remove(node);
        engine_mut(rt)?.settle_style_sheets();
        let size = dom(rt)?.subtree_size(node);
        let room = state::dom_of(rt, target)?.check_subtree_room(size);
        dom_result(rt, room)?;
        let controls = engine_mut(rt)?.take_control_snapshots(node);
        let old_owner = dom(rt)?.node_document(node);
        let subtree = dom_mut(rt)?.take_subtree(node);
        Ok((controls, subtree, old_owner))
    })();
    state::leave(rt, entered);
    let (controls, subtree, old_owner) = taken?;
    let result = dom_mut(rt)?.put_subtree(owner, subtree);
    let placed = dom_result(rt, result)?;
    state::move_node_state(rt, document, &placed.pairs)?;
    // DOM "adopt": the callbacks of the nodes that came from another arena.
    crate::custom_elements::adopted_from(rt, placed.root, (document, old_owner))?;
    put_controls(rt, controls, &placed.pairs)?;
    rt.check_time()?;
    Ok(placed.root)
}

/// The node limits of the current document's arena hold the subtree of `node` of
/// the document `document`.
fn check_room(rt: &mut Runtime, document: state::DocKey, node: NodeId) -> b::Result<()> {
    state::ensure_lent(rt, document)?;
    let size = state::dom_of(rt, document)?.subtree_size(node);
    let result = dom(rt)?.check_subtree_room(size);
    dom_result(rt, result)
}

/// The steps of a removal in the node's document, if it has a parent: a frame
/// inside goes and a focused element inside loses the focus, while the node is
/// still in its place.
fn leave_parent(rt: &mut Runtime, node: NodeRef) -> b::Result<()> {
    let entered = state::switch_if_other(rt, node.document)?;
    let result = (|| -> b::Result<()> {
        if dom(rt)?.parent(node.node).is_some() {
            crate::html::focus::before_removal(rt, node.node, true)?;
            if let Some(now) = node.refreshed(rt)
                && now == node
            {
                dom_mut(rt)?.remove(node.node);
                crate::html::focus::fixup(rt)?;
            }
        }
        Ok(())
    })();
    state::leave(rt, entered);
    result
}

/// The state of the controls among the moved nodes goes to their new document.
fn put_controls(
    rt: &mut Runtime,
    controls: Vec<(NodeId, crate::ControlSnapshot)>,
    pairs: &[(NodeId, NodeId)],
) -> b::Result<()> {
    if controls.is_empty() {
        return Ok(());
    }
    let index: HashMap<NodeId, NodeId> = pairs.iter().copied().collect();
    for (old, snapshot) in controls {
        let Some(&new) = index.get(&old) else {
            continue;
        };
        if engine_mut(rt)?.put_control_snapshot(new, snapshot) == crate::ControlWrite::OverLimit {
            return Err(rt.terminate("the budget of form control values").into());
        }
    }
    Ok(())
}

/// What an insertion of `node`, of another document, inserts here: a fragment
/// stays where it is, and a new fragment here gets its children (measured:
/// the fragment keeps its node document). Any other node moves here.
pub(crate) fn bring_here(rt: &mut Runtime, node: NodeRef, parent: NodeId) -> b::Result<NodeId> {
    state::ensure_lent(rt, node.document)?;
    let is_fragment = matches!(
        state::dom_of(rt, node.document)?.node(node.node).data,
        NodeData::DocumentFragment
    );
    if !is_fragment {
        return transplant(rt, node, parent);
    }
    check_room(rt, node.document, node.node)?;
    let children: Vec<NodeId> = state::dom_of(rt, node.document)?
        .children(node.node)
        .collect();
    let document = dom(rt)?.node_document(parent);
    let result = dom_mut(rt)?.create_node_in(document, NodeData::DocumentFragment);
    let holder = dom_result(rt, result)?;
    // The children leave the fragment together, with one record there.
    let entered = state::switch_if_other(rt, node.document)?;
    let result = dom_mut(rt)?.replace_all(node.node, None);
    let cleared = dom_result(rt, result);
    state::leave(rt, entered);
    cleared?;
    for child in children {
        let moved = move_detached(rt, node.document, child, parent)?;
        let result = dom_mut(rt)?.append_child(holder, moved);
        dom_result(rt, result)?;
    }
    Ok(holder)
}

/// A node argument of a setter or a method whose checks read only the kind of
/// the node (`document.body`, `table.caption`, `select.add`): the node here,
/// moved if it belongs to another document.
pub(crate) fn brought_for_setter(
    rt: &mut Runtime,
    node: NodeRef,
    parent: NodeId,
) -> b::Result<NodeId> {
    match node.here(rt) {
        Some(here) => Ok(here),
        None => bring_here(rt, node, parent),
    }
}

/// DOM "clone a node" of `node`, of another document, with the node document
/// of `owner`, for `importNode`: the copy gets what the cloning steps give it,
/// the "already started" flag of a script and the state of a control.
pub(crate) fn import_here(
    rt: &mut Runtime,
    node: NodeRef,
    owner: NodeId,
    deep: bool,
) -> b::Result<NodeId> {
    state::ensure_lent(rt, node.document)?;
    let subtree = state::dom_of(rt, node.document)?.copy_subtree(node.node, deep);
    let result = dom_mut(rt)?.put_subtree(owner, subtree);
    let placed = dom_result(rt, result)?;
    let entered = state::switch_if_other(rt, node.document)?;
    let copied = (|| -> b::Result<_> {
        let started = placed
            .pairs
            .iter()
            .filter(|&&(old, _)| {
                crate::scripts::has_started(rt, old).unwrap_or(false)
                    && dom(rt).is_ok_and(|d| d.is_html_element(old, &local_name!("script")))
            })
            .map(|&(_, new)| new)
            .collect::<Vec<_>>();
        let controls = engine_mut(rt)?.copy_control_snapshots(node.node, deep);
        Ok((started, controls))
    })();
    state::leave(rt, entered);
    let (started, controls) = copied?;
    for script in started {
        dom_mut(rt)?.mark_script_started(script);
    }
    crate::custom_elements::upgrade::enqueue_copies(rt, placed.root)?;
    put_controls(rt, controls, &placed.pairs)?;
    rt.check_time()?;
    Ok(placed.root)
}
