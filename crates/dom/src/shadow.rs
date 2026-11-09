//! Shadow trees (DOM §4.2.2 and §4.8; ADR 0036 section 1).
//!
//! A shadow root is a node of the arena ([`NodeData::ShadowRoot`]) without a
//! parent; its node document is its host's. The host has the `HOST` bit of
//! its shadow flags (`ElementData`'s padding), and the arena's
//! [`ShadowState`] maps the host to its root. The state also holds the slots
//! and their assigned nodes (`slots.rs`). It is `None` until the first
//! `attachShadow` of the arena, so that a page without shadow roots pays one
//! test per change.
//!
//! The walks of this module follow DOM "shadow-including tree order": a
//! host's shadow tree comes right after the host, before its children.
//! <https://dom.spec.whatwg.org/#concept-shadow-including-tree-order>
//! They are iterative: a script can nest 100,000 shadow trees.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use html5ever::{local_name, ns};

use crate::mutation::DomError;
use crate::tree::{
    Document, NodeData, NodeId, ShadowRootData, ShadowRootMode, SlotAssignment, shadow_flags,
};

/// The options of `attachShadow` (DOM `ShadowRootInit`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShadowRootInit {
    /// The mode.
    pub mode: ShadowRootMode,
    /// The slot assignment.
    pub slot_assignment: SlotAssignment,
    /// The delegates-focus flag.
    pub delegates_focus: bool,
    /// The clonable flag.
    pub clonable: bool,
    /// The serializable flag.
    pub serializable: bool,
}

