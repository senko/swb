//! The resolved value of each longhand (see the module documentation of
//! `resolved`): computed values in Chromium's form, and the values that
//! layout resolves from the [`UsedBox`].

use std::fmt::Write as _;
use std::sync::Arc;

use super::{BoxKind, Resolver, UsedBox, layout_px, number, px, rgba, snap};
use crate::values::{
    Alignment, BackgroundBox, BackgroundRepeatKeyword, BackgroundSize, BoxSizing, ClipPath, Color,
    CompositeOperator, ComputedCalc, Content, ContentItem, CounterList, Direction, FlexBasis,
    FontFamily, FontStyle, GenericFamily, GridLine, Image, LengthPercentage,
    LengthPercentageOrAuto, LineHeight, ListStyleType, MaskClip, MaskImage, MaskMode, MaxSize,
    Position, PositionComponent, Size, SvgPaint, TextAlign, TrackBreadth, TrackList,
    TrackListValue, TrackSize, TransformFunction, VerticalAlign, ZIndex,
};
use crate::{ComputedStyle, PseudoKind};

/// The value of the longhand `name` (canonical, lower case).
pub(super) fn value(name: &str, r: &Resolver<'_>) -> String {
    let rtl = r.style.direction == Direction::Rtl;
    let name = physical(name, rtl).unwrap_or(name);
    if let Some(used) = r.used
        && let Some(value) = used_value(name, r.style, used)
    {
        return value;
    }
    computed(name, r)
}

/// True if layout can change the value of the longhand `name`.
pub(super) fn needs_layout(name: &str) -> bool {
    let name = physical(name, false).unwrap_or(name);
    matches!(
        name,
        "width"
            | "height"
            | "min-width"
            | "min-height"
            | "transform"
            | "transform-origin"
            | "top"
            | "right"
            | "bottom"
            | "left"
    ) || side_of(name, "margin-").is_some()
        || side_of(name, "padding-").is_some()
}

/// The physical longhand of a logical one (CSS Logical 1; swb has only
/// the horizontal writing mode).
fn physical(name: &str, rtl: bool) -> Option<&'static str> {
    let (start, end) = if rtl {
        ("right", "left")
    } else {
        ("left", "right")
    };
    let side = |logical: &str| -> Option<&'static str> {
        Some(match logical {
            "block-start" => "top",
            "block-end" => "bottom",
            "inline-start" => start,
            "inline-end" => end,
            _ => return None,
        })
    };
    if let Some(rest) = name.strip_prefix("margin-") {
        return Some(match side(rest)? {
            "top" => "margin-top",
            "bottom" => "margin-bottom",
            "left" => "margin-left",
            _ => "margin-right",
        });
    }
    if let Some(rest) = name.strip_prefix("padding-") {
        return Some(match side(rest)? {
            "top" => "padding-top",
            "bottom" => "padding-bottom",
            "left" => "padding-left",
            _ => "padding-right",
        });
    }
    if let Some(rest) = name.strip_prefix("inset-") {
        return side(rest);
    }
    if let Some(rest) = name.strip_prefix("border-")
        && let Some((logical, part)) = rest.rsplit_once('-')
        && let Some(physical) = side(logical)
    {
        return Some(match (physical, part) {
            ("top", "width") => "border-top-width",
            ("top", "style") => "border-top-style",
            ("top", "color") => "border-top-color",
            ("bottom", "width") => "border-bottom-width",
            ("bottom", "style") => "border-bottom-style",
            ("bottom", "color") => "border-bottom-color",
            ("left", "width") => "border-left-width",
            ("left", "style") => "border-left-style",
            ("left", "color") => "border-left-color",
            ("right", "width") => "border-right-width",
            ("right", "style") => "border-right-style",
            _ => "border-right-color",
        });
    }
    let (start_corner, end_corner) = if rtl {
        ("right", "left")
    } else {
        ("left", "right")
    };
    Some(match name {
        "inline-size" => "width",
        "block-size" => "height",
        "min-inline-size" => "min-width",
        "min-block-size" => "min-height",
        "max-inline-size" => "max-width",
        "max-block-size" => "max-height",
        "contain-intrinsic-inline-size" => "contain-intrinsic-width",
        "contain-intrinsic-block-size" => "contain-intrinsic-height",
        "overflow-inline" => "overflow-x",
        "overflow-block" => "overflow-y",
        "border-start-start-radius" => corner("top", start_corner),
        "border-start-end-radius" => corner("top", end_corner),
        "border-end-start-radius" => corner("bottom", start_corner),
        "border-end-end-radius" => corner("bottom", end_corner),
        _ => return None,
    })
}

fn corner(vertical: &str, horizontal: &str) -> &'static str {
    match (vertical, horizontal) {
        ("top", "left") => "border-top-left-radius",
        ("top", _) => "border-top-right-radius",
        (_, "left") => "border-bottom-left-radius",
        _ => "border-bottom-right-radius",
    }
}

/// The index of the side of `name` after `prefix` (`margin-top` is 0):
/// top, right, bottom, left.
fn side_of(name: &str, prefix: &str) -> Option<usize> {
    match name.strip_prefix(prefix)? {
        "top" => Some(0),
        "right" => Some(1),
        "bottom" => Some(2),
        "left" => Some(3),
        _ => None,
    }
}

// ----- Values from layout -----

