//! Slots and slot assignment (DOM §4.2.2.1 to §4.2.2.5, "signal a slot
//! change"; ADR 0036 section 2).
//!
//! Assignment is eager: the change functions run the DOM's steps at each
//! insertion into a host, at each insertion or removal of a slot in a
//! shadow tree, at a change of a slot's `name` or of a slottable's `slot`
//! attribute, and at `assign()`. The steps keep the list of assigned nodes
//! of each slot, the assigned slot of each slottable and the `SLOT` bit of
//! the slots, and signal a slot change ("signal slots": the bindings fire
//! `slotchange` in the mutation observer microtask) when a list changes.
//! They run only when the arena has a [`ShadowState`](crate::shadow) and
//! the change touches a host or a slot.
//!
//! A shadow root keeps its slots by name in tree order: DOM "find a slot"
//! of the named assignment is the first slot of the name, so a change of
//! the slots changes an assignment only when the first slot of a name
//! changes (`RootSlots::first`). The steps work on the names that a change
//! touches; the result equals DOM "assign slottables for a tree" of the
//! whole root, which visits every slot.

use std::collections::{HashMap, HashSet, VecDeque};

use html5ever::{QualName, local_name, ns};

use crate::tree::{Document, ElementData, NodeData, NodeId, SlotAssignment, shadow_flags};

/// The slots of one shadow root.
#[derive(Debug, Default)]
pub(crate) struct RootSlots {
    /// The slots of each name, in tree order.
    by_name: HashMap<String, VecDeque<NodeId>>,
    /// The first slot of each name when its assignment was last computed.
    first: HashMap<String, NodeId>,
}

/// The assigned nodes of one slot. Removing the first node moves a start
/// offset instead of the entries, so that a host loses its children from
/// the front at a constant cost; the dead entries go when they are half of
/// the vector.
#[derive(Debug, Default)]
pub(crate) struct AssignedList {
    items: Vec<NodeId>,
    head: usize,
}

impl AssignedList {
    fn new(items: Vec<NodeId>) -> Self {
        Self { items, head: 0 }
    }

    /// The nodes, in order.
    pub(crate) fn as_slice(&self) -> &[NodeId] {
        self.items.get(self.head..).unwrap_or(&[])
    }

    fn push(&mut self, node: NodeId) {
        self.items.push(node);
    }

    /// Removes the node at position `at` of [`AssignedList::as_slice`];
    /// returns the number of entries that moved.
    fn remove(&mut self, at: usize) -> usize {
        let mut moved = 0;
        if at == 0 {
            self.head += 1;
        } else if at + 1 < self.as_slice().len() {
            moved = self.as_slice().len() - at - 1;
            self.items.remove(self.head + at);
        } else {
            self.items.pop();
        }
        if self.head >= 32 && self.head * 2 >= self.items.len() {
            moved += self.items.len() - self.head;
            self.items.drain(..self.head);
            self.head = 0;
        }
        moved
    }

    pub(crate) fn len(&self) -> usize {
        self.as_slice().len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }
}

/// A subtree of at most this many nodes is searched for slots at once.
const SMALL_SUBTREE: usize = 32;

/// A slot is "near an end" of its list when comparing it with the end
/// takes at most this many siblings.
const NEAR_END: usize = 8;

/// Moved list entries that count as one step of work.
const MOVES_PER_STEP: usize = 16;

/// The position of `node` in a list of `len` entries (`at(i)` is entry
/// `i`), found by a search from both ends, and the entries that it
/// visited.
fn position_from_ends(
    len: usize,
    at: impl Fn(usize) -> Option<NodeId>,
    node: NodeId,
) -> (Option<usize>, usize) {
    let (mut low, mut high) = (0, len);
    let mut steps = 1;
    while low < high {
        if at(low) == Some(node) {
            return (Some(low), steps);
        }
        high -= 1;
        if at(high) == Some(node) {
            return (Some(high), steps);
        }
        low += 1;
        steps += 1;
    }
    (None, steps)
}

