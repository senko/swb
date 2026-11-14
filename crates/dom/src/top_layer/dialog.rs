//! The `dialog` element (HTML §4.11.4,
//! <https://html.spec.whatwg.org/multipage/interactive-elements.html#the-dialog-element>;
//! ADR 0037 sections 1, 5, 6, 10 and 11): `show()`, `showModal()`,
//! `close()`, `requestClose()`, the dialog setup and cleanup steps, the
//! computed closed-by state, the dialog focusing steps and the dialog of a
//! form with the method `dialog`.
//!
//! The algorithms run with a [`TopLayerHost`], as those of popovers: it fires
//! `beforetoggle` and `cancel`, queues the dialog `toggle` task and the
//! `close` task, sets the `open` attribute and moves the focus.
//!
//! Where Chromium 148 differs from the HTML Standard of 2026-11-14, swb
//! follows Chromium (measured, `tools/probes/web-top-layer.json`):
//!
//! - Removing the `open` attribute of a modal dialog keeps it modal and in
//!   the top layer (`dialog-basics` #7); the attribute change runs the
//!   cleanup steps only (the close watcher goes).
//! - The dialog focusing steps ignore the dialog's own `autofocus` attribute
//!   and search the flat tree, into shadow trees whose hosts do not delegate
//!   focus (`dialog-focus` #4, #6).
//! - A dialog's `close` and `toggle` tasks: Chromium runs them in either
//!   order; swb queues `toggle` first.
//! - A form with the method `dialog` submitted without a submitter sets
//!   `returnValue` to the empty string (`dialog-form` #7).
//!
//! Not measured, swb's choice: a parsed or script-set `open` attribute does
//! not run the dialog setup steps (no close watcher, not in the open dialogs
//! list), so only dialogs that `show()` or `showModal()` opened react to
//! close requests and light dismiss.

use html5ever::local_name;

use super::watchers::request_close;
use super::{
    Document, NodeId, PopoverError, PopoverErrorKind, ToggleState, TopLayerHost, WatcherKind,
    hide_all_popovers_until, topmost_popover_ancestor,
};
use crate::tree::flags;

/// The state of the `closedby` attribute (HTML "computed closed-by state").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClosedBy {
    /// `any`: close requests and light dismiss.
    Any,
    /// `closerequest`: close requests.
    CloseRequest,
    /// `none`: only scripts.
    None,
}

impl ClosedBy {
    /// The keyword of the state (`closedBy` returns it).
    pub fn as_str(self) -> &'static str {
        match self {
            ClosedBy::Any => "any",
            ClosedBy::CloseRequest => "closerequest",
            ClosedBy::None => "none",
        }
    }

    /// HTML "computed closed-by state" of `dialog`: the attribute's state; a
    /// missing or invalid value is the Auto state, which is "close request"
    /// for a modal dialog and "none" otherwise (measured, `popover-reflection`
    /// #5).
    pub fn of(doc: &Document, dialog: NodeId) -> ClosedBy {
        let Some(e) = doc.element(dialog) else {
            return ClosedBy::None;
        };
        match e.attr("closedby") {
            Some(v) if v.eq_ignore_ascii_case("any") => ClosedBy::Any,
            Some(v) if v.eq_ignore_ascii_case("closerequest") => ClosedBy::CloseRequest,
            Some(v) if v.eq_ignore_ascii_case("none") => ClosedBy::None,
            _ if e.is_modal() => ClosedBy::CloseRequest,
            _ => ClosedBy::None,
        }
    }
}

fn error(kind: PopoverErrorKind) -> PopoverError {
    PopoverError {
        kind,
        after_event: false,
    }
}

impl Document {
    /// `returnValue` of `dialog`.
    pub fn dialog_return_value(&self, dialog: NodeId) -> &str {
        self.dialog_data(dialog).map_or("", |d| &d.return_value)
    }

    /// Sets `returnValue` of `dialog`.
    pub fn set_dialog_return_value(&mut self, dialog: NodeId, value: &str) {
        if value.is_empty() && self.dialog_data(dialog).is_none() {
            return;
        }
        value.clone_into(&mut self.dialog_data_mut(dialog).return_value);
        self.tidy_dialog(dialog);
    }

