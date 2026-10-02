//! Text selection: positions in text nodes, the position at a point,
//! words and blocks, the highlight, and the selected text.
//!
//! A selection is a range between two positions in text nodes. It refers
//! to the DOM, not to the layout, so it survives a new layout (a restyle
//! on hover, a resize).

use std::cmp::Ordering;
use std::collections::HashMap;

use swb_dom::{Document, NodeData, NodeId, local_name};
use swb_layout::{BoxFragment, Fragment, FragmentTree, Point, Rect, TextFragment};
use swb_style::{ComputedStyle, Display, StyleMap, UserSelect, Visibility, WhiteSpace};
use unicode_segmentation::UnicodeSegmentation;

/// A position in a text node: a byte offset in its data, at a character
/// boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextPosition {
    /// The text node.
    pub node: NodeId,
    /// The byte offset in the node's data.
    pub offset: usize,
}

/// A text selection from `anchor` (where it started) to `focus` (where it
/// ends). The focus can be before the anchor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    /// Where the selection started.
    pub anchor: TextPosition,
    /// Where the selection ends.
    pub focus: TextPosition,
}

impl Selection {
    /// A selection from `anchor` to `focus`.
    pub fn new(anchor: TextPosition, focus: TextPosition) -> Self {
        Selection { anchor, focus }
    }

    /// True if the selection is empty.
    pub fn is_collapsed(&self) -> bool {
        self.anchor == self.focus
    }

    /// The start and the end of the selection in tree order.
    pub(crate) fn ordered(&self, order: &TreeOrder) -> (TextPosition, TextPosition) {
        if order.compare(self.anchor, self.focus) == Ordering::Greater {
            (self.focus, self.anchor)
        } else {
            (self.anchor, self.focus)
        }
    }
}

/// The position of each node in tree order. The document does not change
/// after parsing, so this is computed once per document.
#[derive(Clone, Debug, Default)]
pub(crate) struct TreeOrder {
    rank: Vec<u32>,
}

impl TreeOrder {
    pub(crate) fn new(doc: &Document) -> Self {
        let mut rank = vec![u32::MAX; doc.len()];
        for (i, node) in doc.descendants(NodeId::DOCUMENT).enumerate() {
            if let Some(r) = rank.get_mut(node.index()) {
                *r = u32::try_from(i).unwrap_or(u32::MAX);
            }
        }
        TreeOrder { rank }
    }

    /// The node's position in tree order; detached nodes come last.
    fn rank(&self, node: NodeId) -> u32 {
        self.rank.get(node.index()).copied().unwrap_or(u32::MAX)
    }

    pub(crate) fn compare(&self, a: TextPosition, b: TextPosition) -> Ordering {
        self.rank(a.node)
            .cmp(&self.rank(b.node))
            .then(a.offset.cmp(&b.offset))
    }
}

/// The selected part of each text node, for painting.
pub(crate) struct Highlight<'a> {
    pub(crate) start: TextPosition,
    pub(crate) end: TextPosition,
    pub(crate) order: &'a TreeOrder,
}

impl swb_paint::Highlights for Highlight<'_> {
    fn selected(&self, node: NodeId) -> Option<(u32, u32)> {
        let rank = self.order.rank(node);
        if rank < self.order.rank(self.start.node) || rank > self.order.rank(self.end.node) {
            return None;
        }
        let to_u32 = |v: usize| u32::try_from(v).unwrap_or(u32::MAX);
        let from = if node == self.start.node {
            to_u32(self.start.offset)
        } else {
            0
        };
        let to = if node == self.end.node {
            to_u32(self.end.offset)
        } else {
            u32::MAX
        };
        (from < to).then_some((from, to))
    }
}

/// How [`position_at`] picks the position in the text at a point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Snap {
    /// The caret stop nearest to the point (for selecting with the mouse).
    Nearest,
    /// The start of the character under the point (for selecting words).
    Character,
}

