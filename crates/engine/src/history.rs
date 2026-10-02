//! Session history: the committed entries (one per visited document or
//! fragment) and the current position. A navigation adds its entry only
//! when its document arrives (see `Page`), so navigations that never
//! complete leave no entry.

use swb_layout::Point;
use swb_net::Url;

/// One history entry.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Entry {
    pub(crate) url: Url,
    /// The scroll position when the entry was last left; restored when the
    /// user returns to it.
    pub(crate) scroll: Point,
}

/// Session history of a page.
#[derive(Debug, Default)]
pub(crate) struct History {
    entries: Vec<Entry>,
    index: usize,
}

impl History {
    /// Adds an entry after the current one and makes it current; entries
    /// after the current one are dropped. A URL equal to the current
    /// entry's URL replaces the current entry instead, as the HTML standard
    /// requires for navigations to the same URL.
    pub(crate) fn push(&mut self, url: Url) {
        if self.current().is_some_and(|e| e.url == url) {
            self.replace_current(url);
            return;
        }
        if !self.entries.is_empty() {
            self.entries.truncate(self.index + 1);
        }
        self.entries.push(Entry {
            url,
            scroll: Point::default(),
        });
        self.index = self.entries.len() - 1;
    }

    /// Replaces the URL of the current entry (for example after a
    /// redirect), or adds the first entry.
    pub(crate) fn replace_current(&mut self, url: Url) {
        match self.entries.get_mut(self.index) {
            Some(entry) => entry.url = url,
            None => self.push(url),
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
        h.push(u("http://a/"));
        h.push(u("http://b/"));
        h.push(u("http://c/"));
        h.go_to(1);
        assert_eq!(current(&h), "http://b/");
        h.push(u("http://d/"));
        assert_eq!(h.len(), 3);
        assert_eq!(h.index(), 2);
        assert_eq!(current(&h), "http://d/");
        h.replace_current(u("http://e/"));
        assert_eq!(current(&h), "http://e/");
    }

    #[test]
    fn same_url_replaces() {
        let mut h = History::default();
        h.push(u("http://a/"));
        h.current_mut().unwrap().scroll = Point::new(0.0, 50.0);
        h.push(u("http://a/"));
        assert_eq!(h.len(), 1);
        assert_eq!(h.current().unwrap().scroll, Point::new(0.0, 50.0));
    }

    #[test]
    fn go_to_ignores_invalid_indices() {
        let mut h = History::default();
        h.push(u("http://a/"));
        h.go_to(5);
        assert_eq!(h.index(), 0);
    }
}