    /// The pending dialog `toggle` task of `dialog` if its serial is
    /// `serial`, and forgets it. `None` for a task that a later change
    /// replaced.
    pub fn take_dialog_toggle(
        &mut self,
        dialog: NodeId,
        serial: u64,
    ) -> Option<super::PendingToggle> {
        let data = self.top_layer.as_deref_mut()?.dialogs.get_mut(&dialog)?;
        if data.toggle.is_none_or(|t| t.serial != serial) {
            return None;
        }
        let toggle = data.toggle.take();
        self.tidy_dialog(dialog);
        toggle
    }

    /// HTML "open dialogs list", in the order the dialogs opened.
    pub fn open_dialogs(&self) -> &[NodeId] {
        self.top_layer.as_deref().map_or(&[], |s| &s.open_dialogs)
    }

    /// True if a dialog is in the open dialogs list or a dialog pointerdown
    /// target is noted: a press and a release may light dismiss a dialog.
    pub fn dialog_light_dismiss_active(&self) -> bool {
        self.top_layer
            .as_deref()
            .is_some_and(|s| !s.open_dialogs.is_empty() || s.dialog_pointerdown_target.is_some())
    }

    /// HTML "dialog setup steps": `dialog` joins the open dialogs list and
    /// gets a close watcher (for a connected dialog of a fully active
    /// document only, as the attribute change steps say).
    fn dialog_setup(&mut self, dialog: NodeId) {
        let ready = self.node_document(dialog) == NodeId::DOCUMENT && self.is_connected(dialog);
        if !ready
            || self
                .element(dialog)
                .is_none_or(crate::ElementData::is_dialog_open)
        {
            return;
        }
        let state = self.top_layer_state();
        state.open_dialogs.push(dialog);
        state.watchers.create(dialog, WatcherKind::Dialog);
        self.set_flags(dialog, flags::DIALOG_OPEN, true);
    }

    /// HTML "dialog cleanup steps": `dialog` leaves the open dialogs list
    /// and its close watcher is destroyed.
    pub(crate) fn dialog_cleanup(&mut self, dialog: NodeId) {
        self.set_flags(dialog, flags::DIALOG_OPEN, false);
        if let Some(state) = self.top_layer.as_deref_mut() {
            if let Some(index) = state.open_dialogs.iter().rposition(|&n| n == dialog) {
                state.open_dialogs.remove(index);
            }
            state.watchers.destroy(dialog, WatcherKind::Dialog);
            if state.dialog_pointerdown_target == Some(dialog) {
                state.dialog_pointerdown_target = None;
            }
        }
    }

    /// The `open` attribute of `element` was removed: the attribute change
    /// steps of `dialog` run the cleanup steps. A modal dialog stays modal
    /// and in the top layer (Chromium, measured; ADR 0037 section 11).
    pub(crate) fn dialog_open_removed(&mut self, element: NodeId) {
        if self
            .element(element)
            .is_some_and(crate::ElementData::is_dialog_open)
        {
            self.dialog_cleanup(element);
        }
    }

    /// Queues the dialog toggle task state of `dialog` from `old` to `new`
    /// and returns its serial: a pending task keeps its old state (HTML
    /// "queue a dialog toggle event task").
    fn queue_dialog_toggle(
        &mut self,
        dialog: NodeId,
        old: ToggleState,
        new: ToggleState,
        source: Option<NodeId>,
    ) -> u64 {
        let state = self.top_layer_state();
        state.serial += 1;
        let serial = state.serial;
        let data = state.dialogs.entry(dialog).or_default();
        let old = data.toggle.map_or(old, |t| t.old);
        data.toggle = Some(super::PendingToggle {
            old,
            new,
            source,
            serial,
        });
        serial
    }
}

/// True if `dialog` is an element with an `open` attribute.
fn has_open(doc: &Document, dialog: NodeId) -> bool {
    doc.element(dialog).is_some_and(|e| e.has_attr("open"))
}

/// True if `dialog` is a modal dialog.
fn is_modal(doc: &Document, dialog: NodeId) -> bool {
    doc.element(dialog)
        .is_some_and(crate::ElementData::is_modal)
}

/// Fires `beforetoggle` from closed to open; `Ok(false)` if it was canceled.
fn fire_open<H: TopLayerHost + ?Sized>(
    host: &mut H,
    dialog: NodeId,
    source: Option<NodeId>,
) -> Result<bool, PopoverError> {
    host.fire_beforetoggle(dialog, ToggleState::Closed, ToggleState::Open, true, source)
        .ok_or_else(|| error(PopoverErrorKind::Stopped))
}

