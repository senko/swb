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
}

/// The options of [`hide_popover`] (the arguments of HTML "hide popover
/// algorithm").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HideOptions {
    /// Focus the previously focused element (sub-feature 6c).
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
}

/// The state of a dialog in the top layer or with a close watcher (ADR 0037
/// section 1; sub-feature 6d fills it).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DialogData {
    /// HTML "previously focused element".
    pub previously_focused: Option<NodeId>,
    /// HTML "request close return value".
    pub request_close_return_value: Option<String>,
    /// HTML "enable close watcher for request close".
    pub close_watcher_for_request_close: bool,
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
    /// HTML "popover pointerdown target" (sub-feature 6c).
    pointerdown_target: Option<NodeId>,
    /// The showing popovers whose `popover` attribute changed state since the
    /// last [`Document::take_popover_attribute_hides`].
    attribute_hides: Vec<NodeId>,
    /// The serial of the last toggle task.
    serial: u64,
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
        if self
            .popovers
            .get(&element)
            .is_some_and(|d| d.list.is_none() && !d.busy && d.toggle.is_none())
        {
            self.popovers.remove(&element);
        }
    }

    /// The elements of the top layer, in order.
    pub fn layer(&self) -> &[NodeId] {
        &self.layer
    }

    /// HTML "blocked by a modal dialog": the last element of the top layer
    /// that is a modal dialog.
    pub fn blocking(&self, doc: &Document) -> Option<NodeId> {
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
        self.top_layer.get_or_insert_default()
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
        self.top_layer.as_ref().is_some_and(|s| !s.layer.is_empty())
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

    /// The data of the dialog `dialog` (sub-feature 6d).
    pub fn dialog_data(&self, dialog: NodeId) -> Option<&DialogData> {
        self.top_layer.as_deref()?.dialogs.get(&dialog)
    }

    /// The data of the dialog `dialog`, made at the first use.
    pub fn dialog_data_mut(&mut self, dialog: NodeId) -> &mut DialogData {
        self.top_layer_state().dialogs.entry(dialog).or_default()
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
                self.element(n)
                    .is_some_and(|e| e.flags & (flags::POPOVER_SHOWING | flags::TOP_LAYER) != 0)
            })
            .collect();
        for element in found {
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
                self.set_flags(element, flags::TOP_LAYER | flags::MODAL, false);
                if let Some(state) = self.top_layer.as_deref_mut() {
                    state.remove_from_layer(element);
                }
                self.record_top_layer(element);
            }
        }
    }

    /// Forgets the nodes that left the arena (`take_subtree`).
    pub(crate) fn forget_top_layer_nodes(&mut self, taken: &[NodeId]) {
        let Some(state) = self.top_layer.as_deref_mut() else {
            return;
        };
        for node in taken {
            state.popovers.remove(node);
            state.dialogs.remove(node);
            state.attribute_hides.retain(|n| n != node);
            if state.pointerdown_target == Some(*node) {
                state.pointerdown_target = None;
            }
        }
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

/// HTML "check popover validity": `Ok(false)` if the element is not in the
/// expected visibility state.
fn check(
    doc: &Document,
    element: NodeId,
    expect_showing: bool,
    expected_document: Option<NodeId>,
    ignore_dom: bool,
) -> Result<bool, PopoverErrorKind> {
    let Some(e) = doc.get(element).and_then(crate::Node::as_element) else {
        return Err(PopoverErrorKind::NotPopover);
    };
    if !ignore_dom && doc.popover_state(element).is_none() {
        return Err(PopoverErrorKind::NotPopover);
    }
    // Chromium reports a popover in template contents and in a document
    // without a browsing context as not fully active, also for a hide of
    // a hidden popover and for a recheck after an event (measured,
    // `popover-validity-6a`): that check comes before the state check.
    if doc.node_document(element) != NodeId::DOCUMENT {
        return Err(PopoverErrorKind::NotFullyActive);
    }
    if e.is_popover_showing() != expect_showing {
        return Ok(false);
    }
    if !expect_showing && !doc.is_connected(element) {
        return Err(PopoverErrorKind::Disconnected);
    }
    if expected_document.is_some_and(|d| d != doc.node_document(element)) {
        return Err(PopoverErrorKind::OtherDocument);
    }
    if e.is_modal() && e.is_html_named(&local_name!("dialog")) {
        return Err(PopoverErrorKind::ModalDialog);
    }
    Ok(true)
}

/// The end of an algorithm with the error `kind`: returned if `throw` (a
/// stop always), else the algorithm ends silently.
fn fail(throw: bool, kind: PopoverErrorKind, after_event: bool) -> Result<(), PopoverError> {
    if throw || kind == PopoverErrorKind::Stopped {
        Err(PopoverError { kind, after_event })
    } else {
        Ok(())
    }
}

/// [`check`] after an event: `Ok(true)` to go on, `Ok(false)` to end
/// silently.
fn recheck<H: TopLayerHost + ?Sized>(
    host: &mut H,
    element: NodeId,
    expect_showing: bool,
    document: NodeId,
    (throw, ignore_dom): (bool, bool),
) -> Result<bool, PopoverError> {
    match check(
        host.doc(),
        element,
        expect_showing,
        Some(document),
        ignore_dom,
    ) {
        Ok(go) => Ok(go),
        Err(kind) => fail(throw, kind, true).map(|()| false),
    }
}

/// Sets the "showing or hiding" flag of `element` and returns its old value.
fn set_busy(doc: &mut Document, element: NodeId, busy: bool) -> bool {
    let state = doc.top_layer_state();
    let data = state.popovers.entry(element).or_default();
    let old = data.busy;
    data.busy = busy;
    if !busy {
        state.tidy(element);
    }
    old
}

/// HTML "show popover": `showPopover()`, `togglePopover()` and invokers.
/// `throw` returns the errors; `source` is the invoker or the `source`
/// option.
/// <https://html.spec.whatwg.org/multipage/popover.html#show-popover>
///
/// # Errors
///
/// The failed validity check (with `throw`), or a stop of the host.
pub fn show_popover<H: TopLayerHost + ?Sized>(
    host: &mut H,
    element: NodeId,
    throw: bool,
    source: Option<NodeId>,
) -> Result<(), PopoverError> {
    match check(host.doc(), element, false, None, false) {
        Ok(true) => {}
        Ok(false) => return Ok(()),
        Err(kind) => return fail(throw, kind, false),
    }
    // An open non-modal dialog shows as a popover too (Chromium, measured:
    // `popover-validity-6a` #1); only a modal one is refused, by the check.
    let document = host.doc().node_document(element);
    let nested = set_busy(host.doc(), element, true);
    let result = show_steps(host, element, (throw, nested), source, document);
    if !nested {
        set_busy(host.doc(), element, false);
    }
    result
}

/// The steps of [`show_popover`] after its first check; `nested` is true
/// for a show inside the `beforetoggle` event of another show of the same
/// element.
fn show_steps<H: TopLayerHost + ?Sized>(
    host: &mut H,
    element: NodeId,
    (throw, nested): (bool, bool),
    source: Option<NodeId>,
    document: NodeId,
) -> Result<(), PopoverError> {
    match host.fire_beforetoggle(
        element,
        ToggleState::Closed,
        ToggleState::Open,
        true,
        source,
    ) {
        None => return Err(PopoverError::stopped()),
        Some(false) => return Ok(()),
        Some(true) => {}
    }
    if !recheck(host, element, false, document, (throw, false))? {
        return Ok(());
    }
    let Some(original) = host.doc().popover_state(element) else {
        return Ok(());
    };
    let mut list = None;
    if original != PopoverState::Manual {
        let ancestor = topmost_popover_ancestor(host.doc(), element, source);
        let ancestor_list = ancestor.and_then(|a| host.doc().top_layer()?.list_of(a));
        // A show inside the event of the same element's show hides the others
        // without events (HTML of 2023, which Chromium follows).
        let fire = !nested;
        let target = match (original, ancestor_list) {
            (_, Some(List::Hint)) => {
                hide_stack_until(host, ancestor, List::Hint, fire)?;
                List::Hint
            }
            (PopoverState::Hint, None) => {
                hide_stack_until(host, None, List::Hint, fire)?;
                List::Hint
            }
            _ => {
                hide_stack_until(host, None, List::Hint, fire)?;
                hide_stack_until(host, ancestor, List::Auto, fire)?;
                List::Auto
            }
        };
        if host.doc().popover_state(element) != Some(original) {
            fail(throw, PopoverErrorKind::TypeChanged, true)?;
            return Ok(());
        }
        if !recheck(host, element, false, document, (throw, false))? {
            return Ok(());
        }
        list = Some(target);
    }
    let doc = host.doc();
    let state = doc.top_layer_state();
    if let Some(list) = list {
        state.push(list, element);
    }
    state.add_to_layer(element);
    state.popovers.entry(element).or_default().invoker = source;
    let serial = state.queue_toggle(element, ToggleState::Closed, ToggleState::Open, source);
    doc.set_flags(element, flags::POPOVER_SHOWING | flags::TOP_LAYER, true);
    doc.record_top_layer(element);
    host.queue_toggle_task(element, serial);
    Ok(())
}

/// HTML "hide popover algorithm": `hidePopover()`, invokers, the attribute
/// change steps (`ignore_dom`) and the removing steps (no events).
/// <https://html.spec.whatwg.org/multipage/popover.html#hide-popover-algorithm>
///
/// # Errors
///
/// The failed validity check (with `options.throw`), or a stop of the host.
pub fn hide_popover<H: TopLayerHost + ?Sized>(
    host: &mut H,
    element: NodeId,
    mut options: HideOptions,
) -> Result<(), PopoverError> {
    match check(host.doc(), element, true, None, options.ignore_dom) {
        Ok(true) => {}
        Ok(false) => return Ok(()),
        Err(kind) => return fail(options.throw, kind, true),
    }
    let document = host.doc().node_document(element);
    let nested = set_busy(host.doc(), element, true);
    if nested {
        options.fire_events = false;
    }
    let result = hide_steps(host, element, options, document);
    if !nested {
        set_busy(host.doc(), element, false);
    }
    result
}

fn hide_steps<H: TopLayerHost + ?Sized>(
    host: &mut H,
    element: NodeId,
    options: HideOptions,
    document: NodeId,
) -> Result<(), PopoverError> {
    let flags_of = (options.throw, options.ignore_dom);
    let list = host.doc().top_layer().and_then(|s| s.list_of(element));
    if let Some(list) = list {
        hide_above(host, element, list, options.fire_events)?;
        if !recheck(host, element, true, document, flags_of)? {
            return Ok(());
        }
    }
    if options.fire_events {
        let fired = host.fire_beforetoggle(
            element,
            ToggleState::Open,
            ToggleState::Closed,
            false,
            options.source,
        );
        if fired.is_none() {
            return Err(PopoverError::stopped());
        }
        // A listener may have shown popovers above this one.
        if let Some(list) = host.doc().top_layer().and_then(|s| s.list_of(element)) {
            hide_above(host, element, list, false)?;
        }
        if !recheck(host, element, true, document, flags_of)? {
            return Ok(());
        }
    }
    remove_popover(host.doc(), element);
    if options.fire_events {
        let doc = host.doc();
        let serial = doc.top_layer_state().queue_toggle(
            element,
            ToggleState::Open,
            ToggleState::Closed,
            options.source,
        );
        host.queue_toggle_task(element, serial);
    }
    Ok(())
}

/// Hides the popovers above `element` in `list`; for the auto list also all
/// popovers of the hint list.
fn hide_above<H: TopLayerHost + ?Sized>(
    host: &mut H,
    element: NodeId,
    list: List,
    fire: bool,
) -> Result<(), PopoverError> {
    if list == List::Auto {
        hide_stack_until(host, None, List::Hint, fire)?;
    }
    hide_stack_until(host, Some(element), list, fire)
}

/// The end of a hide: the popover leaves its list and the top layer.
fn remove_popover(doc: &mut Document, element: NodeId) {
    let state = doc.top_layer_state();
    state.remove_from_list(element);
    state.remove_from_layer(element);
    if let Some(data) = state.popovers.get_mut(&element) {
        data.invoker = None;
    }
    state.tidy(element);
    doc.set_flags(element, flags::POPOVER_SHOWING | flags::TOP_LAYER, false);
    doc.record_top_layer(element);
}

/// HTML "hide popover stack until": hides the popovers of `list` above
/// `endpoint` (all of them if it is not in the list), the top first. The
/// popovers that the events of the first ones show are hidden without
/// events.
fn hide_stack_until<H: TopLayerHost + ?Sized>(
    host: &mut H,
    endpoint: Option<NodeId>,
    list: List,
    fire_events: bool,
) -> Result<(), PopoverError> {
    let keep = |host: &mut H| {
        let state = host.doc().top_layer()?;
        let items = state.list(list);
        let keep = endpoint
            .and_then(|e| state.index_in(e, list))
            .map_or(0, |i| i + 1);
        (items.len() > keep).then(|| (items.len() - keep, items.last().copied()))
    };
    let Some((mut with_events, _)) = keep(host) else {
        return Ok(());
    };
    while let Some((_, Some(top))) = keep(host) {
        let options = HideOptions {
            fire_events: fire_events && with_events > 0,
            // The list holds showing popovers only: one whose attribute
            // changed is hidden too.
            ignore_dom: true,
            ..HideOptions::default()
        };
        with_events = with_events.saturating_sub(1);
        hide_popover(host, top, options)?;
        // A hide that did not finish (an element that moved to another
        // document) must not stop the walk.
        let still = host
            .doc()
            .top_layer()
            .is_some_and(|s| s.list(list).last() == Some(&top));
        if still {
            remove_popover(host.doc(), top);
        }
    }
    Ok(())
}

/// HTML "hide all popovers until" (light dismiss, sub-feature 6c): hides the
/// popovers above `endpoint`, or all of them.
///
/// # Errors
///
/// A stop of the host.
pub fn hide_all_popovers_until<H: TopLayerHost + ?Sized>(
    host: &mut H,
    endpoint: Option<NodeId>,
    fire_events: bool,
) -> Result<(), PopoverError> {
    let list = endpoint.and_then(|e| host.doc().top_layer()?.list_of(e));
    if list == Some(List::Hint) {
        hide_stack_until(host, endpoint, List::Hint, fire_events)?;
        hide_stack_until(host, None, List::Auto, fire_events)
    } else {
        hide_stack_until(host, None, List::Hint, fire_events)?;
        hide_stack_until(host, endpoint, List::Auto, fire_events)
    }
}

/// HTML "topmost popover ancestor" of the popover `element` that is about
/// to show: the showing auto or hint popover with the highest position of
/// which `element` is a flat tree descendant, or `source` an inclusive one.
/// The walks stop at the top of the lists: a chain of nested popovers shown
/// in order costs one step for each show.
///
/// The algorithm takes the highest position over all flat ancestors. The
/// nearest open ancestor would give the same popover in every state that
/// the algorithms reach: a farther ancestor that showed after a nearer one
/// would have hidden the nearer one (a show hides the popovers above its own
/// topmost ancestor), so no nearer ancestor has a lower position than a
/// farther one (ADR 0037, "6a implementation notes", review fixes).
/// <https://html.spec.whatwg.org/multipage/popover.html#topmost-popover-ancestor>
pub fn topmost_popover_ancestor(
    doc: &Document,
    element: NodeId,
    source: Option<NodeId>,
) -> Option<NodeId> {
    let state = doc.top_layer.as_deref()?;
    let top = (state.auto.len() + state.hint.len()).checked_sub(1)?;
    let mut best: Option<(usize, NodeId)> = None;
    let mut walk = |start: Option<NodeId>| {
        let mut current = start;
        while let Some(node) = current {
            if doc
                .element(node)
                .is_some_and(crate::ElementData::is_popover_showing)
                && let Some(position) = state.position(node)
            {
                if best.is_none_or(|(b, _)| position > b) {
                    best = Some((position, node));
                }
                if position == top {
                    return true;
                }
            }
            current = doc.flat_parent(node);
        }
        false
    };
    if !walk(doc.flat_parent(element)) {
        walk(source);
    }
    best.map(|(_, node)| node)
}

/// `togglePopover(force)`: hides a showing popover, else shows it; with
/// `force`, only towards that state. Returns whether it shows afterwards.
///
/// # Errors
///
/// The errors of [`show_popover`] and [`hide_popover`], or of the validity
/// check when nothing changes.
pub fn toggle_popover<H: TopLayerHost + ?Sized>(
    host: &mut H,
    element: NodeId,
    force: Option<bool>,
    source: Option<NodeId>,
) -> Result<bool, PopoverError> {
    let showing = host.doc().is_popover_showing(element);
    if showing && force != Some(true) {
        let options = HideOptions {
            focus_previous: true,
            fire_events: true,
            throw: true,
            ignore_dom: false,
            source: None,
        };
        hide_popover(host, element, options)?;
    } else if force != Some(false) {
        show_popover(host, element, true, source)?;
    } else if let Err(kind) = check(host.doc(), element, showing, None, false) {
        return Err(PopoverError {
            kind,
            after_event: false,
        });
    }
    Ok(host.doc().is_popover_showing(element))
}

/// HTML "popover target attribute activation behavior": the invoker
/// `invoker` acts on its target `popover` with `action`, for a click whose
/// target was `event_target`. Errors end it silently.
/// <https://html.spec.whatwg.org/multipage/popover.html#popover-target-attribute-activation-behavior>
///
/// # Errors
///
/// A stop of the host.
pub fn popover_target_activation<H: TopLayerHost + ?Sized>(
    host: &mut H,
    invoker: NodeId,
    popover: NodeId,
    (action, event_target): (TargetAction, NodeId),
) -> Result<(), PopoverError> {
    let doc = host.doc();
    if popover != invoker
        && doc.is_shadow_including_inclusive_ancestor(popover, event_target)
        && doc.is_shadow_including_inclusive_ancestor(invoker, popover)
    {
        return Ok(());
    }
    let showing = doc.is_popover_showing(popover);
    match (action, showing) {
        (TargetAction::Show, true) | (TargetAction::Hide, false) => Ok(()),
        (_, true) => {
            let options = HideOptions {
                focus_previous: true,
                fire_events: true,
                throw: false,
                ignore_dom: false,
                source: Some(invoker),
            };
            hide_popover(host, popover, options)
        }
        (_, false) => {
            if check(doc, popover, false, None, false) != Ok(true) {
                return Ok(());
            }
            show_popover(host, popover, false, Some(invoker))
        }
    }
}

/// The element that the `popovertarget` attribute of `invoker` names: the
/// element with that id in the invoker's tree (HTML "get the
/// `popovertarget`-associated element" without an explicitly set element).
pub fn popover_target_by_id(doc: &Document, invoker: NodeId) -> Option<NodeId> {
    let id = doc.element(invoker)?.attr("popovertarget")?;
    if id.is_empty() {
        return None;
    }
    doc.get_element_by_id(doc.root(invoker), id)
}

/// Runs the attribute change steps of the showing popovers whose `popover`
/// attribute changed state ([`Document::take_popover_attribute_hides`]): a
/// hide with `beforetoggle` (not cancelable) and a `toggle` task.
///
/// # Errors
///
/// A stop of the host.
pub fn run_popover_attribute_steps<H: TopLayerHost + ?Sized>(
    host: &mut H,
) -> Result<(), PopoverError> {
    loop {
        let pending = host.doc().take_popover_attribute_hides();
        if pending.is_empty() {
            return Ok(());
        }
        for element in pending {
            let options = HideOptions {
                focus_previous: true,
                fire_events: true,
                throw: false,
                ignore_dom: true,
                source: None,
            };
            hide_popover(host, element, options)?;
        }
    }
}

#[cfg(test)]
mod tests;
