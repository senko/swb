//! `TreeWalker` and `Document.createTreeWalker` (DOM section 6.2,
//! <https://dom.spec.whatwg.org/#interface-treewalker>; M10 feature 9a).
//!
//! A walker is a root, a current node, a `whatToShow` mask, a filter and the
//! "active" flag of DOM "filter". Unlike a `NodeIterator` it has no position that
//! the pre-removing steps update: its steps read the tree when they run, so
//! the walker keeps no entry in an arena. The root is the host word of the
//! object (so a call runs in the root's document and a root that script moves to
//! another document is followed, as for `NodeIterator`); the current node is a
//! document key and a node id in the object's data, followed through
//! [`state::settled`] when it is read.
//!
//! **Lifetime.** The object holds its filter (traced). The wrappers of the nodes live
//! as long as their document (ADR 0031 section 5), so the root and the current
//! node need no tracing and there is nothing to free.
//!
//! **Documents.** The walker is in the realm of the document that
//! `createTreeWalker` was called on and walks the tree of the node it is at:
//! a current node that script moved to another document walks the tree of that
//! document, and is never the root (the root is a node of the other one), as in
//! Chromium. The wrappers that a call makes belong to the realm of the node's
//! document ([`wrapper_in`]).
//!
//! **Time.** A walk with no filter, or with a mask that skips most nodes, is
//! native work: the loops check the time limit every [`WORK_PER_CHECK`] steps, as
//! `NodeIterator` does.

use std::cell::Cell;

use swb_dom::{NodeId, WORK_PER_CHECK};
use swb_js::{Gc, GenericData, HostData, Object, Runtime, Tracer, Value};

use crate::bindings::convert;
use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::generated as g;
use crate::generated::types::NodeFilter;
use crate::node_iterator::{call_filter, node_type, wrapper_in};
use crate::state::{self, DocKey, WebState, state_mut};

/// A node and the document whose arena holds it.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Place {
    key: DocKey,
    node: NodeId,
}

/// The host data of a `TreeWalker`.
struct WalkerData {
    what_to_show: u32,
    filter: Option<Gc<Object>>,
    /// The "active" flag of DOM "filter".
    active: Cell<bool>,
    /// The current node.
    current: Cell<Place>,
    /// The root's document and node.
    word: u64,
}

impl GenericData for WalkerData {
    fn host_word(&self) -> Option<u64> {
        Some(self.word)
    }

    fn set_host_word(&mut self, word: u64) {
        self.word = word;
    }

    fn trace(&self, tracer: &mut Tracer<'_>) {
        if let Some(filter) = self.filter {
            tracer.object(filter);
        }
    }

    fn heap_size(&self) -> usize {
        64
    }
}

fn illegal() -> Exception {
    Exception::type_error("Illegal invocation")
}

fn walker_data(rt: &Runtime, this: Platform) -> b::Result<&WalkerData> {
    match this.data {
        HostData::Generic(handle) => rt
            .heap()
            .generic::<WalkerData>(handle)
            .map_err(|_| illegal()),
        HostData::Word(_) => Err(illegal()),
    }
}

/// The root of the walker `this`, with its new place if it moved.
fn root_of(rt: &mut Runtime, this: Platform) -> b::Result<Place> {
    let HostData::Generic(handle) = this.data else {
        return Err(illegal());
    };
    state::settle_generic(rt, handle)?;
    let word = rt
        .heap()
        .generic_host_word(handle)
        .map_err(|_| illegal())?
        .ok_or_else(illegal)?;
    let (key, node) = b::word_parts(word).ok_or_else(illegal)?;
    Ok(Place { key, node })
}

