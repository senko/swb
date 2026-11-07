//! The items of the display list ([`DisplayItem`]), the list ([`DisplayList`])
//! with its hit test, and the inputs of the builder: the selection
//! ([`Highlights`]), the natural image sizes ([`ImageSizes`]) and the scroll
//! offsets ([`Scrolling`]). The builder is in `builder.rs`.

use std::sync::Arc;

use crate::hit_path::HitWork;
use swb_dom::NodeId;
use swb_layout::svg::{ClipPath, StrokeStyle, SvgPath};
use swb_layout::{
    GroupTransform, Matrix, NaturalSize, NoScroll, Point, PositionedGlyph, Rect, ScrollOffsets,
    StickyCache,
};
use swb_style::{BorderStyle, FillRule, Rgba, StrokeLinejoin};
use swb_text::FontId;

use crate::group_bounds::transform_ends;
use crate::mask::MaskLayer;
use crate::path_cost::stroke_reach;

/// Corner radii (horizontal, vertical) in px: top-left, top-right,
/// bottom-right, bottom-left.
pub type Radii = [(f32, f32); 4];

/// One drawing command.
#[derive(Clone, Debug)]
pub enum DisplayItem {
    /// Fill a rectangle (optionally rounded).
    Rect {
        /// The rectangle.
        rect: Rect,
        /// Corner radii.
        radii: Radii,
        /// The color.
        color: Rgba,
    },
    /// Draw a box border.
    Border {
        /// The border box.
        rect: Rect,
        /// Widths: top, right, bottom, left.
        widths: [f32; 4],
        /// Colors: top, right, bottom, left.
        colors: [Rgba; 4],
        /// Styles: top, right, bottom, left.
        styles: [BorderStyle; 4],
        /// Outer corner radii.
        radii: Radii,
    },
    /// Draw glyphs.
    Text {
        /// Pen position of the first glyph on the baseline.
        origin: Point,
        /// The font.
        font: FontId,
        /// Font size in px.
        size: f32,
        /// Glyphs relative to `origin`.
        glyphs: Arc<[PositionedGlyph]>,
        /// The color.
        color: Rgba,
    },
    /// Draw an image scaled into `rect`, tiled as `tile` describes.
    Image {
        /// What to draw.
        image: ImageRef,
        /// The area to fill.
        rect: Rect,
        /// The size and origin of one tile; equal to `rect` when not
        /// tiled.
        tile: Rect,
        /// Clip.
        clip: Rect,
    },
    /// Fill the part of `rect` inside `clip` with a linear gradient.
    LinearGradient {
        /// The area of the gradient.
        rect: Rect,
        /// The area to paint.
        clip: Rect,
        /// The gradient.
        gradient: Arc<swb_style::LinearGradient>,
        /// The current color for `currentColor` stops.
        current_color: Rgba,
    },
    /// Start clipping to a rectangle.
    PushClip(Rect),
    /// Start a clip that covers the whole target and replaces the
    /// enclosing clips: content fixed to the viewport is not clipped by the
    /// overflow clips of its ancestors. Ends at the next `PopClip`.
    PushViewportClip,
    /// End the most recent clip.
    PopClip,
    /// Start a group that is composited with `opacity`.
    PushOpacity {
        /// The group opacity, 0 to 1. A group with opacity 0 is not drawn,
        /// but its hit regions count.
        opacity: f32,
        /// An area that contains everything the group draws, except
        /// content fixed to the viewport.
        bounds: Rect,
        /// An area that contains what content fixed to the viewport in the
        /// group draws, in viewport coordinates (it moves with the scroll
        /// offset); `None` without such content.
        fixed_bounds: Option<Rect>,
        /// True if the group contains content fixed to the viewport, which
        /// the enclosing clips do not clip.
        escapes_clips: bool,
    },
    /// End the most recent opacity group.
    PopOpacity,
    /// Start a group that the CSS `filter` functions change when it ends
    /// (Filter Effects 1 §5). The group is the element and its
    /// descendants; the functions apply in order, then the enclosing
    /// clip, mask and opacity groups.
    PushFilter {
        /// The computed functions (`url()` is not painted).
        filter: Arc<[swb_style::FilterFunction]>,
        /// The `color` of the element, for `drop-shadow()` without a
        /// color.
        current_color: Rgba,
        /// An area that contains everything the content of the group
        /// draws, except content fixed to the viewport (not the blur or
        /// the shadow that the filter adds).
        bounds: Rect,
        /// An area that contains what content fixed to the viewport in the
        /// group draws, in viewport coordinates; `None` without such
        /// content.
        fixed_bounds: Option<Rect>,
        /// True if the group contains content fixed to the viewport, which
        /// the enclosing clips do not clip.
        escapes_clips: bool,
    },
    /// End the most recent filter group.
    PopFilter,
    /// Draw the backdrop of a box through the `backdrop-filter` functions
    /// (Filter Effects 2 §2,
    /// <https://drafts.fxtf.org/filter-effects-2/#BackdropFilterProperty>):
    /// what is painted before this item in the backdrop root, inside
    /// `rect` (and its rounded corners), goes through the functions and
    /// is drawn over the same area. The box paints after it.
    BackdropFilter {
        /// The computed functions (`url()` is not painted).
        filter: Arc<[swb_style::FilterFunction]>,
        /// The `color` of the element, for `drop-shadow()` without a
        /// color.
        current_color: Rgba,
        /// The border box.
        rect: Rect,
        /// The corner radii of the border box, if it is rounded.
        radii: Option<Radii>,
        /// The opacity that the filtered backdrop is drawn with (the
        /// `opacity` of the box, which applies to the backdrop and to the
        /// box separately).
        alpha: f32,
        /// The number of groups (opacity, mask, clip path and filter) that
        /// the box started before this item: the backdrop is what lies
        /// below them, because they apply to the box and its filtered
        /// backdrop together.
        layers: u8,
    },
    /// Start a group that is masked by `layers` (CSS Masking 1 §7): its
    /// pixels are multiplied by the mask when it ends.
    PushMask {
        /// An area that contains everything the group shows: its content
        /// inside the area of the mask layers.
        bounds: Rect,
        /// The mask layers, top first. The enclosing clips clip the mask,
        /// so they also clip content fixed to the viewport in the group (as
        /// in Chromium).
        layers: Arc<[MaskLayer]>,
    },
    /// End the most recent mask group.
    PopMask,
    /// Start a group that is clipped to the SVG clip path `clip` (ADR
    /// 0023): its pixels are multiplied by the coverage of the clip path
    /// when it ends. A clip that is one axis-aligned rectangle is a plain
    /// [`DisplayItem::PushClip`] instead.
    PushSvgClip {
        /// The clip path, in the coordinates of the SVG content box.
        clip: Arc<ClipPath>,
        /// From the clip path's coordinates to the list's coordinates (the
        /// translation to the content box).
        transform: Matrix,
        /// An area that contains everything the group shows: its content
        /// inside the extent of the clip path.
        bounds: Rect,
        /// True for the rounded overflow clip of a box: if the layer or
        /// the coverage does not fit into the budgets, the group is
        /// clipped to `bounds` (a rectangle) instead of being hidden.
        rect_fallback: bool,
        /// True if the group is a backdrop root (Filter Effects 2 §2): the
        /// `backdrop-filter` of a box inside sees only the group's
        /// content. False for the rounded overflow clip of a box, which is
        /// no backdrop root, and for SVG content (no `backdrop-filter`).
        backdrop_root: bool,
    },
    /// End the most recent SVG clip group.
    PopSvgClip,
    /// Start a group whose items are drawn with a transform, up to the
    /// matching [`DisplayItem::PopTransform`].
    PushTransform {
        /// How the group's coordinates map to the coordinates around it.
        transform: GroupTransform,
        /// An area that contains everything the group draws, in the
        /// group's coordinates, except content fixed to the viewport. For
        /// a [`GroupTransform::Fixed`] group, in viewport coordinates and
        /// with that content.
        bounds: Rect,
        /// `bounds` and the hit regions of the group.
        hit_bounds: Rect,
        /// An area that contains what content fixed to the viewport in the
        /// group draws and its hit regions, in viewport coordinates (it
        /// moves with the scroll offset); `None` without such content and
        /// for a [`GroupTransform::Fixed`] group.
        fixed_bounds: Option<Rect>,
    },
    /// End the most recent transform group.
    PopTransform,
    /// Stroke a line through `points` (butt caps, miter joins).
    Polyline {
        /// The points.
        points: Arc<[Point]>,
        /// The line width.
        width: f32,
        /// The color.
        color: Rgba,
    },
    /// Fill the polygon through `points` (non-zero winding rule).
    Polygon {
        /// The corners.
        points: Arc<[Point]>,
        /// The color.
        color: Rgba,
    },
    /// Fill a path of inline SVG content (anti-aliased).
    FillPath {
        /// The path.
        path: Arc<SvgPath>,
        /// From the path's coordinates to the list's coordinates.
        transform: Matrix,
        /// The color.
        color: Rgba,
        /// The fill rule.
        rule: FillRule,
        /// False to draw without anti-aliasing (`shape-rendering`).
        anti_alias: bool,
    },
    /// Stroke a path of inline SVG content (anti-aliased).
    StrokePath {
        /// The path.
        path: Arc<SvgPath>,
        /// From the path's coordinates to the list's coordinates; it also
        /// applies to the stroke.
        transform: Matrix,
        /// The color.
        color: Rgba,
        /// The stroke, in the path's coordinates.
        stroke: Arc<StrokeStyle>,
        /// False to draw without anti-aliasing (`shape-rendering`).
        anti_alias: bool,
    },
    /// The geometry of an SVG shape that hit testing finds. Not drawn.
    HitShape {
        /// The shape element.
        node: NodeId,
        /// The path.
        path: Arc<SvgPath>,
        /// From the path's coordinates to the list's coordinates.
        transform: Matrix,
        /// The fill rule if the fill takes part.
        fill: Option<FillRule>,
        /// The stroke width in the path's coordinates if the stroke takes
        /// part.
        stroke_width: Option<f32>,
    },
    /// An area that hit testing finds. Not drawn.
    HitRegion {
        /// The area.
        rect: Rect,
        /// The node it belongs to (an element or a text node).
        node: NodeId,
    },
    /// Draw the document of a frame (ADR 0031 section 4) at the place of
    /// the content of its `iframe`: the rasterizer gets the frame's list
    /// through [`crate::ImageSource::frame`], clips it to `rect` and moves
    /// it to the origin of `rect`, inside the groups that are open here.
    /// The item names the element, not the frame, so that a frame that
    /// starts or ends does not change the list of its parent.
    Frame {
        /// The `iframe` element.
        node: NodeId,
        /// The content box of the element: the frame's viewport.
        rect: Rect,
        /// False if hit testing does not go into the frame: the element
        /// has `pointer-events: none` (ADR 0031 section 8).
        interactive: bool,
    },
}

