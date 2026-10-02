//! Offsets between the text of an inline item and the DOM text node it
//! comes from.
//!
//! White-space processing (CSS Text 3 §4.1.1) removes and replaces
//! characters, and `text-transform` can change their length, so a byte
//! offset in the processed text differs from the offset in the node's
//! data. Text selection needs the node offsets: a selection is a range of
//! DOM positions, and it must survive a new layout.

/// The origin of one character of an item's text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CharSource {
    /// Byte offset of the character in the item text.
    pub(crate) item: u32,
    /// Byte length of the character in the item text.
    pub(crate) item_len: u32,
    /// Byte offset of the source character in the node's data.
    pub(crate) node: u32,
    /// Byte length of the source character.
    pub(crate) node_len: u32,
    /// True if the item character is the source character or one
    /// character of the same length (white space replaced by a space, a
    /// letter changed to the other case). Offsets then correspond.
    pub(crate) exact: bool,
}

/// The start of a stretch of the map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Anchor {
    item: u32,
    node: u32,
    /// True if both offsets advance together until the next anchor. False
    /// for one character whose length changed: offsets inside it map to
    /// its end.
    exact: bool,
}

/// Maps byte offsets in the text of an inline item to byte offsets in the
/// data of its DOM text node. Character boundaries of the item text map to
/// character boundaries of the node's data.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SourceMap {
    /// Sorted by both offsets. The last anchor marks the end of the item.
    anchors: Vec<Anchor>,
}

impl SourceMap {
    /// Builds the map from the characters of the item text, in order.
    pub(crate) fn new(chars: &[CharSource]) -> Self {
        let mut anchors: Vec<Anchor> = Vec::new();
        let mut stretch_end: Option<(u32, u32)> = None;
        for c in chars {
            let exact = c.exact && c.item_len == c.node_len;
            if !exact || stretch_end != Some((c.item, c.node)) {
                anchors.push(Anchor {
                    item: c.item,
                    node: c.node,
                    exact,
                });
            }
            // After a character whose length changed, the next character
            // starts a new stretch.
            stretch_end = exact.then_some((c.item + c.item_len, c.node + c.node_len));
        }
        if let Some(last) = chars.last() {
            anchors.push(Anchor {
                item: last.item + last.item_len,
                node: last.node + last.node_len,
                exact: true,
            });
        }
        SourceMap { anchors }
    }

    /// The node offset for a byte offset in the item text.
    pub(crate) fn node_offset(&self, item_offset: usize) -> u32 {
        let offset = u32::try_from(item_offset).unwrap_or(u32::MAX);
        let i = self.anchors.partition_point(|a| a.item <= offset);
        let Some(anchor) = i.checked_sub(1).and_then(|i| self.anchors.get(i)) else {
            return self.anchors.first().map_or(offset, |a| a.node);
        };
        let Some(next) = self.anchors.get(i).map(|a| a.node) else {
            // At or after the end.
            return anchor.node;
        };
        if anchor.exact {
            anchor.node.saturating_add(offset - anchor.item).min(next)
        } else if offset == anchor.item {
            anchor.node
        } else {
            next
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn same(item: u32, node: u32, len: u32) -> CharSource {
        CharSource {
            item,
            item_len: len,
            node,
            node_len: len,
            exact: true,
        }
    }

    fn changed(item: u32, item_len: u32, node: u32, node_len: u32) -> CharSource {
        CharSource {
            item,
            item_len,
            node,
            node_len,
            exact: false,
        }
    }

    #[test]
    fn identity_has_one_stretch() {
        let map = SourceMap::new(&[same(0, 0, 1), same(1, 1, 1), same(2, 2, 2)]);
        assert_eq!(map.anchors.len(), 2);
        assert_eq!(map.node_offset(2), 2);
        assert_eq!(map.node_offset(4), 4);
        assert_eq!(map.node_offset(9), 4);
    }

    #[test]
    fn collapsed_white_space() {
        // "  a   b" -> "a b": 'a' at 2, ' ' at 3, 'b' at 6.
        let map = SourceMap::new(&[same(0, 2, 1), same(1, 3, 1), same(2, 6, 1)]);
        assert_eq!(map.node_offset(0), 2);
        assert_eq!(map.node_offset(1), 3);
        assert_eq!(map.node_offset(2), 6);
        assert_eq!(map.node_offset(3), 7);
        assert_eq!(map.node_offset(4), 7);
    }

    #[test]
    fn offsets_inside_a_longer_character_map_to_its_end() {
        // "ßa" uppercased is "SSA": 'ß' and "SS" have the same length but
        // different character boundaries.
        let map = SourceMap::new(&[changed(0, 2, 0, 2), same(2, 2, 1)]);
        assert_eq!(map.node_offset(1), 2);
        assert_eq!(map.node_offset(2), 2);
        assert_eq!(map.node_offset(3), 3);
        // "ǰa" uppercased is "J̌A" (3 bytes for 2).
        let map = SourceMap::new(&[changed(0, 3, 0, 2), same(3, 2, 1)]);
        assert_eq!(map.node_offset(0), 0);
        assert_eq!(map.node_offset(1), 2);
        assert_eq!(map.node_offset(2), 2);
        assert_eq!(map.node_offset(3), 2);
        assert_eq!(map.node_offset(4), 3);
    }

    #[test]
    fn empty_map_is_the_identity() {
        assert_eq!(SourceMap::default().node_offset(5), 5);
    }
}
