//! The top layer and popovers (HTML §6.12 "The popover attribute",
//! <https://html.spec.whatwg.org/multipage/popover.html>; CSS Position 4
//! §"Top layer"; ADR 0037 sections 1 and 2).
//!
//! The arena keeps a [`TopLayerState`] once the first element is shown as a
//! popover: the top layer in order, the showing auto and hint popover lists
//! and the state of each popover that is showing or has a toggle task. The
//! flag byte of an element tells whether it is a showing popover and whether
//! it is in the top layer, so that style and the box tree need no lookup.
//!
//! The algorithms fire `beforetoggle` in their middle, and a listener can
//! change the DOM; the caller's [`TopLayerHost`] fires the events and queues
//! the `toggle` tasks, and lends the document for short borrows only. The
//! web layer's host runs scripts; the engine's host (documents without
//! scripts) fires nothing. Errors carry Chromium's messages
//! ([`PopoverError`]).
//!
//! The rules follow Chromium 148 where it differs from the HTML Standard of
//! 2026-11-12 (measured with `tools/probes/web-top-layer.json`; ADR 0037
//! "6a implementation notes"):
//!
//! - A `showPopover` from inside a `beforetoggle` listener runs (the
//!   standard's "showing popover" flag of the document would throw): a nested
//!   show of the same element opens it once, and a show of another popover
//!   is hidden again by the outer show.
//! - Hint popovers: a hint with an open auto popover as its ancestor joins
//!   the auto list above it (it stays open when an unrelated hint opens, and
//!   it closes with its ancestor); another hint goes to the hint list. Hiding
//!   or showing an auto popover hides all hints of the hint list first.
//! - Every error of a hide carries the sentence about `beforetoggle`.

use std::collections::HashMap;
use std::fmt;

use html5ever::local_name;

use crate::journal::Change;
use crate::tree::{Document, NodeId, flags};

mod dialog;
mod dismiss;
mod popover;
mod watchers;

pub use dialog::{
    ClosedBy, close_dialog, dialog_form_target, request_close_dialog, show_dialog, show_modal,
};
pub use dismiss::{
    light_dismiss_dialogs_down, light_dismiss_dialogs_up, light_dismiss_down, light_dismiss_up,
    topmost_clicked_popover,
};
pub use popover::*;
pub use watchers::{WatcherKind, process_close_watchers};

/// The state of the `popover` attribute (an enumerated attribute: the empty
/// value is `auto`, an invalid value `manual`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopoverState {
    /// `auto`.
    Auto,
    /// `manual`.
    Manual,
    /// `hint`.
    Hint,
}

impl PopoverState {
    /// The state of the attribute value `value` (`None`: no attribute, the
    /// "no popover" state). ASCII case-insensitive.
    pub fn of(value: Option<&str>) -> Option<PopoverState> {
        let value = value?;
        Some(if value.is_empty() || value.eq_ignore_ascii_case("auto") {
            PopoverState::Auto
        } else if value.eq_ignore_ascii_case("hint") {
            PopoverState::Hint
        } else {
            PopoverState::Manual
        })
    }
}

/// The states of `ToggleEvent`'s `oldState` and `newState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToggleState {
    /// `"closed"`.
    Closed,
    /// `"open"`.
    Open,
}

impl ToggleState {
    /// The string of the event attribute.
    pub fn as_str(self) -> &'static str {
        match self {
            ToggleState::Closed => "closed",
            ToggleState::Open => "open",
        }
    }
}

/// The state of `popovertargetaction` (missing and invalid: toggle).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetAction {
    /// `toggle`.
    Toggle,
    /// `show`.
    Show,
    /// `hide`.
    Hide,
}

impl TargetAction {
    /// The state of the attribute value `value`. ASCII case-insensitive.
    pub fn of(value: Option<&str>) -> TargetAction {
        match value {
            Some(v) if v.eq_ignore_ascii_case("show") => TargetAction::Show,
            Some(v) if v.eq_ignore_ascii_case("hide") => TargetAction::Hide,
            _ => TargetAction::Toggle,
        }
    }
}