impl DisplayItem {
    /// An area that contains everything the item draws, or `None` for items
    /// that do not draw. For text, the area is an estimate with a margin of
    /// one or more font sizes around the pen positions.
    pub(crate) fn bounds(&self) -> Option<Rect> {
        match self {
            DisplayItem::Rect { rect, .. }
            | DisplayItem::Border { rect, .. }
            | DisplayItem::BackdropFilter { rect, .. }
            | DisplayItem::Frame { rect, .. } => Some(*rect),
            DisplayItem::Image { rect, clip, .. }
            | DisplayItem::LinearGradient { rect, clip, .. } => rect.intersection(clip),
            DisplayItem::Text {
                origin,
                size,
                glyphs,
                ..
            } => text_bounds(*origin, *size, glyphs),
            DisplayItem::Polyline { points, width, .. } => polyline_bounds(points, *width),
            // One pixel of margin for the anti-aliased edges.
            DisplayItem::Polygon { points, .. } => polyline_bounds(points, 0.5),
            DisplayItem::FillPath {
                path, transform, ..
            } => Some(path_bounds(path, transform, 0.0)),
            DisplayItem::StrokePath {
                path,
                transform,
                stroke,
                ..
            } => Some(path_bounds(path, transform, stroke_extent(stroke))),
            DisplayItem::PushClip(_)
            | DisplayItem::PushViewportClip
            | DisplayItem::PopClip
            | DisplayItem::PushOpacity { .. }
            | DisplayItem::PopOpacity
            | DisplayItem::PushFilter { .. }
            | DisplayItem::PopFilter
            | DisplayItem::PushMask { .. }
            | DisplayItem::PopMask
            | DisplayItem::PushSvgClip { .. }
            | DisplayItem::PopSvgClip
            | DisplayItem::PushTransform { .. }
            | DisplayItem::PopTransform
            | DisplayItem::HitShape { .. }
            | DisplayItem::HitRegion { .. } => None,
        }
    }

