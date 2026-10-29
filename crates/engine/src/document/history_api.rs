//! What scripts see and ask of the session history (M8 feature 8a1,
//! [`swb_web::EngineHost`], ADR 0028 section 13).
//!
//! The session history belongs to the page. A script task cannot reach it,
//! so the document keeps a copy of what scripts read ([`HistoryInfo`]:
//! the length, `history.state`, the scroll restoration mode) and a list of
//! the changes that scripts made ([`StateChange`]), which the page applies
//! when the task ends (`page/history_api.rs`). A change that scripts must
//! see at once happens in the copy at once: `pushState()` and the
//! navigation to a fragment change the document's URL, `history.length`
//! and `history.state` in the task; the page adds the entry afterwards.
//! Other navigations (`location.href = url`, `history.back()`, `reload()`)
//! wait for the end of the task.

use std::rc::Rc;

use swb_net::Url;
use swb_web::Serialized;

use super::DocumentState;
use super::script_actions::ScriptNavigation;
use crate::history::after_push;

/// The session history as scripts see it.
#[derive(Clone, Debug, Default)]
pub(crate) struct HistoryInfo {
    /// The index of the current entry.
    pub(crate) index: usize,
    /// The number of entries.
    pub(crate) length: usize,
    /// The state of the current entry.
    pub(crate) state: Option<Rc<Serialized>>,
    /// `history.scrollRestoration` is `"manual"`.
    pub(crate) manual_scroll: bool,
}

/// A change of the session history that a script made, in the order it
/// made them.
#[derive(Clone, Debug)]
pub(crate) struct StateChange {
    /// The URL of the new or replaced entry; `None` keeps the current
    /// entry's URL.
    pub(crate) url: Option<Url>,
    /// The state of the entry.
    pub(crate) state: Option<Rc<Serialized>>,
    /// Replace the current entry instead of adding one.
    pub(crate) replace: bool,
    /// For a navigation to a fragment, the URL before it: the page scrolls
    /// to the fragment afterwards.
    pub(crate) fragment_from: Option<Url>,
    /// The scroll position of the viewport before the change: the entry
    /// that the change leaves keeps it.
    pub(crate) scroll: swb_layout::Point,
}

impl DocumentState {
    /// The document's URL changed without a load (`pushState()`, a
    /// navigation to a fragment): the base URL follows unless a `base`
    /// element sets it, and so do the inline style sheets that resolve
    /// against it.
    pub(crate) fn set_url(&mut self, url: Url) {
        self.process_changes();
        self.url = url;
        let generation = self.dom.generation();
        let dom = &self.dom;
        let url = &self.url;
        let old = self.base_url.value().clone();
        self.base_url
            .rebuild(generation, |_| super::derived::base_url(dom, url));
        if *self.base_url.value() != old {
            let base = self.base_url.value().clone();
            for slot in self.sheets.iter_mut() {
                if slot.source == crate::resources::SheetSource::Inline {
                    slot.base_url = base.clone();
                }
            }
        }
    }

    /// `pushState()` and `replaceState()`.
    pub(crate) fn script_push_state(
        &mut self,
        state: Rc<Serialized>,
        url: Option<&str>,
        replace: bool,
    ) {
        let scroll = self.scroll;
        let url = url.and_then(|u| Url::parse(u).ok());
        if let Some(url) = &url {
            self.set_url(url.clone());
        }
        let info = &mut self.history;
        info.state = Some(state.clone());
        if !replace {
            (info.index, info.length) = after_push(info.index);
        }
        self.script_requests.state_changes.push(StateChange {
            url,
            state: Some(state),
            replace,
            fragment_from: None,
            scroll,
        });
    }

    /// `location.href = url`, `assign()` and `replace()`. Returns true for
    /// a navigation to a fragment of this document, which happens at once;
    /// else the page navigates when the task ends.
    pub(crate) fn script_request_navigation(&mut self, url: &str, replace: bool) -> bool {
        let Ok(url) = Url::parse(url) else {
            return false;
        };
        // HTML "navigate": a navigation to the URL of the document is a
        // replacement.
        let identical = url == self.url;
        let replace = replace || identical;
        let same_document =
            url.fragment().is_some() && strip_fragment(&url) == strip_fragment(&self.url);
        if !same_document {
            self.script_requests.navigation = Some(ScriptNavigation::Location { url, replace });
            return false;
        }
        let scroll = self.scroll;
        let from = self.url.clone();
        self.set_url(url.clone());
        let info = &mut self.history;
        // The same URL keeps the state of the entry (Chromium, measured).
        if !identical {
            info.state = None;
        }
        let state = info.state.clone();
        if !replace {
            (info.index, info.length) = after_push(info.index);
        }
        self.script_requests.state_changes.push(StateChange {
            url: Some(url),
            state,
            replace,
            fragment_from: Some(from),
            scroll,
        });
        true
    }
}

fn strip_fragment(url: &Url) -> Url {
    let mut url = url.clone();
    url.set_fragment(None);
    url
}
