//! The pixel work of the CSS `filter` property (Filter Effects 1 §5,
//! <https://drafts.fxtf.org/filter-effects-1/#FilterProperty>) on the
//! premultiplied RGBA8 layer of a group.
//!
//! The functions are measured against Chromium 148
//! (`tools/probes/filter.json`):
//!
//! - The color functions (`brightness()`, `contrast()`, `grayscale()`,
//!   `hue-rotate()`, `invert()`, `opacity()`, `saturate()`, `sepia()`)
//!   work in sRGB on the color without the alpha, with the matrices of the
//!   specification, and clamp the result to 0..1 after every function. They
//!   do not change the alpha, except `opacity()`.
//! - `blur(σ)` is the approximation of the specification
//!   (<https://drafts.fxtf.org/filter-effects-1/#feGaussianBlurElement>):
//!   three box blurs of the size `d = floor(σ · 3 · sqrt(2π) / 4 + 0.5)`
//!   per axis (for an even `d`: two boxes of size `d`, one centered to the
//!   left and one to the right, and one of size `d + 1`), on the
//!   premultiplied pixels, with transparent pixels around. `d` below 2 is
//!   no blur. `σ` is in device px.
//! - `drop-shadow(x y σ color)` blurs the alpha, moves it by the offsets
//!   (in whole device pixels, rounded down), colors it and draws the
//!   element over it. The `σ` is the blur argument as it is.
//!
//! A deviation: the standard deviation is limited to [`MAX_SIGMA`] device
//! px (ADR 0028 section 16; Chromium has no limit).

use swb_layout::Rect;
use swb_style::{FilterFunction, Rgba};

/// The largest standard deviation of a blur, in device px.
pub(crate) const MAX_SIGMA: f32 = 256.0;

/// The most device px that a shadow offset moves the shadow.
const MAX_OFFSET: f32 = 1_000_000.0;

/// One step of a color pass: a function of the color of a pixel, without
/// the alpha.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ColorStep {
    /// `c' = slope · c + intercept` for each channel (brightness, contrast,
    /// invert).
    Linear { slope: f32, intercept: f32 },
    /// The red, green and blue as a 3 by 3 matrix, row by row.
    Matrix([f32; 9]),
    /// The alpha times a factor.
    Opacity(f32),
}

/// A step of a filter on a layer, in device px.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Op {
    /// Color functions in a row.
    Colors(Vec<ColorStep>),
    /// `blur()` with the standard deviation.
    Blur(f32),
    /// `drop-shadow()`.
    Shadow(Shadow),
}

/// A `drop-shadow()` in device px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Shadow {
    pub(crate) dx: i32,
    pub(crate) dy: i32,
    pub(crate) sigma: f32,
    /// Red, green and blue (0 to 1) and the alpha.
    pub(crate) color: [f32; 4],
}

/// Distances beyond a rectangle, in px.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Margins {
    pub(crate) left: u32,
    pub(crate) top: u32,
    pub(crate) right: u32,
    pub(crate) bottom: u32,
}

impl Margins {
    fn add(self, other: Margins) -> Margins {
        Margins {
            left: self.left.saturating_add(other.left),
            top: self.top.saturating_add(other.top),
            right: self.right.saturating_add(other.right),
            bottom: self.bottom.saturating_add(other.bottom),
        }
    }
}

// ----- The functions as steps -----

/// True if the function changes nothing (or is not painted): `url()`,
/// `blur(0)` and the amounts that are the identity.
pub(crate) fn is_identity(f: &FilterFunction) -> bool {
    match *f {
        FilterFunction::Blur(v) => v <= 0.0,
        FilterFunction::Brightness(v)
        | FilterFunction::Contrast(v)
        | FilterFunction::Opacity(v)
        | FilterFunction::Saturate(v) => v == 1.0,
        FilterFunction::Grayscale(v) | FilterFunction::Invert(v) | FilterFunction::Sepia(v) => {
            v == 0.0
        }
        FilterFunction::HueRotate(v) => v % 360.0 == 0.0,
        FilterFunction::DropShadow(_) => false,
        FilterFunction::Url(_) => true,
    }
}

