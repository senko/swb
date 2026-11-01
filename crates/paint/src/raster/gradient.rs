//! Linear gradients for the rasterizer (CSS Images 3 §3,
//! <https://www.w3.org/TR/css-images-3/#gradients>): the gradient line,
//! the color stop fixup, the shader for tiny-skia and the premultiplied
//! interpolation of stops with different alpha. [`gradient_tile`] renders
//! one tile into a pixmap.

use swb_layout::{Point, Rect};
use swb_style::Rgba;
use tiny_skia::{
    GradientStop, IntSize, LinearGradient, Paint, Pixmap, Shader, SpreadMode, Transform,
};

use super::{solid_paint, to_skia_color};

/// The gradient line of a linear gradient whose box is `rect`: its start
/// and end points and its length. The line goes through the center of
/// `rect` at the gradient angle, with the length that CSS Images 3 §3.1.1
/// defines (<https://www.w3.org/TR/css-images-3/#linear-gradient-syntax>).
pub(super) fn gradient_line(rect: Rect, angle_deg: f32) -> (Point, Point, f32) {
    let (sin, cos) = angle_deg.to_radians().sin_cos();
    let half_len = f32::midpoint(rect.width * sin.abs(), rect.height * cos.abs());
    let cx = rect.x + rect.width / 2.0;
    let cy = rect.y + rect.height / 2.0;
    (
        Point::new(cx - sin * half_len, cy + cos * half_len),
        Point::new(cx + sin * half_len, cy - cos * half_len),
        half_len * 2.0,
    )
}

/// The positions of the color stops along the gradient line as fractions
/// of its length (CSS Images 3 §3.4.3,
/// <https://drafts.csswg.org/css-images-3/#color-stop-fixup>): a first
/// stop without a position is at 0 and a last one at 100%, a position
/// before an earlier one is moved up to it, and the stops without a
/// position between two with one are spread evenly. They are not limited
/// to 0..1.
pub(super) fn stop_positions(gradient: &swb_style::LinearGradient, line_len: f32) -> Vec<f32> {
    let n = gradient.stops.len();
    let mut positions: Vec<Option<f32>> = gradient
        .stops
        .iter()
        .map(|(_, pos)| {
            pos.as_ref().map(|p| {
                if line_len > 0.0 {
                    p.resolve(line_len) / line_len
                } else {
                    0.0
                }
            })
        })
        .collect();
    if let Some(first) = positions.first_mut() {
        first.get_or_insert(0.0);
    }
    if let Some(last) = positions.last_mut() {
        last.get_or_insert(1.0);
    }
    let mut highest = f32::NEG_INFINITY;
    for p in positions.iter_mut().flatten() {
        highest = highest.max(*p);
        *p = highest;
    }
    let mut out = vec![0.0; n];
    let mut i = 0;
    while i < n {
        if let Some(p) = positions.get(i).copied().flatten() {
            out[i] = p;
            i += 1;
            continue;
        }
        // A run of stops without a position, between two with one.
        let before = out.get(i.wrapping_sub(1)).copied().unwrap_or(0.0);
        let run_end = (i..n)
            .find(|&j| positions.get(j).copied().flatten().is_some())
            .unwrap_or(n);
        let after = positions.get(run_end).copied().flatten().unwrap_or(before);
        let steps = (run_end - i + 1) as f32;
        for (k, slot) in out.iter_mut().enumerate().take(run_end).skip(i) {
            *slot = before + (after - before) * (k - i + 1) as f32 / steps;
        }
        i = run_end;
    }
    out
}

/// The color of a gradient that has no shader ([`gradient_shader`] gives
/// `None`): the last stop of a repeating gradient whose stops are at one
/// position (Chromium 148 paints the last color, where CSS Images 3
/// asks for the average of the colors: measured in
/// `tools/probes/gradients.json`), else the first stop.
pub(super) fn degenerate_color(gradient: &swb_style::LinearGradient) -> Option<&swb_style::Color> {
    let stop = if gradient.repeating {
        gradient.stops.last()
    } else {
        gradient.stops.first()
    };
    stop.map(|(color, _)| color)
}