/// The text position at `point` (document coordinates). The text is
/// searched in the innermost block that contains the point and has text
/// (the whole document if there is none): the nearest line, then the
/// nearest fragment on it.
pub(crate) fn position_at(tree: &FragmentTree, point: Point, snap: Snap) -> Option<TextPosition> {
    let root = tree.root.as_ref()?;
    let mut scope = None;
    find_text_block(root, Point::default(), point, &mut scope);
    let (block, origin) = scope.unwrap_or((root, Point::default()));
    let mut best: Option<((f32, f32), &TextFragment, Rect)> = None;
    walk_text(block, origin, &mut |fragment, rect| {
        let key = (
            distance(point.y, rect.y, rect.y + rect.height),
            distance(point.x, rect.x, rect.x + rect.width),
        );
        if best.as_ref().is_none_or(|(k, _, _)| key < *k) {
            best = Some((key, fragment, rect));
        }
    });
    let (_, fragment, rect) = best?;
    let x = point.x - rect.x;
    let offset = match snap {
        Snap::Nearest => fragment.offset_at(x),
        Snap::Character => fragment.cluster_at(x).or_else(|| fragment.offset_at(x)),
    }?;
    Some(TextPosition {
        node: fragment.node,
        offset: offset as usize,
    })
}

/// The distance from `v` to the range `start..end`.
fn distance(v: f32, start: f32, end: f32) -> f32 {
    if v < start {
        start - v
    } else if v > end {
        v - end
    } else {
        0.0
    }
}

/// Finds the innermost block-level box that contains `point` and
/// selectable text (the first one in tree order). Returns true if `b` has
/// selectable text.
fn find_text_block<'a>(
    b: &'a BoxFragment,
    origin: Point,
    point: Point,
    found: &mut Option<(&'a BoxFragment, Point)>,
) -> bool {
    let rect = b.border_rect.translate(origin);
    let mut has_text = false;
    for child in b.children.iter() {
        has_text |= match child {
            Fragment::Text(t) => t.is_selectable(),
            Fragment::Box(child) => find_text_block(child, rect.origin(), point, found),
        };
    }
    if has_text && found.is_none() && !b.is_inline && rect.contains(point) {
        *found = Some((b, origin));
    }
    has_text
}

/// Calls `visit` for each selectable text fragment in the subtree of `b`
/// with its absolute rectangle. `origin` is the absolute position of the
/// parent's border-box origin.
fn walk_text<'a>(
    b: &'a BoxFragment,
    origin: Point,
    visit: &mut impl FnMut(&'a TextFragment, Rect),
) {
    let own = origin + b.border_rect.origin();
    for child in b.children.iter() {
        match child {
            Fragment::Text(t) if t.is_selectable() => visit(t, t.rect.translate(own)),
            Fragment::Text(_) => {}
            Fragment::Box(child) => walk_text(child, own, visit),
        }
    }
}

/// The first and last selectable offsets of each text node that has
/// fragments.
pub(crate) fn text_extents(tree: &FragmentTree) -> HashMap<NodeId, (u32, u32)> {
    let mut extents: HashMap<NodeId, (u32, u32)> = HashMap::new();
    if let Some(root) = &tree.root {
        walk_text(root, Point::default(), &mut |t, _| {
            if let Some((start, end)) = t.node_range() {
                extents
                    .entry(t.node)
                    .and_modify(|e| *e = (e.0.min(start), e.1.max(end)))
                    .or_insert((start, end));
            }
        });
    }
    extents
}

/// The selectable text in the subtree of `root`: from the start of its
/// first text node with fragments to the end of the last one.
pub(crate) fn subtree_range(
    doc: &Document,
    root: NodeId,
    extents: &HashMap<NodeId, (u32, u32)>,
) -> Option<Selection> {
    let mut texts = doc
        .descendants(root)
        .filter_map(|n| extents.get(&n).map(|e| (n, *e)));
    let (first, (start, first_end)) = texts.next()?;
    let (last, end) = texts.last().map_or((first, first_end), |(n, e)| (n, e.1));
    Some(Selection::new(
        TextPosition {
            node: first,
            offset: start as usize,
        },
        TextPosition {
            node: last,
            offset: end as usize,
        },
    ))
}

