//! Character classes: `ClassContents` without `v` (`NonemptyClassRanges`,
//! §22.2.1, with the Annex B.1.2 forms) and with `v` (`ClassSetExpression`:
//! nested classes, `&&`, `--` and `\q{...}`). Nested classes with `v` use
//! an explicit stack.

use std::collections::BTreeSet;

use super::{MAX_NESTING, Parser, Result};
use crate::ast::NodeId;
use crate::charset::CharSet;
use crate::error::SyntaxKind;

/// One end of a range without `v`: a character or a class escape.
enum ClassAtom {
    Char(u32),
    Set(CharSet),
}

/// A `CharSet` of the `v` flag: single characters and strings of other
/// lengths (§22.2.2.1).
#[derive(Clone, Debug, Default)]
struct ClassSet {
    chars: CharSet,
    strings: BTreeSet<Vec<u32>>,
}

impl ClassSet {
    fn chars(chars: CharSet) -> ClassSet {
        ClassSet {
            chars,
            strings: BTreeSet::new(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SetOp {
    Union,
    Intersection,
    Subtraction,
}

/// An open class with `v`.
struct ClassFrame {
    negate: bool,
    op: SetOp,
    value: ClassSet,
    /// `MayContainStrings` (§22.2.1.8) of the operands so far.
    may_contain_strings: bool,
    operands: u32,
    /// Whether the last operand was a range.
    last_was_range: bool,
    /// Whether an intersection or subtraction needs its operator next.
    needs_operator: bool,
    /// Ranges of union operands that are not yet merged into `value.chars`
    /// (merging after each operand would be quadratic).
    pending: Vec<(u32, u32)>,
}

impl ClassFrame {
    fn new(negate: bool) -> ClassFrame {
        ClassFrame {
            negate,
            op: SetOp::Union,
            value: ClassSet::default(),
            may_contain_strings: false,
            operands: 0,
            last_was_range: false,
            needs_operator: false,
            pending: Vec::new(),
        }
    }

    /// Merges the pending union ranges into the value (once).
    fn flush(&mut self) {
        if !self.pending.is_empty() {
            let pending = std::mem::take(&mut self.pending);
            self.value.chars =
                CharSet::from_ranges(self.value.chars.ranges().iter().copied().chain(pending));
        }
    }

    /// Adds an operand with the operation of the class.
    fn apply(&mut self, operand: ClassSet, may_contain_strings: bool, range: bool) {
        if self.operands == 0 {
            self.value = operand;
            self.may_contain_strings = may_contain_strings;
        } else {
            let value = &mut self.value;
            match self.op {
                SetOp::Union => {
                    self.pending.extend_from_slice(operand.chars.ranges());
                    value.strings.extend(operand.strings);
                    self.may_contain_strings |= may_contain_strings;
                }
                SetOp::Intersection => {
                    // `pending` is empty: an operator follows at most one operand.
                    value.chars = value.chars.intersect(&operand.chars);
                    value.strings.retain(|s| operand.strings.contains(s));
                    self.may_contain_strings &= may_contain_strings;
                }
                SetOp::Subtraction => {
                    value.chars = value.chars.subtract(&operand.chars);
                    value.strings.retain(|s| !operand.strings.contains(s));
                }
            }
            self.needs_operator = self.op != SetOp::Union;
        }
        self.operands += 1;
        self.last_was_range = range;
    }
}

/// Whether two of `c` in a row are a `ClassSetReservedDoublePunctuator`.
fn is_double_punctuator(c: u32) -> bool {
    matches!(
        c,
        0x26 | 0x21
            | 0x23
            | 0x24
            | 0x25
            | 0x2A
            | 0x2B
            | 0x2C
            | 0x2E
            | 0x3A
            | 0x3B
            | 0x3C
            | 0x3D
            | 0x3E
            | 0x3F
            | 0x40
            | 0x5E
            | 0x60
            | 0x7E
    )
}

/// Whether `c` is a `ClassSetReservedPunctuator` (escapable in a class
/// with `v`).
fn is_reserved_punctuator(c: u32) -> bool {
    matches!(
        c,
        0x26 | 0x2D
            | 0x21
            | 0x23
            | 0x25
            | 0x2C
            | 0x3A
            | 0x3B
            | 0x3C
            | 0x3D
            | 0x3E
            | 0x40
            | 0x60
            | 0x7E
    )
}

/// Whether `c` (after `\`) starts a `CharacterClassEscape`.
fn is_class_escape(c: Option<u32>) -> bool {
    matches!(
        c,
        Some(0x64 | 0x44 | 0x73 | 0x53 | 0x77 | 0x57 | 0x70 | 0x50)
    )
}

impl Parser<'_> {
    /// After `[`: `CharacterClass`.
    pub(super) fn class(&mut self) -> Result<NodeId> {
        if self.sets {
            self.class_set_expression()
        } else {
            self.class_ranges()
        }
    }

    /// `ClassContents` without `v`.
    fn class_ranges(&mut self) -> Result<NodeId> {
        let negate = self.c.eat(b'^');
        let mut ranges: Vec<(u32, u32)> = Vec::new();
        let add = |ranges: &mut Vec<(u32, u32)>, atom: ClassAtom| match atom {
            ClassAtom::Char(c) => ranges.push((c, c)),
            ClassAtom::Set(set) => ranges.extend_from_slice(set.ranges()),
        };
        loop {
            match self.c.unit(0) {
                None => return Err(SyntaxKind::UnterminatedCharacterClass),
                Some(0x5D) => {
                    self.c.skip(1);
                    break;
                }
                Some(_) => {}
            }
            let first = self.class_atom()?;
            if self.c.is(0, b'-') && self.c.unit(1).is_some_and(|u| u != 0x5D) {
                self.c.skip(1);
                let second = self.class_atom()?;
                match (first, second) {
                    (ClassAtom::Char(a), ClassAtom::Char(b)) => {
                        if a > b {
                            return Err(SyntaxKind::RangeOutOfOrder);
                        }
                        ranges.push((a, b));
                    }
                    _ if self.unicode => return Err(SyntaxKind::InvalidCharacterClass),
                    // CharacterRangeOrUnion (B.1.2.8.1).
                    (first, second) => {
                        add(&mut ranges, first);
                        add(&mut ranges, second);
                        ranges.push((0x2D, 0x2D));
                    }
                }
            } else {
                add(&mut ranges, first);
            }
        }
        self.set_node(CharSet::from_ranges(ranges), negate)
    }

    /// `ClassAtom` without `v`.
    fn class_atom(&mut self) -> Result<ClassAtom> {
        let c = self
            .c
            .next()
            .ok_or(SyntaxKind::UnterminatedCharacterClass)?;
        if c != 0x5C {
            return Ok(ClassAtom::Char(c));
        }
        let Some(e) = self.c.peek() else {
            return Err(SyntaxKind::EscapeAtEnd);
        };
        match e {
            0x62 => {
                self.c.skip(1);
                Ok(ClassAtom::Char(0x08))
            }
            0x64 | 0x44 | 0x73 | 0x53 | 0x77 | 0x57 => {
                self.c.skip(1);
                Ok(ClassAtom::Set(self.class_escape(e)))
            }
            0x70 | 0x50 if self.unicode => {
                self.c.skip(1);
                Ok(ClassAtom::Set(self.property(e == 0x50, true)?.0))
            }
            _ => Ok(ClassAtom::Char(self.character_escape(true)?)),
        }
    }

    /// `ClassContents` with `v`, up to the `]` of the outermost class.
    fn class_set_expression(&mut self) -> Result<NodeId> {
        let mut frames = vec![ClassFrame::new(self.c.eat(b'^'))];
        loop {
            if frames.len() + self.frames.len() > MAX_NESTING {
                return Err(SyntaxKind::TooLarge);
            }
            match self.c.unit(0) {
                None => return Err(SyntaxKind::UnterminatedCharacterClass),
                Some(0x5D) => {
                    self.c.skip(1);
                    let mut frame = frames.pop().expect("a class frame is open");
                    frame.flush();
                    if frame.negate && frame.may_contain_strings {
                        return Err(SyntaxKind::NegatedClassMayContainStrings);
                    }
                    let (value, may) = if frame.negate {
                        (ClassSet::chars(self.complement(&frame.value.chars)), false)
                    } else {
                        (frame.value, frame.may_contain_strings)
                    };
                    let Some(parent) = frames.last_mut() else {
                        return self.class_set_node(value);
                    };
                    parent.apply(value, may, false);
                    self.no_range_after(parent)?;
                    continue;
                }
                Some(_) => {}
            }
            let frame = frames.last_mut().expect("a class frame is open");
            if self.class_set_operator(frame)? {
                continue;
            }
            if frame.needs_operator {
                return Err(SyntaxKind::InvalidClassSetOperation);
            }
            if self.c.eat(b'[') {
                let negate = self.c.eat(b'^');
                frames.push(ClassFrame::new(negate));
                continue;
            }
            if self.c.starts_with(b"\\q{") {
                self.c.skip(3);
                let (value, may) = self.class_strings()?;
                frame.apply(value, may, false);
                self.no_range_after(frame)?;
                continue;
            }
            if self.c.is(0, b'\\') && is_class_escape(self.c.unit(1)) {
                let e = self.c.unit(1).expect("checked");
                self.c.skip(2);
                let (chars, may) = if e == 0x70 || e == 0x50 {
                    self.property(e == 0x50, true)?
                } else {
                    (self.class_escape(e), false)
                };
                frame.apply(ClassSet::chars(chars), may, false);
                self.no_range_after(frame)?;
                continue;
            }
            self.class_set_range(frame)?;
        }
    }

    /// `&&` or `--` after an operand: checks that the class has one kind
    /// of operation and no ranges in it. Returns whether it read one.
    fn class_set_operator(&mut self, frame: &mut ClassFrame) -> Result<bool> {
        let op = match (self.c.unit(0), self.c.unit(1)) {
            (Some(0x26), Some(0x26)) => SetOp::Intersection,
            (Some(0x2D), Some(0x2D)) => SetOp::Subtraction,
            _ => return Ok(false),
        };
        if frame.operands == 0 {
            return Ok(false);
        }
        if frame.op == SetOp::Union {
            if frame.operands >= 2 || frame.last_was_range {
                return Err(SyntaxKind::InvalidClassSetOperation);
            }
            frame.op = op;
        } else if frame.op != op {
            return Err(SyntaxKind::InvalidClassSetOperation);
        }
        self.c.skip(2);
        if (op == SetOp::Intersection && self.c.is(0, b'&')) || self.c.is(0, b']') {
            return Err(SyntaxKind::InvalidClassSetCharacter);
        }
        frame.needs_operator = false;
        Ok(true)
    }

    /// A `ClassSetCharacter` operand, or a `ClassSetRange` in a union.
    fn class_set_range(&mut self, frame: &mut ClassFrame) -> Result<()> {
        let first = self.class_set_character()?;
        if frame.op != SetOp::Union || !self.c.is(0, b'-') || self.c.is(1, b'-') {
            let chars = self.maybe_fold(CharSet::single(first));
            frame.apply(ClassSet::chars(chars), false, false);
            return Ok(());
        }
        self.c.skip(1);
        let next = self.c.unit(0);
        if next == Some(0x5B) && self.c.unit(1).is_none() {
            return Err(SyntaxKind::UnterminatedCharacterClass);
        }
        if next == Some(0x5B)
            || (next == Some(0x5C) && (is_class_escape(self.c.unit(1)) || self.c.is(1, b'q')))
        {
            return Err(SyntaxKind::InvalidCharacterClass);
        }
        if next.is_none() {
            return Err(SyntaxKind::UnterminatedCharacterClass);
        }
        if next == Some(0x5D) {
            return Err(SyntaxKind::InvalidClassSetCharacter);
        }
        let last = self.class_set_character()?;
        if first > last {
            return Err(SyntaxKind::RangeOutOfOrder);
        }
        let chars = self.maybe_fold(CharSet::range(first, last));
        frame.apply(ClassSet::chars(chars), false, true);
        Ok(())
    }

    /// A class or class escape is not the start of a range.
    fn no_range_after(&self, frame: &ClassFrame) -> Result<()> {
        if frame.op == SetOp::Union && self.c.is(0, b'-') && !self.c.is(1, b'-') {
            if self.c.unit(1).is_none() {
                return Err(SyntaxKind::UnterminatedCharacterClass);
            }
            return Err(SyntaxKind::InvalidCharacterClass);
        }
        Ok(())
    }

    /// `ClassSetCharacter`.
    fn class_set_character(&mut self) -> Result<u32> {
        let Some(c) = self.c.peek() else {
            return Err(SyntaxKind::UnterminatedCharacterClass);
        };
        if is_double_punctuator(c) && self.c.unit(1) == Some(c) {
            return Err(SyntaxKind::InvalidClassSetOperation);
        }
        match c {
            0x28 | 0x29 | 0x5B | 0x5D | 0x7B | 0x7D | 0x2F | 0x2D | 0x7C => {
                Err(SyntaxKind::InvalidClassSetCharacter)
            }
            0x5C => {
                self.c.skip(1);
                match self.c.peek() {
                    None => Err(SyntaxKind::EscapeAtEnd),
                    Some(0x62) => {
                        self.c.skip(1);
                        Ok(0x08)
                    }
                    Some(e) if is_reserved_punctuator(e) => {
                        self.c.skip(1);
                        Ok(e)
                    }
                    Some(_) => self.character_escape(false),
                }
            }
            _ => {
                self.c.next();
                Ok(c)
            }
        }
    }

    /// After `\q{`: `ClassStringDisjunctionContents` and the `}`. Returns the
    /// set and `MayContainStrings`.
    fn class_strings(&mut self) -> Result<(ClassSet, bool)> {
        let mut value = ClassSet::default();
        let mut singles: Vec<(u32, u32)> = Vec::new();
        let mut may = false;
        let mut current: Vec<u32> = Vec::new();
        loop {
            match self.c.unit(0) {
                None => return Err(SyntaxKind::UnterminatedCharacterClass),
                Some(0x7D | 0x7C) => {
                    let end = self.c.is(0, b'}');
                    self.c.skip(1);
                    let s = std::mem::take(&mut current);
                    if let [c] = s[..] {
                        singles.push((c, c));
                    } else {
                        may = true;
                        value.strings.insert(s);
                    }
                    if end {
                        value.chars = CharSet::from_ranges(singles);
                        return Ok((value, may));
                    }
                }
                Some(_) => {
                    let c = self.class_set_character()?;
                    let folded = self.maybe_fold(CharSet::single(c));
                    current.push(folded.single_char().unwrap_or(c));
                }
            }
        }
    }

    /// The node of a class with `v` (`CompileAtom`, §22.2.2.7): the strings
    /// from the longest to the shortest, then the single characters, then
    /// the empty string.
    fn class_set_node(&mut self, value: ClassSet) -> Result<NodeId> {
        if self.validate {
            return Ok(0);
        }
        if value.strings.is_empty() {
            return self.set_node(value.chars, false);
        }
        let mut strings: Vec<Vec<u32>> = value.strings.into_iter().collect();
        strings.sort_by_key(|s| std::cmp::Reverse(s.len()));
        let mut alternatives = Vec::new();
        let mut empty = false;
        for s in strings {
            if s.is_empty() {
                empty = true;
                continue;
            }
            let mut terms = Vec::with_capacity(s.len());
            for c in s {
                terms.push(self.char_node(c)?);
            }
            let seq = self.sequence(terms)?;
            alternatives.push(seq);
        }
        if !value.chars.is_empty() {
            let singles = self.set_node(value.chars, false)?;
            alternatives.push(singles);
        }
        if empty {
            let node = self.node(crate::ast::Node::Empty)?;
            alternatives.push(node);
        }
        self.alternatives(alternatives)
    }
}
