//! Absolutely positioned and fixed boxes, sticky offsets and transforms.
//!
//! CSS 2.2 §10.1 (containing blocks), §10.3.7, §10.3.8, §10.6.4 and §10.6.5
//! (sizes and positions of absolutely positioned boxes):
//! <https://www.w3.org/TR/CSS22/visudet.html#containing-block-details>;
//! CSS Positioned Layout 3 (<https://www.w3.org/TR/css-position-3/>);
//! CSS Transforms 1 (<https://www.w3.org/TR/css-transforms-1/>). Design:
//! ADR 0016.
//!
//! Absolutely positioned boxes are laid out after the normal flow:
//!
//! 1. Block, inline and flex layout put a *placeholder* (a fragment of size
//!    0) where the box would have been: its static position. The
//!    placeholder moves with its parent like any other fragment, also
//!    when the layout cache of flex items and table cells reuses a
//!    fragment.
//! 2. After the whole document is laid out, [`place_out_of_flow`] walks
//!    the fragment tree once, top-down. It keeps the containing block of
//!    absolutely positioned descendants (the padding box of the nearest
//!    positioned or transformed ancestor, or the initial containing block)
//!    and of fixed descendants (the nearest transformed ancestor, or the
//!    viewport), in the coordinates of the fragment it visits. At a
//!    placeholder, the sizes of all containing blocks are known: it lays
//!    out the box and puts its fragment in place of the placeholder,
//!    positioned relative to the placeholder's parent.
//!
//! So every fragment stays positioned relative to its parent (the walkers
//! of the fragment tree need no change), and the fragment of a positioned
//! box is at its place in tree order (paint order). Paint applies only the
//! overflow clips of the boxes in the containing block chain (see
//! `swb_paint::display_list`).
//!
//! A positioned inline box is the containing block of its absolutely
//! positioned descendants: the rectangle from the top-left padding edge of
//! its first fragment to the bottom-right padding edge of its last one, in
//! the same block container (CSS 2.2 §10.1, item 4, as in Chromium). Also
//! for the boxes in a block inside it (block in inline): the block is not
//! a descendant of the inline box's fragments, but carries
//! [`BoxFragment::in_positioned_inline`], and the fragments of the inline
//! box around it (`BoxContent::GeometryOnly`) come right before it.
//!
//! A scroll container computes its scrollable overflow when it is laid
//! out, before the out-of-flow pass; the pass adds the boxes that it
//! places to the scrollable overflow of the scroll containers above them
//! (`scroll::with_out_of_flow`).
//!
//! Sticky boxes are laid out at their normal position; paint and hit
//! testing add the offset for the current scroll position
//! ([`StickyConstraints`]). Transforms do not change layout either
//! ([`transform_matrix`]).

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use swb_dom::NodeId;
use swb_style::{
    Alignment, ComputedStyle, FlexWrap, LengthPercentageOrAuto, Position, PseudoKind,
    Size as StyleSize, TransformFunction,
};

use crate::LayoutContext;
use crate::align::{Edge, resolve_self_alignment};
use crate::block::{
    BoxEdges, ContainingBlock, clamp_height, clamp_width, finish_fragment, layout_sized,
    margin_or_zero, resolve_size,
};
use crate::box_tree::{
    BlockContainer, BlockLevelBox, BoxBase, IndependentBox, IndependentContents, InlineItem,
};
use crate::fragment::{BoxContent, BoxFragment, Fragment};
use crate::geom::{Edges, Matrix, Point, Rect, Size, clamp_length};

/// Where the static position of an absolutely positioned box comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StaticParent {
    /// Block layout: the box is aligned in the static-position rectangle
    /// at the placeholder's position ([`Placeholder::extent`]); `auto`
    /// takes the parent's `justify-items`.
    Flow,
    /// Inline layout: as [`StaticParent::Flow`], but `auto` is `normal`
    /// (the `justify-items` of the parent does not count, as in Chromium
    /// 148).
    Inline,
    /// A child of a flex container: aligned in the container's content box
    /// as if it were the only flex item (CSS Flexbox 1 §4.1).
    Flex,
    /// A child of a grid container: aligned in the container's padding box
    /// if it is the containing block, else in its content box (CSS Grid 2
    /// §9.4 and §10.2).
    Grid,
}

/// The place of an absolutely positioned box in the fragment tree while
/// layout runs (see the module documentation). A finished fragment tree
/// has no placeholders.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placeholder {
    /// The number of the box ([`BoxBase::id`]).
    box_id: usize,
    parent: StaticParent,
    /// For [`StaticParent::Flow`] and [`StaticParent::Inline`]: the size
    /// of the static-position rectangle from the placeholder's position,
    /// in which `justify-self` and `align-self` align the box (CSS
    /// Position 3 §4.4, measured with Chromium 148). In block layout, the
    /// width of the container's content box and no height. In inline
    /// layout, no width and the height of the line box (for a box that was
    /// block-level and follows content on its line, the rectangle starts
    /// at the bottom of that line).
    extent: Size,
}

impl Placeholder {
    /// Sets the size of the static-position rectangle (see
    /// [`Placeholder::extent`]).
    pub(crate) fn set_extent(&mut self, extent: Size) {
        self.extent = extent;
    }
}

/// A placeholder fragment for the absolutely positioned box `ib` at `at`
/// (relative to the content box of the container being laid out, like
/// the container's other children), with the static-position rectangle
/// `extent` (see [`Placeholder::extent`]).
pub(crate) fn placeholder(
    ctx: &mut LayoutContext<'_>,
    ib: &IndependentBox,
    at: Point,
    parent: StaticParent,
    extent: Size,
) -> Fragment {
    ctx.placeholders += 1;
    let base = BoxBase {
        node: None,
        pseudo: None,
        style: Arc::clone(&ib.base.style),
        id: ib.base.id,
    };
    let rect = Rect::new(at.x, at.y, 0.0, 0.0);
    let mut fragment = finish_fragment(
        &base,
        rect,
        &BoxEdges::default(),
        Vec::new(),
        crate::block::Baselines::default(),
    );
    fragment.content = BoxContent::Placeholder(Placeholder {
        box_id: ib.base.id,
        parent,
        extent,
    });
    Fragment::Box(fragment)
}

/// Adds the placeholders of the absolutely positioned `children` of a
/// flex or grid container (`parent`) to the fragments of its items. They go
/// between the item fragments in document order: they paint as items with
/// `order: 0` would (CSS Flexbox 1 §5.4, CSS Grid 2 §9). With `order` or a
/// reverse direction, the item fragments are not in document order; the
/// placeholders then keep their position among the fragments.
pub(crate) fn add_placeholders(
    ctx: &mut LayoutContext<'_>,
    children: &[IndependentBox],
    fragments: &mut Vec<Fragment>,
    parent: StaticParent,
) {
    if !children
        .iter()
        .any(|c| c.base.style.is_absolutely_positioned())
    {
        return;
    }
    let mut items = std::mem::take(fragments).into_iter();
    let mut merged = Vec::with_capacity(children.len());
    for child in children {
        if child.base.style.is_absolutely_positioned() {
            merged.push(placeholder(
                ctx,
                child,
                Point::default(),
                parent,
                Size::default(),
            ));
        } else {
            merged.extend(items.next());
        }
    }
    merged.extend(items);
    *fragments = merged;
}

// ----- Containing blocks -----

/// True if the box has a transform. Transforms apply to block-level and
/// atomic boxes, not to inline boxes (CSS Transforms 1 §3,
/// "transformable element").
pub fn has_transform(fragment: &BoxFragment) -> bool {
    fragment.style.has_transform() && !fragment.is_inline
}

/// True if the box is the containing block of its absolutely positioned
/// descendants: positioned (CSS 2.2 §10.1), transformed (CSS Transforms 1
/// §1), or with layout containment (hidden contents, CSS Containment 2
/// §3.2).
pub fn is_absolute_containing_block(fragment: &BoxFragment) -> bool {
    fragment.style.position != Position::Static
        || has_transform(fragment)
        || fragment.style.contents_hidden
}

/// True if the box is the containing block of its fixed descendants
/// instead of the viewport: transformed. Deviation: layout containment
/// (hidden contents) does not count, so a fixed box in the hidden
/// contents of a closed `details` is relative to the viewport.
pub fn is_fixed_containing_block(fragment: &BoxFragment) -> bool {
    has_transform(fragment)
}

