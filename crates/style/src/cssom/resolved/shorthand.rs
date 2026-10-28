//! The resolved values of shorthands, put together from their longhands
//! as Chromium does (measured with `tools/probes/web-computed-style.json`):
//! the box sides with as few values as possible, `border` only when the
//! four sides are equal, `outline` as color, style and width, `background`
//! with every part of each layer, `font` without its `normal` parts.

use super::longhand::{self, area_rows, grid_line, lp, track_list, track_size};
use super::{Resolver, number, px, rgba};
use crate::values::{
    Color, FontStyle, FontVariantCaps, GridLine, LineHeight, PositionComponent, TextDecorationStyle,
};

/// The value of the shorthand `name` (canonical, lower case).
#[allow(clippy::too_many_lines)] // One line per shorthand.
pub(super) fn value(name: &str, r: &Resolver<'_>) -> String {
    let get = |longhand: &str| longhand::value(longhand, r);
    let sides_of = |prefix: &str, suffix: &str| {
        sides(
            &["top", "right", "bottom", "left"].map(|side| get(&format!("{prefix}{side}{suffix}"))),
        )
    };
    match name {
        "margin" => sides_of("margin-", ""),
        "padding" => sides_of("padding-", ""),
        "inset" => sides(&["top", "right", "bottom", "left"].map(get)),
        "border-width" => sides_of("border-", "-width"),
        "border-style" => sides_of("border-", "-style"),
        "border-color" => sides_of("border-", "-color"),
        "margin-block" | "margin-inline" | "padding-block" | "padding-inline" | "inset-block"
        | "inset-inline" => pair(&get(&format!("{name}-start")), &get(&format!("{name}-end"))),
        "border-block-width"
        | "border-block-style"
        | "border-block-color"
        | "border-inline-width"
        | "border-inline-style"
        | "border-inline-color" => {
            let (axis, part) = name.rsplit_once('-').unwrap_or((name, ""));
            pair(
                &get(&format!("{axis}-start-{part}")),
                &get(&format!("{axis}-end-{part}")),
            )
        }
        "border-top"
        | "border-right"
        | "border-bottom"
        | "border-left"
        | "border-block-start"
        | "border-block-end"
        | "border-inline-start"
        | "border-inline-end" => border_side(name, r),
        "border" => {
            let all = ["border-top", "border-right", "border-bottom", "border-left"]
                .map(|side| border_side(side, r));
            if all.iter().all(|s| *s == all[0]) {
                all[0].clone()
            } else {
                String::new()
            }
        }
        "border-block" | "border-inline" => {
            let start = border_side(&format!("{name}-start"), r);
            if start == border_side(&format!("{name}-end"), r) {
                start
            } else {
                String::new()
            }
        }
        "border-radius" => radius(r),
        "border-image" => "none".to_owned(),
        "border-spacing" => pair(
            &get("-webkit-border-horizontal-spacing"),
            &get("-webkit-border-vertical-spacing"),
        ),
        "outline" => format!(
            "{} {} {}",
            get("outline-color"),
            get("outline-style"),
            get("outline-width")
        ),
        "background" => background(r),
        "background-position" => positions(
            r.style.background_image.len(),
            &r.style.background_position_x,
            &r.style.background_position_y,
        ),
        "mask-position" => positions(
            r.style.mask_image.len(),
            &r.style.mask_position_x,
            &r.style.mask_position_y,
        ),
        "mask" => mask(r),
        "font" => font(r),
        "font-variant" => get("font-variant-caps"),
        "list-style" => format!(
            "{} {} {}",
            get("list-style-position"),
            get("list-style-image"),
            get("list-style-type")
        ),
        "text-decoration" => text_decoration(r),
        "flex" => format!(
            "{} {} {}",
            get("flex-grow"),
            get("flex-shrink"),
            get("flex-basis")
        ),
        "flex-flow" => format!("{} {}", get("flex-direction"), get("flex-wrap")),
        "gap" => pair(&get("row-gap"), &get("column-gap")),
        "columns" => match (get("column-width"), get("column-count")) {
            (w, c) if w == "auto" && c == "auto" => "auto".to_owned(),
            (w, c) if w == "auto" => c,
            (w, c) if c == "auto" => w,
            (w, c) => format!("{w} {c}"),
        },
        "overflow" => pair(&get("overflow-x"), &get("overflow-y")),
        "contain-intrinsic-size" => pair(
            &get("contain-intrinsic-width"),
            &get("contain-intrinsic-height"),
        ),
        "place-content" => pair(&get("align-content"), &get("justify-content")),
        "place-items" => pair(&get("align-items"), &get("justify-items")),
        "place-self" => pair(&get("align-self"), &get("justify-self")),
        "grid-row" => grid_lines(&[&r.style.grid_row_start, &r.style.grid_row_end]),
        "grid-column" => grid_lines(&[&r.style.grid_column_start, &r.style.grid_column_end]),
        "grid-area" => grid_lines(&[
            &r.style.grid_row_start,
            &r.style.grid_column_start,
            &r.style.grid_row_end,
            &r.style.grid_column_end,
        ]),
        "grid-template" => grid_template(r),
        "grid" => [
            "grid-template-rows",
            "grid-template-columns",
            "grid-template-areas",
            "grid-auto-flow",
            "grid-auto-rows",
            "grid-auto-columns",
        ]
        .map(get)
        .join(" / "),
        _ => String::new(),
    }
}

