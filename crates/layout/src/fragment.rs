//! The fragment tree: the output of layout.
//!
//! Every fragment's position is relative to the border-box origin of its
//! parent [`BoxFragment`]. The root fragment is positioned relative to the
//! initial containing block (the page origin). Use [`FragmentTree::walk`]
//! to visit fragments with absolute positions, and
//! [`FragmentTree::walk_scrolled`] for their positions with the scroll
//! offsets of scroll containers applied (see `scroll.rs`).

use std::sync::Arc;

use swb_dom::NodeId;
use swb_style::{ComputedStyle, PseudoKind};
use swb_text::{FontId, GlyphId};

use crate::geom::{Edges, Matrix, Point, Rect};
use crate::scroll::{NoScroll, ScrollOffsets, ScrollState, clamp_scroll_offset, scroll_range};
use crate::{Ancestry, GroupTransform, StickyCache};

/// The result of laying out a document.
#[derive(Clone, Debug)]
pub struct FragmentTree {
    /// The fragment of the root element, or `None` if the document has no
    /// rendered root.
    pub root: Option<BoxFragment>,
    /// The background of the canvas (propagated from the root or body).
    pub canvas_background: Option<CanvasBackground>,
    /// The size of the scrollable area: the union of the initial containing
    /// block and all content that is not clipped.
    pub scroll_size: crate::geom::Size,
    /// The overflow of the viewport (propagated from the root or the body;
    /// `visible` if neither has another value), horizontal and vertical.
    /// The user cannot scroll an axis with `hidden` or `clip`; scripts can.
    pub viewport_overflow: (swb_style::Overflow, swb_style::Overflow),
    /// The size of the viewport (the initial containing block).
    pub viewport: crate::geom::Size,
}

/// The background that paints the canvas, and the element it comes from.
/// That element does not paint its background again.
#[derive(Clone, Debug)]
pub struct CanvasBackground {
    /// The root element or the body.
    pub source: NodeId,
    /// The style with the background.
    pub style: Arc<ComputedStyle>,
}