    /// The area that the item draws, for the `no-clip` area of masks: as
    /// [`DisplayItem::bounds`], but for text closer to the ink than the
    /// culling estimate: the glyph origins grown by one font size to the
    /// right and above, and a third of it below.
    pub(crate) fn ink_bounds(&self) -> Option<Rect> {
        match self {
            DisplayItem::Text {
                origin,
                size,
                glyphs,
                ..
            } => {
                let (x0, y0, x1, y1) = extent(glyphs.iter().map(|g| (g.x, g.y)))?;
                Some(Rect::new(
                    origin.x + x0,
                    origin.y + y0 - size,
                    x1 - x0 + size,
                    y1 - y0 + size * 4.0 / 3.0,
                ))
            }
            other => other.bounds(),
        }
    }
}

/// The estimated area of a glyph run: glyphs extend at most one font size
/// to the left of their pen position, four to the right, three above the
/// baseline and two below it.
fn text_bounds(origin: Point, size: f32, glyphs: &[PositionedGlyph]) -> Option<Rect> {
    let (x0, y0, x1, y1) = extent(glyphs.iter().map(|g| (g.x, g.y)))?;
    Some(Rect::new(
        origin.x + x0 - size,
        origin.y + y0 - 3.0 * size,
        x1 - x0 + 5.0 * size,
        y1 - y0 + 5.0 * size,
    ))
}