/// True if layout can change the value of the shorthand `name`.
pub(super) fn needs_layout(name: &str) -> bool {
    matches!(
        name,
        "margin"
            | "padding"
            | "inset"
            | "margin-block"
            | "margin-inline"
            | "padding-block"
            | "padding-inline"
            | "inset-block"
            | "inset-inline"
    )
}

/// One to four values for top, right, bottom and left.
fn sides(v: &[String; 4]) -> String {
    let [top, right, bottom, left] = v;
    if left != right {
        format!("{top} {right} {bottom} {left}")
    } else if top != bottom {
        format!("{top} {right} {bottom}")
    } else if top != right {
        format!("{top} {right}")
    } else {
        top.clone()
    }
}

/// One value, or two if they differ.
fn pair(a: &str, b: &str) -> String {
    if a == b {
        a.to_owned()
    } else {
        format!("{a} {b}")
    }
}

/// `border-top` and the other sides: width, style and color.
fn border_side(name: &str, r: &Resolver<'_>) -> String {
    format!(
        "{} {} {}",
        longhand::value(&format!("{name}-width"), r),
        longhand::value(&format!("{name}-style"), r),
        longhand::value(&format!("{name}-color"), r)
    )
}

fn radius(r: &Resolver<'_>) -> String {
    let s = r.style;
    let corners = [
        &s.border_top_left_radius,
        &s.border_top_right_radius,
        &s.border_bottom_right_radius,
        &s.border_bottom_left_radius,
    ];
    let horizontal = corners.map(|c| lp(&c.horizontal));
    let vertical = corners.map(|c| lp(&c.vertical));
    if horizontal == vertical {
        sides(&horizontal)
    } else {
        format!("{} / {}", sides(&horizontal), sides(&vertical))
    }
}

