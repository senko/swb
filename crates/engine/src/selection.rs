//! Text selection: positions in text nodes, the position at a point,
//! words and blocks, the highlight, and the selected text.
//!
//! The selection belongs to the DOM (ADR 0039 section 3: a live range
//! entry of the arena, `swb_dom::Document::selection`), so it follows DOM
//! changes, scripts see the user's selection, and a script's selection is
//! painted and copied. The user's gestures compute positions in text nodes
//! from the layout and write a new selection; the highlight and the copied
//! text read its boundary points, which can also be in elements (`(body,
//! 3)`). They compare boundary points through keys over the tree order
//! ([`TreeOrder::key`]).

use std::collections::HashMap;

use swb_dom::{Boundary, Document, NodeData, NodeId, is_html_whitespace, local_name};
use swb_layout::{
    BoxContent, BoxFragment, Fragment, FragmentTree, NoScroll, Point, Rect, ScrollOffsets,
    ScrollState, TextFragment, clip_property_area, forms_stacking_context,
    is_absolute_containing_block, is_fixed_containing_block,
};
use swb_style::{ComputedStyle, Display, StyleMap, Visibility, WhiteSpace};

/// A position in a text node: a byte offset in its data, at a character
/// boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextPosition {
    /// The text node.
    pub node: NodeId,
    /// The byte offset in the node's data.
    pub offset: usize,
}

impl TextPosition {
    /// The position as a DOM boundary point.
    pub(crate) fn boundary(self) -> Boundary {
        Boundary::new(self.node, u32::try_from(self.offset).unwrap_or(u32::MAX))
    }
}

/// A selection that a gesture makes, from `anchor` (where it started) to
/// `focus` (where it ends). The focus can be before the anchor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    /// Where the selection started.
    pub anchor: Boundary,
    /// Where the selection ends.
    pub focus: Boundary,
}

impl Selection {
    /// A selection from `anchor` to `focus`.
    pub fn new(anchor: Boundary, focus: Boundary) -> Self {
        Selection { anchor, focus }
    }

    /// A selection between two text positions.
    pub(crate) fn between(anchor: TextPosition, focus: TextPosition) -> Self {
        Selection::new(anchor.boundary(), focus.boundary())
    }

    /// True if the selection is empty.
    pub fn is_collapsed(&self) -> bool {
        self.anchor == self.focus
    }
}

/// The place of a boundary point in tree order: the rank of a node and a
/// byte offset in it. A point in a text node is its rank and offset; a point
/// `(P, i)` in an element is the rank of `P`'s child `i` (or of the node
/// after `P`'s subtree) at offset 0, so that the text nodes before it are
/// selected to their end and the ones from it on are not.
pub(crate) type Key = (u32, u32);

/// The position of each node in shadow-including tree order (a shadow tree
/// right after its host, before the host's children), and the end of its
/// subtree. A range has its boundary points in one tree, whose nodes are
/// contiguous in this order. Rebuilt after each DOM change.
#[derive(Clone, Debug, Default)]
pub(crate) struct TreeOrder {
    rank: Vec<u32>,
    /// The rank of the node after the subtree of each node (with its shadow
    /// tree).
    end: Vec<u32>,
}

impl TreeOrder {
    /// The tree order of the nodes of `doc`.
    pub(crate) fn new(doc: &Document) -> Self {
        let mut rank = vec![u32::MAX; doc.len()];
        let mut order = Vec::new();
        for (i, node) in doc
            .shadow_including_descendants(NodeId::DOCUMENT)
            .enumerate()
        {
            if let Some(r) = rank.get_mut(node.index()) {
                *r = u32::try_from(i).unwrap_or(u32::MAX);
            }
            order.push(node);
        }
        // The size of each subtree, from the leaves up (reverse tree order
        // visits every child before its parent, and a shadow root before its
        // host).
        let mut size = vec![1u32; doc.len()];
        for &node in order.iter().rev() {
            let own = size.get(node.index()).copied().unwrap_or(1);
            let up = doc
                .parent(node)
                .or_else(|| doc.node(node).data.as_shadow_root().map(|s| s.host));
            if let Some(up) = up
                && let Some(s) = size.get_mut(up.index())
            {
                *s = s.saturating_add(own);
            }
        }
        let end = rank
            .iter()
            .zip(&size)
            .map(|(&r, &s)| {
                if r == u32::MAX {
                    u32::MAX
                } else {
                    r.saturating_add(s)
                }
            })
            .collect();
        TreeOrder { rank, end }
    }