/// A `toggle` task that waits (HTML "toggle task tracker"): the old state is
/// the one of the first change since the last task (coalescing).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingToggle {
    /// `oldState`.
    pub old: ToggleState,
    /// `newState`.
    pub new: ToggleState,
    /// `source`.
    pub source: Option<NodeId>,
    serial: u64,
}

/// Why a popover algorithm failed. [`PopoverError::name`] and
/// [`PopoverError::message`] are Chromium's (measured, ADR 0037 table).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopoverErrorKind {
    /// The element is not a popover (`NotSupportedError`).
    NotPopover,
    /// The element is not connected (`InvalidStateError`).
    Disconnected,
    /// The element's document is not fully active: a document without a
    /// browsing context, template contents (`InvalidStateError`).
    NotFullyActive,
    /// The element moved to another document during the algorithm.
    OtherDocument,
    /// The element is an open modal dialog.
    ModalDialog,
    /// The `popover` attribute changed while other popovers were hidden.
    TypeChanged,
    /// The host stopped (a script was terminated): the algorithm ended.
    Stopped,
    /// `showModal()` of a dialog that is open as a non-modal dialog.
    DialogOpenNonModal,
    /// `show()` of a dialog that is open as a modal dialog.
    DialogOpenModal,
    /// `showModal()` of a dialog that is a showing popover.
    DialogOpenAsPopover,
    /// `showModal()` of a dialog that is not connected.
    DialogNotInDocument,
    /// `showModal()` of a dialog whose document is not fully active.
    DialogNotFullyActive,
}

/// The error of a popover algorithm: its kind, and whether the message gets
/// Chromium's sentence about the `beforetoggle` event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PopoverError {
    /// The kind.
    pub kind: PopoverErrorKind,
    /// True after an event (and for every error of a hide, as in Chromium).
    pub after_event: bool,
}

impl PopoverError {
    /// The `DOMException` name.
    pub fn name(&self) -> &'static str {
        match self.kind {
            PopoverErrorKind::NotPopover => "NotSupportedError",
            _ => "InvalidStateError",
        }
    }

    /// The message (without the `Failed to execute ...` prefix).
    pub fn message(&self) -> String {
        let text = match self.kind {
            PopoverErrorKind::NotPopover => "Not supported on elements that are not popovers.",
            PopoverErrorKind::Disconnected => "Invalid on disconnected popover elements.",
            PopoverErrorKind::NotFullyActive => {
                "Invalid for popovers within documents that are not fully active."
            }
            // Not measured: swb's own words.
            PopoverErrorKind::OtherDocument => "The popover element moved to another document.",
            PopoverErrorKind::ModalDialog => "Invalid on open modal dialogs.",
            PopoverErrorKind::TypeChanged => {
                "The popover type changed while other popovers were hidden."
            }
            PopoverErrorKind::Stopped => "The script was stopped.",
            PopoverErrorKind::DialogOpenNonModal => {
                "The dialog is already open as a non-modal dialog, and therefore cannot be opened \
                 as a modal dialog."
            }
            PopoverErrorKind::DialogOpenModal => {
                "The dialog is already open as a modal dialog, and therefore cannot be opened as a \
                 non-modal dialog."
            }
            PopoverErrorKind::DialogOpenAsPopover => {
                "The dialog is already open as a Popover, and therefore cannot be opened as a \
                 modal dialog."
            }
            PopoverErrorKind::DialogNotInDocument => "The element is not in a Document.",
            PopoverErrorKind::DialogNotFullyActive => {
                "Invalid for dialogs within documents that are not fully active."
            }
        };
        if self.after_event {
            format!(
                "{text} This might have been the result of the \"beforetoggle\" event handler \
                 changing the state of this popover."
            )
        } else {
            text.to_owned()
        }
    }

    fn stopped() -> PopoverError {
        PopoverError {
            kind: PopoverErrorKind::Stopped,
            after_event: false,
        }
    }
}

