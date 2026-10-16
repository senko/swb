//! Ropes: the strings that a concatenation builds (ADR 0026 sections 7
//! and 15, memo 4.2; Boehm, Atkinson and Plass, "Ropes: an Alternative to
//! Strings", 1995).
//!
//! A rope node holds two child strings, the length, the width flag and
//! the depth. Rules of [`Heap::concat_strings`]:
//!
//! - A result of at most [`FLAT_LIMIT`] code units is a flat copy.
//! - A short flat right part is copied into a fresh leaf with the flat
//!   leaf at the right edge of the left rope (at most two levels down),
//!   if the two fit in [`FLAT_LIMIT`]; a short flat left part likewise
//!   with the leaf at the left edge of the right rope. So a loop that
//!   appends or prepends small pieces (or does both in turn) makes leaves
//!   of up to [`FLAT_LIMIT`] units, not a chain of tiny leaves: the memory
//!   per code unit stays bounded, and the depth grows by one per filled
//!   leaf.
//! - Otherwise the result is a new node. Its depth must stay at most
//!   [`MAX_DEPTH`]: `Runtime::concat` first rebalances an operand that
//!   has reached the limit ([`Heap::rebalance_with`]).
//!
//! The rebalance follows Boehm, Atkinson and Plass (section "Balance"):
//! the leaves in order become the leaves of a balanced tree, and short
//! adjacent leaves are joined into flat chunks. Here the chunk size grows
//! with the length (a 1/[`CHUNKS`] part), so a rebalanced rope has at most
//! about `5 * CHUNKS` leaves and a depth of about 10, whatever the history.
//! A loop that appends pieces too long to merge then rebalances once per
//! `MAX_DEPTH - 10` appends, visits at most `5 * CHUNKS` old leaves each
//! time, and copies each code unit about once per doubling of the length.
//! A flatten instead would copy the whole string each time (quadratic).
//!
//! No operation recurses in Rust over a rope: [`LeafCursor`] walks the
//! leaves in order with a fixed stack of `MAX_DEPTH + 1` entries, the
//! flatten copies the leaves into one buffer, and the collector marks
//! children through its work list. A flatten writes the flat text into
//! the rope's own slot, and a rebalance its new root, so every holder of
//! the handle sees the result; the old children become garbage unless
//! something else holds them. Equality and the order of two strings walk
//! the leaves without allocation; everything else reads the flat text
//! ([`Heap::text`]) after a flatten.

use std::cmp::Ordering;

use swb_js_text::{Str16, String16};

use super::JsString;
use crate::error::{Error, Result};
use crate::heap::{Arena, Gc, Heap};

/// Concatenations whose result has at most this many code units copy
/// both parts into a flat string; leaf merges stop at this size.
pub(crate) const FLAT_LIMIT: usize = 1024;

/// The deepest rope. A concatenation rebalances an operand at this depth
/// before it makes a new node above it.
pub(crate) const MAX_DEPTH: u16 = 256;

/// A rebalance joins short leaves into chunks of at least 1/`CHUNKS` of
/// the length.
const CHUNKS: usize = 64;

/// The entries of the walk stack: a rope of depth `d` needs `d + 1`.
const STACK: usize = MAX_DEPTH as usize + 1;

/// A flatten or rebalance reports its progress after about this many code
/// units.
const PROGRESS_UNITS: usize = 1 << 16;

/// The cost of visiting one leaf, in code units of copying.
const LEAF_COST: usize = 16;

/// A rope node: the concatenation of `left` and `right`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Rope {
    pub(crate) left: Gc<JsString>,
    pub(crate) right: Gc<JsString>,
    len: u32,
    depth: u16,
    latin1: bool,
}

impl Rope {
    /// The number of code units.
    pub(crate) fn len(&self) -> usize {
        self.len as usize
    }

    /// Whether all code units are below 256.
    pub(crate) fn is_latin1(&self) -> bool {
        self.latin1
    }

    /// The longest path to a flat leaf (at least 1).
    pub(crate) fn depth(&self) -> u16 {
        self.depth
    }

    /// The bytes of the flat text.
    fn flat_bytes(&self) -> usize {
        if self.latin1 {
            self.len()
        } else {
            self.len() * 2
        }
    }
}

/// A walk over the flat leaves of a string in order (a flat string is its
/// own leaf); empty leaves are skipped. Allocates nothing: the stack is a
/// fixed array, enough for [`MAX_DEPTH`]. The cursor does not borrow the
/// heap, so the caller can allocate between two steps (the rebalance).
pub(crate) struct LeafCursor {
    stack: [Gc<JsString>; STACK],
    top: usize,
}

impl LeafCursor {
    pub(crate) fn new(root: Gc<JsString>) -> Self {
        LeafCursor {
            stack: [root; STACK],
            top: 1,
        }
    }