/// The value of `name` that layout resolves, if it does for this box.
fn used_value(name: &str, style: &ComputedStyle, used: &UsedBox) -> Option<String> {
    let inline = used.kind == BoxKind::Inline;
    match name {
        "width" | "height" if !inline => Some(layout_px(used_size(style, used, name == "width"))),
        "min-width" | "min-height" => {
            let value = if name == "min-width" {
                &style.min_width
            } else {
                &style.min_height
            };
            let item = matches!(used.kind, BoxKind::FlexItem { .. } | BoxKind::GridItem);
            value
                .is_auto()
                .then(|| if item { "auto" } else { "0px" }.to_owned())
        }
        "transform" => Some(transform(style, used)),
        "transform-origin" => {
            let (w, h) = reference_box(used);
            let origin = &style.transform_origin;
            Some(format!(
                "{} {}",
                px(origin.x.resolve(w)),
                px(origin.y.resolve(h))
            ))
        }
        "top" | "right" | "bottom" | "left" => inset(name, style, used),
        _ => {
            if inline {
                return None;
            }
            if let Some(side) = side_of(name, "margin-") {
                if margins(style)
                    .get(side)
                    .and_then(|m| m.non_auto())
                    .is_some_and(fixed)
                {
                    return None;
                }
                return Some(layout_px(margin(style, used, side).0));
            }
            let side = side_of(name, "padding-")?;
            if paddings(style).get(side).is_some_and(|p| fixed(p)) {
                return None;
            }
            used.padding.get(side).map(|&p| layout_px(p))
        }
    }
}

/// The used `width` (or `height`): the content box, or the border box
/// with `box-sizing: border-box`.
fn used_size(style: &ComputedStyle, used: &UsedBox, horizontal: bool) -> f32 {
    let [top, right, bottom, left] = used.padding;
    let [b_top, b_right, b_bottom, b_left] = used.border;
    let (outer, extra) = if horizontal {
        (used.width, left + right + b_left + b_right)
    } else {
        (used.height, top + bottom + b_top + b_bottom)
    };
    match style.box_sizing {
        BoxSizing::BorderBox => outer,
        BoxSizing::ContentBox => (outer - extra).max(0.0),
    }
}

/// The margins of the sides of a box: top, right, bottom, left.
fn margins(style: &ComputedStyle) -> [&LengthPercentageOrAuto; 4] {
    [
        &style.margin_top,
        &style.margin_right,
        &style.margin_bottom,
        &style.margin_left,
    ]
}

/// The insets of the sides of a box: top, right, bottom, left.
fn insets(style: &ComputedStyle) -> [&LengthPercentageOrAuto; 4] {
    [&style.top, &style.right, &style.bottom, &style.left]
}

/// The paddings of the sides of a box: top, right, bottom, left.
fn paddings(style: &ComputedStyle) -> [&LengthPercentage; 4] {
    [
        &style.padding_top,
        &style.padding_right,
        &style.padding_bottom,
        &style.padding_left,
    ]
}

/// True if `lp` has no percentage. For a margin, padding or inset of such
/// a length, Chromium gives the computed value as it is, not snapped to
/// layout units (measured: `padding: 0.4em` at 16px is `6.4px`, `left:
/// 5.3px` is `5.3px`); `width` and `height` are snapped always.
fn fixed(lp: &LengthPercentage) -> bool {
    !lp.has_percentage()
}

/// The used margin of side `side` (top, right, bottom, left): in layout
/// units, and as layout computed it. Percentages refer to the width of
/// the containing block; `auto` comes from the position of the box.
fn margin(style: &ComputedStyle, used: &UsedBox, side: usize) -> (f32, f32) {
    let all = margins(style);
    if let Some(lp) = all.get(side).and_then(|m| m.non_auto()) {
        let raw = lp.resolve(used.cb_width);
        return (snap(f64::from(raw)) as f32, raw);
    }
    let horizontal = side % 2 == 1;
    let start = side == 3 || side == 0;
    // The space between the border box and the containing block's edge.
    let space = |size: f32, pos: f32, extent: f32| {
        if start { pos } else { size - pos - extent }
    };
    let value = match used.kind {
        BoxKind::Block if horizontal => space(used.cb_width, used.x, used.width),
        BoxKind::FlexItem { row } if row != horizontal => {
            if horizontal {
                space(used.cb_width, used.x, used.width)
            } else {
                space(used.cb_height, used.y, used.height)
            }
        }
        BoxKind::Absolute => {
            let inset = insets(style)
                .get(side)
                .and_then(|i| i.non_auto())
                .map(|lp| {
                    lp.resolve(if horizontal {
                        used.cb_width
                    } else {
                        used.cb_height
                    })
                });
            match inset {
                Some(inset) if horizontal && start => used.x - inset,
                Some(inset) if horizontal => used.cb_width - inset - used.x - used.width,
                Some(inset) if start => used.y - inset,
                Some(inset) => used.cb_height - inset - used.y - used.height,
                None => 0.0,
            }
        }
        _ => 0.0,
    };
    (snap(f64::from(value)) as f32, value)
}

/// The resolved value of an inset (CSSOM §9: the used value for
/// positioned boxes).
fn inset(name: &str, style: &ComputedStyle, used: &UsedBox) -> Option<String> {
    let side = match name {
        "top" => 0,
        "right" => 1,
        "bottom" => 2,
        _ => 3,
    };
    let opposite = (side + 2) % 4;
    let horizontal = side % 2 == 1;
    let all = insets(style);
    let this = all.get(side).and_then(|i| i.non_auto());
    let other = all.get(opposite).and_then(|i| i.non_auto());
    if style.position != Position::Static && this.is_some_and(fixed) {
        return None;
    }
    match style.position {
        Position::Static => None,
        Position::Sticky => {
            let basis = if horizontal {
                used.scrollport.0
            } else {
                used.scrollport.1
            };
            this.map(|lp| layout_px(lp.resolve(basis)))
        }
        Position::Relative => {
            let basis = if horizontal {
                used.cb_width
            } else {
                used.cb_height
            };
            Some(match (this, other) {
                (Some(lp), _) => layout_px(lp.resolve(basis)),
                // Chromium negates the other side without layout units.
                (None, Some(lp)) => px(-lp.resolve(basis)),
                (None, None) => "0px".to_owned(),
            })
        }
        Position::Absolute | Position::Fixed => {
            let (cb, size, pos) = if horizontal {
                (used.cb_width, used.width, used.x)
            } else {
                (used.cb_height, used.height, used.y)
            };
            // The start side (left or top) and the end side, in layout
            // units as Chromium computes them: a specified side resolved,
            // an `auto` side as the rest of the containing block, both
            // `auto` from the static position.
            let (start_side, end_side) = if horizontal { (3, 1) } else { (0, 2) };
            let resolve = |side: usize| {
                all.get(side)
                    .and_then(|i| i.non_auto())
                    .map(|lp| snap(f64::from(lp.resolve(cb))))
            };
            let (m_start, m_start_raw) = margin(style, used, start_side);
            let (m_end, _) = margin(style, used, end_side);
            let rest = |other: f64| {
                snap(f64::from(cb))
                    - other
                    - snap(f64::from(size))
                    - f64::from(m_start)
                    - f64::from(m_end)
            };
            let start = match (resolve(start_side), resolve(end_side)) {
                (Some(start), _) => start,
                (None, Some(end)) => rest(end),
                (None, None) => snap(f64::from(pos - m_start_raw)),
            };
            let value = if side == start_side {
                start
            } else {
                resolve(end_side).unwrap_or_else(|| rest(start))
            };
            Some(format!("{}px", super::super::value::format_number(value)))
        }
    }
}

