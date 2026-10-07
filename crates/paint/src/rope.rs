//! A list of display items in chunks, for building the display list.
//!
//! The builder moves the items of positioned boxes out of the list and
//! back into the list of their stacking context (`display_list.rs`). With
//! one vector, every nesting level copied all the items of its subtree, so
//! deeply nested positioned or transformed boxes cost items × depth. Here a
//! split copies only the items after the split point in its chunk (the
//! items of the box that the builder is painting), and appending moves
//! whole chunks. Each item is copied a bounded number of times. The chunk
//! headers (at most a few per positioned box) still move once per nesting
//! level, and the box tree is at most 256 levels deep. A list in one chunk
//! (a page without positioned boxes) becomes the display list without a
//! copy.

use crate::display_list::DisplayItem;

/// Display items in order: closed chunks, then the open chunk that new
/// items go into.
#[derive(Debug, Default)]
pub(crate) struct ItemRope {
    /// The closed chunks (none empty).
    done: Vec<Vec<DisplayItem>>,
    /// The number of items in `done`.
    done_len: usize,
    /// The open chunk, after `done`.
    tail: Vec<DisplayItem>,
}

impl ItemRope {
    /// The number of items.
    pub(crate) fn len(&self) -> usize {
        self.done_len + self.tail.len()
    }

    /// Adds an item at the end.
    pub(crate) fn push(&mut self, item: DisplayItem) {
        self.tail.push(item);
    }

    /// The item at `index`, for changing it (the recent chunks are
    /// searched first).
    pub(crate) fn get_mut(&mut self, index: usize) -> Option<&mut DisplayItem> {
        let (chunk, start) = self.chunk_of(index);
        match self.done.get_mut(chunk) {
            Some(chunk) => chunk.get_mut(index.checked_sub(start)?),
            None => self.tail.get_mut(index.checked_sub(self.done_len)?),
        }
    }

    /// The items from `at` on, in order. The cost is the number of chunks
    /// and items from `at` on.
    pub(crate) fn iter_from(&self, at: usize) -> impl Iterator<Item = &DisplayItem> {
        let (chunk, start) = self.chunk_of(at);
        self.done
            .get(chunk..)
            .unwrap_or_default()
            .iter()
            .flatten()
            .chain(&self.tail)
            .skip(at.saturating_sub(start))
    }

    /// The index of the closed chunk that contains item `at` (`done.len()`
    /// for an item in the open chunk), and the index of its first item.
    /// Searches from the end: the builder reads and splits recent items.
    fn chunk_of(&self, at: usize) -> (usize, usize) {
        let mut start = self.done_len;
        let mut index = self.done.len();
        for (i, chunk) in self.done.iter().enumerate().rev() {
            if at >= start {
                break;
            }
            start -= chunk.len();
            index = i;
        }
        (index, start)
    }

    /// Removes the items from `at` on and returns them. Copies only the
    /// items after `at` in the chunk that contains it.
    pub(crate) fn split_off(&mut self, at: usize) -> ItemRope {
        let at = at.min(self.len());
        if at >= self.done_len {
            // In the open chunk; it keeps its capacity.
            return ItemRope {
                done: Vec::new(),
                done_len: 0,
                tail: self.tail.split_off(at - self.done_len),
            };
        }
        let (split, start) = self.chunk_of(at);
        let mut moved: Vec<Vec<DisplayItem>> = self.done.split_off(split);
        if let Some(first) = moved.first_mut()
            && at > start
        {
            let rest = first.split_off(at - start);
            self.done.push(std::mem::replace(first, rest));
        }
        let tail = std::mem::take(&mut self.tail);
        let moved_len = self.done_len - at;
        self.done_len = at;
        // The last closed chunk becomes the open one, so that pushes go on
        // after the items that stay.
        if let Some(last) = self.done.pop() {
            self.done_len -= last.len();
            self.tail = last;
        }
        ItemRope {
            done: moved,
            done_len: moved_len,
            tail,
        }
    }

    /// Moves the items of `other` to the end.
    pub(crate) fn append(&mut self, other: ItemRope) {
        if other.len() == 0 {
            return;
        }
        if self.len() == 0 {
            *self = other;
            return;
        }
        let tail = std::mem::take(&mut self.tail);
        if !tail.is_empty() {
            self.done_len += tail.len();
            self.done.push(tail);
        }
        self.done_len += other.done_len;
        self.done.extend(other.done);
        // The appended open chunk stays open: new items follow it.
        self.tail = other.tail;
    }