    fn push(&mut self, handle: Gc<JsString>) -> Result<()> {
        let slot = self
            .stack
            .get_mut(self.top)
            .ok_or(Error::invariant("a rope deeper than the depth limit"))?;
        *slot = handle;
        self.top += 1;
        Ok(())
    }

    /// The next leaf. After an error the walk is over.
    pub(crate) fn next_leaf(&mut self, strings: &Arena<JsString>) -> Result<Option<Gc<JsString>>> {
        let step = self.step(strings);
        if step.is_err() {
            self.top = 0;
        }
        step
    }

    fn step(&mut self, strings: &Arena<JsString>) -> Result<Option<Gc<JsString>>> {
        while self.top > 0 {
            self.top -= 1;
            let handle = *self
                .stack
                .get(self.top)
                .ok_or(Error::invariant("the rope walk stack"))?;
            let string = strings.get(handle)?;
            match string.as_rope() {
                None if string.is_empty() => {}
                None => return Ok(Some(handle)),
                Some(rope) => {
                    self.push(rope.right)?;
                    self.push(rope.left)?;
                }
            }
        }
        Ok(None)
    }
}

/// The texts of the leaves of a string in order ([`LeafCursor`] with the
/// heap borrowed).
pub(crate) struct Leaves<'a> {
    strings: &'a Arena<JsString>,
    cursor: LeafCursor,
}

impl<'a> Leaves<'a> {
    pub(crate) fn new(strings: &'a Arena<JsString>, root: Gc<JsString>) -> Self {
        Leaves {
            strings,
            cursor: LeafCursor::new(root),
        }
    }

    fn step(&mut self) -> Result<Option<Str16<'a>>> {
        let Some(leaf) = self.cursor.next_leaf(self.strings)? else {
            return Ok(None);
        };
        let text = self
            .strings
            .get(leaf)?
            .as_flat()
            .ok_or(Error::invariant("a leaf is flat"))?;
        Ok(Some(text))
    }
}

impl<'a> Iterator for Leaves<'a> {
    type Item = Result<Str16<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        self.step().transpose()
    }
}

/// The empty text.
const EMPTY: Str16<'static> = Str16::Latin1(&[]);

/// `a` followed by `b`, in an exact allocation.
fn joined(a: Str16<'_>, b: Str16<'_>) -> Result<String16> {
    let wide = matches!(a, Str16::Wide(_)) || matches!(b, Str16::Wide(_));
    let mut text = String16::try_with_capacity(a.len() + b.len(), wide)?;
    text.push_str16(a);
    text.push_str16(b);
    Ok(text)
}

/// The order of two texts by code units (§7.2.13 step 3 for strings).
pub(crate) fn compare_str16(a: Str16<'_>, b: Str16<'_>) -> Ordering {
    match (a, b) {
        (Str16::Latin1(x), Str16::Latin1(y)) => x.cmp(y),
        (Str16::Wide(x), Str16::Wide(y)) => x.cmp(y),
        _ => a.units().cmp(b.units()),
    }
}

/// The first `n` units of `text` and the rest (`n` is at most its length).
fn split(text: Str16<'_>, n: usize) -> (Str16<'_>, Str16<'_>) {
    let len = text.len();
    (
        text.slice(0, n).unwrap_or(EMPTY),
        text.slice(n, len).unwrap_or(EMPTY),
    )
}

/// The result of a concatenation and the code units it copied.
pub(crate) struct Concat {
    pub(crate) string: Gc<JsString>,
    pub(crate) copied: usize,
}

impl Heap {
    /// The concatenation of two non-empty strings (the rules of the module
    /// documentation). The caller checked the maximum length and flattened
    /// an operand at [`MAX_DEPTH`]. Never collects: every allocation is of
    /// a size below a constant (a leaf of at most [`FLAT_LIMIT`] units,
    /// a node).
    pub(crate) fn concat_strings(&mut self, a: Gc<JsString>, b: Gc<JsString>) -> Result<Concat> {
        let total = self.string(a)?.len() + self.string(b)?.len();
        if total <= FLAT_LIMIT {
            // The parts are flat, except for the inner nodes of a
            // rebalanced rope (scripts do not see them).
            let wide = !(self.string(a)?.is_latin1() && self.string(b)?.is_latin1());
            let mut text = String16::try_with_capacity(total, wide)?;
            self.push_string(&mut text, a)?;
            self.push_string(&mut text, b)?;
            let string = self.alloc_string(text)?;
            return Ok(Concat {
                string,
                copied: total,
            });
        }
        if let Some(done) = self.merge_right(a, b)? {
            return Ok(done);
        }
        if let Some(done) = self.merge_left(a, b)? {
            return Ok(done);
        }
        let string = self.alloc_rope(a, b)?;
        Ok(Concat { string, copied: 0 })
    }