/// The reference box of transforms: the border box (zero for a part of an
/// inline box, which transforms do not apply to).
fn reference_box(used: &UsedBox) -> (f32, f32) {
    if used.kind == BoxKind::Inline {
        (0.0, 0.0)
    } else {
        (used.width, used.height)
    }
}

/// `transform` as Chromium resolves it: `none`, or the product of the
/// functions as `matrix()`.
fn transform(style: &ComputedStyle, used: &UsedBox) -> String {
    if style.transform.is_empty() {
        return "none".to_owned();
    }
    let (w, h) = reference_box(used);
    let mut m = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    for f in style.transform.iter() {
        m = multiply(m, function_matrix(f, w, h));
    }
    let parts: Vec<String> = m
        .iter()
        .map(|&v| super::super::value::format_number(v))
        .collect();
    format!("matrix({})", parts.join(", "))
}

/// The matrix `[a, b, c, d, e, f]` of a transform function.
fn function_matrix(function: &TransformFunction, width: f32, height: f32) -> [f64; 6] {
    match function {
        TransformFunction::Matrix(v) => v.map(f64::from),
        TransformFunction::Translate(tx, ty) => [
            1.0,
            0.0,
            0.0,
            1.0,
            f64::from(tx.resolve(width)),
            f64::from(ty.resolve(height)),
        ],
        TransformFunction::Scale(sx, sy) => [f64::from(*sx), 0.0, 0.0, f64::from(*sy), 0.0, 0.0],
        TransformFunction::Rotate(deg) => {
            let (sin, cos) = sin_cos(f64::from(*deg));
            [cos, sin, -sin, cos, 0.0, 0.0]
        }
        TransformFunction::Skew(ax, ay) => [
            1.0,
            f64::from(*ay).to_radians().tan(),
            f64::from(*ax).to_radians().tan(),
            1.0,
            0.0,
            0.0,
        ],
    }
}

/// The sine and cosine of an angle in degrees, exact for multiples of 90
/// degrees (as in Chromium: `rotate(0.5turn)` is `matrix(-1, 0, 0, -1, 0,
/// 0)`).
fn sin_cos(deg: f64) -> (f64, f64) {
    let quarter = deg / 90.0;
    if quarter.fract() == 0.0 {
        return match quarter.rem_euclid(4.0) as u8 {
            0 => (0.0, 1.0),
            1 => (1.0, 0.0),
            2 => (0.0, -1.0),
            _ => (-1.0, 0.0),
        };
    }
    deg.to_radians().sin_cos()
}

/// `a × b` for 2D matrices `[a, b, c, d, e, f]`.
fn multiply(a: [f64; 6], b: [f64; 6]) -> [f64; 6] {
    [
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
        a[0] * b[4] + a[2] * b[5] + a[4],
        a[1] * b[4] + a[3] * b[5] + a[5],
    ]
}

// ----- Computed values -----