/// New lists of assigned nodes, applied in tree order of the slots.
type Changes = Vec<(NodeId, Vec<NodeId>)>;

impl Document {
    /// True if `node` is an HTML `slot` element.
    #[inline]
    pub fn is_slot(&self, node: NodeId) -> bool {
        self.element(node)
            .is_some_and(|e| e.is_html_named(&local_name!("slot")))
    }

    /// True if `node` is a slottable (an element or a text node).
    #[inline]
    fn is_slottable(&self, node: NodeId) -> bool {
        matches!(
            self.node(node).data,
            NodeData::Element(_) | NodeData::Text(_)
        )
    }

    /// The name of a slot: its `name` attribute or the empty string.
    fn slot_name_of(&self, slot: NodeId) -> &str {
        self.element(slot)
            .and_then(|e| e.attr("name"))
            .unwrap_or("")
    }

    /// The name of a slottable: the `slot` attribute of an element, else
    /// the empty string.
    fn slottable_name(&self, node: NodeId) -> &str {
        self.element(node)
            .and_then(|e| e.attr("slot"))
            .unwrap_or("")
    }

    /// The assigned nodes of `slot` (DOM "assigned nodes"), empty for a node
    /// that is no slot or is not in a shadow tree.
    pub fn assigned_nodes(&self, slot: NodeId) -> &[NodeId] {
        if !self
            .element(slot)
            .is_some_and(ElementData::has_assigned_nodes)
        {
            return &[];
        }
        self.shadow
            .as_ref()
            .and_then(|s| s.assigned.get(&slot))
            .map_or(&[], AssignedList::as_slice)
    }

    /// The assigned slot of a slottable, if it is assigned.
    pub fn assigned_slot(&self, node: NodeId) -> Option<NodeId> {
        self.shadow.as_ref()?.assigned_slot.get(&node).copied()
    }

    /// DOM "find a slot" for `slottable`; with `open`, `None` for a slot in a
    /// closed shadow root (`assignedSlot`).
    /// <https://dom.spec.whatwg.org/#find-a-slot>
    pub fn find_slot(&self, slottable: NodeId, open: bool) -> Option<NodeId> {
        if !self.is_slottable(slottable) {
            return None;
        }
        let parent = self.parent(slottable)?;
        let root = self.shadow_root(parent)?;
        let data = self.shadow_root_data(root)?;
        if open && data.mode == crate::tree::ShadowRootMode::Closed {
            return None;
        }
        match data.slot_assignment {
            SlotAssignment::Manual => self
                .shadow
                .as_ref()?
                .manual_slot
                .get(&slottable)
                .copied()
                .filter(|&slot| self.root(slot) == root),
            SlotAssignment::Named => self.first_slot(root, self.slottable_name(slottable)),
        }
    }

    /// DOM "find flattened slottables" (`assignedNodes({flatten: true})`):
    /// the assigned nodes of `slot`, or its slottable children if it has
    /// none, with each slot of a shadow tree among them replaced by its own
    /// flattened list. Empty for a slot outside a shadow tree.
    /// <https://dom.spec.whatwg.org/#find-flattened-slotables>
    pub fn flattened_assigned_nodes(&self, slot: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        if !self.is_slot(slot) || !self.in_shadow_tree(slot) {
            return out;
        }
        let mut stack = vec![self.slottables_or_fallback(slot).into_iter()];
        while let Some(top) = stack.last_mut() {
            match top.next() {
                None => {
                    stack.pop();
                }
                Some(node) if self.is_slot(node) && self.in_shadow_tree(node) => {
                    stack.push(self.slottables_or_fallback(node).into_iter());
                }
                Some(node) => out.push(node),
            }
        }
        out
    }