/// The shader of a linear gradient whose gradient line goes from
/// `line.0` to `line.1` (mapped by `transform` to device px) and is
/// `line_len` CSS px long; `None` for a degenerate gradient (a single stop
/// or a zero-length line, or a repeating one whose stops are at one
/// position). The shader's own line goes from the first to the last stop,
/// so that a repeating gradient repeats that part of the line and stops
/// outside 0..1 are right.
pub(super) fn gradient_shader(
    gradient: &swb_style::LinearGradient,
    current: Rgba,
    line_len: f32,
    (start, end): (Point, Point),
    transform: Transform,
) -> Option<Shader<'static>> {
    let positions = stop_positions(gradient, line_len);
    let first = *positions.first()?;
    let last = *positions.last()?;
    let span = last - first;
    let repeating = gradient.repeating;
    if !(span.is_finite() && first.is_finite()) || (repeating && span <= 1e-6) {
        return None;
    }
    // Stops at one position make a step: a shorter line is enough.
    let span = span.max(1e-6);
    let stops: Vec<(f32, tiny_skia::Color)> = gradient
        .stops
        .iter()
        .zip(&positions)
        .map(|((color, _), &t)| {
            (
                ((t - first) / span).clamp(0.0, 1.0),
                to_skia_color(color.resolve(current)),
            )
        })
        .collect();
    let stops: Vec<GradientStop> = premultiplied_stops(stops)
        .into_iter()
        .map(|(t, color)| GradientStop::new(t, color))
        .collect();
    let spread = if repeating {
        SpreadMode::Repeat
    } else {
        SpreadMode::Pad
    };
    let at = |t: f32| {
        tiny_skia::Point::from_xy(
            start.x + (end.x - start.x) * t,
            start.y + (end.y - start.y) * t,
        )
    };
    LinearGradient::new(at(first), at(first + span), stops, spread, transform)
}

/// The most stops that [`premultiplied_stops`] adds to the two that each
/// pair of stops keeps (the sections beyond the first of each pair). The
/// result has at most `2 * (n - 1) + MAX_GRADIENT_STOPS` stops for `n`
/// input stops: linear in the input, which the parser does not limit.
pub(super) const MAX_GRADIENT_STOPS: usize = 1024;

/// The most sections that [`premultiplied_stops`] cuts a pair of stops
/// into: the error of the approximation is below 0.5 of a level.
pub(super) const MAX_STOP_SECTIONS: usize = 32;