/// The area of a stroked line: the box of its points, grown by twice the
/// line width (which covers miter joins of moderate angles).
fn polyline_bounds(points: &[Point], width: f32) -> Option<Rect> {
    let (x0, y0, x1, y1) = extent(points.iter().map(|p| (p.x, p.y)))?;
    let grow = width.max(0.0) * 2.0;
    Some(Rect::new(
        x0 - grow,
        y0 - grow,
        x1 - x0 + 2.0 * grow,
        y1 - y0 + 2.0 * grow,
    ))
}

/// The area of a path drawn with `transform`, with `grow` (in the path's
/// coordinates) around its points and one px for anti-aliasing.
pub(crate) fn path_bounds(path: &SvgPath, transform: &Matrix, grow: f32) -> Rect {
    let b = path.bounds();
    let grown = Rect::new(
        b.x - grow,
        b.y - grow,
        b.width + 2.0 * grow,
        b.height + 2.0 * grow,
    );
    let r = transform.map_rect(&grown);
    Rect::new(r.x - 1.0, r.y - 1.0, r.width + 2.0, r.height + 2.0)
}

/// How far a stroke reaches beyond the points of its path: half its width,
/// times the miter limit for miter joins (and at least √2 for square
/// caps).
pub(crate) fn stroke_extent(stroke: &StrokeStyle) -> f32 {
    let miter = (stroke.join == StrokeLinejoin::Miter).then_some(stroke.miter_limit);
    stroke_reach(stroke.width, miter)
}

/// The smallest and largest coordinates of `points`, as
/// `(x0, y0, x1, y1)`, or `None` if there are no points.
fn extent(points: impl Iterator<Item = (f32, f32)>) -> Option<(f32, f32, f32, f32)> {
    points.fold(None, |extent, (x, y)| {
        let (x0, y0, x1, y1) = extent.unwrap_or((x, y, x, y));
        Some((x0.min(x), y0.min(y), x1.max(x), y1.max(y)))
    })
}

/// An area that contains everything: the bounds of a group whose content
/// moves with the scroll offset.
pub(crate) const UNBOUNDED: Rect = Rect::new(
    -swb_style::Length::MAX_PX,
    -swb_style::Length::MAX_PX,
    2.0 * swb_style::Length::MAX_PX,
    2.0 * swb_style::Length::MAX_PX,
);