    /// The assigned nodes of `slot`, or its slottable children if it has
    /// none.
    fn slottables_or_fallback(&self, slot: NodeId) -> Vec<NodeId> {
        let assigned = self.assigned_nodes(slot);
        if assigned.is_empty() {
            self.children(slot)
                .filter(|&c| self.is_slottable(c))
                .collect()
        } else {
            assigned.to_vec()
        }
    }

    /// HTML `assign(...nodes)` of the slot `slot`: its manually assigned
    /// nodes become `nodes` (without duplicates), each node leaves the
    /// manual list of its earlier slot, and the assignment runs again for
    /// the slots that changed.
    /// <https://html.spec.whatwg.org/multipage/scripting.html#dom-slot-assign>
    pub fn assign_slot_manually(&mut self, slot: NodeId, nodes: &[NodeId]) {
        let state = self.shadow_state();
        // Step 1.
        for node in state.manual.remove(&slot).unwrap_or_default() {
            if state.manual_slot.get(&node) == Some(&slot) {
                state.manual_slot.remove(&node);
            }
        }
        // Steps 2 to 4. The nodes leave the lists of their earlier slots
        // together: one pass over each list.
        let mut left: HashMap<NodeId, HashSet<NodeId>> = HashMap::new();
        let mut set = Vec::with_capacity(nodes.len());
        let mut seen = HashSet::with_capacity(nodes.len());
        for &node in nodes {
            if !seen.insert(node) {
                continue;
            }
            if let Some(previous) = state.manual_slot.insert(node, slot)
                && previous != slot
            {
                left.entry(previous).or_default().insert(node);
            }
            set.push(node);
        }
        let mut touched = Vec::with_capacity(left.len() + 1);
        for (previous, gone) in left {
            if let Some(list) = state.manual.get_mut(&previous) {
                state.work += list.len();
                list.retain(|n| !gone.contains(n));
            }
            touched.push(previous);
        }
        state.work += nodes.len();
        if !set.is_empty() {
            state.manual.insert(slot, set);
        }
        // Step 5, for the slots whose lists can have changed.
        touched.push(slot);
        touched.sort_unstable();
        touched.dedup();
        let changes: Changes = touched
            .into_iter()
            .map(|s| (s, self.find_slottables(s)))
            .collect();
        self.apply_changes(changes);
        if self
            .shadow
            .as_ref()
            .is_some_and(|s| s.work >= crate::iterator::WORK_PER_CHECK)
        {
            self.request_shadow_attention();
        }
    }

    /// Takes the signal slots (DOM "signal slots"): the slots whose
    /// `slotchange` event is due, in the order of their signals.
    pub fn take_slot_signals(&mut self) -> Vec<NodeId> {
        let Some(state) = self.shadow.as_mut() else {
            return Vec::new();
        };
        state.signal_set.clear();
        std::mem::take(&mut state.signal_slots)
    }

    /// Gives `slots`, taken by [`Document::take_slot_signals`] and not
    /// fired, back (a termination interrupted the delivery): they go before
    /// the slots signalled since, in their order.
    pub fn restore_slot_signals(&mut self, slots: &[NodeId]) {
        let state = self.shadow_state();
        let mut list: Vec<NodeId> = slots
            .iter()
            .copied()
            .filter(|&slot| state.signal_set.insert(slot))
            .collect();
        list.append(&mut state.signal_slots);
        state.signal_slots = list;
        self.request_shadow_attention();
    }

    /// True if a slot change was signalled and not taken yet.
    pub fn has_slot_signals(&self) -> bool {
        self.shadow
            .as_ref()
            .is_some_and(|s| !s.signal_slots.is_empty())
    }

    /// DOM "signal a slot change": `slot` joins the signal slots once.
    /// <https://dom.spec.whatwg.org/#signal-a-slot-change>
    fn signal_slot(&mut self, slot: NodeId) {
        let state = self.shadow_state();
        if state.signal_set.insert(slot) {
            state.signal_slots.push(slot);
        }
        self.request_shadow_attention();
    }