/// The area of the `clip` property of an absolutely positioned box whose
/// border box is at `border_rect` (`auto` edges are the border box edges),
/// or `None` if the box has no such clip
/// (<https://www.w3.org/TR/CSS22/visufx.html#clipping>).
pub fn clip_property_area(fragment: &BoxFragment, border_rect: Rect) -> Option<Rect> {
    let style = &fragment.style;
    let clip = style.clip.filter(|_| style.is_absolutely_positioned())?;
    let left = clip.left.unwrap_or(0.0);
    let top = clip.top.unwrap_or(0.0);
    let right = clip.right.unwrap_or(border_rect.width);
    let bottom = clip.bottom.unwrap_or(border_rect.height);
    Some(Rect::new(
        border_rect.x + left,
        border_rect.y + top,
        (right - left).max(0.0),
        (bottom - top).max(0.0),
    ))
}

/// True if the box forms a stacking context in paint (CSS 2.2 Appendix E,
/// CSS Transforms 1 §3, CSS Masking 1): a transform, fixed or sticky
/// positioning, a positioned box with an integer `z-index`, opacity below
/// 1, or a mask. The root element forms one whatever its style; this
/// function does not check for it (callers start in the root's context).
pub fn forms_stacking_context(fragment: &BoxFragment) -> bool {
    let style = &fragment.style;
    has_transform(fragment)
        || matches!(style.position, Position::Fixed | Position::Sticky)
        || (style.position != Position::Static && style.z_index != swb_style::ZIndex::Auto)
        || style.opacity < 1.0
        || style.has_mask()
}

/// The padding box of a fragment, relative to its own border-box origin.
fn own_padding_box(fragment: &BoxFragment) -> Rect {
    let r = fragment.border_rect;
    Rect::new(0.0, 0.0, r.width, r.height).inset(&fragment.border)
}

/// The content box of a fragment, relative to its own border-box origin.
fn own_content_box(fragment: &BoxFragment) -> Rect {
    own_padding_box(fragment).inset(&fragment.padding)
}

/// The containing blocks of the positioned inline boxes in one block
/// container, by element: the rectangle from the top-left padding edge
/// of the first fragment to the bottom-right padding edge of the last
/// one, relative to the block container. The fragments of an inline box
/// around blocks (block in inline) are in the anonymous blocks of the
/// container and around the blocks.
#[derive(Default)]
struct InlineContainingBlocks {
    rects: HashMap<(Option<NodeId>, Option<PseudoKind>), (Rect, Rect)>,
}

impl InlineContainingBlocks {
    /// The positioned inline boxes among the inline-level descendants of
    /// `block`, or `None` if there are none.
    fn of(block: &BoxFragment) -> Option<Self> {
        /// Adds the positioned inline boxes among the inline children of
        /// a fragment whose border-box origin is at `origin`, in tree order.
        fn scan(parent: &BoxFragment, origin: Point, found: &mut Option<InlineContainingBlocks>) {
            for child in parent.children.iter() {
                let Fragment::Box(b) = child else {
                    continue;
                };
                if !b.is_inline {
                    if b.node.is_none() {
                        scan(b, origin + b.border_rect.origin(), found);
                    }
                    continue;
                }
                if b.style.position != Position::Static {
                    let rect = b.padding_rect().translate(origin);
                    found
                        .get_or_insert_with(InlineContainingBlocks::default)
                        .rects
                        .entry((b.node, b.pseudo))
                        .and_modify(|(_, last)| *last = rect)
                        .or_insert((rect, rect));
                }
                scan(b, origin + b.border_rect.origin(), found);
            }
        }
        let mut found = None;
        scan(block, Point::default(), &mut found);
        found
    }

    /// The containing block of the element of the inline fragment `b`,
    /// relative to the block container.
    fn get(&self, b: &BoxFragment) -> Option<Rect> {
        let (first, last) = self.rects.get(&(b.node, b.pseudo))?;
        Some(Rect::new(
            first.x,
            first.y,
            (last.right() - first.x).max(0.0),
            (last.bottom() - first.y).max(0.0),
        ))
    }
}

/// The containing blocks for the descendants of the fragment being
/// visited, in the coordinates of its border box.
#[derive(Clone, Copy)]
struct Scope<'m> {
    /// For absolutely positioned descendants.
    absolute: Rect,
    /// For fixed descendants.
    fixed: Rect,
    /// The containing blocks of the positioned inline boxes of the
    /// enclosing block container, and that container's border-box origin
    /// in the coordinates of the fragment being visited.
    inline: Option<(&'m InlineContainingBlocks, Point)>,
}

impl Scope<'_> {
    /// The scope in the coordinates of a child at `at`.
    fn translated(self, at: Point) -> Self {
        let back = Point::new(-at.x, -at.y);
        Scope {
            absolute: self.absolute.translate(back),
            fixed: self.fixed.translate(back),
            inline: self.inline.map(|(m, o)| (m, o + back)),
        }
    }
}

// ----- The out-of-flow pass -----

/// Lays out the absolutely positioned and fixed boxes of a laid-out
/// document and puts them in place of their placeholders (see the module
/// documentation). `root` is the fragment of `root_box`.
pub(crate) fn place_out_of_flow(
    ctx: &mut LayoutContext<'_>,
    root_box: &IndependentBox,
    root: &mut BoxFragment,
    viewport: Size,
) {
    if ctx.placeholders == 0 {
        return;
    }
    // The cached fragments share their children with the tree; without
    // the cache, the walk below changes the tree without copies.
    ctx.layouts.clear();
    let pass = OutOfFlow {
        boxes: out_of_flow_boxes(root_box),
    };
    let icb = Rect::new(
        -root.border_rect.x,
        -root.border_rect.y,
        viewport.width,
        viewport.height,
    );
    let scope = Scope {
        absolute: icb,
        fixed: icb,
        inline: None,
    };
    // No scroll container is above the root, so "placed" is not needed.
    let _ = pass.visit(ctx, root, scope);
}

/// The absolutely positioned boxes of a box tree, by box number.
fn out_of_flow_boxes(root: &IndependentBox) -> HashMap<usize, &IndependentBox> {
    enum Item<'t> {
        Box(&'t IndependentBox),
        Container(&'t BlockContainer),
    }
    let mut boxes = HashMap::new();
    let mut stack = vec![Item::Box(root)];
    while let Some(item) = stack.pop() {
        match item {
            Item::Box(ib) => {
                if ib.base.style.is_absolutely_positioned() {
                    boxes.insert(ib.base.id, ib);
                }
                match &ib.contents {
                    IndependentContents::Flow(c) => stack.push(Item::Container(c)),
                    IndependentContents::Flex(items) | IndependentContents::Grid(items) => {
                        stack.extend(items.iter().map(Item::Box));
                    }
                    IndependentContents::Table(table) => {
                        table.for_each_box(|b| stack.push(Item::Box(b)));
                    }
                    IndependentContents::Control(control) => match &control.contents {
                        crate::control::ControlContents::Flow(c) => {
                            stack.push(Item::Container(c));
                        }
                        crate::control::ControlContents::Flex(items) => {
                            stack.extend(items.iter().map(Item::Box));
                        }
                        crate::control::ControlContents::None
                        | crate::control::ControlContents::Text { .. } => {}
                    },
                    IndependentContents::Replaced(_) => {}
                }
            }
            Item::Container(BlockContainer::Blocks(children)) => {
                for child in children {
                    let child = match child {
                        BlockLevelBox::InInline(b) => &b.block,
                        other => other,
                    };
                    match child {
                        BlockLevelBox::Block { contents, .. } => {
                            stack.push(Item::Container(contents));
                        }
                        BlockLevelBox::Independent(ib)
                        | BlockLevelBox::Float(ib)
                        | BlockLevelBox::AbsolutelyPositioned(ib) => stack.push(Item::Box(ib)),
                        // Box construction never nests these.
                        BlockLevelBox::InInline(_) => {}
                    }
                }
            }
            Item::Container(BlockContainer::Inline(ifc)) => {
                for item in &ifc.items {
                    match item {
                        InlineItem::Atomic { inner, .. } => stack.push(Item::Box(inner)),
                        InlineItem::Float(ib) | InlineItem::AbsolutelyPositioned(ib) => {
                            stack.push(Item::Box(ib));
                        }
                        InlineItem::StartBox { .. }
                        | InlineItem::EndBox { .. }
                        | InlineItem::Text { .. }
                        | InlineItem::LineBreak(_) => {}
                    }
                }
            }
        }
    }
    boxes
}

/// The state of the out-of-flow pass.
struct OutOfFlow<'t> {
    boxes: HashMap<usize, &'t IndependentBox>,
}

