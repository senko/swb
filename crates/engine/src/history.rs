//! Session history: the committed entries (one per visited document or
//! fragment) and the current position. A navigation adds its entry only
//! when its document arrives (see `Page`), so navigations that never
//! complete leave no entry. An entry keeps what a reload or a traversal
//! requests again: the URL, the body of a `POST` (ADR 0013) and the origin
//! of the document that started the navigation (ADR 0012). Each entry
//! knows the document it was committed with: entries of one document (it
//! and its fragments) can be traversed without loading.

use std::sync::Arc;

use swb_layout::Point;
use swb_net::{Origin, Url};

/// The body of a `POST` request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PostData {
    pub(crate) body: Arc<[u8]>,
    pub(crate) content_type: String,
}

/// A committed document: what a new or replaced history entry records.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Commit {
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
    pub(crate) url: Url,
    /// The scroll position when the entry was last left; restored when the
    /// user returns to it.
    pub(crate) scroll: Point,
    /// The body of the `POST` request that loaded the document, if any.
    pub(crate) post: Option<PostData>,
    /// The origin of the document that started the navigation.
    pub(crate) initiator: Option<Origin>,
    /// The number of the document (see `Page`) that the entry shows.
    pub(crate) document: u64,
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
                .is_some_and(|e| e.url == commit.url && e.post.is_none())
        {
            self.replace_current(commit);
            return;
        }
        self.push_new(commit);
    }

    /// Adds an entry after the current one and makes it current, also for
    /// the URL of the current entry; entries after the current one are
    /// dropped.
    pub(crate) fn push_new(&mut self, commit: Commit) {
        if !self.entries.is_empty() {
            self.entries.truncate(self.index + 1);
        }
        self.entries.push(Entry {
            url: commit.url,
            scroll: Point::default(),
            post: commit.post,
            initiator: commit.initiator,
            document: commit.document,
        });
        self.index = self.entries.len() - 1;
    }

    /// Replaces the URL, the `POST` body, the initiator and the document of
    /// the current entry (after a redirect or a reload); keeps its scroll
    /// position. Adds the first entry if there is none.
    pub(crate) fn replace_current(&mut self, commit: Commit) {
        match self.entries.get_mut(self.index) {
            Some(entry) => {
                entry.url = commit.url;
                entry.post = commit.post;
                entry.initiator = commit.initiator;
                entry.document = commit.document;
            }
            None => self.push(commit),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    fn current(h: &History) -> &str {
        h.current().unwrap().url.as_str()
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
        assert_eq!(h.current().unwrap().post, Some(post));
        // A GET of the same URL after a POST result is a new entry too.
        h.push(Commit::get(u("http://a/"), 1));
        assert_eq!(h.len(), 3);
    }

    #[test]
    fn go_to_ignores_invalid_indices() {
        let mut h = History::default();
        h.push(Commit::get(u("http://a/"), 1));
        h.go_to(5);
        assert_eq!(h.index(), 0);
    }
}