    /// DOM "find slottables" from the slot's own data.
    /// <https://dom.spec.whatwg.org/#find-slotables>
    fn find_slottables(&self, slot: NodeId) -> Vec<NodeId> {
        let root = self.root(slot);
        let Some(data) = self.shadow_root_data(root) else {
            return Vec::new();
        };
        let host = data.host;
        match data.slot_assignment {
            SlotAssignment::Manual => self
                .shadow
                .as_ref()
                .and_then(|s| s.manual.get(&slot))
                .map(|list| {
                    list.iter()
                        .copied()
                        .filter(|&n| self.parent(n) == Some(host))
                        .collect()
                })
                .unwrap_or_default(),
            SlotAssignment::Named => {
                let name = self.slot_name_of(slot);
                if self.first_slot(root, name) == Some(slot) {
                    self.named_slottables(host, name)
                } else {
                    Vec::new()
                }
            }
        }
    }

    /// The slottable children of `host` whose name is `name`, in tree order.
    fn named_slottables(&self, host: NodeId, name: &str) -> Vec<NodeId> {
        self.children(host)
            .filter(|&c| self.is_slottable(c) && self.slottable_name(c) == name)
            .collect()
    }

    /// The first slot of the name `name` in the shadow tree of `root`.
    fn first_slot(&self, root: NodeId, name: &str) -> Option<NodeId> {
        self.shadow
            .as_ref()?
            .slots
            .get(&root)?
            .by_name
            .get(name)?
            .front()
            .copied()
    }

    /// [`Document::set_assigned`] for a list that gets `node` at its end (an
    /// append to the host): no copy of the list, so that appending many
    /// children stays linear.
    fn append_assigned(&mut self, slot: NodeId, node: NodeId) {
        let Some(state) = self.shadow.as_mut() else {
            return;
        };
        state.assigned.entry(slot).or_default().push(node);
        state.assigned_slot.insert(node, slot);
        state.work += 1;
        if let Some(e) = self.element_mut(slot) {
            e.shadow_flags |= shadow_flags::SLOT;
        }
        self.signal_slot(slot);
    }

    /// [`Document::set_assigned`] for a list that loses `node` (a removal
    /// from the host).
    fn remove_assigned(&mut self, slot: NodeId, node: NodeId) {
        let Some(state) = self.shadow.as_mut() else {
            return;
        };
        let Some(list) = state.assigned.get_mut(&slot) else {
            return;
        };
        // The search goes from both ends, so that removing the first or
        // the last node of a long list is cheap.
        let (at, steps) = position_from_ends(list.len(), |i| list.as_slice().get(i).copied(), node);
        let Some(at) = at else {
            state.work += steps;
            return;
        };
        let moved = list.remove(at);
        state.work += steps + moved / MOVES_PER_STEP;
        let empty = list.is_empty();
        if empty {
            state.assigned.remove(&slot);
        }
        if state.assigned_slot.get(&node) == Some(&slot) {
            state.assigned_slot.remove(&node);
        }
        if empty && let Some(e) = self.element_mut(slot) {
            e.shadow_flags &= !shadow_flags::SLOT;
        }
        self.signal_slot(slot);
    }

    /// Sets the assigned nodes of `slot` (DOM "assign slottables", the end):
    /// signals a slot change if the list differs, and keeps the assigned
    /// slots of the nodes and the `SLOT` bit.
    fn set_assigned(&mut self, slot: NodeId, nodes: Vec<NodeId>) {
        let Some(state) = self.shadow.as_mut() else {
            return;
        };
        let same = match state.assigned.get(&slot) {
            Some(old) => old.as_slice() == nodes.as_slice(),
            None => nodes.is_empty(),
        };
        if same {
            return;
        }
        let old = state.assigned.remove(&slot).unwrap_or_default();
        state.work += old.len() + nodes.len();
        for node in old.as_slice() {
            if state.assigned_slot.get(node) == Some(&slot) {
                state.assigned_slot.remove(node);
            }
        }
        for &node in &nodes {
            state.assigned_slot.insert(node, slot);
        }
        let filled = !nodes.is_empty();
        if filled {
            state.assigned.insert(slot, AssignedList::new(nodes));
        }
        if let Some(e) = self.element_mut(slot) {
            if filled {
                e.shadow_flags |= shadow_flags::SLOT;
            } else {
                e.shadow_flags &= !shadow_flags::SLOT;
            }
        }
        self.signal_slot(slot);
    }