/// The word (or run of spaces or punctuation) at `position`, by the word
/// boundaries of UAX #29.
pub(crate) fn word_at(doc: &Document, position: TextPosition) -> Option<Selection> {
    let text = doc.get(position.node)?.as_text()?;
    let (start, word) = text
        .split_word_bound_indices()
        .find(|(start, word)| position.offset < start + word.len())
        .or_else(|| text.split_word_bound_indices().next_back())?;
    let at = |offset| TextPosition {
        node: position.node,
        offset,
    };
    Some(Selection::new(at(start), at(start + word.len())))
}

/// True if the element is rendered and is not inline-level: a block
/// boundary for paragraphs.
fn is_block(styles: &StyleMap, node: NodeId) -> bool {
    styles.get(node).is_some_and(|s| {
        !s.display.is_inline_level() && !matches!(s.display, Display::Contents | Display::None)
    })
}

/// The paragraph at `position`, which a triple click selects: the text of
/// the nearest block between its nested blocks and line breaks (`<br>`),
/// from the start of its first text node with fragments to the end of the
/// last one.
pub(crate) fn paragraph_at(
    doc: &Document,
    styles: &StyleMap,
    extents: &HashMap<NodeId, (u32, u32)>,
    position: TextPosition,
) -> Option<Selection> {
    let block = doc
        .ancestors(position.node)
        .find(|&n| is_block(styles, n))?;
    // The current run of text nodes, and whether it has the position.
    let mut run: Option<(NodeId, NodeId)> = None;
    let mut found = false;
    let mut stack: Vec<NodeId> = doc.children(block).collect();
    stack.reverse();
    while let Some(node) = stack.pop() {
        let boundary = match &doc.node(node).data {
            NodeData::Text(_) => {
                if extents.contains_key(&node) {
                    run = Some(run.map_or((node, node), |(first, _)| (first, node)));
                    found |= node == position.node;
                }
                false
            }
            NodeData::Element(e) if e.is_html_named(&local_name!("br")) => true,
            NodeData::Element(_) if is_block(styles, node) => true,
            NodeData::Element(_) => {
                if styles.get(node).is_some() {
                    let first = stack.len();
                    stack.extend(doc.children(node));
                    stack[first..].reverse();
                }
                false
            }
            _ => false,
        };
        if boundary {
            if found {
                break;
            }
            run = None;
        }
    }
    let (first, last) = run.filter(|_| found)?;
    let (start, end) = (extents.get(&first)?.0, extents.get(&last)?.1);
    Some(Selection::new(
        TextPosition {
            node: first,
            offset: start as usize,
        },
        TextPosition {
            node: last,
            offset: end as usize,
        },
    ))
}