/// A reference to an image.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ImageRef {
    /// The image of a replaced element.
    Node(NodeId),
    /// An image loaded from a URL (backgrounds, list markers).
    Url(Arc<str>),
}

/// The text selection, for painting the highlight.
pub trait Highlights {
    /// The selected range of node offsets of text node `node` (the end can
    /// be `u32::MAX`), or `None` if no part of it is selected.
    fn selected(&self, node: NodeId) -> Option<(u32, u32)>;
}

/// No selection.
pub struct NoHighlights;

impl Highlights for NoHighlights {
    fn selected(&self, _node: NodeId) -> Option<(u32, u32)> {
        None
    }
}

/// The background of selected text (Chromium's default, measured with
/// Chromium 148 on Linux).
pub const SELECTION_BACKGROUND: Rgba = Rgba::rgb(51, 103, 209);

/// The color of selected text.
pub(super) const SELECTION_TEXT: Rgba = Rgba::WHITE;

/// Natural sizes of images by reference, for background positioning and
/// for `object-fit`. For [`ImageRef::Node`], the size is the natural size
/// of the element's image (for `<img>`, density-corrected).
pub trait ImageSizes {
    /// The natural dimensions of an image, if it is loaded.
    fn size(&self, image: &ImageRef) -> Option<NaturalSize>;
}

/// A list of drawing commands.
#[derive(Clone, Debug, Default)]
pub struct DisplayList {
    /// The commands in paint order.
    pub items: Vec<DisplayItem>,
}

/// What a hit test found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    /// A node of the document.
    Node(NodeId),
    /// The frame of the `iframe` element `node` (ADR 0031 section 8): the
    /// topmost thing at the point is the content of the frame. `point` and
    /// `rect` are in the coordinates of the list's groups; `rect` is the
    /// content box of the element, the viewport of the frame. The point in
    /// the frame's viewport is `point` minus the corner of `rect` (snapped
    /// to a device pixel, as the rasterizer draws the frame).
    Frame {
        /// The `iframe` element.
        node: NodeId,
        /// The point.
        point: Point,
        /// The content box.
        rect: Rect,
    },
}

impl DisplayList {
    /// The node of the topmost hit region at `point` (document coordinates,
    /// CSS px) that is not clipped away, at the viewport scroll offset
    /// `scroll` (fixed and sticky boxes move with it). Regions that are
    /// painted later are on top. Masks do not matter, as in Chromium. An SVG clip path hides what lies outside the outline of its shapes. A
    /// region in a transform group is hit in the group's coordinates; a
    /// group whose transform cannot be inverted is not hit. Groups whose
    /// bounds do not contain the point are skipped, so the list must have
    /// the bounds that [`build_display_list`] sets. A frame counts as its
    /// `iframe` element ([`DisplayList::hit_test_with`] tells them apart).
    pub fn hit_test(&self, point: Point, scroll: Point) -> Option<NodeId> {
        let mut hit = None;
        self.hits(point, scroll, &HitWork::new(), |h| {
            if let Hit::Node(node) = h {
                hit = Some(node);
            }
        });
        hit
    }

    /// The topmost hit at `point`, as [`DisplayList::hit_test`] finds it,
    /// but the content of a frame that nothing covers at the point is
    /// reported as [`Hit::Frame`]. The path work of the test is taken from
    /// `work`, so that one budget can cover a hit test that goes through
    /// several frames.
    pub fn hit_test_with(&self, point: Point, scroll: Point, work: &HitWork) -> Option<Hit> {
        let mut hit = None;
        self.hits(point, scroll, work, |h| hit = Some(h));
        hit
    }

    /// The nodes of all hit regions at `point` that are not clipped away,
    /// topmost first (as [`DisplayList::hit_test`]; a node can appear more
    /// than once). For `elementsFromPoint`.
    pub fn hit_test_all(&self, point: Point, scroll: Point) -> Vec<NodeId> {
        let mut hits = Vec::new();
        self.hits(point, scroll, &HitWork::new(), |h| {
            if let Hit::Node(node) = h {
                hits.push(node);
            }
        });
        hits.reverse();
        hits
    }

