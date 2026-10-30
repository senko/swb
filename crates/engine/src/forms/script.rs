//! What scripts write to form controls (M8 feature 6c): the values that a
//! control gets before it has a state in [`Forms`], and the merge of
//! those values into a rebuilt [`Forms`].
//!
//! The state of a control lives in [`Forms`], which holds only the
//! controls in the document. A script can write the `value`, the
//! checkedness or the selectedness of a control that is not in the
//! document (a new `input`, `new Option()`), or of one whose state is not
//! rebuilt yet. Those writes are [`Seeds`]. A rebuild applies the seeds
//! to the controls it finds and keeps the dirty state of controls that
//! left the document, so that a control that a script removes and inserts
//! again keeps what it holds (the dirty value flag and the dirty
//! checkedness flag of HTML are properties of the element).

use std::collections::HashMap;

use swb_dom::{Document, NodeId};

use super::select::{detached_options, settle};
use super::{ControlState, ControlType, Forms, InputType, in_document};

/// What a script wrote to one control.
#[derive(Clone, Debug, Default)]
struct Seed {
    /// The value of a text field or text area (sanitized).
    value: Option<String>,
    /// The checkedness of an `input` and its stamp. A text field keeps it
    /// until its `type` becomes a checkbox or radio button.
    checked: Option<(bool, u64)>,
    /// The form owner of the input when the script checked it: a radio
    /// button that has another owner when the seed is applied was moved
    /// after the write, which makes it the newest of its new group.
    checked_owner: Option<NodeId>,
    /// The selection of a text field that is not in the document (UTF-16
    /// offsets, backward).
    selection: Option<(usize, usize, bool)>,
    /// The selectedness of an option.
    selected: Option<bool>,
    /// True if the dirtiness flag of the option is set. Without it (a
    /// `new Option()`, an option that the selectedness setting algorithm
    /// deselected) a change of the `selected` attribute drops `selected`.
    selected_dirty: bool,
}

impl Seed {
    fn is_empty(&self) -> bool {
        self.value.is_none()
            && self.checked.is_none()
            && self.selected.is_none()
            && self.selection.is_none()
    }
}

/// The values that scripts wrote to controls that have no state yet.
#[derive(Debug, Default)]
pub(crate) struct Seeds {
    map: HashMap<NodeId, Seed>,
    /// The bytes of the values in `map`.
    value_bytes: usize,
}

impl Seeds {
    /// The value that a script wrote to `node`.
    pub(crate) fn value(&self, node: NodeId) -> Option<&str> {
        self.map.get(&node)?.value.as_deref()
    }

    /// Forgets what scripts wrote to `node` (the form reset algorithm).
    pub(crate) fn forget(&mut self, node: NodeId) {
        self.remove(node);
    }

    /// The checkedness that a script wrote to `node`.
    pub(crate) fn checked(&self, node: NodeId) -> Option<bool> {
        self.map.get(&node)?.checked.map(|(checked, _)| checked)
    }

    /// The selectedness that a script wrote to the option `node`.
    pub(crate) fn selected(&self, node: NodeId) -> Option<bool> {
        self.map.get(&node)?.selected
    }

    /// The selectedness and the dirtiness flag of the option `node`, if it
    /// has a seed.
    pub(crate) fn option(&self, node: NodeId) -> Option<(bool, bool)> {
        let seed = self.map.get(&node)?;
        Some((seed.selected?, seed.selected_dirty))
    }

    /// The bytes of the values in the seeds (for the budget of control
    /// values).
    pub(crate) fn value_bytes(&self) -> usize {
        self.value_bytes
    }

    /// The selection that a script set on `node`.
    pub(crate) fn selection(&self, node: NodeId) -> Option<(usize, usize, bool)> {
        self.map.get(&node)?.selection
    }

    pub(crate) fn set_selection(&mut self, node: NodeId, selection: (usize, usize, bool)) {
        self.map.entry(node).or_default().selection = Some(selection);
    }

    /// Takes the value that a script wrote to `node`.
    pub(crate) fn take_value(&mut self, node: NodeId) -> Option<String> {
        let value = self.map.get_mut(&node)?.value.take()?;
        self.value_bytes = self.value_bytes.saturating_sub(value.len());
        if self.map.get(&node).is_some_and(Seed::is_empty) {
            self.map.remove(&node);
        }
        Some(value)
    }