/// True if some function of the list paints.
pub(crate) fn paints(list: &[FilterFunction]) -> bool {
    list.iter().any(|f| !is_identity(f))
}

/// The step of a color function, `None` for the others.
fn color_step(f: &FilterFunction) -> Option<ColorStep> {
    Some(match *f {
        FilterFunction::Brightness(a) => ColorStep::Linear {
            slope: a,
            intercept: 0.0,
        },
        FilterFunction::Contrast(a) => ColorStep::Linear {
            slope: a,
            intercept: 0.5 - 0.5 * a,
        },
        FilterFunction::Invert(a) => ColorStep::Linear {
            slope: 1.0 - 2.0 * a,
            intercept: a,
        },
        FilterFunction::Opacity(a) => ColorStep::Opacity(a),
        FilterFunction::Grayscale(a) => {
            let k = 1.0 - a;
            ColorStep::Matrix([
                0.2126 + 0.7874 * k,
                0.7152 - 0.7152 * k,
                0.0722 - 0.0722 * k,
                0.2126 - 0.2126 * k,
                0.7152 + 0.2848 * k,
                0.0722 - 0.0722 * k,
                0.2126 - 0.2126 * k,
                0.7152 - 0.7152 * k,
                0.0722 + 0.9278 * k,
            ])
        }
        FilterFunction::Sepia(a) => {
            let k = 1.0 - a;
            ColorStep::Matrix([
                0.393 + 0.607 * k,
                0.769 - 0.769 * k,
                0.189 - 0.189 * k,
                0.349 - 0.349 * k,
                0.686 + 0.314 * k,
                0.168 - 0.168 * k,
                0.272 - 0.272 * k,
                0.534 - 0.534 * k,
                0.131 + 0.869 * k,
            ])
        }
        FilterFunction::Saturate(s) => ColorStep::Matrix([
            0.213 + 0.787 * s,
            0.715 - 0.715 * s,
            0.072 - 0.072 * s,
            0.213 - 0.213 * s,
            0.715 + 0.285 * s,
            0.072 - 0.072 * s,
            0.213 - 0.213 * s,
            0.715 - 0.715 * s,
            0.072 + 0.928 * s,
        ]),
        FilterFunction::HueRotate(degrees) => {
            let (sin, cos) = degrees.to_radians().sin_cos();
            ColorStep::Matrix([
                0.213 + cos * 0.787 - sin * 0.213,
                0.715 - cos * 0.715 - sin * 0.715,
                0.072 - cos * 0.072 + sin * 0.928,
                0.213 - cos * 0.213 + sin * 0.143,
                0.715 + cos * 0.285 + sin * 0.140,
                0.072 - cos * 0.072 - sin * 0.283,
                0.213 - cos * 0.213 - sin * 0.787,
                0.715 - cos * 0.715 + sin * 0.715,
                0.072 + cos * 0.928 + sin * 0.072,
            ])
        }
        FilterFunction::Blur(_) | FilterFunction::DropShadow(_) | FilterFunction::Url(_) => {
            return None;
        }
    })
}

