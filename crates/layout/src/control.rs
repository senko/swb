//! Form controls: text fields, text areas, buttons, checkboxes, radio
//! buttons and drop-down selects.
//!
//! The engine knows the state of each control (its value, checkedness,
//! caret) and supplies it through [`FormControls`]. A control is an atomic
//! box with its own formatting context, like a replaced element (also
//! with `display: inline` or `block`). Box construction gives it generated
//! content:
//!
//! - text fields, buttons and selects: one line of text (the value or the
//!   placeholder, the label, the selected option) in an inline formatting
//!   context; text areas: the value, wrapped;
//! - `<button>` elements: their DOM content (as flex items with
//!   `display: flex`);
//! - checkboxes and radio buttons: nothing (paint draws them).
//!
//! The text of a text field or a text area has caret stops: byte offsets
//! in [`Control::text`]. Its text fragments belong to the control element.
//!
//! Sizes follow Chromium 148 (measured; see `tests/layout/forms-*.html`):
//!
//! - A text field is `size` characters wide (default 20): with the average
//!   character width `a` of the primary font and its maximum character
//!   width `m` (the bounding box width, rounded), `ceil(a * size + m - a)`.
//!   `a` is OS/2 `xAvgCharWidth`, rounded up when its fraction is 0.5 or
//!   more (Blink rounds it, and a value that rounds down is not used),
//!   both at the font size in 26.6 fixed point. For the families whose
//!   average width Blink does not trust (`Courier`, `Times`, `Helvetica`
//!   and others), `a` is the width of `0` and `m` is not added. The height
//!   is the line height.
//! - A text area is `ceil(a * cols)` plus a scroll bar (15 px) wide and
//!   `rows` lines high.
//! - Buttons and `<button>` elements are as wide as their content; their
//!   content is centered vertically.
//! - A select is as wide as its widest option (indented by four spaces in
//!   an `optgroup`, measured with the select's font, rounded up) plus 4 px
//!   of padding on the left and 16 px on the right (the arrow), and one
//!   line plus 1 px above and below high.
//! - Checkboxes and radio buttons are 13 × 13 px; their baseline is the
//!   bottom of the border box.
//!
//! The baseline of a text field, a button or a select is the baseline of
//! its text (the bottom of the content box for an empty `<button>`); a
//! text area has none (it is a scroll container).

use std::sync::Arc;

use swb_dom::NodeId;
use swb_style::{
    BackgroundBox, BorderStyle, BoxSizing, ComputedStyle, Display, PseudoKind, Rgba, WhiteSpace,
};

use crate::LayoutContext;
use crate::block::{
    Baselines, BoxEdges, ChildOptions, ContainingBlock, clamp_height, clamp_width, finish_fragment,
    layout_block_container, resolve_size,
};
use crate::box_tree::{
    BlockContainer, BoxBase, BuildContext, BuildState, IndependentBox, IndependentContents,
    InlineFormattingContext, build_block_container, build_flex_items, control_text,
};
use crate::fonts;
use crate::fragment::{BoxContent, BoxFragment, ControlContent, Fragment};
use crate::geom::{Point, Rect, clamp_length};
use crate::inline;
use crate::intrinsic::{self, ContentSizes};

/// The states of form controls, supplied by the engine.
pub trait FormControls {
    /// True if element `node` is a form control. Cheaper than
    /// [`FormControls::control`].
    fn is_control(&self, node: NodeId) -> bool;

    /// The control that element `node` renders, or `None` if it is not a
    /// form control (it then gets an ordinary box).
    fn control(&self, node: NodeId) -> Option<Control>;
}

/// A [`FormControls`] without controls: every element gets an ordinary
/// box.
pub struct NoFormControls;

impl FormControls for NoFormControls {
    fn is_control(&self, _node: NodeId) -> bool {
        false
    }

    fn control(&self, _node: NodeId) -> Option<Control> {
        None
    }
}

