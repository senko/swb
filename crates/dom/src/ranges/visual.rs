//! The caret positions that show the boundary points of a selection, and
//! `Selection.containsNode` over them (ADR 0039 section 3, decision 4).
//!
//! Chromium answers `containsNode` from the positions where the selection is
//! shown, not from its DOM range (measured, cases `collapse-extend` and
//! `contains-node` of `tools/probes/web-selection.json`, three equal runs):
//!
//! - a boundary point between nodes stands for the caret position next to
//!   it: the start of a range moves forward to the first position of the
//!   next text (`(p, 1)` before `<b>two</b>` is `("two", 0)`), the end and a
//!   caret move back to the last position of the previous text (`(p, 1)`
//!   after `"one "` is `("one ", 4)`). Neither crosses the edge of a block
//!   box or a form control. A point between blocks (`(body, 1)` between two
//!   paragraphs) is the start of the next block;
//! - a text node is contained if the moved range reaches into it or touches
//!   it, also when collapsed (`containsNode(text)` is true for a caret inside
//!   or at either end of it, and for a range that ends at its offset 0);
//! - other nodes follow the DOM standard's "contained" and, with partial
//!   containment allowed, "partially contained" over the moved range, so an
//!   element at an edge of the range whose text the range starts in is only
//!   partially contained.
//!
//! The steps walk the tree in tree order and stop at the first text, form
//! control or block edge; the host says how each node takes part in the lines
//! ([`Flow`]).

use std::cmp::Ordering;

use super::{Boundary, LiveRange};
use crate::tree::{Document, NodeData, NodeId};

/// How a node takes part in the lines of text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    /// A block-level element: lines do not continue across its edges.
    Block,
    /// An inline element: its content is on the lines around it.
    Inline,
    /// An inline-level form control or replaced element: one unit of its
    /// line, without positions inside.
    Atomic,
    /// A text node with rendered text.
    Text,
    /// Nothing rendered: comments, text without fragments, elements without a
    /// box (and their content).
    Skipped,
}

impl Flow {
    /// The flow of `node` without rendering information: every element is
    /// inline, every text node with data is rendered.
    pub fn of_dom(doc: &Document, node: NodeId) -> Flow {
        match &doc.node(node).data {
            NodeData::Text(t) if !t.is_empty() => Flow::Text,
            NodeData::Element(_) => Flow::Inline,
            _ => Flow::Skipped,
        }
    }
}

/// A place between nodes: in `parent`, before `next` (at the end without).
#[derive(Clone, Copy)]
struct Cursor {
    parent: NodeId,
    next: Option<NodeId>,
}

/// One step of a walk.
enum Token {
    /// The start of an element (forward: into it; backward: out of it).
    Start(NodeId),
    /// The end of an element (forward: out of it; backward: into it).
    End(NodeId),
    /// A node without children of its own in the walk (character data).
    Leaf(NodeId),
}

impl Document {
    /// The token after `c`, and the cursor after it.
    fn token_after(&self, c: Cursor) -> Option<(Token, Cursor)> {
        match c.next {
            Some(n) if matches!(self.node(n).data, NodeData::Element(_)) => Some((
                Token::Start(n),
                Cursor {
                    parent: n,
                    next: self.node(n).first_child(),
                },
            )),
            Some(n) => Some((
                Token::Leaf(n),
                Cursor {
                    parent: c.parent,
                    next: self.node(n).next_sibling(),
                },
            )),
            None => {
                let parent = self.parent(c.parent)?;
                Some((
                    Token::End(c.parent),
                    Cursor {
                        parent,
                        next: self.node(c.parent).next_sibling(),
                    },
                ))
            }
        }
    }

    /// The token before `c`, and the cursor before it.
    fn token_before(&self, c: Cursor) -> Option<(Token, Cursor)> {
        let prev = match c.next {
            Some(n) => self.node(n).prev_sibling(),
            None => self.node(c.parent).last_child(),
        };
        match prev {
            Some(n) if matches!(self.node(n).data, NodeData::Element(_)) => Some((
                Token::End(n),
                Cursor {
                    parent: n,
                    next: None,
                },
            )),
            Some(n) => Some((
                Token::Leaf(n),
                Cursor {
                    parent: c.parent,
                    next: Some(n),
                },
            )),
            None => {
                let parent = self.parent(c.parent)?;
                Some((
                    Token::Start(c.parent),
                    Cursor {
                        parent,
                        next: Some(c.parent),
                    },
                ))
            }
        }
    }

    /// The boundary point of a cursor.
    fn cursor_point(&self, c: Cursor) -> Boundary {
        let index = match c.next {
            Some(n) => self.index_in_parent(n),
            None => self.node_length(c.parent),
        };
        Boundary::new(c.parent, index)
    }

    /// The cursor before (`after` false) or after a node with a parent.
    fn cursor_beside(&self, node: NodeId, after: bool) -> Option<Cursor> {
        let parent = self.parent(node)?;
        Some(Cursor {
            parent,
            next: if after {
                self.node(node).next_sibling()
            } else {
                Some(node)
            },
        })
    }

