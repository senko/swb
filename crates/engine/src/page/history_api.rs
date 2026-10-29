//! The session history and `location` for scripts (M8 feature 8a1, ADR
//! 0028 section 13): the page applies the history changes that a script
//! task made, keeps the document's copy of the history current, and fires
//! the events of navigations within the document (`popstate`,
//! `hashchange`).

use std::rc::Rc;

use swb_net::Url;
use swb_web::Serialized;

use super::{HistoryHandling, Page, is_loadable};
use crate::document::{HistoryInfo, StateChange};
use crate::history::Commit;

impl Page {
    /// What scripts read of the session history, from the current entry.
    pub(super) fn history_info(&self) -> HistoryInfo {
        HistoryInfo {
            index: self.history.index(),
            length: self.history.len(),
            state: self.history.current().and_then(|e| e.state.clone()),
            manual_scroll: self.history.current().is_some_and(|e| e.manual_scroll),
        }
    }

    /// Copies the session history to the document for scripts to read.
    pub(super) fn sync_history_info(&mut self) {
        let info = self.history_info();
        if let Some(doc) = self.doc.as_deref_mut() {
            doc.history = info;
        }
    }

    /// Applies the history changes of a script task, in order
    /// (`pushState()`, `replaceState()` and navigations to a fragment; the
    /// document's URL changed in the task already), and the scroll
    /// restoration mode that the script set.
    pub(super) fn apply_state_changes(&mut self, changes: Vec<StateChange>) {
        let manual = self.doc.as_deref().map(|d| d.history.manual_scroll);
        if let (Some(manual), Some(entry)) = (manual, self.history.current_mut()) {
            entry.manual_scroll = manual;
        }
        for change in changes {
            self.apply_state_change(change);
        }
        self.sync_history_info();
    }

    fn apply_state_change(&mut self, change: StateChange) {
        let Some(current) = self.history.current().map(|e| e.commit.clone()) else {
            return;
        };
        let url = change.url.clone().unwrap_or(current.url);
        let commit = Commit {
            url: url.clone(),
            post: if change.replace { current.post } else { None },
            initiator: current.initiator,
            document: self.document_number,
        };
        if change.replace {
            self.history.replace_with_state(commit, change.state);
        } else {
            // The entry that the push leaves keeps the scroll position of
            // the time of the call.
            if let Some(entry) = self.history.current_mut() {
                entry.scroll = change.scroll;
            }
            self.history.push_with_state(commit, change.state);
        }
        self.url = Some(url.clone());
        if change.fragment_from.is_some() {
            let fragment = url.fragment().unwrap_or_default().to_owned();
            let target = self.set_target(Some(&fragment));
            self.scroll_to_fragment(&fragment);
            self.focus_fragment_target(target);
        }
    }

    /// `location.href = url`, `assign()` and `replace()` to another
    /// document: a navigation that the document starts.
    pub(super) fn location_navigate(&mut self, url: Url, replace: bool) {
        if !is_loadable(&url) {
            log::info!("ignoring a location change to the scheme {}:", url.scheme());
            return;
        }
        if !self.may_load_subresource(&url) {
            log::warn!("not allowed to load local resource {url}");
            return;
        }
        // A navigation to the URL of the document is a reload that keeps
        // the entry's state (Chromium, measured: `location.href =
        // location.href`, `port = port`); one to another URL that replaces
        // the entry (`replace()`, a navigation before the `load` event
        // finished) gives the new document an entry without state.
        let handling = if self.doc.as_ref().is_some_and(|d| *d.url() == url) {
            HistoryHandling::Reload
        } else if replace {
            HistoryHandling::Replace
        } else {
            HistoryHandling::Push
        };
        let initiator = self.document_origin();
        self.start_navigation(url, None, handling, None, initiator);
    }

    /// Fires `popstate` with `state` and `hashchange` for a navigation
    /// within the document, if the document has scripts.
    pub(super) fn same_document_events(
        &mut self,
        state: Option<Rc<Serialized>>,
        hash_change: Option<(String, String)>,
    ) {
        if self.script.is_some() {
            self.run_task(|script| script.same_document_navigation(state, hash_change));
        }
    }
}