impl OutOfFlow<'_> {
    /// Visits `f` and its descendants. `outer` are the containing blocks
    /// of `f`'s ancestors, relative to `f`. Returns true if it placed a box
    /// (the scrollable overflow of the scroll containers above can grow).
    fn visit(&self, ctx: &mut LayoutContext<'_>, f: &mut BoxFragment, outer: Scope<'_>) -> bool {
        let mut scope = outer;
        if f.is_inline {
            if f.style.position != Position::Static
                && let Some((cbs, origin)) = outer.inline
                && let Some(rect) = cbs.get(f)
            {
                scope.absolute = rect.translate(origin);
            }
        } else {
            if is_absolute_containing_block(f) {
                scope.absolute = own_padding_box(f);
            }
            if is_fixed_containing_block(f) {
                scope.fixed = own_padding_box(f);
            }
        }
        // An anonymous block is part of the inline content of its parent.
        let container = !f.is_inline && f.node.is_some();
        let inline_cbs = if container {
            InlineContainingBlocks::of(f)
        } else {
            None
        };
        if container {
            scope.inline = inline_cbs.as_ref().map(|m| (m, Point::default()));
        }
        let content = own_content_box(f);
        let padding = own_padding_box(f);
        let contains = (
            is_absolute_containing_block(f),
            is_fixed_containing_block(f),
        );
        let BoxFragment {
            style, children, ..
        } = f;
        let parent = StaticContext {
            style,
            content,
            padding,
            contains,
        };
        let mut dropped = false;
        let mut placed = false;
        // The containing block of the positioned inline box around the next
        // block (block in inline): the innermost positioned one of the
        // fragments right before it.
        let mut around: Option<Rect> = None;
        for child in Arc::make_mut(children) {
            let Fragment::Box(b) = child else {
                around = None;
                continue;
            };
            if let BoxContent::Placeholder(p) = b.content {
                match self.lay_out(ctx, p, b.border_rect.origin(), &parent, scope) {
                    Some(fragment) => {
                        *b = fragment;
                        placed = true;
                    }
                    None => dropped = true,
                }
                around = None;
                continue;
            }
            if b.is_inline && b.content == BoxContent::GeometryOnly {
                if b.style.position != Position::Static
                    && let Some((cbs, origin)) = scope.inline
                    && let Some(rect) = cbs.get(b)
                {
                    around = Some(rect.translate(origin));
                }
                continue;
            }
            let at = b.border_rect.origin();
            let mut inner = scope;
            if b.in_positioned_inline
                && !b.is_inline
                && let Some(rect) = around
            {
                inner.absolute = rect;
            }
            placed |= self.visit(ctx, b, inner.translated(at));
            around = None;
        }
        if dropped {
            Arc::make_mut(children).retain(
                |c| !matches!(c, Fragment::Box(b) if matches!(b.content, BoxContent::Placeholder(_))),
            );
        }
        if placed && let Some(overflow) = f.scrollable_overflow {
            f.scrollable_overflow = Some(crate::scroll::with_out_of_flow(f, overflow));
        }
        placed
    }

    /// Lays out the box of placeholder `p` (at `at` in `parent`) and its
    /// out-of-flow descendants. `None` if the box is not known.
    fn lay_out(
        &self,
        ctx: &mut LayoutContext<'_>,
        p: Placeholder,
        at: Point,
        parent: &StaticContext<'_>,
        scope: Scope<'_>,
    ) -> Option<BoxFragment> {
        let Some(&ib) = self.boxes.get(&p.box_id) else {
            // `out_of_flow_boxes` misses a kind of container.
            debug_assert!(false, "no box for a placeholder");
            log::error!("no box for an absolutely positioned placeholder; it is not laid out");
            return None;
        };
        let style = &ib.base.style;
        let cb = if style.position == Position::Fixed {
            scope.fixed
        } else {
            scope.absolute
        };
        // `justify-self: auto` takes the parent's `justify-items` (not in
        // inline layout); `align-self: auto` the parent's `align-items` in
        // a grid, not in a block container (Chromium 148).
        let justify = match (style.justify_self, p.parent) {
            (Alignment::Auto, StaticParent::Inline) => Alignment::Normal,
            (Alignment::Auto, _) => parent.style.justify_items,
            (a, _) => a,
        };
        let position = match p.parent {
            StaticParent::Flow | StaticParent::Inline => {
                let rect = Rect::new(at.x, at.y, p.extent.width, p.extent.height);
                StaticPosition::aligned(rect, justify, style.align_self)
            }
            StaticParent::Flex => flex_static_position(parent.style, parent.content, style),
            // The padding box if the grid container is the containing block
            // (the area of `auto` lines, CSS Grid 2 §9.4), else the content
            // box (§10.2), as in Chromium.
            StaticParent::Grid => {
                let is_containing_block = if style.position == Position::Fixed {
                    parent.contains.1
                } else {
                    parent.contains.0
                };
                let rect = if is_containing_block {
                    parent.padding
                } else {
                    parent.content
                };
                let align = resolve_self_alignment(style.align_self, parent.style.align_items);
                StaticPosition::aligned(rect, justify, align)
            }
        };
        let mut fragment = layout_absolute(ctx, ib, cb, position);
        let origin = fragment.border_rect.origin();
        // The caller reports this box as placed, whatever the result.
        let _ = self.visit(ctx, &mut fragment, scope.translated(origin));
        Some(fragment)
    }
}

/// What the static position in a container depends on.
struct StaticContext<'s> {
    style: &'s ComputedStyle,
    /// The container's content box, relative to its border box.
    content: Rect,
    /// The container's padding box, relative to its border box.
    padding: Rect,
    /// True if the container is the containing block of its absolutely
    /// positioned children, and of its fixed children.
    contains: (bool, bool),
}

// ----- Static positions -----

/// The static position of an absolutely positioned box: a point and the
/// edges of its margin box that are there.
#[derive(Clone, Copy, Debug, PartialEq)]
struct StaticPosition {
    point: Point,
    x: Edge,
    y: Edge,
}

impl StaticPosition {
    /// The static position of a box aligned by `justify` and `align` in
    /// the static-position rectangle `rect` (CSS Position 3 §4.4). As in
    /// Chromium, an overflowing box is not moved back (unsafe alignment).
    fn aligned(rect: Rect, justify: Alignment, align: Alignment) -> Self {
        let x = edge_of(justify, true);
        let y = edge_of(align, false);
        StaticPosition {
            point: Point::new(x.along(rect.x, rect.width), y.along(rect.y, rect.height)),
            x,
            y,
        }
    }
}

/// The edge of the margin box that a self-alignment value puts at the
/// alignment point (`left` and `right` only in the horizontal axis);
/// `normal`, `stretch`, `baseline` and the distributed values align at
/// the start.
fn edge_of(alignment: Alignment, horizontal: bool) -> Edge {
    match alignment {
        Alignment::Center => Edge::Center,
        Alignment::End | Alignment::FlexEnd | Alignment::SelfEnd => Edge::End,
        Alignment::Right if horizontal => Edge::End,
        _ => Edge::Start,
    }
}

/// True if a self-alignment value lets an `auto` size fill the
/// inset-modified containing block (`normal` and `stretch`).
fn stretches(alignment: Alignment) -> bool {
    matches!(
        alignment,
        Alignment::Auto | Alignment::Normal | Alignment::Stretch
    )
}

/// The static position of an absolutely positioned child (style
/// `child`) of a flex container with content box `content`: as if it were
/// the only flex item (CSS Flexbox 1 §4.1,
/// <https://www.w3.org/TR/css-flexbox-1/#abspos-items>), aligned by
/// `justify-content` and `align-self`. A distributed alignment with one
/// item is `flex-start` (`space-between`) or `center` (`space-around`,
/// `space-evenly`).
fn flex_static_position(
    container: &ComputedStyle,
    content: Rect,
    child: &ComputedStyle,
) -> StaticPosition {
    let row = container.flex_direction.is_row();
    let reverse = container.flex_direction.is_reverse();
    let wrap_reverse = container.flex_wrap == FlexWrap::WrapReverse;
    // `flex-start` and `flex-end` swap in reverse directions; `start`,
    // `end`, `left` and `right` do not depend on the direction (`left` and
    // `right` apply to the horizontal axis only).
    let (flex_start, flex_end) = if reverse {
        (Edge::End, Edge::Start)
    } else {
        (Edge::Start, Edge::End)
    };
    let main = match container.justify_content {
        Alignment::Center | Alignment::SpaceAround | Alignment::SpaceEvenly => Edge::Center,
        Alignment::End => Edge::End,
        Alignment::Right if row => Edge::End,
        Alignment::Start | Alignment::Left | Alignment::Right => Edge::Start,
        Alignment::FlexEnd => flex_end,
        // `normal`, `stretch`, `flex-start`, `space-between`.
        _ => flex_start,
    };
    let align = resolve_self_alignment(child.align_self, container.align_items);
    let (cross_start, cross_end) = if wrap_reverse {
        (Edge::End, Edge::Start)
    } else {
        (Edge::Start, Edge::End)
    };
    let cross = match align {
        Alignment::Center => Edge::Center,
        // `baseline` falls back to `start` (Chromium), also with
        // `wrap-reverse`.
        Alignment::Start | Alignment::SelfStart | Alignment::Baseline => Edge::Start,
        Alignment::End | Alignment::SelfEnd => Edge::End,
        Alignment::FlexEnd => cross_end,
        // `normal`, `stretch` (an absolutely positioned box does not
        // stretch here), `flex-start`.
        _ => cross_start,
    };
    let (x, y) = if row { (main, cross) } else { (cross, main) };
    StaticPosition {
        point: Point::new(
            x.along(content.x, content.width),
            y.along(content.y, content.height),
        ),
        x,
        y,
    }
}