    /// Applies new lists: the slots that left their shadow tree first, then
    /// those of a shadow tree in tree order. Chromium signals a removed slot
    /// before the slots of the tree that it left (measured, case
    /// `slotchange-order`); DOM "remove" runs "assign slottables for a tree"
    /// of the tree first.
    fn apply_changes(&mut self, mut changes: Changes) {
        if changes.len() > 1 {
            let (mut inside, mut outside): (Changes, Changes) = changes
                .into_iter()
                .partition(|(slot, _)| self.in_shadow_tree(*slot));
            inside.sort_by(|(a, _), (b, _)| {
                if self.precedes(*a, *b) {
                    std::cmp::Ordering::Less
                } else if self.precedes(*b, *a) {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Equal
                }
            });
            outside.extend(inside);
            changes = outside;
        }
        for (slot, nodes) in changes {
            self.set_assigned(slot, nodes);
        }
    }

    /// The changes of the named assignment of the name `name` in the shadow
    /// tree of `root` after its slots changed: none if the first slot of the
    /// name is the one of the last computation, else the old first slot
    /// loses its nodes and the new one gets the matching children of the
    /// host.
    fn name_changes(&mut self, root: NodeId, name: &str, changes: &mut Changes) {
        let first = self.first_slot(root, name);
        let Some(host) = self.shadow_host(root) else {
            return;
        };
        let Some(slots) = self.shadow.as_mut().and_then(|s| s.slots.get_mut(&root)) else {
            return;
        };
        let seen = match first {
            Some(f) => slots.first.insert(name.to_owned(), f),
            None => slots.first.remove(name),
        };
        if seen == first {
            return;
        }
        if let Some(old) = seen
            && Some(old) != first
        {
            changes.push((old, Vec::new()));
        }
        if let Some(first) = first {
            let nodes = self.named_slottables(host, name);
            self.count_shadow_work(self.child_count(host));
            changes.push((first, nodes));
        }
    }

    /// The slots among `node` and its descendants (not in nested shadow
    /// trees or template contents), in tree order, with the root of `parent`,
    /// when `parent` is in a shadow tree: only then do they change a
    /// root's slots. A small subtree is walked at once. A large one is
    /// walked only after the root of `parent` is known to be a shadow root,
    /// so that building a large detached tree costs no walk per insertion.
    fn slots_below(&mut self, parent: NodeId, node: NodeId) -> Option<(NodeId, Vec<NodeId>)> {
        self.element(node)?;
        let mut slots = Vec::new();
        let mut visited = 0;
        let mut complete = true;
        for n in self.descendants(node) {
            visited += 1;
            if visited > SMALL_SUBTREE {
                complete = false;
                break;
            }
            if self.is_slot(n) {
                slots.push(n);
            }
        }
        self.count_shadow_work(visited);
        if complete && slots.is_empty() {
            return None;
        }
        let mut depth = 0;
        let root = self
            .ancestors(parent)
            .inspect(|_| depth += 1)
            .last()
            .unwrap_or(parent);
        self.count_shadow_work(depth);
        self.shadow_host(root)?;
        if !complete {
            let mut visited = 0;
            slots = self
                .descendants(node)
                .inspect(|_| visited += 1)
                .filter(|&n| self.is_slot(n))
                .collect();
            self.count_shadow_work(visited);
        }
        (!slots.is_empty()).then_some((root, slots))
    }