/// The computed value of `name`, serialized.
#[allow(clippy::too_many_lines)] // One line per property.
fn computed(name: &str, r: &Resolver<'_>) -> String {
    let s = r.style;
    let color = |c: Color| rgba(c.resolve(s.color));
    match name {
        "color" => rgba(s.color),
        "font-family" => font_family(s),
        "font-size" => px(s.font_size),
        "font-weight" => number(s.font_weight),
        "font-style" => match s.font_style {
            FontStyle::Normal => "normal",
            // Chromium writes `oblique` without an angle as `italic`.
            FontStyle::Italic | FontStyle::Oblique => "italic",
        }
        .to_owned(),
        "font-stretch" => format!("{}%", number(s.font_stretch)),
        "font-variant-caps" => s.font_variant_caps.as_str().to_owned(),
        "font-variation-settings" => settings(
            s.font_variation_settings
                .iter()
                .map(|(t, v)| (t, number(*v))),
        ),
        "font-feature-settings" => settings(s.font_feature_settings.iter().map(|(t, v)| {
            (
                t,
                if *v == 1 {
                    String::new()
                } else {
                    v.to_string()
                },
            )
        })),
        "line-height" => match s.line_height {
            LineHeight::Normal => "normal".to_owned(),
            LineHeight::Number(n) => px(n * s.font_size),
            LineHeight::Px(v) => px(v),
        },
        "text-align" => match s.text_align {
            TextAlign::MatchParent => "start",
            other => other.as_str(),
        }
        .to_owned(),
        "text-indent" => lp(&s.text_indent),
        "text-transform" => s.text_transform.as_str().to_owned(),
        "white-space" => s.white_space.as_str().to_owned(),
        // Chromium computes `letter-spacing: 0` to `normal`.
        "letter-spacing" if s.letter_spacing == 0.0 => "normal".to_owned(),
        "letter-spacing" => px(s.letter_spacing),
        "word-spacing" => px(s.word_spacing),
        "word-break" => s.word_break.as_str().to_owned(),
        "overflow-wrap" => s.overflow_wrap.as_str().to_owned(),
        "hyphens" => s.hyphens.as_str().to_owned(),
        "tab-size" => number(s.tab_size),
        "visibility" => s.visibility.as_str().to_owned(),
        "list-style-type" => s.list_style_type.as_str().to_owned(),
        "list-style-position" => s.list_style_position.as_str().to_owned(),
        "list-style-image" => s
            .list_style_image
            .as_ref()
            .map_or_else(|| "none".to_owned(), image),
        "cursor" => s.cursor.as_str().to_owned(),
        "direction" => s.direction.as_str().to_owned(),
        "border-collapse" => s.border_collapse.as_str().to_owned(),
        "-webkit-border-horizontal-spacing" => px(s.border_spacing_horizontal),
        "-webkit-border-vertical-spacing" => px(s.border_spacing_vertical),
        "caption-side" => s.caption_side.as_str().to_owned(),
        "empty-cells" => s.empty_cells.as_str().to_owned(),
        "pointer-events" => s.pointer_events.as_str().to_owned(),
        "fill" => paint(&s.fill, s),
        "fill-rule" => s.fill_rule.as_str().to_owned(),
        "clip-rule" => s.clip_rule.as_str().to_owned(),
        "shape-rendering" => s.shape_rendering.as_str().to_owned(),
        "fill-opacity" => number(s.fill_opacity),
        "stroke" => paint(&s.stroke, s),
        "stroke-width" => lp(&s.stroke_width),
        "stroke-linecap" => s.stroke_linecap.as_str().to_owned(),
        "stroke-linejoin" => s.stroke_linejoin.as_str().to_owned(),
        "stroke-miterlimit" => number(s.stroke_miterlimit),
        "stroke-dasharray" if s.stroke_dasharray.is_empty() => "none".to_owned(),
        "stroke-dasharray" => join(s.stroke_dasharray.iter().map(lp), ", "),
        "stroke-dashoffset" => lp(&s.stroke_dashoffset),
        "stroke-opacity" => number(s.stroke_opacity),
        "display" => s.display.as_str().to_owned(),
        "position" => s.position.as_str().to_owned(),
        "float" => s.float.as_str().to_owned(),
        "clear" => s.clear.as_str().to_owned(),
        "top" => lp_auto(&s.top),
        "right" => lp_auto(&s.right),
        "bottom" => lp_auto(&s.bottom),
        "left" => lp_auto(&s.left),
        "z-index" => match s.z_index {
            ZIndex::Auto => "auto".to_owned(),
            ZIndex::Integer(i) => i.to_string(),
        },
        // Without a box Chromium gives `none`.
        "transform" | "border-image-source" => "none".to_owned(),
        "transform-origin" => format!(
            "{} {}",
            lp(&s.transform_origin.x),
            lp(&s.transform_origin.y)
        ),
        "clip" => match s.clip {
            None => "auto".to_owned(),
            Some(c) => {
                let side = |v: Option<f32>| v.map_or_else(|| "auto".to_owned(), px);
                format!(
                    "rect({}, {}, {}, {})",
                    side(c.top),
                    side(c.right),
                    side(c.bottom),
                    side(c.left)
                )
            }
        },
        "clip-path" => match &s.clip_path {
            ClipPath::None => "none".to_owned(),
            ClipPath::Url(u) => url(u),
        },
        "width" => size(&s.width),
        "height" => size(&s.height),
        // `auto` is 0 except for flex and grid items (which have a box).
        "min-width" | "min-height" => {
            let value = if name == "min-width" {
                &s.min_width
            } else {
                &s.min_height
            };
            if value.is_auto() {
                "0px".to_owned()
            } else {
                size(value)
            }
        }
        "max-width" => max_size(&s.max_width),
        "max-height" => max_size(&s.max_height),
        "box-sizing" => s.box_sizing.as_str().to_owned(),
        "aspect-ratio" => {
            let ratio = s.aspect_ratio.ratio.map(|v| format!("{} / 1", number(v)));
            match (s.aspect_ratio.auto, ratio) {
                (true, Some(ratio)) => format!("auto {ratio}"),
                (_, Some(ratio)) => ratio,
                (_, None) => "auto".to_owned(),
            }
        }
        "margin-top" => lp_auto(&s.margin_top),
        "margin-right" => lp_auto(&s.margin_right),
        "margin-bottom" => lp_auto(&s.margin_bottom),
        "margin-left" => lp_auto(&s.margin_left),
        "padding-top" => lp(&s.padding_top),
        "padding-right" => lp(&s.padding_right),
        "padding-bottom" => lp(&s.padding_bottom),
        "padding-left" => lp(&s.padding_left),
        "border-top-width" => px(s.border_top_width),
        "border-right-width" => px(s.border_right_width),
        "border-bottom-width" => px(s.border_bottom_width),
        "border-left-width" => px(s.border_left_width),
        "border-top-style" => s.border_top_style.as_str().to_owned(),
        "border-right-style" => s.border_right_style.as_str().to_owned(),
        "border-bottom-style" => s.border_bottom_style.as_str().to_owned(),
        "border-left-style" => s.border_left_style.as_str().to_owned(),
        "border-top-color" => color(s.border_top_color),
        "border-right-color" => color(s.border_right_color),
        "border-bottom-color" => color(s.border_bottom_color),
        "border-left-color" => color(s.border_left_color),
        "border-top-left-radius" => radius(&s.border_top_left_radius),
        "border-top-right-radius" => radius(&s.border_top_right_radius),
        "border-bottom-right-radius" => radius(&s.border_bottom_right_radius),
        "border-bottom-left-radius" => radius(&s.border_bottom_left_radius),
        "outline-width" => px(s.outline_width),
        "outline-style" => s.outline_style.as_str().to_owned(),
        "outline-color" => color(s.outline_color),
        "outline-offset" => px(s.outline_offset),
        "background-color" => color(s.background_color),
        "background-image" => join(
            s.background_image
                .iter()
                .map(|i| i.as_ref().map_or_else(|| "none".to_owned(), image)),
            ", ",
        ),
        "background-position-x" => {
            layers(s.background_image.len(), &s.background_position_x, position)
        }
        "background-position-y" => {
            layers(s.background_image.len(), &s.background_position_y, position)
        }
        "background-size" => layers(s.background_image.len(), &s.background_size, bg_size),
        "background-repeat" => layers(s.background_image.len(), &s.background_repeat, |r| {
            repeat(*r)
        }),
        "background-origin" => layers(s.background_image.len(), &s.background_origin, |b| {
            b.as_str().to_owned()
        }),
        "background-clip" => layers(s.background_image.len(), &s.background_clip, |b| {
            background_box(*b)
        }),
        "background-attachment" => {
            layers(s.background_image.len(), &s.background_attachment, |a| {
                a.as_str().to_owned()
            })
        }
        "mask-image" => join(s.mask_image.iter().map(mask_image), ", "),
        "mask-mode" => layers(s.mask_image.len(), &s.mask_mode, |m| {
            match m {
                MaskMode::MatchSource => "match-source",
                MaskMode::Alpha => "alpha",
                MaskMode::Luminance => "luminance",
            }
            .to_owned()
        }),
        "-webkit-mask-position-x" => layers(s.mask_image.len(), &s.mask_position_x, position),
        "-webkit-mask-position-y" => layers(s.mask_image.len(), &s.mask_position_y, position),
        "mask-size" => layers(s.mask_image.len(), &s.mask_size, bg_size),
        "mask-repeat" => layers(s.mask_image.len(), &s.mask_repeat, |r| repeat(*r)),
        "mask-origin" => layers(s.mask_image.len(), &s.mask_origin, |b| {
            b.as_str().to_owned()
        }),
        "mask-clip" => layers(s.mask_image.len(), &s.mask_clip, |c| {
            match c {
                MaskClip::BorderBox => "border-box",
                MaskClip::PaddingBox => "padding-box",
                MaskClip::ContentBox => "content-box",
                MaskClip::NoClip => "no-clip",
            }
            .to_owned()
        }),
        "mask-composite" => layers(s.mask_image.len(), &s.mask_composite, |c| {
            composite(*c).to_owned()
        }),
        "overflow-x" => s.overflow_x.as_str().to_owned(),
        "overflow-y" => s.overflow_y.as_str().to_owned(),
        "text-overflow" => s.text_overflow.as_str().to_owned(),
        "opacity" => number(s.opacity),
        "vertical-align" => match &s.vertical_align {
            VerticalAlign::Keyword(k) => k.as_str().to_owned(),
            VerticalAlign::LengthPercentage(v) => lp(v),
        },
        "text-decoration-line" => {
            let line = s.text_decoration_line;
            let names: Vec<&str> = [
                (crate::values::TextDecorationLine::UNDERLINE, "underline"),
                (crate::values::TextDecorationLine::OVERLINE, "overline"),
                (
                    crate::values::TextDecorationLine::LINE_THROUGH,
                    "line-through",
                ),
            ]
            .into_iter()
            .filter(|(flag, _)| line.contains(*flag))
            .map(|(_, name)| name)
            .collect();
            if names.is_empty() {
                "none".to_owned()
            } else {
                names.join(" ")
            }
        }
        "text-decoration-color" => color(s.text_decoration_color),
        "text-decoration-style" => s.text_decoration_style.as_str().to_owned(),
        "flex-direction" => s.flex_direction.as_str().to_owned(),
        "flex-wrap" => s.flex_wrap.as_str().to_owned(),
        "flex-grow" => number(s.flex_grow),
        "flex-shrink" => number(s.flex_shrink),
        "flex-basis" => match &s.flex_basis {
            FlexBasis::Content => "content".to_owned(),
            FlexBasis::Size(v) => size(v),
        },
        "order" => s.order.to_string(),
        "justify-content" => alignment(s.justify_content),
        "align-items" => alignment(s.align_items),
        "align-self" => alignment(s.align_self),
        "align-content" => alignment(s.align_content),
        "justify-items" => alignment(s.justify_items),
        "justify-self" => alignment(s.justify_self),
        "row-gap" => gap(&s.row_gap),
        "column-gap" => gap(&s.column_gap),
        "column-width" => s.column_width.map_or_else(|| "auto".to_owned(), px),
        "column-count" => s
            .column_count
            .map_or_else(|| "auto".to_owned(), |n| n.to_string()),
        "grid-template-columns" => track_list(&s.grid_template_columns),
        "grid-template-rows" => track_list(&s.grid_template_rows),
        "grid-template-areas" => grid_areas(s),
        "grid-auto-columns" => join(s.grid_auto_columns.iter().map(track_size), " "),
        "grid-auto-rows" => join(s.grid_auto_rows.iter().map(track_size), " "),
        "grid-auto-flow" => match (s.grid_auto_flow.column, s.grid_auto_flow.dense) {
            (false, false) => "row",
            (false, true) => "dense",
            (true, false) => "column",
            (true, true) => "column dense",
        }
        .to_owned(),
        "grid-row-start" => grid_line(&s.grid_row_start),
        "grid-row-end" => grid_line(&s.grid_row_end),
        "grid-column-start" => grid_line(&s.grid_column_start),
        "grid-column-end" => grid_line(&s.grid_column_end),
        "table-layout" => s.table_layout.as_str().to_owned(),
        "content" => content(&s.content, r.pseudo),
        "counter-reset" => counters(&s.counter_reset),
        "counter-increment" => counters(&s.counter_increment),
        "counter-set" => counters(&s.counter_set),
        "contain" => contain(s),
        "contain-intrinsic-width" => contain_intrinsic(s.contain_intrinsic_width),
        "contain-intrinsic-height" => contain_intrinsic(s.contain_intrinsic_height),
        "object-fit" => s.object_fit.as_str().to_owned(),
        "scrollbar-width" => s.scrollbar_width.as_str().to_owned(),
        "object-position" => format!(
            "{} {}",
            position(&s.object_position[0]),
            position(&s.object_position[1])
        ),
        "user-select" => s.user_select.as_str().to_owned(),
        "unicode-bidi" => s.unicode_bidi.as_str().to_owned(),
        // Properties that swb accepts and does not compute: their initial
        // values, as Chromium gives them.
        "text-decoration-thickness"
        | "text-underline-offset"
        | "text-underline-position"
        | "text-decoration-skip-ink" => "auto".to_owned(),
        "mask-type" => "luminance".to_owned(),
        "border-image-slice" => "100%".to_owned(),
        "border-image-width" => "1".to_owned(),
        "border-image-outset" => "0".to_owned(),
        "border-image-repeat" => "stretch".to_owned(),
        _ => String::new(),
    }
}

