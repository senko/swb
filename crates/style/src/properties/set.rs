//! A set of longhands, as a bit set indexed by `LonghandId as usize`.

use super::ids::LonghandId;

/// The number of 64-bit words of a [`LonghandSet`].
const WORDS: usize = LonghandId::COUNT.div_ceil(64);

/// A set of longhands: bit `id as usize`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LonghandSet {
    words: [u64; WORDS],
}

impl LonghandSet {
    /// The empty set.
    pub(crate) const fn new() -> Self {
        LonghandSet { words: [0; WORDS] }
    }

    /// Adds `id`. True if it was not in the set.
    pub(crate) fn insert(&mut self, id: LonghandId) -> bool {
        let (word, bit) = Self::position(id);
        match self.words.get_mut(word) {
            Some(w) => {
                let new = *w & bit == 0;
                *w |= bit;
                new
            }
            None => false,
        }
    }

    /// Removes `id`.
    pub(crate) fn remove(&mut self, id: LonghandId) {
        let (word, bit) = Self::position(id);
        if let Some(w) = self.words.get_mut(word) {
            *w &= !bit;
        }
    }

    /// True if `id` is in the set.
    pub(crate) fn contains(&self, id: LonghandId) -> bool {
        let (word, bit) = Self::position(id);
        self.words.get(word).is_some_and(|w| w & bit != 0)
    }

    /// Adds every longhand of `other`.
    pub(crate) fn union_with(&mut self, other: &LonghandSet) {
        for (word, added) in self.words.iter_mut().zip(other.words) {
            *word |= added;
        }
    }

    /// The members in the order of [`LonghandId::ALL`].
    pub(crate) fn iter(&self) -> impl Iterator<Item = LonghandId> + '_ {
        self.words
            .iter()
            .enumerate()
            .flat_map(|(word_index, &word)| {
                let mut bits = word;
                std::iter::from_fn(move || {
                    if bits == 0 {
                        return None;
                    }
                    let index = word_index * 64 + bits.trailing_zeros() as usize;
                    bits &= bits - 1;
                    Some(index)
                })
            })
            .filter_map(|index| LonghandId::ALL.get(index).copied())
    }

    /// The word and the bit mask of `id`.
    fn position(id: LonghandId) -> (usize, u64) {
        let index = id as usize;
        (index / 64, 1u64 << (index % 64))
    }
}

impl Default for LonghandSet {
    fn default() -> Self {
        LonghandSet::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_remove_contains() {
        let mut set = LonghandSet::new();
        assert!(!set.contains(LonghandId::Color));
        assert!(set.insert(LonghandId::Color));
        assert!(!set.insert(LonghandId::Color));
        assert!(set.contains(LonghandId::Color));
        set.remove(LonghandId::Color);
        assert!(!set.contains(LonghandId::Color));
    }

    #[test]
    fn iterates_in_order_and_unions() {
        let mut a = LonghandSet::new();
        a.insert(LonghandId::Width);
        a.insert(LonghandId::Color);
        let mut b = LonghandSet::new();
        b.insert(LonghandId::Height);
        a.union_with(&b);
        let ids: Vec<usize> = a.iter().map(|id| id as usize).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted);
        assert_eq!(ids.len(), 3);
    }
}
