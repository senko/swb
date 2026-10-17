//! Sets of characters (the `CharSets` of ECMA-262 §22.2.2.1 without
//! strings) as sorted, disjoint, non-adjacent ranges of character values,
//! and the case operations on them: the closure under Canonicalize
//! (§22.2.2.7.3) that turns a class into the set of input characters it
//! matches, and `MaybeSimpleCaseFolding` (§22.2.2.9.5).
//!
//! A character is a code unit (0 to 0xFFFF) without the `u` and `v` flags
//! and a code point (0 to 0x10FFFF) with them.

use std::sync::OnceLock;

use swb_js_text::unicode::case;

/// The largest code unit.
pub(crate) const MAX_UNIT: u32 = 0xFFFF;
/// The largest code point.
pub(crate) const MAX_CODE_POINT: u32 = 0x0010_FFFF;

/// A set of characters.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct CharSet {
    ranges: Vec<(u32, u32)>,
}

impl CharSet {
    /// The empty set.
    pub(crate) fn new() -> CharSet {
        CharSet { ranges: Vec::new() }
    }

    /// The characters `first..=last`.
    pub(crate) fn range(first: u32, last: u32) -> CharSet {
        CharSet {
            ranges: vec![(first, last)],
        }
    }

    /// The one character `c`.
    pub(crate) fn single(c: u32) -> CharSet {
        CharSet::range(c, c)
    }

    /// The set of any ranges, in any order, overlapping or not.
    pub(crate) fn from_ranges(ranges: impl IntoIterator<Item = (u32, u32)>) -> CharSet {
        let mut ranges: Vec<(u32, u32)> = ranges.into_iter().filter(|r| r.0 <= r.1).collect();
        ranges.sort_unstable();
        let mut merged: Vec<(u32, u32)> = Vec::with_capacity(ranges.len());
        for (first, last) in ranges {
            match merged.last_mut() {
                Some(prev) if first <= prev.1.saturating_add(1) => prev.1 = prev.1.max(last),
                _ => merged.push((first, last)),
            }
        }
        merged.shrink_to_fit();
        CharSet { ranges: merged }
    }

    /// Releases the unused capacity.
    pub(crate) fn shrink_to_fit(&mut self) {
        self.ranges.shrink_to_fit();
    }

    /// The ranges, sorted, disjoint and not adjacent.
    pub(crate) fn ranges(&self) -> &[(u32, u32)] {
        &self.ranges
    }

    /// Whether the set is empty.
    pub(crate) fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    /// The character, if the set has exactly one.
    pub(crate) fn single_char(&self) -> Option<u32> {
        match self.ranges[..] {
            [(first, last)] if first == last => Some(first),
            _ => None,
        }
    }

    /// Whether `c` is in the set.
    #[cfg(test)]
    pub(crate) fn contains(&self, c: u32) -> bool {
        let index = self.ranges.partition_point(|r| r.1 < c);
        self.ranges.get(index).is_some_and(|r| r.0 <= c)
    }

    /// The union of two sets.
    pub(crate) fn union(&self, other: &CharSet) -> CharSet {
        if other.is_empty() {
            return self.clone();
        }
        if self.is_empty() {
            return other.clone();
        }
        CharSet::from_ranges(self.ranges.iter().chain(&other.ranges).copied())
    }

    /// Adds the characters of `other`.
    pub(crate) fn add(&mut self, other: &CharSet) {
        if !other.is_empty() {
            *self = self.union(other);
        }
    }

    /// The characters `0..=max` that are not in the set.
    pub(crate) fn complement(&self, max: u32) -> CharSet {
        let mut out = Vec::with_capacity(self.ranges.len() + 1);
        let mut next = 0u32;
        for &(first, last) in &self.ranges {
            if first > max {
                break;
            }
            if first > next {
                out.push((next, first - 1));
            }
            next = last.saturating_add(1);
        }
        if next <= max {
            out.push((next, max));
        }
        CharSet { ranges: out }
    }

    /// The intersection of two sets.
    pub(crate) fn intersect(&self, other: &CharSet) -> CharSet {
        let mut out = Vec::new();
        let (mut i, mut j) = (0, 0);
        while let (Some(&a), Some(&b)) = (self.ranges.get(i), other.ranges.get(j)) {
            let first = a.0.max(b.0);
            let last = a.1.min(b.1);
            if first <= last {
                out.push((first, last));
            }
            if a.1 < b.1 {
                i += 1;
            } else {
                j += 1;
            }
        }
        CharSet { ranges: out }
    }

