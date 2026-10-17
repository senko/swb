//! The pattern parser: ECMA-262 §22.2.1 (the grammar with `u` and `v`) and
//! Annex B.1.2 (the web grammar without them), with the early errors of
//! §22.2.1.1 and B.1.2.1 (<https://tc39.es/ecma262/#sec-patterns>,
//! <https://tc39.es/ecma262/#sec-regular-expressions-patterns>).
//!
//! The parser keeps the open groups on an explicit stack and builds the
//! tree of [`crate::ast`] bottom-up, so it never recurses in Rust. Error
//! messages are V8's; where several errors are possible, the parser
//! reports the first one from the left, as V8 does, and the checks that
//! need the whole pattern (unknown group names) at the end.
//!
//! Annex B.1.2.9 parses a pattern without `u` and `v` once without named
//! groups and again with them if the first parse found a group name. The
//! two parses differ only in `\k`, so a scan before the parse decides
//! whether the pattern has a group name. The same scan counts the
//! capture groups, which a decimal escape of the web grammar needs
//! (B.1.2: `\N` is a backreference only if `N` is at most the count).

mod class;
mod cursor;

use std::collections::HashMap;
use std::sync::Arc;

use swb_js_text::Str16;
use swb_js_text::unicode::property::{self, Property};
use swb_js_text::unicode::{self, property_sets};

use crate::Flags;
use crate::ast::{Assertion, Ast, INFINITE, Node, NodeId};
use crate::charset::{self, CaseMode, CharSet, MAX_CODE_POINT, MAX_UNIT};
use crate::error::SyntaxKind;
use cursor::Cursor;

/// The most capture groups a pattern may have (V8 accepts 32,767).
pub(crate) const MAX_CAPTURES: u32 = 32_767;
/// The deepest nesting of groups and classes. V8 accepts deeper nesting
/// of plain groups (1,000,000 levels of `(?:`) but not of lookaheads
/// (100,000 levels: "Regular expression too large") or classes.
pub(crate) const MAX_NESTING: usize = 32_768;
/// The most nodes a tree may have.
pub(crate) const MAX_NODES: usize = 1 << 22;

/// The most ranges in all the sets whose nodes the parser caches.
const MAX_CACHED_RANGES: usize = 1 << 18;
/// The most ranges in all the sets of a tree (8 bytes each).
const MAX_SET_RANGES: usize = 1 << 22;

/// The most strings of a class that the parser compiles as an alternation
/// of sequences; a class with more strings becomes a trie
/// ([`Node::Strings`]).
const MAX_LINEAR_STRINGS: usize = 8;

/// The most characters in all string lists of a pattern (a limit on
/// memory and on the size of the program).
const MAX_STRING_CHARS: usize = 1 << 23;

/// A property as `\p` and `\P` use it: the property, `\P` and the `i` flag.
type PropertyKey = (Property, bool, bool);

type Result<T> = std::result::Result<T, SyntaxKind>;

/// The flags that modifiers can change (§22.2.2.7.4, `UpdateModifiers`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Mods {
    pub(crate) ignore_case: bool,
    pub(crate) multiline: bool,
    pub(crate) dot_all: bool,
}

/// Parses a pattern with its flags.
pub(crate) fn parse(pattern: Str16<'_>, flags: Flags) -> Result<Ast> {
    Parser::new(pattern, flags, false).run()
}

/// Checks a pattern for its early errors without building the tree (for
/// literals in source text, which can be megabytes long).
pub(crate) fn validate(pattern: Str16<'_>, flags: Flags) -> Result<()> {
    Parser::new(pattern, flags, true).run().map(|_| ())
}

/// What the scan before the parse found.
struct Scan {
    groups: u32,
    named: bool,
}

/// Counts the capture groups and looks for a group name: every `(` outside
/// a class that is not followed by `?`, or by `?<` and then not `=` or `!`.
fn scan(text: Str16<'_>, sets: bool) -> Scan {
    let unit = |i: usize| text.get(i).map(u32::from);
    let is = |i: usize, b: u8| unit(i) == Some(u32::from(b));
    let (mut groups, mut named, mut depth) = (0u32, false, 0u32);
    let mut i = 0;
    while let Some(u) = unit(i) {
        match u {
            0x5C => {
                i += 2;
                continue;
            }
            0x5B if depth == 0 || sets => depth += 1,
            0x5D if depth > 0 => depth -= 1,
            0x28 if depth == 0 => {
                if !is(i + 1, b'?') {
                    groups = groups.saturating_add(1);
                } else if is(i + 2, b'<') && !is(i + 3, b'=') && !is(i + 3, b'!') {
                    groups = groups.saturating_add(1);
                    named = true;
                }
            }
            _ => {}
        }
        i += 1;
    }
    Scan { groups, named }
}

/// What a term is, for the quantifier after it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TermKind {
    Atom,
    /// `^`, `$`, `\b`, `\B`: never quantifiable.
    Assertion,
    /// Quantifiable only without `u` and `v` (Annex B.1.2,
    /// `QuantifiableAssertion`).
    Lookahead,
    Lookbehind,
}