/// Why `attachShadow` failed (DOM "attach a shadow root"; the bindings give
/// Chromium's messages).
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AttachError {
    /// The element is not in the HTML namespace or its local name is not a
    /// valid shadow host name.
    #[error("NotSupportedError: the element does not support shadow roots")]
    Unsupported,
    /// The element's custom element definition has the disable-shadow flag.
    #[error("NotSupportedError: shadow roots are disabled by the definition")]
    Disabled,
    /// The element is a shadow host already.
    #[error("NotSupportedError: the element hosts a shadow root already")]
    Exists,
    /// A limit of the arena.
    #[error(transparent)]
    Limit(#[from] DomError),
}

/// The names of the elements that can host a shadow root besides the valid
/// custom element names (DOM "valid shadow host name").
/// <https://dom.spec.whatwg.org/#valid-shadow-host-name>
const SHADOW_HOST_NAMES: [html5ever::LocalName; 18] = [
    local_name!("article"),
    local_name!("aside"),
    local_name!("blockquote"),
    local_name!("body"),
    local_name!("div"),
    local_name!("footer"),
    local_name!("h1"),
    local_name!("h2"),
    local_name!("h3"),
    local_name!("h4"),
    local_name!("h5"),
    local_name!("h6"),
    local_name!("header"),
    local_name!("main"),
    local_name!("nav"),
    local_name!("p"),
    local_name!("section"),
    local_name!("span"),
];

/// The shadow roots and slots of an arena.
#[derive(Debug, Default)]
pub(crate) struct ShadowState {
    /// The shadow root of each host.
    pub(crate) roots: HashMap<NodeId, NodeId>,
    /// The slots of each shadow root (`slots.rs`).
    pub(crate) slots: HashMap<NodeId, crate::slots::RootSlots>,
    /// The assigned nodes of each slot whose list is not empty.
    pub(crate) assigned: HashMap<NodeId, crate::slots::AssignedList>,
    /// The assigned slot of each assigned slottable.
    pub(crate) assigned_slot: HashMap<NodeId, NodeId>,
    /// The manually assigned nodes of each slot (`assign()`), and the
    /// manual slot assignment of each of those nodes.
    pub(crate) manual: HashMap<NodeId, Vec<NodeId>>,
    pub(crate) manual_slot: HashMap<NodeId, NodeId>,
    /// DOM "signal slots": the slots whose `slotchange` is due, without
    /// duplicates.
    pub(crate) signal_slots: Vec<NodeId>,
    pub(crate) signal_set: HashSet<NodeId>,
    /// The nodes that the assignment steps visited since the last
    /// [`Document::take_shadow_work`].
    pub(crate) work: usize,
    /// A shadow root was attached to a connected host since the last
    /// [`Document::take_shadow_changed`].
    pub(crate) changed: bool,
    /// The cell that asks the script runtime for attention
    /// ([`Document::set_shadow_attention`]).
    pub(crate) attention: Option<Rc<Cell<bool>>>,
}

impl Document {
    /// The shadow state, made at its first use.
    pub(crate) fn shadow_state(&mut self) -> &mut ShadowState {
        self.shadow.get_or_insert_default()
    }

    /// True if the arena has (or had) a shadow root or a manual slot
    /// assignment: the shadow steps of the change functions run only then.
    #[inline]
    pub fn has_shadow_state(&self) -> bool {
        self.shadow.is_some()
    }

    /// DOM "attach a shadow root" for `attachShadow`: checks the host in
    /// the DOM's order and makes its shadow root. `disabled` is the
    /// disable-shadow flag of the custom element definition that the
    /// registry of the host's document has for the host's name (the
    /// bindings look it up). No journal entry and no mutation record; a
    /// connected host sets the flag of [`Document::take_shadow_changed`].
    /// <https://dom.spec.whatwg.org/#concept-attach-a-shadow-root>
    ///
    /// # Errors
    ///
    /// The failed check, or the node limit.
    pub fn attach_shadow(
        &mut self,
        host: NodeId,
        init: ShadowRootInit,
        disabled: bool,
    ) -> Result<NodeId, AttachError> {
        let element = self.element(host).ok_or(AttachError::Unsupported)?;
        // Steps 1 and 2. Chromium accepts the 18 names of the standard in any
        // namespace (an SVG or MathML `div`, a `div` without a namespace) and
        // a custom element name in the HTML namespace only (measured, case
        // `attach-namespaces`); the standard wants the HTML namespace for
        // both.
        let local = &element.name.local;
        let valid = SHADOW_HOST_NAMES.contains(local)
            || (element.name.ns == ns!(html) && crate::custom::is_valid_custom_element_name(local));
        if !valid {
            return Err(AttachError::Unsupported);
        }
        // Step 3.
        if disabled {
            return Err(AttachError::Disabled);
        }
        // Step 4 (swb has no declarative roots).
        if element.is_shadow_host() {
            return Err(AttachError::Exists);
        }
        self.check_room(1, 0)?;
        let data = ShadowRootData {
            host,
            mode: init.mode,
            slot_assignment: init.slot_assignment,
            delegates_focus: init.delegates_focus,
            clonable: init.clonable,
            serializable: init.serializable,
            declarative: false,
        };
        Ok(self.make_shadow_root(host, data))
    }

    /// Makes the shadow root of `host` with `data` (after the checks).
    pub(crate) fn make_shadow_root(&mut self, host: NodeId, data: ShadowRootData) -> NodeId {
        let owner = self.node_document(host);
        let root = self.push_node(NodeData::ShadowRoot(data), owner);
        if let Some(e) = self.element_mut(host) {
            e.shadow_flags |= shadow_flags::HOST;
        }
        self.shadow_state().roots.insert(host, root);
        self.generation += 1;
        if self.is_connected(host) {
            self.shadow_state().changed = true;
        }
        root
    }

    /// True if a shadow root was attached to a connected host since the last
    /// call, and clears the flag (the engine restyles; no journal entry, as
    /// for [`Document::take_defined_changed`]).
    pub fn take_shadow_changed(&mut self) -> bool {
        self.shadow
            .as_mut()
            .is_some_and(|s| std::mem::take(&mut s.changed))
    }

    /// The shadow root of `host`, if it is a shadow host (also a closed
    /// root: the bindings decide what scripts see).
    #[inline]
    pub fn shadow_root(&self, host: NodeId) -> Option<NodeId> {
        if !self.element(host)?.is_shadow_host() {
            return None;
        }
        self.shadow.as_ref()?.roots.get(&host).copied()
    }

    /// The data of the shadow root `root`.
    pub fn shadow_root_data(&self, root: NodeId) -> Option<&ShadowRootData> {
        self.get(root)?.data.as_shadow_root()
    }

    /// The host of the shadow root `root`.
    pub fn shadow_host(&self, root: NodeId) -> Option<NodeId> {
        self.shadow_root_data(root).map(|d| d.host)
    }

    /// True if `node` is in a shadow tree (its root is a shadow root).
    pub fn in_shadow_tree(&self, node: NodeId) -> bool {
        self.shadow.is_some() && self.shadow_host(self.root(node)).is_some()
    }

    /// The parent of `node` in the shadow-including tree: its parent, or for
    /// a shadow root its host.
    #[inline]
    pub fn shadow_including_parent(&self, node: NodeId) -> Option<NodeId> {
        let n = self.node(node);
        n.parent.or_else(|| n.data.as_shadow_root().map(|d| d.host))
    }

    /// The shadow-including ancestors of `node`, nearest first (DOM
    /// "shadow-including ancestor"), without `node`.
    pub fn shadow_including_ancestors(&self, node: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        std::iter::successors(self.shadow_including_parent(node), |&n| {
            self.shadow_including_parent(n)
        })
    }

    /// DOM "shadow-including root": the root of `node`'s root's host, and so
    /// on, up to a root that is not a shadow root.
    /// <https://dom.spec.whatwg.org/#concept-shadow-including-root>
    pub fn shadow_including_root(&self, node: NodeId) -> NodeId {
        let mut root = self.root(node);
        while let Some(data) = self.node(root).data.as_shadow_root() {
            root = self.root(data.host);
        }
        root
    }

    /// True if `ancestor` is a shadow-including inclusive ancestor of `node`.
    pub fn is_shadow_including_inclusive_ancestor(&self, ancestor: NodeId, node: NodeId) -> bool {
        node == ancestor || self.shadow_including_ancestors(node).any(|a| a == ancestor)
    }

    /// The shadow-including inclusive descendants of `node` in
    /// shadow-including tree order.
    pub fn shadow_including_descendants(&self, node: NodeId) -> ShadowIncludingDescendants<'_> {
        ShadowIncludingDescendants {
            doc: self,
            root: node,
            next: Some(node),
            hosts: Vec::new(),
        }
    }

    /// True if `a` comes before `b` in shadow-including tree order. Both must
    /// have the same shadow-including root; otherwise the result is false.
    pub fn shadow_including_precedes(&self, a: NodeId, b: NodeId) -> bool {
        if a == b {
            return false;
        }
        if self.shadow.is_none() {
            return self.precedes(a, b);
        }
        let path = |n: NodeId| {
            let mut path: Vec<NodeId> = std::iter::once(n)
                .chain(self.shadow_including_ancestors(n))
                .collect();
            path.reverse();
            path
        };
        let (pa, pb) = (path(a), path(b));
        if pa.first() != pb.first() {
            return false;
        }
        let common = pa.iter().zip(&pb).take_while(|(x, y)| x == y).count();
        match (pa.get(common), pb.get(common)) {
            (None, Some(_)) => true,
            (Some(&ca), Some(&cb)) => {
                // A shadow root comes before its host's children.
                let shadow_a = self.node(ca).data.as_shadow_root().is_some();
                let shadow_b = self.node(cb).data.as_shadow_root().is_some();
                if shadow_a || shadow_b {
                    return shadow_a;
                }
                std::iter::successors(self.node(ca).next_sibling, |&n| self.node(n).next_sibling)
                    .any(|n| n == cb)
            }
            (_, None) => false,
        }
    }

    /// Installs the cell that the arena sets when a `slotchange` is due or
    /// when its shadow steps did much work: the runtime's signal, which makes
    /// the bindings look at [`Document::take_slot_signals`] and
    /// [`Document::take_shadow_work`]. Does nothing in an arena without
    /// shadow state (no slot can change there).
    pub fn set_shadow_attention(&mut self, cell: Rc<Cell<bool>>) {
        if let Some(state) = self.shadow.as_mut() {
            state.attention = Some(cell);
        }
    }

    /// Asks the host for attention (see [`Document::set_shadow_attention`]).
    pub(crate) fn request_shadow_attention(&self) {
        if let Some(cell) = self.shadow.as_ref().and_then(|s| s.attention.as_ref()) {
            cell.set(true);
        }
    }

    /// True if the shadow steps visited at least `limit` nodes since the last
    /// call, and resets the count then: the bindings check the time limit
    /// (native work that the countdown of the VM does not see).
    pub fn take_shadow_work(&mut self, limit: usize) -> bool {
        let Some(state) = self.shadow.as_mut() else {
            return false;
        };
        if state.work < limit {
            return false;
        }
        state.work = 0;
        true
    }

    /// Counts work of the shadow steps; asks for attention when it is large.
    pub(crate) fn count_shadow_work(&mut self, nodes: usize) {
        let Some(state) = self.shadow.as_mut() else {
            return;
        };
        state.work += nodes;
        if state.work >= crate::iterator::WORK_PER_CHECK {
            self.request_shadow_attention();
        }
    }
}

