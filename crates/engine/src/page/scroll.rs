//! Scrolling, and the element that a URL fragment indicates: the
//! scroll target of a navigation and the `:target` element.

use std::collections::HashMap;

use swb_dom::{Document, NodeId, local_name};
use swb_layout::{FragmentRef, FragmentTree, Point, Size};

use super::{Page, ScrollTarget};

impl Page {
    /// The size of the scrollable content in CSS px.
    pub fn content_size(&mut self) -> Size {
        self.update_layout();
        self.fragments
            .as_ref()
            .map_or(self.viewport, |f| f.scroll_size)
    }

    /// Scrolls by a delta in CSS px.
    pub fn scroll_by(&mut self, dx: f32, dy: f32) {
        self.scroll_to(Point::new(self.scroll.x + dx, self.scroll.y + dy));
    }

    /// Scrolls to a position in CSS px, clamped to the content. Replaces a
    /// scroll to the fragment that waits for the first layout.
    pub fn scroll_to(&mut self, p: Point) {
        self.update_layout();
        self.pending_scroll = None;
        let before = self.scroll;
        self.scroll = p;
        self.clamp_scroll();
        if self.scroll != before {
            // Other content is under the pointer now.
            self.update_hover(false);
        }
    }

    /// Clamps the scroll position to the content. Without a layout the
    /// content size is not known; the position is then clamped after the
    /// next layout.
    pub(super) fn clamp_scroll(&mut self) {
        let max = swb_style::Length::MAX_PX;
        self.scroll.x = self.scroll.x.clamp(0.0, max);
        self.scroll.y = self.scroll.y.clamp(0.0, max);
        let Some(fragments) = &self.fragments else {
            return;
        };
        let size = fragments.scroll_size;
        let max_x = (size.width - self.viewport.width).max(0.0);
        let max_y = (size.height - self.viewport.height).max(0.0);
        self.scroll.x = self.scroll.x.min(max_x);
        self.scroll.y = self.scroll.y.min(max_y);
    }

    /// Scrolls to the element that the fragment indicates (HTML "scroll to
    /// the fragment"): the element with that ID or an `<a>` with that name,
    /// or the top of the document for an empty fragment or `top`. Without a
    /// layout yet, scrolls after the first layout.
    pub(super) fn scroll_to_fragment(&mut self, fragment: &str) {
        self.update_layout();
        if self.fragments.is_none() {
            self.pending_scroll = Some(ScrollTarget::Fragment(fragment.to_owned()));
            return;
        }
        if let Some(position) = self.fragment_position(fragment) {
            self.scroll_to(position);
        }
    }

    /// Makes the element that `fragment` indicates the target (`:target`)
    /// and returns it.
    pub(super) fn set_target(&mut self, fragment: Option<&str>) -> Option<NodeId> {
        let target = self
            .document
            .as_ref()
            .and_then(|doc| indicated(doc, fragment?));
        self.update_states(|s| s.target = target);
        target
    }

    /// The scroll position for a fragment, if it indicates something.
    /// <https://html.spec.whatwg.org/multipage/browsing-the-web.html#the-indicated-part-of-the-document>
    pub(super) fn fragment_position(&self, fragment: &str) -> Option<Point> {
        let doc = self.document.as_ref()?;
        let tree = self.fragments.as_ref()?;
        let decoded = percent_decode(fragment);
        match indicated(doc, fragment) {
            Some(target) => node_position(doc, tree, target).map(|p| Point::new(0.0, p.y)),
            None if decoded.is_empty() || decoded.eq_ignore_ascii_case("top") => {
                Some(Point::default())
            }
            None => None,
        }
    }
}

/// The element that a fragment indicates: by its text, then percent-decoded.
pub(super) fn indicated(doc: &Document, fragment: &str) -> Option<NodeId> {
    indicated_element(doc, fragment).or_else(|| indicated_element(doc, &percent_decode(fragment)))
}

/// The element with the ID `name`, or the first `<a>` with that name.
fn indicated_element(doc: &Document, name: &str) -> Option<NodeId> {
    if name.is_empty() {
        return None;
    }
    doc.element_by_id(name).or_else(|| {
        doc.find_element(NodeId::DOCUMENT, |e| {
            e.is_html_named(&local_name!("a")) && e.attr("name") == Some(name)
        })
    })
}

/// The position of the first fragment of `node`. A node without fragments
/// (for example an empty `<a name>`) uses the first fragment that follows
/// it in tree order.
fn node_position(doc: &Document, tree: &FragmentTree, node: NodeId) -> Option<Point> {
    let mut first: HashMap<NodeId, Point> = HashMap::new();
    tree.walk(|fragment, origin| {
        let (owner, rect) = match fragment {
            FragmentRef::Box(b) if b.pseudo.is_none() => match b.node {
                Some(owner) => (owner, b.border_rect.translate(origin)),
                None => return,
            },
            FragmentRef::Text(t) => (t.node, t.rect.translate(origin)),
            FragmentRef::Box(_) => return,
        };
        first.entry(owner).or_insert(rect.origin());
    });
    doc.descendants(NodeId::DOCUMENT)
        .skip_while(|&n| n != node)
        .find_map(|n| first.get(&n).copied())
}

/// Percent-decodes `s` (<https://url.spec.whatwg.org/#percent-decode>) and
/// decodes the result as UTF-8, with replacement characters for invalid
/// sequences.
fn percent_decode(s: &str) -> String {
    String::from_utf8_lossy(&swb_net::percent_decode(s.as_bytes())).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("%C4%8D"), "č");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%4"), "%4");
        assert_eq!(percent_decode("%zz"), "%zz");
    }
}