/// How a form control is laid out and painted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlKind {
    /// A one-line text field, `size` characters wide.
    TextField {
        /// The `size` attribute (at least 1).
        size: u32,
    },
    /// A text area of `cols` × `rows` characters.
    TextArea {
        /// The `cols` attribute (at least 1).
        cols: u32,
        /// The `rows` attribute (at least 1).
        rows: u32,
    },
    /// An `input` button (submit, reset, button): its label.
    Button,
    /// A `<button>` element: its content.
    ButtonElement,
    /// A checkbox.
    Checkbox,
    /// A radio button.
    Radio,
    /// A drop-down `<select>`: the selected option and an arrow.
    Select,
}

/// A form control as layout sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct Control {
    /// The kind.
    pub kind: ControlKind,
    /// The text that the control shows: the value of a text field (one
    /// bullet per character for a password), the placeholder, the label
    /// of a button, the selected option of a select. Empty for the other
    /// kinds.
    pub text: String,
    /// True if `text` is the placeholder, which has the `::placeholder`
    /// style and no caret stops.
    pub placeholder: bool,
    /// The labels of all options of a select (indented in an `optgroup`),
    /// for its width.
    pub options: Vec<String>,
    /// The caret offset in `text` (a character boundary), if the control
    /// is focused and editable and the selection is collapsed.
    pub caret: Option<usize>,
    /// The moving end of the selection (the caret position, also with a
    /// selection or in a read-only field) as an offset in `text`, if the
    /// control is focused. Layout scrolls the text so that it is visible.
    pub focus: Option<usize>,
    /// The scroll offset of the text (from the previous layout, see
    /// [`ControlContent::scroll`]).
    pub scroll: Point,
    /// True for a checked checkbox or radio button.
    pub checked: bool,
    /// True if the control is disabled.
    pub disabled: bool,
}

/// The box of a form control.
#[derive(Debug)]
pub(crate) struct ControlBox {
    pub(crate) control: Control,
    pub(crate) contents: ControlContents,
}

/// The content of a control box.
#[derive(Debug)]
pub(crate) enum ControlContents {
    /// Nothing (checkboxes and radio buttons).
    None,
    /// Generated text in the style `style`.
    Text {
        ifc: InlineFormattingContext,
        style: Arc<ComputedStyle>,
    },
    /// The DOM content of a `<button>`.
    Flow(BlockContainer),
    /// The DOM content of a `<button>` with `display: flex`: flex items.
    Flex(Vec<IndependentBox>),
}

/// The size of checkboxes and radio buttons (Chromium's theme).
const CHECKBOX_SIZE: f32 = 13.0;

/// The width that Chromium reserves for the scroll bar of a text area.
const SCROLLBAR_WIDTH: f32 = 15.0;

/// The space around the label of a select: left, right (with the arrow),
/// top and bottom.
const SELECT_PADDING: (f32, f32, f32, f32) = (4.0, 16.0, 1.0, 1.0);

/// At most this many options count for the width of a select (the engine
/// sends at most this many labels).
pub const MAX_SELECT_OPTIONS: usize = 10_000;

/// Families whose average character width Blink ignores
/// (`HasValidAvgCharWidth` in `layout_text_control.cc`).
const INVALID_AVG_CHAR_WIDTH_FAMILIES: [&str; 31] = [
    "American Typewriter",
    "Arial Hebrew",
    "Chalkboard",
    "Cochin",
    "Corsiva Hebrew",
    "Courier",
    "Euphemia UCAS",
    "Geneva",
    "Gill Sans",
    "Hei",
    "Helvetica",
    "Hoefler Text",
    "InaiMathi",
    "Marker Felt",
    "Monaco",
    "Mshtakan",
    "New Peninim MT",
    "Osaka",
    "Raanana",
    "STHeiti",
    "Symbol",
    "Times",
    "Apple Braille",
    "Apple LiGothic",
    "Apple LiSung",
    "Apple Symbols",
    "AppleGothic",
    "AppleMyungjo",
    "#GungSeo",
    "#HeadLineA",
    "#PCMyungjo",
];

