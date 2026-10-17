//! The compiled form of a pattern: a backtracking program of Rust enum
//! instructions with fixed-width jump targets (ADR 0026 section 10).
//!
//! The matcher keeps its state in slots (plain integers): the capture
//! positions, the start position of each open capture group, and the
//! registers of loops and lookarounds. A counted quantifier keeps its
//! count in a register, so the program size does not depend on the counts.

use crate::ast::Assertion;
use crate::charset::CaseMode;

/// A slot value: an unset capture.
pub(crate) const UNSET: u32 = u32::MAX;

/// A test of one character.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Test {
    /// Exactly this character.
    Char(u32),
    /// A character of the set with this index.
    Set(u32),
    /// Any character.
    Any,
    /// Any character but a line terminator.
    AnyButLineTerminator,
}

/// An instruction. `back` marks the instructions of a lookbehind, which
/// read the input backward (§22.2.2.3.4, `MatchSequence`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Inst {
    /// Matches one character.
    Test { test: Test, back: bool },
    /// Matches the test `min..=max` times (an atom of one character
    /// without captures), as many as possible first if `greedy`.
    Repeat {
        test: Test,
        back: bool,
        greedy: bool,
        min: u32,
        max: u32,
    },
    /// A zero-width assertion.
    Assert(Assertion),
    /// Continues at the next instruction; on failure at `alt`.
    Fork { alt: u32 },
    /// Continues at `first`; on failure at the next instruction.
    ForkFirst { first: u32 },
    /// Continues at `to`.
    Jump { to: u32 },
    /// Records the position where capture group `group` starts (in the
    /// matching direction).
    Open { group: u32 },
    /// Sets capture group `group` from the recorded position to here.
    Close { group: u32 },
    /// Clears the capture groups `first + 1 ..= first + count` (each
    /// iteration of a quantifier, §22.2.2.3.1 step 4).
    Reset { first: u32, count: u32 },
    /// A backreference to the first set group of a list.
    Backref {
        list: u32,
        back: bool,
        ignore_case: bool,
    },
    /// Sets the counter in slot `reg` to 0.
    LoopInit { reg: u32 },
    /// The decision at the start of an iteration of a counted loop with
    /// the counter in `reg`: iterate (the next instruction) or leave
    /// (`exit`).
    LoopHead {
        reg: u32,
        min: u32,
        max: u32,
        greedy: bool,
        exit: u32,
    },
    /// The end of an iteration: fails an empty iteration after the minimum
    /// if `empty_check` (§22.2.2.3.1 step 2.b; the start position is in
    /// `reg + 1`), counts it and goes back to `head`.
    LoopTail {
        reg: u32,
        min: u32,
        head: u32,
        empty_check: bool,
    },
    /// Records the position in slot `reg`.
    SavePos { reg: u32 },
    /// Fails if the position is the one recorded in `reg` (the empty check
    /// of a loop without a minimum).
    CheckProgress { reg: u32 },
    /// Starts a lookaround; `reg` gets the backtrack stack depth, and `end`
    /// is where a negative lookaround continues when its body fails.
    LookStart { reg: u32, negate: bool, end: u32 },
    /// The end of a lookaround body.
    LookEnd { reg: u32, negate: bool },
    /// The pattern matched.
    Match,
}

/// A set of characters for [`Test::Set`]: sorted ranges and a bitmap of
/// the first 256 characters.
#[derive(Clone, Debug)]
pub(crate) struct SetData {
    pub(crate) ranges: Box<[(u32, u32)]>,
    pub(crate) latin1: [u64; 4],
}

impl SetData {
    pub(crate) fn new(ranges: &[(u32, u32)]) -> SetData {
        let mut latin1 = [0u64; 4];
        for &(first, last) in ranges {
            for c in first..=last.min(255) {
                if let Some(word) = latin1.get_mut((c / 64) as usize) {
                    *word |= 1 << (c % 64);
                }
            }
        }
        SetData {
            ranges: ranges.into(),
            latin1,
        }
    }

    #[inline]
    pub(crate) fn contains(&self, c: u32) -> bool {
        if c < 256 {
            return self.latin1[(c / 64) as usize] & (1 << (c % 64)) != 0;
        }
        let index = self.ranges.partition_point(|r| r.1 < c);
        self.ranges.get(index).is_some_and(|r| r.0 <= c)
    }
}

/// What the first character of every match must be, so that the search
/// can skip start positions.
#[derive(Clone, Debug)]
pub(crate) enum Prefix {
    /// Exactly this character.
    Char(u32),
    /// A character of this set.
    Set(SetData),
}

/// A compiled pattern.
#[derive(Clone, Debug)]
pub(crate) struct Program {
    pub(crate) insts: Vec<Inst>,
    pub(crate) sets: Vec<SetData>,
    pub(crate) backref_lists: Vec<Box<[u32]>>,
    /// The number of capture groups (without the whole match).
    pub(crate) group_count: u32,
    /// All slots: 2 per capture group and 2 for the whole match, then one
    /// start position per group, then the registers.
    pub(crate) slot_count: u32,
    /// `u` or `v`: the matcher reads code points.
    pub(crate) unicode: bool,
    /// The case mapping of backreferences with `i`.
    pub(crate) case_mode: CaseMode,
    /// `y`: only the start position is tried.
    pub(crate) sticky: bool,
    pub(crate) prefix: Option<Prefix>,
}

impl Program {
    /// The slot of the start of group `group` (0 is the whole match).
    pub(crate) fn capture_slot(group: u32) -> u32 {
        group * 2
    }

    /// The slot where an open group records its position.
    pub(crate) fn open_slot(&self, group: u32) -> u32 {
        (self.group_count + 1) * 2 + group - 1
    }

    /// The approximate number of bytes on the heap.
    pub(crate) fn heap_bytes(&self) -> usize {
        let insts = self.insts.capacity() * size_of::<Inst>();
        let table = self.sets.capacity() * size_of::<SetData>()
            + self.backref_lists.capacity() * size_of::<Box<[u32]>>();
        let sets: usize = self.sets.iter().map(|s| s.ranges.len() * 8).sum();
        let lists: usize = self.backref_lists.iter().map(|l| l.len() * 4).sum();
        let prefix = match &self.prefix {
            Some(Prefix::Set(s)) => size_of::<SetData>() + s.ranges.len() * 8,
            _ => 0,
        };
        insts + table + sets + lists + prefix
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instructions_stay_small() {
        assert!(size_of::<Inst>() <= 24, "{}", size_of::<Inst>());
        let set = SetData::new(&[(0x41, 0x5A), (0x100, 0x17F)]);
        assert!(set.contains(0x41) && set.contains(0x17F) && !set.contains(0x61));
        assert!(!set.contains(0x180) && !set.contains(0xFF));
    }
}