    pub(crate) fn set_value(&mut self, node: NodeId, value: String) {
        let added = value.len();
        let old = self.map.entry(node).or_default().value.replace(value);
        self.value_bytes = self.value_bytes.saturating_sub(old.map_or(0, |v| v.len())) + added;
    }

    /// Drops the seed of `node`.
    fn remove(&mut self, node: NodeId) {
        if let Some(seed) = self.map.remove(&node) {
            self.value_bytes = self
                .value_bytes
                .saturating_sub(seed.value.map_or(0, |v| v.len()));
        }
    }

    /// Drops the parts of the seed of `node` that a state took: the value
    /// if `value`, the checkedness if `checked`.
    fn consume(&mut self, node: NodeId, value: bool, checked: bool) {
        let Some(seed) = self.map.get_mut(&node) else {
            return;
        };
        if value {
            seed.selection = None;
            if let Some(old) = seed.value.take() {
                self.value_bytes = self.value_bytes.saturating_sub(old.len());
            }
        }
        if checked {
            seed.checked = None;
        }
        if seed.is_empty() {
            self.map.remove(&node);
        }
    }

    pub(crate) fn set_checked(
        &mut self,
        node: NodeId,
        checked: bool,
        stamp: u64,
        owner: Option<NodeId>,
    ) {
        let seed = self.map.entry(node).or_default();
        seed.checked = Some((checked, stamp));
        seed.checked_owner = owner;
    }

    /// Sets the selectedness of the option `node` and its dirtiness flag.
    pub(crate) fn set_option(&mut self, node: NodeId, selected: bool, dirty: bool) {
        let seed = self.map.entry(node).or_default();
        seed.selected = Some(selected);
        seed.selected_dirty = dirty;
    }

    /// Sets the selectedness of the option `node` and sets its dirtiness
    /// flag (`option.selected = x`).
    pub(crate) fn set_selected(&mut self, node: NodeId, selected: bool) {
        self.set_option(node, selected, true);
    }

    /// Sets the selectedness of the option `node` without making it
    /// dirty (HTML "selectedness" of `new Option()`).
    pub(crate) fn set_selected_clean(&mut self, node: NodeId, selected: bool) {
        self.set_option(node, selected, false);
    }

    /// Drops the selectedness of the option `node` if it is not dirty:
    /// the `selected` attribute was added or removed, and the
    /// selectedness follows the attribute again. True if it dropped one.
    pub(crate) fn drop_clean_selected(&mut self, node: NodeId) -> bool {
        let Some(seed) = self.map.get_mut(&node) else {
            return false;
        };
        let dropped = seed.selected.is_some() && !seed.selected_dirty;
        if dropped {
            seed.selected = None;
        }
        if seed.is_empty() {
            self.map.remove(&node);
        }
        dropped
    }
}

impl Forms {
    /// The merge step of a rebuild: `self` is the new state, `old` the
    /// state before it. Dirty controls of `old` that are no longer in
    /// the document become seeds, then the seeds are applied to the
    /// controls of `self`.
    pub(crate) fn merge_seeds(&mut self, old: &Forms, doc: &Document, seeds: &mut Seeds) {
        self.orphans_to_seeds(old, doc, seeds);
        self.apply_seeds(doc, seeds);
    }

