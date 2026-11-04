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
//! `backdrop-filter` (Filter Effects 2 §2; ADR 0033) uses the same steps
//! on the pixels of the backdrop, with the image mirrored at its edges
//! ([`Edge::Mirror`]) instead of transparent pixels around it.
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

/// What a blur reads beyond the edges of the pixels it works on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Edge {
    /// Transparent pixels (`filter`).
    Transparent,
    /// The image mirrored at its edges, `abcd|dcba` (`backdrop-filter`,
    /// measured in Chromium 148: the backdrop of a box is clipped to its
    /// border box and a blur does not fade at the edges).
    Mirror,
}

/// Applies the steps to `data`, `width` by `height` premultiplied RGBA8
/// pixels. `scratch` is a buffer of the same size that the blur and shadow
/// steps need; without it (shorter) they are skipped.
pub(crate) fn apply(data: &mut [u8], scratch: &mut [u8], width: usize, height: usize, ops: &[Op]) {
    apply_with(data, scratch, width, height, ops, Edge::Transparent);
}

/// [`apply`] for a backdrop: the blurs read the image mirrored at its
/// edges ([`Edge::Mirror`]). The caller gives no scratch buffer: the
/// mirrored blur makes the buffers it needs.
pub(crate) fn apply_mirrored(data: &mut [u8], width: usize, height: usize, ops: &[Op]) {
    apply_with(data, &mut [], width, height, ops, Edge::Mirror);
}

