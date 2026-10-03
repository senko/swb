//! The display list: drawing commands in CSS px, in paint order.
//!
//! Built from the fragment tree by [`build_display_list`]. Paint order
//! follows a simplified form of CSS 2.2 Appendix E
//! (<https://www.w3.org/TR/CSS22/zindex.html>): normal-flow content in tree
//! order, then positioned boxes in z-index order (stable for equal values).
//! Deliberate simplifications: boxes with `z-index: auto` are treated as
//! stacking contexts, and a box paints its background, border and content
//! before the next box in tree order (no separate phases for block
//! backgrounds, floats and inline content).
//!
//! The list also contains hit regions in paint order, so that hit testing
//! ([`DisplayList::hit_test`]) finds what is painted on top.

use std::sync::Arc;

use swb_dom::NodeId;
use swb_layout::{
    BoxContent, BoxFragment, CollapsedEdge, Fragment, FragmentTree, NaturalSize, Point,
    PositionedGlyph, Rect, TextFragment,
};

use crate::control;
use swb_style::{
    BackgroundBox, BorderStyle, ComputedStyle, Image, Rgba, TextDecorationLine, Visibility, ZIndex,
};
use swb_text::FontId;

use crate::background;

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
    /// End the most recent clip.
    PopClip,
    /// Start a group that is composited with `opacity`.
    PushOpacity {
        /// The group opacity, 0 to 1. A group with opacity 0 is not drawn,
        /// but its hit regions count.
        opacity: f32,
        /// An area that contains everything the group draws.
        bounds: Rect,
    },
    /// End the most recent opacity group.
    PopOpacity,
    /// Stroke a line through `points` (butt caps, miter joins).
    Polyline {
        /// The points.
        points: Arc<[Point]>,
        /// The line width.
        width: f32,
        /// The color.
        color: Rgba,
    },
    /// An area that hit testing finds. Not drawn.
    HitRegion {
        /// The area.
        rect: Rect,
        /// The node it belongs to (an element or a text node).
        node: NodeId,
    },
}

impl DisplayItem {
    /// An area that contains everything the item draws, or `None` for items
    /// that do not draw. For text, the area is an estimate with a margin of
    /// one or more font sizes around the pen positions.
    pub(crate) fn bounds(&self) -> Option<Rect> {
        match self {
            DisplayItem::Rect { rect, .. } | DisplayItem::Border { rect, .. } => Some(*rect),
            DisplayItem::Image { rect, clip, .. }
            | DisplayItem::LinearGradient { rect, clip, .. } => rect.intersection(clip),
            DisplayItem::Text {
                origin,
                size,
                glyphs,
                ..
            } => text_bounds(*origin, *size, glyphs),
            DisplayItem::Polyline { points, width, .. } => polyline_bounds(points, *width),
            DisplayItem::PushClip(_)
            | DisplayItem::PopClip
            | DisplayItem::PushOpacity { .. }
            | DisplayItem::PopOpacity
            | DisplayItem::HitRegion { .. } => None,
        }
    }
}

/// The estimated area of a glyph run: glyphs extend at most one font size
/// to the left of their pen position, four to the right, three above the
/// baseline and two below it.
fn text_bounds(origin: Point, size: f32, glyphs: &[PositionedGlyph]) -> Option<Rect> {
    let first = glyphs.first()?;
    let (mut x0, mut x1, mut y0, mut y1) = (first.x, first.x, first.y, first.y);
    for g in glyphs {
        x0 = x0.min(g.x);
        x1 = x1.max(g.x);
        y0 = y0.min(g.y);
        y1 = y1.max(g.y);
    }
    Some(Rect::new(
        origin.x + x0 - size,
        origin.y + y0 - 3.0 * size,
        x1 - x0 + 5.0 * size,
        y1 - y0 + 5.0 * size,
    ))
}

