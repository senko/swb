//! Close watchers (HTML §6.4 "Close requests and close watchers",
//! <https://html.spec.whatwg.org/multipage/interaction.html#close-requests-and-close-watchers>;
//! ADR 0037 section 6): the internal manager that the key path (Escape)
//! asks to close what is open. Each showing auto or hint popover has a
//! watcher; sub-feature 6d gives the modal dialogs and some non-modal ones
//! theirs ([`WatcherKind::Dialog`]).
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
//!
//! The manager belongs to the document: the engine tells it about the
//! activations that the frame tree records (HTML "activation notification":
//! the frame, its ancestors and the frames of its origin).

use super::{Document, HideOptions, NodeId, PopoverError, TopLayerHost, hide_popover};

/// What a watcher closes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatcherKind {
    /// A showing auto or hint popover: the close action hides it.
    Popover,
    /// A dialog (sub-feature 6d).
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
}

impl Default for CloseWatchers {
    fn default() -> Self {
        CloseWatchers {
            groups: Vec::new(),
            allowed: 1,
            next_allows_new_group: true,
            history_action: false,
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

    /// The watchers of the last group, oldest first.
    fn last_group(&self) -> Vec<(NodeId, WatcherKind)> {
        self.groups.last().cloned().unwrap_or_default()
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

    /// The number of groups and of watchers of the close watcher manager.
    pub fn close_watcher_counts(&self) -> (usize, usize) {
        self.top_layer
            .as_deref()
            .map_or((0, 0), |s| s.watchers.counts())
    }

    /// True if a request to close would find a watcher: the key path asks
    /// before it makes a host.
    pub fn has_close_watchers(&self) -> bool {
        self.close_watcher_counts().0 > 0
    }
}

/// HTML "process close watchers": the key default of Escape (and the
/// platform's back gesture). Requests close of the watchers of the last
/// group, the newest first; a popover hides with events and gives the focus
/// back (the close action of a popover is "hide popover" with focus
/// restoration and events). Returns whether a watcher was processed (the
/// key then counts as handled).
/// <https://html.spec.whatwg.org/multipage/interaction.html#process-close-watchers>
///
/// # Errors
///
/// A stop of the host.
pub fn process_close_watchers<H: TopLayerHost + ?Sized>(
    host: &mut H,
) -> Result<bool, PopoverError> {
    let group = match host.doc().top_layer() {
        Some(state) => state.watchers.last_group(),
        None => return Ok(false),
    };
    if group.is_empty() {
        return Ok(false);
    }
    for &(element, kind) in group.iter().rev() {
        // A listener of an earlier request may have destroyed the watcher.
        let doc = host.doc();
        if !doc.has_close_watcher(element, kind) {
            continue;
        }
        match kind {
            WatcherKind::Popover => {
                let options = HideOptions {
                    focus_previous: true,
                    fire_events: true,
                    throw: false,
                    ignore_dom: true,
                    source: None,
                };
                hide_popover(host, element, options)?;
            }
            // Sub-feature 6d: `cancel`, the close.
            WatcherKind::Dialog => {}
        }
        // The close action ends the watcher whether or not the hide ran.
        host.doc().destroy_close_watcher(element, kind);
    }
    Ok(true)
}