/// An open group (or the whole pattern).
struct Frame {
    kind: FrameKind,
    alternatives: Vec<NodeId>,
    terms: Vec<NodeId>,
    /// Where the contents start (in code units).
    start: usize,
    /// Where the current alternative starts.
    alternative_start: usize,
    /// The number of capture groups before this group.
    first_group: u32,
    /// The flags outside the group.
    outer: Mods,
    /// The name of the group repeats the name of a group that can also
    /// participate: an error when the group closes (V8 reports an
    /// unterminated group first).
    duplicate_name: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FrameKind {
    Pattern,
    Capture(u32),
    NonCapture,
    Look { behind: bool, negate: bool },
}

struct Parser<'a> {
    c: Cursor<'a>,
    /// `u` or `v`.
    unicode: bool,
    /// `v`.
    sets: bool,
    /// The `[NamedCaptureGroups]` parameter.
    named: bool,
    mods: Mods,
    ast: Ast,
    /// The capture groups of the whole pattern.
    total_groups: u32,
    frames: Vec<Frame>,
    /// The groups of each name, with the position of their `(`.
    names: HashMap<Vec<u16>, Vec<(u32, usize)>>,
    /// The `\k<name>` references: the index of the group list of each name,
    /// to fill at the end.
    references: HashMap<Vec<u16>, u32>,
    /// The group that opens has a duplicate name (see [`Frame`]).
    duplicate_name: bool,
    /// The node of each small set already seen, by the set, `invert` and
    /// the `i` flag: the case closure is costly and the same escapes and
    /// classes repeat.
    set_nodes: HashMap<(CharSet, bool, bool), Node>,
    /// The ranges in the keys of `set_nodes`.
    cached_ranges: usize,
    /// The set of each property already seen, by the property, `\P` and
    /// the `i` flag.
    property_sets: HashMap<PropertyKey, CharSet>,
    /// The strings of each property of strings already seen, by the
    /// property and the `i` flag: decoded, folded (with `i`), sorted.
    property_string_lists: HashMap<(Property, bool), Arc<[Vec<u32>]>>,
    /// The node of each property outside a class already seen (same key).
    property_nodes: HashMap<PropertyKey, Node>,
    /// Only check the early errors: no nodes and no sets.
    validate: bool,
}

impl<'a> Parser<'a> {
    fn new(pattern: Str16<'a>, flags: Flags, validate: bool) -> Self {
        let sets = flags.contains(Flags::UNICODE_SETS);
        let unicode = sets || flags.contains(Flags::UNICODE);
        let scan = scan(pattern, sets);
        Parser {
            c: Cursor::new(pattern, unicode),
            unicode,
            sets,
            named: unicode || scan.named,
            mods: Mods {
                ignore_case: flags.contains(Flags::IGNORE_CASE),
                multiline: flags.contains(Flags::MULTILINE),
                dot_all: flags.contains(Flags::DOT_ALL),
            },
            ast: Ast::default(),
            total_groups: scan.groups,
            frames: Vec::new(),
            names: HashMap::new(),
            references: HashMap::new(),
            duplicate_name: false,
            set_nodes: HashMap::new(),
            cached_ranges: 0,
            property_sets: HashMap::new(),
            property_string_lists: HashMap::new(),
            property_nodes: HashMap::new(),
            validate,
        }
    }

    fn run(mut self) -> Result<Ast> {
        self.frames.push(Frame {
            kind: FrameKind::Pattern,
            alternatives: Vec::new(),
            terms: Vec::new(),
            start: 0,
            alternative_start: 0,
            first_group: 0,
            outer: self.mods,
            duplicate_name: false,
        });
        while let Some(c) = self.c.peek() {
            match c {
                0x7C => {
                    self.c.skip(1);
                    self.end_alternative()?;
                    let pos = self.c.pos();
                    if let Some(frame) = self.frames.last_mut() {
                        frame.alternative_start = pos;
                    }
                }
                0x29 => {
                    if self.frames.len() <= 1 {
                        return Err(SyntaxKind::UnmatchedParen);
                    }
                    self.c.skip(1);
                    self.close_group()?;
                }
                0x28 => {
                    self.c.skip(1);
                    self.open_group()?;
                }
                _ => {
                    let first_group = self.ast.group_count;
                    let (node, kind) = self.term()?;
                    self.quantified(node, kind, first_group)?;
                }
            }
        }
        if self.frames.len() > 1 {
            return Err(SyntaxKind::UnterminatedGroup);
        }
        self.end_alternative()?;
        let frame = self
            .frames
            .pop()
            .expect("the pattern frame is on the stack");
        self.ast.root = self.alternatives(frame.alternatives)?;
        self.resolve_references()?;
        Ok(self.ast)
    }

    fn node(&mut self, node: Node) -> Result<NodeId> {
        if self.validate {
            return Ok(0);
        }
        if self.ast.nodes.len() >= MAX_NODES {
            return Err(SyntaxKind::TooLarge);
        }
        Ok(self.ast.push(node))
    }

    fn frame(&mut self) -> &mut Frame {
        self.frames
            .last_mut()
            .expect("the pattern frame stays on the stack")
    }

    /// The node of a sequence of terms.
    fn sequence(&mut self, mut terms: Vec<NodeId>) -> Result<NodeId> {
        match terms.len() {
            0 => self.node(Node::Empty),
            1 => Ok(terms.pop().expect("one term")),
            _ => self.node(Node::Seq(terms)),
        }
    }