/// The steps of a filter at the device scale `scale` (device px per CSS
/// px), with the color of `currentColor` in a shadow. The functions that
/// change nothing are left out.
pub(crate) fn device_ops(list: &[FilterFunction], current_color: Rgba, scale: f32) -> Vec<Op> {
    let mut ops: Vec<Op> = Vec::new();
    for f in list {
        if is_identity(f) {
            continue;
        }
        if let Some(step) = color_step(f) {
            match ops.last_mut() {
                Some(Op::Colors(steps)) => steps.push(step),
                _ => ops.push(Op::Colors(vec![step])),
            }
            continue;
        }
        match *f {
            FilterFunction::Blur(sigma) => {
                let sigma = (sigma * scale).clamp(0.0, MAX_SIGMA);
                if box_size(sigma) >= 2 {
                    ops.push(Op::Blur(sigma));
                }
            }
            FilterFunction::DropShadow(shadow) => {
                let color = shadow.color.resolve(current_color);
                if color.a == 0 {
                    continue;
                }
                let offset = |v: f32| (v * scale).clamp(-MAX_OFFSET, MAX_OFFSET).floor() as i32;
                ops.push(Op::Shadow(Shadow {
                    dx: offset(shadow.x),
                    dy: offset(shadow.y),
                    sigma: (shadow.blur * scale).clamp(0.0, MAX_SIGMA),
                    color: [
                        f32::from(color.r) / 255.0,
                        f32::from(color.g) / 255.0,
                        f32::from(color.b) / 255.0,
                        f32::from(color.a) / 255.0,
                    ],
                }));
            }
            _ => {}
        }
    }
    ops
}

// ----- Extents and work -----

/// How far a blur of `sigma` CSS px can reach in CSS px, for the bounds of
/// groups in the display list (which do not know the scale): generous,
/// and at most 3,200 px (the longest reach of a limited blur at a scale
/// of 0.25 or more).
fn css_reach(sigma: f32) -> f32 {
    if sigma.is_finite() {
        (3.0 * sigma + 8.0).min(3200.0)
    } else {
        3200.0
    }
}

/// An area that contains the output of the `filter` functions for content
/// in `content` (CSS px): the blur and the shadows grow it.
pub(crate) fn css_output(content: Rect, list: &[FilterFunction]) -> Rect {
    let mut area = content;
    for f in list {
        match f {
            FilterFunction::Blur(sigma) if *sigma > 0.0 => {
                let r = css_reach(*sigma);
                area = Rect::new(
                    area.x - r,
                    area.y - r,
                    area.width + 2.0 * r,
                    area.height + 2.0 * r,
                );
            }
            FilterFunction::DropShadow(s) => {
                let r = css_reach(s.blur);
                let moved = Rect::new(
                    area.x + s.x - r,
                    area.y + s.y - r,
                    area.width + 2.0 * r,
                    area.height + 2.0 * r,
                );
                area = area.union(&moved);
            }
            _ => {}
        }
    }
    area
}

/// The window of the box blur for the standard deviation `sigma`: below 2
/// is no blur.
pub(crate) fn box_size(sigma: f32) -> usize {
    let d = (sigma * 3.0 * (2.0 * std::f32::consts::PI).sqrt() / 4.0 + 0.5).floor();
    if d.is_finite() && d > 0.0 {
        d as usize
    } else {
        0
    }
}

/// How far a blur with `sigma` moves the ink in any direction, in px.
fn blur_reach(sigma: f32) -> u32 {
    let d = box_size(sigma);
    if d < 2 { 0 } else { (3 * d / 2 + 1) as u32 }
}

fn uniform(r: u32) -> Margins {
    Margins {
        left: r,
        top: r,
        right: r,
        bottom: r,
    }
}

/// How far beyond the input rectangle the output of the steps can have
/// ink.
pub(crate) fn forward_growth(ops: &[Op]) -> Margins {
    extents(ops).0
}

/// How far beyond the input rectangle any result of the steps (also the
/// blurred shadow before it is moved) can have ink: the size of the
/// layer that the steps work on.
pub(crate) fn peak_extent(ops: &[Op]) -> Margins {
    extents(ops).1
}