/// A reference to a fragment of either kind.
#[derive(Clone, Copy, Debug)]
pub enum FragmentRef<'a> {
    /// A box fragment.
    Box(&'a BoxFragment),
    /// A text fragment.
    Text(&'a TextFragment),
}

impl FragmentTree {
    /// Calls `visit` for every fragment, the root included, in tree order
    /// (parents before children). The second argument is the absolute
    /// position of the fragment's parent border-box origin. Scroll offsets
    /// are not applied: these are the positions of the layout.
    pub fn walk<'a>(&'a self, visit: impl FnMut(FragmentRef<'a>, Point)) {
        self.walk_scrolled(&NoScroll, visit);
    }

    /// [`FragmentTree::walk`] with the scroll offsets of scroll containers
    /// applied: the second argument is the absolute position that the
    /// fragment is placed against (for an absolutely positioned box, the
    /// offsets of the scroll containers outside its containing block are
    /// taken back; see [`ScrollState`]).
    pub fn walk_scrolled<'a>(
        &'a self,
        offsets: &dyn ScrollOffsets,
        mut visit: impl FnMut(FragmentRef<'a>, Point),
    ) {
        fn walk_box<'a>(
            b: &'a BoxFragment,
            origin: Point,
            state: ScrollState,
            offsets: &dyn ScrollOffsets,
            visit: &mut impl FnMut(FragmentRef<'a>, Point),
        ) {
            let origin = state.origin_of(b, origin);
            visit(FragmentRef::Box(b), origin);
            let (child_origin, child_state) =
                state.enter(b, origin + b.border_rect.origin(), offsets);
            for child in b.children.iter() {
                match child {
                    Fragment::Box(cb) => walk_box(cb, child_origin, child_state, offsets, visit),
                    Fragment::Text(t) => visit(FragmentRef::Text(t), child_origin),
                }
            }
        }
        if let Some(root) = &self.root {
            walk_box(
                root,
                Point::default(),
                ScrollState::default(),
                offsets,
                &mut visit,
            );
        }
    }

    /// Calls `visit` for every fragment in tree order (parents before
    /// children) with its absolute rectangle (the border box of a box, the
    /// rectangle of a text), with the scroll offsets of scroll containers
    /// `offsets` applied ([`ScrollState`]), and the transform from these
    /// coordinates to painted document coordinates at the viewport scroll
    /// offset `scroll`: the transforms of the box and its ancestors, the
    /// scroll offset for boxes fixed to the viewport, and sticky offsets
    /// (see [`Ancestry::group_transforms`]). Paint and hit testing place
    /// the fragments the same way.
    pub fn walk_painted<'a>(
        &'a self,
        offsets: &dyn ScrollOffsets,
        scroll: Point,
        visit: impl FnMut(FragmentRef<'a>, Rect, &Matrix),
    ) {
        self.walk_painted_in(offsets, scroll, false, visit);
    }

    /// [`FragmentTree::walk_painted`]; with `hidden`, it also visits the
    /// fragments in [`BoxFragment::hidden`].
    fn walk_painted_in<'a>(
        &'a self,
        offsets: &dyn ScrollOffsets,
        scroll: Point,
        hidden: bool,
        mut visit: impl FnMut(FragmentRef<'a>, Rect, &Matrix),
    ) {
        /// The state of the walk above a box.
        struct Above<'s> {
            origin: Point,
            matrix: Matrix,
            state: ScrollState,
            offsets: &'s dyn ScrollOffsets,
            scroll: Point,
            hidden: bool,
        }
        fn walk_box<'a>(
            b: &'a BoxFragment,
            above: &Above<'_>,
            ancestry: &Ancestry,
            cache: &mut (StickyCache, Vec<GroupTransform>),
            visit: &mut impl FnMut(FragmentRef<'a>, Rect, &Matrix),
        ) {
            let origin = above.state.origin_of(b, above.origin);
            let rect = b.border_rect.translate(origin);
            let groups = ancestry.group_transforms(b, rect);
            // The cache is keyed by the address of the sticky constraints:
            // they stay alive until the walk ends.
            cache.1.extend(
                groups
                    .iter()
                    .filter(|g| matches!(g, GroupTransform::Sticky(_)))
                    .cloned(),
            );
            let matrix = groups.iter().fold(above.matrix, |m, g| {
                g.resolve_with(&m, above.scroll, &mut cache.0)
            });
            visit(FragmentRef::Box(b), rect, &matrix);
            let (child_origin, state) = above.state.enter(b, rect.origin(), above.offsets);
            let shift = child_origin - rect.origin();
            let inner = ancestry.enter(b, rect, &groups, shift);
            let below = Above {
                origin: child_origin,
                matrix,
                state,
                ..*above
            };
            let hidden = b
                .hidden
                .iter()
                .filter(|_| above.hidden)
                .flat_map(|h| h.iter());
            for child in b.children.iter().chain(hidden) {
                match child {
                    Fragment::Box(child) => walk_box(child, &below, &inner, cache, visit),
                    Fragment::Text(t) => {
                        visit(
                            FragmentRef::Text(t),
                            t.rect.translate(child_origin),
                            &matrix,
                        );
                    }
                }
            }
        }
        if let Some(root) = &self.root {
            let above = Above {
                origin: Point::default(),
                matrix: Matrix::IDENTITY,
                state: ScrollState::default(),
                offsets,
                scroll,
                hidden,
            };
            let mut cache = (StickyCache::default(), Vec::new());
            walk_box(
                root,
                &above,
                &Ancestry::root(self.viewport),
                &mut cache,
                &mut visit,
            );
        }
    }

    /// The union of the border boxes of each element's fragments as
    /// painted without scroll offsets (pseudo-element boxes excluded). See
    /// [`FragmentTree::element_boxes_scrolled`].
    pub fn element_boxes(&self) -> std::collections::HashMap<NodeId, Rect> {
        self.element_boxes_scrolled(&NoScroll, Point::default())
    }

    /// The union of the border boxes of each element's fragments as
    /// painted with the scroll offsets of scroll containers `offsets` and
    /// at the viewport scroll offset `scroll`, in document coordinates
    /// (pseudo-element boxes excluded; the boxes in hidden contents are
    /// included, see [`BoxFragment::hidden`]). A transformed box gives the
    /// bounding box of its transformed border box, as `getClientRects()`
    /// in browsers.
    pub fn element_boxes_scrolled(
        &self,
        offsets: &dyn ScrollOffsets,
        scroll: Point,
    ) -> std::collections::HashMap<NodeId, Rect> {
        let mut boxes: std::collections::HashMap<NodeId, Rect> = std::collections::HashMap::new();
        self.walk_painted_in(offsets, scroll, true, |f, layout_rect, matrix| {
            if let FragmentRef::Box(b) = f
                && let Some(node) = b.node
                && b.pseudo.is_none()
            {
                let r = matrix.map_rect(&layout_rect);
                let clamp = crate::geom::clamp_length;
                let rect = Rect::new(clamp(r.x), clamp(r.y), clamp(r.width), clamp(r.height));
                boxes
                    .entry(node)
                    .and_modify(|r| *r = r.union(&rect))
                    .or_insert(rect);
                if let BoxContent::Svg(svg) = &b.content {
                    // The elements inside an `<svg>` have boxes of their
                    // own (their bounding boxes); the `<svg>` is placed
                    // like any box.
                    let content = b
                        .content_rect()
                        .translate(layout_rect.origin() - b.border_rect.origin());
                    let size = crate::geom::Size::new(content.width, content.height);
                    for (node, inner) in svg.element_boxes(size) {
                        let r = matrix.map_rect(&inner.translate(content.origin()));
                        let clamp = crate::geom::clamp_length;
                        boxes.insert(
                            node,
                            Rect::new(clamp(r.x), clamp(r.y), clamp(r.width), clamp(r.height)),
                        );
                    }
                }
            }
        });
        boxes
    }

    /// The absolute border boxes of all fragments generated by `node`, in
    /// tree order. An inline element split across lines has several.
    pub fn border_boxes(&self, node: NodeId) -> Vec<Rect> {
        let mut out = Vec::new();
        self.walk(|f, origin| {
            if let FragmentRef::Box(b) = f
                && b.node == Some(node)
                && b.pseudo.is_none()
            {
                out.push(b.border_rect.translate(origin));
            }
        });
        out
    }
}