impl fmt::Display for PopoverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.name(), self.message())
    }
}

impl std::error::Error for PopoverError {}

/// What the popover algorithms need of their caller (ADR 0037 section 2).
pub trait TopLayerHost {
    /// The document. Callers borrow it shortly: the other methods run
    /// scripts that change it.
    fn doc(&mut self) -> &mut Document;

    /// Fires `beforetoggle` (a `ToggleEvent`) at `element`. Returns
    /// `Some(false)` if a listener canceled it, `Some(true)` if not, `None`
    /// if the host stopped (a script was terminated): the algorithm then
    /// ends with [`PopoverErrorKind::Stopped`].
    fn fire_beforetoggle(
        &mut self,
        element: NodeId,
        old: ToggleState,
        new: ToggleState,
        cancelable: bool,
        source: Option<NodeId>,
    ) -> Option<bool>;

    /// Queues the `toggle` task of `element` (the DOM manipulation task
    /// source). The task takes its states with [`Document::take_toggle`] and
    /// `serial`; a task whose serial is no longer the element's does nothing
    /// (a later change moved the task to the end of the queue).
    fn queue_toggle_task(&mut self, element: NodeId, serial: u64);

    /// The focused element of the document, if any (HTML "document's focused
    /// area"). A host without focus has none.
    fn focused_element(&mut self) -> Option<NodeId> {
        None
    }

    /// Runs the focusing steps for `element` (the popover focusing steps and
    /// the restoration of the focus). Returns false if `element` is not a
    /// focusable area that is rendered: nothing changed. A host without
    /// focus changes nothing.
    fn focus(&mut self, element: NodeId) -> bool {
        let _ = element;
        false
    }

    /// Gives the focus to the dialog `dialog` itself, the last step of the
    /// dialog focusing steps (measured: the dialog takes it although it is no
    /// focusable area). Returns false if nothing changed.
    fn focus_dialog(&mut self, dialog: NodeId) -> bool {
        self.focus(dialog)
    }

    /// Sets (`open` true) or removes the `open` attribute of `dialog`, as a
    /// script's attribute change would (the web layer runs the custom
    /// element reactions and the live `Attr` objects). Returns false if the
    /// host stopped.
    fn set_dialog_open(&mut self, dialog: NodeId, open: bool) -> bool {
        let doc = self.doc();
        if open {
            let _ = doc.set_attr(dialog, "open", "");
        } else {
            doc.remove_attr(dialog, "open");
        }
        true
    }

    /// Fires `cancel` (an `Event`, not bubbling) at `dialog`. Returns
    /// `Some(false)` if a listener canceled it, `None` if the host stopped.
    fn fire_cancel(&mut self, dialog: NodeId, cancelable: bool) -> Option<bool> {
        let _ = (dialog, cancelable);
        Some(true)
    }

    /// Queues the dialog `toggle` task of `dialog` (HTML "queue a dialog
    /// toggle event task"); the task takes its states with
    /// [`Document::take_dialog_toggle`]. A host without scripts runs it at
    /// once (nothing listens).
    fn queue_dialog_toggle_task(&mut self, dialog: NodeId, serial: u64) {
        let _ = self.doc().take_dialog_toggle(dialog, serial);
    }

    /// Queues the task that fires `close` at `dialog` (the user interaction
    /// task source). A host without scripts does nothing.
    fn queue_close_task(&mut self, dialog: NodeId) {
        let _ = dialog;
    }
}

/// The options of [`hide_popover`] (the arguments of HTML "hide popover
/// algorithm").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HideOptions {
    /// Focus the previously focused element when the focus is inside the
    /// popover (HTML "hide popover algorithm", `focusPreviousElement`).
    pub focus_previous: bool,
    /// Fire `beforetoggle` and queue the `toggle` task.
    pub fire_events: bool,
    /// Return the errors (else they end the algorithm silently).
    pub throw: bool,
    /// Do not require the `popover` attribute (its attribute change steps).
    pub ignore_dom: bool,
    /// The `source` of the events.
    pub source: Option<NodeId>,
}