    /// `a + b` for a rope `a` and a short flat `b`: `b` joins the flat leaf
    /// at the right edge of `a`, one or two levels down.
    fn merge_right(&mut self, a: Gc<JsString>, b: Gc<JsString>) -> Result<Option<Concat>> {
        let Some(units) = self.short_flat(b)? else {
            return Ok(None);
        };
        let Some(outer) = self.string(a)?.as_rope().copied() else {
            return Ok(None);
        };
        if let Some(leaf) = self.merged(outer.right, b, units, false)? {
            let string = self.alloc_rope(outer.left, leaf)?;
            return Ok(Some(Concat {
                string,
                copied: self.string(leaf)?.len(),
            }));
        }
        let Some(inner) = self.string(outer.right)?.as_rope().copied() else {
            return Ok(None);
        };
        if let Some(leaf) = self.merged(inner.right, b, units, false)? {
            let middle = self.alloc_rope(inner.left, leaf)?;
            let string = self.alloc_rope(outer.left, middle)?;
            return Ok(Some(Concat {
                string,
                copied: self.string(leaf)?.len(),
            }));
        }
        Ok(None)
    }

    /// `a + b` for a short flat `a` and a rope `b`: `a` joins the flat leaf
    /// at the left edge of `b`, one or two levels down.
    fn merge_left(&mut self, a: Gc<JsString>, b: Gc<JsString>) -> Result<Option<Concat>> {
        let Some(units) = self.short_flat(a)? else {
            return Ok(None);
        };
        let Some(outer) = self.string(b)?.as_rope().copied() else {
            return Ok(None);
        };
        if let Some(leaf) = self.merged(outer.left, a, units, true)? {
            let string = self.alloc_rope(leaf, outer.right)?;
            return Ok(Some(Concat {
                string,
                copied: self.string(leaf)?.len(),
            }));
        }
        let Some(inner) = self.string(outer.left)?.as_rope().copied() else {
            return Ok(None);
        };
        if let Some(leaf) = self.merged(inner.left, a, units, true)? {
            let middle = self.alloc_rope(leaf, inner.right)?;
            let string = self.alloc_rope(middle, outer.right)?;
            return Ok(Some(Concat {
                string,
                copied: self.string(leaf)?.len(),
            }));
        }
        Ok(None)
    }

    /// The length of `string` if it is flat and shorter than
    /// [`FLAT_LIMIT`].
    fn short_flat(&self, string: Gc<JsString>) -> Result<Option<usize>> {
        let string = self.string(string)?;
        Ok((!string.is_rope() && string.len() < FLAT_LIMIT).then(|| string.len()))
    }

    /// A new leaf with the units of the flat `edge` and of the short flat
    /// `piece` (`piece` first if `before`), if `edge` is flat and the two
    /// fit in [`FLAT_LIMIT`].
    fn merged(
        &mut self,
        edge: Gc<JsString>,
        piece: Gc<JsString>,
        piece_units: usize,
        before: bool,
    ) -> Result<Option<Gc<JsString>>> {
        let Some(edge_text) = self.string(edge)?.as_flat() else {
            return Ok(None);
        };
        if edge_text.len() + piece_units > FLAT_LIMIT {
            return Ok(None);
        }
        let piece_text = self.text(piece)?;
        let text = if before {
            joined(piece_text, edge_text)?
        } else {
            joined(edge_text, piece_text)?
        };
        Ok(Some(self.alloc_string(text)?))
    }

    /// A new rope node. Its depth must stay within [`MAX_DEPTH`] and its
    /// length within `u32`.
    fn alloc_rope(&mut self, left: Gc<JsString>, right: Gc<JsString>) -> Result<Gc<JsString>> {
        let rope = self.make_rope(left, right)?;
        self.charge(Arena::<JsString>::slot_size());
        self.arenas.strings.insert(JsString::rope(rope))
    }

    /// The node of `left` followed by `right` (not yet stored).
    fn make_rope(&self, left: Gc<JsString>, right: Gc<JsString>) -> Result<Rope> {
        let l = self.string(left)?;
        let r = self.string(right)?;
        let depth = l.depth().max(r.depth()) + 1;
        if depth > MAX_DEPTH {
            return Err(Error::invariant("a rope above the depth limit"));
        }
        let len = u32::try_from(l.len() + r.len())
            .map_err(|_| Error::invariant("a rope above the length limit"))?;
        Ok(Rope {
            left,
            right,
            len,
            depth,
            latin1: l.is_latin1() && r.is_latin1(),
        })
    }