/// The steps that `show()` and `showModal()` share after the `open`
/// attribute is set: the previously focused element, the popovers that are
/// not ancestors of the dialog hide, the dialog focusing steps.
fn after_open<H: TopLayerHost + ?Sized>(host: &mut H, dialog: NodeId) -> Result<(), PopoverError> {
    let previous = host.focused_element();
    host.doc().dialog_data_mut(dialog).previously_focused = previous;
    let doc = host.doc();
    if doc.light_dismiss_active() {
        let until = topmost_popover_ancestor(doc, dialog, None);
        hide_all_popovers_until(host, until, true)?;
    }
    focusing_steps(host, dialog);
    Ok(())
}

/// `show()`: opens `dialog` as a non-modal dialog.
/// <https://html.spec.whatwg.org/multipage/interactive-elements.html#dom-dialog-show>
///
/// # Errors
///
/// `InvalidStateError` for an open modal dialog, or a stop of the host.
pub fn show_dialog<H: TopLayerHost + ?Sized>(
    host: &mut H,
    dialog: NodeId,
) -> Result<(), PopoverError> {
    let doc = host.doc();
    if has_open(doc, dialog) {
        if is_modal(doc, dialog) {
            return Err(error(PopoverErrorKind::DialogOpenModal));
        }
        return Ok(());
    }
    if !fire_open(host, dialog, None)? || has_open(host.doc(), dialog) {
        return Ok(());
    }
    let serial =
        host.doc()
            .queue_dialog_toggle(dialog, ToggleState::Closed, ToggleState::Open, None);
    host.queue_dialog_toggle_task(dialog, serial);
    if !host.set_dialog_open(dialog, true) {
        return Err(error(PopoverErrorKind::Stopped));
    }
    host.doc().dialog_setup(dialog);
    after_open(host, dialog)
}

/// `showModal()` (HTML "show a modal dialog"): opens `dialog` as a modal
/// dialog in the top layer; `source` is the invoker.
/// <https://html.spec.whatwg.org/multipage/interactive-elements.html#show-a-modal-dialog>
///
/// # Errors
///
/// `InvalidStateError` with Chromium's messages (an open non-modal dialog, a
/// document that is not fully active, a disconnected dialog, a showing
/// popover), or a stop of the host.
pub fn show_modal<H: TopLayerHost + ?Sized>(
    host: &mut H,
    dialog: NodeId,
    source: Option<NodeId>,
) -> Result<(), PopoverError> {
    let doc = host.doc();
    if has_open(doc, dialog) {
        if is_modal(doc, dialog) {
            return Ok(());
        }
        return Err(error(PopoverErrorKind::DialogOpenNonModal));
    }
    if doc.node_document(dialog) != NodeId::DOCUMENT {
        return Err(error(PopoverErrorKind::DialogNotFullyActive));
    }
    if !doc.is_connected(dialog) {
        return Err(error(PopoverErrorKind::DialogNotInDocument));
    }
    if doc.is_popover_showing(dialog) {
        return Err(error(PopoverErrorKind::DialogOpenAsPopover));
    }
    if !fire_open(host, dialog, source)? {
        return Ok(());
    }
    let doc = host.doc();
    if has_open(doc, dialog) || !doc.is_connected(dialog) || doc.is_popover_showing(dialog) {
        return Ok(());
    }
    let serial = doc.queue_dialog_toggle(dialog, ToggleState::Closed, ToggleState::Open, source);
    host.queue_dialog_toggle_task(dialog, serial);
    if !host.set_dialog_open(dialog, true) {
        return Err(error(PopoverErrorKind::Stopped));
    }
    let doc = host.doc();
    doc.dialog_setup(dialog);
    // A listener of the attribute change (a custom element reaction) may
    // have removed the dialog.
    if doc.is_connected(dialog) {
        let in_layer = doc
            .element(dialog)
            .is_some_and(crate::ElementData::in_top_layer);
        if !in_layer {
            doc.top_layer_state().add_to_layer(dialog);
        }
        doc.set_flags(dialog, flags::MODAL | flags::TOP_LAYER, true);
        doc.refresh_blocking();
        doc.record_top_layer(dialog);
    }
    after_open(host, dialog)
}