/// A fragment.
#[derive(Clone, Debug)]
pub enum Fragment {
    /// A box (block, inline box piece, atomic inline, replaced element).
    Box(BoxFragment),
    /// A run of glyphs on one line.
    Text(TextFragment),
}

impl Fragment {
    /// Moves the fragment by (`dx`, `dy`) relative to its parent.
    pub(crate) fn move_by(&mut self, dx: f32, dy: f32) {
        let rect = match self {
            Fragment::Box(b) => &mut b.border_rect,
            Fragment::Text(t) => &mut t.rect,
        };
        rect.x += dx;
        rect.y += dy;
    }
}

/// What a box fragment contains besides its children.
#[derive(Clone, Debug, PartialEq)]
pub enum BoxContent {
    /// Nothing special.
    None,
    /// The image of a replaced element, painted into the content box.
    Image(NodeId),
    /// A table. Its border box contains its captions; the table's
    /// background and border are painted around the grid.
    Table(Arc<TablePaint>),
    /// A table cell that paints more than its own background and border.
    TableCell(Arc<CellPaint>),
    /// A table row group or row. It paints no background or border (cells
    /// paint them, see [`CellPaint`]).
    TablePart,
    /// A box that only has geometry: it paints nothing and hit testing
    /// does not find it. Table column groups and columns (their boxes cover
    /// their cells), and the part of an inline box around a block-level
    /// child.
    GeometryOnly,
    /// A form control. Its text (value, label) is in its children.
    Control(ControlContent),
    /// A media element (`<video>`, `<audio>`): its poster and controls,
    /// painted into the content box.
    Media(Arc<MediaContent>),
    /// The static position of an absolutely positioned box while layout
    /// runs (see `positioned.rs`). A finished fragment tree has none.
    Placeholder(crate::Placeholder),
    /// An inline `<svg>` element: its content, painted into the content
    /// box.
    Svg(Arc<crate::svg::SvgContent>),
}

/// What a table paints besides its children.
#[derive(Clone, Debug, PartialEq)]
pub struct TablePaint {
    /// The table grid box (the area without captions), relative to the
    /// table's border box. The table's background and border fill it.
    pub grid: Rect,
    /// The borders of the collapsing border model, painted by the table
    /// after its cells; `None` in the separated border model (the table
    /// paints its own border).
    pub collapsed: Option<Vec<CollapsedEdge>>,
}

