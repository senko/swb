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
    BoxContent, BoxFragment, Fragment, FragmentTree, Point, PositionedGlyph, Rect, TextFragment,
};
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
    /// Fill `rect` with a linear gradient.
    LinearGradient {
        /// The area.
        rect: Rect,
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
            DisplayItem::Rect { rect, .. }
            | DisplayItem::Border { rect, .. }
            | DisplayItem::LinearGradient { rect, .. } => Some(*rect),
            DisplayItem::Image { rect, clip, .. } => rect.intersection(clip),
            DisplayItem::Text {
                origin,
                size,
                glyphs,
                ..
            } => text_bounds(*origin, *size, glyphs),
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

/// A reference to an image.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ImageRef {
    /// The image of a replaced element.
    Node(NodeId),
    /// An image loaded from a URL (backgrounds, list markers).
    Url(Arc<str>),
}

/// Natural sizes of images by reference, for background positioning.
pub trait ImageSizes {
    /// The natural size of an image in CSS px, if it is loaded.
    fn size(&self, image: &ImageRef) -> Option<(f32, f32)>;
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

/// Builds the display list for the whole document.
pub fn build_display_list(tree: &FragmentTree, images: &dyn ImageSizes) -> DisplayList {
    let mut builder = Builder {
        list: Vec::new(),
        images,
        contexts: Vec::new(),
        clips: Vec::new(),
        canvas_source: None,
    };
    if let Some(canvas_background) = &tree.canvas_background {
        let bg = &canvas_background.style;
        let canvas = Rect::new(0.0, 0.0, tree.scroll_size.width, tree.scroll_size.height);
        // Images are positioned relative to the root element (CSS
        // Backgrounds 3 §2.11.2); the canvas is the painting area.
        let (border, padding) = tree.root.as_ref().map_or((canvas, canvas), |root| {
            (root.border_rect, root.padding_rect())
        });
        let areas = BackgroundAreas {
            border,
            padding,
            canvas: Some(canvas),
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
    /// For the canvas background: the painting area of all layers.
    canvas: Option<Rect>,
}

impl BackgroundAreas {
    fn of(b: &BoxFragment, origin: Point) -> Self {
        BackgroundAreas {
            border: b.border_rect.translate(origin),
            padding: b.padding_rect().translate(origin),
            canvas: None,
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
        self.canvas.unwrap_or_else(|| self.area(clip))
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
        if group.context {
            self.paint_deferred(negative_z_at);
        }
        if clipped {
            self.list.push(DisplayItem::PopClip);
            self.clips.pop();
        }
        self.outline(&b.style, rect);
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
        if style.visibility != Visibility::Visible {
            return;
        }
        let rect = b.border_rect.translate(origin);
        if let Some(node) = b.node {
            self.list.push(DisplayItem::HitRegion { rect, node });
        }
        let radii = resolve_radii(style, rect);
        // The root's background (or the body's, if it was propagated)
        // paints the canvas instead.
        let paints_canvas =
            is_root || (b.pseudo.is_none() && b.node.is_some() && b.node == self.canvas_source);
        if !paints_canvas {
            self.background(style, &BackgroundAreas::of(b, origin), radii);
        }
        self.border(b, rect, radii);
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

    fn outline(&mut self, style: &ComputedStyle, rect: Rect) {
        if style.visibility != Visibility::Visible
            || style.outline_style == BorderStyle::None
            || style.outline_width <= 0.0
        {
            return;
        }
        let w = style.outline_width;
        let grow = w + style.outline_offset;
        let outer = Rect::new(
            rect.x - grow,
            rect.y - grow,
            rect.width + 2.0 * grow,
            rect.height + 2.0 * grow,
        );
        let color = style.outline_color.resolve(style.color);
        self.list.push(DisplayItem::Border {
            rect: outer,
            widths: [w; 4],
            colors: [color; 4],
            styles: [style.outline_style; 4],
            radii: [(0.0, 0.0); 4],
        });
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
                radii: if areas.canvas.is_some() {
                    [(0.0, 0.0); 4]
                } else {
                    radii
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
                    let (tile, area) = background::tile(&layer, positioning, clip, natural);
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
                    let rect = if areas.canvas.is_some() {
                        clip
                    } else {
                        clip.intersection(&positioning).unwrap_or(clip)
                    };
                    self.list.push(DisplayItem::LinearGradient {
                        rect,
                        gradient: Arc::clone(gradient),
                        current_color: style.color,
                    });
                }
            }
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
        // Underlines and overlines are painted below the text, line-through
        // above it.
        let thickness = (t.font_size / 16.0).max(1.0).round();
        for d in decorations {
            if d.lines.contains(TextDecorationLine::UNDERLINE) {
                let y = (baseline.y + (t.font_size / 9.0).max(1.0)).round();
                self.decoration_line(rect.x, y, rect.width, thickness, d.color);
            }
            if d.lines.contains(TextDecorationLine::OVERLINE) {
                self.decoration_line(rect.x, rect.y.round(), rect.width, thickness, d.color);
            }
        }
        self.list.push(DisplayItem::Text {
            origin: baseline,
            font: t.font,
            size: t.font_size,
            glyphs: Arc::clone(&t.glyphs),
            color: style.color,
        });
        for d in decorations {
            if d.lines.contains(TextDecorationLine::LINE_THROUGH) {
                let y = (baseline.y - t.font_size * 0.3).round();
                self.decoration_line(rect.x, y, rect.width, thickness, d.color);
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

/// The value of a background property for layer `i`. The lists of the
/// properties repeat as needed (CSS Backgrounds 3 §2.2); `None` only for an
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
/// overlapping radii (CSS Backgrounds 3 §5.5).
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