/// The area of a stroked line: the box of its points, grown by the line
/// width (which covers miter joins of moderate angles).
fn polyline_bounds(points: &[Point], width: f32) -> Option<Rect> {
    let first = points.first()?;
    let (mut x0, mut x1, mut y0, mut y1) = (first.x, first.x, first.y, first.y);
    for p in points {
        x0 = x0.min(p.x);
        x1 = x1.max(p.x);
        y0 = y0.min(p.y);
        y1 = y1.max(p.y);
    }
    let grow = width.max(0.0) * 2.0;
    Some(Rect::new(
        x0 - grow,
        y0 - grow,
        x1 - x0 + 2.0 * grow,
        y1 - y0 + 2.0 * grow,
    ))
}

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
pub const SELECTION_TEXT: Rgba = Rgba::WHITE;

/// Natural sizes of images by reference, for background positioning.
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

impl DisplayList {
    /// The node of the topmost hit region at `point` (document coordinates,
    /// CSS px) that is not clipped away. Regions that are painted later are
    /// on top.
    pub fn hit_test(&self, point: Point) -> Option<NodeId> {
        let mut clips: Vec<bool> = Vec::new();
        let mut hit = None;
        for item in &self.items {
            match item {
                DisplayItem::PushClip(rect) => {
                    let inside = clips.last().is_none_or(|&c| c) && rect.contains(point);
                    clips.push(inside);
                }
                DisplayItem::PopClip => {
                    clips.pop();
                }
                DisplayItem::HitRegion { rect, node }
                    if clips.last().is_none_or(|&c| c) && rect.contains(point) =>
                {
                    hit = Some(*node);
                }
                _ => {}
            }
        }
        hit
    }
}

/// Builds the display list for the whole document, with the selection
/// highlight.
pub fn build_display_list(
    tree: &FragmentTree,
    images: &dyn ImageSizes,
    highlights: &dyn Highlights,
) -> DisplayList {
    let mut builder = Builder {
        list: Vec::new(),
        images,
        highlights,
        contexts: Vec::new(),
        clips: Vec::new(),
        canvas_source: None,
    };
    if let Some(canvas_background) = &tree.canvas_background {
        let bg = &canvas_background.style;
        let canvas = Rect::new(0.0, 0.0, tree.scroll_size.width, tree.scroll_size.height);
        // Images are positioned relative to the root element (CSS
        // Backgrounds 3 §2.11.1,
        // <https://www.w3.org/TR/css-backgrounds-3/#root-background>); the
        // canvas is the painting area.
        let (border, padding) = tree.root.as_ref().map_or((canvas, canvas), |root| {
            (root.border_rect, root.padding_rect())
        });
        let areas = BackgroundAreas {
            border,
            padding,
            painting: PaintingArea::Canvas(canvas),
        };
        builder.background(bg, &areas, [(0.0, 0.0); 4]);
        builder.canvas_source = Some(canvas_background.source);
    }
    if let Some(root) = &tree.root {
        builder.box_contents(root, Point::default(), &[], true);
    }
    DisplayList {
        items: builder.list,
    }
}

/// A positioned box painted after the normal flow of its stacking context.
struct Deferred {
    z: i32,
    items: Vec<DisplayItem>,
}

/// An open overflow clip.
struct OpenClip {
    /// The clip rectangle (the padding box).
    rect: Rect,
    /// The number of open stacking contexts outside the clipping box. Clips
    /// of the current stacking context have the value `contexts.len()`.
    context_depth: usize,
}

struct Builder<'a> {
    list: Vec<DisplayItem>,
    images: &'a dyn ImageSizes,
    highlights: &'a dyn Highlights,
    /// One entry per open stacking context: its positioned descendants, in
    /// tree order.
    contexts: Vec<Vec<Deferred>>,
    /// Overflow clips of the boxes that are being painted, outermost first.
    clips: Vec<OpenClip>,
    /// The element whose background paints the canvas; its own box does
    /// not paint the background again.
    canvas_source: Option<NodeId>,
}

/// A text decoration propagated from an ancestor.
#[derive(Clone, Copy)]
struct Decoration {
    lines: TextDecorationLine,
    color: Rgba,
}