/// A computed length or percentage.
pub(super) fn lp(v: &LengthPercentage) -> String {
    match v {
        LengthPercentage::Px(v) => px(*v),
        LengthPercentage::Percent(p) => percent(*p),
        LengthPercentage::Calc(c) => calc(c),
    }
}

fn percent(fraction: f32) -> String {
    format!("{}%", number(fraction * 100.0))
}

/// A `calc()` with percentages: `calc(P% + Npx)` when it is linear in the
/// percentage basis (Chromium's form), else its terms.
fn calc(c: &ComputedCalc) -> String {
    let at = |basis: f32| c.resolve(basis);
    let fixed = at(0.0);
    let per_cent = at(100.0) - fixed;
    if (at(200.0) - at(100.0) - per_cent).abs() <= 1e-3 {
        return match (per_cent == 0.0, fixed == 0.0) {
            (true, _) => px(fixed),
            (false, true) => format!("{}%", number(per_cent)),
            (false, false) => {
                let sign = if fixed < 0.0 { '-' } else { '+' };
                format!("calc({}% {sign} {})", number(per_cent), px(fixed.abs()))
            }
        };
    }
    format!("calc({})", calc_terms(c))
}

fn calc_terms(c: &ComputedCalc) -> String {
    match c {
        ComputedCalc::Px(v) => px(*v),
        ComputedCalc::Percent(p) => percent(*p),
        ComputedCalc::Number(n) => number(*n),
        ComputedCalc::Sum(terms) => join(terms.iter().map(calc_terms), " + "),
        ComputedCalc::Product(a, b) => format!("{} * {}", calc_terms(a), calc_terms(b)),
        ComputedCalc::Quotient(a, b) => format!("{} / {}", calc_terms(a), calc_terms(b)),
        ComputedCalc::Min(terms) => format!("min({})", join(terms.iter().map(calc_terms), ", ")),
        ComputedCalc::Max(terms) => format!("max({})", join(terms.iter().map(calc_terms), ", ")),
        ComputedCalc::Clamp(a, b, c) => format!(
            "clamp({}, {}, {})",
            calc_terms(a),
            calc_terms(b),
            calc_terms(c)
        ),
    }
}

