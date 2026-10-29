//! Session history: the committed entries (one per visited document or
//! fragment) and the current position. A navigation adds its entry only
//! when its document arrives (see `Page`), so navigations that never
//! complete leave no entry. An entry keeps what a reload or a traversal
//! requests again: the URL, the body of a `POST` (ADR 0013) and the origin
//! of the document that started the navigation (ADR 0012). Each entry
//! knows the document it was committed with: entries of one document (it
//! and its fragments) can be traversed without loading.

use std::rc::Rc;
use std::sync::Arc;

use swb_layout::Point;
use swb_net::{Origin, Url};
use swb_web::Serialized;

/// The most entries that the session history holds: the oldest entry goes
/// when a new one would be the 51st (Chromium's cap, measured through
/// `history.length` in `tools/probes/web-location-history.json`).
pub(crate) const MAX_ENTRIES: usize = 50;

/// The index of the current entry and the number of entries after a new
/// entry follows the current one: the entries after the current one go,
/// and the oldest goes if the cap is reached.
pub(crate) fn after_push(index: usize) -> (usize, usize) {
    let length = index + 2;
    if length > MAX_ENTRIES {
        (MAX_ENTRIES - 1, MAX_ENTRIES)
    } else {
        (index + 1, length)
    }
}

/// The body of a `POST` request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PostData {
    pub(crate) body: Arc<[u8]>,
    pub(crate) content_type: String,
}

/// A committed document: what a new or replaced history entry records.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Commit {
    /// The URL of the document, with the fragment of the entry.
    pub(crate) url: Url,
    /// The body of the `POST` request that loaded the document, if any.
    pub(crate) post: Option<PostData>,
    /// The origin of the document that started the navigation (a link or
    /// a form); `None` if the user started it.
    pub(crate) initiator: Option<Origin>,
    /// The number of the document (see `Page`) that the entry shows.
    pub(crate) document: u64,
}

impl Commit {
    /// A commit of `url` without a body and an initiator.
    #[cfg(test)]
    fn get(url: Url, document: u64) -> Commit {
        Commit {
            url,
            post: None,
            initiator: None,
            document,
        }
    }
}

/// One history entry.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Entry {
    /// What the entry shows and requests again.
    pub(crate) commit: Commit,
    /// The scroll position when the entry was last left; restored when the
    /// user returns to it.
    pub(crate) scroll: Point,
    /// The classic history API state (`history.state`), set by
    /// `pushState()` and `replaceState()`; `None` is `null`.
    pub(crate) state: Option<Rc<Serialized>>,
    /// `history.scrollRestoration` is `"manual"` for this entry.
    pub(crate) manual_scroll: bool,
}

/// Session history of a page.
#[derive(Debug, Default)]
pub(crate) struct History {
    entries: Vec<Entry>,
    index: usize,
}

impl History {
    /// Adds an entry after the current one and makes it current; entries
    /// after the current one are dropped. A `GET` navigation to the URL of
    /// the current entry (also not a `POST` result) replaces the current
    /// entry instead. Deviation: the HTML standard ("navigate") turns every
    /// navigation to the URL of the current document into a replacement;
    /// swb follows Chromium (measured), which keeps a `POST` navigation as
    /// a new entry (see [`History::push_new`]).
    pub(crate) fn push(&mut self, commit: Commit) {
        if commit.post.is_none()
            && self
                .current()
                .is_some_and(|e| e.commit.url == commit.url && e.commit.post.is_none())
        {
            self.replace_document(commit);
            return;
        }
        self.push_new(commit);
    }

    /// Adds an entry after the current one and makes it current, also for
    /// the URL of the current entry; entries after the current one are
    /// dropped.
    pub(crate) fn push_new(&mut self, commit: Commit) {
        let manual_scroll = self.current().is_some_and(|e| e.manual_scroll);
        self.push_entry(commit, None, manual_scroll);
    }

    /// Adds an entry with `state` after the current one and makes it
    /// current (`pushState()`, a navigation to a fragment); the entry keeps
    /// the scroll restoration mode of the current one.
    pub(crate) fn push_with_state(&mut self, commit: Commit, state: Option<Rc<Serialized>>) {
        let manual_scroll = self.current().is_some_and(|e| e.manual_scroll);
        self.push_entry(commit, state, manual_scroll);
    }