    /// The node of a list of alternatives.
    fn alternatives(&mut self, mut alternatives: Vec<NodeId>) -> Result<NodeId> {
        match alternatives.len() {
            0 => self.node(Node::Empty),
            1 => Ok(alternatives.pop().expect("one alternative")),
            _ => self.node(Node::Alt(alternatives)),
        }
    }

    fn end_alternative(&mut self) -> Result<()> {
        let terms = std::mem::take(&mut self.frame().terms);
        let node = self.sequence(terms)?;
        self.frame().alternatives.push(node);
        Ok(())
    }

    fn push_frame(&mut self, kind: FrameKind, outer: Mods) -> Result<()> {
        if self.frames.len() >= MAX_NESTING {
            return Err(SyntaxKind::TooLarge);
        }
        let pos = self.c.pos();
        let first_group = match kind {
            FrameKind::Capture(n) => n - 1,
            _ => self.ast.group_count,
        };
        self.frames.push(Frame {
            kind,
            alternatives: Vec::new(),
            terms: Vec::new(),
            start: pos,
            alternative_start: pos,
            first_group,
            outer,
            duplicate_name: std::mem::take(&mut self.duplicate_name),
        });
        Ok(())
    }

    /// After `(`: the kind of group.
    fn open_group(&mut self) -> Result<()> {
        let open = self.c.pos() - 1;
        let outer = self.mods;
        if !self.c.eat(b'?') {
            let n = self.new_group(None, open)?;
            return self.push_frame(FrameKind::Capture(n), outer);
        }
        let kind = match self.c.unit(0) {
            Some(0x3A) => {
                self.c.skip(1);
                FrameKind::NonCapture
            }
            Some(0x3D) => {
                self.c.skip(1);
                FrameKind::Look {
                    behind: false,
                    negate: false,
                }
            }
            Some(0x21) => {
                self.c.skip(1);
                FrameKind::Look {
                    behind: false,
                    negate: true,
                }
            }
            Some(0x3C) if self.c.is(1, b'=') || self.c.is(1, b'!') => {
                let negate = self.c.is(1, b'!');
                self.c.skip(2);
                FrameKind::Look {
                    behind: true,
                    negate,
                }
            }
            Some(0x3C) => {
                self.c.skip(1);
                let name = self.group_name()?;
                let n = self.new_group(Some(name), open)?;
                FrameKind::Capture(n)
            }
            _ => {
                self.modifiers()?;
                FrameKind::NonCapture
            }
        };
        self.push_frame(kind, outer)
    }

    /// `(?ims-ims:` after `(?`: the modifiers and their early errors
    /// (§22.2.1.1), with V8's messages.
    fn modifiers(&mut self) -> Result<()> {
        let (mut add, mut remove, mut dash) = (0u8, 0u8, false);
        loop {
            let bit = match self.c.unit(0) {
                Some(0x69) => 1,
                Some(0x6D) => 2,
                Some(0x73) => 4,
                Some(0x2D) => {
                    if dash {
                        return Err(SyntaxKind::MultipleDashes);
                    }
                    dash = true;
                    self.c.skip(1);
                    continue;
                }
                Some(0x3A) => {
                    if dash && add == 0 && remove == 0 {
                        return Err(SyntaxKind::InvalidFlagGroup);
                    }
                    self.c.skip(1);
                    break;
                }
                _ => return Err(SyntaxKind::InvalidGroup),
            };
            if (add | remove) & bit != 0 {
                return Err(SyntaxKind::RepeatedFlag);
            }
            if dash {
                remove |= bit;
            } else {
                add |= bit;
            }
            self.c.skip(1);
        }
        let apply = |flag: &mut bool, bit: u8| {
            if remove & bit != 0 {
                *flag = false;
            } else if add & bit != 0 {
                *flag = true;
            }
        };
        apply(&mut self.mods.ignore_case, 1);
        apply(&mut self.mods.multiline, 2);
        apply(&mut self.mods.dot_all, 4);
        Ok(())
    }

    /// Counts a new capture group and checks its name.
    fn new_group(&mut self, name: Option<Vec<u16>>, open: usize) -> Result<u32> {
        if self.ast.group_count >= MAX_CAPTURES {
            return Err(SyntaxKind::TooManyCaptures);
        }
        self.ast.group_count += 1;
        let n = self.ast.group_count;
        if let Some(name) = &name {
            if let Some(&(_, previous)) = self.names.get(name).and_then(|groups| groups.last())
                && self.might_both_participate(previous)
            {
                self.duplicate_name = true;
            }
            self.names.entry(name.clone()).or_default().push((n, open));
        }
        self.ast.group_names.push(name);
        Ok(n)
    }

    /// `MightBothParticipate` (§22.2.1.4) of the group that starts here and
    /// an earlier group whose `(` is at `previous`: false if they are in
    /// different alternatives of the innermost disjunction that contains
    /// both. That disjunction is the innermost open group that started
    /// before `previous`; the earlier group is in its current alternative
    /// if it starts after that alternative did. Checking each group
    /// against the last earlier group of the same name is enough: of three
    /// groups in source order, the first and the last can only both
    /// participate if one of the neighbouring pairs can.
    fn might_both_participate(&self, previous: usize) -> bool {
        let index = self.frames.partition_point(|f| f.start <= previous);
        let Some(frame) = index.checked_sub(1).and_then(|i| self.frames.get(i)) else {
            return true;
        };
        previous >= frame.alternative_start
    }