/// The extents of the output of the steps, and the largest extents of all
/// results on the way, beyond the input rectangle.
fn extents(ops: &[Op]) -> (Margins, Margins) {
    let mut m = Margins::default();
    let mut peak = m;
    let max = |a: Margins, b: Margins| Margins {
        left: a.left.max(b.left),
        top: a.top.max(b.top),
        right: a.right.max(b.right),
        bottom: a.bottom.max(b.bottom),
    };
    for op in ops {
        match op {
            Op::Colors(_) => {}
            Op::Blur(sigma) => m = m.add(uniform(blur_reach(*sigma))),
            Op::Shadow(s) => {
                let r = i64::from(blur_reach(s.sigma));
                peak = max(peak, m.add(uniform(r as u32)));
                let grow = |old: u32, delta: i64| (i64::from(old) + delta.max(0)) as u32;
                m = Margins {
                    left: grow(m.left, r - i64::from(s.dx)),
                    top: grow(m.top, r - i64::from(s.dy)),
                    right: grow(m.right, r + i64::from(s.dx)),
                    bottom: grow(m.bottom, r + i64::from(s.dy)),
                };
            }
        }
        peak = max(peak, m);
    }
    (m, peak)
}

/// How far beyond a rectangle of output the input can matter.
pub(crate) fn backward_reach(ops: &[Op]) -> Margins {
    let mut m = Margins::default();
    for op in ops {
        match op {
            Op::Colors(_) => {}
            Op::Blur(sigma) => m = m.add(uniform(blur_reach(*sigma))),
            Op::Shadow(s) => {
                let r = i64::from(blur_reach(s.sigma));
                let need = |delta: i64| delta.max(0) as u32;
                m = m.add(Margins {
                    left: need(r + i64::from(s.dx)),
                    top: need(r + i64::from(s.dy)),
                    right: need(r - i64::from(s.dx)),
                    bottom: need(r - i64::from(s.dy)),
                });
            }
        }
    }
    m
}

/// The work of the steps on a layer of `pixels` pixels, in units of one
/// pixel of a color pass (a blur takes six passes over the pixels, a shadow
/// eight).
pub(crate) fn work(ops: &[Op], pixels: u64) -> u64 {
    let units: u64 = ops
        .iter()
        .map(|op| match op {
            Op::Colors(steps) => 1 + steps.len() as u64 / 4,
            Op::Blur(_) => 6,
            Op::Shadow(_) => 8,
        })
        .sum();
    units.saturating_mul(pixels)
}

// ----- Applying -----

/// Applies the steps to `data`, `width` by `height` premultiplied RGBA8
/// pixels. `scratch` is a buffer of the same size that the blur and shadow
/// steps need; without it (shorter) they are skipped.
pub(crate) fn apply(data: &mut [u8], scratch: &mut [u8], width: usize, height: usize, ops: &[Op]) {
    if data.len() != width * height * 4 {
        return;
    }
    let has_scratch = scratch.len() >= data.len();
    for op in ops {
        match op {
            Op::Colors(steps) => color_pass(data, steps),
            Op::Blur(sigma) if has_scratch => {
                box_blur::<4>(data, scratch, width, height, *sigma);
            }
            Op::Shadow(shadow) if has_scratch => {
                drop_shadow(data, scratch, width, height, shadow);
            }
            Op::Blur(_) | Op::Shadow(_) => {}
        }
    }
}

/// Applies color steps to every pixel.
fn color_pass(data: &mut [u8], steps: &[ColorStep]) {
    let (pixels, _) = data.as_chunks_mut::<4>();
    for px in pixels {
        let alpha8 = px[3];
        if alpha8 == 0 {
            continue;
        }
        let scale = 1.0 / f32::from(alpha8);
        let mut rgb = [
            f32::from(px[0]) * scale,
            f32::from(px[1]) * scale,
            f32::from(px[2]) * scale,
        ];
        let mut alpha = f32::from(alpha8) / 255.0;
        for step in steps {
            match *step {
                ColorStep::Linear { slope, intercept } => {
                    for channel in &mut rgb {
                        *channel = (*channel * slope + intercept).clamp(0.0, 1.0);
                    }
                }
                ColorStep::Matrix(m) => {
                    let [red, green, blue] = rgb;
                    rgb = [
                        (m[0] * red + m[1] * green + m[2] * blue).clamp(0.0, 1.0),
                        (m[3] * red + m[4] * green + m[5] * blue).clamp(0.0, 1.0),
                        (m[6] * red + m[7] * green + m[8] * blue).clamp(0.0, 1.0),
                    ];
                }
                ColorStep::Opacity(factor) => alpha = (alpha * factor).clamp(0.0, 1.0),
            }
        }
        let premultiplied = |channel: f32| (channel * alpha * 255.0 + 0.5) as u8;
        px[0] = premultiplied(rgb[0]);
        px[1] = premultiplied(rgb[1]);
        px[2] = premultiplied(rgb[2]);
        px[3] = (alpha * 255.0 + 0.5) as u8;
    }
}

