//! Close watchers (HTML §6.4 "Close requests and close watchers",
//! <https://html.spec.whatwg.org/multipage/interaction.html#close-requests-and-close-watchers>;
//! ADR 0037 section 6): the internal manager that the key path (Escape)
//! asks to close what is open. Each showing auto or hint popover has a
//! watcher, and each dialog that `show()` or `showModal()` opened
//! ([`WatcherKind::Dialog`]; the `closedby` state enables it, `dialog.rs`).
//!
//! The manager keeps the watchers in groups. A watcher made while the
//! number of groups is below the allowed number gets a group of its own,
//! else it joins the last group; a request closes the last group, top
//! first. The allowed number grows by the user's activations. The rule is
//! the standard's ("create a close watcher": the allowed number of groups
//! and "next user interaction allows a new group"), checked against
//! Chromium 148 (cases `escape-popovers-*`, `xo-*`, ADR 0037 "6c
//! implementation notes"):
//!
//! - The allowed number starts at 1 and the flag at true.
//! - An activation (a key other than Escape, a press) with the flag true
//!   adds 1 to the allowed number and clears the flag.
//! - Making a watcher sets the flag (also when it joins a group).
//! - Each close request (Escape that no listener canceled) takes 1 from the
//!   allowed number, not below 1, if the manager has a group (sub-feature 6d;
//!   Chromium keeps the number at an Escape with nothing open).
//! - A dialog's `cancel` can prevent the close if the number of groups is
//!   below the allowed number and the window has a history-action
//!   activation; a prevented close consumes that activation.
//!
//! The manager belongs to the document: the engine tells it about the
//! activations that the frame tree records (HTML "activation notification":
//! the frame, its ancestors and the frames of its origin).

use std::collections::HashMap;

use super::{Document, HideOptions, NodeId, PopoverError, TopLayerHost, hide_popover};

/// What a watcher closes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WatcherKind {
    /// A showing auto or hint popover: the close action hides it.
    Popover,
    /// A dialog that `show()` or `showModal()` opened: the cancel action
    /// fires `cancel`, the close action closes it.
    Dialog,
}

/// The close watchers of a document.
#[derive(Debug)]
pub(super) struct CloseWatchers {
    /// The groups, oldest first; the watchers of a group in creation order.
    groups: Vec<Vec<(NodeId, WatcherKind)>>,
    /// HTML "allowed number of groups".
    allowed: usize,
    /// HTML "next user interaction allows a new group".
    next_allows_new_group: bool,
    /// HTML "history-action activation": set by each activation, cleared by
    /// a request that a `cancel` listener canceled (sub-feature 6d).
    history_action: bool,
    /// The serial of each live watcher: tells a watcher from a later one of
    /// the same element (a listener that closes and shows a dialog again).
    serials: HashMap<(NodeId, WatcherKind), u64>,
    /// The serial of the next watcher.
    next_serial: u64,
}

impl Default for CloseWatchers {
    fn default() -> Self {
        CloseWatchers {
            groups: Vec::new(),
            allowed: 1,
            next_allows_new_group: true,
            history_action: false,
            serials: HashMap::new(),
            next_serial: 0,
        }
    }
}

impl CloseWatchers {
    /// HTML "activation notification" for the window of the document.
    pub(super) fn note_activation(&mut self) {
        self.history_action = true;
        if self.next_allows_new_group {
            self.allowed = self.allowed.saturating_add(1);
            self.next_allows_new_group = false;
        }
    }

    /// HTML "create a close watcher" for `element`.
    pub(super) fn create(&mut self, element: NodeId, kind: WatcherKind) {
        let watcher = (element, kind);
        let new_group = self.groups.len() < self.allowed;
        match self.groups.last_mut() {
            Some(last) if !new_group => last.push(watcher),
            _ => self.groups.push(vec![watcher]),
        }
        self.next_serial += 1;
        self.serials.insert(watcher, self.next_serial);
        self.next_allows_new_group = true;
    }

    /// HTML "destroy" a close watcher: takes it out of its group and drops
    /// an empty group. The watcher is at or near the end in the common
    /// orders (the top closes first), so the search starts there.
    pub(super) fn destroy(&mut self, element: NodeId, kind: WatcherKind) {
        let watcher = (element, kind);
        for index in (0..self.groups.len()).rev() {
            let Some(group) = self.groups.get_mut(index) else {
                continue;
            };
            if let Some(at) = group.iter().rposition(|&w| w == watcher) {
                group.remove(at);
                self.serials.remove(&watcher);
                if group.is_empty() {
                    self.groups.remove(index);
                }
                return;
            }
        }
    }