// ----- Sizes and positions -----

/// One axis of the constraint of CSS 2.2 §10.3.7 (horizontal) or §10.6.4
/// (vertical): `start + margin-start + edges + size + margin-end + end`
/// equals the size of the containing block.
#[derive(Clone, Copy, Debug)]
struct Axis {
    /// The size of the containing block (its padding box).
    cb: f32,
    /// The insets (`left` and `right`, or `top` and `bottom`); `None` for
    /// `auto`.
    start: Option<f32>,
    end: Option<f32>,
    /// The margins; `None` for `auto`.
    margin_start: Option<f32>,
    margin_end: Option<f32>,
    /// Border and padding.
    edges: f32,
    /// The static position, relative to the start of the containing
    /// block, and the edge of the margin box that is there.
    static_position: f32,
    static_edge: Edge,
    /// True for the horizontal axis: `auto` margins do not become
    /// negative (CSS 2.2 §10.3.7; §10.6.4 has no such rule).
    horizontal: bool,
    /// The self-alignment (`justify-self` or `align-self`; `auto` as
    /// `normal`) in the inset-modified containing block, when both insets
    /// are set (CSS Position 3 §4.4).
    align: Alignment,
}

/// The solution of an [`Axis`]: the content size, the margins, and the
/// start of the margin box relative to the start of the containing
/// block.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Solved {
    size: f32,
    margin_start: f32,
    margin_end: f32,
    offset: f32,
}

impl Axis {
    /// The size that an `auto` size fills: the containing block without
    /// the insets (with both insets `auto`: the space on the side of the
    /// static position, CSS Position 3 §4.1), margins and edges.
    fn available(&self) -> f32 {
        let margins = self.margin_start.unwrap_or(0.0) + self.margin_end.unwrap_or(0.0);
        let space = match (self.start, self.end) {
            (None, None) => match self.static_edge {
                Edge::Start | Edge::Baseline => self.cb - self.static_position,
                Edge::End => self.static_position,
                Edge::Center => 2.0 * self.static_position.min(self.cb - self.static_position),
            },
            (Some(s), None) => self.cb - s,
            (None, Some(e)) => self.cb - e,
            (Some(s), Some(e)) => self.cb - s - e,
        };
        (space - margins - self.edges).max(0.0)
    }

    /// Solves the axis for the content size `size`. `size_auto` is true
    /// if the size property is `auto` (the size came from the content or
    /// from [`Axis::available`]): `auto` margins are then 0.
    fn solve(&self, size: f32, size_auto: bool) -> Solved {
        let outer = |ms: f32, me: f32| ms + self.edges + size + me;
        match (self.start, self.end, size_auto) {
            (Some(start), Some(end), false) => {
                let fixed = self.margin_start.unwrap_or(0.0) + self.margin_end.unwrap_or(0.0);
                let free = self.cb - start - end - self.edges - size - fixed;
                let (ms, me) = match (self.margin_start, self.margin_end) {
                    (None, None) if self.horizontal && free < 0.0 => (0.0, free),
                    (None, None) => (free / 2.0, free / 2.0),
                    (None, Some(me)) => (free, me),
                    (Some(ms), None) => (ms, free),
                    // Over-constrained: the self-alignment places the
                    // margin box in the inset-modified containing block;
                    // an overflowing box stays at the start (safe, as
                    // Chromium does without `unsafe`).
                    (Some(ms), Some(me)) => {
                        let edge = edge_of(self.align, self.horizontal);
                        let shift = if free <= 0.0 {
                            0.0
                        } else {
                            edge.along(0.0, free)
                        };
                        (ms + shift, me)
                    }
                };
                Solved {
                    size,
                    margin_start: ms,
                    margin_end: me,
                    offset: start,
                }
            }
            (start, end, _) => {
                let ms = self.margin_start.unwrap_or(0.0);
                let me = self.margin_end.unwrap_or(0.0);
                let width = outer(ms, me);
                let offset = match (start, end) {
                    (Some(start), _) => start,
                    (None, Some(end)) => self.cb - end - width,
                    (None, None) => self.static_position - self.static_edge.along(0.0, width),
                };
                Solved {
                    size,
                    margin_start: ms,
                    margin_end: me,
                    offset,
                }
            }
        }
    }
}

/// Lays out an absolutely positioned root element in the initial
/// containing block `icb` (its static position is the top-left corner).
pub(crate) fn layout_absolute_root(
    ctx: &mut LayoutContext<'_>,
    root: &IndependentBox,
    icb: ContainingBlock,
) -> BoxFragment {
    let cb = Rect::new(0.0, 0.0, icb.width, icb.height.unwrap_or(0.0));
    let position = StaticPosition::aligned(Rect::default(), Alignment::Start, Alignment::Start);
    layout_absolute(ctx, root, cb, position)
}

/// Lays out the absolutely positioned box `ib` in the containing block
/// `cb` (a padding box) with the static position `position`, both in the
/// same coordinates; the fragment is positioned in them.
fn layout_absolute(
    ctx: &mut LayoutContext<'_>,
    ib: &IndependentBox,
    cb: Rect,
    position: StaticPosition,
) -> BoxFragment {
    let style = &ib.base.style;
    let containing = ContainingBlock {
        width: cb.width,
        height: Some(cb.height),
    };
    let box_edges = BoxEdges::resolve(style, cb.width);
    let edges = box_edges.sum();
    let horizontal = axis(
        cb.width,
        (&style.left, &style.right),
        (&style.margin_left, &style.margin_right),
        cb.width,
        edges.horizontal(),
        (position.point.x - cb.x, position.x),
        (true, style.justify_self),
    );
    let vertical = axis(
        cb.height,
        (&style.top, &style.bottom),
        (&style.margin_top, &style.margin_bottom),
        cb.width,
        edges.vertical(),
        (position.point.y - cb.y, position.y),
        (false, style.align_self),
    );
    let replaced = match &ib.contents {
        IndependentContents::Replaced(r) => {
            Some(crate::replaced::used_size(style, r, containing, &box_edges))
        }
        _ => None,
    };
    // A block, flex or grid container with an `aspect-ratio` and an `auto`
    // width that does not stretch between the insets takes its width from
    // its height, if that is definite: specified, or stretched between
    // `top` and `bottom`. Otherwise the width is as usual and the height
    // follows from it (Chromium; even between `top` and `bottom`).
    let has_ratio = crate::aspect::has_ratio(ib);
    let v_edges = edges.vertical();
    let table = matches!(ib.contents, IndependentContents::Table(_));
    let keyword = matches!(
        style.height,
        StyleSize::MinContent | StyleSize::MaxContent | StyleSize::FitContent(_)
    );
    let specified_h = resolve_size(&style.height, Some(cb.height), style.box_sizing, v_edges);
    let stretched_h = (!table
        && !keyword
        && specified_h.is_none()
        && vertical.start.is_some()
        && vertical.end.is_some()
        && stretches(vertical.align))
    .then(|| vertical.available());
    let stretch_w =
        horizontal.start.is_some() && horizontal.end.is_some() && stretches(horizontal.align);
    let ratio_width = (has_ratio && !stretch_w)
        .then(|| specified_h.or(stretched_h))
        .flatten()
        .and_then(|h| {
            let h = clamp_height(style, h, Some(cb.height), v_edges);
            crate::aspect::width_from_height(style, h, Some(cb.width), &box_edges)
        })
        .map(|w| clamp_width(style, w, cb.width, edges.horizontal()));
    let x = match (replaced, ratio_width) {
        (Some((w, _)), _) | (None, Some(w)) => horizontal.solve(w, false),
        (None, None) => solve_width(ctx, ib, &horizontal, cb.width, edges),
    };
    // Tables size as fit-content in both axes (Chromium): they do not
    // stretch between the insets, and their `auto` margins can center
    // them.
    // The content height and whether it fills the inset-modified
    // containing block, if the height does not depend on the content:
    // replaced, specified, or stretched between `top` and `bottom` (then
    // clamped, CSS 2.2 §10.7). The intrinsic size keywords and a
    // self-alignment other than `normal` and `stretch` take the height of
    // the content (as `solve_width` does for widths). With a ratio and a
    // width that is not from the height, `layout_sized` takes the height
    // from the width.
    let height = if let Some((_, h)) = replaced {
        Some((h, false))
    } else if has_ratio && ratio_width.is_none() && specified_h.is_none() {
        None
    } else {
        specified_h.or(stretched_h).map(|h| {
            let clamped = clamp_height(style, h, Some(cb.height), v_edges);
            (clamped, specified_h.is_none() && clamped == h)
        })
    };
    let fragment = layout_sized(ctx, ib, x.size, height.map(|(h, _)| h), containing);
    // Without a height that is known before layout, the height comes from
    // the content (`layout_sized` applies `min-height` and `max-height`).
    // In the block axis, `fit-content(<length>)` is the content height too
    // (CSS Sizing 3: clamped between the min-content and the max-content
    // height, which are both the content height).
    let (h, size_auto) = height.unwrap_or_else(|| {
        let content = (fragment.border_rect.height - v_edges).max(0.0);
        (content, false)
    });
    let y = vertical.solve(h, size_auto);
    place(fragment, cb, x, y)
}