/// A showing popover list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum List {
    Auto,
    Hint,
}

/// The state of a popover that is showing, is being shown or hidden, or has
/// a toggle task.
#[derive(Clone, Debug, Default)]
struct PopoverData {
    /// The list and the index in it, while the popover is in one.
    list: Option<(List, usize)>,
    /// HTML "popover trigger": the source of the show.
    invoker: Option<NodeId>,
    /// HTML "popover showing or hiding".
    busy: bool,
    /// HTML "popover toggle task tracker".
    toggle: Option<PendingToggle>,
    /// HTML "previously focused element": the element that had the focus
    /// when the popover opened a stack, which the hide gives it back to.
    previously_focused: Option<NodeId>,
    /// The popover has a close watcher (an auto or hint popover while it
    /// shows).
    watcher: bool,
}

/// The state of a dialog that is open, has a `toggle` task or a
/// `returnValue` (ADR 0037 section 1; `top_layer/dialog.rs`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DialogData {
    /// HTML "previously focused element".
    pub previously_focused: Option<NodeId>,
    /// HTML "request close return value".
    pub request_close_return_value: Option<String>,
    /// HTML "enable close watcher for request close".
    pub close_watcher_for_request_close: bool,
    /// `returnValue` (initially the empty string).
    pub return_value: String,
    /// HTML "dialog toggle task tracker".
    toggle: Option<PendingToggle>,
    /// HTML close watcher "is running cancel action".
    running_cancel: bool,
}

impl DialogData {
    /// True if the data holds nothing but the defaults.
    fn is_empty(&self) -> bool {
        self.previously_focused.is_none()
            && self.request_close_return_value.is_none()
            && !self.close_watcher_for_request_close
            && self.return_value.is_empty()
            && self.toggle.is_none()
            && !self.running_cancel
    }
}

/// The top layer and the popover state of an arena (ADR 0037 section 1).
/// Only the arena's document of [`NodeId::DOCUMENT`] has a browsing context,
/// so all of it belongs to that document.
#[derive(Debug, Default)]
pub struct TopLayerState {
    /// The top layer in order; an element is in it at most once.
    layer: Vec<NodeId>,
    /// HTML "showing auto popover list" (with the hints that have an auto
    /// popover as ancestor) and "showing hint popover list".
    auto: Vec<NodeId>,
    hint: Vec<NodeId>,
    popovers: HashMap<NodeId, PopoverData>,
    dialogs: HashMap<NodeId, DialogData>,
    /// HTML "open dialogs list": the dialogs that `show()` and `showModal()`
    /// opened, in that order (the flag `DIALOG_OPEN`).
    open_dialogs: Vec<NodeId>,
    /// HTML "popover pointerdown target" (light dismiss).
    pointerdown_target: Option<NodeId>,
    /// HTML "dialog pointerdown target" (light dismiss of dialogs).
    dialog_pointerdown_target: Option<NodeId>,
    /// The close watchers (`watchers.rs`).
    watchers: watchers::CloseWatchers,
    /// The showing popovers whose `popover` attribute changed state since the
    /// last [`Document::take_popover_attribute_hides`].
    attribute_hides: Vec<NodeId>,
    /// The serial of the last toggle task.
    serial: u64,
    /// The modal dialog that blocks the document (HTML "blocked by a modal
    /// dialog"): the last element of `layer` that has the flag `MODAL`. It
    /// is made again by [`Document::refresh_blocking`] after each change of
    /// the flag or of the layer membership of a modal dialog (show, close,
    /// removal), so that [`TopLayerState::blocking`] costs O(1).
    blocking: Option<NodeId>,
}

impl TopLayerState {
    fn list(&self, list: List) -> &Vec<NodeId> {
        match list {
            List::Auto => &self.auto,
            List::Hint => &self.hint,
        }
    }

    fn list_mut(&mut self, list: List) -> &mut Vec<NodeId> {
        match list {
            List::Auto => &mut self.auto,
            List::Hint => &mut self.hint,
        }
    }