/// The selected text, as the `innerText` algorithm would produce it for
/// the selected part of the document
/// (<https://html.spec.whatwg.org/multipage/dom.html#the-innertext-idl-attribute>):
/// collapsed white space, line breaks between blocks, two between
/// paragraphs, tabs between table cells. Deliberate simplification:
/// `text-transform` is not applied.
pub(crate) fn selected_text(
    doc: &Document,
    styles: &StyleMap,
    start: TextPosition,
    end: TextPosition,
    order: &TreeOrder,
) -> String {
    enum Step {
        Enter(NodeId),
        /// The end of an element: its required line break count, and true
        /// for a table cell that a tab separates from the next cell.
        Leave(usize, bool),
    }
    let (start_rank, end_rank) = (order.rank(start.node), order.rank(end.node));
    let mut out = TextBuilder::default();
    let mut stack: Vec<Step> = doc
        .document_element()
        .map(Step::Enter)
        .into_iter()
        .collect();
    while let Some(step) = stack.pop() {
        let node = match step {
            Step::Leave(breaks, tab) => {
                out.breaks(breaks);
                if tab {
                    out.tab();
                }
                continue;
            }
            Step::Enter(node) => node,
        };
        let rank = order.rank(node);
        if rank > end_rank {
            break;
        }
        match &doc.node(node).data {
            NodeData::Text(text) if rank >= start_rank => {
                let Some(style) = doc.parent(node).and_then(|p| styles.get(p)) else {
                    continue;
                };
                if style.visibility != Visibility::Visible || style.user_select == UserSelect::None
                {
                    continue;
                }
                let from = if node == start.node { start.offset } else { 0 };
                let to = if node == end.node {
                    end.offset
                } else {
                    text.len()
                };
                let (from, to) = (text.floor_char_boundary(from), text.floor_char_boundary(to));
                if let Some(slice) = text.get(from..to) {
                    out.text(slice, style.white_space);
                }
            }
            NodeData::Element(element) => {
                let Some(style) = styles.get(node).filter(|s| s.display != Display::None) else {
                    continue;
                };
                let breaks = required_line_breaks(element, style);
                out.breaks(breaks);
                if element.is_html_named(&local_name!("br")) {
                    out.newline();
                }
                let tab = style.display == Display::TableCell && has_next_cell(doc, styles, node);
                stack.push(Step::Leave(breaks, tab));
                let first = stack.len();
                stack.extend(doc.children(node).map(Step::Enter));
                stack[first..].reverse();
            }
            _ => {}
        }
    }
    out.finish()
}

/// The required line break count of an element for `innerText`: 2 for
/// `<p>`, 1 for other block-level boxes (not table cells), otherwise 0.
fn required_line_breaks(element: &swb_dom::ElementData, style: &ComputedStyle) -> usize {
    if element.is_html_named(&local_name!("p")) {
        return 2;
    }
    let is_block = !style.display.is_inline_level()
        && !matches!(style.display, Display::Contents | Display::TableCell);
    usize::from(is_block)
}

/// True if a rendered table cell follows the cell `node` in its row (a tab
/// separates them).
fn has_next_cell(doc: &Document, styles: &StyleMap, node: NodeId) -> bool {
    std::iter::successors(doc.next_sibling_element(node), |&n| {
        doc.next_sibling_element(n)
    })
    .any(|n| {
        styles
            .get(n)
            .is_some_and(|s| s.display == Display::TableCell)
    })
}

/// Builds the selected text: collapses white space and merges line breaks.
/// Nothing is recorded before the first text.
#[derive(Default)]
struct TextBuilder {
    out: String,
    /// The required line break count before the next text.
    pending_breaks: usize,
    /// A tab between table cells before the next text.
    pending_tab: bool,
    started: bool,
}

impl TextBuilder {
    fn breaks(&mut self, count: usize) {
        if self.started {
            self.pending_breaks = self.pending_breaks.max(count);
        }
    }

    /// A tab between table cells. It is written before the next text, so
    /// the copied text does not end with one.
    fn tab(&mut self) {
        self.pending_tab = self.started;
    }

    fn has_pending(&self) -> bool {
        self.pending_breaks > 0 || self.pending_tab
    }

    /// Writes the pending line breaks, or else the pending tab.
    fn flush_breaks(&mut self) {
        if self.pending_breaks > 0 {
            self.trim_trailing_spaces();
            for _ in 0..self.pending_breaks {
                self.out.push('\n');
            }
        } else if self.pending_tab {
            self.trim_trailing_spaces();
            self.out.push('\t');
        }
        self.pending_breaks = 0;
        self.pending_tab = false;
    }

    fn trim_trailing_spaces(&mut self) {
        let trimmed = self.out.trim_end_matches(' ').len();
        self.out.truncate(trimmed);
    }

    /// True at the start of a line or a table cell, where collapsible
    /// spaces are removed.
    fn at_line_start(&self) -> bool {
        self.out.is_empty() || self.out.ends_with(['\n', '\t'])
    }

    /// A forced line break (`<br>`).
    fn newline(&mut self) {
        if self.started {
            self.flush_breaks();
            self.trim_trailing_spaces();
            self.out.push('\n');
        }
    }