/// The [`Axis`] of an inset pair: percentages of the insets refer to
/// `cb_size`, those of the margins to the containing block's width.
fn axis(
    cb_size: f32,
    (start, end): (&LengthPercentageOrAuto, &LengthPercentageOrAuto),
    (margin_start, margin_end): (&LengthPercentageOrAuto, &LengthPercentageOrAuto),
    cb_width: f32,
    edges: f32,
    (static_position, static_edge): (f32, Edge),
    (horizontal, align): (bool, Alignment),
) -> Axis {
    let resolve = |v: &LengthPercentageOrAuto, basis: f32| v.resolve(basis).map(clamp_length);
    Axis {
        cb: cb_size,
        start: resolve(start, cb_size),
        end: resolve(end, cb_size),
        margin_start: resolve(margin_start, cb_width),
        margin_end: resolve(margin_end, cb_width),
        edges,
        static_position,
        static_edge,
        horizontal,
        align,
    }
}

/// Solves the horizontal axis of a box that is not replaced: its
/// `width`, or else shrink-to-fit (or the available width if both insets
/// are set), then `max-width` and `min-width` (CSS 2.2 §10.3.7 and
/// §10.4: a clamped width is solved again as if it were specified).
fn solve_width(
    ctx: &mut LayoutContext<'_>,
    ib: &IndependentBox,
    axis: &Axis,
    cb_width: f32,
    edges: Edges,
) -> Solved {
    let style = &ib.base.style;
    let edge_sum = edges.horizontal();
    let available = axis.available();
    let mut sizes = || crate::intrinsic::independent_content_sizes(ctx, ib);
    // The intrinsic size keywords (CSS Sizing 3 §3.2) are not `auto`.
    let specified = match &style.width {
        StyleSize::MinContent => Some(sizes().min),
        StyleSize::MaxContent => Some(sizes().max),
        StyleSize::FitContent(limit) => {
            let s = sizes();
            let limit = limit.as_ref().map_or(available, |lp| {
                resolve_size(
                    &StyleSize::LengthPercentage(lp.clone()),
                    Some(cb_width),
                    style.box_sizing,
                    edge_sum,
                )
                .unwrap_or(available)
            });
            Some(s.max.min(limit).max(s.min))
        }
        other => resolve_size(other, Some(cb_width), style.box_sizing, edge_sum),
    };
    // Tables size as fit-content (see `layout_absolute`).
    let table = matches!(ib.contents, IndependentContents::Table(_));
    // `auto` fills the inset-modified containing block only with
    // `justify-self: normal` or `stretch`.
    let stretch = axis.start.is_some() && axis.end.is_some() && !table && stretches(axis.align);
    let tentative = specified.unwrap_or_else(|| {
        if stretch {
            available
        } else {
            let s = sizes();
            s.max.min(available).max(s.min)
        }
    });
    let clamped = clamp_width(style, tentative, cb_width, edge_sum);
    let size_auto = specified.is_none() && stretch && clamped == tentative;
    axis.solve(clamped, size_auto)
}

/// Positions a laid-out fragment (at the origin) in the containing block
/// `cb`.
fn place(mut fragment: BoxFragment, cb: Rect, x: Solved, y: Solved) -> BoxFragment {
    fragment.border_rect.x = clamp_length(cb.x + x.offset + x.margin_start);
    fragment.border_rect.y = clamp_length(cb.y + y.offset + y.margin_start);
    fragment
}

// ----- Sticky positioning and paint ancestry -----
//
// Parts of the sticky offset code below (`StickyConstraints`, its offsets
// and offset ranges, and `sticky_offset`) are derived from Chromium's
// `third_party/blink/renderer/core/page/scrolling/
// sticky_position_scrolling_constraints.cc` (Copyright The Chromium
// Authors, BSD-3-Clause); see `THIRD_PARTY_NOTICES.md`.

/// What a sticky box needs to find its offset for a scroll position (CSS
/// Positioned Layout 3 §3.4,
/// <https://www.w3.org/TR/css-position-3/#stickypos-insets>). The
/// algorithm follows Chromium's `StickyPositionScrollingConstraints`.
/// Rectangles are in document coordinates with the scroll offsets of
/// scroll containers applied, at viewport scroll offset 0.
#[derive(Clone, Debug, PartialEq)]
pub struct StickyConstraints {
    /// The border box at its normal position.
    pub rect: Rect,
    /// The area that the border box stays in: the content box of the
    /// containing block, shrunk by the box's margins.
    pub limit: Rect,
    /// `top`, `right`, `bottom` and `left` in px; `None` for `auto`.
    pub insets: [Option<f32>; 4],
    /// The scrollport: `None` for the viewport, which moves with the
    /// scroll offset; else a fixed rectangle (the content box of the
    /// scroll container, whose scroll offset the other rectangles already
    /// have, or the viewport for a box inside a fixed box).
    pub scrollport: Option<Rect>,
    /// The size of the viewport.
    pub viewport: Size,
    /// The nearest sticky ancestor between the box and its containing
    /// block: its accumulated offset moves [`StickyConstraints::rect`]
    /// before this box's own offset is computed (Chromium's
    /// `nearest_sticky_layer_shifting_sticky_box`).
    pub shift_box: Option<Arc<StickyConstraints>>,
    /// The nearest sticky ancestor that is the containing block or
    /// contains it, in the same scroll container: its accumulated offset
    /// moves the rect and [`StickyConstraints::limit`] (Chromium's
    /// `nearest_sticky_layer_shifting_containing_block`).
    pub shift_containing_block: Option<Arc<StickyConstraints>>,
}

/// The offsets of a sticky box: its own, and the sums that its sticky
/// descendants are moved by (Chromium's `total_sticky_box_sticky_offset`
/// and `total_containing_block_sticky_offset`).
#[derive(Clone, Copy, Debug, Default)]
struct StickyOffsets {
    own: Point,
    total_box: Point,
    total_containing_block: Point,
}

/// The offsets and offset ranges of sticky boxes, shared by the groups of
/// one frame, hit test or walk, so that each sticky box of a chain is
/// computed once (keyed by the address of its constraints, which stay
/// alive while the cache is used).
#[derive(Debug, Default)]
pub struct StickyCache {
    /// The viewport scroll offset of `offsets`.
    scroll: Option<Point>,
    offsets: HashMap<usize, StickyOffsets>,
    ranges: HashMap<usize, StickyRanges>,
}

impl StickyConstraints {
    /// [`StickyConstraints::offset_at_with`] without a shared cache.
    #[cfg(test)]
    fn offset_at(&self, scroll: Point) -> Point {
        self.offset_at_with(scroll, &mut StickyCache::default())
    }

    /// The offset of the box from its normal position (after its sticky
    /// ancestors moved it) at the viewport scroll offset `scroll`. `cache`
    /// holds the offsets of the sticky boxes of a frame.
    pub fn offset_at_with(&self, scroll: Point, cache: &mut StickyCache) -> Point {
        if cache.scroll != Some(scroll) {
            cache.scroll = Some(scroll);
            cache.offsets.clear();
        }
        self.offsets(scroll, &mut cache.offsets).own
    }