/// Builds the contents of the box of control element `node`, if it is a
/// control.
pub(crate) fn build(
    ctx: &BuildContext<'_>,
    node: NodeId,
    base: &BoxBase,
    state: &mut BuildState,
) -> Option<IndependentContents> {
    let control = ctx.controls.control(node)?;
    let contents = match control.kind {
        ControlKind::Checkbox | ControlKind::Radio => ControlContents::None,
        ControlKind::ButtonElement => match base.style.display {
            Display::Flex | Display::InlineFlex => {
                ControlContents::Flex(build_flex_items(ctx, base, state))
            }
            _ => ControlContents::Flow(build_block_container(ctx, base, state)),
        },
        _ => {
            let style = text_style(ctx, node, &base.style, &control);
            let selectable = !control.placeholder
                && matches!(
                    control.kind,
                    ControlKind::TextField { .. } | ControlKind::TextArea { .. }
                );
            let ifc = control_text(node, &style, &control.text, selectable, state);
            ControlContents::Text { ifc, style }
        }
    };
    Some(IndependentContents::Control(Box::new(ControlBox {
        control,
        contents,
    })))
}

/// The style of the text of a control: the control's style (or its
/// `::placeholder` style) inherited by an anonymous box. Text fields,
/// buttons and selects do not wrap.
fn text_style(
    ctx: &BuildContext<'_>,
    node: NodeId,
    style: &Arc<ComputedStyle>,
    control: &Control,
) -> Arc<ComputedStyle> {
    let source = if control.placeholder {
        ctx.styles
            .pseudo(node, PseudoKind::Placeholder)
            .unwrap_or(style)
    } else {
        style
    };
    let mut text = ComputedStyle::anonymous_from(source);
    text.white_space = match control.kind {
        ControlKind::TextArea { .. } if control.placeholder => WhiteSpace::PreWrap,
        ControlKind::TextArea { .. } => style.white_space,
        _ => WhiteSpace::Pre,
    };
    Arc::new(text)
}

/// The min-content and max-content widths of a control's content box.
pub(crate) fn content_sizes(
    ctx: &mut LayoutContext<'_>,
    style: &ComputedStyle,
    control: &ControlBox,
) -> ContentSizes {
    let width = match (&control.control.kind, &control.contents) {
        (ControlKind::TextField { size }, _) => text_field_width(ctx, style, *size),
        (ControlKind::TextArea { cols, .. }, _) => text_area_width(ctx, style, *cols),
        (ControlKind::Checkbox | ControlKind::Radio, _) => checkbox_content_size(style, true),
        (ControlKind::Select, _) => {
            options_width(ctx, style, &control.control.options)
                + SELECT_PADDING.0
                + SELECT_PADDING.1
        }
        (ControlKind::Button, ControlContents::Text { ifc, style }) => {
            inline::content_sizes(ctx, ifc, style).max
        }
        (ControlKind::ButtonElement, ControlContents::Flow(container)) => {
            return intrinsic::container_content_sizes(ctx, container, style);
        }
        (ControlKind::ButtonElement, ControlContents::Flex(items)) => {
            return intrinsic::flex_content_sizes(ctx, style, items);
        }
        _ => 0.0,
    };
    let width = clamp_length(width);
    ContentSizes {
        min: width,
        max: width,
    }
}

/// The used width of a block-level control: the specified width, or
/// shrink-to-fit (controls do not fill their containing block).
pub(crate) fn block_level_width(
    ctx: &mut LayoutContext<'_>,
    style: &ComputedStyle,
    control: &ControlBox,
    cb: ContainingBlock,
    edges: &BoxEdges,
) -> f32 {
    let edge_sum = edges.sum().horizontal();
    let width = resolve_size(&style.width, Some(cb.width), style.box_sizing, edge_sum)
        .unwrap_or_else(|| {
            let sizes = content_sizes(ctx, style, control);
            let margin = |m: &swb_style::LengthPercentageOrAuto| m.resolve(cb.width).unwrap_or(0.0);
            let available =
                (cb.width - margin(&style.margin_left) - margin(&style.margin_right) - edge_sum)
                    .max(0.0);
            sizes.max.min(available).max(sizes.min)
        });
    clamp_width(style, width, cb.width, edge_sum)
}

/// The laid-out content of a control, relative to the top left of the area
/// it is placed in.
struct Inner {
    fragments: Vec<Fragment>,
    /// The width of the text (the right edge of its last glyph).
    width: f32,
    /// The height of the content: the line boxes, or one line if there is
    /// no text.
    height: f32,
    /// The baseline of the first line.
    baseline: Option<f32>,
    /// The caret, if the control has one.
    caret: Option<Rect>,
    /// The position of the moving end of the selection, which scrolling
    /// keeps visible.
    focus: Option<Rect>,
}