    /// Inserts the items of `other` before index `at`.
    pub(crate) fn insert(&mut self, at: usize, other: ItemRope) {
        if other.len() == 0 {
            return;
        }
        let tail = self.split_off(at);
        self.append(other);
        self.append(tail);
    }

    /// The items in one vector (without a copy if they are in one chunk).
    /// The vector is the allocation of the largest chunk: a new large
    /// allocation on every build would cost page faults (measured on the
    /// Wikipedia fixture: twice the time of the build).
    pub(crate) fn into_vec(self) -> Vec<DisplayItem> {
        let len = self.len();
        let mut chunks = self.done;
        chunks.push(self.tail);
        let Some(largest) = (0..chunks.len()).max_by_key(|&i| chunks[i].capacity()) else {
            return Vec::new();
        };
        let mut items = std::mem::take(&mut chunks[largest]);
        if items.len() == len {
            return items;
        }
        let after: usize = chunks[largest + 1..].iter().map(Vec::len).sum();
        let before = len - items.len() - after;
        items.reserve(before + after);
        // The chunks before it go to the end and are rotated to the front
        // (in place, without another allocation).
        let mut rest = chunks.into_iter();
        for chunk in rest.by_ref().take(largest) {
            items.extend(chunk);
        }
        items.rotate_right(before);
        for chunk in rest.skip(1) {
            items.extend(chunk);
        }
        items
    }
}

impl Extend<DisplayItem> for ItemRope {
    fn extend<I: IntoIterator<Item = DisplayItem>>(&mut self, iter: I) {
        self.tail.extend(iter);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(i: usize) -> DisplayItem {
        DisplayItem::PushClip(swb_layout::Rect::new(i as f32, 0.0, 1.0, 1.0))
    }

    fn value(item: &DisplayItem) -> usize {
        match item {
            DisplayItem::PushClip(r) => r.x as usize,
            _ => usize::MAX,
        }
    }

    fn values(rope: ItemRope) -> Vec<usize> {
        rope.into_vec().iter().map(value).collect()
    }

    #[test]
    fn splits_appends_and_inserts_keep_the_order() {
        let mut a = ItemRope::default();
        a.extend((0..5).map(item));
        let mut b = a.split_off(3);
        assert_eq!((a.len(), b.len()), (3, 2));
        b.push(item(5));
        a.append(b);
        let mut c = ItemRope::default();
        c.extend((10..12).map(item));
        a.insert(1, c);
        assert_eq!(a.len(), 8);
        assert!(matches!(a.get_mut(1), Some(DisplayItem::PushClip(r)) if r.x == 10.0));
        assert!(matches!(a.get_mut(7), Some(DisplayItem::PushClip(r)) if r.x == 5.0));
        assert_eq!(a.iter_from(6).count(), 2);
        a.push(item(6));
        let tail = a.split_off(4);
        assert_eq!(values(tail), vec![2, 3, 4, 5, 6]);
        a.push(item(7));
        assert_eq!(values(a), vec![0, 10, 11, 1, 7]);
    }

    #[test]
    fn splits_at_the_ends() {
        let mut a = ItemRope::default();
        a.extend((0..3).map(item));
        let mut all = a.split_off(0);
        assert_eq!(a.len(), 0);
        let none = all.split_off(3);
        assert_eq!(none.len(), 0);
        assert!(all.get_mut(3).is_none());
        assert_eq!(values(all), vec![0, 1, 2]);
    }

    #[test]
    fn splits_in_closed_chunks() {
        // Three chunks: [0 1] [2 3] and the open [4 5].
        let mut a = ItemRope::default();
        a.extend((0..2).map(item));
        for range in [2..4, 4..6] {
            let mut b = ItemRope::default();
            b.extend(range.map(item));
            a.append(b);
        }
        for at in 0..8 {
            let from: Vec<usize> = a.iter_from(at).map(value).collect();
            assert_eq!(from, (at.min(6)..6).collect::<Vec<_>>(), "{at}");
        }
        let tail = a.split_off(3);
        assert_eq!(a.len(), 3);
        a.push(item(9));
        assert_eq!(values(tail), vec![3, 4, 5]);
        assert_eq!(values(a), vec![0, 1, 2, 9]);
    }
}