    /// After `)`: closes the innermost group and reads its quantifier.
    fn close_group(&mut self) -> Result<()> {
        self.end_alternative()?;
        let frame = self.frames.pop().expect("a group frame is open");
        if frame.duplicate_name {
            return Err(SyntaxKind::DuplicateCaptureGroupName);
        }
        let child = self.alternatives(frame.alternatives)?;
        self.mods = frame.outer;
        let (node, kind) = match frame.kind {
            FrameKind::Capture(n) => (
                self.node(Node::Group {
                    capture: Some(n),
                    child,
                })?,
                TermKind::Atom,
            ),
            FrameKind::NonCapture | FrameKind::Pattern => (child, TermKind::Atom),
            FrameKind::Look { behind, negate } => (
                self.node(Node::Look {
                    behind,
                    negate,
                    child,
                })?,
                if behind {
                    TermKind::Lookbehind
                } else {
                    TermKind::Lookahead
                },
            ),
        };
        self.quantified(node, kind, frame.first_group)
    }

    /// Reads the quantifier after a term, if any, and adds the term to the
    /// current alternative.
    fn quantified(&mut self, node: NodeId, kind: TermKind, first_group: u32) -> Result<()> {
        if kind == TermKind::Assertion && self.unicode && self.c.is(0, b'{') {
            // A `{` after an assertion is not a quantifier of it.
            let start = self.c.pos();
            let braced = self.braced();
            self.c.reset(start);
            if !matches!(braced, Ok(Some(_)) | Err(_)) {
                return Err(SyntaxKind::LoneQuantifierBrackets);
            }
        }
        let Some((min, max, greedy)) = self.quantifier()? else {
            self.push_term(node);
            return Ok(());
        };
        match kind {
            TermKind::Assertion => return Err(SyntaxKind::NothingToRepeat),
            TermKind::Lookbehind => return Err(SyntaxKind::InvalidQuantifier),
            TermKind::Lookahead if self.unicode => return Err(SyntaxKind::InvalidQuantifier),
            TermKind::Atom | TermKind::Lookahead => {}
        }
        let group_count = self.ast.group_count - first_group;
        let node = self.node(Node::Repeat {
            min,
            max,
            greedy,
            child: node,
            first_group,
            group_count,
        })?;
        self.push_term(node);
        Ok(())
    }

    fn push_term(&mut self, node: NodeId) {
        if !self.validate {
            self.frame().terms.push(node);
        }
    }

    /// A quantifier (§22.2.1, Quantifier): `(min, max, greedy)`.
    fn quantifier(&mut self) -> Result<Option<(u32, u32, bool)>> {
        let (min, max) = match self.c.unit(0) {
            Some(0x2A) => {
                self.c.skip(1);
                (0, INFINITE)
            }
            Some(0x2B) => {
                self.c.skip(1);
                (1, INFINITE)
            }
            Some(0x3F) => {
                self.c.skip(1);
                (0, 1)
            }
            Some(0x7B) => match self.braced()? {
                Some(bounds) => bounds,
                None if self.unicode => return Err(SyntaxKind::IncompleteQuantifier),
                None => return Ok(None),
            },
            _ => return Ok(None),
        };
        let greedy = !self.c.eat(b'?');
        Ok(Some((min, max, greedy)))
    }

    /// `{n}`, `{n,}` or `{n,m}` at a `{`; `None` (and nothing read) if the
    /// text does not have this form. Counts above 2^32 - 2 are treated as
    /// unbounded: no string is that long.
    fn braced(&mut self) -> Result<Option<(u32, u32)>> {
        let start = self.c.pos();
        self.c.skip(1);
        let low = self.c.digits();
        if low.0 == low.1 {
            self.c.reset(start);
            return Ok(None);
        }
        let count = |value: u64, is_max: bool| match u32::try_from(value) {
            Ok(v) if v < INFINITE => v,
            _ if is_max => INFINITE,
            _ => INFINITE - 1,
        };
        let min = count(self.c.number(low.0, low.1), false);
        if self.c.eat(b'}') {
            let max = count(self.c.number(low.0, low.1), true);
            return Ok(Some((min, max.max(min))));
        }
        if !self.c.eat(b',') {
            self.c.reset(start);
            return Ok(None);
        }
        if self.c.eat(b'}') {
            return Ok(Some((min, INFINITE)));
        }
        let high = self.c.digits();
        if high.0 == high.1 || !self.c.eat(b'}') {
            self.c.reset(start);
            return Ok(None);
        }
        if self.c.compare_numbers(low, high) == std::cmp::Ordering::Greater {
            return Err(SyntaxKind::QuantifierOutOfOrder);
        }
        let max = count(self.c.number(high.0, high.1), true);
        Ok(Some((min, max.max(min))))
    }