impl Inner {
    /// Content without text: block or flex children (or none).
    fn boxes(fragments: Vec<Fragment>, width: f32, height: f32, baseline: Option<f32>) -> Inner {
        Inner {
            fragments,
            width,
            height,
            baseline,
            caret: None,
            focus: None,
        }
    }
}

/// Lays out a control with a content-box width of `content_width`. The
/// height is `content_height` if given, else the specified height, else
/// the intrinsic height; then clamped. The fragment is at (0, 0).
pub(crate) fn layout(
    ctx: &mut LayoutContext<'_>,
    base: &BoxBase,
    control: &ControlBox,
    content_width: f32,
    content_height: Option<f32>,
    cb: ContainingBlock,
) -> BoxFragment {
    let style = &base.style;
    let edges = BoxEdges::resolve(style, cb.width);
    let edge_sum = edges.sum();
    let kind = control.control.kind;
    let text_width = match kind {
        ControlKind::Select => (content_width - SELECT_PADDING.0 - SELECT_PADDING.1).max(0.0),
        _ => content_width,
    };
    let inner = layout_inner(ctx, base.node, control, style, text_width);
    let height = content_height.unwrap_or_else(|| {
        let intrinsic = intrinsic_height(ctx, style, kind, inner.height);
        let vertical = edge_sum.vertical();
        let specified = resolve_size(&style.height, cb.height, style.box_sizing, vertical);
        clamp_height(style, specified.unwrap_or(intrinsic), cb.height, vertical)
    });
    let (x, y) = content_position(kind, height, inner.height);
    let scroll = scroll_offset(&control.control, &inner, content_width, height);
    let dx = x - scroll.x;
    let dy = y - scroll.y;
    let mut children = inner.fragments;
    for child in &mut children {
        child.move_by(dx, dy);
    }
    let baseline = match kind {
        ControlKind::TextArea { .. } => None,
        ControlKind::Checkbox | ControlKind::Radio => Some(height + edge_sum.vertical()),
        // A button without text: the bottom of the content box.
        ControlKind::ButtonElement if inner.baseline.is_none() => Some(edge_sum.top + height),
        _ => inner.baseline.map(|b| b + dy + edge_sum.top),
    };
    let border_box = Rect::new(
        0.0,
        0.0,
        content_width + edge_sum.horizontal(),
        height + edge_sum.vertical(),
    );
    let baselines = Baselines {
        first: baseline,
        last: baseline,
    };
    let mut fragment = finish_fragment(base, border_box, &edges, children, baselines);
    fragment.content = BoxContent::Control(ControlContent {
        kind,
        checked: control.control.checked,
        disabled: control.control.disabled,
        native: has_native_look(kind, style),
        caret: inner
            .caret
            .map(|c| c.translate(Point::new(dx + edge_sum.left, dy + edge_sum.top))),
        scroll,
    });
    fragment
}

/// The content-box height of a control without a specified height, for
/// content `inner_height` px high.
fn intrinsic_height(
    ctx: &mut LayoutContext<'_>,
    style: &ComputedStyle,
    kind: ControlKind,
    inner_height: f32,
) -> f32 {
    match kind {
        ControlKind::TextArea { rows, .. } => clamp_length(rows as f32 * line_height(ctx, style)),
        ControlKind::Checkbox | ControlKind::Radio => checkbox_content_size(style, false),
        ControlKind::Select => inner_height + SELECT_PADDING.2 + SELECT_PADDING.3,
        _ => inner_height,
    }
}

/// Where content `inner_height` px high goes in a content box `height` px
/// high, before scrolling: text areas start at the top, other controls
/// center their content vertically.
fn content_position(kind: ControlKind, height: f32, inner_height: f32) -> (f32, f32) {
    match kind {
        ControlKind::TextArea { .. } => (0.0, 0.0),
        ControlKind::Select => (
            SELECT_PADDING.0,
            SELECT_PADDING.2 + (height - SELECT_PADDING.2 - SELECT_PADDING.3 - inner_height) / 2.0,
        ),
        _ => (0.0, (height - inner_height) / 2.0),
    }
}