    /// The offsets at `scroll`. `memo` holds those of the ancestors already
    /// computed (by address), so that a chain of sticky boxes costs linear
    /// time.
    fn offsets(&self, scroll: Point, memo: &mut HashMap<usize, StickyOffsets>) -> StickyOffsets {
        let key = std::ptr::from_ref(self) as usize;
        if let Some(offsets) = memo.get(&key) {
            return *offsets;
        }
        let shift_box = self
            .shift_box
            .as_ref()
            .map_or(Point::default(), |s| s.offsets(scroll, memo).total_box);
        let shift_cb = self
            .shift_containing_block
            .as_ref()
            .map_or(Point::default(), |s| {
                s.offsets(scroll, memo).total_containing_block
            });
        let port = self.scrollport.unwrap_or(Rect::new(
            scroll.x,
            scroll.y,
            self.viewport.width,
            self.viewport.height,
        ));
        let rect = self.rect.translate(shift_box + shift_cb);
        let limit = self.limit.translate(shift_cb);
        let own = sticky_offset(rect, limit, self.insets, port);
        let offsets = StickyOffsets {
            own,
            total_box: shift_box + own,
            total_containing_block: shift_box + shift_cb + own,
        };
        memo.insert(key, offsets);
        offsets
    }

    /// [`StickyConstraints::offset_range_with`] without a shared cache.
    #[cfg(test)]
    fn offset_range(&self) -> (Point, Point) {
        self.offset_range_with(&mut StickyCache::default())
    }

    /// The smallest and the largest offset of the box from its normal
    /// position (see [`StickyConstraints::offset_at_with`]) at any scroll
    /// offset, for the bounds of what the box paints. `cache` holds the
    /// ranges of the sticky boxes of a frame.
    pub fn offset_range_with(&self, cache: &mut StickyCache) -> (Point, Point) {
        self.ranges(&mut cache.ranges).own
    }

    /// The ranges of the offsets of [`StickyConstraints::offsets`] at any
    /// scroll offset. The box moves only until it reaches the edge of its
    /// limit: the offset is between that edge and 0 (`sticky_offset`).
    /// The shift of the containing block moves the box and its limit
    /// together, so only the shift of the box changes the range.
    fn ranges(&self, memo: &mut HashMap<usize, StickyRanges>) -> StickyRanges {
        let key = std::ptr::from_ref(self) as usize;
        if let Some(ranges) = memo.get(&key) {
            return *ranges;
        }
        let zero = (Point::default(), Point::default());
        let shift_box = self
            .shift_box
            .as_ref()
            .map_or(zero, |s| s.ranges(memo).total_box);
        let shift_cb = self
            .shift_containing_block
            .as_ref()
            .map_or(zero, |s| s.ranges(memo).total_containing_block);
        let (rect, limit) = (self.rect, self.limit);
        let [top, right, bottom, left] = self.insets;
        let low = |inset: Option<f32>, room: f32| inset.map_or(0.0, |_| room.min(0.0));
        let high = |inset: Option<f32>, room: f32| inset.map_or(0.0, |_| room.max(0.0));
        let own = (
            Point::new(
                low(right, limit.x - rect.x - shift_box.1.x),
                low(bottom, limit.y - rect.y - shift_box.1.y),
            ),
            Point::new(
                high(left, limit.right() - rect.right() - shift_box.0.x),
                high(top, limit.bottom() - rect.bottom() - shift_box.0.y),
            ),
        );
        let sum = |a: (Point, Point), b: (Point, Point)| {
            let clamp = |p: Point| Point::new(clamp_length(p.x), clamp_length(p.y));
            (clamp(a.0 + b.0), clamp(a.1 + b.1))
        };
        let ranges = StickyRanges {
            own: sum(own, zero),
            total_box: sum(shift_box, own),
            total_containing_block: sum(sum(shift_box, shift_cb), own),
        };
        memo.insert(key, ranges);
        ranges
    }
}

/// The ranges (smallest, largest) of the [`StickyOffsets`] of a sticky box
/// at any scroll offset.
#[derive(Clone, Copy, Debug)]
struct StickyRanges {
    own: (Point, Point),
    total_box: (Point, Point),
    total_containing_block: (Point, Point),
}

/// The offset of a sticky box whose border box is `r`, which stays in
/// `limit`, when the visible area of its scroll container is `port`. `top`
/// wins over `bottom` and `left` over `right`.
fn sticky_offset(r: Rect, limit: Rect, insets: [Option<f32>; 4], port: Rect) -> Point {
    let [top, right, bottom, left] = insets;
    let mut offset = Point::default();
    if let Some(right) = right {
        let delta = (port.right() - right - r.right()).min(0.0);
        let room = (limit.x - r.x).min(0.0);
        offset.x += delta.max(room);
    }
    if let Some(left) = left {
        let delta = (port.x + left - r.x).max(0.0);
        let room = (limit.right() - r.right()).max(0.0);
        offset.x += delta.min(room);
    }
    if let Some(bottom) = bottom {
        let delta = (port.bottom() - bottom - r.bottom()).min(0.0);
        let room = (limit.y - r.y).min(0.0);
        offset.y += delta.max(room);
    }
    if let Some(top) = top {
        let delta = (port.y + top - r.y).max(0.0);
        let room = (limit.bottom() - r.bottom()).max(0.0);
        offset.y += delta.min(room);
    }
    Point::new(clamp_length(offset.x), clamp_length(offset.y))
}

/// The ancestors of a box that decide where it is painted: walkers of the
/// fragment tree (paint, the painted element boxes) keep one per level.
/// Rectangles are absolute layout coordinates with the scroll offsets of
/// scroll containers applied ([`crate::ScrollState`]), at viewport scroll
/// offset 0 and without transforms.
#[derive(Clone, Debug)]
pub struct Ancestry {
    /// The content box of the nearest ancestor that is the containing
    /// block of a sticky box: not an inline box, an anonymous box, a table
    /// row or a row group (cells stick in their table, as in Chromium).
    block_content: Rect,
    /// The rectangle that sticky boxes stick in: the scrollport of the
    /// nearest scroll container in the chain of containing blocks, without
    /// its padding (as in Chromium), or the viewport inside a fixed box;
    /// `None` for the viewport.
    scrollport: Option<Rect>,
    /// The `scrollport` at the containing block of absolutely positioned
    /// descendants (for its children): such a box scrolls with it, not
    /// with the scroll containers between them.
    absolute_scrollport: Option<Rect>,
    /// The `scrollport` at the containing block of fixed descendants (the
    /// nearest transformed ancestor; unused without one).
    fixed_scrollport: Option<Rect>,
    /// True if an ancestor has a transform: fixed boxes are positioned
    /// relative to it, not to the viewport.
    transformed: bool,
    viewport: Size,
    /// The nearest sticky ancestor below the containing block of sticky
    /// descendants (see [`StickyConstraints::shift_box`]).
    sticky_box: Option<Arc<StickyConstraints>>,
    /// The nearest sticky ancestor that is that containing block or
    /// contains it, in the same scroll container and not outside a fixed
    /// box (see [`StickyConstraints::shift_containing_block`]).
    sticky_containing_block: Option<Arc<StickyConstraints>>,
}

impl Ancestry {
    /// The ancestry of the root box in a viewport of size `viewport`.
    pub fn root(viewport: Size) -> Self {
        Ancestry {
            block_content: Rect::new(0.0, 0.0, viewport.width, viewport.height),
            scrollport: None,
            absolute_scrollport: None,
            fixed_scrollport: None,
            transformed: false,
            viewport,
            sticky_box: None,
            sticky_containing_block: None,
        }
    }

    /// True if `b` is fixed to the viewport: `position: fixed` without a
    /// transformed ancestor. Its layout position is relative to the
    /// viewport; paint adds the scroll offset.
    pub fn is_viewport_fixed(&self, b: &BoxFragment) -> bool {
        b.style.position == Position::Fixed && !self.transformed
    }

    /// The sticky constraints of the sticky box `b` whose border box is
    /// at `rect`.
    fn sticky(&self, b: &BoxFragment, rect: Rect) -> StickyConstraints {
        let cb = self.block_content;
        let style = &b.style;
        let margin = |m: &LengthPercentageOrAuto| clamp_length(margin_or_zero(m, cb.width));
        let limit = cb.inset(&Edges::new(
            margin(&style.margin_top),
            margin(&style.margin_right),
            margin(&style.margin_bottom),
            margin(&style.margin_left),
        ));
        let port = self
            .scrollport
            .map_or(Size::new(self.viewport.width, self.viewport.height), |r| {
                Size::new(r.width, r.height)
            });
        let inset = |v: &LengthPercentageOrAuto, basis: f32| v.resolve(basis).map(clamp_length);
        StickyConstraints {
            rect,
            limit,
            insets: [
                inset(&style.top, port.height),
                inset(&style.right, port.width),
                inset(&style.bottom, port.height),
                inset(&style.left, port.width),
            ],
            scrollport: self.scrollport,
            viewport: self.viewport,
            shift_box: self.sticky_box.clone(),
            shift_containing_block: self.sticky_containing_block.clone(),
        }
    }