pub(super) fn lp_auto(v: &LengthPercentageOrAuto) -> String {
    match v {
        LengthPercentageOrAuto::Auto => "auto".to_owned(),
        LengthPercentageOrAuto::LengthPercentage(v) => lp(v),
    }
}

fn size(v: &Size) -> String {
    match v {
        Size::Auto => "auto".to_owned(),
        Size::LengthPercentage(v) => lp(v),
        Size::MinContent => "min-content".to_owned(),
        Size::MaxContent => "max-content".to_owned(),
        Size::FitContent(None) => "fit-content".to_owned(),
        Size::FitContent(Some(v)) => format!("fit-content({})", lp(v)),
    }
}

fn max_size(v: &MaxSize) -> String {
    match v {
        MaxSize::None => "none".to_owned(),
        MaxSize::LengthPercentage(v) => lp(v),
        MaxSize::MinContent => "min-content".to_owned(),
        MaxSize::MaxContent => "max-content".to_owned(),
        MaxSize::FitContent(None) => "fit-content".to_owned(),
        MaxSize::FitContent(Some(v)) => format!("fit-content({})", lp(v)),
    }
}

fn gap(v: &crate::values::Gap) -> String {
    match v {
        crate::values::Gap::Normal => "normal".to_owned(),
        crate::values::Gap::LengthPercentage(v) => lp(v),
    }
}

fn radius(v: &crate::values::CornerRadius) -> String {
    if v.horizontal == v.vertical {
        lp(&v.horizontal)
    } else {
        format!("{} {}", lp(&v.horizontal), lp(&v.vertical))
    }
}

fn join(items: impl Iterator<Item = String>, separator: &str) -> String {
    items.collect::<Vec<_>>().join(separator)
}

/// A list property of the background or mask layers: one value per layer
/// of the images (Chromium repeats or cuts the list to that count).
fn layers<T>(count: usize, list: &Arc<[T]>, f: impl Fn(&T) -> String) -> String {
    let count = count.max(1);
    join(
        (0..count).filter_map(|i| list.get(i % list.len().max(1)).map(&f)),
        ", ",
    )
}

/// A `background-position-x` (or `y`) component: an offset from the
/// start, or `calc(100% - offset)` from the end.
pub(super) fn position(p: &PositionComponent) -> String {
    if !p.from_end {
        return lp(&p.offset);
    }
    match &p.offset {
        LengthPercentage::Percent(f) => percent(1.0 - f),
        LengthPercentage::Px(v) if *v == 0.0 => "100%".to_owned(),
        LengthPercentage::Px(v) => format!("calc(100% - {})", px(*v)),
        other @ LengthPercentage::Calc(_) => format!("calc(100% - {})", lp(other)),
    }
}

pub(super) fn bg_size(v: &BackgroundSize) -> String {
    match v {
        BackgroundSize::Cover => "cover".to_owned(),
        BackgroundSize::Contain => "contain".to_owned(),
        BackgroundSize::Auto => "auto".to_owned(),
        BackgroundSize::Explicit(w, LengthPercentageOrAuto::Auto) => lp_auto(w),
        BackgroundSize::Explicit(w, h) => format!("{} {}", lp_auto(w), lp_auto(h)),
    }
}