/// Lays out the content of a control in an area `width` px wide.
fn layout_inner(
    ctx: &mut LayoutContext<'_>,
    node: Option<NodeId>,
    control: &ControlBox,
    style: &Arc<ComputedStyle>,
    width: f32,
) -> Inner {
    let cb = ContainingBlock {
        width,
        height: None,
    };
    match &control.contents {
        ControlContents::None => Inner::boxes(Vec::new(), 0.0, 0.0, None),
        ControlContents::Flex(items) => {
            let flex = crate::flex::layout_flex(ctx, style, items, cb);
            Inner::boxes(
                flex.fragments,
                width,
                flex.content_height,
                flex.first_baseline,
            )
        }
        ControlContents::Flow(container) => {
            let options = ChildOptions {
                collapse_with_parent_start: false,
                collapse_with_parent_end: false,
            };
            let children =
                layout_block_container(ctx, container, style, cb, options, &mut Vec::new());
            Inner::boxes(
                children.fragments,
                width,
                children.content_height,
                children.baselines.first,
            )
        }
        ControlContents::Text { ifc, style } => {
            layout_text(ctx, node, &control.control, ifc, style, width)
        }
    }
}

/// Lays out the generated text of a control, with its caret and the
/// position of the moving end of its selection.
fn layout_text(
    ctx: &mut LayoutContext<'_>,
    node: Option<NodeId>,
    control: &Control,
    ifc: &InlineFormattingContext,
    style: &Arc<ComputedStyle>,
    width: f32,
) -> Inner {
    let lines = inline::layout_inline(ctx, ifc, style, width, &mut Vec::new());
    let (above, below, ascent, descent) = strut(ctx, style);
    let text_width = lines
        .fragments
        .iter()
        .map(|f| match f {
            Fragment::Text(t) => t.rect.right(),
            Fragment::Box(b) => b.border_rect.right(),
        })
        .fold(0.0, f32::max);
    let (height, baseline) = if lines.line_count == 0 {
        (above + below, Some(above))
    } else {
        (lines.height, lines.first_baseline)
    };
    let empty_line = EmptyLine {
        x: empty_line_x(style, width),
        height: above + below,
        caret_top: above - ascent,
        caret_height: ascent + descent,
    };
    let rect_at = |offset| caret_rect(&lines.fragments, node, offset, &control.text, &empty_line);
    let caret = control.caret.map(rect_at);
    let focus = control.focus.map(rect_at);
    Inner {
        fragments: lines.fragments,
        width: text_width,
        height,
        baseline,
        caret,
        focus,
    }
}

/// The x of the caret on an empty line of `width`, for `text-align`.
fn empty_line_x(style: &ComputedStyle, width: f32) -> f32 {
    use swb_style::TextAlign;
    match style.text_align {
        TextAlign::Center | TextAlign::WebkitCenter => width / 2.0,
        TextAlign::Right | TextAlign::WebkitRight | TextAlign::End => (width - 1.0).max(0.0),
        _ => 0.0,
    }
}

/// The geometry of the caret on a line without text.
struct EmptyLine {
    /// The x of the caret (for `text-align`).
    x: f32,
    /// The height of a line.
    height: f32,
    /// The top of the caret below the top of the line, and its height.
    caret_top: f32,
    caret_height: f32,
}