/// `background`: each layer with all its parts, the color first in the
/// last layer.
fn background(r: &Resolver<'_>) -> String {
    let s = r.style;
    let count = s.background_image.len().max(1);
    let at = |list_len: usize, i: usize| i % list_len.max(1);
    let mut layers = Vec::with_capacity(count);
    for i in 0..count {
        let mut parts = Vec::new();
        if i + 1 == count {
            parts.push(rgba(s.background_color.resolve(s.color)));
        }
        parts.push(
            s.background_image
                .get(i)
                .and_then(Option::as_ref)
                .map_or_else(|| "none".to_owned(), longhand::image),
        );
        if let Some(repeat) = s.background_repeat.get(at(s.background_repeat.len(), i)) {
            parts.push(longhand::repeat(*repeat));
        }
        if let Some(a) = s
            .background_attachment
            .get(at(s.background_attachment.len(), i))
        {
            parts.push(a.as_str().to_owned());
        }
        let x = s
            .background_position_x
            .get(at(s.background_position_x.len(), i));
        let y = s
            .background_position_y
            .get(at(s.background_position_y.len(), i));
        if let (Some(x), Some(y)) = (x, y) {
            parts.push(format!(
                "{} {}",
                longhand::position(x),
                longhand::position(y)
            ));
        }
        if let Some(size) = s.background_size.get(at(s.background_size.len(), i)) {
            parts.push(format!("/ {}", longhand::bg_size(size)));
        }
        if let Some(origin) = s.background_origin.get(at(s.background_origin.len(), i)) {
            parts.push(origin.as_str().to_owned());
        }
        if let Some(clip) = s.background_clip.get(at(s.background_clip.len(), i)) {
            parts.push(longhand::background_box(*clip));
        }
        layers.push(parts.join(" "));
    }
    layers.join(", ")
}

/// `background-position` and `mask-position`: `x y` per layer.
fn positions(count: usize, x: &[PositionComponent], y: &[PositionComponent]) -> String {
    let count = count.max(1);
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        if let (Some(x), Some(y)) = (x.get(i % x.len().max(1)), y.get(i % y.len().max(1))) {
            out.push(format!(
                "{} {}",
                longhand::position(x),
                longhand::position(y)
            ));
        }
    }
    out.join(", ")
}

/// `mask`: the image of each layer and the parts that are not initial
/// (Chromium leaves those out).
fn mask(r: &Resolver<'_>) -> String {
    let s = r.style;
    let count = s.mask_image.len().max(1);
    let at = |list_len: usize, i: usize| i % list_len.max(1);
    let mut layers = Vec::with_capacity(count);
    for i in 0..count {
        let mut parts = vec![
            s.mask_image
                .get(i)
                .map_or_else(|| "none".to_owned(), longhand::mask_image),
        ];
        let x = s.mask_position_x.get(at(s.mask_position_x.len(), i));
        let y = s.mask_position_y.get(at(s.mask_position_y.len(), i));
        let size = s.mask_size.get(at(s.mask_size.len(), i));
        let initial_position = x.is_none_or(|x| *x == PositionComponent::default())
            && y.is_none_or(|y| *y == PositionComponent::default());
        let initial_size = size.is_none_or(|s| *s == crate::values::BackgroundSize::Auto);
        if let (Some(x), Some(y)) = (x, y)
            && (!initial_position || !initial_size)
        {
            parts.push(format!(
                "{} {}",
                longhand::position(x),
                longhand::position(y)
            ));
        }
        if let Some(size) = size.filter(|_| !initial_size) {
            parts.push(format!("/ {}", longhand::bg_size(size)));
        }
        if let Some(repeat) = s.mask_repeat.get(at(s.mask_repeat.len(), i)) {
            let repeat = longhand::repeat(*repeat);
            if repeat != "repeat" {
                parts.push(repeat);
            }
        }
        layers.push(parts.join(" "));
    }
    layers.join(", ")
}