/// The areas that a background uses, in document coordinates.
struct BackgroundAreas {
    border: Rect,
    padding: Rect,
    painting: PaintingArea,
}

/// Where the layers of a background are painted.
#[derive(Clone, Copy)]
enum PaintingArea {
    /// The box's own area that `background-clip` selects.
    Own,
    /// The canvas, for the root background. A gradient fills all of it.
    Canvas(Rect),
    /// A table cell, for the background of a row, row group, column or
    /// column group: the layers are positioned in the part's areas and
    /// painted in the cell (CSS 2.2 §17.5.1).
    Cell(Rect),
}

impl BackgroundAreas {
    fn of(b: &BoxFragment, origin: Point) -> Self {
        BackgroundAreas {
            border: b.border_rect.translate(origin),
            padding: b.padding_rect().translate(origin),
            painting: PaintingArea::Own,
        }
    }

    fn area(&self, which: BackgroundBox) -> Rect {
        match which {
            BackgroundBox::BorderBox => self.border,
            // `content-box` is painted as `padding-box` (not supported yet).
            _ => self.padding,
        }
    }

    fn painting_area(&self, clip: BackgroundBox) -> Rect {
        match self.painting {
            PaintingArea::Own => self.area(clip),
            PaintingArea::Canvas(area) | PaintingArea::Cell(area) => area,
        }
    }
}

/// What [`Builder::open_group`] started for a box.
struct Group {
    /// The index of the `PushOpacity` item, if the box has opacity < 1.
    opacity_item: Option<usize>,
    /// True if the box opened a stacking context.
    context: bool,
    /// `contexts.len()` before the box's own context.
    outer_depth: usize,
}

