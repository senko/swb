//! The selection of a document (ADR 0039 section 3; Selection API,
//! <https://w3c.github.io/selection-api/>): its range is a live range entry
//! of the arena, so the live range steps keep it where the DOM changes, and
//! the engine (the user's gestures, the highlight, copy) and the bindings
//! (`Selection`) read and write one state.
//!
//! - **Entries.** A script's `Range` object owns the entry of its id (ids
//!   below [`HOST_ID_LIMIT`], handed out by the host). The selection owns the
//!   entry it names; an entry that the arena makes for the selection (a user
//!   gesture, a `Selection` method) has an id from the upper half and no
//!   object until the host wraps it ([`Document::wrap_selection_entry`]). An
//!   entry goes when neither owns it.
//! - **Changes.** Every change of the state or of the selection's range bumps
//!   the selection generation (the engine rebuilds its display list when it
//!   differs). A change that the Selection API reports with `selectionchange`
//!   also sets the "changed" flag and asks the host to look (the request flag
//!   of [`ArenaWork`](crate::ArenaWork)); the live range steps of insertions
//!   and removals do not (measured in Chromium 148).
//! - **Text controls.** While a text control owns the document's selection,
//!   the range is collapsed at the control's place in its parent and the state
//!   names the control; the host reports the control's selection.

use super::{Boundary, LiveRange, LiveRanges};
use crate::tree::{Document, NodeId};

/// The ids below this one are the host's (its `Range` objects); the arena
/// makes the ids of the selection's own entries from here up.
pub const HOST_ID_LIMIT: u32 = 1 << 31;

/// The selection of a document as the engine and the bindings read it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionState {
    /// The id of the range entry.
    pub id: u32,
    /// The range.
    pub range: LiveRange,
    /// True if the focus is before the anchor (the anchor is the end).
    pub backward: bool,
    /// The text control that owns the selection, if any.
    pub control: Option<NodeId>,
}

impl SelectionState {
    /// The anchor: where the selection started.
    pub fn anchor(&self) -> Boundary {
        if self.backward {
            self.range.end
        } else {
            self.range.start
        }
    }

    /// The focus: where the selection ends.
    pub fn focus(&self) -> Boundary {
        if self.backward {
            self.range.start
        } else {
            self.range.end
        }
    }
}

/// The selection's part of the registry.
#[derive(Debug, Default)]
pub(super) struct SelectionSlot {
    current: Option<Current>,
    /// Bumped at every change of the selection or of its range.
    generation: u64,
    /// The Selection API's changes since the host last looked.
    changed: bool,
    /// The next id of an entry of the arena's own.
    next_own: u32,
    /// Bumped when the selection is replaced or ends, not when a script
    /// object wraps its entry (which renames it): the engine's drag tells a
    /// script's replacement from a `getRangeAt` with it.
    replacements: u32,
}

/// The selection's entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Current {
    id: u32,
    backward: bool,
    control: Option<NodeId>,
    /// True while a script object owns the entry too.
    object: bool,
}

impl LiveRanges {
    /// The id of the selection's entry.
    pub(super) fn selection_id(&self) -> Option<u32> {
        self.selection.current.map(|c| c.id)
    }

    /// The selection's range.
    pub(super) fn selection_range(&self) -> Option<LiveRange> {
        let id = self.selection_id()?;
        self.entries.get(&id).copied()
    }

    /// Bumps the generation; with `report`, sets the flag of
    /// `selectionchange` and asks the host to look.
    pub(super) fn note_selection_change(&mut self, report: bool) {
        let slot = &mut self.selection;
        slot.generation = slot.generation.wrapping_add(1);
        if report {
            slot.changed = true;
            if let Some(work) = &self.work {
                work.ask();
            }
        }
    }

    /// The script object of entry `id` is gone: the entry goes unless it is
    /// the selection's.
    pub(super) fn release(&mut self, id: u32) {
        match &mut self.selection.current {
            Some(current) if current.id == id => current.object = false,
            _ => {
                self.forget(id);
            }
        }
    }

    /// Ends the selection (no range); its entry goes unless an object owns
    /// it.
    fn drop_selection(&mut self) {
        self.selection.replacements = self.selection.replacements.wrapping_add(1);
        if let Some(old) = self.selection.current.take()
            && !old.object
        {
            self.forget(old.id);
        }
    }

    /// If entry `id` is the selection's range, the selection has no range any
    /// more (an object keeps its entry).
    pub(super) fn unselect(&mut self, id: u32) {
        if self.selection.current.is_some_and(|c| c.id == id) {
            self.drop_selection();
            self.note_selection_change(true);
        }
    }

    /// A new id for an entry of the arena's own.
    fn own_id(&mut self) -> u32 {
        loop {
            let id = HOST_ID_LIMIT | (self.selection.next_own & (HOST_ID_LIMIT - 1));
            self.selection.next_own = self.selection.next_own.wrapping_add(1);
            if !self.entries.contains_key(&id) {
                return id;
            }
        }
    }
}

impl Document {
    /// The selection, if it has a range.
    pub fn selection(&self) -> Option<SelectionState> {
        let ranges = self.ranges.as_deref()?;
        let current = ranges.selection.current?;
        let range = *ranges.entries.get(&current.id)?;
        Some(SelectionState {
            id: current.id,
            range,
            backward: current.backward,
            control: current.control,
        })
    }