/// `Document.createTreeWalker` (DOM "create a TreeWalker"): the object is in
/// the realm of the document that the method was called on, its root names the
/// root's document in its host word.
pub(crate) fn create(
    cx: &mut Cx<'_>,
    root: b::NodeRef,
    what_to_show: u32,
    filter: Option<NodeFilter>,
) -> b::Result<Gc<Object>> {
    let realm = cx.realm();
    let rt = &mut *cx.rt;
    let data = WalkerData {
        what_to_show,
        filter: filter.map(|f| f.0),
        active: Cell::new(false),
        current: Cell::new(Place {
            key: root.document,
            node: root.node,
        }),
        word: b::word_of(root.document, root.node),
    };
    let object = rt
        .alloc_host_data(data)
        .and_then(|data| rt.new_host_object(realm, g::TREE_WALKER, data))?;
    Ok(object)
}

/// The state of one call: the walker, its mask and filter, the traversal method
/// (for messages) and the work counter.
struct Walk<'a> {
    rt: &'a mut Runtime,
    this: Platform,
    what_to_show: u32,
    filter: Option<Gc<Object>>,
    method: &'static str,
    steps: usize,
}

/// `next` for the siblings and `first` for the children.
type Forward = bool;

impl Walk<'_> {
    fn new<'a>(rt: &'a mut Runtime, this: Platform, method: &'static str) -> b::Result<Walk<'a>> {
        let (what_to_show, filter) = {
            let data = walker_data(rt, this)?;
            (data.what_to_show, data.filter)
        };
        Ok(Walk {
            rt,
            this,
            what_to_show,
            filter,
            method,
            steps: 0,
        })
    }

    /// Counts one step of a loop and checks the time limit now and then.
    fn tick(&mut self) -> b::Result<()> {
        self.steps += 1;
        if self.steps.is_multiple_of(WORK_PER_CHECK) {
            self.rt.check_time()?;
        }
        Ok(())
    }

    fn settle(&mut self, place: Place) -> Place {
        match state_mut(self.rt) {
            Ok(state) => {
                let (key, node) = state::settled(state, place.key, place.node);
                Place { key, node }
            }
            Err(_) => place,
        }
    }

    fn root(&mut self) -> b::Result<Place> {
        root_of(self.rt, self.this)
    }

    fn current(&mut self) -> b::Result<Place> {
        let place = walker_data(self.rt, self.this)?.current.get();
        let place = self.settle(place);
        walker_data(self.rt, self.this)?.current.set(place);
        Ok(place)
    }

    fn set_current(&mut self, place: Place) -> b::Result<()> {
        walker_data(self.rt, self.this)?.current.set(place);
        Ok(())
    }

    fn is_root(&mut self, place: Place) -> b::Result<bool> {
        let place = self.settle(place);
        Ok(place == self.root()?)
    }

    /// The node that `read` finds from `place` in its arena.
    fn relative(
        &mut self,
        place: Place,
        read: impl FnOnce(&swb_dom::Node) -> Option<NodeId>,
    ) -> Option<Place> {
        let place = self.settle(place);
        state::ensure_lent(self.rt, place.key).ok()?;
        let dom = state::dom_of(self.rt, place.key).ok()?;
        let node = read(dom.get(place.node)?)?;
        Some(Place {
            key: place.key,
            node,
        })
    }

    fn parent(&mut self, place: Place) -> Option<Place> {
        self.relative(place, swb_dom::Node::parent)
    }

    fn child(&mut self, place: Place, first: Forward) -> Option<Place> {
        if first {
            self.relative(place, swb_dom::Node::first_child)
        } else {
            self.relative(place, swb_dom::Node::last_child)
        }
    }

    fn sibling(&mut self, place: Place, next: Forward) -> Option<Place> {
        if next {
            self.relative(place, swb_dom::Node::next_sibling)
        } else {
            self.relative(place, swb_dom::Node::prev_sibling)
        }
    }

    /// DOM "filter" (<https://dom.spec.whatwg.org/#concept-node-filter>): the
    /// result of the mask and the filter for `place`.
    fn filter(&mut self, place: Place) -> b::Result<u16> {
        if walker_data(self.rt, self.this)?.active.get() {
            return Err(Exception::dom(
                "InvalidStateError",
                "Filter function can't be recursive",
            ));
        }
        let place = self.settle(place);
        let Some(kind) = node_type(self.rt, place.key, place.node) else {
            return Ok(FILTER_SKIP);
        };
        // Bit `nodeType - 1` of `whatToShow` (a type of 32 or more has no bit).
        if self.what_to_show & (1u32 << (kind - 1)) == 0 {
            return Ok(FILTER_SKIP);
        }
        let Some(filter) = self.filter else {
            return Ok(FILTER_ACCEPT);
        };
        let argument = wrapper_in(self.rt, place.key, place.node)?;
        b::keep(self.rt, Value::Object(argument));
        walker_data(self.rt, self.this)?.active.set(true);
        let called = call_filter(self.rt, filter, argument, "TreeWalker", self.method);
        // Also when the filter threw: the walker works again (measured in
        // Chromium 148).
        if let Ok(data) = walker_data(self.rt, self.this) {
            data.active.set(false);
        }
        called
    }

    /// Makes `place` the current node and returns its wrapper.
    fn accept(&mut self, place: Place) -> b::Result<Option<Gc<Object>>> {
        let place = self.settle(place);
        self.set_current(place)?;
        Ok(Some(wrapper_in(self.rt, place.key, place.node)?))
    }

    /// DOM `parentNode()`.
    fn parent_node(&mut self) -> b::Result<Option<Gc<Object>>> {
        let mut node = Some(self.current()?);
        while let Some(at) = node {
            self.tick()?;
            if self.is_root(at)? {
                break;
            }
            node = self.parent(at);
            if let Some(parent) = node
                && self.filter(parent)? == FILTER_ACCEPT
            {
                return self.accept(parent);
            }
        }
        Ok(None)
    }

    /// DOM "traverse children" (<https://dom.spec.whatwg.org/#concept-traverse-children>).
    fn traverse_children(&mut self, first: Forward) -> b::Result<Option<Gc<Object>>> {
        let current = self.current()?;
        let mut node = self.child(current, first);
        while let Some(at) = node {
            self.tick()?;
            let result = self.filter(at)?;
            if result == FILTER_ACCEPT {
                return self.accept(at);
            }
            if result == FILTER_SKIP
                && let Some(child) = self.child(at, first)
            {
                node = Some(child);
                continue;
            }
            let mut up = at;
            node = loop {
                self.tick()?;
                if let Some(sibling) = self.sibling(up, first) {
                    break Some(sibling);
                }
                let Some(parent) = self.parent(up) else {
                    return Ok(None);
                };
                let current = self.current()?;
                if self.is_root(parent)? || self.settle(parent) == current {
                    return Ok(None);
                }
                up = parent;
            };
        }
        Ok(None)
    }

    /// DOM "traverse siblings" (<https://dom.spec.whatwg.org/#concept-traverse-siblings>).
    fn traverse_siblings(&mut self, next: Forward) -> b::Result<Option<Gc<Object>>> {
        let mut node = self.current()?;
        if self.is_root(node)? {
            return Ok(None);
        }
        loop {
            let mut sibling = self.sibling(node, next);
            while let Some(at) = sibling {
                self.tick()?;
                node = at;
                let result = self.filter(node)?;
                if result == FILTER_ACCEPT {
                    return self.accept(node);
                }
                sibling = self.child(node, next);
                if result == FILTER_REJECT || sibling.is_none() {
                    sibling = self.sibling(node, next);
                }
            }
            self.tick()?;
            let Some(parent) = self.parent(node) else {
                return Ok(None);
            };
            node = parent;
            if self.is_root(node)? || self.filter(node)? == FILTER_ACCEPT {
                return Ok(None);
            }
        }
    }

    /// DOM `previousNode()`.
    fn previous_node(&mut self) -> b::Result<Option<Gc<Object>>> {
        let mut node = self.current()?;
        while !self.is_root(node)? {
            let mut sibling = self.sibling(node, false);
            while let Some(at) = sibling {
                self.tick()?;
                node = at;
                let mut result = self.filter(node)?;
                while result != FILTER_REJECT
                    && let Some(last) = self.child(node, false)
                {
                    self.tick()?;
                    node = last;
                    result = self.filter(node)?;
                }
                if result == FILTER_ACCEPT {
                    return self.accept(node);
                }
                sibling = self.sibling(node, false);
            }
            if self.is_root(node)? {
                return Ok(None);
            }
            let Some(parent) = self.parent(node) else {
                return Ok(None);
            };
            self.tick()?;
            node = parent;
            if self.filter(node)? == FILTER_ACCEPT {
                return self.accept(node);
            }
        }
        Ok(None)
    }

    /// DOM `nextNode()`.
    fn next_node(&mut self) -> b::Result<Option<Gc<Object>>> {
        let mut node = self.current()?;
        let mut result = FILTER_ACCEPT;
        loop {
            while result != FILTER_REJECT
                && let Some(child) = self.child(node, true)
            {
                self.tick()?;
                node = child;
                result = self.filter(node)?;
                if result == FILTER_ACCEPT {
                    return self.accept(node);
                }
            }
            // The standard filters `node` again when the climb ends above the
            // top of a tree that is not the root's (the current node is outside
            // the root's tree, or a filter detached it); Chromium returns
            // `null` there (measured, probe cases `outside-root` and `mutation`).
            let mut temporary = Some(node);
            loop {
                let Some(at) = temporary else {
                    return Ok(None);
                };
                self.tick()?;
                if self.is_root(at)? {
                    return Ok(None);
                }
                if let Some(sibling) = self.sibling(at, true) {
                    node = sibling;
                    break;
                }
                temporary = self.parent(at);
            }
            result = self.filter(node)?;
            if result == FILTER_ACCEPT {
                return self.accept(node);
            }
        }
    }
}