    /// Adds `slots` (in tree order, now in the shadow tree of `root`) to the
    /// root's slots and assigns (DOM "insert" step 7.6, "assign slottables
    /// for a tree").
    fn slots_added(&mut self, root: NodeId, slots: &[NodeId]) {
        let Some(data) = self.shadow_root_data(root).copied() else {
            return;
        };
        let mut names: Vec<String> = Vec::new();
        for &slot in slots {
            let name = self.slot_name_of(slot).to_owned();
            self.index_slot(root, slot, &name);
            if !names.contains(&name) {
                names.push(name);
            }
        }
        let mut changes = Vec::new();
        match data.slot_assignment {
            SlotAssignment::Named => {
                for name in &names {
                    self.name_changes(root, name, &mut changes);
                }
            }
            SlotAssignment::Manual => {
                for &slot in slots {
                    changes.push((slot, self.find_slottables(slot)));
                }
            }
        }
        self.apply_changes(changes);
    }

    /// Removes `slots` (in tree order, removed from the shadow tree of
    /// `root`) from the root's slots and assigns: the root's slots of the
    /// same names, then the removed slots, which have no assigned nodes
    /// outside the tree (DOM "remove" step 13).
    fn slots_removed(&mut self, root: NodeId, slots: &[NodeId]) {
        let Some(data) = self.shadow_root_data(root).copied() else {
            return;
        };
        let mut names: Vec<String> = Vec::new();
        for &slot in slots {
            let name = self.slot_name_of(slot).to_owned();
            self.unindex_slot(root, slot, &name);
            if !names.contains(&name) {
                names.push(name);
            }
        }
        let mut changes = Vec::new();
        if data.slot_assignment == SlotAssignment::Named {
            for name in &names {
                self.name_changes(root, name, &mut changes);
            }
        }
        for &slot in slots {
            if !changes.iter().any(|(s, _)| *s == slot) {
                changes.push((slot, Vec::new()));
            }
        }
        self.apply_changes(changes);
    }

    /// Puts `slot` into the list of its name in tree order. A slot that
    /// goes to either end (an append or a prepend to a root) costs a few
    /// steps; one in the middle costs a binary search whose comparisons
    /// scan siblings. The steps are counted as work.
    fn index_slot(&mut self, root: NodeId, slot: NodeId, name: &str) {
        let Some(state) = self.shadow.as_mut() else {
            return;
        };
        let list = state
            .slots
            .entry(root)
            .or_default()
            .by_name
            .entry(name.to_owned())
            .or_default();
        let mut list = std::mem::take(list);
        let mut steps = 1;
        let before = |a: NodeId, b: NodeId, limit: usize, steps: &mut usize| {
            self.precedes_within(a, b, limit, steps) == Some(true)
        };
        if list
            .back()
            .is_none_or(|&last| before(last, slot, NEAR_END, &mut steps))
        {
            list.push_back(slot);
        } else if list
            .front()
            .is_some_and(|&first| before(slot, first, NEAR_END, &mut steps))
        {
            list.push_front(slot);
        } else {
            let at = list.partition_point(|&s| before(s, slot, usize::MAX, &mut steps));
            steps += at.min(list.len() - at) / MOVES_PER_STEP;
            list.insert(at, slot);
        }
        self.count_shadow_work(steps);
        if let Some(state) = self.shadow.as_mut() {
            state
                .slots
                .entry(root)
                .or_default()
                .by_name
                .insert(name.to_owned(), list);
        }
    }