/// A segment of a collapsed border, relative to the table's border box.
#[derive(Clone, Debug, PartialEq)]
pub struct CollapsedEdge {
    /// The area of the segment.
    pub rect: Rect,
    /// True for a vertical segment (between columns).
    pub vertical: bool,
    /// The border style.
    pub style: swb_style::BorderStyle,
    /// The border color.
    pub color: swb_style::Rgba,
}

/// What a table cell paints besides its own background and border.
#[derive(Clone, Debug, PartialEq)]
pub struct CellPaint {
    /// The backgrounds of its column group, column, row group and row, in
    /// paint order (CSS 2.2 §17.5.1), painted below its own background.
    pub backgrounds: Vec<PartBackground>,
    /// True if the cell paints no background and border
    /// (`empty-cells: hide` and no content).
    pub hidden: bool,
    /// True if the table paints the cell's borders (collapsing borders).
    pub collapsed_borders: bool,
}

/// The background of a table part, painted in a cell.
#[derive(Clone, Debug, PartialEq)]
pub struct PartBackground {
    /// The style with the background.
    pub style: Arc<ComputedStyle>,
    /// The part's border box (the background positioning area), relative
    /// to the cell's border box.
    pub area: Rect,
    /// The area to paint, relative to the cell's border box.
    pub clip: Rect,
}

/// What a media element paints in its content box (see `media.rs`).
#[derive(Clone, Debug, PartialEq)]
pub struct MediaContent {
    /// The element. A video shows the image of this node (its poster),
    /// placed with `object-fit` and `object-position`.
    pub node: NodeId,
    /// True for a video without a `poster` attribute: Chromium fills it
    /// with a dark gray ("default poster").
    pub default_poster: bool,
    /// True for audio controls (their look differs from video controls).
    pub audio: bool,
    /// The parts of the controls in paint order, relative to the content
    /// box.
    pub parts: Vec<MediaPart>,
}

/// One part of the controls of a media element.
#[derive(Clone, Debug, PartialEq)]
pub struct MediaPart {
    /// What the part is.
    pub kind: MediaPartKind,
    /// Its area, relative to the content box. Buttons are 48 × 48 px and
    /// paint draws their icons centered in them.
    pub rect: Rect,
    /// False if the part is dimmed (the control is not available).
    pub enabled: bool,
}

/// The kinds of parts of media controls.
#[derive(Clone, Debug, PartialEq)]
pub enum MediaPartKind {
    /// The background of the controls: a gradient at the bottom of a
    /// video, a rounded panel behind audio controls.
    Panel,
    /// The play button.
    Play,
    /// The current time (and the duration of audio): the glyphs of the
    /// text. The part's area is the text's content area (ascent to
    /// descent) and starts at the pen position.
    Time(MediaText),
    /// The timeline (the seek bar).
    Timeline,
    /// The mute button.
    Mute,
    /// The fullscreen button.
    Fullscreen,
    /// The overflow menu button.
    Menu,
}

/// Shaped text of media controls.
#[derive(Clone, Debug, PartialEq)]
pub struct MediaText {
    /// The font.
    pub font: FontId,
    /// The font size in px.
    pub size: f32,
    /// The glyphs, relative to the pen position on the baseline.
    pub glyphs: Arc<[PositionedGlyph]>,
    /// The baseline, relative to the top of the part's area.
    pub baseline: f32,
}

/// What paint needs to know about a form control.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControlContent {
    /// The kind of control.
    pub kind: crate::ControlKind,
    /// True for a checked checkbox or radio button.
    pub checked: bool,
    /// True if the control is disabled.
    pub disabled: bool,
    /// True if the control has the native look (its border and background
    /// are the user-agent defaults); paint then draws it as Chromium's
    /// theme does instead of with its CSS border and background.
    pub native: bool,
    /// The text caret, relative to the border box, if the control is
    /// focused and editable.
    pub caret: Option<Rect>,
    /// The scroll offset of the text that layout used. The next layout
    /// starts from it, so that the text does not jump.
    pub scroll: Point,
}