const FILTER_ACCEPT: u16 = 1;
const FILTER_REJECT: u16 = 2;
const FILTER_SKIP: u16 = 3;

impl g::tree_walker::TreeWalkerImpl for WebState {
    fn root(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        let root = root_of(cx.rt, this)?;
        Ok(wrapper_in(cx.rt, root.key, root.node)?)
    }

    fn what_to_show(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        Ok(walker_data(cx.rt, this)?.what_to_show)
    }

    fn filter(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<NodeFilter>> {
        Ok(walker_data(cx.rt, this)?.filter.map(NodeFilter))
    }

    fn current_node(cx: &mut Cx<'_>, this: Platform) -> b::Result<Value> {
        let mut walk = Walk::new(cx.rt, this, "currentNode")?;
        let current = walk.current()?;
        Ok(Value::Object(wrapper_in(
            walk.rt,
            current.key,
            current.node,
        )?))
    }

    /// The setter takes any node of any document of the runtime (DOM: a
    /// `Node`); a value that is no node is a `TypeError` with the message of the
    /// glue's `Node` conversion.
    fn set_current_node(cx: &mut Cx<'_>, this: Platform, value: Value) -> b::Result<()> {
        let node = convert::node_ref(cx.rt, value, g::NODE, "Node")
            .map_err(|_| Exception::type_error("Failed to convert value to 'Node'."))?;
        walker_data(cx.rt, this)?.current.set(Place {
            key: node.document,
            node: node.node,
        });
        Ok(())
    }

    fn parent_node(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<Gc<Object>>> {
        Walk::new(cx.rt, this, "parentNode")?.parent_node()
    }

    fn first_child(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<Gc<Object>>> {
        Walk::new(cx.rt, this, "firstChild")?.traverse_children(true)
    }

    fn last_child(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<Gc<Object>>> {
        Walk::new(cx.rt, this, "lastChild")?.traverse_children(false)
    }

    fn previous_sibling(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<Gc<Object>>> {
        Walk::new(cx.rt, this, "previousSibling")?.traverse_siblings(false)
    }

    fn next_sibling(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<Gc<Object>>> {
        Walk::new(cx.rt, this, "nextSibling")?.traverse_siblings(true)
    }

    fn previous_node(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<Gc<Object>>> {
        Walk::new(cx.rt, this, "previousNode")?.previous_node()
    }

    fn next_node(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<Gc<Object>>> {
        Walk::new(cx.rt, this, "nextNode")?.next_node()
    }
}