/// [`apply`] with the given edge mode for the blurs. The scratch buffer
/// contract depends on the mode: [`Edge::Transparent`] needs `scratch` of
/// the size of `data` (without it the blur and shadow steps are skipped);
/// [`Edge::Mirror`] ignores `scratch` and allocates its own buffers.
fn apply_with(
    data: &mut [u8],
    scratch: &mut [u8],
    width: usize,
    height: usize,
    ops: &[Op],
    edge: Edge,
) {
    if data.len() != width * height * 4 {
        return;
    }
    let has_scratch = edge == Edge::Mirror || scratch.len() >= data.len();
    for op in ops {
        match op {
            Op::Colors(steps) => color_pass(data, steps),
            Op::Blur(sigma) if has_scratch => {
                box_blur::<4>(data, scratch, width, height, *sigma, edge);
            }
            Op::Shadow(shadow) if has_scratch => {
                drop_shadow(data, scratch, width, height, shadow, edge);
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
    edge: Edge,
) {
    let d = box_size(sigma.min(MAX_SIGMA));
    if d < 2 || width == 0 || height == 0 {
        return;
    }
    if edge == Edge::Mirror {
        box_blur_mirrored::<N>(data, width, height, sigma);
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

/// The index in `0..n` that the position `i` stands for when the image
/// repeats mirrored at its edges (`abcd|dcba|abcd`).
fn mirror_index(i: i64, n: usize) -> usize {
    let n = n.max(1) as i64;
    let m = i.rem_euclid(2 * n);
    (if m < n { m } else { 2 * n - 1 - m }) as usize
}

/// Blurs `N` interleaved channels of `data` with the standard deviation
/// `sigma` like [`box_blur`], but with the image mirrored at its edges
/// (see [`Edge::Mirror`]): each row and then each column is extended by
/// the reach of the blur on both sides, blurred in the extended line, and
/// cropped. (Mirroring the line before every one of the three box passes
/// would not match Chromium: with an even window the intermediate lines are
/// not symmetric about the edge. Measured: the extension is made once.)
/// The rows go first, then the columns; the work buffers are two blocks of
/// [`BLOCK`] lines, and the vertical passes of an image that fits
/// [`MAX_PADDED_PIXELS`] use two images of their own.
fn box_blur_mirrored<const N: usize>(data: &mut [u8], width: usize, height: usize, sigma: f32) {
    let sigma = sigma.min(MAX_SIGMA);
    let d = box_size(sigma);
    if d < 2 || width == 0 || height == 0 || data.len() < width * height * N {
        return;
    }
    let plan = MirrorPlan {
        passes: box_passes(d),
        reach: blur_reach(sigma) as usize,
    };
    let longest = width.max(height) + 2 * plan.reach;
    let mut blocks = Blocks {
        line: vec![0_u8; BLOCK * longest * N],
        other: vec![0_u8; BLOCK * longest * N],
    };
    blur_rows_mirrored::<N>(data, width, &plan, &mut blocks);
    blur_columns_mirrored::<N>(data, (width, height), &plan, &mut blocks);
}

/// The box passes of a mirrored blur and how far each line is extended.
struct MirrorPlan {
    passes: [BoxPass; 3],
    /// The extension on each side of a line, in pixels.
    reach: usize,
}

/// The two work buffers of [`BLOCK`] extended lines each that the passes
/// of a block alternate between.
struct Blocks {
    line: Vec<u8>,
    other: Vec<u8>,
}

impl Blocks {
    /// The three box passes over `lines` lines of `len` pixels in `line`;
    /// the result is in `other`.
    fn blur<const N: usize>(&mut self, len: usize, lines: usize, plan: &MirrorPlan) {
        blur_block::<N>(&mut self.line, &mut self.other, len, lines, &plan.passes);
    }
}

/// The horizontal half of the mirrored blur: each row of `data` (`width`
/// pixels) is extended mirrored, blurred and cropped.
fn blur_rows_mirrored<const N: usize>(
    data: &mut [u8],
    width: usize,
    plan: &MirrorPlan,
    blocks: &mut Blocks,
) {
    let reach = plan.reach;
    let len = width + 2 * reach;
    let map = mirror_map(width, reach);
    let row_bytes = width * N;
    for rows in data.chunks_mut(BLOCK * row_bytes) {
        let lines = rows.len() / row_bytes;
        for (line, row) in blocks
            .line
            .chunks_exact_mut(len * N)
            .zip(rows.chunks_exact(row_bytes))
        {
            for (out, &from) in line.as_chunks_mut::<N>().0.iter_mut().zip(&map) {
                if let Some(src) = row.get(from * N..from * N + N) {
                    out.copy_from_slice(src);
                }
            }
        }
        blocks.blur::<N>(len, lines, plan);
        for (line, row) in blocks
            .other
            .chunks_exact(len * N)
            .zip(rows.chunks_exact_mut(row_bytes))
        {
            if let Some(middle) = line.get(reach * N..(reach + width) * N) {
                row.copy_from_slice(middle);
            }
        }
    }
}

/// The vertical half of the mirrored blur of `data` (`width` by `height`
/// pixels).
fn blur_columns_mirrored<const N: usize>(
    data: &mut [u8],
    (width, height): (usize, usize),
    plan: &MirrorPlan,
    blocks: &mut Blocks,
) {
    let map = mirror_map(height, plan.reach);
    if map.len() * width <= MAX_PADDED_PIXELS {
        blur_columns_padded::<N>(data, (width, height), plan, &map);
    } else {
        blur_columns_blocked::<N>(data, (width, height), plan, &map, blocks);
    }
}

/// The columns of an image that fits [`MAX_PADDED_PIXELS`] once extended:
/// the rows are copied mirrored above and below the image (`map` says
/// which), and the vertical passes (which work on whole rows) run over the
/// extended image.
fn blur_columns_padded<const N: usize>(
    data: &mut [u8],
    (width, height): (usize, usize),
    plan: &MirrorPlan,
    map: &[usize],
) {
    let row_bytes = width * N;
    let mut padded = vec![0_u8; map.len() * row_bytes];
    for (out, &y) in padded.chunks_exact_mut(row_bytes).zip(map) {
        if let Some(src) = data.get(y * row_bytes..(y + 1) * row_bytes) {
            out.copy_from_slice(src);
        }
    }
    let mut result = vec![0_u8; padded.len()];
    let mut sums = vec![0_u32; row_bytes];
    let rows = map.len();
    let [first, second, third] = plan.passes;
    vertical::<N>(&padded, &mut result, width, rows, first, &mut sums);
    vertical::<N>(&result, &mut padded, width, rows, second, &mut sums);
    vertical::<N>(&padded, &mut result, width, rows, third, &mut sums);
    if let (Some(middle), Some(image)) = (
        result.get(plan.reach * row_bytes..(plan.reach + height) * row_bytes),
        data.get_mut(..height * row_bytes),
    ) {
        image.copy_from_slice(middle);
    }
}

/// The columns of an image that is too large to extend: a line is a
/// column, and a block takes [`BLOCK`] neighbors (`map` says which pixel
/// each position of the extended column mirrors).
fn blur_columns_blocked<const N: usize>(
    data: &mut [u8],
    (width, height): (usize, usize),
    plan: &MirrorPlan,
    map: &[usize],
    blocks: &mut Blocks,
) {
    let len = map.len();
    for x0 in (0..width).step_by(BLOCK) {
        let lines = BLOCK.min(width - x0);
        for (i, &y) in map.iter().enumerate() {
            let Some(row) = data.get((y * width + x0) * N..(y * width + x0 + lines) * N) else {
                continue;
            };
            for (k, px) in row.as_chunks::<N>().0.iter().enumerate() {
                if let Some(out) = blocks
                    .line
                    .get_mut((k * len + i) * N..(k * len + i + 1) * N)
                {
                    out.copy_from_slice(px);
                }
            }
        }
        blocks.blur::<N>(len, lines, plan);
        for k in 0..lines {
            for y in 0..height {
                let from = (k * len + plan.reach + y) * N;
                let to = (y * width + x0 + k) * N;
                if let (Some(src), Some(dst)) =
                    (blocks.other.get(from..from + N), data.get_mut(to..to + N))
                {
                    dst.copy_from_slice(src);
                }
            }
        }
    }
}

/// The lines that a block of the mirrored blur holds.
const BLOCK: usize = 16;

/// The most pixels of the extended image that the vertical passes of the
/// mirrored blur work on (16 MiB for each of the two buffers); a larger
/// image is blurred column by column in blocks, which is slower.
const MAX_PADDED_PIXELS: usize = 4 * 1024 * 1024;

/// For each position of a line of `n` pixels extended by `reach` on both
/// sides, the pixel that it mirrors.
fn mirror_map(n: usize, reach: usize) -> Vec<usize> {
    (0..n + 2 * reach)
        .map(|i| mirror_index(i as i64 - reach as i64, n))
        .collect()
}

/// The three box passes over `lines` lines of `len` pixels each, one after
/// the other in `line`; the result is in `other`.
fn blur_block<const N: usize>(
    line: &mut [u8],
    other: &mut [u8],
    len: usize,
    lines: usize,
    passes: &[BoxPass; 3],
) {
    horizontal::<N>(line, other, len, lines, passes[0]);
    horizontal::<N>(other, line, len, lines, passes[1]);
    horizontal::<N>(line, other, len, lines, passes[2]);
}

/// `drop-shadow()`: the layer over its own shadow.
fn drop_shadow(
    data: &mut [u8],
    scratch: &mut [u8],
    width: usize,
    height: usize,
    shadow: &Shadow,
    edge: Edge,
) {
    let pixels = width * height;
    let mut alpha: Vec<u8> = data.as_chunks::<4>().0.iter().map(|px| px[3]).collect();
    // The mirrored blur needs no scratch plane (see `apply_with`).
    let mut no_plane: [u8; 0] = [];
    let plane = match edge {
        Edge::Mirror => Some(&mut no_plane[..]),
        Edge::Transparent => scratch.get_mut(..pixels),
    };
    if let Some(plane) = plane {
        box_blur::<1>(&mut alpha, plane, width, height, shadow.sigma, edge);
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