    /// The node's position in tree order; detached nodes come last.
    pub(crate) fn rank(&self, node: NodeId) -> u32 {
        self.rank.get(node.index()).copied().unwrap_or(u32::MAX)
    }

    /// The rank of the node after the subtree of `node`.
    fn end_of(&self, node: NodeId) -> u32 {
        self.end.get(node.index()).copied().unwrap_or(u32::MAX)
    }

    /// The key of boundary point `b` (see [`Key`]).
    pub(crate) fn key(&self, doc: &Document, b: Boundary) -> Key {
        let Some(node) = doc.get(b.node) else {
            return (u32::MAX, 0);
        };
        if matches!(
            node.data,
            NodeData::Text(_) | NodeData::Comment(_) | NodeData::ProcessingInstruction { .. }
        ) {
            return (self.rank(b.node), b.offset);
        }
        // The child at the offset, from the nearer end of the child list.
        let count = doc.child_count(b.node);
        let offset = b.offset as usize;
        if offset >= count {
            return (self.end_of(b.node), 0);
        }
        let child = if offset <= count / 2 {
            doc.children(b.node).nth(offset)
        } else {
            std::iter::successors(node.last_child(), |&c| doc.node(c).prev_sibling())
                .nth(count - 1 - offset)
        };
        child.map_or((self.end_of(b.node), 0), |c| (self.rank(c), 0))
    }

    /// The start and the end of a range as keys.
    pub(crate) fn keys(&self, doc: &Document, start: Boundary, end: Boundary) -> (Key, Key) {
        (self.key(doc, start), self.key(doc, end))
    }
}

/// The selected bytes `from..to` of a text node at `rank` between the keys
/// `start` and `end` (`to` is the node's length when the range goes past it);
/// `None` if the range does not reach into it.
fn selected_part(rank: u32, len: u32, start: Key, end: Key) -> Option<(u32, u32)> {
    if rank < start.0 || rank > end.0 {
        return None;
    }
    let from = if rank == start.0 { start.1 } else { 0 };
    let to = if rank == end.0 { end.1 } else { len };
    Some((from, to))
}

/// The selected part of each text node, and of the text of the focused
/// form control, for painting.
pub(crate) struct Highlight<'a> {
    /// The page selection: its start and end as keys.
    pub(crate) page: Option<(Key, Key)>,
    /// The selection in the focused text control: the control and the
    /// range of offsets in its shown text.
    pub(crate) control: Option<(NodeId, u32, u32)>,
    pub(crate) order: &'a TreeOrder,
    pub(crate) doc: &'a Document,
}