    /// The transform groups that paint applies to the box `b` whose
    /// border box is at `rect`, outermost first: the scroll offset if it is
    /// fixed to the viewport, its sticky offset, and its transform.
    pub fn group_transforms(&self, b: &BoxFragment, rect: Rect) -> Vec<GroupTransform> {
        let style = &b.style;
        if !matches!(style.position, Position::Fixed | Position::Sticky) && !style.has_transform() {
            return Vec::new();
        }
        let mut groups = Vec::new();
        if self.is_viewport_fixed(b) {
            groups.push(GroupTransform::Fixed);
        }
        if b.style.position == Position::Sticky {
            groups.push(GroupTransform::Sticky(Arc::new(self.sticky(b, rect))));
        }
        if let Some(m) = transform_matrix(b, rect) {
            groups.push(GroupTransform::Matrix(m));
        }
        groups
    }

    /// The ancestry of the children of `b`, whose border box is at `rect`.
    /// `groups` are the box's transform groups
    /// ([`Ancestry::group_transforms`]); `shift` moves the children (minus
    /// the scroll offset of a scroll container, see
    /// [`crate::ScrollState::enter`]). Borrowed if it does not change
    /// (most inline boxes, and boxes without box children).
    #[must_use]
    pub fn enter(
        &self,
        b: &BoxFragment,
        rect: Rect,
        groups: &[GroupTransform],
        shift: Point,
    ) -> Cow<'_, Ancestry> {
        let scroll_container = crate::scroll::is_scroll_container(&b.style);
        // A positioned inline box changes only the scrollport of absolutely
        // positioned descendants.
        let unchanged = !b.children.iter().any(|c| matches!(c, Fragment::Box(_)))
            || (b.is_inline
                && groups.is_empty()
                && !scroll_container
                && (b.style.position == Position::Static
                    || self.absolute_scrollport == self.scrollport));
        if unchanged {
            return Cow::Borrowed(self);
        }
        let mut inner = self.clone();
        let own = |r: Rect| {
            r.translate(Point::new(
                rect.x - b.border_rect.x,
                rect.y - b.border_rect.y,
            ))
        };
        let sticky = groups.iter().find_map(|g| match g {
            GroupTransform::Sticky(s) => Some(Arc::clone(s)),
            _ => None,
        });
        let containing_block =
            !b.is_inline && b.node.is_some() && !matches!(b.content, BoxContent::TablePart);
        if containing_block {
            inner.block_content = match b.scrollable_overflow {
                // The content of a scroll container moves with its scroll
                // offset; sticky boxes stay in its scrollable overflow
                // without the padding (measured in Chromium 148).
                Some(overflow) if scroll_container => overflow
                    .inset(&b.padding)
                    .translate(rect.origin())
                    .translate(shift),
                _ => own(b.content_rect()),
            };
            // The new containing block moves with the box itself, or with
            // the sticky ancestors that contain it.
            inner.sticky_containing_block = sticky
                .or_else(|| self.sticky_box.clone())
                .or_else(|| self.sticky_containing_block.clone());
            inner.sticky_box = None;
        } else if sticky.is_some() {
            inner.sticky_box = sticky;
        }
        let viewport_fixed = self.is_viewport_fixed(b);
        if scroll_container || viewport_fixed {
            // Sticky ancestors move the scrollport (or the fixed box does
            // not move with them): only those inside shift a sticky box.
            inner.sticky_box = None;
            inner.sticky_containing_block = None;
        }
        // The scrollport that `b` scrolls with, and the one for its children.
        let own_port = match b.style.position {
            _ if viewport_fixed => Some(Rect::new(
                0.0,
                0.0,
                self.viewport.width,
                self.viewport.height,
            )),
            Position::Absolute => self.absolute_scrollport,
            Position::Fixed => self.fixed_scrollport,
            _ => self.scrollport,
        };
        inner.scrollport = if scroll_container {
            Some(own(b.content_rect()))
        } else {
            own_port
        };
        if is_absolute_containing_block(b) {
            inner.absolute_scrollport = inner.scrollport;
        } else if b.in_positioned_inline {
            // The positioned inline box around `b` is the containing block.
            inner.absolute_scrollport = own_port;
        }
        if is_fixed_containing_block(b) {
            inner.fixed_scrollport = inner.scrollport;
        }
        inner.transformed |= has_transform(b);
        Cow::Owned(inner)
    }
}

/// A transform that paint applies to a box and its descendants (a
/// transform group of the display list).
#[derive(Clone, Debug, PartialEq)]
pub enum GroupTransform {
    /// A 2D affine transform (CSS `transform`).
    Matrix(Matrix),
    /// A box fixed to the viewport (`position: fixed`): it moves with the
    /// viewport scroll offset. This replaces the translations of the
    /// enclosing groups, which can only be fixed and sticky groups (with a
    /// transformed ancestor, the box is not fixed to the viewport).
    Fixed,
    /// A sticky box (`position: sticky`): it moves by its sticky offset
    /// for the scroll offset.
    Sticky(Arc<StickyConstraints>),
}

impl GroupTransform {
    /// The transform from the group's coordinates to the coordinates in
    /// which `outer` is the transform of the enclosing group, at the
    /// viewport scroll offset `scroll`. `cache` holds the offsets of sticky
    /// boxes, shared by the other groups of a frame.
    pub fn resolve_with(&self, outer: &Matrix, scroll: Point, cache: &mut StickyCache) -> Matrix {
        match self {
            GroupTransform::Matrix(m) => outer.multiply(m),
            GroupTransform::Fixed => Matrix::translate(scroll.x, scroll.y),
            GroupTransform::Sticky(sticky) => {
                let offset = sticky.offset_at_with(scroll, cache);
                outer.multiply(&Matrix::translate(offset.x, offset.y))
            }
        }
    }
}

// ----- Transforms -----

/// The transform of a box whose border box is `border_rect` (in any
/// coordinates), as a matrix in those coordinates: translate to the
/// `transform-origin`, apply the transform functions from left to right,
/// translate back (CSS Transforms 1 §6,
/// <https://www.w3.org/TR/css-transforms-1/#transform-rendering>).
/// Percentages refer to the border box. `None` if the box has no
/// transform. A transform with infinite or NaN entries maps everything to
/// one point, so that the box is not visible.
pub fn transform_matrix(fragment: &BoxFragment, border_rect: Rect) -> Option<Matrix> {
    if !has_transform(fragment) {
        return None;
    }
    Some(style_transform(&fragment.style, border_rect))
}