    /// Keeps the dirty state of the controls of `old` that `self` does not
    /// have (they left the document), the dirty checkedness of an input
    /// whose type cannot show it any more, and the selectedness of the
    /// options that left a select: an option keeps its selectedness and
    /// its dirtiness flag when it is not in a select.
    fn orphans_to_seeds(&self, old: &Forms, doc: &Document, seeds: &mut Seeds) {
        for (&node, state) in &old.controls {
            let now = self.controls.get(&node);
            if state.ty == ControlType::Select {
                for option in &state.options {
                    // Only a selectedness that the attribute does not give
                    // is worth keeping.
                    let by_attribute = doc
                        .element(option.node)
                        .is_some_and(|e| e.has_attr("selected"));
                    if !self.option_select.contains_key(&option.node)
                        && doc.get(option.node).is_some()
                        && (option.dirty || option.selected != by_attribute)
                    {
                        seeds.set_option(option.node, option.selected, option.dirty);
                    }
                }
            }
            if doc.get(node).is_none() {
                continue;
            }
            match now {
                // The type changed: a checkbox or radio button keeps its
                // dirty checkedness for a type that cannot show it; it is
                // "inserted" into its group when the type comes back.
                Some(now) if now.ty != state.ty => {
                    if state.dirty && state.ty.is_checkable() && !now.ty.is_checkable() {
                        seeds.set_checked(node, state.checked, 0, state.owner);
                    }
                }
                None if state.dirty && state.ty.is_text() => {
                    seeds.set_value(node, state.edit.text().to_owned());
                    let (start, end) = state.edit.selection_utf16();
                    seeds.set_selection(node, (start, end, state.edit.is_backward()));
                }
                None if state.dirty && state.ty.is_checkable() => {
                    seeds.set_checked(node, state.checked, 0, state.owner);
                }
                // Still in the document with its type, or nothing to keep.
                Some(_) | None => {}
            }
        }
    }

    /// Applies the seeds of the controls and options that `self` has, and
    /// drops the seeds that it applied.
    fn apply_seeds(&mut self, doc: &Document, seeds: &mut Seeds) {
        if seeds.map.is_empty() {
            return;
        }
        let mut radios = Vec::new();
        let mut selects = Vec::new();
        let mut consumed = Vec::new();
        let mut applied = Vec::new();
        // Walk the controls and options of the new state, not the seeds:
        // the seeds of options that left their select stay for a later
        // insertion and can be many.
        let nodes: Vec<NodeId> = self.controls.keys().copied().collect();
        for node in nodes {
            let Some(seed) = seeds.map.get(&node) else {
                continue;
            };
            let Some(state) = self.controls.get_mut(&node) else {
                continue;
            };
            // A value is for a text field; for another type it is stale.
            if let (Some(value), true) = (&seed.value, state.ty.is_text()) {
                state.edit.replace_text(value);
                state.dirty = true;
            }
            if let (Some((start, end, backward)), true) = (seed.selection, state.ty.is_text()) {
                state.edit.select_range_utf16(start, end, backward);
            }
            let checkable = state.ty.is_checkable();
            if let (Some((checked, stamp)), true) = (seed.checked, checkable) {
                state.checked = checked;
                state.dirty = true;
                if state.ty == ControlType::Input(InputType::Radio) {
                    // Another owner than at the write: the button moved, so
                    // it is the newest of its group.
                    let moved = seed.checked_owner != state.owner;
                    radios.push((if moved { 0 } else { stamp }, node));
                }
            }
            consumed.push((node, true, checkable));
        }
        for (&option, &select) in &self.option_select {
            if seeds.selected(option).is_some() {
                selects.push(select);
                applied.push(option);
            }
        }
        selects.sort_unstable();
        selects.dedup();
        for select in selects {
            self.apply_option_seeds(doc, select, seeds);
        }
        for node in applied {
            seeds.remove(node);
        }
        for (node, value, checked) in consumed {
            seeds.consume(node, value, checked);
        }
        if !radios.is_empty() {
            self.stamp_seeded_radios(doc, radios);
            let checked: Vec<NodeId> = doc
                .descendants(NodeId::DOCUMENT)
                .filter(|&n| {
                    self.get(n)
                        .is_some_and(|s| s.ty == ControlType::Input(InputType::Radio) && s.checked)
                })
                .collect();
            self.keep_last_checked(doc, &checked);
        }
    }

    /// Gives the radio buttons that scripts checked since the last rebuild
    /// their place among the checked buttons of their groups: a radio
    /// button that meets its group (it is inserted) is the newest of it.
    /// The buttons that were in the document when a script checked them
    /// (stamp above 0) come first, in the order of the writes; the buttons
    /// that were outside it (stamp 0) come after them, in tree order, as
    /// when a script inserts them one after the other.
    fn stamp_seeded_radios(&mut self, doc: &Document, mut radios: Vec<(u64, NodeId)>) {
        let mut rank: HashMap<NodeId, usize> = HashMap::new();
        if radios.iter().any(|&(stamp, _)| stamp == 0) {
            rank = doc
                .descendants(NodeId::DOCUMENT)
                .enumerate()
                .map(|(i, n)| (n, i))
                .collect();
        }
        radios.sort_unstable_by_key(|&(stamp, node)| {
            (stamp == 0, stamp, rank.get(&node).copied().unwrap_or(0))
        });
        for (_, node) in radios {
            let stamp = self.next_stamp();
            if let Some(state) = self.controls.get_mut(&node) {
                state.checked_at = stamp;
            }
        }
    }