    /// The cursor of a boundary point in a node with children.
    fn cursor_at(&self, p: Boundary) -> Cursor {
        Cursor {
            parent: p.node,
            next: self.children(p.node).nth(p.offset as usize),
        }
    }

    /// The most forward boundary point that shows at the same place as `p`
    /// (the start of a range). `p` itself if nothing equivalent follows.
    fn visual_forward(&self, p: Boundary, flow: &dyn Fn(NodeId) -> Flow) -> Boundary {
        let mut cursor = match &self.node(p.node).data {
            NodeData::Text(t) if flow(p.node) == Flow::Text && (p.offset as usize) < t.len() => {
                return p;
            }
            NodeData::Element(_) | NodeData::Document | NodeData::DocumentFragment => {
                self.cursor_at(p)
            }
            NodeData::ShadowRoot(_) => self.cursor_at(p),
            // The end of a text, a point in other character data: after it,
            // or before it for an unrendered node at its start.
            _ => {
                let after = flow(p.node) == Flow::Text || p.offset > 0;
                match self.cursor_beside(p.node, after) {
                    Some(c) => c,
                    None => return p,
                }
            }
        };
        while let Some((token, next)) = self.token_after(cursor) {
            match token {
                Token::Start(e) => match flow(e) {
                    Flow::Atomic => return self.cursor_point(cursor),
                    Flow::Skipped => {
                        cursor = match self.cursor_beside(e, true) {
                            Some(c) => c,
                            None => return p,
                        };
                        continue;
                    }
                    _ => {}
                },
                Token::End(e) => {
                    if flow(e) == Flow::Block {
                        return p;
                    }
                }
                Token::Leaf(n) => {
                    if flow(n) == Flow::Text {
                        return Boundary::new(n, 0);
                    }
                }
            }
            cursor = next;
        }
        p
    }

    /// The most backward boundary point that shows at the same place as `p`
    /// (the end of a range, a caret); `None` if the walk meets the edge of a
    /// block before a text or a form control (`p` is between blocks).
    fn visual_backward(&self, p: Boundary, flow: &dyn Fn(NodeId) -> Flow) -> Option<Boundary> {
        let mut cursor = match &self.node(p.node).data {
            NodeData::Text(_) if flow(p.node) == Flow::Text && p.offset > 0 => return Some(p),
            NodeData::Element(_) | NodeData::Document | NodeData::DocumentFragment => {
                self.cursor_at(p)
            }
            NodeData::ShadowRoot(_) => self.cursor_at(p),
            _ => self.cursor_beside(p.node, false)?,
        };
        while let Some((token, next)) = self.token_before(cursor) {
            match token {
                Token::End(e) => match flow(e) {
                    Flow::Atomic => return Some(self.cursor_point(cursor)),
                    Flow::Block => return None,
                    Flow::Skipped => {
                        cursor = self.cursor_beside(e, false)?;
                        continue;
                    }
                    _ => {}
                },
                Token::Start(e) => {
                    if flow(e) == Flow::Block {
                        return None;
                    }
                }
                Token::Leaf(n) => {
                    if flow(n) == Flow::Text {
                        return Some(Boundary::new(n, self.node_length(n)));
                    }
                }
            }
            cursor = next;
        }
        None
    }

    /// `range` with its boundary points moved to the places that show them:
    /// the start forward, the end back (forward between blocks); a collapsed
    /// range is a caret, back where it can (see the module comment). `flow`
    /// says how each node takes part in the lines.
    pub fn visual_range(&self, range: LiveRange, flow: &dyn Fn(NodeId) -> Flow) -> LiveRange {
        let back = |p: Boundary| {
            self.visual_backward(p, flow)
                .unwrap_or_else(|| self.visual_forward(p, flow))
        };
        if range.is_collapsed() {
            return LiveRange::collapsed_at(back(range.start));
        }
        let start = self.visual_forward(range.start, flow);
        let end = back(range.end);
        if self.compare_boundaries(start, end) == Ordering::Greater {
            // Nothing shows between the two: the range is a caret.
            return LiveRange::collapsed_at(back(range.start));
        }
        LiveRange { start, end }
    }

    /// `Selection.containsNode` of `node` over `range`, a range that
    /// [`Document::visual_range`] moved (see the module comment). `text` is
    /// true for a text node with rendered text. `node` must be in the range's
    /// root.
    pub fn visually_contains(
        &self,
        range: LiveRange,
        node: NodeId,
        text: bool,
        allow_partial: bool,
    ) -> bool {
        if text {
            let len = self.node_length(node);
            return self.compare_boundaries(range.start, Boundary::new(node, len))
                != Ordering::Greater
                && self.compare_boundaries(Boundary::new(node, 0), range.end) != Ordering::Greater;
        }
        let Some(parent) = self.parent(node) else {
            return false;
        };
        let index = self.index_in_parent(node);
        let (before, after) = (
            Boundary::new(parent, index),
            Boundary::new(parent, index + 1),
        );
        let contained = self.compare_boundaries(before, range.start) != Ordering::Less
            && self.compare_boundaries(after, range.end) != Ordering::Greater;
        if contained {
            return true;
        }
        allow_partial
            && self.compare_boundaries(before, range.end) == Ordering::Less
            && self.compare_boundaries(after, range.start) == Ordering::Greater
    }
}