pub(super) fn repeat((x, y): (BackgroundRepeatKeyword, BackgroundRepeatKeyword)) -> String {
    use BackgroundRepeatKeyword::{NoRepeat, Repeat};
    match (x, y) {
        (Repeat, NoRepeat) => "repeat-x".to_owned(),
        (NoRepeat, Repeat) => "repeat-y".to_owned(),
        (x, y) if x == y => x.as_str().to_owned(),
        (x, y) => format!("{} {}", x.as_str(), y.as_str()),
    }
}

pub(super) fn background_box(b: BackgroundBox) -> String {
    b.as_str().to_owned()
}

pub(super) fn composite(c: CompositeOperator) -> &'static str {
    match c {
        CompositeOperator::SourceOut => "subtract",
        CompositeOperator::SourceIn => "intersect",
        CompositeOperator::Xor => "exclude",
        CompositeOperator::Clear => "clear",
        CompositeOperator::Copy => "copy",
        CompositeOperator::SourceAtop => "source-atop",
        CompositeOperator::DestinationOver => "destination-over",
        // `add` (source-over) and the operators that only the
        // `-webkit-` syntax has.
        _ => "add",
    }
}

pub(super) fn mask_image(m: &MaskImage) -> String {
    match m {
        MaskImage::Image(i) => image(i),
        MaskImage::None | MaskImage::Failed | MaskImage::Unsupported => "none".to_owned(),
    }
}

/// `url("...")`.
pub(super) fn url(u: &str) -> String {
    let mut out = String::from("url(");
    swb_css::serialize_string(u, &mut out);
    out.push(')');
    out
}

pub(super) fn image(i: &Image) -> String {
    match i {
        Image::Url(u) => url(u),
        Image::LinearGradient(g) => {
            let mut out = String::from(if g.repeating {
                "repeating-linear-gradient("
            } else {
                "linear-gradient("
            });
            let direction = match g.angle_deg {
                180.0 => None,
                0.0 => Some("to top".to_owned()),
                90.0 => Some("to right".to_owned()),
                270.0 => Some("to left".to_owned()),
                a => Some(format!("{}deg", number(a))),
            };
            let mut parts: Vec<String> = direction.into_iter().collect();
            for (color, stop) in &g.stops {
                let color = match color {
                    Color::Rgba(c) => rgba(*c),
                    Color::CurrentColor => "currentcolor".to_owned(),
                };
                parts.push(match stop {
                    Some(stop) => format!("{color} {}", lp(stop)),
                    None => color,
                });
            }
            out.push_str(&parts.join(", "));
            out.push(')');
            out
        }
    }
}

fn paint(p: &SvgPaint, s: &ComputedStyle) -> String {
    match p {
        SvgPaint::None => "none".to_owned(),
        SvgPaint::Color(c) => rgba(c.resolve(s.color)),
        SvgPaint::Url { url: u, fallback } => match fallback {
            Some(c) => format!("{} {}", url(u), rgba(c.resolve(s.color))),
            None => url(u),
        },
    }
}

fn alignment(a: Alignment) -> String {
    a.as_str().to_owned()
}

/// `font-feature-settings` and `font-variation-settings`: `normal`, or
/// `"tag" value` pairs (the value left out where it is empty).
fn settings<'a>(items: impl Iterator<Item = (&'a [u8; 4], String)>) -> String {
    let parts: Vec<String> = items
        .map(|(tag, value)| {
            let mut out = String::new();
            swb_css::serialize_string(&String::from_utf8_lossy(tag), &mut out);
            if !value.is_empty() {
                out.push(' ');
                out.push_str(&value);
            }
            out
        })
        .collect();
    if parts.is_empty() {
        "normal".to_owned()
    } else {
        parts.join(", ")
    }
}

/// `font-family`. The initial value is Chromium's standard font, `"Times
/// New Roman"` (an explicit `serif` stays `serif`).
pub(super) fn font_family(s: &ComputedStyle) -> String {
    if Arc::ptr_eq(&s.font_family, &ComputedStyle::initial().font_family) {
        return "\"Times New Roman\"".to_owned();
    }
    join(
        s.font_family.iter().map(|f| match f {
            FontFamily::Generic(g) => generic_name(*g).to_owned(),
            FontFamily::Named(name) => {
                if super::super::longhand::is_plain_identifier(name)
                    && GenericFamily::from_ident(name).is_none()
                {
                    name.to_string()
                } else {
                    let mut out = String::new();
                    swb_css::serialize_string(name, &mut out);
                    out
                }
            }
        }),
        ", ",
    )
}

fn generic_name(g: GenericFamily) -> &'static str {
    match g {
        GenericFamily::Serif => "serif",
        GenericFamily::SansSerif => "sans-serif",
        GenericFamily::Monospace => "monospace",
        GenericFamily::Cursive => "cursive",
        GenericFamily::Fantasy => "fantasy",
        GenericFamily::SystemUi => "system-ui",
        GenericFamily::Math => "math",
        GenericFamily::Emoji => "emoji",
    }
}

fn track_breadth(b: &TrackBreadth) -> String {
    match b {
        TrackBreadth::Length(v) => lp(v),
        TrackBreadth::Flex(v) => format!("{}fr", number(*v)),
        TrackBreadth::MinContent => "min-content".to_owned(),
        TrackBreadth::MaxContent => "max-content".to_owned(),
        TrackBreadth::Auto => "auto".to_owned(),
    }
}

pub(super) fn track_size(t: &TrackSize) -> String {
    match t {
        TrackSize::Breadth(b) => track_breadth(b),
        TrackSize::MinMax(a, b) => format!("minmax({}, {})", track_breadth(a), track_breadth(b)),
        TrackSize::FitContent(v) => format!("fit-content({})", lp(v)),
    }
}

fn line_names(names: &[crate::values::LineName]) -> Option<String> {
    (!names.is_empty()).then(|| format!("[{}]", join(names.iter().map(ToString::to_string), " ")))
}