    /// A term that does not start with `(`, `)` or `|`: an assertion or an
    /// atom other than a group.
    fn term(&mut self) -> Result<(NodeId, TermKind)> {
        let c = self.c.peek().expect("the caller checked for the end");
        let node = match c {
            0x5E => {
                self.c.skip(1);
                let a = if self.mods.multiline {
                    Assertion::LineStart
                } else {
                    Assertion::Start
                };
                return Ok((self.node(Node::Assert(a))?, TermKind::Assertion));
            }
            0x24 => {
                self.c.skip(1);
                let a = if self.mods.multiline {
                    Assertion::LineEnd
                } else {
                    Assertion::End
                };
                return Ok((self.node(Node::Assert(a))?, TermKind::Assertion));
            }
            0x5C => {
                self.c.skip(1);
                return self.atom_escape();
            }
            0x5B => {
                self.c.skip(1);
                self.class()?
            }
            0x2E => {
                self.c.skip(1);
                if self.mods.dot_all {
                    self.node(Node::Any)?
                } else {
                    self.node(Node::AnyButLineTerminator)?
                }
            }
            0x2A | 0x2B | 0x3F => return Err(SyntaxKind::NothingToRepeat),
            0x7B => {
                // InvalidBracedQuantifier (B.1.2.1) or a lone `{`.
                let braced = matches!(self.braced(), Ok(Some(_)) | Err(_));
                if braced {
                    return Err(SyntaxKind::NothingToRepeat);
                }
                if self.unicode {
                    return Err(SyntaxKind::LoneQuantifierBrackets);
                }
                self.c.skip(1);
                self.char_node(c)?
            }
            0x7D | 0x5D if self.unicode => return Err(SyntaxKind::LoneQuantifierBrackets),
            _ => {
                self.c.next();
                self.char_node(c)?
            }
        };
        Ok((node, TermKind::Atom))
    }

    /// After `\` outside a class: `AtomEscape` (§22.2.1, B.1.2) or `\b`,
    /// `\B`.
    fn atom_escape(&mut self) -> Result<(NodeId, TermKind)> {
        let Some(c) = self.c.peek() else {
            return Err(SyntaxKind::EscapeAtEnd);
        };
        let node = match c {
            0x62 | 0x42 => {
                self.c.skip(1);
                let a = Assertion::WordBoundary {
                    negate: c == 0x42,
                    extra: self.unicode && self.mods.ignore_case,
                };
                return Ok((self.node(Node::Assert(a))?, TermKind::Assertion));
            }
            0x31..=0x39 => {
                let digits = self.c.digits();
                let n = self.c.number(digits.0, digits.1);
                if n <= u64::from(self.total_groups) {
                    self.backref(vec![n as u32])?
                } else if self.unicode {
                    return Err(SyntaxKind::InvalidEscape);
                } else {
                    self.c.reset(digits.0);
                    let ch = self.legacy_digit_escape();
                    self.char_node(ch)?
                }
            }
            0x6B if self.named => {
                self.c.skip(1);
                if !self.c.eat(b'<') {
                    return Err(SyntaxKind::InvalidNamedReference);
                }
                let name = self.group_name()?;
                // References to one name share one list.
                let next = self.ast.backref_lists.len() as u32;
                let list = *self.references.entry(name).or_insert(next);
                if list == next {
                    self.ast.backref_lists.push(Vec::new());
                }
                self.node(Node::Backref {
                    list,
                    ignore_case: self.mods.ignore_case,
                })?
            }
            0x64 | 0x44 | 0x73 | 0x53 | 0x77 | 0x57 => {
                self.c.skip(1);
                let set = self.class_escape(c);
                self.set_node(set, false)?
            }
            0x70 | 0x50 if self.unicode => {
                self.c.skip(1);
                let property = self.property(c == 0x50, false)?;
                self.property_atom(property, c == 0x50)?
            }
            _ => {
                let ch = self.character_escape(false)?;
                self.char_node(ch)?
            }
        };
        Ok((node, TermKind::Atom))
    }

    /// A backreference to these groups.
    fn backref(&mut self, groups: Vec<u32>) -> Result<NodeId> {
        let list = self.ast.backref_lists.len() as u32;
        self.ast.backref_lists.push(groups);
        self.node(Node::Backref {
            list,
            ignore_case: self.mods.ignore_case,
        })
    }

    /// After `\` at a digit without `u` and `v`, where the digits are not a
    /// backreference: `LegacyOctalEscapeSequence` (B.1.2, §12.9.4) for `0`
    /// to `7`, the digit itself for `8` and `9` (`IdentityEscape`).
    fn legacy_digit_escape(&mut self) -> u32 {
        let octal = |u: Option<u32>| u.filter(|u| (0x30..=0x37).contains(u)).map(|u| u - 0x30);
        let Some(d0) = octal(self.c.unit(0)) else {
            return self.c.next().unwrap_or(0x5C);
        };
        self.c.skip(1);
        let Some(d1) = octal(self.c.unit(0)) else {
            return d0;
        };
        self.c.skip(1);
        let value = d0 * 8 + d1;
        if d0 <= 3
            && let Some(d2) = octal(self.c.unit(0))
        {
            self.c.skip(1);
            return value * 8 + d2;
        }
        value
    }