/// The window of one box blur: its size and the number of pixels before
/// the output pixel that it covers.
#[derive(Clone, Copy)]
struct BoxPass {
    size: usize,
    before: usize,
}

/// The three box blurs of one axis for the window `d` (see the module
/// documentation).
fn box_passes(d: usize) -> [BoxPass; 3] {
    if d % 2 == 1 {
        let pass = BoxPass {
            size: d,
            before: d / 2,
        };
        [pass; 3]
    } else {
        [
            BoxPass {
                size: d,
                before: d / 2,
            },
            BoxPass {
                size: d,
                before: d / 2 - 1,
            },
            BoxPass {
                size: d + 1,
                before: d / 2,
            },
        ]
    }
}

/// Blurs `N` interleaved channels of `data` with the standard deviation
/// `sigma`. `scratch` has the size of `data`.
fn box_blur<const N: usize>(
    data: &mut [u8],
    scratch: &mut [u8],
    width: usize,
    height: usize,
    sigma: f32,
) {
    let d = box_size(sigma.min(MAX_SIGMA));
    if d < 2 || width == 0 || height == 0 {
        return;
    }
    let passes = box_passes(d);
    // Each pass reads one buffer and writes the other. The three
    // horizontal passes end in `scratch`, the three vertical ones in
    // `data`.
    let mut sums = vec![0_u32; width * N];
    for (i, pass) in passes.iter().enumerate() {
        if i % 2 == 0 {
            horizontal::<N>(data, scratch, width, height, *pass);
        } else {
            horizontal::<N>(scratch, data, width, height, *pass);
        }
    }
    for (i, pass) in passes.iter().enumerate() {
        if i % 2 == 0 {
            vertical::<N>(scratch, data, width, height, *pass, &mut sums);
        } else {
            vertical::<N>(data, scratch, width, height, *pass, &mut sums);
        }
    }
}

/// A rounded division by `size` with a multiplication.
#[derive(Clone, Copy)]
struct Divider {
    size: u64,
    factor: u64,
}

impl Divider {
    const SHIFT: u32 = 32;

    fn new(size: usize) -> Divider {
        let size = size.max(1) as u64;
        Divider {
            size,
            factor: (1_u64 << Self::SHIFT).div_ceil(size),
        }
    }

    fn divide(self, sum: u32) -> u8 {
        let rounded = u64::from(sum) + self.size / 2;
        ((rounded * self.factor) >> Self::SHIFT).min(255) as u8
    }
}

/// One horizontal box blur from `src` to `dst`.
fn horizontal<const N: usize>(
    src: &[u8],
    dst: &mut [u8],
    width: usize,
    height: usize,
    pass: BoxPass,
) {
    let divider = Divider::new(pass.size);
    let after = pass.size - 1 - pass.before;
    for y in 0..height {
        let row = y * width * N;
        let (Some(src_row), Some(dst_row)) = (
            src.get(row..row + width * N),
            dst.get_mut(row..row + width * N),
        ) else {
            return;
        };
        // The window of pixel x is [x - before, x + after].
        let mut sum = [0_u32; N];
        for x in 0..=after.min(width - 1) {
            for c in 0..N {
                sum[c] += u32::from(src_row[x * N + c]);
            }
        }
        for x in 0..width {
            for c in 0..N {
                dst_row[x * N + c] = divider.divide(sum[c]);
            }
            // Move the window to x + 1.
            let entering = x + after + 1;
            if entering < width {
                for c in 0..N {
                    sum[c] += u32::from(src_row[entering * N + c]);
                }
            }
            if x >= pass.before {
                let leaving = x - pass.before;
                for c in 0..N {
                    sum[c] -= u32::from(src_row[leaving * N + c]);
                }
            }
        }
    }
}