    /// True if `element` has a watcher of `kind`.
    pub(super) fn contains(&self, element: NodeId, kind: WatcherKind) -> bool {
        self.groups
            .iter()
            .rev()
            .any(|g| g.iter().rev().any(|&w| w == (element, kind)))
    }

    /// The serial of the watcher of `element`, if it has one.
    fn serial(&self, element: NodeId, kind: WatcherKind) -> Option<u64> {
        self.serials.get(&(element, kind)).copied()
    }

    /// The watchers of the last group, oldest first.
    fn last_group(&self) -> Vec<(NodeId, WatcherKind)> {
        self.groups.last().cloned().unwrap_or_default()
    }

    /// HTML "request to close", step 6: a `cancel` can prevent the close if
    /// fewer groups exist than are allowed and the window has a
    /// history-action activation.
    fn can_prevent_close(&self) -> bool {
        self.groups.len() < self.allowed && self.history_action
    }

    /// The number of groups and the number of watchers.
    pub(super) fn counts(&self) -> (usize, usize) {
        (self.groups.len(), self.groups.iter().map(Vec::len).sum())
    }
}

impl Document {
    /// HTML "activation notification" for this document's window: the user
    /// pressed a button or a key in it or in a frame of its tree (not
    /// Escape). The manager may allow a new group of close watchers.
    pub fn note_user_activation(&mut self) {
        match self.top_layer.as_deref_mut() {
            Some(state) => state.watchers.note_activation(),
            // Nothing shows yet: the manager that the first popover makes
            // starts after this activation.
            None => self.early_activation = true,
        }
    }

    /// HTML "history-action activation" of the window (6d reads it for the
    /// `cancel` rule): true after an activation, until a canceled `cancel`
    /// consumes it.
    pub fn has_history_action_activation(&self) -> bool {
        self.top_layer
            .as_deref()
            .map_or(self.early_activation, |s| s.watchers.history_action)
    }

    /// Consumes the history-action activation (HTML "consume history-action
    /// user activation").
    pub fn consume_history_action_activation(&mut self) {
        self.early_activation = false;
        if let Some(state) = self.top_layer.as_deref_mut() {
            state.watchers.history_action = false;
        }
    }

    /// Makes a close watcher of `kind` for `element` (HTML "create a close
    /// watcher"). The popover algorithms make the watchers of popovers;
    /// sub-feature 6d makes those of dialogs.
    pub fn create_close_watcher(&mut self, element: NodeId, kind: WatcherKind) {
        self.top_layer_state().watchers.create(element, kind);
    }

    /// Destroys the close watcher of `kind` of `element`, if it has one.
    pub fn destroy_close_watcher(&mut self, element: NodeId, kind: WatcherKind) {
        if let Some(state) = self.top_layer.as_deref_mut() {
            state.watchers.destroy(element, kind);
        }
    }

    /// True if `element` has a close watcher of `kind`.
    pub fn has_close_watcher(&self, element: NodeId, kind: WatcherKind) -> bool {
        self.top_layer
            .as_deref()
            .is_some_and(|s| s.watchers.contains(element, kind))
    }

    /// The serial of the close watcher of `element`: differs between two
    /// watchers of the same element.
    pub(super) fn close_watcher_serial(&self, element: NodeId, kind: WatcherKind) -> Option<u64> {
        self.top_layer
            .as_deref()
            .and_then(|s| s.watchers.serial(element, kind))
    }

    /// The number of groups and of watchers of the close watcher manager.
    pub fn close_watcher_counts(&self) -> (usize, usize) {
        self.top_layer
            .as_deref()
            .map_or((0, 0), |s| s.watchers.counts())
    }

    /// True if a close request (Escape) can change something: the document
    /// has a close watcher manager, or an activation raised the allowed
    /// number of groups of the manager to come. The key path asks before it
    /// makes a host.
    pub fn close_requests_matter(&self) -> bool {
        self.top_layer.is_some() || self.early_activation
    }

    /// True if a request to close would find a watcher.
    pub fn has_close_watchers(&self) -> bool {
        self.close_watcher_counts().0 > 0
    }
}