/// The stops of a gradient for tiny-skia, which interpolates colors without
/// the alpha, so that the gradient is interpolated in premultiplied space
/// (CSS Images 3 §3.5.3, <https://drafts.csswg.org/css-images-3/#coloring-gradient-line>;
/// Chromium does the same, measured with `tools/probes/gradients.json`).
///
/// Between two stops of different alpha the color without alpha is
/// `(c0·a0·(1-t) + c1·a1·t) / (a0·(1-t) + a1·t)`, which is not linear:
/// the pair is cut into up to [`MAX_STOP_SECTIONS`] sections (in
/// proportion to the difference of the alphas) with the exact color at
/// their ends. A fully transparent stop has the color of the stop next to
/// it on each side (a stop at the position of both neighbors becomes two).
/// Stops with the same alpha or the same color need no change.
pub(super) fn premultiplied_stops(
    stops: Vec<(f32, tiny_skia::Color)>,
) -> Vec<(f32, tiny_skia::Color)> {
    let plain = |a: tiny_skia::Color, b: tiny_skia::Color| {
        a.alpha() == b.alpha()
            || (a.red() == b.red() && a.green() == b.green() && a.blue() == b.blue())
    };
    if stops.len() < 2 || stops.windows(2).all(|w| plain(w[0].1, w[1].1)) {
        return stops;
    }
    // The sections of each pair, scaled to the budget.
    let wanted: Vec<usize> = stops
        .windows(2)
        .map(|w| {
            let (a0, a1) = (w[0].1.alpha(), w[1].1.alpha());
            if plain(w[0].1, w[1].1) || a0 == 0.0 || a1 == 0.0 {
                1
            } else {
                ((a0 - a1).abs() * MAX_STOP_SECTIONS as f32).ceil().max(2.0) as usize
            }
        })
        .collect();
    let total: usize = wanted.iter().sum::<usize>() + 2 * stops.len();
    let scale = if total > MAX_GRADIENT_STOPS {
        MAX_GRADIENT_STOPS as f32 / total as f32
    } else {
        1.0
    };
    let mut out: Vec<(f32, tiny_skia::Color)> = Vec::with_capacity(total.min(MAX_GRADIENT_STOPS));
    let mut push = |t: f32, color: tiny_skia::Color| {
        if out.last().is_none_or(|(lt, lc)| *lt != t || *lc != color) {
            out.push((t, color));
        }
    };
    for (pair, sections) in stops.windows(2).zip(wanted) {
        let ((t0, c0), (t1, c1)) = ((pair[0].0, pair[0].1), (pair[1].0, pair[1].1));
        let (a0, a1) = (c0.alpha(), c1.alpha());
        if plain(c0, c1) {
            push(t0, c0);
            push(t1, c1);
            continue;
        }
        let with_alpha = |from: tiny_skia::Color, a: f32| {
            tiny_skia::Color::from_rgba(from.red(), from.green(), from.blue(), a).unwrap_or(from)
        };
        // A transparent end has the color of the other end.
        let start = if a0 == 0.0 { with_alpha(c1, 0.0) } else { c0 };
        let end = if a1 == 0.0 { with_alpha(c0, 0.0) } else { c1 };
        push(t0, start);
        let sections = ((sections as f32 * scale) as usize).max(1);
        for k in 1..sections {
            let u = k as f32 / sections as f32;
            let a = a0 + (a1 - a0) * u;
            if a <= 0.0 {
                continue;
            }
            let mix = |x0: f32, x1: f32| ((x0 * a0 * (1.0 - u) + x1 * a1 * u) / a).clamp(0.0, 1.0);
            let color = tiny_skia::Color::from_rgba(
                mix(c0.red(), c1.red()),
                mix(c0.green(), c1.green()),
                mix(c0.blue(), c1.blue()),
                a.clamp(0.0, 1.0),
            );
            if let Some(color) = color {
                push(t0 + (t1 - t0) * u, color);
            }
        }
        push(t1, end);
    }
    out
}

/// Renders a linear gradient whose box is `tile` (CSS px) into a pixmap of
/// `size`.
pub(super) fn gradient_tile(
    gradient: &swb_style::LinearGradient,
    current: Rgba,
    tile: Rect,
    size: IntSize,
) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(size.width(), size.height())?;
    let (width, height) = (size.width() as f32, size.height() as f32);
    // The gradient is computed in CSS px and scaled to the pixmap, so its
    // angle stays right when the pixmap has other proportions.
    let to_pixmap = Transform::from_scale(width / tile.width, height / tile.height);
    let (start, end, line_len) = gradient_line(
        Rect::new(0.0, 0.0, tile.width, tile.height),
        gradient.angle_deg,
    );
    let paint = match gradient_shader(gradient, current, line_len, (start, end), to_pixmap) {
        Some(shader) => Paint {
            shader,
            anti_alias: false,
            ..Paint::default()
        },
        None => solid_paint(degenerate_color(gradient)?.resolve(current), false),
    };
    let rect = tiny_skia::Rect::from_xywh(0.0, 0.0, width, height)?;
    pixmap.fill_rect(rect, &paint, Transform::identity(), None);
    Some(pixmap)
}