    /// The characters of `self` that are not in `other`.
    pub(crate) fn subtract(&self, other: &CharSet) -> CharSet {
        if other.is_empty() || self.is_empty() {
            return self.clone();
        }
        let max = self.ranges.last().map_or(0, |r| r.1);
        self.intersect(&other.complement(max))
    }
}

/// The case mapping of a regular expression with the `i` flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CaseMode {
    /// Without `u` and `v`: the toUppercase rule of Canonicalize.
    Upper,
    /// With `u` or `v`: simple case folding.
    Fold,
}

/// The characters that a case mapping changes: `(char, canonical)` by
/// char and by canonical character.
struct Changes {
    by_char: Vec<(u32, u32)>,
    by_canonical: Vec<(u32, u32)>,
}

impl Changes {
    fn new(by_char: Vec<(u32, u32)>) -> Changes {
        let mut by_canonical: Vec<(u32, u32)> = by_char.iter().map(|&(c, k)| (k, c)).collect();
        by_canonical.sort_unstable();
        Changes {
            by_char,
            by_canonical,
        }
    }

    fn of(mode: CaseMode) -> &'static Changes {
        static UPPER: OnceLock<Changes> = OnceLock::new();
        static FOLD: OnceLock<Changes> = OnceLock::new();
        match mode {
            CaseMode::Upper => UPPER.get_or_init(|| {
                Changes::new(
                    case::regexp_canonicalize_changes()
                        .map(|(u, k)| (u32::from(u), u32::from(k)))
                        .collect(),
                )
            }),
            CaseMode::Fold => {
                FOLD.get_or_init(|| Changes::new(case::simple_fold_changes().collect()))
            }
        }
    }
}

/// The entries of a sorted pair list whose first value is in `first..=last`.
fn entries_in(list: &[(u32, u32)], first: u32, last: u32) -> &[(u32, u32)] {
    let start = list.partition_point(|e| e.0 < first);
    let end = list.partition_point(|e| e.0 <= last);
    list.get(start..end).unwrap_or(&[])
}

/// Canonicalize (§22.2.2.7.3) of one character.
pub(crate) fn canonicalize(mode: CaseMode, c: u32) -> u32 {
    match mode {
        CaseMode::Upper => u16::try_from(c).map_or(c, |u| u32::from(case::regexp_canonicalize(u))),
        CaseMode::Fold => case::simple_fold(c),
    }
}

/// The canonical characters of the set: `{ Canonicalize(c) | c in set }`.
fn canonical_image(mode: CaseMode, set: &CharSet) -> CharSet {
    let changes = Changes::of(mode);
    let mut changed = Vec::new();
    let mut mapped = Vec::new();
    for &(first, last) in set.ranges() {
        for &(c, k) in entries_in(&changes.by_char, first, last) {
            changed.push((c, c));
            mapped.push((k, k));
        }
    }
    if changed.is_empty() {
        return set.clone();
    }
    set.subtract(&CharSet::from_ranges(changed))
        .union(&CharSet::from_ranges(mapped))
}

/// The set of input characters that a class with this set matches with
/// the `i` flag: every character whose canonical character is the
/// canonical character of a member (`CharacterSetMatcher`, §22.2.2.7.1).
pub(crate) fn case_closure(mode: CaseMode, set: &CharSet) -> CharSet {
    let changes = Changes::of(mode);
    let canonical = canonical_image(mode, set);
    // A character that the mapping does not change matches if it is
    // canonical and in the image; a changed one if its canonical
    // character is in the image.
    let mut changed = Vec::new();
    let mut extra = Vec::new();
    for &(first, last) in canonical.ranges() {
        for &(c, _) in entries_in(&changes.by_char, first, last) {
            changed.push((c, c));
        }
        for &(_, c) in entries_in(&changes.by_canonical, first, last) {
            extra.push((c, c));
        }
    }
    let base = if changed.is_empty() {
        canonical
    } else {
        canonical.subtract(&CharSet::from_ranges(changed))
    };
    base.union(&CharSet::from_ranges(extra))
}