/// The fragment of a box.
#[derive(Clone, Debug)]
pub struct BoxFragment {
    /// The element that generated the box; `None` for anonymous boxes.
    pub node: Option<NodeId>,
    /// The pseudo-element, if the box was generated by one.
    pub pseudo: Option<PseudoKind>,
    /// The box's computed style.
    pub style: Arc<ComputedStyle>,
    /// The border box, relative to the parent fragment's border-box origin.
    pub border_rect: Rect,
    /// Border widths. For inline boxes split across lines, the sides at the
    /// split are zero.
    pub border: Edges,
    /// Padding widths (split sides zero, as for `border`).
    pub padding: Edges,
    /// Replaced content.
    pub content: BoxContent,
    /// Child fragments. Shared, so that copying a fragment is cheap: the
    /// flex item cache returns copies of laid-out subtrees.
    pub children: Arc<Vec<Fragment>>,
    /// The baseline of the first line box in the box (or of the first flex
    /// item that has one), relative to the border-box top, if any.
    pub first_baseline: Option<f32>,
    /// The baseline of the last line box in the box, relative to the
    /// border-box top, if any. The baseline of an inline-block. For a flex
    /// container, the same as `first_baseline`.
    pub last_baseline: Option<f32>,
    /// True if this fragment is part of an inline box (not a block).
    pub is_inline: bool,
    /// The scrollable overflow rectangle (CSS Overflow 3 §2.2), relative
    /// to the border-box origin, if the box is a scroll container. It
    /// starts at the padding box (see `scroll.rs`).
    pub scrollable_overflow: Option<Rect>,
    /// True for a block-level box inside a positioned inline box (block in
    /// inline): that inline box is the containing block of the absolutely
    /// positioned boxes in it, but it is not an ancestor in the fragment
    /// tree (its fragments are siblings of this one, right before it).
    /// Positioned layout (`positioned.rs`), scrolling (`scroll.rs`),
    /// paint's clips and the selection clip use it.
    pub in_positioned_inline: bool,
    /// For an inline box on a line where white space hangs at the end
    /// (`white-space: pre-wrap`): the end of the line's content, relative
    /// to the box's own border-box origin. The box and its inline content
    /// after it do not count in scrollable overflow (as in Chromium); see
    /// `scroll::box_overflow_rect`.
    pub hanging_from: Option<f32>,
    /// The children of a box with hidden contents
    /// ([`ComputedStyle::contents_hidden`]: the content of a closed
    /// `details`), moved out of `children` when layout is done. They are
    /// laid out, so that the boxes of their elements are known
    /// ([`FragmentTree::element_boxes`]), but paint, hit testing, the
    /// selection and scrolling do not see them. Positions are relative to
    /// this box as for `children`. `None` for other boxes.
    pub hidden: Option<Arc<Vec<Fragment>>>,
}

impl BoxFragment {
    /// The padding box, relative to the parent's border-box origin.
    pub fn padding_rect(&self) -> Rect {
        self.border_rect.inset(&self.border)
    }

    /// The element whose scroll offset moves the content of this box: the
    /// element of a scroll container. Pseudo-element boxes and anonymous
    /// boxes cannot be scrolled.
    pub fn scroll_node(&self) -> Option<NodeId> {
        self.scrollable_overflow?;
        self.node.filter(|_| self.pseudo.is_none())
    }

    /// The padding box relative to the box's own border-box origin: the
    /// scrollport of a scroll container.
    pub fn scrollport(&self) -> Rect {
        let size = self.border_rect;
        Rect::new(0.0, 0.0, size.width, size.height).inset(&self.border)
    }

    /// The largest scroll offset of a scroll container on each axis (zero
    /// for other boxes): how far the scrollable overflow extends beyond
    /// the scrollport.
    pub fn max_scroll_offset(&self) -> Point {
        let Some(overflow) = self.scrollable_overflow else {
            return Point::default();
        };
        let port = self.scrollport();
        // The overflow rectangle starts at the scrollport.
        scroll_range(
            crate::geom::Size::new(overflow.width, overflow.height),
            crate::geom::Size::new(port.width, port.height),
        )
    }