    /// Takes `slot` out of the list of its name.
    fn unindex_slot(&mut self, root: NodeId, slot: NodeId, name: &str) {
        let Some(slots) = self.shadow.as_mut().and_then(|s| s.slots.get_mut(&root)) else {
            return;
        };
        let Some(list) = slots.by_name.get_mut(name) else {
            return;
        };
        let (at, mut steps) = position_from_ends(list.len(), |i| list.get(i).copied(), slot);
        if let Some(at) = at {
            steps += at.min(list.len() - at) / MOVES_PER_STEP;
            list.remove(at);
        }
        if list.is_empty() {
            slots.by_name.remove(name);
        }
        self.count_shadow_work(steps);
    }

    /// Indexes the slots of the shadow tree of `root` and assigns: for a
    /// root that got its content without the change functions (a clone, a
    /// transplant).
    pub(crate) fn assign_new_root(&mut self, root: NodeId) {
        let slots: Vec<NodeId> = self
            .descendants(root)
            .filter(|&n| self.is_slot(n))
            .collect();
        if !slots.is_empty() {
            self.slots_added(root, &slots);
        }
    }

    /// DOM "insert" steps 7.4 to 7.6 after `node` was linked into `parent`:
    /// a slottable child of a host gets its slot, a slot without assigned
    /// nodes signals its new fallback content, and the inserted slots join
    /// their shadow tree.
    pub(crate) fn shadow_inserted(&mut self, parent: NodeId, node: NodeId) {
        if self
            .element(parent)
            .is_some_and(ElementData::is_shadow_host)
            && self.is_slottable(node)
        {
            self.slottable_inserted(parent, node);
        }
        self.fallback_changed(parent);
        if let Some((root, slots)) = self.slots_below(parent, node) {
            self.slots_added(root, &slots);
        }
    }

    /// DOM "remove" steps 10 to 13 after `node` left `parent`.
    pub(crate) fn shadow_removed(&mut self, parent: NodeId, node: NodeId) {
        if self
            .element(parent)
            .is_some_and(ElementData::is_shadow_host)
            && let Some(slot) = self.assigned_slot(node)
        {
            self.remove_assigned(slot, node);
        }
        self.fallback_changed(parent);
        if let Some((root, slots)) = self.slots_below(parent, node) {
            self.slots_removed(root, &slots);
        }
    }

    /// DOM "insert" step 7.5 and "remove" step 12: the children of a slot of
    /// a shadow tree without assigned nodes changed (its fallback content).
    fn fallback_changed(&mut self, parent: NodeId) {
        if self.is_slot(parent)
            && !self
                .element(parent)
                .is_some_and(ElementData::has_assigned_nodes)
            && self.in_shadow_tree(parent)
        {
            self.signal_slot(parent);
        }
    }

    /// A slottable was inserted into `host` (DOM "assign a slot"; also for a
    /// manual assignment: Chromium assigns a manually assigned node when it
    /// becomes a child of the host, measured).
    fn slottable_inserted(&mut self, host: NodeId, node: NodeId) {
        let Some(root) = self.shadow_root(host) else {
            return;
        };
        let Some(data) = self.shadow_root_data(root).copied() else {
            return;
        };
        let slot = match data.slot_assignment {
            SlotAssignment::Named => self.first_slot(root, self.slottable_name(node)),
            SlotAssignment::Manual => self.find_slot(node, false),
        };
        let Some(slot) = slot else {
            return;
        };
        if data.slot_assignment == SlotAssignment::Named && self.node(node).next_sibling.is_none() {
            // The common case, an append: the node comes last.
            self.append_assigned(slot, node);
            return;
        }
        self.count_shadow_work(self.child_count(host));
        let nodes = self.find_slottables(slot);
        self.set_assigned(slot, nodes);
    }