/// The caret at byte offset `offset` of `text`, from the caret stops of
/// the text fragments: the last stop at the offset (at a wrapped line end,
/// the start of the next line), or else the last stop before it (inside a
/// cluster). At the start of an empty line (no text, between two line
/// breaks, after a final line break), the caret is at the start of that
/// line: below the line of the last stop before it, one line per line
/// break.
fn caret_rect(
    fragments: &[Fragment],
    node: Option<NodeId>,
    offset: usize,
    text: &str,
    empty: &EmptyLine,
) -> Rect {
    let target = u32::try_from(offset).unwrap_or(u32::MAX);
    // The last stop at or before the offset, and its fragment.
    let mut best: Option<(u32, Rect, f32, f32)> = None;
    for fragment in fragments {
        let Fragment::Text(t) = fragment else {
            continue;
        };
        if Some(t.node) != node {
            continue;
        }
        for stop in t.carets.iter().filter(|c| c.offset <= target) {
            if best.is_none_or(|(o, ..)| stop.offset >= o) {
                let rect = Rect::new(t.rect.x + stop.x, t.rect.y, 1.0, t.rect.height);
                best = Some((stop.offset, rect, t.rect.y + t.line_top, t.line_height));
            }
        }
    }
    let at_empty_line = text
        .get(..offset)
        .is_none_or(|before| before.is_empty() || before.ends_with('\n'))
        && text
            .get(offset..)
            .is_none_or(|after| after.is_empty() || after.starts_with('\n'));
    match best {
        Some((_, rect, ..)) if !at_empty_line => rect,
        Some((stop, _, line_top, line_height)) => {
            let breaks = text
                .get(stop as usize..offset)
                .map_or(0, |between| between.matches('\n').count());
            let top = line_top + line_height + breaks.saturating_sub(1) as f32 * empty.height;
            Rect::new(empty.x, top + empty.caret_top, 1.0, empty.caret_height)
        }
        None => {
            let breaks = text
                .get(..offset)
                .map_or(0, |before| before.matches('\n').count());
            let top = breaks as f32 * empty.height;
            Rect::new(empty.x, top + empty.caret_top, 1.0, empty.caret_height)
        }
    }
}

/// The scroll offset that keeps the moving end of the selection visible:
/// the previous offset, moved as little as possible, and clamped to the
/// content.
fn scroll_offset(control: &Control, inner: &Inner, width: f32, height: f32) -> Point {
    let mut scroll = control.scroll;
    match control.kind {
        ControlKind::TextField { .. } => {
            if let Some(focus) = inner.focus {
                if focus.x < scroll.x {
                    scroll.x = focus.x;
                } else if focus.right() > scroll.x + width {
                    scroll.x = focus.right() - width;
                }
            }
            let max = (inner.width + 1.0 - width).max(0.0);
            Point::new(clamp_length(scroll.x.clamp(0.0, max)), 0.0)
        }
        ControlKind::TextArea { .. } => {
            if let Some(focus) = inner.focus {
                if focus.y < scroll.y {
                    scroll.y = focus.y;
                } else if focus.bottom() > scroll.y + height {
                    scroll.y = focus.bottom() - height;
                }
            }
            let bottom = inner
                .focus
                .map_or(inner.height, |c| c.bottom().max(inner.height));
            let max = (bottom - height).max(0.0);
            Point::new(0.0, clamp_length(scroll.y.clamp(0.0, max)))
        }
        _ => Point::default(),
    }
}

/// The ascent and descent with half-leading, and the font ascent and
/// descent, of the first available font of `style`.
fn strut(ctx: &mut LayoutContext<'_>, style: &ComputedStyle) -> (f32, f32, f32, f32) {
    let font = fonts::primary_font(ctx.fonts, style);
    let metrics = fonts::line_metrics(ctx.fonts, font, style.font_size);
    let (above, below) = inline::text_extent(style, metrics);
    (above, below, metrics.ascent, metrics.descent)
}

/// The used line height of `style`.
fn line_height(ctx: &mut LayoutContext<'_>, style: &ComputedStyle) -> f32 {
    let (above, below, _, _) = strut(ctx, style);
    above + below
}

/// The content width (or height) of a checkbox or radio button: 13 px for
/// the border box, as Chromium's theme sets.
fn checkbox_content_size(style: &ComputedStyle, horizontal: bool) -> f32 {
    if style.box_sizing == BoxSizing::ContentBox {
        return CHECKBOX_SIZE;
    }
    let edges = BoxEdges::resolve(style, 0.0).sum();
    let edges = if horizontal {
        edges.horizontal()
    } else {
        edges.vertical()
    };
    (CHECKBOX_SIZE - edges).max(0.0)
}

/// The character widths that text fields and text areas are measured in.
enum CharWidth {
    /// The average and the maximum character width.
    Average { average: f32, max: f32 },
    /// The width of `0`, for fonts without a usable average width.
    Zero(f32),
}