/// A track list as specified (`none`, sizes, `repeat()` and line names).
pub(super) fn track_list(list: &TrackList) -> String {
    if list.is_none() {
        return "none".to_owned();
    }
    let mut parts: Vec<String> = Vec::new();
    for entry in list.entries() {
        parts.extend(line_names(&entry.names));
        match &entry.value {
            TrackListValue::Track(t) => parts.push(track_size(t)),
            TrackListValue::Repeat(r) => {
                let count = match r.count {
                    crate::values::RepeatCount::Count(n) => n.to_string(),
                    crate::values::RepeatCount::AutoFill => "auto-fill".to_owned(),
                    crate::values::RepeatCount::AutoFit => "auto-fit".to_owned(),
                };
                let mut inner: Vec<String> = Vec::new();
                for (i, t) in r.tracks.iter().enumerate() {
                    if let Some(names) = r.names.get(i) {
                        inner.extend(line_names(names));
                    }
                    inner.push(track_size(t));
                }
                if let Some(names) = r.names.get(r.tracks.len()) {
                    inner.extend(line_names(names));
                }
                parts.push(format!("repeat({count}, {})", inner.join(" ")));
            }
        }
    }
    parts.extend(line_names(list.trailing_names()));
    parts.join(" ")
}

/// `grid-template-areas`: the rows as strings, `.` for cells of no area.
pub(super) fn grid_areas(s: &ComputedStyle) -> String {
    let Some(areas) = &s.grid_template_areas else {
        return "none".to_owned();
    };
    join(
        area_rows(areas).into_iter().map(|row| {
            let mut out = String::new();
            swb_css::serialize_string(&row.join(" "), &mut out);
            out
        }),
        " ",
    )
}

/// The names of the cells of each row of a `grid-template-areas`.
pub(super) fn area_rows(areas: &crate::values::GridTemplateAreas) -> Vec<Vec<String>> {
    let rows = usize::try_from(areas.rows).unwrap_or(0).min(1000);
    let columns = usize::try_from(areas.columns).unwrap_or(0).min(1000);
    let mut cells = vec![vec![".".to_owned(); columns]; rows];
    for area in areas.areas() {
        for row in area.rows.0..area.rows.1 {
            for column in area.columns.0..area.columns.1 {
                if let Some(cell) = cells
                    .get_mut(row as usize)
                    .and_then(|r| r.get_mut(column as usize))
                {
                    *cell = area.name.to_string();
                }
            }
        }
    }
    cells
}

pub(super) fn grid_line(l: &GridLine) -> String {
    match l {
        GridLine::Auto => "auto".to_owned(),
        GridLine::Name(n) => n.to_string(),
        GridLine::Line(i, None) => i.to_string(),
        GridLine::Line(i, Some(n)) => format!("{i} {n}"),
        GridLine::Span(i, None) => format!("span {i}"),
        GridLine::Span(1, Some(n)) => format!("span {n}"),
        GridLine::Span(i, Some(n)) => format!("span {i} {n}"),
    }
}

/// `content`: on an element `none` is `normal`, on `::before` and
/// `::after` `normal` is `none` (CSS Content 3 §2); adjacent strings are
/// one string (Chromium joins them, after `attr()` became a string).
fn content(c: &Content, pseudo: Option<PseudoKind>) -> String {
    let before_after = matches!(pseudo, Some(PseudoKind::Before | PseudoKind::After));
    let items = match c {
        Content::Normal | Content::None if before_after => return "none".to_owned(),
        Content::None if pseudo.is_some() => return "none".to_owned(),
        Content::Normal | Content::None => return "normal".to_owned(),
        Content::Items(items) => items,
    };
    let mut parts: Vec<String> = Vec::new();
    let mut text: Option<String> = None;
    let flush = |text: &mut Option<String>, parts: &mut Vec<String>| {
        if let Some(t) = text.take() {
            let mut out = String::new();
            swb_css::serialize_string(&t, &mut out);
            parts.push(out);
        }
    };
    for item in items.iter() {
        match item {
            ContentItem::String(s) => text.get_or_insert_with(String::new).push_str(s),
            other => {
                flush(&mut text, &mut parts);
                parts.push(match other {
                    ContentItem::Image(i) => image(i),
                    ContentItem::Counter {
                        name,
                        separator,
                        style,
                    } => {
                        let mut out = String::new();
                        match separator {
                            Some(sep) => {
                                let _ = write!(out, "counters({name}, ");
                                swb_css::serialize_string(sep, &mut out);
                            }
                            None => {
                                let _ = write!(out, "counter({name}");
                            }
                        }
                        if *style != ListStyleType::Decimal {
                            let _ = write!(out, ", {}", style.as_str());
                        }
                        out.push(')');
                        out
                    }
                    ContentItem::OpenQuote => "open-quote".to_owned(),
                    ContentItem::CloseQuote => "close-quote".to_owned(),
                    ContentItem::String(_) => String::new(),
                });
            }
        }
    }
    flush(&mut text, &mut parts);
    parts.join(" ")
}

fn counters(list: &CounterList) -> String {
    if list.is_empty() {
        return "none".to_owned();
    }
    join(
        list.iter().map(|(name, value)| format!("{name} {value}")),
        " ",
    )
}

fn contain(s: &ComputedStyle) -> String {
    let c = s.contain;
    if c == crate::values::Contain::STRICT {
        return "strict".to_owned();
    }
    if c == crate::values::Contain::CONTENT {
        return "content".to_owned();
    }
    let names: Vec<&str> = [
        (c.size, "size"),
        (c.inline_size, "inline-size"),
        (c.layout, "layout"),
        (c.style, "style"),
        (c.paint, "paint"),
    ]
    .into_iter()
    .filter(|(on, _)| *on)
    .map(|(_, name)| name)
    .collect();
    if names.is_empty() {
        "none".to_owned()
    } else {
        names.join(" ")
    }
}

fn contain_intrinsic(c: crate::values::ContainIntrinsic) -> String {
    let length = c.length.map_or_else(|| "none".to_owned(), px);
    if c.auto {
        format!("auto {length}")
    } else {
        length
    }
}