    /// Applies the selectedness that scripts wrote to the options of a
    /// select, then the selectedness setting algorithm.
    fn apply_option_seeds(&mut self, doc: &Document, select: NodeId, seeds: &Seeds) {
        let Some(state) = self.controls.get_mut(&select) else {
            return;
        };
        let mut last_selected = None;
        for (i, option) in state.options.iter_mut().enumerate() {
            if let Some((selected, dirty)) = seeds.option(option.node) {
                option.selected = selected;
                option.dirty = dirty;
                if selected {
                    last_selected = Some(i);
                }
            }
        }
        let multiple = doc.element(select).is_some_and(|e| e.has_attr("multiple"));
        if !multiple && let Some(keep) = last_selected {
            for (i, option) in state.options.iter_mut().enumerate() {
                option.selected = i == keep;
            }
        }
        state.dirty = true;
        let disabled = |n: NodeId| option_is_disabled(doc, n);
        settle(doc, select, &mut state.options, &disabled);
    }

    /// True if `node` has a state: it was in the document at the last
    /// rebuild.
    pub(crate) fn has_state(&self, node: NodeId) -> bool {
        self.controls.contains_key(&node)
    }

    /// True if `option` was in a select at the last rebuild.
    pub(crate) fn has_option(&self, option: NodeId) -> bool {
        self.option_select.contains_key(&option)
    }

    /// The bytes of the values of the text fields and text areas that
    /// scripts or the user changed.
    pub(crate) fn dirty_value_bytes(&self) -> usize {
        self.controls
            .values()
            .filter(|s| s.dirty && s.ty.is_text())
            .map(|s| s.edit.text().len())
            .sum()
    }

    /// The control type of `node` in the state, if it has one.
    pub(crate) fn typed_state(&self, node: NodeId, ty: ControlType) -> Option<&ControlState> {
        self.get(node).filter(|s| s.ty == ty)
    }

    /// The same, for changes.
    pub(crate) fn typed_state_mut(
        &mut self,
        node: NodeId,
        ty: ControlType,
    ) -> Option<&mut ControlState> {
        self.get_mut(node).filter(|s| s.ty == ty)
    }
}

/// True if the option or its `optgroup` parent has a `disabled`
/// attribute.
pub(crate) fn option_is_disabled(doc: &Document, option: NodeId) -> bool {
    doc.element(option).is_some_and(|e| e.has_attr("disabled"))
        || doc
            .parent(option)
            .and_then(|p| doc.element(p))
            .is_some_and(|p| &**p.local_name() == "optgroup" && p.has_attr("disabled"))
}

/// The select that an option belongs to: its parent, or the parent of its
/// `optgroup` parent.
pub(crate) fn select_of_option(doc: &Document, option: NodeId) -> Option<NodeId> {
    let is = |n: NodeId, name: &str| {
        doc.element(n)
            .is_some_and(|e| e.is_html() && &**e.local_name() == name)
    };
    if !is(option, "option") {
        return None;
    }
    let parent = doc.parent(option)?;
    if is(parent, "select") {
        return Some(parent);
    }
    let grand = doc.parent(parent)?;
    (is(parent, "optgroup") && is(grand, "select")).then_some(grand)
}

/// True if the select is in the document (it has a state in [`Forms`]).
pub(crate) fn select_is_attached(doc: &Document, select: NodeId) -> bool {
    in_document(doc, select)
}

/// The options of a select that is not in the document, with the
/// selectedness that scripts wrote.
pub(crate) fn options_of_detached(
    doc: &Document,
    select: NodeId,
    seeds: &Seeds,
) -> Vec<super::OptionState> {
    detached_options(doc, select, &|n| seeds.option(n))
}