    /// Rebalances a rope in place (see the module documentation): its
    /// leaves in order become the leaves of a balanced tree; leaves shorter
    /// than half a chunk are joined into flat chunks of at least
    /// `len / CHUNKS` units. The string keeps its handle and its code
    /// units; only its node changes. Nothing happens to a flat string.
    /// Does not collect: each new chunk must fit under the heap limit
    /// ([`Termination::HeapLimit`] otherwise). `progress` gets the work
    /// done, as in [`Heap::flatten_with`].
    ///
    /// [`Termination::HeapLimit`]: crate::error::Termination::HeapLimit
    pub(crate) fn rebalance_with(
        &mut self,
        string: Gc<JsString>,
        progress: &mut dyn FnMut(usize) -> Result<()>,
    ) -> Result<()> {
        let Some(old) = self.string(string)?.as_rope().copied() else {
            return Ok(());
        };
        let chunk = (old.len() / CHUNKS).max(FLAT_LIMIT);
        let pieces = self.rebalance_pieces(string, chunk, progress)?;
        let mut level = pieces;
        while level.len() > 2 {
            let mut next = Vec::new();
            next.try_reserve_exact(level.len().div_ceil(2))?;
            for pair in level.chunks(2) {
                next.push(match *pair {
                    [left, right] => self.alloc_rope(left, right)?,
                    [single] => single,
                    _ => return Err(Error::invariant("a pair of rope pieces")),
                });
            }
            level = next;
        }
        let [left, right] = level[..] else {
            // A rope has at least two leaves, which give at least two
            // pieces; flatten if not.
            return self.flatten_with(string, progress);
        };
        let root = self.make_rope(left, right)?;
        if root.len() != old.len() || root.is_latin1() != old.is_latin1() {
            return Err(Error::invariant("a rebalance changed the text"));
        }
        self.arenas.strings.get_mut(string)?.set_rope(root);
        Ok(())
    }

    /// The leaves of a rebalanced rope: the leaves of at least half a
    /// chunk as they are, the runs of shorter leaves joined into new flat
    /// leaves of at least `chunk` units (a run ends before a long leaf
    /// and at the end). At most `5 * CHUNKS + 1` pieces.
    fn rebalance_pieces(
        &mut self,
        string: Gc<JsString>,
        chunk: usize,
        progress: &mut dyn FnMut(usize) -> Result<()>,
    ) -> Result<Vec<Gc<JsString>>> {
        let mut pieces = Vec::new();
        pieces.try_reserve_exact(5 * CHUNKS + 2)?;
        let mut run = String16::new();
        let mut cursor = LeafCursor::new(string);
        let mut work = 0;
        while let Some(leaf) = cursor.next_leaf(&self.arenas.strings)? {
            let text = self.text(leaf)?;
            work += LEAF_COST;
            if text.len() >= chunk / 2 {
                self.end_run(&mut run, &mut pieces)?;
                pieces.push(leaf);
            } else {
                work += text.len();
                run.push_str16(text);
                if run.len() >= chunk {
                    self.end_run(&mut run, &mut pieces)?;
                }
            }
            if work >= PROGRESS_UNITS {
                progress(work)?;
                work = 0;
            }
        }
        self.end_run(&mut run, &mut pieces)?;
        progress(work)?;
        Ok(pieces)
    }

    /// Stores a run of joined leaves as a new flat leaf (if it is not
    /// empty) and adds it to the pieces.
    fn end_run(&mut self, run: &mut String16, pieces: &mut Vec<Gc<JsString>>) -> Result<()> {
        if run.is_empty() {
            return Ok(());
        }
        let text = std::mem::take(run);
        self.check_room(text.capacity() * if text.is_latin1() { 1 } else { 2 })?;
        let leaf = self.alloc_string(text)?;
        pieces.try_reserve(1)?;
        pieces.push(leaf);
        Ok(())
    }