    fn text(&mut self, text: &str, white_space: WhiteSpace) {
        if !white_space.collapses_spaces() {
            if text.is_empty() {
                return;
            }
            self.started = true;
            self.flush_breaks();
            self.out.extend(text.chars().filter(|&c| c != '\r'));
            return;
        }
        let pre_line = white_space == WhiteSpace::PreLine;
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if !matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0C') {
                self.started = true;
                self.flush_breaks();
                self.out.push(c);
                continue;
            }
            let mut newlines = usize::from(c == '\n');
            while let Some(&next) = chars.peek() {
                if !matches!(next, ' ' | '\t' | '\n' | '\r' | '\x0C') {
                    break;
                }
                newlines += usize::from(next == '\n');
                chars.next();
            }
            if !self.started {
                continue;
            }
            if pre_line && newlines > 0 {
                self.flush_breaks();
                self.trim_trailing_spaces();
                for _ in 0..newlines {
                    self.out.push('\n');
                }
            } else if !self.has_pending() && !self.at_line_start() && !self.out.ends_with(' ') {
                self.out.push(' ');
            }
        }
    }

    fn finish(mut self) -> String {
        self.trim_trailing_spaces();
        let trimmed = self.out.trim_end_matches('\n').len();
        self.out.truncate(trimmed);
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_space_and_breaks() {
        let mut b = TextBuilder::default();
        b.breaks(1);
        b.text("\n  Hello   world ", WhiteSpace::Normal);
        b.breaks(2);
        b.text(" second ", WhiteSpace::Normal);
        b.text(" para", WhiteSpace::Normal);
        b.newline();
        b.text("  after br", WhiteSpace::Normal);
        b.breaks(1);
        assert_eq!(b.finish(), "Hello world\n\nsecond para\nafter br");
    }

    #[test]
    fn tabs_only_between_cells() {
        let mut b = TextBuilder::default();
        b.text(" a ", WhiteSpace::Normal);
        b.tab();
        b.text(" b ", WhiteSpace::Normal);
        b.tab();
        assert_eq!(b.finish(), "a\tb");
    }

    #[test]
    fn preserved_white_space() {
        let mut b = TextBuilder::default();
        b.text("a  b\n c", WhiteSpace::Pre);
        b.breaks(1);
        b.text("x \n y", WhiteSpace::PreLine);
        assert_eq!(b.finish(), "a  b\n c\nx\ny");
    }

    #[test]
    fn tree_order() {
        let doc = swb_dom::parse_html("<p id=a>x</p><p id=b>y</p>");
        let order = TreeOrder::new(&doc);
        let text = |id: &str| {
            doc.element_by_id(id)
                .and_then(|e| doc.children(e).next())
                .expect("a text child")
        };
        let at = |id: &str, offset| TextPosition {
            node: text(id),
            offset,
        };
        assert_eq!(order.compare(at("a", 1), at("b", 0)), Ordering::Less);
        assert_eq!(order.compare(at("a", 1), at("a", 0)), Ordering::Greater);
        let s = Selection::new(at("b", 1), at("a", 0));
        assert_eq!(s.ordered(&order), (at("a", 0), at("b", 1)));
    }

    #[test]
    fn words() {
        let doc = swb_dom::parse_html("<p>hello, big world</p>");
        let p = doc
            .find_element(NodeId::DOCUMENT, |e| e.is_html_named(&local_name!("p")))
            .expect("p");
        let node = doc.children(p).next().expect("text");
        let word = |offset| {
            let s = word_at(&doc, TextPosition { node, offset }).expect("a word");
            (s.anchor.offset, s.focus.offset)
        };
        assert_eq!(word(0), (0, 5));
        assert_eq!(word(4), (0, 5));
        assert_eq!(word(5), (5, 6));
        assert_eq!(word(8), (7, 10));
        assert_eq!(word(16), (11, 16));
        assert_eq!(word(99), (11, 16));
    }
}