/// HTML "close the dialog" with the return value `result`: `close()`, the
/// close action of the dialog's close watcher, and the form method `dialog`.
/// <https://html.spec.whatwg.org/multipage/interactive-elements.html#close-the-dialog>
///
/// # Errors
///
/// A stop of the host.
pub fn close_dialog<H: TopLayerHost + ?Sized>(
    host: &mut H,
    dialog: NodeId,
    result: Option<String>,
) -> Result<(), PopoverError> {
    if !has_open(host.doc(), dialog) {
        return Ok(());
    }
    let fired = host.fire_beforetoggle(dialog, ToggleState::Open, ToggleState::Closed, false, None);
    if fired.is_none() {
        return Err(error(PopoverErrorKind::Stopped));
    }
    if !has_open(host.doc(), dialog) {
        return Ok(());
    }
    let serial =
        host.doc()
            .queue_dialog_toggle(dialog, ToggleState::Open, ToggleState::Closed, None);
    host.queue_dialog_toggle_task(dialog, serial);
    if !host.set_dialog_open(dialog, false) {
        return Err(error(PopoverErrorKind::Stopped));
    }
    let doc = host.doc();
    let was_modal = is_modal(doc, dialog);
    if doc
        .element(dialog)
        .is_some_and(crate::ElementData::in_top_layer)
        && was_modal
    {
        if let Some(state) = doc.top_layer.as_deref_mut() {
            state.remove_from_layer(dialog);
        }
        doc.set_flags(dialog, flags::TOP_LAYER, false);
    }
    if was_modal {
        doc.set_flags(dialog, flags::MODAL, false);
        doc.refresh_blocking();
        doc.record_top_layer(dialog);
    }
    let data = doc.dialog_data_mut(dialog);
    if let Some(result) = result {
        data.return_value = result;
    }
    data.request_close_return_value = None;
    let previous = data.previously_focused.take();
    doc.tidy_dialog(dialog);
    if let Some(previous) = previous {
        let inside = host
            .focused_element()
            .is_some_and(|f| host.doc().is_shadow_including_inclusive_ancestor(dialog, f));
        if inside || was_modal {
            host.focus(previous);
        }
    }
    host.queue_close_task(dialog);
    Ok(())
}

/// HTML "request to close the dialog" with the return value `result`
/// (`requestClose()`): `cancel` (always cancelable here), then the close.
/// <https://html.spec.whatwg.org/multipage/interactive-elements.html#dialog-request-close>
///
/// # Errors
///
/// A stop of the host.
pub fn request_close_dialog<H: TopLayerHost + ?Sized>(
    host: &mut H,
    dialog: NodeId,
    result: Option<String>,
) -> Result<(), PopoverError> {
    let doc = host.doc();
    if !has_open(doc, dialog)
        || !doc.is_connected(dialog)
        || doc.node_document(dialog) != NodeId::DOCUMENT
    {
        return Ok(());
    }
    let data = doc.dialog_data_mut(dialog);
    data.close_watcher_for_request_close = true;
    data.request_close_return_value = result;
    let outcome = request_close(host, dialog, WatcherKind::Dialog, false);
    let doc = host.doc();
    if doc.dialog_data(dialog).is_some() {
        doc.dialog_data_mut(dialog).close_watcher_for_request_close = false;
        doc.tidy_dialog(dialog);
    }
    outcome.map(|_| ())
}

/// The enabled state of the close watcher of `dialog` (HTML "set the dialog
/// close watcher", getEnabledState).
pub(super) fn watcher_enabled(doc: &Document, dialog: NodeId) -> bool {
    doc.dialog_data(dialog)
        .is_some_and(|d| d.close_watcher_for_request_close)
        || ClosedBy::of(doc, dialog) != ClosedBy::None
}

/// True while the cancel action of the close watcher of `dialog` runs (HTML
/// "is running cancel action"): a request to close returns true then,
/// without closing anything.
pub(super) fn is_running_cancel(doc: &Document, dialog: NodeId) -> bool {
    doc.dialog_data(dialog).is_some_and(|d| d.running_cancel)
}

/// The cancel action of the close watcher of `dialog`: fires `cancel`.
/// Returns whether to go on (`None`: the host stopped). The caller has
/// checked [`is_running_cancel`].
pub(super) fn cancel_action<H: TopLayerHost + ?Sized>(
    host: &mut H,
    dialog: NodeId,
    cancelable: bool,
) -> Option<bool> {
    host.doc().dialog_data_mut(dialog).running_cancel = true;
    let proceed = host.fire_cancel(dialog, cancelable);
    let doc = host.doc();
    if doc.dialog_data(dialog).is_some() {
        doc.dialog_data_mut(dialog).running_cancel = false;
        doc.tidy_dialog(dialog);
    }
    proceed
}