/// The matrix of the `transform` and `transform-origin` of `style` for the
/// reference box `reference` (in any coordinates; percentages refer to it):
/// translate to the origin, apply the transform functions from left to
/// right, translate back. A transform with infinite or NaN entries maps
/// everything to the origin.
pub(crate) fn style_transform(style: &ComputedStyle, reference: Rect) -> Matrix {
    let origin = Point::new(
        reference.x + style.transform_origin.x.resolve(reference.width),
        reference.y + style.transform_origin.y.resolve(reference.height),
    );
    let mut m = Matrix::translate(origin.x, origin.y);
    for function in style.transform.iter() {
        let f = match function {
            TransformFunction::Matrix(m) => Matrix::new(m[0], m[1], m[2], m[3], m[4], m[5]),
            TransformFunction::Translate(x, y) => {
                Matrix::translate(x.resolve(reference.width), y.resolve(reference.height))
            }
            TransformFunction::Scale(x, y) => Matrix::new(*x, 0.0, 0.0, *y, 0.0, 0.0),
            TransformFunction::Rotate(degrees) => {
                let (sin, cos) = degrees.to_radians().sin_cos();
                Matrix::new(cos, sin, -sin, cos, 0.0, 0.0)
            }
            TransformFunction::Skew(x, y) => Matrix::new(
                1.0,
                y.to_radians().tan(),
                x.to_radians().tan(),
                1.0,
                0.0,
                0.0,
            ),
        };
        m = m.multiply(&f);
    }
    m = m.multiply(&Matrix::translate(-origin.x, -origin.y));
    if m.is_finite() {
        m
    } else {
        Matrix::new(0.0, 0.0, 0.0, 0.0, origin.x, origin.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FragmentRef;
    use crate::test_support::{body, layout_html};

    #[test]
    fn many_absolute_boxes_are_laid_out_once_each() {
        const COUNT: usize = 20_000;
        let boxes = "<span style='position:absolute; left:1px'>x</span>".repeat(COUNT);
        let l = layout_html(&body(&format!(
            "<div style='position:relative'>{boxes}</div>"
        )));
        let mut found = 0;
        l.tree.walk(|f, _| {
            if let FragmentRef::Box(b) = f {
                assert!(!matches!(b.content, BoxContent::Placeholder(_)));
                found += usize::from(b.style.is_absolutely_positioned() && b.node.is_some());
            }
        });
        assert_eq!(found, COUNT);
    }

    #[test]
    fn absolute_boxes_in_cached_flex_items_are_laid_out_once() {
        // Flex layout measures each item several times; only the final
        // layout's placeholder becomes a box.
        const DEPTH: usize = 20;
        let open = "<div style='display:flex'><div>".repeat(DEPTH);
        let l = layout_html(&body(&format!(
            "{open}<span id=a style='position:absolute; top:3px'>a</span>x"
        )));
        let a = l.node("a");
        assert_eq!(l.tree.border_boxes(a).len(), 1);
        assert_eq!(l.rect("a").y, 3.0);
    }

    #[test]
    fn hostile_values_give_finite_boxes() {
        let l = layout_html(&body(
            "<div id=a style='position:absolute; left:1e39px; right:-1e39px; top:1e30%; \
                             margin:auto; width:1e39px; height:-5px'></div>\
             <div id=b style='transform:scale(1e30) rotate(45deg) translate(1e39px, 50%)'>b</div>\
             <div id=c style='transform:matrix(1e38, 1e38, -1e38, 1e38, 1e38, 0) scale(0)'>c</div>\
             <div id=d style='position:sticky; top:1e39px; bottom:-1e39px; height:10px'></div>",
        ));
        for id in ["a", "b", "c", "d"] {
            let r = l.rect(id);
            assert!(
                [r.x, r.y, r.width, r.height].iter().all(|v| v.is_finite()),
                "#{id}: {r:?}"
            );
        }
        assert!(l.tree.scroll_size.width.is_finite() && l.tree.scroll_size.height.is_finite());
    }

    #[test]
    fn transforms_compose_around_the_origin() {
        let l = layout_html(&body(
            "<div id=r style='margin:100px; width:20px; height:10px; transform:rotate(90deg)'></div>\
             <div id=o style='width:20px; height:10px; transform:scale(2); transform-origin:0 0'></div>",
        ));
        let r = l.rect("r");
        // Rotated around its center (110, 105): 10 wide, 20 high.
        assert!(
            (r.x - 105.0).abs() < 0.01 && (r.y - 95.0).abs() < 0.01,
            "{r:?}"
        );
        assert!((r.width - 10.0).abs() < 0.01 && (r.height - 20.0).abs() < 0.01);
        let o = l.rect("o");
        assert_eq!((o.x, o.y, o.width, o.height), (0.0, 210.0, 40.0, 20.0));
    }

    fn axis(start: Option<f32>, end: Option<f32>, margins: (Option<f32>, Option<f32>)) -> Axis {
        Axis {
            cb: 100.0,
            start,
            end,
            margin_start: margins.0,
            margin_end: margins.1,
            edges: 10.0,
            static_position: 20.0,
            static_edge: Edge::Start,
            horizontal: true,
            align: Alignment::Normal,
        }
    }

    #[test]
    fn constraint_rules() {
        // All auto: the static position.
        let a = axis(None, None, (Some(5.0), None));
        assert_eq!(a.available(), 65.0);
        assert_eq!(a.solve(30.0, true).offset, 20.0);
        // End set: the box ends at the end inset.
        let a = axis(None, Some(10.0), (Some(5.0), Some(5.0)));
        let s = a.solve(30.0, true);
        assert_eq!(s.offset + s.margin_start, 45.0);
        // Both insets and the size: auto margins center the box, but not
        // with negative margins horizontally.
        let a = axis(Some(10.0), Some(10.0), (None, None));
        assert_eq!(a.solve(40.0, false).margin_start, 15.0);
        assert_eq!(a.solve(90.0, false).margin_start, 0.0);
        let v = Axis {
            horizontal: false,
            ..a
        };
        assert_eq!(v.solve(90.0, false).margin_start, -10.0);
        // Over-constrained: the end inset is ignored.
        let a = axis(Some(10.0), Some(10.0), (Some(0.0), Some(0.0)));
        assert_eq!(a.solve(10.0, false).offset, 10.0);
        // Both insets, auto size: auto margins are 0.
        let a = axis(Some(10.0), Some(20.0), (None, Some(4.0)));
        assert_eq!(a.available(), 56.0);
        assert_eq!(a.solve(56.0, true).margin_start, 0.0);
    }

    #[test]
    fn static_position_edges() {
        let mut a = axis(None, None, (Some(0.0), Some(0.0)));
        a.static_position = 50.0;
        a.static_edge = Edge::Center;
        assert_eq!(a.available(), 90.0);
        assert_eq!(a.solve(20.0, true).offset, 35.0);
        a.static_edge = Edge::End;
        assert_eq!(a.available(), 40.0);
        assert_eq!(a.solve(20.0, true).offset, 20.0);
    }

    #[test]
    fn sticky_offsets() {
        let c = StickyConstraints {
            rect: Rect::new(0.0, 100.0, 50.0, 20.0),
            limit: Rect::new(0.0, 50.0, 100.0, 150.0),
            insets: [Some(10.0), None, None, None],
            scrollport: None,
            viewport: Size::new(800.0, 600.0),
            shift_box: None,
            shift_containing_block: None,
        };
        let scroll = |y: f32| Point::new(0.0, y);
        assert_eq!(c.offset_at(scroll(0.0)), Point::default());
        assert_eq!(c.offset_at(scroll(100.0)), Point::new(0.0, 10.0));
        // It stops at the bottom of its containing block.
        assert_eq!(c.offset_at(scroll(1000.0)), Point::new(0.0, 80.0));
        let bottom = StickyConstraints {
            insets: [None, None, Some(0.0), None],
            scrollport: Some(Rect::new(0.0, 0.0, 800.0, 110.0)),
            ..c.clone()
        };
        // Below the scrollport: moved up, but not above the limit.
        assert_eq!(bottom.offset_at(scroll(0.0)), Point::new(0.0, -10.0));
        let short = StickyConstraints {
            scrollport: Some(Rect::new(0.0, 0.0, 800.0, 10.0)),
            ..bottom
        };
        assert_eq!(short.offset_at(scroll(0.0)), Point::new(0.0, -50.0));
        // A sticky box inside a 300 px high sticky box that sticks at the
        // top: the inner box is 50 px below the outer box's top, so it does
        // not stick (it moves with the outer box only).
        let outer = StickyConstraints {
            rect: Rect::new(0.0, 100.0, 50.0, 300.0),
            limit: Rect::new(0.0, 0.0, 100.0, 1000.0),
            insets: [Some(0.0), None, None, None],
            ..c.clone()
        };
        let inner = StickyConstraints {
            rect: Rect::new(0.0, 150.0, 50.0, 20.0),
            limit: Rect::new(0.0, 100.0, 50.0, 300.0),
            shift_containing_block: Some(Arc::new(outer.clone())),
            ..outer.clone()
        };
        assert_eq!(outer.offset_at(scroll(560.0)), Point::new(0.0, 460.0));
        assert_eq!(inner.offset_at(scroll(560.0)), Point::default());
    }

    #[test]
    fn sticky_offset_ranges() {
        let c = StickyConstraints {
            rect: Rect::new(0.0, 100.0, 50.0, 20.0),
            limit: Rect::new(0.0, 50.0, 100.0, 150.0),
            insets: [Some(10.0), None, Some(0.0), None],
            scrollport: None,
            viewport: Size::new(800.0, 600.0),
            shift_box: None,
            shift_containing_block: None,
        };
        // Up to the top and the bottom of the limit.
        let range = (Point::new(0.0, -50.0), Point::new(0.0, 80.0));
        assert_eq!(c.offset_range(), range);
        for y in [-1000.0, 0.0, 100.0, 1000.0] {
            let offset = c.offset_at(Point::new(0.0, y));
            assert!(offset.y >= range.0.y && offset.y <= range.1.y, "{y}");
        }
        // A sticky ancestor between the box and its containing block that
        // can move 30 px down leaves less room below.
        let outer = StickyConstraints {
            rect: Rect::new(0.0, 90.0, 60.0, 40.0),
            limit: Rect::new(0.0, 50.0, 100.0, 110.0),
            insets: [Some(0.0), None, None, None],
            ..c.clone()
        };
        let inner = StickyConstraints {
            shift_box: Some(Arc::new(outer.clone())),
            ..c.clone()
        };
        assert_eq!(
            outer.offset_range(),
            (Point::default(), Point::new(0.0, 30.0))
        );
        assert_eq!(
            inner.offset_range(),
            (Point::new(0.0, -80.0), Point::new(0.0, 80.0))
        );
    }
}