/// One vertical box blur from `src` to `dst`.
fn vertical<const N: usize>(
    src: &[u8],
    dst: &mut [u8],
    width: usize,
    height: usize,
    pass: BoxPass,
    sums: &mut [u32],
) {
    let divider = Divider::new(pass.size);
    let after = pass.size - 1 - pass.before;
    let stride = width * N;
    let Some(sums) = sums.get_mut(..stride) else {
        return;
    };
    sums.fill(0);
    for y in 0..=after.min(height - 1) {
        let Some(row) = src.get(y * stride..(y + 1) * stride) else {
            return;
        };
        for (s, v) in sums.iter_mut().zip(row) {
            *s += u32::from(*v);
        }
    }
    for y in 0..height {
        let Some(out) = dst.get_mut(y * stride..(y + 1) * stride) else {
            return;
        };
        for (o, s) in out.iter_mut().zip(sums.iter()) {
            *o = divider.divide(*s);
        }
        let entering = y + after + 1;
        if entering < height
            && let Some(row) = src.get(entering * stride..(entering + 1) * stride)
        {
            for (s, v) in sums.iter_mut().zip(row) {
                *s += u32::from(*v);
            }
        }
        if y >= pass.before
            && let Some(row) = src.get((y - pass.before) * stride..(y - pass.before + 1) * stride)
        {
            for (s, v) in sums.iter_mut().zip(row) {
                *s -= u32::from(*v);
            }
        }
    }
}

/// `drop-shadow()`: the layer over its own shadow.
fn drop_shadow(data: &mut [u8], scratch: &mut [u8], width: usize, height: usize, shadow: &Shadow) {
    let pixels = width * height;
    let mut alpha: Vec<u8> = data.as_chunks::<4>().0.iter().map(|px| px[3]).collect();
    if let Some(plane) = scratch.get_mut(..pixels) {
        box_blur::<1>(&mut alpha, plane, width, height, shadow.sigma);
    }
    let [red, green, blue, color_alpha] = shadow.color;
    for y in 0..height {
        let Some(from_y) = (y as i64).checked_sub(i64::from(shadow.dy)) else {
            continue;
        };
        if from_y < 0 || from_y >= height as i64 {
            continue;
        }
        for x in 0..width {
            let from_x = x as i64 - i64::from(shadow.dx);
            if from_x < 0 || from_x >= width as i64 {
                continue;
            }
            let covered = alpha
                .get(from_y as usize * width + from_x as usize)
                .map_or(0.0, |&a| f32::from(a) / 255.0 * color_alpha);
            if covered <= 0.0 {
                continue;
            }
            let at = (y * width + x) * 4;
            let Some(px) = data.get_mut(at..at + 4) else {
                continue;
            };
            let behind = 1.0 - f32::from(px[3]) / 255.0;
            let amount = covered * behind * 255.0;
            px[0] = (f32::from(px[0]) + red * amount + 0.5).min(255.0) as u8;
            px[1] = (f32::from(px[1]) + green * amount + 0.5).min(255.0) as u8;
            px[2] = (f32::from(px[2]) + blue * amount + 0.5).min(255.0) as u8;
            px[3] = (f32::from(px[3]) + amount + 0.5).min(255.0) as u8;
        }
    }
}

#[cfg(test)]
mod tests;