    fn list_of(&self, element: NodeId) -> Option<List> {
        self.popovers
            .get(&element)
            .and_then(|d| d.list)
            .map(|(list, _)| list)
    }

    /// The index of `element` in its list, if it is in `list`.
    fn index_in(&self, element: NodeId, list: List) -> Option<usize> {
        match self.popovers.get(&element)?.list? {
            (l, index) if l == list => Some(index),
            _ => None,
        }
    }

    /// HTML "popover stack position" in the auto list followed by the hint
    /// list (0 for the first).
    fn position(&self, element: NodeId) -> Option<usize> {
        match self.popovers.get(&element)?.list? {
            (List::Auto, index) => Some(index),
            (List::Hint, index) => Some(self.auto.len() + index),
        }
    }

    fn push(&mut self, list: List, element: NodeId) {
        let index = self.list(list).len();
        self.list_mut(list).push(element);
        self.popovers.entry(element).or_default().list = Some((list, index));
    }

    /// Takes `element` out of its list; the indexes after it move.
    fn remove_from_list(&mut self, element: NodeId) {
        let Some((list, index)) = self.popovers.get_mut(&element).and_then(|d| d.list.take())
        else {
            return;
        };
        let items = self.list_mut(list);
        if items.get(index) != Some(&element) {
            return;
        }
        items.remove(index);
        let moved: Vec<NodeId> = self.list(list).get(index..).unwrap_or_default().to_vec();
        for (offset, node) in moved.into_iter().enumerate() {
            if let Some(data) = self.popovers.get_mut(&node) {
                data.list = Some((list, index + offset));
            }
        }
    }

    /// CSS Position 4 "add an element to the top layer": moves it to the end.
    fn add_to_layer(&mut self, element: NodeId) {
        self.remove_from_layer(element);
        self.layer.push(element);
    }

    fn remove_from_layer(&mut self, element: NodeId) {
        if let Some(index) = self.layer.iter().rposition(|&n| n == element) {
            self.layer.remove(index);
        }
    }

    /// Notes a toggle task of `element` from `old` to `new` and returns its
    /// serial: a pending task keeps its old state and is replaced (HTML
    /// "queue a popover toggle event task").
    fn queue_toggle(
        &mut self,
        element: NodeId,
        old: ToggleState,
        new: ToggleState,
        source: Option<NodeId>,
    ) -> u64 {
        self.serial += 1;
        let serial = self.serial;
        let data = self.popovers.entry(element).or_default();
        let old = data.toggle.map_or(old, |t| t.old);
        data.toggle = Some(PendingToggle {
            old,
            new,
            source,
            serial,
        });
        serial
    }

    /// Drops the data of `element` if nothing needs it any more.
    fn tidy(&mut self, element: NodeId) {
        if self.popovers.get(&element).is_some_and(|d| {
            d.list.is_none()
                && !d.busy
                && d.toggle.is_none()
                && d.invoker.is_none()
                && d.previously_focused.is_none()
                && !d.watcher
        }) {
            self.popovers.remove(&element);
        }
    }

    /// The elements of the top layer, in order.
    pub fn layer(&self) -> &[NodeId] {
        &self.layer
    }

    /// HTML "blocked by a modal dialog": the last element of the top layer
    /// that is a modal dialog (kept by [`Document::refresh_blocking`]).
    pub fn blocking(&self) -> Option<NodeId> {
        self.blocking
    }

    /// [`TopLayerState::blocking`] by a walk over the top layer from its end
    /// (the length of the top layer).
    pub(crate) fn blocking_scan(&self, doc: &Document) -> Option<NodeId> {
        self.layer
            .iter()
            .rev()
            .copied()
            .find(|&n| doc.element(n).is_some_and(crate::ElementData::is_modal))
    }
}

impl Document {
    /// The top layer state, made at its first use.
    fn top_layer_state(&mut self) -> &mut TopLayerState {
        let early = self.early_activation;
        self.top_layer.get_or_insert_with(|| {
            let mut state = TopLayerState::default();
            if early {
                state.watchers.note_activation();
            }
            Box::new(state)
        })
    }