/// The character width of the primary font of `style` (see the module
/// documentation).
fn char_width(ctx: &mut LayoutContext<'_>, style: &ComputedStyle) -> CharWidth {
    let font = fonts::primary_font(ctx.fonts, style);
    let size = style.font_size;
    let metrics = ctx.fonts.metrics(font, size);
    let family_valid = match style.font_family.first() {
        Some(swb_style::FontFamily::Named(name)) => {
            !name.starts_with('.')
                && !INVALID_AVG_CHAR_WIDTH_FAMILIES
                    .iter()
                    .any(|f| f.eq_ignore_ascii_case(name.as_ref()))
        }
        _ => true,
    };
    if let Some(average) = metrics.avg_char_width.filter(|_| family_valid) {
        // FreeType sizes are 26.6 fixed-point numbers.
        let scale = if size > 0.0 {
            (size * 64.0).round() / 64.0 / size
        } else {
            0.0
        };
        let average = average * scale;
        return CharWidth::Average {
            average: average.round().max(average),
            max: (metrics.max_char_width * scale).round(),
        };
    }
    let options = swb_text::ShapeOptions {
        direction: swb_text::Direction::Ltr,
        language: None,
        features: &[],
    };
    let run = ctx.fonts.shape(font, size, "0", &options);
    CharWidth::Zero(run.glyphs.iter().map(|g| g.x_advance).sum())
}

/// The content width of a text field `size` characters wide.
fn text_field_width(ctx: &mut LayoutContext<'_>, style: &ComputedStyle, size: u32) -> f32 {
    let n = size.max(1) as f32;
    let width = match char_width(ctx, style) {
        CharWidth::Average { average, max } => average * n + max - average,
        CharWidth::Zero(zero) => zero * n,
    };
    ceil(width)
}

/// The content width of a text area `cols` characters wide, with room
/// for a scroll bar.
fn text_area_width(ctx: &mut LayoutContext<'_>, style: &ComputedStyle, cols: u32) -> f32 {
    let n = cols.max(1) as f32;
    let width = match char_width(ctx, style) {
        CharWidth::Average { average, .. } => average * n,
        CharWidth::Zero(zero) => zero * n,
    };
    ceil(width) + SCROLLBAR_WIDTH
}

/// Rounds up, ignoring float noise below 1/1000 px.
fn ceil(v: f32) -> f32 {
    clamp_length((v - 0.001).ceil().max(0.0))
}

/// The width of the widest option label in the select's font, rounded up.
fn options_width(ctx: &mut LayoutContext<'_>, style: &ComputedStyle, options: &[String]) -> f32 {
    let families = fonts::family_names(&style.font_family);
    let query = fonts::query(style, &families);
    let shape_options = swb_text::ShapeOptions {
        direction: swb_text::Direction::Ltr,
        language: None,
        features: &[],
    };
    let mut widest: f32 = 0.0;
    for label in options.iter().take(MAX_SELECT_OPTIONS) {
        let mut width = 0.0;
        for run in ctx.fonts.itemize(label, &query) {
            let Some(text) = label.get(run.range.clone()) else {
                continue;
            };
            let shaped = ctx
                .fonts
                .shape(run.font, style.font_size, text, &shape_options);
            width += shaped.glyphs.iter().map(|g| g.x_advance).sum::<f32>();
        }
        widest = widest.max(width);
    }
    ceil(widest)
}