    /// After `\`: `CharacterEscape` (§22.2.1, B.1.2) and the escapes of a
    /// class that are characters. `in_class` allows `\-` with `u` and
    /// `\cN`, `\c_` without `u`.
    fn character_escape(&mut self, in_class: bool) -> Result<u32> {
        let Some(c) = self.c.peek() else {
            return Err(SyntaxKind::EscapeAtEnd);
        };
        let control = |c: u32| match c {
            0x66 => Some(0x0C),
            0x6E => Some(0x0A),
            0x72 => Some(0x0D),
            0x74 => Some(0x09),
            0x76 => Some(0x0B),
            _ => None,
        };
        if let Some(value) = control(c) {
            self.c.skip(1);
            return Ok(value);
        }
        match c {
            0x63 => {
                let next = self.c.unit(1);
                let letter = next.filter(|&u| u < 0x80 && (u as u8).is_ascii_alphabetic());
                let class_letter =
                    next.filter(|&u| u < 0x80 && ((u as u8).is_ascii_digit() || u == 0x5F));
                if let Some(u) = letter {
                    self.c.skip(2);
                    Ok(u % 32)
                } else if let Some(u) = class_letter.filter(|_| in_class && !self.unicode) {
                    self.c.skip(2);
                    Ok(u % 32)
                } else if self.unicode {
                    Err(SyntaxKind::InvalidUnicodeEscape)
                } else {
                    // `\` before a `c` that does not start a control escape
                    // is itself (B.1.2, ExtendedAtom :: \ [lookahead = c]).
                    Ok(0x5C)
                }
            }
            0x30 if !self.c.unit(1).is_some_and(|u| (0x30..=0x39).contains(&u)) => {
                self.c.skip(1);
                Ok(0)
            }
            // V8 names `\8` and `\9` invalid escapes, the octal digits
            // invalid decimal escapes.
            0x30..=0x37 if self.unicode => Err(SyntaxKind::InvalidDecimalEscape),
            0x30..=0x39 if !self.unicode => Ok(self.legacy_digit_escape()),
            0x78 => {
                self.c.skip(1);
                match self.c.hex_digits(2) {
                    Some(value) => Ok(value),
                    None if self.unicode => Err(SyntaxKind::InvalidEscape),
                    None => Ok(0x78),
                }
            }
            0x75 => {
                self.c.skip(1);
                match self.unicode_escape(self.unicode) {
                    Some(value) => Ok(value),
                    None if self.unicode => Err(SyntaxKind::InvalidUnicodeEscape),
                    None => Ok(0x75),
                }
            }
            _ if self.unicode => {
                let syntax = matches!(
                    c,
                    0x5E | 0x24
                        | 0x5C
                        | 0x2E
                        | 0x2A
                        | 0x2B
                        | 0x3F
                        | 0x28
                        | 0x29
                        | 0x5B
                        | 0x5D
                        | 0x7B
                        | 0x7D
                        | 0x7C
                        | 0x2F
                );
                if syntax || (in_class && c == 0x2D) {
                    self.c.skip(1);
                    Ok(c)
                } else {
                    Err(SyntaxKind::InvalidEscape)
                }
            }
            0x6B if self.named => Err(SyntaxKind::InvalidEscape),
            _ => {
                self.c.next();
                Ok(c)
            }
        }
    }

    /// After `\u`: `RegExpUnicodeEscapeSequence` (§22.2.1). With `unicode`,
    /// `\u{...}` and a pair of `\uXXXX` surrogate escapes form one code
    /// point. Reads nothing and returns `None` if the text is not an
    /// escape.
    fn unicode_escape(&mut self, unicode: bool) -> Option<u32> {
        let start = self.c.pos();
        if unicode && self.c.eat(b'{') {
            let mut value: u32 = 0;
            let mut any = false;
            while let Some(d) = self.c.hex_digit() {
                any = true;
                value = value.saturating_mul(16).saturating_add(d);
            }
            if any && value <= MAX_CODE_POINT && self.c.eat(b'}') {
                return Some(value);
            }
            self.c.reset(start);
            return None;
        }
        let Some(value) = self.c.hex_digits(4) else {
            self.c.reset(start);
            return None;
        };
        if unicode && (0xD800..=0xDBFF).contains(&value) && self.c.starts_with(b"\\u") {
            let after_lead = self.c.pos();
            self.c.skip(2);
            match self.c.hex_digits(4) {
                Some(trail) if (0xDC00..=0xDFFF).contains(&trail) => {
                    return Some(0x10000 + ((value - 0xD800) << 10) + (trail - 0xDC00));
                }
                _ => self.c.reset(after_lead),
            }
        }
        Some(value)
    }

    /// After `<`: a group name and the `>` after it (`GroupName`, §22.2.1),
    /// as UTF-16.
    fn group_name(&mut self) -> Result<Vec<u16>> {
        let mut name = Vec::new();
        let mut first = true;
        loop {
            let Some(mut cp) = self.c.next() else {
                return Err(SyntaxKind::InvalidCaptureGroupName);
            };
            if cp == 0x3E && !first {
                break;
            }
            if cp == 0x5C {
                if !self.c.eat(b'u') {
                    return Err(SyntaxKind::InvalidCaptureGroupName);
                }
                cp = self
                    .unicode_escape(true)
                    .ok_or(SyntaxKind::InvalidUnicodeEscape)?;
            } else if !self.unicode && (0xD800..=0xDBFF).contains(&cp) {
                // A surrogate pair in the pattern text of the web grammar
                // (RegExpIdentifierStart :: UnicodeLeadSurrogate
                // UnicodeTrailSurrogate).
                if let Some(trail) = self.c.peek().filter(|t| (0xDC00..=0xDFFF).contains(t)) {
                    self.c.next();
                    cp = 0x10000 + ((cp - 0xD800) << 10) + (trail - 0xDC00);
                }
            }
            if !unicode::is_identifier_char(cp, first) {
                return Err(SyntaxKind::InvalidCaptureGroupName);
            }
            push_utf16(&mut name, cp);
            first = false;
        }
        Ok(name)
    }

