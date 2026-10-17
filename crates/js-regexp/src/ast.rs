//! The tree of a parsed pattern and its analyses (ADR 0026 section 10).
//!
//! The nodes live in one vector. The parser creates a node only after its
//! children, so every child has a smaller index than its parent, and the
//! analyses (the shortest length a node can match, whether it can only
//! match the empty string) are computed when a node is added. Nothing that
//! walks the tree recurses in Rust.
//!
//! The flags `i`, `m` and `s` can change inside a pattern (modifiers,
//! §22.2.2.7.4), so the parser resolves them into the leaves: a character
//! with `i` becomes the set of characters it matches, `^` with `m` is a
//! line start, and so on.

use crate::charset::CharSet;

/// The index of a node.
pub(crate) type NodeId = u32;

/// The maximum of a quantifier without an upper bound.
pub(crate) const INFINITE: u32 = u32::MAX;

/// A zero-width assertion (§22.2.2.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Assertion {
    /// `^` without `m`.
    Start,
    /// `$` without `m`.
    End,
    /// `^` with `m`.
    LineStart,
    /// `$` with `m`.
    LineEnd,
    /// `\b` (or `\B` if `negate`). `extra` adds the two non-ASCII word
    /// characters of `u` or `v` with `i` (`WordCharacters`, §22.2.2.9.3).
    WordBoundary { negate: bool, extra: bool },
}

/// A node of the pattern tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Node {
    /// Matches the empty string.
    Empty,
    /// Exactly this character.
    Char(u32),
    /// A character of the set with this index in [`Ast::sets`].
    Set(u32),
    /// Every character but the line terminators (`.` without `s`).
    AnyButLineTerminator,
    /// Every character.
    Any,
    /// A zero-width assertion.
    Assert(Assertion),
    /// A backreference to the groups (by number, from 1) in the list with
    /// this index of [`Ast::backref_lists`]; at most one of them is set
    /// (§22.2.2.7.2).
    Backref { list: u32, ignore_case: bool },
    /// A group, capturing (with its number, from 1) or not.
    Group { capture: Option<u32>, child: NodeId },
    /// A lookahead or lookbehind.
    Look {
        behind: bool,
        negate: bool,
        child: NodeId,
    },
    /// A quantified atom; its capture groups are `first_group + 1 ..=
    /// first_group + group_count` (parenIndex and parenCount of
    /// `RepeatMatcher`, §22.2.2.3.1).
    Repeat {
        min: u32,
        max: u32,
        greedy: bool,
        child: NodeId,
        first_group: u32,
        group_count: u32,
    },
    /// A sequence of terms.
    Seq(Vec<NodeId>),
    /// Alternatives, tried from left to right.
    Alt(Vec<NodeId>),
}

/// What the analyses know about a node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Info {
    /// The smallest number of characters that the node matches.
    pub(crate) min_len: u32,
    /// Whether the node only ever matches the empty string.
    pub(crate) zero_width: bool,
}

/// A parsed pattern.
#[derive(Debug, Default)]
pub(crate) struct Ast {
    pub(crate) nodes: Vec<Node>,
    pub(crate) info: Vec<Info>,
    /// The node of the whole pattern.
    pub(crate) root: NodeId,
    /// The character sets of [`Node::Set`]: the input characters that
    /// match, after case closure and complement.
    pub(crate) sets: Vec<CharSet>,
    /// The group lists of [`Node::Backref`].
    pub(crate) backref_lists: Vec<Vec<u32>>,
    /// The number of capture groups.
    pub(crate) group_count: u32,
    /// The name of each capture group (index 0 is group 1), as UTF-16.
    pub(crate) group_names: Vec<Option<Vec<u16>>>,
    /// Whether the pattern has `\p{...}` or `\P{...}` (their sets come in
    /// M7 feature 8c).
    pub(crate) uses_properties: bool,
}

impl Ast {
    /// The node with this index.
    pub(crate) fn node(&self, id: NodeId) -> &Node {
        static EMPTY: Node = Node::Empty;
        self.nodes.get(id as usize).unwrap_or(&EMPTY)
    }

    /// The analyses of the node with this index.
    pub(crate) fn info(&self, id: NodeId) -> Info {
        self.info.get(id as usize).copied().unwrap_or(Info {
            min_len: 0,
            zero_width: true,
        })
    }

    /// Adds a node whose children are already in the tree.
    pub(crate) fn push(&mut self, node: Node) -> NodeId {
        let info = self.analyze(&node);
        let id = self.nodes.len() as NodeId;
        self.nodes.push(node);
        self.info.push(info);
        id
    }

    /// Adds a set and returns its node.
    pub(crate) fn push_set(&mut self, set: CharSet) -> NodeId {
        let index = self.sets.len() as u32;
        self.sets.push(set);
        self.push(Node::Set(index))
    }

    fn analyze(&self, node: &Node) -> Info {
        const CHAR: Info = Info {
            min_len: 1,
            zero_width: false,
        };
        const EMPTY: Info = Info {
            min_len: 0,
            zero_width: true,
        };
        match node {
            Node::Empty | Node::Assert(_) | Node::Look { .. } => EMPTY,
            Node::Char(_) | Node::Set(_) | Node::Any | Node::AnyButLineTerminator => CHAR,
            Node::Backref { .. } => Info {
                min_len: 0,
                zero_width: false,
            },
            Node::Group { child, .. } => self.info(*child),
            Node::Repeat {
                min, max, child, ..
            } => {
                let inner = self.info(*child);
                Info {
                    min_len: inner.min_len.saturating_mul(*min),
                    zero_width: *max == 0 || inner.zero_width,
                }
            }
            Node::Seq(items) => items.iter().fold(EMPTY, |acc, &id| {
                let info = self.info(id);
                Info {
                    min_len: acc.min_len.saturating_add(info.min_len),
                    zero_width: acc.zero_width && info.zero_width,
                }
            }),
            Node::Alt(items) => {
                let mut infos = items.iter().map(|&id| self.info(id));
                let first = infos.next().unwrap_or(EMPTY);
                infos.fold(first, |acc, info| Info {
                    min_len: acc.min_len.min(info.min_len),
                    zero_width: acc.zero_width && info.zero_width,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyses_follow_the_children() {
        let mut ast = Ast::default();
        let a = ast.push(Node::Char(0x61));
        let empty = ast.push(Node::Empty);
        let alt = ast.push(Node::Alt(vec![a, empty]));
        assert_eq!(ast.info(alt).min_len, 0);
        assert!(!ast.info(alt).zero_width);
        let star = ast.push(Node::Repeat {
            min: 3,
            max: INFINITE,
            greedy: true,
            child: a,
            first_group: 0,
            group_count: 0,
        });
        assert_eq!(ast.info(star).min_len, 3);
        let look = ast.push(Node::Look {
            behind: false,
            negate: false,
            child: a,
        });
        let seq = ast.push(Node::Seq(vec![look, empty]));
        assert!(ast.info(seq).zero_width);
        let seq = ast.push(Node::Seq(vec![a, star]));
        assert_eq!(ast.info(seq).min_len, 4);
    }
}