    /// A count of the replacements of the selection (a new range or none),
    /// which `getRangeAt` does not change (0 without a registry).
    pub fn selection_replacements(&self) -> u32 {
        self.ranges
            .as_deref()
            .map_or(0, |r| r.selection.replacements)
    }

    /// The selection generation: it changes with every change of the
    /// selection or of its range (0 without a registry).
    pub fn selection_generation(&self) -> u64 {
        self.ranges.as_deref().map_or(0, |r| r.selection.generation)
    }

    /// True if the selection changed in a way that the Selection API reports
    /// (`selectionchange`) since the last call.
    pub fn take_selection_changed(&mut self) -> bool {
        self.ranges
            .as_deref_mut()
            .is_some_and(|r| std::mem::take(&mut r.selection.changed))
    }

    /// Replaces the selection with `range` (`None`: no range) in a new entry
    /// of the arena (a user gesture or a `Selection` method replaces the
    /// range object; measured). `backward` puts the anchor at the end. A
    /// selection with the same range and direction and no control stays.
    /// Returns true if the selection changed; with `report`, the change sets
    /// the flag of `selectionchange`.
    pub fn set_selection(
        &mut self,
        range: Option<LiveRange>,
        backward: bool,
        report: bool,
    ) -> bool {
        let backward = backward && range.is_some_and(|r| !r.is_collapsed());
        let current = self.selection();
        let same = match (current, range) {
            (None, None) => true,
            (Some(c), Some(r)) => c.range == r && c.backward == backward && c.control.is_none(),
            _ => false,
        };
        if same {
            return false;
        }
        if range.is_none() && self.ranges.is_none() {
            return false;
        }
        let ranges = self.ranges.get_or_insert_with(Box::default);
        ranges.drop_selection();
        if let Some(range) = range {
            let id = ranges.own_id();
            ranges.place(id, range);
            ranges.selection.current = Some(Current {
                id,
                backward,
                control: None,
                object: false,
            });
        }
        ranges.note_selection_change(report);
        true
    }

    /// Makes the existing entry `id` (owned by a script object) the
    /// selection's range (`Selection.addRange`), forward. Returns true if
    /// the selection changed.
    pub fn set_selection_entry(&mut self, id: u32) -> bool {
        let Some(ranges) = self.ranges.as_deref_mut() else {
            return false;
        };
        if !ranges.entries.contains_key(&id) {
            return false;
        }
        if ranges.selection.current.is_some_and(|c| c.id == id) {
            return false;
        }
        ranges.drop_selection();
        ranges.selection.current = Some(Current {
            id,
            backward: false,
            control: None,
            object: true,
        });
        ranges.note_selection_change(true);
        true
    }

    /// The text control `control` owns the selection: the range is collapsed
    /// at its place in its parent (the host reports the type, the direction
    /// and the text of the control's selection). Not a change that
    /// `selectionchange` at the document reports (the control's events do).
    /// Returns true if the state changed.
    pub fn set_selection_control(&mut self, control: NodeId) -> bool {
        let Some(parent) = self.parent(control) else {
            return false;
        };
        let place = LiveRange::collapsed_at(Boundary::new(parent, self.index_in_parent(control)));
        if self
            .selection()
            .is_some_and(|c| c.range == place && c.control == Some(control))
        {
            return false;
        }
        let ranges = self.ranges.get_or_insert_with(Box::default);
        ranges.drop_selection();
        let id = ranges.own_id();
        ranges.place(id, place);
        ranges.selection.current = Some(Current {
            id,
            backward: false,
            control: Some(control),
            object: false,
        });
        ranges.note_selection_change(false);
        true
    }

    /// The selection's entry gets the id `id` of a new script object of the
    /// host, if no object owns it yet; returns the id that the object must
    /// hold (the old one if an object owns it). `None` without a selection.
    pub fn wrap_selection_entry(&mut self, id: u32) -> Option<u32> {
        let ranges = self.ranges.as_deref_mut()?;
        let current = ranges.selection.current?;
        if current.object {
            return Some(current.id);
        }
        let range = *ranges.entries.get(&current.id)?;
        ranges.forget(current.id);
        ranges.place(id, range);
        ranges.selection.current = Some(Current {
            id,
            object: true,
            ..current
        });
        Some(id)
    }

    /// True if a script object owns the selection's entry.
    pub fn selection_entry_has_object(&self) -> bool {
        self.ranges
            .as_deref()
            .and_then(|r| r.selection.current)
            .is_some_and(|c| c.object)
    }

    /// The script object of live range `id` is gone: its entry goes, unless
    /// it is the selection's range.
    pub fn release_live_range(&mut self, id: u32) {
        if let Some(ranges) = &mut self.ranges {
            ranges.release(id);
        }
    }

    /// The selection's range left the document's tree (a script set it to
    /// nodes of another root): the selection has no range any more.
    pub(super) fn selection_left_tree(&mut self, id: u32) {
        if let Some(ranges) = self.ranges.as_deref_mut() {
            ranges.unselect(id);
        }
    }

    /// True if `range` is in the document's tree (its shadow-including root
    /// is the document), where a selection can be.
    pub fn is_in_document_tree(&self, range: LiveRange) -> bool {
        self.shadow_including_root(range.start.node) == NodeId::DOCUMENT
    }
}