/// `font`: `[style] [small-caps] [weight] [stretch] size[ / line-height]
/// family`, the `normal` parts left out; empty if a longhand has a value
/// that the shorthand cannot write.
fn font(r: &Resolver<'_>) -> String {
    let s = r.style;
    let mut parts = Vec::new();
    if s.font_style != FontStyle::Normal {
        parts.push("italic".to_owned());
    }
    match s.font_variant_caps {
        FontVariantCaps::Normal => {}
        FontVariantCaps::SmallCaps => parts.push("small-caps".to_owned()),
        _ => return String::new(),
    }
    if s.font_weight != 400.0 {
        parts.push(number(s.font_weight));
    }
    if s.font_stretch != 100.0 {
        let keyword = match s.font_stretch {
            50.0 => "ultra-condensed",
            62.5 => "extra-condensed",
            75.0 => "condensed",
            87.5 => "semi-condensed",
            112.5 => "semi-expanded",
            125.0 => "expanded",
            150.0 => "extra-expanded",
            200.0 => "ultra-expanded",
            _ => return String::new(),
        };
        parts.push(keyword.to_owned());
    }
    let size = px(s.font_size);
    parts.push(match s.line_height {
        LineHeight::Normal => size,
        LineHeight::Number(n) => format!("{size} / {}", px(n * s.font_size)),
        LineHeight::Px(v) => format!("{size} / {}", px(v)),
    });
    parts.push(longhand::font_family(s));
    parts.join(" ")
}

/// `text-decoration`: the line, the style unless `solid`, the color unless
/// `currentcolor`; `none` if all are left out.
fn text_decoration(r: &Resolver<'_>) -> String {
    let s = r.style;
    let mut parts = Vec::new();
    let line = longhand::value("text-decoration-line", r);
    if line != "none" {
        parts.push(line);
    }
    if s.text_decoration_style != TextDecorationStyle::Solid {
        parts.push(s.text_decoration_style.as_str().to_owned());
    }
    if let Color::Rgba(c) = s.text_decoration_color {
        parts.push(rgba(c));
    }
    if parts.is_empty() {
        "none".to_owned()
    } else {
        parts.join(" ")
    }
}

/// `grid-row`, `grid-column` (start, end) and `grid-area` (row start,
/// column start, row end, column end): the trailing values that the
/// shorthand would fill in are left out (CSS Grid 2 §8.4: an omitted end
/// is the start's name, or `auto`).
fn grid_lines(lines: &[&GridLine]) -> String {
    let default_of = |l: &GridLine| {
        if l.is_name() {
            l.clone()
        } else {
            GridLine::Auto
        }
    };
    // The value each position takes when it is left out.
    let defaults: Vec<GridLine> = match lines {
        [start, _] => vec![(*start).clone(), default_of(start)],
        [rs, cs, _, _] => vec![
            (*rs).clone(),
            default_of(rs),
            default_of(rs),
            default_of(cs),
        ],
        _ => Vec::new(),
    };
    let mut keep = lines.len();
    while keep > 1
        && lines
            .get(keep - 1)
            .zip(defaults.get(keep - 1))
            .is_some_and(|(l, d)| **l == *d)
    {
        keep -= 1;
    }
    lines
        .iter()
        .take(keep)
        .map(|l| grid_line(l))
        .collect::<Vec<_>>()
        .join(" / ")
}

/// `grid-template`: `none`, `rows / columns`, or the area strings with
/// their row sizes.
fn grid_template(r: &Resolver<'_>) -> String {
    let s = r.style;
    let rows = track_list(&s.grid_template_rows);
    let columns = track_list(&s.grid_template_columns);
    let Some(areas) = &s.grid_template_areas else {
        if s.grid_template_rows.is_none() && s.grid_template_columns.is_none() {
            return "none".to_owned();
        }
        return format!("{rows} / {columns}");
    };
    let sizes: Vec<String> = s
        .grid_template_rows
        .entries()
        .iter()
        .map(|e| match &e.value {
            crate::values::TrackListValue::Track(t) => track_size(t),
            crate::values::TrackListValue::Repeat(_) => String::new(),
        })
        .collect();
    let mut parts = Vec::new();
    for (i, row) in area_rows(areas).iter().enumerate() {
        let mut text = String::new();
        swb_css::serialize_string(&row.join(" "), &mut text);
        parts.push(text);
        if let Some(size) = sizes.get(i).filter(|t| !t.is_empty() && *t != "auto") {
            parts.push(size.clone());
        }
    }
    format!("{} / {columns}", parts.join(" "))
}