impl Builder<'_> {
    fn box_fragment(&mut self, b: &BoxFragment, origin: Point, decorations: &[Decoration]) {
        if b.style.position == swb_style::Position::Static {
            self.box_contents(b, origin, decorations, false);
            return;
        }
        // A positioned box is painted after the normal-flow content of the
        // enclosing stacking context, in z-index order (`auto` as 0). The
        // overflow clips between that stacking context and the box still
        // apply, unless the box is absolutely positioned: its containing
        // block is then outside those clipping boxes.
        let z = match b.style.z_index {
            ZIndex::Auto => 0,
            ZIndex::Integer(z) => z,
        };
        let clips: Vec<Rect> = if b.style.is_absolutely_positioned() {
            Vec::new()
        } else {
            let depth = self.contexts.len();
            self.clips
                .iter()
                .filter(|c| c.context_depth == depth)
                .map(|c| c.rect)
                .collect()
        };
        let saved = std::mem::take(&mut self.list);
        self.list
            .extend(clips.iter().map(|&r| DisplayItem::PushClip(r)));
        self.box_contents(b, origin, decorations, true);
        self.list.extend(clips.iter().map(|_| DisplayItem::PopClip));
        let items = std::mem::replace(&mut self.list, saved);
        match self.contexts.last_mut() {
            Some(context) => context.push(Deferred { z, items }),
            None => self.list.extend(items),
        }
    }

    /// Paints a box and its descendants. A stacking context root collects
    /// its positioned descendants and paints them at the end: negative
    /// z-index below its normal-flow content, the others after its content.
    /// Both are inside the box's own overflow clip.
    fn box_contents(
        &mut self,
        b: &BoxFragment,
        origin: Point,
        decorations: &[Decoration],
        stacking_context: bool,
    ) {
        let rect = b.border_rect.translate(origin);
        let group = self.open_group(b, stacking_context);
        let is_root = group.outer_depth == 0;
        self.paint_box(b, origin, is_root);
        let own = child_decorations(b, decorations);
        let clipped = self.push_overflow_clip(b, origin, group.outer_depth);
        let negative_z_at = self.list.len();
        for child in b.children.iter() {
            match child {
                Fragment::Box(cb) => self.box_fragment(cb, rect.origin(), &own),
                Fragment::Text(t) => self.text(t, rect.origin(), &own),
            }
        }
        if let BoxContent::Table(table) = &b.content
            && let Some(collapsed) = &table.collapsed
        {
            self.collapsed_borders(collapsed, rect.origin());
        }
        if let BoxContent::Control(c) = &b.content
            && b.style.visibility == Visibility::Visible
        {
            self.list.extend(control::caret(c, &b.style, rect.origin()));
        }
        if group.context {
            self.paint_deferred(negative_z_at);
        }
        if clipped {
            self.list.push(DisplayItem::PopClip);
            self.clips.pop();
        }
        self.outline(b, origin);
        self.close_group(&group);
    }

    /// Starts the opacity group and the stacking context of a box, as
    /// needed.
    fn open_group(&mut self, b: &BoxFragment, stacking_context: bool) -> Group {
        let opacity = b.style.opacity;
        let outer_depth = self.contexts.len();
        let opacity_item = (opacity < 1.0).then(|| {
            self.list.push(DisplayItem::PushOpacity {
                opacity: opacity.max(0.0),
                bounds: Rect::default(),
            });
            self.list.len() - 1
        });
        let context = stacking_context || opacity_item.is_some() || outer_depth == 0;
        if context {
            self.contexts.push(Vec::new());
        }
        Group {
            opacity_item,
            context,
            outer_depth,
        }
    }

    /// Ends the opacity group of a box and records the area it draws.
    fn close_group(&mut self, group: &Group) {
        let Some(at) = group.opacity_item else {
            return;
        };
        let bounds = self.list[at + 1..]
            .iter()
            .filter_map(DisplayItem::bounds)
            .reduce(|a, b| a.union(&b))
            .unwrap_or_default();
        if let Some(DisplayItem::PushOpacity { bounds: slot, .. }) = self.list.get_mut(at) {
            *slot = bounds;
        }
        self.list.push(DisplayItem::PopOpacity);
    }

    /// The box's own hit region, background, border and replaced content.
    fn paint_box(&mut self, b: &BoxFragment, origin: Point, is_root: bool) {
        let style = &b.style;
        if style.visibility != Visibility::Visible || b.content == BoxContent::GeometryOnly {
            return;
        }
        let rect = b.border_rect.translate(origin);
        if let Some(node) = b.node {
            self.list.push(DisplayItem::HitRegion { rect, node });
        }
        let mut areas = BackgroundAreas::of(b, origin);
        let mut border_rect = rect;
        let mut paint_border = true;
        match &b.content {
            // Cells paint the backgrounds of rows and row groups.
            BoxContent::TablePart => return,
            BoxContent::TableCell(cell) => {
                // The part's background is positioned in the part (a
                // gradient is continuous across its cells) and painted in
                // the cell.
                for part in &cell.backgrounds {
                    let area = part.area.translate(rect.origin());
                    let part_areas = BackgroundAreas {
                        border: area,
                        padding: area,
                        painting: PaintingArea::Cell(part.clip.translate(rect.origin())),
                    };
                    self.background(&part.style, &part_areas, [(0.0, 0.0); 4]);
                }
                if cell.hidden {
                    return;
                }
                paint_border = !cell.collapsed_borders;
            }
            // The table's background and border surround the grid, not the
            // captions; collapsed borders are painted after the cells.
            BoxContent::Table(table) => {
                border_rect = table.grid.translate(rect.origin());
                areas = BackgroundAreas {
                    border: border_rect,
                    padding: border_rect.inset(&b.border),
                    painting: PaintingArea::Own,
                };
                paint_border = table.collapsed.is_none();
            }
            BoxContent::None
            | BoxContent::Image(_)
            | BoxContent::GeometryOnly
            | BoxContent::Control(_) => {}
        }
        let radii = resolve_radii(style, border_rect);
        // The root's background (or the body's, if it was propagated)
        // paints the canvas instead.
        let paints_canvas =
            is_root || (b.pseudo.is_none() && b.node.is_some() && b.node == self.canvas_source);
        // Controls with the native look draw their own background and
        // border.
        let native = matches!(&b.content, BoxContent::Control(c) if c.native);
        if !paints_canvas && !native {
            self.background(style, &areas, radii);
        }
        if paint_border && !native {
            self.border(b, border_rect, radii);
        }
        if let BoxContent::Control(c) = &b.content {
            self.list.extend(control::native_look(c, style, rect));
        }
        if let BoxContent::Image(node) = b.content {
            let content = b.content_rect().translate(origin);
            self.list.push(DisplayItem::Image {
                image: ImageRef::Node(node),
                rect: content,
                tile: content,
                clip: content,
            });
        }
    }

    /// Starts the overflow clip of a box, if it has one. Returns true if it
    /// did.
    fn push_overflow_clip(&mut self, b: &BoxFragment, origin: Point, outer_depth: usize) -> bool {
        let style = &b.style;
        if !(style.overflow_x.clips() || style.overflow_y.clips()) {
            return false;
        }
        let rect = b.padding_rect().translate(origin);
        self.list.push(DisplayItem::PushClip(rect));
        self.clips.push(OpenClip {
            rect,
            context_depth: outer_depth,
        });
        true
    }

    /// Paints the positioned descendants of the stacking context that is
    /// closing: negative z-index at `negative_z_at` (below the normal-flow
    /// content), the others at the end.
    fn paint_deferred(&mut self, negative_z_at: usize) {
        let mut deferred = self.contexts.pop().unwrap_or_default();
        // Stable sort: equal z-index keeps tree order.
        deferred.sort_by_key(|d| d.z);
        let split = deferred.partition_point(|d| d.z < 0);
        let positive = deferred.split_off(split);
        let negative: Vec<DisplayItem> = deferred.into_iter().flat_map(|d| d.items).collect();
        self.list.splice(negative_z_at..negative_z_at, negative);
        for d in positive {
            self.list.extend(d.items);
        }
    }

    fn outline(&mut self, b: &BoxFragment, origin: Point) {
        let style = &b.style;
        if style.visibility != Visibility::Visible {
            return;
        }
        let rect = b.border_rect.translate(origin);
        let Some(border_style) = style.outline_style.border_style() else {
            self.focus_ring(style, with_descendants(b, origin));
            return;
        };
        if border_style == BorderStyle::None || style.outline_width <= 0.0 {
            return;
        }
        let w = style.outline_width;
        let outer = outset(rect, w + style.outline_offset);
        let color = style.outline_color.resolve(style.color);
        self.list.push(DisplayItem::Border {
            rect: outer,
            widths: [w; 4],
            colors: [color; 4],
            styles: [border_style; 4],
            radii: [(0.0, 0.0); 4],
        });
    }

    /// Paints `outline-style: auto` as Chromium does around `rect` (the
    /// border box and its descendants): a ring of 2 px in the outline
    /// color, centered on `outline-offset` outside the border box (1 px
    /// further in if the box has a border on every side), with a ring of
    /// 1 px in white around it for contrast. The corners are rounded. The
    /// outline width does not matter. An empty box gets no ring. (Measured
    /// with Chromium 148.) Deliberate simplification: the line fragments of
    /// a wrapped inline box get one ring each; Chromium merges them.
    fn focus_ring(&mut self, style: &ComputedStyle, rect: Rect) {
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return;
        }
        let min_border = style
            .border_top_width
            .min(style.border_right_width)
            .min(style.border_bottom_width)
            .min(style.border_left_width);
        let center = style.outline_offset - if min_border >= 1.0 { 1.0 } else { 0.0 };
        let radii = resolve_radii(style, rect);
        let ring = |distance: f32| -> (Rect, Radii) {
            // The radius of the ring's center line is at least 2 px.
            let radius = |r: f32| (r + distance).max(2.0 + distance - center);
            let radii = radii.map(|(h, v)| (radius(h), radius(v)));
            (outset(rect, distance), radii)
        };
        // Both rings or none: the color ring must not be degenerate.
        let (color_rect, _) = ring(center + 1.0);
        if color_rect.width <= 4.0 || color_rect.height <= 4.0 {
            return;
        }
        let color = style.outline_color.resolve(style.color);
        for (outer, width, color) in [(center + 2.0, 1.0, Rgba::WHITE), (center + 1.0, 2.0, color)]
        {
            let (rect, radii) = ring(outer);
            self.list.push(DisplayItem::Border {
                rect,
                widths: [width; 4],
                colors: [color; 4],
                styles: [BorderStyle::Solid; 4],
                radii,
            });
        }
    }

    fn background(&mut self, style: &ComputedStyle, areas: &BackgroundAreas, radii: Radii) {
        let color = style.background_color.resolve(style.color);
        // The color is painted with the clip of the bottom layer.
        let color_clip = style
            .background_clip
            .last()
            .copied()
            .unwrap_or(BackgroundBox::BorderBox);
        if !color.is_transparent() {
            self.list.push(DisplayItem::Rect {
                rect: areas.painting_area(color_clip),
                radii: match areas.painting {
                    PaintingArea::Own => radii,
                    PaintingArea::Canvas(_) | PaintingArea::Cell(_) => [(0.0, 0.0); 4],
                },
                color,
            });
        }
        // Layers are painted bottom (last) to top (first).
        for (i, image) in style.background_image.iter().enumerate().rev() {
            let (Some(image), Some(&origin), Some(&clip)) = (
                image,
                layer_value(&style.background_origin, i),
                layer_value(&style.background_clip, i),
            ) else {
                continue;
            };
            let positioning = areas.area(origin);
            let clip = areas.painting_area(clip);
            match image {
                Image::Url(url) => {
                    let image_ref = ImageRef::Url(Arc::clone(url));
                    let (
                        Some(natural),
                        Some(size),
                        Some(position_x),
                        Some(position_y),
                        Some(&repeat),
                    ) = (
                        self.images.size(&image_ref),
                        layer_value(&style.background_size, i),
                        layer_value(&style.background_position_x, i),
                        layer_value(&style.background_position_y, i),
                        layer_value(&style.background_repeat, i),
                    )
                    else {
                        continue;
                    };
                    let layer = background::Layer {
                        size,
                        position_x,
                        position_y,
                        repeat,
                    };
                    let (tile, area) = background::tile(&layer, positioning, clip, &natural);
                    self.list.push(DisplayItem::Image {
                        image: image_ref,
                        rect: area,
                        tile,
                        clip,
                    });
                }
                Image::LinearGradient(gradient) => {
                    // Gradients are not tiled yet: one gradient fills the
                    // positioning area (the whole canvas for the canvas).
                    let rect = match areas.painting {
                        PaintingArea::Own => clip.intersection(&positioning).unwrap_or(clip),
                        PaintingArea::Canvas(_) => clip,
                        PaintingArea::Cell(_) => positioning,
                    };
                    self.list.push(DisplayItem::LinearGradient {
                        rect,
                        clip,
                        gradient: Arc::clone(gradient),
                        current_color: style.color,
                    });
                }
            }
        }
    }

    /// Paints the borders of a table with collapsing borders. `origin` is
    /// the table's border-box origin.
    fn collapsed_borders(&mut self, edges: &[CollapsedEdge], origin: Point) {
        for edge in edges {
            let width = if edge.vertical {
                edge.rect.width
            } else {
                edge.rect.height
            };
            if width <= 0.0 || edge.style == BorderStyle::None || edge.style == BorderStyle::Hidden
            {
                continue;
            }
            // A one-sided border: the left side of a vertical segment, the
            // top side of a horizontal one.
            let widths = if edge.vertical {
                [0.0, 0.0, 0.0, width]
            } else {
                [width, 0.0, 0.0, 0.0]
            };
            self.list.push(DisplayItem::Border {
                rect: edge.rect.translate(origin),
                widths,
                colors: [edge.color; 4],
                styles: [edge.style; 4],
                radii: [(0.0, 0.0); 4],
            });
        }
    }

    fn border(&mut self, b: &BoxFragment, rect: Rect, radii: Radii) {
        let s = &b.style;
        let widths = [b.border.top, b.border.right, b.border.bottom, b.border.left];
        if widths.iter().all(|w| *w <= 0.0) {
            return;
        }
        let colors = [
            s.border_top_color.resolve(s.color),
            s.border_right_color.resolve(s.color),
            s.border_bottom_color.resolve(s.color),
            s.border_left_color.resolve(s.color),
        ];
        let styles = [
            s.border_top_style,
            s.border_right_style,
            s.border_bottom_style,
            s.border_left_style,
        ];
        self.list.push(DisplayItem::Border {
            rect,
            widths,
            colors,
            styles,
            radii,
        });
    }

    /// Paints a text fragment with the decorations of its ancestors. The
    /// decoration of the element that contains the text is already in
    /// `decorations` (added by that element's box).
    fn text(&mut self, t: &TextFragment, origin: Point, decorations: &[Decoration]) {
        let style = &t.style;
        if style.visibility != Visibility::Visible {
            return;
        }
        let rect = t.rect.translate(origin);
        self.list
            .push(DisplayItem::HitRegion { rect, node: t.node });
        if t.glyphs.is_empty() {
            return;
        }
        let baseline = Point::new(rect.x, rect.y + t.baseline);
        // The highlight fills the line box, as in Chromium.
        let selected = self
            .highlights
            .selected(t.node)
            .filter(|_| t.is_selectable())
            .and_then(|(start, end)| t.x_range(start, end))
            .map(|(x0, x1)| Rect::new(rect.x + x0, rect.y + t.line_top, x1 - x0, t.line_height));
        if let Some(selected) = selected {
            self.list.push(DisplayItem::Rect {
                rect: selected,
                radii: [(0.0, 0.0); 4],
                color: SELECTION_BACKGROUND,
            });
        }
        self.text_and_decorations(t, rect, baseline, decorations, None);
        if let Some(selected) = selected {
            // The selected part again, in the selection color.
            self.list.push(DisplayItem::PushClip(selected));
            self.text_and_decorations(t, rect, baseline, decorations, Some(SELECTION_TEXT));
            self.list.push(DisplayItem::PopClip);
        }
    }

    /// Paints the glyphs and the decorations of a text fragment, in their
    /// own colors or all in `color`. Underlines and overlines are painted
    /// below the text, line-through above it.
    fn text_and_decorations(
        &mut self,
        t: &TextFragment,
        rect: Rect,
        baseline: Point,
        decorations: &[Decoration],
        color: Option<Rgba>,
    ) {
        let thickness = (t.font_size / 16.0).max(1.0).round();
        for d in decorations {
            let line_color = color.unwrap_or(d.color);
            if d.lines.contains(TextDecorationLine::UNDERLINE) {
                let y = (baseline.y + (t.font_size / 9.0).max(1.0)).round();
                self.decoration_line(rect.x, y, rect.width, thickness, line_color);
            }
            if d.lines.contains(TextDecorationLine::OVERLINE) {
                self.decoration_line(rect.x, rect.y.round(), rect.width, thickness, line_color);
            }
        }
        self.list.push(DisplayItem::Text {
            origin: baseline,
            font: t.font,
            size: t.font_size,
            glyphs: Arc::clone(&t.glyphs),
            color: color.unwrap_or(t.style.color),
        });
        for d in decorations {
            if d.lines.contains(TextDecorationLine::LINE_THROUGH) {
                let y = (baseline.y - t.font_size * 0.3).round();
                self.decoration_line(rect.x, y, rect.width, thickness, color.unwrap_or(d.color));
            }
        }
    }

    fn decoration_line(&mut self, x: f32, y: f32, width: f32, thickness: f32, color: Rgba) {
        self.list.push(DisplayItem::Rect {
            rect: Rect::new(x, y, width, thickness),
            radii: [(0.0, 0.0); 4],
            color,
        });
    }
}