    fn push_entry(&mut self, commit: Commit, state: Option<Rc<Serialized>>, manual_scroll: bool) {
        if !self.entries.is_empty() {
            self.entries.truncate(self.index + 1);
        }
        self.entries.push(Entry {
            commit,
            scroll: Point::default(),
            state,
            manual_scroll,
        });
        if self.entries.len() > MAX_ENTRIES {
            self.entries.remove(0);
        }
        self.index = self.entries.len() - 1;
    }

    /// Replaces the current entry's commit and state (`replaceState()`);
    /// keeps its scroll position.
    pub(crate) fn replace_with_state(&mut self, commit: Commit, state: Option<Rc<Serialized>>) {
        match self.entries.get_mut(self.index) {
            Some(entry) => {
                entry.commit = commit;
                entry.state = state;
            }
            None => self.push_with_state(commit, state),
        }
    }

    /// Replaces the commit (URL, `POST` body, initiator and document) of
    /// the current entry (after a redirect or a reload); keeps its scroll
    /// position. Adds the first entry if there is none. The state and the
    /// scroll restoration mode were set by documents of the entry's
    /// origin: a commit of another origin (a reload or a traversal that
    /// the server redirected) clears them, so the new document never
    /// reads them.
    pub(crate) fn replace_current(&mut self, commit: Commit) {
        match self.entries.get_mut(self.index) {
            Some(entry) => {
                if !same_origin(&entry.commit.url, &commit.url) {
                    entry.state = None;
                    entry.manual_scroll = false;
                }
                entry.commit = commit;
            }
            None => self.push(commit),
        }
    }

    /// Replaces the current entry with a new document (`location.replace()`,
    /// a navigation to the URL of the current entry): like
    /// [`History::replace_current`], but the entry's state goes with the old
    /// document, so a new (maybe cross-origin) document never reads it.
    pub(crate) fn replace_document(&mut self, commit: Commit) {
        self.replace_current(commit);
        if let Some(entry) = self.current_mut() {
            entry.state = None;
        }
    }

    /// The current entry.
    pub(crate) fn current(&self) -> Option<&Entry> {
        self.entries.get(self.index)
    }

    /// The current entry, for updating its scroll position.
    pub(crate) fn current_mut(&mut self) -> Option<&mut Entry> {
        self.entries.get_mut(self.index)
    }

    /// The entry at `index`.
    pub(crate) fn get(&self, index: usize) -> Option<&Entry> {
        self.entries.get(index)
    }

    /// The index of the current entry.
    pub(crate) fn index(&self) -> usize {
        self.index
    }

    /// The number of entries.
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Makes the entry at `index` current. Does nothing if there is no
    /// such entry.
    pub(crate) fn go_to(&mut self, index: usize) {
        if index < self.entries.len() {
            self.index = index;
        }
    }
}

/// True if `a` and `b` have the same origin
/// (<https://html.spec.whatwg.org/multipage/browsers.html#same-origin>).
/// Each opaque origin (`file:`, `data:`) is new in the `url` crate, so two
/// URLs with opaque origins count as the same only if they are equal
/// without the fragment (a reload or a traversal of the same entry).
fn same_origin(a: &Url, b: &Url) -> bool {
    match (a.origin(), b.origin()) {
        (a_origin @ Origin::Tuple(..), b_origin @ Origin::Tuple(..)) => a_origin == b_origin,
        _ => without_fragment(a) == without_fragment(b),
    }
}