/// DOM "valid shadow host name": a valid custom element name, or one of
/// the 18 names of the standard.
pub fn is_valid_shadow_host_name(local: &html5ever::LocalName) -> bool {
    SHADOW_HOST_NAMES.contains(local) || crate::custom::is_valid_custom_element_name(local)
}

/// Iterator over the shadow-including inclusive descendants of a node. See
/// [`Document::shadow_including_descendants`].
pub struct ShadowIncludingDescendants<'a> {
    doc: &'a Document,
    root: NodeId,
    next: Option<NodeId>,
    /// The hosts whose shadow tree the walk is in, innermost last.
    hosts: Vec<NodeId>,
}

impl ShadowIncludingDescendants<'_> {
    /// The node after `current`.
    fn advance(&mut self, current: NodeId) -> Option<NodeId> {
        let doc = self.doc;
        if let Some(root) = doc.shadow_root(current) {
            self.hosts.push(current);
            return Some(root);
        }
        let mut n = current;
        let mut down = true;
        loop {
            if down && let Some(child) = doc.node(n).first_child {
                return Some(child);
            }
            if n == self.root {
                return None;
            }
            let node = doc.node(n);
            if let Some(sibling) = node.next_sibling {
                return Some(sibling);
            }
            if let Some(parent) = node.parent {
                n = parent;
                down = false;
            } else {
                // The end of a shadow tree: the host's children follow.
                n = self.hosts.pop()?;
                down = true;
            }
        }
    }
}

impl Iterator for ShadowIncludingDescendants<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let current = self.next?;
        self.next = self.advance(current);
        Some(current)
    }
}

impl ShadowRootMode {
    /// The IDL value.
    pub fn as_str(self) -> &'static str {
        match self {
            ShadowRootMode::Open => "open",
            ShadowRootMode::Closed => "closed",
        }
    }
}

impl SlotAssignment {
    /// The IDL value.
    pub fn as_str(self) -> &'static str {
        match self {
            SlotAssignment::Named => "named",
            SlotAssignment::Manual => "manual",
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