    /// DOM "attribute change steps" of slots and slottables: a change of the
    /// `name` of a slot in a shadow tree, or of the `slot` of a child of a
    /// host.
    pub(crate) fn shadow_attribute_changed(
        &mut self,
        element: NodeId,
        name: &QualName,
        old: Option<&str>,
    ) {
        if name.ns != ns!() {
            return;
        }
        let is_slot_attribute = name.local == local_name!("slot");
        let is_name = name.local == local_name!("name");
        if !is_slot_attribute && !is_name {
            return;
        }
        let new = self.element(element).and_then(|e| e.attr(&name.local));
        if old.unwrap_or("") == new.unwrap_or("") {
            return;
        }
        let old = old.unwrap_or("").to_owned();
        let new = new.unwrap_or("").to_owned();
        if is_slot_attribute {
            self.slottable_renamed(element, &old, &new);
        }
        if is_name && self.is_slot(element) {
            self.slot_renamed(element, &old, &new);
        }
    }

    /// The `slot` attribute of `element` changed from `old` to `new`.
    fn slottable_renamed(&mut self, element: NodeId, old: &str, new: &str) {
        let Some(host) = self
            .parent(element)
            .filter(|&p| self.element(p).is_some_and(ElementData::is_shadow_host))
        else {
            return;
        };
        let Some(root) = self.shadow_root(host) else {
            return;
        };
        if self
            .shadow_root_data(root)
            .is_none_or(|d| d.slot_assignment != SlotAssignment::Named)
        {
            return;
        }
        // The old assigned slot first, then the new one.
        for name in [old, new] {
            if let Some(slot) = self.first_slot(root, name) {
                let nodes = self.named_slottables(host, name);
                self.count_shadow_work(self.child_count(host));
                self.set_assigned(slot, nodes);
            }
        }
    }

    /// The `name` of the slot `slot` changed from `old` to `new`.
    fn slot_renamed(&mut self, slot: NodeId, old: &str, new: &str) {
        let root = self.root(slot);
        let Some(data) = self.shadow_root_data(root).copied() else {
            return;
        };
        self.unindex_slot(root, slot, old);
        self.index_slot(root, slot, new);
        if data.slot_assignment == SlotAssignment::Named {
            let mut changes = Vec::new();
            self.name_changes(root, old, &mut changes);
            self.name_changes(root, new, &mut changes);
            self.apply_changes(changes);
        }
    }

    /// Drops what the shadow state holds of `nodes`, which leave the arena (a
    /// transplant): their shadow roots, slots, assigned and manual lists,
    /// and their signals. The cost depends on `nodes` and the lists that
    /// they are in, not on the size of the state.
    pub(crate) fn forget_shadow_nodes(&mut self, nodes: &[NodeId]) {
        let Some(state) = self.shadow.as_mut() else {
            return;
        };
        // Manual lists of slots that stay, and the nodes that leave them.
        let mut manual_left: HashMap<NodeId, HashSet<NodeId>> = HashMap::new();
        let mut signalled = false;
        let mut work = 0;
        for &node in nodes {
            state.roots.remove(&node);
            state.slots.remove(&node);
            if let Some(list) = state.assigned.remove(&node) {
                work += list.len();
                for assigned in list.as_slice() {
                    if state.assigned_slot.get(assigned) == Some(&node) {
                        state.assigned_slot.remove(assigned);
                    }
                }
            }
            state.assigned_slot.remove(&node);
            if let Some(list) = state.manual.remove(&node) {
                work += list.len();
                for assigned in list {
                    if state.manual_slot.get(&assigned) == Some(&node) {
                        state.manual_slot.remove(&assigned);
                    }
                }
            }
            if let Some(slot) = state.manual_slot.remove(&node) {
                manual_left.entry(slot).or_default().insert(node);
            }
            signalled |= state.signal_set.remove(&node);
        }
        for (slot, gone) in manual_left {
            if let Some(list) = state.manual.get_mut(&slot) {
                work += list.len();
                list.retain(|n| !gone.contains(n));
            }
        }
        if signalled {
            let still = &state.signal_set;
            state.signal_slots.retain(|s| still.contains(s));
            work += state.signal_slots.len();
        }
        self.count_shadow_work(work + nodes.len());
    }
}

#[cfg(test)]
mod tests;