    /// The top layer state, if the arena ever showed a popover.
    pub fn top_layer(&self) -> Option<&TopLayerState> {
        self.top_layer.as_deref()
    }

    /// True if the arena has showing popovers or a top layer: the removal
    /// steps of the change functions run only then.
    #[inline]
    pub(crate) fn has_top_layer_entries(&self) -> bool {
        // Every showing popover is in the top layer.
        self.top_layer
            .as_ref()
            .is_some_and(|s| !s.layer.is_empty() || !s.open_dialogs.is_empty())
    }

    /// The modal dialog that blocks the document: the last modal dialog of
    /// the top layer (HTML "blocked by a modal dialog"). O(1).
    pub fn blocking_dialog(&self) -> Option<NodeId> {
        self.top_layer.as_deref()?.blocking()
    }

    /// Makes [`TopLayerState::blocking`] again, after a change of the `MODAL`
    /// flag or of the layer membership of a modal dialog. Costs the length
    /// of the top layer (a walk from its end that stops at the first modal
    /// dialog).
    pub(crate) fn refresh_blocking(&mut self) {
        let Some(state) = self.top_layer.as_deref() else {
            return;
        };
        let blocking = state.blocking_scan(self);
        if let Some(state) = self.top_layer.as_deref_mut() {
            state.blocking = blocking;
        }
    }

    /// True if `node` is an HTML `dialog` element with the `open` attribute.
    pub fn is_open_dialog(&self, node: NodeId) -> bool {
        self.element(node)
            .is_some_and(|e| e.is_html_named(&local_name!("dialog")) && e.has_attr("open"))
    }

    /// True if `node` is a `dialog` that is shown: open (the `open`
    /// attribute) or showing as a popover. A click on its text, padding or
    /// backdrop focuses it (Chromium 148, measured: `e-click-*`,
    /// `x-popover-dialog` of `tools/probes/web-top-layer.json`).
    pub fn is_shown_dialog(&self, node: NodeId) -> bool {
        self.is_open_dialog(node)
            || (self.is_popover_showing(node)
                && self
                    .element(node)
                    .is_some_and(|e| e.is_html_named(&local_name!("dialog"))))
    }

    /// True if a modal dialog blocks the document and `node` is not that
    /// dialog or a flat tree descendant of it: the node is inert (ADR 0037
    /// section 7). A node outside the document's flat tree is not (it is not
    /// in the document). Costs the flat tree depth of `node`; the style walk
    /// does not use it (it sets `ComputedStyle::modal_inert` once per
    /// document).
    pub fn is_modal_inert(&self, node: NodeId) -> bool {
        match self.blocking_dialog() {
            Some(dialog) => self.is_inert_under(node, dialog),
            None => false,
        }
    }

    /// [`Document::is_modal_inert`] for the blocking dialog `dialog`.
    pub fn is_inert_under(&self, node: NodeId, dialog: NodeId) -> bool {
        let mut top = node;
        for ancestor in std::iter::once(node).chain(self.flat_ancestors(node)) {
            if ancestor == dialog {
                return false;
            }
            top = ancestor;
        }
        matches!(self.node(top).data, crate::NodeData::Document)
    }

    /// True if `element` is a showing popover.
    pub fn is_popover_showing(&self, element: NodeId) -> bool {
        self.get(element)
            .and_then(crate::Node::as_element)
            .is_some_and(crate::ElementData::is_popover_showing)
    }

    /// The popover state of `element`: `None` for an element that is not an
    /// HTML element with a `popover` attribute.
    pub fn popover_state(&self, element: NodeId) -> Option<PopoverState> {
        let e = self.get(element)?.as_element()?;
        if !e.is_html() {
            return None;
        }
        PopoverState::of(e.attr("popover"))
    }