    /// Fills the group lists of the `\k<name>` references (§22.2.1.1: the
    /// name must belong to a group).
    fn resolve_references(&mut self) -> Result<()> {
        for (name, list) in std::mem::take(&mut self.references) {
            let groups = self
                .names
                .get(&name)
                .ok_or(SyntaxKind::InvalidNamedCaptureReferenced)?;
            if let Some(slot) = self.ast.backref_lists.get_mut(list as usize) {
                *slot = groups.iter().map(|&(n, _)| n).collect();
            }
        }
        Ok(())
    }

    /// The case mapping of `i` in this pattern.
    fn case_mode(&self) -> CaseMode {
        if self.unicode {
            CaseMode::Fold
        } else {
            CaseMode::Upper
        }
    }

    /// The largest character value.
    fn max_char(&self) -> u32 {
        if self.unicode {
            MAX_CODE_POINT
        } else {
            MAX_UNIT
        }
    }

    /// `AllCharacters` (§22.2.2.9.4).
    fn all_characters(&self) -> CharSet {
        if self.sets && self.mods.ignore_case {
            charset::fold_fixed_points().clone()
        } else {
            CharSet::range(0, self.max_char())
        }
    }

    /// `CharacterComplement` (§22.2.2.9.6).
    fn complement(&self, set: &CharSet) -> CharSet {
        self.all_characters().subtract(set)
    }

    /// `MaybeSimpleCaseFolding` (§22.2.2.9.5).
    fn maybe_fold(&self, set: CharSet) -> CharSet {
        if self.sets && self.mods.ignore_case {
            charset::simple_fold_set(&set)
        } else {
            set
        }
    }

    /// The set of `\d`, `\D`, `\s`, `\S`, `\w` or `\W` (`CompileToCharSet`,
    /// §22.2.2.9).
    fn class_escape(&self, c: u32) -> CharSet {
        let set = match c | 0x20 {
            0x64 => CharSet::range(0x30, 0x39),
            0x73 => CharSet::from_ranges(
                [
                    (0x09, 0x0D),
                    (0x20, 0x20),
                    (0xA0, 0xA0),
                    (0x2028, 0x2029),
                    (0xFEFF, 0xFEFF),
                ]
                .into_iter()
                .chain(unicode::space_separators().iter().copied()),
            ),
            _ => self.maybe_fold(self.word_characters()),
        };
        if c & 0x20 == 0 {
            self.complement(&set)
        } else {
            set
        }
    }

    /// `WordCharacters` (§22.2.2.9.3): the ASCII word characters and, with
    /// `u` or `v` and `i`, U+017F and U+212A, which fold to `s` and `k`.
    fn word_characters(&self) -> CharSet {
        let mut ranges = vec![(0x30, 0x39), (0x41, 0x5A), (0x5F, 0x5F), (0x61, 0x7A)];
        if self.unicode && self.mods.ignore_case {
            ranges.extend([(0x17F, 0x17F), (0x212A, 0x212A)]);
        }
        CharSet::from_ranges(ranges)
    }

    /// After `\p` or `\P`: `{name=value}` or `{name}`. Checks the names
    /// (§22.2.1.1) and returns the property.
    fn property(&mut self, negate: bool, in_class: bool) -> Result<Property> {
        let error = if in_class {
            SyntaxKind::InvalidClassPropertyName
        } else {
            SyntaxKind::InvalidPropertyName
        };
        if !self.c.eat(b'{') {
            return Err(error);
        }
        let word = |p: &mut Self| {
            let mut s = String::new();
            while let Some(u) =
                p.c.unit(0)
                    .filter(|&u| u < 0x80 && ((u as u8).is_ascii_alphanumeric() || u == 0x5F))
            {
                s.push(char::from(u as u8));
                p.c.skip(1);
            }
            s
        };
        let name = word(self);
        let value = if self.c.eat(b'=') {
            Some(word(self))
        } else {
            None
        };
        if !self.c.eat(b'}') || name.is_empty() || value.as_ref().is_some_and(String::is_empty) {
            return Err(error);
        }
        if value.is_some() && name.bytes().any(|b| b.is_ascii_digit()) {
            return Err(error);
        }
        let found = property::lookup(&name, value.as_deref()).ok_or(error)?;
        if found.has_strings() && (!self.sets || negate) {
            return Err(error);
        }
        Ok(found)
    }