/// The close action of the close watcher of `dialog`: closes it with its
/// request close return value.
pub(super) fn close_action<H: TopLayerHost + ?Sized>(
    host: &mut H,
    dialog: NodeId,
) -> Result<(), PopoverError> {
    let doc = host.doc();
    let result = doc
        .top_layer
        .as_deref_mut()
        .and_then(|s| s.dialogs.get_mut(&dialog))
        .and_then(|d| d.request_close_return_value.take());
    close_dialog(host, dialog, result)
}

/// The most nodes that the dialog focusing steps visit.
const FOCUS_SCAN_LIMIT: usize = 100_000;

/// HTML "dialog focusing steps", as Chromium 148 runs them (measured,
/// `dialog-focus`): the first descendant with `autofocus` in flat tree
/// order that can take the focus, else the first focusable area in flat
/// tree order (also with `tabindex=-1`, also inside a shadow tree), else
/// the dialog itself. The dialog's own `autofocus` attribute is not used.
/// The search visits at most [`FOCUS_SCAN_LIMIT`] nodes.
/// <https://html.spec.whatwg.org/multipage/interactive-elements.html#dialog-focusing-steps>
fn focusing_steps<H: TopLayerHost + ?Sized>(host: &mut H, dialog: NodeId) {
    let doc = host.doc();
    let autofocus = doc.ever_had_autofocus();
    let mut stack: Vec<NodeId> = doc.flat_children(dialog).collect();
    stack.reverse();
    let mut order = Vec::new();
    let mut candidates = Vec::new();
    while let Some(node) = stack.pop() {
        if order.len() >= FOCUS_SCAN_LIMIT {
            break;
        }
        let Some(element) = doc.element(node) else {
            continue;
        };
        if autofocus && element.has_attr("autofocus") {
            candidates.push(node);
        }
        order.push(node);
        let mark = stack.len();
        stack.extend(doc.flat_children(node));
        if let Some(added) = stack.get_mut(mark..) {
            added.reverse();
        }
    }
    for node in candidates.into_iter().chain(order) {
        if host.focus(node) {
            return;
        }
    }
    host.focus_dialog(dialog);
}

/// What a form submission does when the method is `dialog` (HTML "form
/// submission algorithm", the `dialog` method): `None` if the method of
/// `form` with `submitter` is not `dialog`; else the nearest ancestor dialog
/// of the form (`None`: nothing happens) and the result: the coordinate of
/// an image button (`x,y`; `image_coordinate`, 0,0 without one), else the
/// submitter's `value` attribute (none if it has none: the `returnValue`
/// stays), and the empty string without a submitter (Chromium).
/// <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#form-submission-algorithm>
pub fn dialog_form_target(
    doc: &Document,
    form: NodeId,
    submitter: Option<NodeId>,
    image_coordinate: Option<(i32, i32)>,
) -> Option<Option<(NodeId, Option<String>)>> {
    let button = submitter.and_then(|s| doc.element(s));
    let method = button
        .and_then(|e| e.attr("formmethod"))
        .or_else(|| doc.element(form)?.attr("method"))?;
    if !method.eq_ignore_ascii_case("dialog") {
        return None;
    }
    let dialog = doc.ancestors(form).find(|&n| {
        doc.element(n)
            .is_some_and(|e| e.is_html_named(&local_name!("dialog")))
    });
    let Some(dialog) = dialog else {
        return Some(None);
    };
    let image = button.is_some_and(|e| {
        e.is_html_named(&local_name!("input"))
            && e.attr("type")
                .is_some_and(|t| t.eq_ignore_ascii_case("image"))
    });
    let result = if image {
        let (x, y) = image_coordinate.unwrap_or((0, 0));
        Some(format!("{x},{y}"))
    } else if let Some(button) = button {
        button.attr("value").map(str::to_owned)
    } else {
        // Without a submitter (`requestSubmit()`, `submit()`) Chromium sets
        // `returnValue` to the empty string (measured, `dialog-form` #7); the
        // standard's result is null, which keeps it.
        Some(String::new())
    };
    Some(Some((dialog, result)))
}