/// The border box of `b` united with the boxes and text of its descendants
/// (Chromium's focus ring encloses them, for example an image in a link).
/// The content of a box that clips its overflow does not count. `origin`
/// is the absolute position of the parent's border-box origin.
fn with_descendants(b: &BoxFragment, origin: Point) -> Rect {
    let rect = b.border_rect.translate(origin);
    if b.style.overflow_x.clips() || b.style.overflow_y.clips() {
        return rect;
    }
    let own = rect.origin();
    b.children.iter().fold(rect, |acc, child| {
        let r = match child {
            Fragment::Box(child) => with_descendants(child, own),
            Fragment::Text(t) => t.rect.translate(own),
        };
        if r.width > 0.0 || r.height > 0.0 {
            acc.union(&r)
        } else {
            acc
        }
    })
}

/// `rect` grown by `amount` on every side (shrunk if negative).
fn outset(rect: Rect, amount: f32) -> Rect {
    Rect::new(
        rect.x - amount,
        rect.y - amount,
        rect.width + 2.0 * amount,
        rect.height + 2.0 * amount,
    )
}

/// The value of a background property for layer `i`. The lists of the
/// properties repeat as needed (CSS Backgrounds 3 §2.1,
/// <https://www.w3.org/TR/css-backgrounds-3/#layering>); `None` only for an
/// empty list.
fn layer_value<T>(list: &[T], i: usize) -> Option<&T> {
    list.get(i % list.len().max(1))
}