/// The serialization of `url` without its fragment (a serialized URL has
/// `#` only before the fragment).
fn without_fragment(url: &Url) -> &str {
    url.as_str()
        .split_once('#')
        .map_or(url.as_str(), |(head, _)| head)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    fn current(h: &History) -> &str {
        h.current().unwrap().commit.url.as_str()
    }

    #[test]
    fn push_truncates_forward_entries() {
        let mut h = History::default();
        h.push(Commit::get(u("http://a/"), 1));
        h.push(Commit::get(u("http://b/"), 2));
        h.push(Commit::get(u("http://c/"), 3));
        h.go_to(1);
        assert_eq!(current(&h), "http://b/");
        h.push(Commit::get(u("http://d/"), 4));
        assert_eq!(h.len(), 3);
        assert_eq!(h.index(), 2);
        assert_eq!(current(&h), "http://d/");
        h.replace_current(Commit::get(u("http://e/"), 5));
        assert_eq!(current(&h), "http://e/");
    }

    #[test]
    fn same_url_replaces() {
        let mut h = History::default();
        h.push(Commit::get(u("http://a/"), 1));
        h.current_mut().unwrap().scroll = Point::new(0.0, 50.0);
        h.push(Commit::get(u("http://a/"), 1));
        assert_eq!(h.len(), 1);
        assert_eq!(h.current().unwrap().scroll, Point::new(0.0, 50.0));
        h.push_new(Commit::get(u("http://a/"), 2));
        assert_eq!(h.len(), 2);
        assert_eq!(h.current().unwrap().scroll, Point::default());
    }

    #[test]
    fn post_results_get_their_own_entries() {
        let post = PostData {
            body: Arc::from(&b"a=1"[..]),
            content_type: "application/x-www-form-urlencoded".to_owned(),
        };
        let mut h = History::default();
        h.push(Commit::get(u("http://a/"), 1));
        h.push(Commit {
            post: Some(post.clone()),
            ..Commit::get(u("http://a/"), 2)
        });
        assert_eq!(h.len(), 2);
        assert_eq!(h.current().unwrap().commit.post, Some(post));
        // A GET of the same URL after a POST result is a new entry too.
        h.push(Commit::get(u("http://a/"), 1));
        assert_eq!(h.len(), 3);
    }

    #[test]
    fn the_history_keeps_fifty_entries() {
        let mut h = History::default();
        for i in 0..60 {
            h.push(Commit::get(u(&format!("http://a/{i}")), i));
        }
        assert_eq!(h.len(), MAX_ENTRIES);
        assert_eq!(h.index(), MAX_ENTRIES - 1);
        assert_eq!(current(&h), "http://a/59");
        assert_eq!(h.get(0).unwrap().commit.url.as_str(), "http://a/10");
        assert_eq!(after_push(10), (11, 12));
        assert_eq!(after_push(49), (49, 50));
    }

    #[test]
    fn states_stay_with_their_entries() {
        let mut h = History::default();
        h.push(Commit::get(u("http://a/"), 1));
        let state = Rc::new(Serialized::null());
        h.push_with_state(Commit::get(u("http://a/x"), 1), Some(state.clone()));
        assert!(h.current().unwrap().state.is_some());
        h.replace_current(Commit::get(u("http://a/y"), 1));
        assert!(h.current().unwrap().state.is_some());
        h.replace_with_state(Commit::get(u("http://a/z"), 1), None);
        assert!(h.current().unwrap().state.is_none());
        h.go_to(0);
        assert!(h.current().unwrap().state.is_none());
    }

    #[test]
    fn a_replaced_document_loses_the_state_and_a_reload_keeps_it() {
        let mut h = History::default();
        h.push(Commit::get(u("http://a/"), 1));
        h.replace_with_state(
            Commit::get(u("http://a/"), 1),
            Some(Rc::new(Serialized::null())),
        );
        // A reload replaces the commit only.
        h.replace_current(Commit::get(u("http://a/"), 2));
        assert!(h.current().unwrap().state.is_some());
        // A new document (`location.replace()`) replaces the state too.
        h.replace_document(Commit::get(u("http://b/"), 3));
        assert!(h.current().unwrap().state.is_none());
        assert_eq!(current(&h), "http://b/");
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn a_reload_redirected_to_another_origin_loses_the_state() {
        let mut h = History::default();
        h.push(Commit::get(u("http://a/"), 1));
        h.replace_with_state(
            Commit::get(u("http://a/"), 1),
            Some(Rc::new(Serialized::null())),
        );
        h.current_mut().unwrap().manual_scroll = true;
        // Same origin, another path (a redirect within the site).
        h.replace_current(Commit::get(u("http://a/other"), 2));
        assert!(h.current().unwrap().state.is_some());
        assert!(h.current().unwrap().manual_scroll);
        // Another origin: the state and the scroll mode go.
        h.replace_current(Commit::get(u("http://b/"), 3));
        assert!(h.current().unwrap().state.is_none());
        assert!(!h.current().unwrap().manual_scroll);
    }

    #[test]
    fn a_reload_of_a_file_keeps_the_state() {
        let mut h = History::default();
        h.push(Commit::get(u("file:///tmp/a.html"), 1));
        h.replace_with_state(
            Commit::get(u("file:///tmp/a.html#x"), 1),
            Some(Rc::new(Serialized::null())),
        );
        h.replace_current(Commit::get(u("file:///tmp/a.html#x"), 2));
        assert!(h.current().unwrap().state.is_some());
        h.replace_current(Commit::get(u("file:///tmp/b.html"), 3));
        assert!(h.current().unwrap().state.is_none());
    }

    #[test]
    fn go_to_ignores_invalid_indices() {
        let mut h = History::default();
        h.push(Commit::get(u("http://a/"), 1));
        h.go_to(5);
        assert_eq!(h.index(), 0);
    }
}