    /// True if `element` has a `toggle` task that waits.
    pub fn has_pending_toggle(&self, element: NodeId) -> bool {
        self.top_layer
            .as_deref()
            .and_then(|s| s.popovers.get(&element))
            .is_some_and(|d| d.toggle.is_some())
    }

    /// The pending `toggle` task of `element` if its serial is `serial`, and
    /// forgets it. `None` for a task that a later change replaced.
    pub fn take_toggle(&mut self, element: NodeId, serial: u64) -> Option<PendingToggle> {
        let state = self.top_layer.as_deref_mut()?;
        let data = state.popovers.get_mut(&element)?;
        if data.toggle.is_none_or(|t| t.serial != serial) {
            return None;
        }
        let toggle = data.toggle.take();
        state.tidy(element);
        toggle
    }

    /// The showing popovers whose `popover` attribute changed to another
    /// state since the last call: the caller runs their attribute change
    /// steps ([`run_popover_attribute_steps`]).
    pub fn take_popover_attribute_hides(&mut self) -> Vec<NodeId> {
        self.top_layer
            .as_deref_mut()
            .map(|s| std::mem::take(&mut s.attribute_hides))
            .unwrap_or_default()
    }

    /// True if [`Document::take_popover_attribute_hides`] has elements.
    pub fn has_popover_attribute_hides(&self) -> bool {
        self.top_layer
            .as_deref()
            .is_some_and(|s| !s.attribute_hides.is_empty())
    }

    /// The data of the dialog `dialog`.
    pub fn dialog_data(&self, dialog: NodeId) -> Option<&DialogData> {
        self.top_layer.as_deref()?.dialogs.get(&dialog)
    }

    /// The data of the dialog `dialog`, made at the first use.
    pub fn dialog_data_mut(&mut self, dialog: NodeId) -> &mut DialogData {
        self.top_layer_state().dialogs.entry(dialog).or_default()
    }

    /// Drops the data of `dialog` if it holds only defaults.
    fn tidy_dialog(&mut self, dialog: NodeId) {
        if let Some(state) = self.top_layer.as_deref_mut()
            && state.dialogs.get(&dialog).is_some_and(DialogData::is_empty)
        {
            state.dialogs.remove(&dialog);
        }
    }

    /// HTML "popover pointerdown target" (sub-feature 6c).
    pub fn popover_pointerdown_target(&self) -> Option<NodeId> {
        self.top_layer.as_deref()?.pointerdown_target
    }

    /// Sets the "popover pointerdown target".
    pub fn set_popover_pointerdown_target(&mut self, target: Option<NodeId>) {
        if target.is_some() || self.top_layer.is_some() {
            self.top_layer_state().pointerdown_target = target;
        }
    }

    /// The source of the show of the showing popover `element` (HTML
    /// "popover trigger").
    pub fn popover_invoker(&self, element: NodeId) -> Option<NodeId> {
        self.top_layer.as_deref()?.popovers.get(&element)?.invoker
    }

    /// The showing auto popovers (with the hints that have an auto popover
    /// as ancestor) and the showing hint popovers, bottom first.
    pub fn showing_popover_lists(&self) -> (&[NodeId], &[NodeId]) {
        match self.top_layer.as_deref() {
            Some(s) => (&s.auto, &s.hint),
            None => (&[], &[]),
        }
    }

    /// Sets or clears flag bits of `element`.
    fn set_flags(&mut self, element: NodeId, bits: u8, on: bool) {
        if let Some(e) = self.element_mut(element) {
            if on {
                e.flags |= bits;
            } else {
                e.flags &= !bits;
            }
        }
    }

    /// Reports that `element` entered or left the top layer or changed its
    /// popover visibility: a journal entry (the engine restyles), no mutation
    /// record.
    fn record_top_layer(&mut self, element: NodeId) {
        self.generation += 1;
        if self.journaling {
            self.journal.record(&Change::TopLayer { element });
        } else {
            self.journal.skip();
        }
    }