    /// Calls `hit` with each hit region and frame at `point` in paint
    /// order (see [`DisplayList::hit_test`]).
    fn hits(&self, point: Point, scroll: Point, work: &HitWork, mut hit: impl FnMut(Hit)) {
        let mut clips: Vec<bool> = Vec::new();
        // The transforms of the enclosing groups and the point in their
        // coordinates.
        let mut groups: Vec<(Matrix, Option<Point>)> = Vec::new();
        let mut matrix = Matrix::IDENTITY;
        let mut local = Some(point);
        let mut sticky = StickyCache::default();
        // The ends of the transform groups, once a group is skipped.
        let mut ends: Option<Vec<usize>> = None;
        let inside = |rect: &Rect, local: Option<Point>| local.is_some_and(|p| rect.contains(p));
        let mut i = 0;
        while let Some(item) = self.items.get(i) {
            i += 1;
            match item {
                DisplayItem::PushClip(rect) => {
                    let visible = clips.last().is_none_or(|&c| c) && inside(rect, local);
                    clips.push(visible);
                }
                DisplayItem::PushSvgClip {
                    clip,
                    transform,
                    bounds,
                    ..
                } => {
                    // The clip path is only tested inside an enclosing clip.
                    let in_clip = clips.last().is_none_or(|&c| c)
                        && inside(bounds, local)
                        && local.zip(transform.invert()).is_some_and(|(p, inverse)| {
                            crate::hit_path::clip_contains(clip, inverse.apply(p), work)
                        });
                    clips.push(in_clip);
                }
                DisplayItem::PushViewportClip => clips.push(true),
                DisplayItem::PopClip | DisplayItem::PopSvgClip => {
                    clips.pop();
                }
                DisplayItem::PushTransform {
                    transform,
                    hit_bounds,
                    fixed_bounds,
                    ..
                } => {
                    let m = transform.resolve_with(&matrix, scroll, &mut sticky);
                    let p = m.invert().map(|inverse| inverse.apply(point));
                    // A group whose bounds the point misses has no region
                    // there: skip it (and its nested groups).
                    let near = inside(hit_bounds, p)
                        || fixed_bounds.is_some_and(|f| f.translate(scroll).contains(point));
                    if !near {
                        let ends = ends.get_or_insert_with(|| transform_ends(&self.items));
                        i = ends.get(i - 1).map_or(self.items.len(), |end| end + 1);
                        continue;
                    }
                    groups.push((matrix, local));
                    matrix = m;
                    local = p;
                }
                DisplayItem::PopTransform => {
                    if let Some((m, p)) = groups.pop() {
                        matrix = m;
                        local = p;
                    }
                }
                DisplayItem::HitRegion { rect, node }
                    if clips.last().is_none_or(|&c| c) && inside(rect, local) =>
                {
                    hit(Hit::Node(*node));
                }
                DisplayItem::Frame {
                    node,
                    rect,
                    interactive: true,
                } if clips.last().is_none_or(|&c| c) && inside(rect, local) => {
                    if let Some(point) = local {
                        hit(Hit::Frame {
                            node: *node,
                            point,
                            rect: *rect,
                        });
                    }
                }
                DisplayItem::HitShape {
                    node,
                    path,
                    transform,
                    fill,
                    stroke_width,
                } if clips.last().is_none_or(|&c| c)
                    && local.is_some_and(|p| {
                        crate::hit_path::contains(path, transform, *fill, *stroke_width, p, work)
                    }) =>
                {
                    hit(Hit::Node(*node));
                }
                _ => {}
            }
        }
    }
}

/// The scroll state of the page's scroll containers, for painting.
#[derive(Clone, Copy)]
pub struct Scrolling<'a> {
    /// The scroll offset of each scroll container.
    pub offsets: &'a dyn ScrollOffsets,
    /// True to draw overlay scroll indicators on the scroll containers
    /// that the user can scroll (the GUI; screenshots for comparisons with
    /// Chromium have none, as Chromium's headless shell hides scrollbars).
    pub indicators: bool,
    /// Device pixels per CSS px. Inline SVG content is placed at a whole
    /// device pixel, as Chromium does.
    pub scale: f32,
}

impl Scrolling<'_> {
    /// Nothing is scrolled, and there are no indicators.
    pub const NONE: Scrolling<'static> = Scrolling {
        offsets: &NoScroll,
        indicators: false,
        scale: 1.0,
    };
}