    /// The code units of a flat string. A rope is an internal error:
    /// flatten it first ([`Heap::flatten`], `Runtime::flatten`).
    #[inline]
    pub fn text(&self, string: Gc<JsString>) -> Result<Str16<'_>> {
        self.string(string)?
            .as_flat()
            .ok_or(Error::invariant("a rope where flat text is needed"))
    }

    /// Flattens `string` if it is a rope ([`Heap::flatten`]) and returns its
    /// code units.
    pub fn flat_text(&mut self, string: Gc<JsString>) -> Result<Str16<'_>> {
        self.flatten(string)?;
        self.text(string)
    }

    /// Flattens a rope in place: the string gets its flat text, its
    /// children become garbage (unless something else holds them).
    /// Nothing happens to a flat string. Does not collect: it fails with
    /// [`Termination::HeapLimit`] if the flat text does not fit under the
    /// limit. `Runtime::flatten` reserves first (a safepoint) and charges
    /// the time; prefer it where a safepoint is allowed.
    ///
    /// [`Termination::HeapLimit`]: crate::error::Termination::HeapLimit
    pub fn flatten(&mut self, string: Gc<JsString>) -> Result<()> {
        self.flatten_with(string, &mut |_| Ok(()))
    }

    /// [`Heap::flatten`] that calls `progress` with the work done (in code
    /// units, leaf visits included) about every [`PROGRESS_UNITS`] units,
    /// so that the caller can run the time check. An error from `progress`
    /// ends the flatten; the string stays a rope.
    pub(crate) fn flatten_with(
        &mut self,
        string: Gc<JsString>,
        progress: &mut dyn FnMut(usize) -> Result<()>,
    ) -> Result<()> {
        let Some(rope) = self.string(string)?.as_rope().copied() else {
            return Ok(());
        };
        let len = rope.len();
        let bytes = rope.flat_bytes();
        self.check_room(bytes)?;
        let mut text = String16::try_with_capacity(len, !rope.is_latin1())?;
        let mut work = 0;
        for leaf in Leaves::new(&self.arenas.strings, string) {
            let leaf = leaf?;
            if text.len() + leaf.len() > len {
                return Err(Error::invariant("a rope longer than its length"));
            }
            text.push_str16(leaf);
            work += leaf.len() + LEAF_COST;
            if work >= PROGRESS_UNITS {
                progress(work)?;
                work = 0;
            }
        }
        progress(work)?;
        if text.len() != len {
            return Err(Error::invariant("a rope shorter than its length"));
        }
        self.charge(bytes);
        self.arenas.strings.get_mut(string)?.set_flat(text);
        Ok(())
    }

    /// Whether two strings have the same code units. Ropes are compared
    /// leaf by leaf, without a flatten and without allocation.
    pub fn strings_equal(&self, a: Gc<JsString>, b: Gc<JsString>) -> Result<bool> {
        if a == b {
            return Ok(true);
        }
        let x = self.string(a)?;
        let y = self.string(b)?;
        if x.len() != y.len() {
            return Ok(false);
        }
        if let (Some(x), Some(y)) = (x.as_flat(), y.as_flat()) {
            return Ok(x == y);
        }
        Ok(self.compare_ropes(a, b)? == Ordering::Equal)
    }

    /// The order of two strings by code units. Ropes are compared leaf by
    /// leaf, without a flatten and without allocation.
    pub fn compare_strings(&self, a: Gc<JsString>, b: Gc<JsString>) -> Result<Ordering> {
        if let (Some(x), Some(y)) = (self.string(a)?.as_flat(), self.string(b)?.as_flat()) {
            return Ok(compare_str16(x, y));
        }
        self.compare_ropes(a, b)
    }

    /// [`Heap::compare_strings`] when one of the strings is a rope.
    #[cold]
    #[inline(never)]
    fn compare_ropes(&self, a: Gc<JsString>, b: Gc<JsString>) -> Result<Ordering> {
        let mut left = Leaves::new(&self.arenas.strings, a);
        let mut right = Leaves::new(&self.arenas.strings, b);
        let (mut first, mut second) = (EMPTY, EMPTY);
        loop {
            if first.is_empty()
                && let Some(leaf) = left.next()
            {
                first = leaf?;
            }
            if second.is_empty()
                && let Some(leaf) = right.next()
            {
                second = leaf?;
            }
            match (first.is_empty(), second.is_empty()) {
                (true, true) => return Ok(Ordering::Equal),
                (true, false) => return Ok(Ordering::Less),
                (false, true) => return Ok(Ordering::Greater),
                (false, false) => {}
            }
            let common = first.len().min(second.len());
            let (first_head, first_rest) = split(first, common);
            let (second_head, second_rest) = split(second, common);
            let order = compare_str16(first_head, second_head);
            if order != Ordering::Equal {
                return Ok(order);
            }
            first = first_rest;
            second = second_rest;
        }
    }

    /// A copy of the code units of a string in an exact allocation, leaf
    /// by leaf, without a flatten. The copy is outside the heap: the caller
    /// checks its size like other buffers that a script sizes.
    pub fn copy_string(&self, string: Gc<JsString>) -> Result<String16> {
        let record = self.string(string)?;
        let mut out = String16::try_with_capacity(record.len(), !record.is_latin1())?;
        self.push_string(&mut out, string)?;
        Ok(out)
    }

    /// Appends the code units of a string to `out`, leaf by leaf, without
    /// a flatten (for builders that copy whole strings).
    pub fn push_string(&self, out: &mut String16, string: Gc<JsString>) -> Result<()> {
        self.each_leaf(string, |leaf| out.push_str16(leaf))
    }

    /// The code units of a string in order, without a flatten (for
    /// messages and diagnostics; the walk of a rope is boxed and ends early
    /// at a stale handle).
    pub(crate) fn string_units(&self, string: Gc<JsString>) -> impl Iterator<Item = u16> + '_ {
        match self.string(string).map(JsString::as_flat) {
            Ok(Some(text)) => StringUnits::Flat(text.units()),
            _ => StringUnits::Rope(Box::new(
                Leaves::new(&self.arenas.strings, string)
                    .map_while(Result::ok)
                    .flat_map(Str16::units),
            )),
        }
    }

    /// The first `limit` code units of a string as UTF-8 (unpaired
    /// surrogates become U+FFFD), without a flatten: for messages and
    /// diagnostics. A stale handle gives the empty text.
    pub(crate) fn string_lossy(&self, string: Gc<JsString>, limit: usize) -> String {
        match self.string(string).map(JsString::as_flat) {
            Ok(Some(text)) => text
                .slice(0, limit.min(text.len()))
                .map(Str16::to_string_lossy)
                .unwrap_or_default(),
            Ok(None) => self.rope_lossy(string, limit),
            Err(_) => String::new(),
        }
    }

    /// [`Heap::string_lossy`] of a rope.
    #[cold]
    #[inline(never)]
    fn rope_lossy(&self, string: Gc<JsString>, limit: usize) -> String {
        char::decode_utf16(self.string_units(string).take(limit))
            .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect()
    }

    /// Calls `f` with the leaves of a string in order (a flat string is
    /// one leaf), without a flatten.
    pub(crate) fn each_leaf(
        &self,
        string: Gc<JsString>,
        mut f: impl FnMut(Str16<'_>),
    ) -> Result<()> {
        if let Some(text) = self.string(string)?.as_flat() {
            f(text);
            return Ok(());
        }
        self.each_rope_leaf(string, &mut f)
    }

    /// [`Heap::each_leaf`] of a rope.
    #[cold]
    #[inline(never)]
    fn each_rope_leaf(&self, string: Gc<JsString>, f: &mut dyn FnMut(Str16<'_>)) -> Result<()> {
        for leaf in Leaves::new(&self.arenas.strings, string) {
            f(leaf?);
        }
        Ok(())
    }
}

/// The code units of [`Heap::string_units`]: those of a flat string
/// directly, those of a rope through a walk of its leaves.
enum StringUnits<'a> {
    Flat(swb_js_text::Units<'a>),
    Rope(Box<dyn Iterator<Item = u16> + 'a>),
}

impl Iterator for StringUnits<'_> {
    type Item = u16;

    fn next(&mut self) -> Option<u16> {
        match self {
            StringUnits::Flat(units) => units.next(),
            StringUnits::Rope(units) => units.next(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::{HeapConfig, NoRoots, RootSource, Tracer};

    fn heap() -> Heap {
        Heap::new(HeapConfig::default())
    }

    fn flat(heap: &mut Heap, text: &str) -> Gc<JsString> {
        heap.alloc_str(text).unwrap()
    }

    /// The concatenation as `Runtime::concat` makes it (rebalancing an
    /// operand at the depth limit).
    fn cat(heap: &mut Heap, a: Gc<JsString>, b: Gc<JsString>) -> Gc<JsString> {
        for part in [a, b] {
            if heap.string(part).unwrap().depth() >= MAX_DEPTH {
                heap.rebalance_with(part, &mut |_| Ok(())).unwrap();
                assert!(heap.string(part).unwrap().depth() <= 12);
            }
        }
        heap.concat_strings(a, b).unwrap().string
    }

    fn leaf_count(heap: &Heap, string: Gc<JsString>) -> usize {
        Leaves::new(&heap.arenas.strings, string).count()
    }

    fn content(heap: &Heap, string: Gc<JsString>) -> String16 {
        let mut text = String16::new();
        heap.each_leaf(string, |leaf| text.push_str16(leaf))
            .unwrap();
        text
    }

    fn long(c: char, n: usize) -> String {
        std::iter::repeat_n(c, n).collect()
    }

    #[test]
    fn short_results_are_flat() {
        let mut heap = heap();
        let a = flat(&mut heap, "ab");
        let b = flat(&mut heap, "cd");
        let s = cat(&mut heap, a, b);
        assert!(heap.text(s).unwrap().eq_str("abcd"));
    }

    #[test]
    fn long_results_are_ropes_with_length_and_width() {
        let mut heap = heap();
        let a = flat(&mut heap, &long('a', FLAT_LIMIT));
        let b = flat(&mut heap, "\u{100}");
        let s = cat(&mut heap, a, b);
        let string = heap.string(s).unwrap();
        assert!(string.is_rope());
        assert_eq!(string.len(), FLAT_LIMIT + 1);
        assert!(!string.is_latin1());
        assert!(heap.text(s).is_err());
        let narrow = cat(&mut heap, a, a);
        assert!(heap.string(narrow).unwrap().is_latin1());
    }

    #[test]
    fn appends_fill_leaves_instead_of_chains() {
        let mut heap = heap();
        let mut s = flat(&mut heap, &long('x', 300));
        let ab = flat(&mut heap, "ab");
        for _ in 0..10_000 {
            s = cat(&mut heap, s, ab);
        }
        let string = heap.string(s).unwrap();
        assert_eq!(string.len(), 20_300);
        // One level per filled leaf of FLAT_LIMIT units.
        assert!(string.depth() <= 1 + (20_000 / (FLAT_LIMIT - 2)) as u16 + 1);
        let mut expected = String16::from(long('x', 300).as_str());
        for _ in 0..10_000 {
            expected.push_str16(Str16::Latin1(b"ab"));
        }
        assert_eq!(content(&heap, s), expected);
    }

    #[test]
    fn alternating_appends_and_prepends_stay_shallow() {
        let mut heap = heap();
        let mut s = flat(&mut heap, &long('m', 300));
        let x = flat(&mut heap, "x");
        let y = flat(&mut heap, "y");
        for i in 0..20_000 {
            s = if i % 2 == 0 {
                cat(&mut heap, s, y)
            } else {
                cat(&mut heap, x, s)
            };
        }
        let depth = heap.string(s).unwrap().depth();
        assert!(depth < 200, "depth {depth}");
        let mut expected = String16::from(long('x', 10_000).as_str());
        expected.push_str16(String16::from(long('m', 300).as_str()).as_str16());
        expected.push_str16(String16::from(long('y', 10_000).as_str()).as_str16());
        assert_eq!(content(&heap, s), expected);
        heap.flatten(s).unwrap();
        assert_eq!(heap.text(s).unwrap(), expected.as_str16());
    }

    #[test]
    fn deep_unbalanced_ropes_rebalance_at_the_depth_limit() {
        let mut heap = heap();
        // Pieces longer than FLAT_LIMIT never merge: every append is a
        // node, so the depth reaches the limit and the rebalance runs.
        let pieces: Vec<_> = (0..26u8)
            .map(|i| flat(&mut heap, &long(char::from(b'a' + i), FLAT_LIMIT + 1)))
            .collect();
        let count = 10 * MAX_DEPTH as usize;
        let mut expected = String16::new();
        let mut s = pieces[0];
        expected.push_str16(heap.text(s).unwrap().to_string16().as_str16());
        for i in 1..count {
            let piece = pieces[i % 26];
            s = cat(&mut heap, s, piece);
            expected.push_str16(heap.text(piece).unwrap().to_string16().as_str16());
            assert!(heap.string(s).unwrap().depth() <= MAX_DEPTH);
        }
        assert_eq!(heap.string(s).unwrap().len(), count * (FLAT_LIMIT + 1));
        // The rebalances joined the leaves into chunks.
        assert!(leaf_count(&heap, s) <= 5 * CHUNKS + MAX_DEPTH as usize);
        assert_eq!(content(&heap, s), expected);
        assert_eq!(heap.flat_text(s).unwrap(), expected.as_str16());
        // Right-deep (prepends), with a wide piece.
        let wide = flat(&mut heap, &long('\u{3b1}', FLAT_LIMIT + 1));
        let mut t = wide;
        let mut expected = heap.text(wide).unwrap().to_string16();
        for i in 0..(3 * MAX_DEPTH as usize) {
            let piece = pieces[i % 26];
            t = cat(&mut heap, piece, t);
            let mut next = heap.text(piece).unwrap().to_string16();
            next.push_str16(expected.as_str16());
            expected = next;
        }
        assert!(!heap.string(t).unwrap().is_latin1());
        assert_eq!(content(&heap, t), expected);
        assert_eq!(heap.flat_text(t).unwrap(), expected.as_str16());
    }

    #[test]
    fn rebalance_keeps_the_handle_and_the_holders() {
        let mut heap = heap();
        let piece = flat(&mut heap, &long('q', FLAT_LIMIT + 1));
        let mut s = piece;
        for _ in 1..MAX_DEPTH {
            s = heap.concat_strings(s, piece).unwrap().string;
        }
        let holder = heap.concat_strings(s, piece).unwrap().string;
        assert_eq!(heap.string(holder).unwrap().depth(), MAX_DEPTH);
        heap.rebalance_with(s, &mut |_| Ok(())).unwrap();
        assert!(heap.string(s).unwrap().depth() <= 10);
        let units = MAX_DEPTH as usize * (FLAT_LIMIT + 1);
        assert_eq!(heap.string(s).unwrap().len(), units);
        assert_eq!(content(&heap, s).len(), units);
        // The holder sees the same text through the rebalanced node.
        assert_eq!(content(&heap, holder).len(), units + FLAT_LIMIT + 1);
        // A flat string is left alone.
        heap.rebalance_with(piece, &mut |_| Ok(())).unwrap();
        assert!(!heap.string(piece).unwrap().is_rope());
    }

    #[test]
    fn flatten_of_mixed_widths_keeps_the_units() {
        let mut heap = heap();
        let left = flat(&mut heap, &long('a', FLAT_LIMIT));
        let right = flat(&mut heap, &long('\u{3b1}', FLAT_LIMIT));
        let s = cat(&mut heap, left, right);
        assert!(heap.string(s).unwrap().is_rope());
        assert!(!heap.string(s).unwrap().is_latin1());
        let text = heap.flat_text(s).unwrap();
        assert!(matches!(text, Str16::Wide(_)));
        assert_eq!(text.get(0), Some(0x61));
        assert_eq!(text.get(FLAT_LIMIT - 1), Some(0x61));
        assert_eq!(text.get(FLAT_LIMIT), Some(0x3b1));
        assert_eq!(text.get(2 * FLAT_LIMIT - 1), Some(0x3b1));
        // The flat text stays in the same string.
        assert!(!heap.string(s).unwrap().is_rope());
        assert_eq!(heap.string(s).unwrap().len(), 2 * FLAT_LIMIT);
        // A narrow left part after a flatten keeps its own width.
        assert!(heap.string(left).unwrap().is_latin1());
    }

    #[test]
    fn doubling_shares_children() {
        let mut heap = heap();
        let mut s = flat(&mut heap, &long('d', 200));
        for _ in 0..10 {
            s = cat(&mut heap, s, s);
        }
        assert_eq!(heap.string(s).unwrap().len(), 200 << 10);
        assert!(heap.string(s).unwrap().depth() <= 10);
        assert_eq!(heap.flat_text(s).unwrap().len(), 200 << 10);
    }

    #[test]
    fn equality_and_order_walk_the_leaves() {
        let mut heap = heap();
        let a1 = flat(&mut heap, &long('a', FLAT_LIMIT / 2 + 50));
        let a2 = flat(&mut heap, &long('a', FLAT_LIMIT / 2 + 70));
        let b = flat(&mut heap, &long('a', FLAT_LIMIT + 120));
        let wide = flat(&mut heap, "\u{100}");
        let r1 = cat(&mut heap, a1, a2);
        assert!(heap.string(r1).unwrap().is_rope());
        assert!(heap.strings_equal(r1, b).unwrap());
        assert!(heap.string(r1).unwrap().is_rope(), "no flatten");
        let r2 = cat(&mut heap, r1, wide);
        let c = flat(&mut heap, &long('a', FLAT_LIMIT + 121));
        assert_eq!(heap.compare_strings(r2, c).unwrap(), Ordering::Greater);
        assert_eq!(heap.compare_strings(r1, r2).unwrap(), Ordering::Less);
        assert_eq!(heap.compare_strings(b, r1).unwrap(), Ordering::Equal);
        assert!(!heap.strings_equal(r2, c).unwrap());
    }

    #[test]
    fn flatten_needs_room_under_the_limit() {
        let mut heap = Heap::new(HeapConfig {
            limit: 1 << 20,
            ..HeapConfig::default()
        });
        let mut s = flat(&mut heap, &long('z', 200));
        for _ in 0..13 {
            s = cat(&mut heap, s, s);
        }
        assert_eq!(heap.string(s).unwrap().len(), 200 << 13);
        assert_eq!(
            heap.flatten(s),
            Err(Error::Terminated(crate::error::Termination::HeapLimit))
        );
        assert!(heap.string(s).unwrap().is_rope());
    }

    #[test]
    fn the_collector_keeps_children_through_the_rope() {
        struct One(Gc<JsString>);
        impl RootSource for One {
            fn trace_roots(&self, tracer: &mut Tracer<'_>) {
                tracer.string(self.0);
            }
        }
        let mut heap = heap();
        let mut s = flat(&mut heap, &long('g', 300));
        let piece = flat(&mut heap, &long('h', 300));
        for _ in 0..2_000 {
            s = cat(&mut heap, s, piece);
        }
        heap.collect(&One(s));
        assert_eq!(heap.stats().last_stale_roots, 0);
        let text = heap.flat_text(s).unwrap();
        assert_eq!(text.len(), 300 * 2_001);
        heap.collect(&One(s));
        // The flat string no longer holds its children.
        assert_eq!(heap.counts().strings, 2);
        heap.collect(&NoRoots);
        assert!(!heap.is_live_string(s));
    }
}