    /// The characters and strings of `\p{...}` or `\P{...}` (§22.2.2.9):
    /// the set of the property with `MaybeSimpleCaseFolding` (with `v` and
    /// `i`), or its complement (`CharacterComplement`, for `\P`). The
    /// strings are those of a property of strings. The sets of the
    /// properties are built once per pattern and kept.
    ///
    /// Deliberate difference from §22.2.2.9, which leaves a lone
    /// `General_Category` value (`\p{Lu}`) unfolded: Chromium 148 folds it
    /// like every other property (`/\P{Lu}/iv` does not match `A`), and so
    /// does swb.
    fn property_value(&mut self, property: Property, negate: bool) -> CharSet {
        if self.validate {
            return CharSet::new();
        }
        let key = (property, negate, self.mods.ignore_case);
        if let Some(set) = self.property_sets.get(&key) {
            return set.clone();
        }
        let mut set = CharSet::from_ranges(property_sets::ranges(property));
        set = self.maybe_fold(set);
        if negate {
            set = self.complement(&set);
        }
        set.shrink_to_fit();
        self.property_sets.insert(key, set.clone());
        set
    }

    /// The strings of a property of strings (with `v`): sorted, without
    /// duplicates, and case folded with `i` (`MaybeSimpleCaseFolding`).
    /// Decoding and folding them costs milliseconds, so they are kept.
    fn property_strings(&mut self, property: Property) -> Arc<[Vec<u32>]> {
        if self.validate {
            return Arc::from(Vec::new());
        }
        let fold = self.sets && self.mods.ignore_case;
        if let Some(list) = self.property_string_lists.get(&(property, fold)) {
            return Arc::clone(list);
        }
        let mut strings = property_sets::strings(property);
        if fold {
            for string in &mut strings {
                for c in string.iter_mut() {
                    *c = charset::canonicalize(CaseMode::Fold, *c);
                }
            }
            strings.sort_unstable();
            strings.dedup();
        }
        let list: Arc<[Vec<u32>]> = strings.into();
        self.property_string_lists
            .insert((property, fold), Arc::clone(&list));
        list
    }

    /// The node of `\p{...}` or `\P{...}` outside a class. The node of
    /// a property without strings is kept: the case closure of a large
    /// set costs milliseconds.
    fn property_atom(&mut self, property: Property, negate: bool) -> Result<NodeId> {
        if self.validate {
            return self.node(Node::Empty);
        }
        if property.has_strings() {
            let chars = self.property_value(property, negate);
            let strings = self.property_strings(property);
            if strings.len() > MAX_LINEAR_STRINGS {
                return self.strings_node(chars, strings);
            }
            return self.class_set_node(class::ClassSet {
                chars,
                strings: strings.iter().cloned().collect(),
            });
        }
        let key = (property, negate, self.mods.ignore_case);
        if let Some(node) = self.property_nodes.get(&key) {
            return self.node(node.clone());
        }
        let set = self.property_value(property, negate);
        let id = self.set_node(set, false)?;
        self.property_nodes.insert(key, self.ast.node(id).clone());
        Ok(id)
    }

    /// The node of one character: with `i`, the set of characters that
    /// match it.
    fn char_node(&mut self, c: u32) -> Result<NodeId> {
        if self.mods.ignore_case && !self.validate {
            let set = charset::case_closure(self.case_mode(), &CharSet::single(c));
            if set.single_char() != Some(c) {
                return self.special_set(set);
            }
        }
        self.node(Node::Char(c))
    }

    /// The node of a class of single characters (`CharacterSetMatcher`,
    /// §22.2.2.7.1): with `i`, the characters whose canonical form is that
    /// of a member; with `invert`, the others.
    fn set_node(&mut self, set: CharSet, invert: bool) -> Result<NodeId> {
        if self.validate {
            return Ok(0);
        }
        let key = (set, invert, self.mods.ignore_case);
        if let Some(node) = self.set_nodes.get(&key) {
            return self.node(node.clone());
        }
        let mut closed = if self.mods.ignore_case {
            charset::case_closure(self.case_mode(), &key.0)
        } else {
            key.0.clone()
        };
        if invert {
            closed = closed.complement(self.max_char());
        }
        let id = self.special_set(closed)?;
        if self.cached_ranges + key.0.ranges().len() <= MAX_CACHED_RANGES {
            self.cached_ranges += key.0.ranges().len();
            self.set_nodes.insert(key, self.ast.node(id).clone());
        }
        Ok(id)
    }

    /// The node of a set of input characters.
    fn special_set(&mut self, set: CharSet) -> Result<NodeId> {
        let max = self.max_char();
        if let Some(c) = set.single_char() {
            return self.node(Node::Char(c));
        }
        if set.ranges() == [(0, max)] {
            return self.node(Node::Any);
        }
        if set.ranges() == [(0, 0x09), (0x0B, 0x0C), (0x0E, 0x2027), (0x202A, max)] {
            return self.node(Node::AnyButLineTerminator);
        }
        if self.ast.nodes.len() >= MAX_NODES
            || self.ast.set_ranges + set.ranges().len() > MAX_SET_RANGES
        {
            return Err(SyntaxKind::TooLarge);
        }
        let mut set = set;
        set.shrink_to_fit();
        Ok(self.ast.push_set(set))
    }
}

/// Appends a code point as UTF-16.
fn push_utf16(out: &mut Vec<u16>, cp: u32) {
    match char::from_u32(cp) {
        Some(ch) => {
            let mut buf = [0u16; 2];
            out.extend_from_slice(ch.encode_utf16(&mut buf));
        }
        None => out.push(cp as u16),
    }
}