    /// The scroll offset of this box from `offsets`, clamped to its scroll
    /// range; zero if the box cannot be scrolled.
    pub fn scroll_offset(&self, offsets: &dyn ScrollOffsets) -> Point {
        let Some(node) = self.scroll_node() else {
            return Point::default();
        };
        clamp_scroll_offset(offsets.scroll_offset(node), self.max_scroll_offset())
    }

    /// The content box, relative to the parent's border-box origin.
    pub fn content_rect(&self) -> Rect {
        self.padding_rect().inset(&self.padding)
    }
}

/// A positioned glyph.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PositionedGlyph {
    /// The glyph.
    pub id: GlyphId,
    /// Horizontal pen position relative to the text fragment's left edge.
    pub x: f32,
    /// Vertical offset from the baseline (down is positive).
    pub y: f32,
}

/// A caret stop in a text fragment: a boundary between glyph clusters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Caret {
    /// Byte offset in the data of the text node. Always a character
    /// boundary.
    pub offset: u32,
    /// Horizontal position relative to the fragment's left edge.
    pub x: f32,
}

/// A run of glyphs in one font on one line.
#[derive(Clone, Debug)]
pub struct TextFragment {
    /// The text node.
    pub node: NodeId,
    /// The style of the text (the parent element's style).
    pub style: Arc<ComputedStyle>,
    /// The text's content area (font ascent to descent), relative to the
    /// parent fragment's border-box origin.
    pub rect: Rect,
    /// The baseline position, relative to `rect.y`.
    pub baseline: f32,
    /// The font.
    pub font: FontId,
    /// The font size in px.
    pub font_size: f32,
    /// The glyphs.
    pub glyphs: Arc<[PositionedGlyph]>,
    /// The text of the run (after white-space processing). For debugging
    /// dumps, and for the hanging white space at its end
    /// (`scroll::text_overflow_rect`).
    pub text: Arc<str>,
    /// The caret stops in visual order, from the left edge to the right
    /// edge of the glyphs. Empty for generated content (pseudo-elements,
    /// list markers), which cannot be selected.
    pub carets: Arc<[Caret]>,
    /// The top of the line box that contains the fragment, relative to
    /// `rect.y` (the selection highlight fills the line box).
    pub line_top: f32,
    /// The height of the line box.
    pub line_height: f32,
}

impl TextFragment {
    /// True if the text comes from a text node and can be selected
    /// (`user-select` is not `none`).
    pub fn is_selectable(&self) -> bool {
        !self.carets.is_empty() && self.style.user_select != swb_style::UserSelect::None
    }

    /// The range of node offsets that the fragment shows.
    pub fn node_range(&self) -> Option<(u32, u32)> {
        Some((self.carets.first()?.offset, self.carets.last()?.offset))
    }

    /// The node offset of the caret stop nearest to `x` (relative to the
    /// fragment's left edge). The caret stops are searched one by one:
    /// their x positions decrease with a large negative `letter-spacing`.
    pub fn offset_at(&self, x: f32) -> Option<u32> {
        self.carets
            .iter()
            .min_by(|a, b| (a.x - x).abs().total_cmp(&(b.x - x).abs()))
            .map(|c| c.offset)
    }

    /// The node offset of the start of the cluster at `x` (relative to the
    /// fragment's left edge): the rightmost cluster start at or before `x`,
    /// or the leftmost one if `x` is before all of them.
    pub fn cluster_at(&self, x: f32) -> Option<u32> {
        // The end stop starts no cluster.
        let starts = self.carets.get(..self.carets.len().checked_sub(1)?)?;
        starts
            .iter()
            .filter(|c| c.x <= x)
            .max_by(|a, b| a.x.total_cmp(&b.x))
            .or_else(|| starts.iter().min_by(|a, b| a.x.total_cmp(&b.x)))
            .map(|c| c.offset)
    }

    /// The horizontal extent (relative to the fragment's left edge) of the
    /// node offsets `start..end`, if it is not empty.
    pub fn x_range(&self, start: u32, end: u32) -> Option<(f32, f32)> {
        let left = self.carets.iter().find(|c| c.offset >= start)?;
        let right = self.carets.iter().rev().find(|c| c.offset <= end)?;
        (right.x > left.x).then_some((left.x, right.x))
    }
}