/// `MaybeSimpleCaseFolding` (§22.2.2.9.5) of a set of single characters:
/// the simple case folding of each member.
pub(crate) fn simple_fold_set(set: &CharSet) -> CharSet {
    canonical_image(CaseMode::Fold, set)
}

/// The code points that simple case folding does not change (`AllCharacters`
/// with `v` and `i`, §22.2.2.9.4).
pub(crate) fn fold_fixed_points() -> &'static CharSet {
    static SET: OnceLock<CharSet> = OnceLock::new();
    SET.get_or_init(|| {
        let changed = CharSet::from_ranges(
            Changes::of(CaseMode::Fold)
                .by_char
                .iter()
                .map(|&(c, _)| (c, c)),
        );
        changed.complement(MAX_CODE_POINT)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(ranges: &[(u32, u32)]) -> CharSet {
        CharSet::from_ranges(ranges.iter().copied())
    }

    #[test]
    fn normalizes_ranges() {
        let s = set(&[(5, 9), (1, 3), (4, 4), (20, 30), (25, 26)]);
        assert_eq!(s.ranges(), &[(1, 9), (20, 30)]);
        assert!(s.contains(1) && s.contains(9) && s.contains(25));
        assert!(!s.contains(0) && !s.contains(10) && !s.contains(31));
        assert_eq!(set(&[(3, 2)]).ranges(), &[]);
    }

    #[test]
    fn set_operations() {
        let a = set(&[(0, 10), (20, 30)]);
        let b = set(&[(5, 25)]);
        assert_eq!(a.union(&b).ranges(), &[(0, 30)]);
        assert_eq!(a.intersect(&b).ranges(), &[(5, 10), (20, 25)]);
        assert_eq!(a.subtract(&b).ranges(), &[(0, 4), (26, 30)]);
        assert_eq!(a.complement(40).ranges(), &[(11, 19), (31, 40)]);
        assert_eq!(CharSet::new().complement(5).ranges(), &[(0, 5)]);
        assert_eq!(
            set(&[(0, MAX_CODE_POINT)])
                .complement(MAX_CODE_POINT)
                .ranges(),
            &[]
        );
        assert_eq!(CharSet::single(7).single_char(), Some(7));
        assert_eq!(a.single_char(), None);
    }

    #[test]
    fn closure_without_unicode() {
        let a = case_closure(CaseMode::Upper, &CharSet::single(u32::from(b'a')));
        assert_eq!(a.ranges(), &[(0x41, 0x41), (0x61, 0x61)]);
        // ſ maps to S, but non-ASCII to ASCII is not allowed: alone.
        let s = case_closure(CaseMode::Upper, &CharSet::single(0x17F));
        assert_eq!(s.ranges(), &[(0x17F, 0x17F)]);
        let k = case_closure(CaseMode::Upper, &CharSet::single(u32::from(b'k')));
        assert!(!k.contains(0x212A));
        let sigma = case_closure(CaseMode::Upper, &CharSet::single(0x3C2));
        assert!(sigma.contains(0x3A3) && sigma.contains(0x3C3) && sigma.contains(0x3C2));
        // [E-f] contains [ \ ] ^ _ ` and all ASCII letters.
        let range = case_closure(CaseMode::Upper, &CharSet::range(0x45, 0x66));
        assert!(range.contains(u32::from(b'a')) && range.contains(u32::from(b'Z')));
        assert!(range.contains(u32::from(b'_')));
    }

    #[test]
    fn closure_with_folding() {
        let k = case_closure(CaseMode::Fold, &CharSet::single(u32::from(b'k')));
        assert_eq!(k.ranges(), &[(0x4B, 0x4B), (0x6B, 0x6B), (0x212A, 0x212A)]);
        let az = case_closure(CaseMode::Fold, &CharSet::range(0x61, 0x7A));
        assert!(az.contains(0x17F) && az.contains(0x212A) && az.contains(0x41));
        let folded = simple_fold_set(&CharSet::range(0x100, 0x10D));
        assert!(folded.contains(0x101) && !folded.contains(0x100));
        assert!(!fold_fixed_points().contains(0x41));
        // Backreferences compare folded code points unit by unit: simple
        // case folding never moves a code point into another plane.
        for (c, f) in case::simple_fold_changes() {
            assert_eq!(c > 0xFFFF, f > 0xFFFF, "{c:#x}");
        }
        assert!(fold_fixed_points().contains(0x61));
    }
}