/// HTML "process close watchers": the key default of Escape (and the
/// platform's back gesture). Requests close of the watchers of the last
/// group, the newest first, until a `cancel` listener prevents a close; a
/// popover hides with events and gives the focus back, a dialog fires
/// `cancel` and closes. Then the allowed number of groups goes down by one
/// (not below one; step 3 of the standard, which `xo-escape-cancel-click-
/// between` confirms), but only if the manager had a group: Chromium keeps
/// the number at an Escape with nothing open (ADR 0037 "6d implementation
/// notes"). Returns whether
/// an enabled watcher was processed (the key then counts as handled).
/// <https://html.spec.whatwg.org/multipage/interaction.html#process-close-watchers>
///
/// # Errors
///
/// A stop of the host.
pub fn process_close_watchers<H: TopLayerHost + ?Sized>(
    host: &mut H,
) -> Result<bool, PopoverError> {
    let doc = host.doc();
    if doc.top_layer.is_none() && !doc.early_activation {
        // The allowed number of groups is 1: nothing changes.
        return Ok(false);
    }
    let group = doc.top_layer_state().watchers.last_group();
    if group.is_empty() {
        // Chromium lowers nothing without a group (measured, `esc0`); the
        // standard's step 3 lowers the number also then (ADR 0037, 6d).
        return Ok(false);
    }
    let mut processed = false;
    for &(element, kind) in group.iter().rev() {
        // A listener of an earlier request may have destroyed the watcher.
        let doc = host.doc();
        if !doc.has_close_watcher(element, kind) {
            continue;
        }
        if enabled(doc, element, kind) {
            processed = true;
        }
        if !request_close(host, element, kind, true)? {
            break;
        }
    }
    if let Some(state) = host.doc().top_layer.as_deref_mut() {
        let watchers = &mut state.watchers;
        if watchers.allowed > 1 {
            watchers.allowed -= 1;
        }
    }
    Ok(processed)
}

/// The enabled state of the watcher of `element`: a popover's is always on,
/// a dialog's depends on its `closedby` attribute and `requestClose()`.
fn enabled(doc: &Document, element: NodeId, kind: WatcherKind) -> bool {
    match kind {
        WatcherKind::Popover => true,
        WatcherKind::Dialog => super::dialog::watcher_enabled(doc, element),
    }
}

/// HTML "request to close" the watcher of `element`: the cancel action (a
/// dialog's `cancel`, which a listener can cancel if
/// `require_history_action` is false, or if the manager allows it), then the
/// close action, which ends the watcher. Returns false if a listener
/// prevented the close (the history-action activation is consumed then). A
/// dialog without a watcher (opened by its attribute) runs the same steps
/// for `requestClose()`.
/// <https://html.spec.whatwg.org/multipage/interaction.html#close-watcher-request-close>
///
/// # Errors
///
/// A stop of the host.
pub(super) fn request_close<H: TopLayerHost + ?Sized>(
    host: &mut H,
    element: NodeId,
    kind: WatcherKind,
    require_history_action: bool,
) -> Result<bool, PopoverError> {
    let doc = host.doc();
    if !enabled(doc, element, kind) {
        return Ok(true);
    }
    match kind {
        WatcherKind::Popover => {
            // A popover has no cancel action; its close action hides it.
            let options = HideOptions {
                focus_previous: true,
                fire_events: true,
                throw: false,
                ignore_dom: true,
                source: None,
            };
            hide_popover(host, element, options)?;
        }
        WatcherKind::Dialog => {
            if doc.node_document(element) != NodeId::DOCUMENT
                || super::dialog::is_running_cancel(doc, element)
            {
                // A request inside the cancel action of the same watcher
                // returns at once, without a close (HTML "is running cancel
                // action").
                return Ok(true);
            }
            let serial = doc.close_watcher_serial(element, kind);
            let can_prevent = !require_history_action
                || doc
                    .top_layer()
                    .is_some_and(|s| s.watchers.can_prevent_close());
            let proceed = super::dialog::cancel_action(host, element, can_prevent)
                .ok_or_else(PopoverError::stopped)?;
            if !proceed && can_prevent {
                host.doc().consume_history_action_activation();
                return Ok(false);
            }
            // The close action of a watcher that a listener destroyed or
            // replaced does nothing: the request is over.
            if host.doc().close_watcher_serial(element, kind) != serial {
                return Ok(true);
            }
            super::dialog::close_action(host, element)?;
        }
    }
    // The close action ends the watcher whether or not it closed anything.
    host.doc().destroy_close_watcher(element, kind);
    Ok(true)
}