/// The decorations that the content of `b` gets: those propagated from
/// its ancestors and its own. Decorations propagate to inline content and
/// in-flow blocks, but not into atomic inlines, floats or absolutely
/// positioned boxes.
fn child_decorations(b: &BoxFragment, decorations: &[Decoration]) -> Vec<Decoration> {
    let style = &b.style;
    let mut own: Vec<Decoration> = if b.is_inline || is_block_container_for_text(style) {
        decorations.to_vec()
    } else {
        Vec::new()
    };
    if !style.text_decoration_line.is_empty() {
        own.push(Decoration {
            lines: style.text_decoration_line,
            color: style.text_decoration_color.resolve(style.color),
        });
    }
    own
}

/// True for block containers whose text inherits their decorations
/// (everything except atomic inlines, floats and positioned boxes).
fn is_block_container_for_text(style: &ComputedStyle) -> bool {
    !style.display.is_inline_level() && !style.is_floating() && !style.is_absolutely_positioned()
}

/// Resolves `border-*-radius` against the border box, scaling down
/// overlapping radii (CSS Backgrounds 3 §4.5,
/// <https://www.w3.org/TR/css-backgrounds-3/#corner-overlap>).
fn resolve_radii(style: &ComputedStyle, rect: Rect) -> Radii {
    let r = |c: &swb_style::CornerRadius| {
        (
            c.horizontal.resolve(rect.width).max(0.0),
            c.vertical.resolve(rect.height).max(0.0),
        )
    };
    let mut radii = [
        r(&style.border_top_left_radius),
        r(&style.border_top_right_radius),
        r(&style.border_bottom_right_radius),
        r(&style.border_bottom_left_radius),
    ];
    let sums = [
        (radii[0].0 + radii[1].0, rect.width),
        (radii[1].1 + radii[2].1, rect.height),
        (radii[2].0 + radii[3].0, rect.width),
        (radii[3].1 + radii[0].1, rect.height),
    ];
    let f = sums
        .iter()
        .filter(|(sum, _)| *sum > 0.0)
        .map(|(sum, len)| len / sum)
        .fold(1.0_f32, f32::min);
    if f < 1.0 {
        for (h, v) in &mut radii {
            *h *= f;
            *v *= f;
        }
    }
    radii
}