impl swb_paint::Highlights for Highlight<'_> {
    fn selected(&self, node: NodeId) -> Option<(u32, u32)> {
        if let Some((control, from, to)) = self.control
            && control == node
        {
            return (from < to).then_some((from, to));
        }
        // The page selection covers text nodes only; the text of form
        // controls belongs to the control elements.
        let (start, end) = self.page?;
        self.doc.get(node)?.as_text()?;
        let (from, to) = selected_part(self.order.rank(node), u32::MAX, start, end)?;
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

/// The text position at `point` (document coordinates), with the scroll
/// offsets of scroll containers applied. The text is searched in the
/// innermost block that contains the point and has text (the whole
/// document if there is none): the nearest line, then the nearest fragment
/// on it. Text that clipping boxes (scroll containers, `overflow: hidden`)
/// hide is skipped.
pub(crate) fn position_at(
    tree: &FragmentTree,
    offsets: &dyn ScrollOffsets,
    point: Point,
    snap: Snap,
) -> Option<TextPosition> {
    let root = tree.root.as_ref()?;
    let mut scope = None;
    find_text_block(root, Place::ROOT, offsets, point, &mut scope);
    let (block, place) = scope.unwrap_or((root, Place::ROOT));
    let mut best: Option<((f32, f32), &TextFragment, Rect)> = None;
    walk_text(block, place, offsets, &mut |fragment, rect, visible| {
        let key = distance_to(point, rect);
        if visible && best.as_ref().is_none_or(|(k, _, _)| key < *k) {
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

/// True if a block with selectable text contains `point` (document
/// coordinates, scroll offsets applied); a press outside every such block
/// (below the content) puts the caret at the end of the body, as in
/// Chromium (measured, `user-click-empty`).
pub(crate) fn text_block_at(
    tree: &FragmentTree,
    offsets: &dyn ScrollOffsets,
    point: Point,
) -> bool {
    let Some(root) = tree.root.as_ref() else {
        return false;
    };
    let mut scope = None;
    find_text_block(root, Place::ROOT, offsets, point, &mut scope);
    scope.is_some()
}

/// The vertical and the horizontal distance from `point` to `rect` (0
/// inside it), in this order, so that comparisons find the nearest line
/// first.
fn distance_to(point: Point, rect: Rect) -> (f32, f32) {
    let distance = |v: f32, start: f32, end: f32| {
        if v < start {
            start - v
        } else if v > end {
            v - end
        } else {
            0.0
        }
    };
    (
        distance(point.y, rect.y, rect.y + rect.height),
        distance(point.x, rect.x, rect.x + rect.width),
    )
}

/// The area that the clipping boxes (scroll containers, `overflow:
/// hidden`, `overflow: clip`) above a box leave visible.
#[derive(Clone, Copy, Debug)]
enum Clip {
    /// Nothing clips.
    None,
    /// Only this area is visible. An axis that no box clips has a range
    /// that covers all coordinates.
    Area(Rect),
    /// Nothing is visible.
    Empty,
}

impl Clip {
    /// This clip and the clip `other`.
    fn and_clip(self, other: Clip) -> Clip {
        match other {
            Clip::None => self,
            Clip::Area(area) => self.and(area),
            Clip::Empty => Clip::Empty,
        }
    }

    /// This clip and `area`. An area without width or height leaves
    /// nothing visible.
    fn and(self, area: Rect) -> Clip {
        match self {
            Clip::None if area.width > 0.0 && area.height > 0.0 => Clip::Area(area),
            Clip::Area(c) => c.intersection(&area).map_or(Clip::Empty, Clip::Area),
            Clip::None | Clip::Empty => Clip::Empty,
        }
    }

    /// True if a part of `r` is visible.
    fn shows(self, r: Rect) -> bool {
        match self {
            Clip::None => true,
            Clip::Area(c) => {
                r.x < c.right() && r.right() > c.x && r.y < c.bottom() && r.bottom() > c.y
            }
            Clip::Empty => false,
        }
    }

    /// True if `p` is visible.
    fn contains(self, p: Point) -> bool {
        match self {
            Clip::None => true,
            Clip::Area(c) => c.contains(p),
            Clip::Empty => false,
        }
    }
}

/// The area that box `b` clips its content to (its padding box on the
/// axes where its overflow clips), in absolute coordinates; `None` if it
/// does not clip. `origin` is the absolute position that `b` is placed
/// against.
fn clip_area(b: &BoxFragment, origin: Point) -> Option<Rect> {
    let style = &b.style;
    if !style.clips_x() && !style.clips_y() {
        return None;
    }
    let padding = b.padding_rect().translate(origin);
    let far = 4.0 * swb_style::Length::MAX_PX;
    let (x, width) = if style.clips_x() {
        (padding.x, padding.width)
    } else {
        (-far, 2.0 * far)
    };
    let (y, height) = if style.clips_y() {
        (padding.y, padding.height)
    } else {
        (-far, 2.0 * far)
    };
    Some(Rect::new(x, y, width, height))
}

/// Where a box is during a walk of the fragment tree with the scroll
/// offsets applied: the absolute position it is placed against, the
/// scroll state, and the clip of the boxes above it.
#[derive(Clone, Copy)]
struct Place {
    origin: Point,
    scroll: ScrollState,
    clip: Clip,
    /// The clip of the absolutely positioned boxes here: inside the nearest
    /// positioned or transformed box (their containing block), or outside
    /// the nearest block in a positioned inline box below it (the inline
    /// box is their containing block). `None` without either.
    cb_clip: Option<Clip>,
    /// The clip of the fixed boxes here: inside the nearest transformed
    /// box (their containing block); `None` without one.
    fixed_clip: Option<Clip>,
    /// The `clip` rectangles of the ancestors (CSS 2.2 §11.1.2): they also
    /// clip fixed boxes.
    property_clip: Clip,
}

impl Place {
    /// The place of the root box.
    const ROOT: Place = Place {
        origin: Point::new(0.0, 0.0),
        scroll: ScrollState::DEFAULT,
        clip: Clip::None,
        cb_clip: None,
        fixed_clip: None,
        property_clip: Clip::None,
    };

    /// The absolute border box of `b` at this place, the clip that applies
    /// to `b`, and the place of its children: moved by the scroll offset of
    /// `b` and clipped by it. As paint's clips, an absolutely positioned box
    /// is clipped by its containing block (a positioned or transformed box)
    /// and the boxes above it, or, inside a positioned inline box, by the
    /// boxes above the nearest block in it; without such an ancestor it is
    /// not clipped. A fixed box is clipped only by a transformed containing
    /// block and the boxes above it. Inside a stacking context below their
    /// containing block, both are clipped by the clips around that stacking
    /// context (paint draws them in it). `clip` rectangles clip all
    /// descendants, also fixed ones.
    fn enter(self, b: &BoxFragment, offsets: &dyn ScrollOffsets) -> (Rect, Clip, Place) {
        let origin = self.scroll.origin_of(b, self.origin);
        let rect = b.border_rect.translate(origin);
        let own = match b.style.position {
            swb_style::Position::Fixed => self
                .fixed_clip
                .unwrap_or(Clip::None)
                .and_clip(self.property_clip),
            swb_style::Position::Absolute => self.cb_clip.unwrap_or(Clip::None),
            _ => self.clip,
        };
        let property = clip_property_area(b, rect);
        let own = property.map_or(own, |area| own.and(area));
        let property_clip =
            property.map_or(self.property_clip, |area| self.property_clip.and(area));
        let clip = clip_area(b, origin).map_or(own, |area| own.and(area));
        let context = forms_stacking_context(b);
        let cb_clip = if is_absolute_containing_block(b) {
            // The containing block of the absolutely positioned boxes
            // inside: they are clipped by it (paint draws them in its
            // stacking context, inside its clip).
            Some(clip)
        } else if b.in_positioned_inline || context {
            Some(own)
        } else {
            self.cb_clip
        };
        let fixed_clip = if is_fixed_containing_block(b) {
            Some(clip)
        } else if context {
            // Only for fixed boxes with a transformed containing block:
            // boxes fixed to the viewport are not clipped.
            self.fixed_clip.map(|_| own)
        } else {
            self.fixed_clip
        };
        let (child_origin, scroll) = self.scroll.enter(b, rect.origin(), offsets);
        let place = Place {
            origin: child_origin,
            scroll,
            clip,
            cb_clip,
            fixed_clip,
            property_clip,
        };
        (rect, own, place)
    }
}

/// Finds the innermost block-level box that contains `point` (and is not
/// clipped away there) and selectable text (the first one in tree order).
/// Returns true if `b` has selectable text. `place` is where `b` is.
fn find_text_block<'a>(
    b: &'a BoxFragment,
    place: Place,
    offsets: &dyn ScrollOffsets,
    point: Point,
    found: &mut Option<(&'a BoxFragment, Place)>,
) -> bool {
    if is_control(b) {
        return false;
    }
    let (rect, clip, children) = place.enter(b, offsets);
    let mut has_text = false;
    for child in b.children.iter() {
        has_text |= match child {
            Fragment::Text(t) => t.is_selectable(),
            Fragment::Box(child) => find_text_block(child, children, offsets, point, found),
        };
    }
    if has_text && found.is_none() && !b.is_inline && rect.contains(point) && clip.contains(point) {
        *found = Some((b, place));
    }
    has_text
}

/// True for the box of a form control: its text is not part of the page
/// selection.
fn is_control(b: &BoxFragment) -> bool {
    matches!(b.content, BoxContent::Control(_))
}

/// Calls `visit` for each selectable text fragment in the subtree of `b`
/// (outside form controls) with its absolute rectangle and whether a part
/// of it is visible (not clipped away). `place` is where `b` is.
fn walk_text<'a>(
    b: &'a BoxFragment,
    place: Place,
    offsets: &dyn ScrollOffsets,
    visit: &mut impl FnMut(&'a TextFragment, Rect, bool),
) {
    if is_control(b) {
        return;
    }
    walk_all_text(b, place, offsets, visit);
}

/// [`walk_text`] including the text of form controls.
fn walk_all_text<'a>(
    b: &'a BoxFragment,
    place: Place,
    offsets: &dyn ScrollOffsets,
    visit: &mut impl FnMut(&'a TextFragment, Rect, bool),
) {
    let (_, _, children) = place.enter(b, offsets);
    for child in b.children.iter() {
        match child {
            Fragment::Text(t) if t.is_selectable() => {
                let rect = t.rect.translate(children.origin);
                visit(t, rect, children.clip.shows(rect));
            }
            Fragment::Text(_) => {}
            Fragment::Box(child) => walk_text(child, children, offsets, visit),
        }
    }
}

/// The offset in the shown text of form control `control` nearest to
/// `point` (document coordinates, scroll offsets applied): the nearest
/// line, then the nearest caret stop. `None` if the control has no box; 0
/// if it has no text.
pub(crate) fn control_offset_at(
    tree: &FragmentTree,
    offsets: &dyn ScrollOffsets,
    control: NodeId,
    point: Point,
) -> Option<u32> {
    let mut found = None;
    tree.walk_scrolled(offsets, |fragment, origin| {
        if found.is_none()
            && let swb_layout::FragmentRef::Box(b) = fragment
            && b.node == Some(control)
            && is_control(b)
        {
            found = Some((b, origin));
        }
    });
    let (b, origin) = found?;
    // The origin already has the scroll offsets; a control is not a scroll
    // container.
    let place = Place {
        origin,
        ..Place::ROOT
    };
    let mut best: Option<((f32, f32), u32)> = None;
    walk_all_text(b, place, &NoScroll, &mut |t, rect, _| {
        if t.node != control {
            return;
        }
        let key = distance_to(point, rect);
        if let Some(offset) = t.offset_at(point.x - rect.x)
            && best.is_none_or(|(k, _)| key < k)
        {
            best = Some((key, offset));
        }
    });
    Some(best.map_or(0, |(_, offset)| offset))
}

/// The first and last selectable offsets of each text node that has
/// fragments (also where clipping boxes hide them).
pub(crate) fn text_extents(tree: &FragmentTree) -> HashMap<NodeId, (u32, u32)> {
    let mut extents: HashMap<NodeId, (u32, u32)> = HashMap::new();
    for root in tree.roots() {
        walk_text(root, Place::ROOT, &NoScroll, &mut |t, _, _| {
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
    Some(text_range(first, start, last, end))
}

/// The selection from offset `start` in text node `first` to offset `end`
/// in text node `last`.
fn text_range(first: NodeId, start: u32, last: NodeId, end: u32) -> Selection {
    Selection::new(Boundary::new(first, start), Boundary::new(last, end))
}

/// The selection of select all (Ctrl+A) as Chromium makes it (measured,
/// `user-select-all` of `tools/probes/web-selection.json`): from the start of
/// the first text node with fragments to the end of the body (`(body,
/// childCount)`); from the start of the body without text. `None` without a
/// body or a document element.
pub(crate) fn select_all_range(
    doc: &Document,
    extents: &HashMap<NodeId, (u32, u32)>,
) -> Option<Selection> {
    let container = doc.body().or_else(|| doc.document_element())?;
    let count = u32::try_from(doc.child_count(container)).unwrap_or(u32::MAX);
    let end = Boundary::new(container, count);
    let start = doc
        .descendants(NodeId::DOCUMENT)
        .find_map(|n| extents.get(&n).map(|e| Boundary::new(n, e.0)))
        .unwrap_or(Boundary::new(container, 0));
    Some(Selection::new(start, end))
}

/// The word (or run of spaces or punctuation) at `position`, by the word
/// boundaries of UAX #29.
pub(crate) fn word_at(doc: &Document, position: TextPosition) -> Option<Selection> {
    let text = doc.get(position.node)?.as_text()?;
    let word = crate::edit::word_at(text, position.offset)?;
    let at = |offset| TextPosition {
        node: position.node,
        offset,
    };
    Some(Selection::between(at(word.start), at(word.end)))
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
    Some(text_range(first, start, last, end))
}

/// The selected text, as the `innerText` algorithm would produce it for
/// the selected part of the document
/// (<https://html.spec.whatwg.org/multipage/dom.html#the-innertext-idl-attribute>):
/// collapsed white space, line breaks between blocks, two between
/// paragraphs, tabs between table cells. `start` and `end` are the keys of
/// the range ([`TreeOrder::key`]) in the tree of `scope` (the document, or a
/// shadow root for a range in a shadow tree). Deliberate simplification:
/// `text-transform` is not applied.
pub(crate) fn selected_text(
    doc: &Document,
    styles: &StyleMap,
    scope: NodeId,
    (start, end): (Key, Key),
    order: &TreeOrder,
) -> String {
    enum Step {
        Enter(NodeId),
        /// The end of an element: its required line break count, and true
        /// for a table cell that a tab separates from the next cell.
        Leave(usize, bool),
    }
    let mut out = TextBuilder::default();
    let mut stack: Vec<Step> = doc.children(scope).map(Step::Enter).collect();
    stack.reverse();
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
        // A node at the end key's rank starts after the range (an element),
        // or holds its end (a text node).
        if rank > end.0 || (rank == end.0 && !matches!(doc.node(node).data, NodeData::Text(_))) {
            break;
        }
        match &doc.node(node).data {
            NodeData::Text(text) => {
                let len = u32::try_from(text.len()).unwrap_or(u32::MAX);
                let Some((from, to)) = selected_part(rank, len, start, end) else {
                    continue;
                };
                let Some(style) = doc.parent(node).and_then(|p| styles.get(p)) else {
                    continue;
                };
                if style.visibility != Visibility::Visible || !style.is_user_selectable() {
                    continue;
                }
                let (from, to) = (
                    text.floor_char_boundary(from as usize),
                    text.floor_char_boundary(to as usize),
                );
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
                // A form control or a replaced element inside the range keeps
                // the spaces on both sides (they are in different runs of
                // text; measured: "pre  post").
                if rank >= start.0 && is_atomic_inline(element, style) {
                    out.atomic();
                    continue;
                }
                let tab = style.display == Display::TableCell && has_next_cell(doc, styles, node);
                stack.push(Step::Leave(breaks, tab));
                let first = stack.len();
                // The hidden contents of a closed `details` are not text of
                // the page: only its summary is.
                let hidden = style.skips_contents()
                    || styles
                        .pseudo(node, swb_style::PseudoKind::DetailsContent)
                        .is_some_and(|s| s.contents_hidden);
                let summary = doc.details_summary(node);
                stack.extend(
                    doc.children(node)
                        .filter(|&c| !hidden || Some(c) == summary)
                        .map(Step::Enter),
                );
                stack[first..].reverse();
            }
            _ => {}
        }
    }
    out.finish()
}

/// How `node` takes part in the lines of text (for the caret positions of
/// `Selection.containsNode`): from its computed style, and for a text node
/// from its fragments (`extents`, [`text_extents`]).
pub(crate) fn flow_of(
    doc: &Document,
    styles: &StyleMap,
    extents: &HashMap<NodeId, (u32, u32)>,
    node: NodeId,
) -> swb_dom::Flow {
    use swb_dom::Flow;
    match &doc.node(node).data {
        NodeData::Text(_) if extents.contains_key(&node) => Flow::Text,
        NodeData::Element(e) => match styles.get(node) {
            Some(s) if s.display == Display::None => Flow::Skipped,
            Some(s) if is_atomic_inline(e, s) => Flow::Atomic,
            Some(s) if s.display.is_inline_level() || s.display == Display::Contents => {
                Flow::Inline
            }
            Some(_) => Flow::Block,
            None => Flow::Skipped,
        },
        _ => Flow::Skipped,
    }
}

/// True for an inline-level form control or replaced element: its content
/// is not text of the page (a text area's text belongs to the control).
fn is_atomic_inline(element: &swb_dom::ElementData, style: &ComputedStyle) -> bool {
    element.is_html()
        && style.display.is_inline_level()
        && matches!(
            &**element.local_name(),
            "input"
                | "textarea"
                | "select"
                | "img"
                | "iframe"
                | "video"
                | "audio"
                | "canvas"
                | "embed"
                | "object"
        )
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
    /// True after a form control or replaced element: the next space is
    /// kept although the text ends with one.
    after_atomic: bool,
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

    /// A form control or replaced element: no text, but content of its
    /// line: the line breaks before it are written (measured: select all
    /// ends with the break before a line of controls), and the spaces around
    /// it do not collapse into one.
    fn atomic(&mut self) {
        if self.started {
            self.flush_breaks();
            self.after_atomic = true;
        }
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
            self.after_atomic = false;
            self.flush_breaks();
            self.out.extend(text.chars().filter(|&c| c != '\r'));
            return;
        }
        let pre_line = white_space.is_pre_line();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if !is_html_whitespace(c) {
                self.started = true;
                self.after_atomic = false;
                self.flush_breaks();
                self.out.push(c);
                continue;
            }
            let mut newlines = usize::from(c == '\n');
            while let Some(&next) = chars.peek() {
                if !is_html_whitespace(next) {
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
            } else if !self.has_pending()
                && !self.at_line_start()
                && (!self.out.ends_with(' ') || self.after_atomic)
            {
                self.out.push(' ');
                self.after_atomic = false;
            }
        }
    }

    fn finish(mut self) -> String {
        self.trim_trailing_spaces();
        // The breaks before a last form control or replaced element stay.
        if !self.after_atomic {
            let trimmed = self.out.trim_end_matches('\n').len();
            self.out.truncate(trimmed);
        }
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
    fn keys_of_boundary_points() {
        let doc = swb_dom::parse_html("<p id=a>x</p><p id=b>y<i>z</i></p>");
        let order = TreeOrder::new(&doc);
        let el = |id: &str| doc.element_by_id(id).expect("the element");
        let text = |id: &str| doc.children(el(id)).next().expect("a text child");
        let (a, b) = (el("a"), el("b"));
        let i = doc.children(b).nth(1).expect("i");
        let z = doc.children(i).next().expect("z");
        let key = |node, offset| order.key(&doc, Boundary::new(node, offset));
        assert!(key(text("a"), 1) < key(text("b"), 0));
        assert_eq!(key(text("b"), 1), (order.rank(text("b")), 1));
        // `(p, i)` is the rank of child `i`, or of the node after `p`.
        assert_eq!(key(b, 0), (order.rank(text("b")), 0));
        assert_eq!(key(b, 1), (order.rank(i), 0));
        assert_eq!(key(b, 2), (order.rank(z) + 1, 0));
        assert_eq!(key(a, 1), (order.rank(b), 0));
        // The text of `b` is selected to its end from `(a, 0)` to `(b, 1)`,
        // and `z` is not.
        let (start, end) = order.keys(&doc, Boundary::new(a, 0), Boundary::new(b, 1));
        let rank = order.rank(text("b"));
        assert_eq!(selected_part(rank, 1, start, end), Some((0, 1)));
        assert_eq!(selected_part(order.rank(z), 1, start, end), None);
    }

    #[test]
    fn spaces_around_a_control_stay() {
        let mut b = TextBuilder::default();
        b.text("pre ", WhiteSpace::Normal);
        b.atomic();
        b.text(" post", WhiteSpace::Normal);
        assert_eq!(b.finish(), "pre  post");
        // The break before a last line of controls stays.
        let mut b = TextBuilder::default();
        b.text("c3", WhiteSpace::Normal);
        b.breaks(1);
        b.atomic();
        assert_eq!(b.finish(), "c3\n");
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