/// True if the control has Chromium's native look: always for checkboxes
/// and radio buttons; for the others, if the border and the background are
/// the user-agent defaults (Chromium uses the native look unless the
/// author styles the border or the background). swb compares the computed
/// values: border styles and widths, background color and image, radius.
fn has_native_look(kind: ControlKind, style: &ComputedStyle) -> bool {
    let (border_style, border_width, backgrounds): (BorderStyle, f32, &[Rgba]) = match kind {
        ControlKind::Checkbox | ControlKind::Radio => return true,
        ControlKind::TextField { .. } => (BorderStyle::Inset, 2.0, &FIELD_BACKGROUNDS),
        ControlKind::TextArea { .. } | ControlKind::Select => {
            (BorderStyle::Solid, 1.0, &FIELD_BACKGROUNDS)
        }
        ControlKind::Button | ControlKind::ButtonElement => {
            (BorderStyle::Outset, 2.0, &BUTTON_BACKGROUNDS)
        }
    };
    let borders = [
        (style.border_top_style, style.border_top_width),
        (style.border_right_style, style.border_right_width),
        (style.border_bottom_style, style.border_bottom_width),
        (style.border_left_style, style.border_left_width),
    ];
    let radii = [
        &style.border_top_left_radius,
        &style.border_top_right_radius,
        &style.border_bottom_right_radius,
        &style.border_bottom_left_radius,
    ];
    borders
        .iter()
        .all(|&(s, w)| s == border_style && (w - border_width).abs() < 0.01)
        && radii
            .iter()
            .all(|r| r.horizontal.is_zero() && r.vertical.is_zero())
        && style.background_image.iter().all(Option::is_none)
        && style
            .background_clip
            .iter()
            .all(|c| *c == BackgroundBox::BorderBox)
        && backgrounds.contains(&style.background_color.resolve(style.color))
}

/// The user-agent backgrounds of text fields (`Field` and the disabled
/// color).
const FIELD_BACKGROUNDS: [Rgba; 2] = [Rgba::WHITE, Rgba::new(239, 239, 239, 77)];

/// The user-agent backgrounds of buttons (`ButtonFace` and the disabled
/// color).
const BUTTON_BACKGROUNDS: [Rgba; 2] = [Rgba::rgb(239, 239, 239), Rgba::new(239, 239, 239, 77)];

#[cfg(test)]
mod tests {
    use crate::test_support::layout_html;

    fn body(html: &str) -> String {
        format!("<!DOCTYPE html><body style='margin:0; font: 16px/20px sans-serif'>{html}")
    }

    #[test]
    fn text_field_widths_follow_the_average_character_width() {
        // Measured in Chromium 148 with the test fonts (content widths).
        let l = layout_html(&body(
            "<input id=a><input id=b size=1><input id=c size=17 style='font: 10pt monospace'>\
             <input id=d size=40 style='font: 20px monospace'><input id=e size=10 style='font: 13.333333px serif'>\
             <input id=f size=10 style='font: 13.333333px Times'>",
        ));
        let content = |id: &str| l.rect(id).width - 8.0;
        assert_eq!(content("a"), 177.0);
        assert_eq!(content("b"), 25.0);
        assert_eq!(content("c"), 144.0);
        assert_eq!(content("d"), 493.0);
        assert_eq!(content("e"), 90.0);
        assert_eq!(content("f"), 67.0);
        assert_eq!(l.rect("a").height, 21.0);
    }

    #[test]
    fn checkbox_baseline_is_its_bottom() {
        let l = layout_html(&body("<div id=d>x<input id=c type=checkbox></div>"));
        let c = l.rect("c");
        assert_eq!((c.width, c.height), (13.0, 13.0));
        // The line's baseline is at 16 (Chromium: the checkbox's bottom).
        assert_eq!(c.bottom(), 16.0);
        assert_eq!(l.rect("d").height, 21.0);
    }

    #[test]
    fn buttons_center_their_content() {
        let l = layout_html(&body(
            "<input id=s type=submit value=Go><button id=b style='height:50px'>x</button>",
        ));
        assert_eq!(l.rect("s").height, 21.0);
        assert_eq!(l.rect("b").height, 50.0);
        let texts = l.texts();
        let x = crate::test_support::rects_of_text(&texts, "x")[0];
        let b = l.rect("b");
        assert!((x.y + x.height / 2.0 - (b.y + 25.0)).abs() < 1.5);
    }

    #[test]
    fn controls_keep_their_width_as_blocks() {
        let l = layout_html(&body(
            "<input id=a style='display:block'><button id=b style='display:block'>b</button>",
        ));
        assert_eq!(l.rect("a").width, 185.0);
        assert!(l.rect("b").width < 100.0);
    }

    #[test]
    fn huge_sizes_give_finite_geometry() {
        let l = layout_html(&body(
            "<input id=a size=4294967295><textarea id=t cols=4294967295 rows=4294967295></textarea>",
        ));
        let a = l.rect("a");
        let t = l.rect("t");
        assert!(a.width.is_finite() && t.width.is_finite() && t.height.is_finite());
    }
}