    /// The `popover` attribute of `element` changed from `old` to `new`:
    /// notes a showing popover whose state changed (HTML attribute change
    /// steps of `popover`; the caller runs the hide with events).
    pub(crate) fn popover_attribute_changed(
        &mut self,
        element: NodeId,
        old: Option<&str>,
        new: Option<&str>,
    ) {
        if self.is_popover_showing(element) && PopoverState::of(old) != PopoverState::of(new) {
            let state = self.top_layer_state();
            if !state.attribute_hides.contains(&element) {
                state.attribute_hides.push(element);
            }
        }
    }

    /// The removing steps of the showing popovers and top-layer elements in
    /// the shadow-including subtree of the removed node `root`: they are
    /// hidden without events and without focus changes (HTML "removing
    /// steps"; measured: no `beforetoggle`, a queued `toggle` task still
    /// fires).
    pub(crate) fn top_layer_removed(&mut self, root: NodeId) {
        let found: Vec<NodeId> = self
            .shadow_including_descendants(root)
            .filter(|&n| {
                self.element(n).is_some_and(|e| {
                    e.flags & (flags::POPOVER_SHOWING | flags::TOP_LAYER | flags::DIALOG_OPEN) != 0
                })
            })
            .collect();
        let mut left_layer = false;
        for element in found {
            // HTML "removing steps" of `dialog`: an open dialog runs the
            // dialog cleanup steps; a modal one leaves the top layer below.
            if self
                .element(element)
                .is_some_and(crate::ElementData::is_dialog_open)
            {
                self.dialog_cleanup(element);
            }
            if self.is_popover_showing(element) {
                let options = HideOptions {
                    ignore_dom: true,
                    ..HideOptions::default()
                };
                // Without events no script runs: only a stop could fail.
                let _ = hide_popover(&mut Silent(self), element, options);
            }
            if self
                .element(element)
                .is_some_and(crate::ElementData::in_top_layer)
            {
                left_layer = true;
                self.set_flags(element, flags::TOP_LAYER | flags::MODAL, false);
                if let Some(state) = self.top_layer.as_deref_mut() {
                    state.remove_from_layer(element);
                }
                self.record_top_layer(element);
            }
        }
        if left_layer {
            self.refresh_blocking();
        }
    }

    /// Forgets the nodes that left the arena (`take_subtree`). Returns the
    /// `returnValue` of the dialogs that have one: it goes with the node.
    pub(crate) fn forget_top_layer_nodes(&mut self, taken: &[NodeId]) -> Vec<(NodeId, String)> {
        let mut values = Vec::new();
        let Some(state) = self.top_layer.as_deref_mut() else {
            return values;
        };
        for node in taken {
            if state.popovers.remove(node).is_some_and(|d| d.watcher) {
                state.watchers.destroy(*node, WatcherKind::Popover);
            }
            if let Some(data) = state.dialogs.remove(node)
                && !data.return_value.is_empty()
            {
                values.push((*node, data.return_value));
            }
            if let Some(index) = state.open_dialogs.iter().position(|n| n == node) {
                state.open_dialogs.remove(index);
                state.watchers.destroy(*node, WatcherKind::Dialog);
            }
            state.attribute_hides.retain(|n| n != node);
            if state.pointerdown_target == Some(*node) {
                state.pointerdown_target = None;
            }
            if state.dialog_pointerdown_target == Some(*node) {
                state.dialog_pointerdown_target = None;
            }
        }
        values
    }
}

/// The host of the steps without events: the removing steps, and a hide that
/// did not finish (see [`hide_stack_until`]).
struct Silent<'a>(&'a mut Document);

impl TopLayerHost for Silent<'_> {
    fn doc(&mut self) -> &mut Document {
        self.0
    }

    fn fire_beforetoggle(
        &mut self,
        _element: NodeId,
        _old: ToggleState,
        _new: ToggleState,
        _cancelable: bool,
        _source: Option<NodeId>,
    ) -> Option<bool> {
        Some(true)
    }

    fn queue_toggle_task(&mut self, element: NodeId, serial: u64) {
        let _ = self.0.take_toggle(element, serial);
    }
}

#[cfg(test)]
mod tests;
