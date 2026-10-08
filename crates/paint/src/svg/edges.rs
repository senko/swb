//! The cost of filling or stroking a path with tiny-skia, from the extents
//! of the path's edges (ADR 0023, ADR 0011).
//!
//! The cost is a model fitted to timings of `Pixmap::fill_path` and
//! `stroke_path` (ADR 0023, part 3). It does not describe how tiny-skia
//! works. The timings show:
//!
//! - The time grows with the *edge rows*: the sum over edges of the rows
//!   that they cross. A row of a curve costs more than a row of a line,
//!   and every edge has a cost of its own.
//! - Edges that cross cost more: up to 3 ns per pair of random curves that
//!   cross each other. Pairs that overlap in y but do not cross cost almost
//!   nothing (a zigzag of 25,600 lines that all overlap in y takes 17 ns per
//!   edge row and nothing for the pairs).
//! - A stroke of one device pixel or less (a hairline) takes time in
//!   proportion to its length along the longer axis, and does not depend on
//!   pairs.
//!
//! Two edges can cross only if their bounding boxes overlap in x and in y.
//! The number of such pairs (`box_pairs`) is an upper bound on the pairs
//! that cross. [`EdgeSweep`] counts it in O(n log n) without listing the
//! pairs: it sweeps over y and counts, with Fenwick trees over x, how many
//! of the edges that are active when an edge starts overlap its x range.
//! The pairs that overlap in y alone (`row_pairs`) get a small weight of
//! their own.
//!
//! A path of 40,000 curves across the full height of 100 rows has 600
//! million pairs of boxes and takes 2.6 s. A noisy line chart of 40,000
//! segments in a path 1,200 px wide has 540 million pairs that overlap in
//! y but only 7 million pairs of boxes with a 2 px stroke (and takes 1.3 s),
//! and takes 0.24 s with a 1 px stroke (a hairline).
//!
//! An anti-aliased fill whose spans are narrower than a pixel, such as a
//! comb of separate thin teeth or the area under a noisy chart, takes time
//! that neither count sees: 3,000 teeth 0.2 px wide across 1,200 x 780 px
//! take 6 s. `spans.rs` charges it, from the bound [`Edges::span_bound`]
//! and a count of the spans at sample rows.

/// A point (x, y).
pub(crate) type Point = (f32, f32);

/// A window of rows (top, bottom) and columns (left, right).
pub(crate) type Window = ((f32, f32), (f32, f32));

/// The weights of the model for filling or stroking a path in one way, in
/// the units of `cost.rs` (0.3 ns each). The values are from the
/// measurements in ADR 0023, rounded up to cover the slowest case of each
/// kind.
struct Weights {
    /// One row of a straight edge (measured: 17 to 55 ns).
    line_row: f64,
    /// A straight edge (measured: up to 270 ns).
    line_part: f64,
    /// One row of a curve (measured: 60 to 300 ns).
    curve_row: f64,
    /// A monotone part of a curve.
    curve_part: f64,
    /// A pair of edges that overlap in y.
    row_pair: f64,
    /// A pair of edges whose boxes overlap (measured: up to 3 ns for
    /// random curves that cross each other).
    box_pair: f64,
}

/// A fill with anti-aliasing.
const FILL: Weights = Weights {
    line_row: 190.0,
    line_part: 900.0,
    curve_row: 210.0,
    curve_part: 600.0,
    row_pair: 1.0,
    box_pair: 30.0,
};
/// A fill without anti-aliasing.
const FILL_NO_AA: Weights = Weights {
    line_row: 60.0,
    line_part: 400.0,
    curve_row: 120.0,
    curve_part: 400.0,
    row_pair: 1.0,
    box_pair: 20.0,
};
/// A stroke wider than a pixel, with anti-aliasing: the outline has more
/// edges than the path.
const STROKE: Weights = Weights {
    line_row: 300.0,
    line_part: 300.0,
    curve_row: 1000.0,
    curve_part: 1500.0,
    row_pair: 3.0,
    box_pair: 28.0,
};
/// A stroke wider than a pixel, without anti-aliasing.
const STROKE_NO_AA: Weights = Weights {
    line_row: 100.0,
    line_part: 500.0,
    curve_row: 700.0,
    curve_part: 4000.0,
    row_pair: 2.0,
    box_pair: 25.0,
};

/// The weights of the model for a hairline.
struct HairlineWeights {
    /// One pixel along the longer axis (measured: 18 to 60 ns, up to
    /// 400 ns without anti-aliasing).
    pixel: f64,
    /// A straight segment.
    line_part: f64,
    /// A monotone part of a curve, on top of its pixels (measured: up to
    /// 3 us per curve, pixels included).
    curve_part: f64,
}

/// A hairline with anti-aliasing.
const HAIRLINE: HairlineWeights = HairlineWeights {
    pixel: 200.0,
    line_part: 300.0,
    curve_part: 3500.0,
};
/// A hairline without anti-aliasing.
const HAIRLINE_NO_AA: HairlineWeights = HairlineWeights {
    pixel: 1300.0,
    line_part: 1500.0,
    curve_part: 16000.0,
};

/// The cost of one line that a curve is cut into for a fill without
/// anti-aliasing (measured: 110 to 200 ns, including the edge setup).
pub(crate) const FLAT_LINE: f64 = 500.0;
/// The cost of counting one segment of a path (measured: up to 270 ns).
pub(crate) const SWEEP_SEGMENT: f64 = 1000.0;
/// The most bins that a sweep uses in each direction.
const MAX_BINS: usize = 1 << 16;
/// The most lines that a curve is cut into for a fill without
/// anti-aliasing.
pub(crate) const FLAT_STEPS: u32 = 64;

/// One segment of a path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Segment {
    Move(Point),
    Line(Point),
    Quad(Point, Point),
    Cubic(Point, Point, Point),
    Close,
}

/// The edges of one kind (straight or curved).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Rows {
    /// The sum over edges of the height that they cover in the window (a
    /// curve: the sum of the heights of its monotone parts).
    pub(crate) height: f64,
    /// The sum over edges of their length along the longer axis, inside
    /// the window: what a hairline stroke draws.
    pub(crate) length: f64,
    /// The number of edges (monotone parts) that reach into the window:
    /// each crosses at least one row, and a height of `h` crosses up to
    /// `h + 1`.
    pub(crate) parts: f64,
}

/// The numbers that [`EdgeSweep`] counts.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Edges {
    pub(crate) lines: Rows,
    pub(crate) curves: Rows,
    /// The lines that the curves are cut into for a fill without
    /// anti-aliasing ([`flat_steps`]).
    pub(crate) flat_lines: f64,
    /// The pairs of edges that overlap in y.
    pub(crate) row_pairs: f64,
    /// The pairs of edges whose bounding boxes overlap in x and y: an
    /// upper bound on the pairs that cross.
    pub(crate) box_pairs: f64,
    /// A bound on the groups of spans of a fill, summed over rows (see
    /// `spans.rs`).
    pub(crate) span_bound: f64,
}

/// The work of a scan conversion, in the units of `cost.rs`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Work {
    /// The part that grows with the rows that the edges cross (and so with
    /// the size of the rendering).
    pub(crate) rows: f64,
    /// The rest: the edges themselves and the pairs.
    pub(crate) fixed: f64,
}

impl Work {
    /// All of it.
    pub(crate) fn total(&self) -> f64 {
        self.rows + self.fixed
    }
}

impl std::ops::Add for Work {
    type Output = Work;
    fn add(self, other: Work) -> Work {
        Work {
            rows: self.rows + other.rows,
            fixed: self.fixed + other.fixed,
        }
    }
}

/// How tiny-skia draws a path, as far as the cost is concerned.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Draw {
    /// A fill.
    Fill,
    /// A stroke wider than one device pixel.
    Stroke,
    /// A stroke of one device pixel or less. Its time follows its length,
    /// not an outline (measured: a 1 px stroke of a chart takes a fifth of
    /// the time of a 2 px stroke).
    Hairline,
}

impl Edges {
    /// The work of drawing the edges in the way `draw`.
    pub(crate) fn work(&self, anti_alias: bool, draw: Draw) -> Work {
        if draw == Draw::Hairline {
            let w = if anti_alias { HAIRLINE } else { HAIRLINE_NO_AA };
            return Work {
                rows: (self.lines.length + self.curves.length) * w.pixel,
                fixed: self.lines.parts * w.line_part + self.curves.parts * w.curve_part,
            };
        }
        let w = match (draw, anti_alias) {
            (Draw::Fill, true) => FILL,
            (Draw::Fill, false) => FILL_NO_AA,
            (_, true) => STROKE,
            (_, false) => STROKE_NO_AA,
        };
        // A fill without anti-aliasing cuts curves into lines first.
        let flat = if draw == Draw::Fill && !anti_alias {
            FLAT_LINE * self.flat_lines
        } else {
            0.0
        };
        Work {
            rows: self.lines.height * w.line_row + self.curves.height * w.curve_row,
            fixed: self.lines.parts * w.line_part
                + self.curves.parts * w.curve_part
                + self.row_pairs * w.row_pair
                + self.box_pairs * w.box_pair
                + flat,
        }
    }
}

/// The extent of a curve in one direction: the range of its values, the
/// distance that it travels, and its monotone parts.
#[derive(Clone, Copy, Debug)]
struct Extent {
    lo: f32,
    hi: f32,
    travel: f32,
    parts: f32,
}

impl Extent {
    /// A straight edge between two values.
    fn line(a: f32, b: f32) -> Extent {
        Extent {
            lo: a.min(b),
            hi: a.max(b),
            travel: (a - b).abs(),
            parts: 1.0,
        }
    }

    /// A quadratic (3 values) or cubic (4 values) curve with these control
    /// values.
    fn curve(v: &[f32]) -> Extent {
        let (ts, n) = monotone_ts(v);
        let ts = &ts[..=n];
        let (mut lo, mut hi, mut travel, mut previous) = (v[0], v[0], 0.0, v[0]);
        for t in &ts[1..] {
            let value = if *t >= 1.0 { v[v.len() - 1] } else { at(v, *t) };
            lo = lo.min(value);
            hi = hi.max(value);
            travel += (value - previous).abs();
            previous = value;
        }
        Extent {
            lo,
            hi,
            travel,
            parts: n as f32,
        }
    }
}

/// The parameters that cut a quadratic (3 values) or cubic (4 values) curve
/// into parts that are monotone in these values: `0`, the roots of the
/// derivative in (0, 1) in order, and `1`; and the number of parts.
#[allow(clippy::many_single_char_names)] // The names of the polynomial.
fn monotone_ts(v: &[f32]) -> ([f32; 4], usize) {
    // The roots of the derivative that lie in (0, 1).
    let roots = if let [a, b, c] = *v {
        // Zero at (a - b) / (a - 2 b + c).
        [(a - b) / (a - 2.0 * b + c), f32::NAN]
    } else if let [y0, y1, y2, y3] = *v {
        // The derivative is a t² + b t + c.
        let a = y3 - 3.0 * y2 + 3.0 * y1 - y0;
        let b = 2.0 * (y2 - 2.0 * y1 + y0);
        let c = y1 - y0;
        if a.abs() > f32::EPSILON * b.abs().max(c.abs()).max(1e-30) {
            let d = b * b - 4.0 * a * c;
            if d >= 0.0 {
                let sq = d.sqrt();
                [(-b - sq) / (2.0 * a), (-b + sq) / (2.0 * a)]
            } else {
                [f32::NAN; 2]
            }
        } else {
            [-c / b, f32::NAN]
        }
    } else {
        [f32::NAN; 2]
    };
    let mut ts = [0.0f32; 4];
    let mut n = 1;
    for t in roots {
        // `!` and `<` also reject NaN.
        if t > 0.0 && t < 1.0 {
            ts[n] = t;
            n += 1;
        }
    }
    ts[n] = 1.0;
    ts[..=n].sort_by(f32::total_cmp);
    (ts, n)
}

/// The monotone parts of a quadratic (3 values) or cubic (4 values) curve
/// in these values, as ranges of the parameter.
pub(crate) fn monotone_parts(v: &[f32]) -> impl Iterator<Item = (f32, f32)> {
    let (ts, n) = monotone_ts(v);
    (0..n).map(move |i| (ts[i], ts[i + 1]))
}

/// The number of lines that a curve with these control points (device px)
/// is cut into: one per pixel of the control polygon, between 4 and
/// [`FLAT_STEPS`].
pub(crate) fn flat_steps(points: &[Point]) -> u32 {
    let length: f32 = points
        .windows(2)
        .map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1))
        .sum();
    (length.ceil().max(0.0) as u32).clamp(4, FLAT_STEPS)
}

/// The value at `t` of the Bézier curve with these control values.
#[allow(clippy::many_single_char_names)] // The names of the polynomial.
fn at(v: &[f32], t: f32) -> f32 {
    let u = 1.0 - t;
    match *v {
        [a, b, c] => u * u * a + 2.0 * u * t * b + t * t * c,
        [a, b, c, d] => u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d,
        _ => 0.0,
    }
}

/// The part of an edge that lies in the window, in bins.
#[derive(Clone, Copy, Debug)]
struct EdgeBox {
    /// The first and last bin in y.
    y0: u32,
    y1: u32,
    /// The range in x.
    x0: f32,
    x1: f32,
}

/// Collects the extents of edges that lie in a window of rows.
#[derive(Debug)]
pub(crate) struct EdgeSweep {
    /// How far the edges reach beyond their points: half the width of a
    /// stroke.
    grow: f32,
    top: f32,
    bottom: f32,
    left: f32,
    right: f32,
    /// The height of a bin.
    unit: f32,
    bins: usize,
    lines: Rows,
    curves: Rows,
    flat_lines: f64,
    boxes: Vec<EdgeBox>,
}

impl EdgeSweep {
    /// A sweep over the rows from `top` to `bottom` and the columns from
    /// `left` to `right` (not NaN; the columns only limit the length of
    /// hairlines) for about `edges` edges that reach `grow` beyond their
    /// points. The window is
    /// cut into at most `2 * edges + 64` bins (and [`MAX_BINS`]), so that
    /// the work stays linear in the number of edges; coarse bins only
    /// raise the counts of pairs.
    pub(crate) fn new(
        (top, bottom): (f32, f32),
        (left, right): (f32, f32),
        grow: f32,
        edges: usize,
    ) -> EdgeSweep {
        let span = (bottom - top).max(0.0);
        let bins = edges.saturating_mul(2).saturating_add(64).min(MAX_BINS);
        EdgeSweep {
            grow,
            top,
            bottom: top + span,
            left,
            right: left + (right - left).max(0.0),
            unit: if span > 0.0 { span / bins as f32 } else { 1.0 },
            bins,
            lines: Rows::default(),
            curves: Rows::default(),
            flat_lines: 0.0,
            boxes: Vec::with_capacity(edges.min(1 << 20)),
        }
    }

    /// Adds an edge with the extents `x` and `y`.
    fn add(&mut self, y: Extent, x: Extent, curve: bool) {
        // An edge outside the window is clipped away cheaply (and `!` also
        // catches NaN).
        let (lo, hi) = (y.lo - self.grow, y.hi + self.grow);
        if !(lo <= hi && hi >= self.top && lo <= self.bottom) {
            return;
        }
        let (clipped_lo, clipped_hi) = (lo.max(self.top), hi.min(self.bottom));
        // The part of the travel inside the window; a stroke adds its
        // width to each part.
        let travel = y.travel + 2.0 * self.grow * y.parts;
        let inside = if hi > lo {
            (clipped_hi - clipped_lo) / (hi - lo)
        } else {
            1.0
        };
        let height = travel * inside;
        // Infinite values: take the part of the window that the edge covers.
        let height = f64::from(if height.is_finite() {
            height
        } else {
            (clipped_hi - clipped_lo) * y.parts
        });
        let length = self.length(y, x, inside);
        let rows = if curve {
            &mut self.curves
        } else {
            &mut self.lines
        };
        rows.height += height;
        rows.length += length;
        rows.parts += f64::from(y.parts);
        let last = self.bins - 1;
        let bin = |v: f32| (((v - self.top) / self.unit) as usize).min(last) as u32;
        self.boxes.push(EdgeBox {
            y0: bin(clipped_lo),
            y1: bin(clipped_hi),
            x0: x.lo - self.grow,
            x1: x.hi + self.grow,
        });
    }

    /// The length of an edge along its longer axis inside the window, given
    /// the part `inside_y` of its y range that is inside.
    fn length(&self, y: Extent, x: Extent, inside_y: f32) -> f64 {
        let inside_x = if x.hi > x.lo {
            (x.hi.min(self.right) - x.lo.max(self.left)).max(0.0) / (x.hi - x.lo)
        } else if x.lo >= self.left && x.lo <= self.right {
            1.0
        } else {
            0.0
        };
        let length = y.travel.max(x.travel) * inside_y.min(inside_x);
        if length.is_finite() {
            f64::from(length)
        } else {
            // Infinite values: the longest line in the window.
            f64::from((self.bottom - self.top).max(self.right - self.left)) * f64::from(y.parts)
        }
    }

    /// A straight edge between two points.
    pub(crate) fn line(&mut self, a: Point, b: Point) {
        self.add(Extent::line(a.1, b.1), Extent::line(a.0, b.0), false);
    }

    /// A curve with these control points: monotone parts, like the edges
    /// that tiny-skia makes of it.
    fn curve(&mut self, points: &[Point]) {
        let (mut xs, mut ys) = ([0.0f32; 4], [0.0f32; 4]);
        for (i, p) in points.iter().take(4).enumerate() {
            (xs[i], ys[i]) = *p;
        }
        let n = points.len().min(4);
        let x = Extent::curve(&xs[..n]);
        self.add(Extent::curve(&ys[..n]), x, true);
        self.flat_lines += f64::from(flat_steps(points));
    }

    /// Adds the edges of a path. A fill (`close`) closes each open subpath
    /// with an edge, a stroke does not.
    pub(crate) fn path(&mut self, segments: impl IntoIterator<Item = Segment>, close: bool) {
        walk_path(segments, close, |edge| match edge {
            PathEdge::Line(a, b) => self.line(a, b),
            PathEdge::Curve(points) => self.curve(points),
        });
    }

    /// The counts. In the order of the edges' first bins, each edge counts
    /// the earlier edges that are still active at its first bin: the pairs
    /// that overlap in y, and among them those whose x ranges overlap.
    pub(crate) fn finish(self) -> Edges {
        let (row_pairs, box_pairs) = self.count_pairs();
        Edges {
            lines: self.lines,
            curves: self.curves,
            flat_lines: self.flat_lines,
            row_pairs,
            box_pairs,
            span_bound: self.span_bound(),
        }
    }

    /// A bound on the groups of spans of a fill (see `spans.rs`): a row
    /// that `a` edges cross has at most `a / 2` spans, so its groups add up
    /// to at most `(a / 2)²`. The sum over the bins of the rows of a bin
    /// times that square for the edges that reach into the bin.
    fn span_bound(&self) -> f64 {
        let mut changes = vec![0i64; self.bins + 1];
        for edge in &self.boxes {
            changes[edge.y0 as usize] += 1;
            changes[edge.y1 as usize + 1] -= 1;
        }
        let (mut active, mut sum) = (0i64, 0.0f64);
        for change in &changes[..self.bins] {
            active += change;
            sum += (active as f64 / 2.0).powi(2);
        }
        sum * f64::from(self.unit)
    }

    /// The pairs of edges that overlap in y, and those that overlap in x
    /// too.
    fn count_pairs(&self) -> (f64, f64) {
        let n = self.boxes.len();
        let columns = n.saturating_mul(2).saturating_add(64).min(MAX_BINS);
        let window = (self.left - self.grow, self.right + self.grow);
        let x = XBins::new(&self.boxes, window, columns);
        let by_start = order_by(&self.boxes, self.bins, |b| b.y0);
        let by_end = order_by(&self.boxes, self.bins, |b| b.y1);
        let (mut starts, mut ends) = (Fenwick::new(columns), Fenwick::new(columns));
        let (mut active, mut removed) = (0usize, 0usize);
        let (mut row_pairs, mut box_pairs) = (0.0f64, 0.0f64);
        for &i in &by_start {
            let edge = self.boxes[i as usize];
            // Edges that ended before this one starts leave. They started
            // earlier, so they are in.
            while let Some(&j) = by_end.get(removed) {
                let gone = self.boxes[j as usize];
                if gone.y1 >= edge.y0 {
                    break;
                }
                let (first, last) = x.range(&gone);
                starts.add(first, -1);
                ends.add(last, -1);
                active -= 1;
                removed += 1;
            }
            let (first, last) = x.range(&edge);
            // Active edges that start at or before `last` and end at or
            // after `first` overlap the x range.
            let overlap = starts.prefix(last + 1) - ends.prefix(first);
            row_pairs += active as f64;
            box_pairs += f64::from(overlap);
            starts.add(first, 1);
            ends.add(last, 1);
            active += 1;
        }
        (row_pairs, box_pairs)
    }
}

/// An edge of a path, for [`walk_path`].
pub(crate) enum PathEdge<'a> {
    /// A straight edge between two points.
    Line(Point, Point),
    /// A quadratic (3 points) or cubic (4 points) curve.
    Curve(&'a [Point]),
}

/// Calls `edge` for each edge of a path. A fill (`close`) closes each open
/// subpath with a line, a stroke does not.
pub(crate) fn walk_path(
    segments: impl IntoIterator<Item = Segment>,
    close: bool,
    mut edge: impl FnMut(PathEdge<'_>),
) {
    let (mut start, mut current) = ((0.0, 0.0), (0.0, 0.0));
    let mut open = false;
    for segment in segments {
        match segment {
            Segment::Move(p) => {
                if open && close {
                    edge(PathEdge::Line(current, start));
                }
                (start, current, open) = (p, p, false);
            }
            Segment::Line(p) => {
                edge(PathEdge::Line(current, p));
                (current, open) = (p, true);
            }
            Segment::Quad(c, p) => {
                edge(PathEdge::Curve(&[current, c, p]));
                (current, open) = (p, true);
            }
            Segment::Cubic(c1, c2, p) => {
                edge(PathEdge::Curve(&[current, c1, c2, p]));
                (current, open) = (p, true);
            }
            Segment::Close => {
                edge(PathEdge::Line(current, start));
                (current, open) = (start, false);
            }
        }
    }
    if open && close {
        edge(PathEdge::Line(current, start));
    }
}

/// The edges' indices in the order of a key of at most `bins` values.
fn order_by(boxes: &[EdgeBox], bins: usize, key: impl Fn(&EdgeBox) -> u32) -> Vec<u32> {
    let mut next = vec![0u32; bins + 1];
    for b in boxes {
        next[key(b) as usize + 1] += 1;
    }
    for k in 1..next.len() {
        next[k] += next[k - 1];
    }
    let mut order = vec![0u32; boxes.len()];
    for (i, b) in boxes.iter().enumerate() {
        let slot = &mut next[key(b) as usize];
        order[*slot as usize] = i as u32;
        *slot += 1;
    }
    order
}

/// The columns that the x ranges of the edges fall into.
struct XBins {
    left: f32,
    unit: f32,
    columns: usize,
}

impl XBins {
    /// Columns over the finite x values of `boxes`, cut to `window`: one
    /// edge far outside the window must not squeeze all the others into a
    /// few columns. Values outside the window fall into the first or the
    /// last column; that only raises the counts.
    fn new(boxes: &[EdgeBox], window: (f32, f32), columns: usize) -> XBins {
        let (mut left, mut right) = (f32::INFINITY, f32::NEG_INFINITY);
        for value in boxes.iter().flat_map(|b| [b.x0, b.x1]) {
            if value.is_finite() {
                left = left.min(value);
                right = right.max(value);
            }
        }
        (left, right) = (left.max(window.0), right.min(window.1));
        if left > right {
            (left, right) = (0.0, 0.0);
        }
        let span = right - left;
        XBins {
            left,
            unit: if span > 0.0 && span.is_finite() {
                span / columns as f32
            } else {
                1.0
            },
            columns,
        }
    }

    /// The first and last column of the x range of an edge. A range that
    /// is not a number covers all columns.
    fn range(&self, edge: &EdgeBox) -> (usize, usize) {
        let last = self.columns - 1;
        if edge.x0.is_nan() || edge.x1.is_nan() {
            return (0, last);
        }
        let column = |v: f32| (((v - self.left) / self.unit) as usize).min(last);
        (column(edge.x0), column(edge.x1))
    }
}

/// A Fenwick tree: counts per column with prefix sums.
struct Fenwick(Vec<i32>);

impl Fenwick {
    fn new(columns: usize) -> Fenwick {
        Fenwick(vec![0; columns + 1])
    }

    /// Adds `delta` to a column.
    fn add(&mut self, column: usize, delta: i32) {
        let mut i = column + 1;
        while i < self.0.len() {
            self.0[i] += delta;
            i += i.isolate_lowest_one();
        }
    }

    /// The sum of the first `count` columns.
    fn prefix(&self, count: usize) -> i32 {
        let (mut i, mut sum) = (count.min(self.0.len() - 1), 0);
        while i > 0 {
            sum += self.0[i];
            i -= i.isolate_lowest_one();
        }
        sum
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Columns wide enough for any test edge.
    const WIDE: (f32, f32) = (-1e30, 1e30);

    /// The pairs of edges by brute force: those that overlap in y, and
    /// those that overlap in x and y.
    fn brute(boxes: &[(f32, f32, f32, f32)]) -> (f64, f64) {
        let (mut rows, mut both) = (0.0, 0.0);
        for (i, a) in boxes.iter().enumerate() {
            for b in &boxes[i + 1..] {
                if a.2 <= b.3 && b.2 <= a.3 {
                    rows += 1.0;
                    if a.0 <= b.1 && b.0 <= a.1 {
                        both += 1.0;
                    }
                }
            }
        }
        (rows, both)
    }

    #[test]
    fn counts_pairs_that_overlap() {
        let mut sweep = EdgeSweep::new((0.0, 100.0), WIDE, 0.0, 4);
        for _ in 0..4 {
            sweep.line((10.0, 0.0), (10.0, 100.0));
        }
        let edges = sweep.finish();
        // Six pairs of four edges.
        assert_eq!(edges.row_pairs, 6.0);
        assert_eq!(edges.box_pairs, 6.0);
        assert!((edges.lines.height - 400.0).abs() < 1e-6, "{edges:?}");
        assert_eq!(edges.lines.parts, 4.0);
    }

    #[test]
    fn edges_that_overlap_in_y_only_are_not_box_pairs() {
        // Vertical edges side by side: all overlap in y, none in x.
        let mut sweep = EdgeSweep::new((0.0, 100.0), WIDE, 0.0, 100);
        for i in 0..50 {
            sweep.line((i as f32, 0.0), (i as f32, 100.0));
        }
        let edges = sweep.finish();
        assert_eq!(edges.row_pairs, 50.0 * 49.0 / 2.0);
        assert_eq!(edges.box_pairs, 0.0);
    }

    #[test]
    fn a_far_edge_does_not_merge_the_columns() {
        // Vertical edges 24 px apart in a window 1,200 px wide, and one
        // horizontal edge far beyond the window on both sides: only the
        // pairs with the far edge overlap in x.
        let mut sweep = EdgeSweep::new((0.0, 100.0), (0.0, 1200.0), 0.0, 51);
        for i in 0..50 {
            let x = i as f32 * 24.0;
            sweep.line((x, 0.0), (x, 100.0));
        }
        sweep.line((-1e7, 10.0), (1e7, 10.0));
        let edges = sweep.finish();
        assert_eq!(edges.row_pairs, 51.0 * 50.0 / 2.0);
        assert_eq!(edges.box_pairs, 50.0);
    }

    #[test]
    fn edges_apart_do_not_pair_up() {
        let mut sweep = EdgeSweep::new((0.0, 100.0), WIDE, 0.0, 100);
        for i in 0..50 {
            let y = i as f32 * 2.0;
            sweep.line((0.0, y), (1.0, y + 1.0));
        }
        let edges = sweep.finish();
        assert_eq!((edges.row_pairs, edges.box_pairs), (0.0, 0.0));
    }

    #[test]
    fn matches_a_brute_force_count() {
        let mut state = 7u32;
        let mut random = move || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) as f32 / (1u32 << 24) as f32
        };
        let mut sweep = EdgeSweep::new((0.0, 1000.0), WIDE, 0.0, 300);
        let mut boxes = vec![];
        for _ in 0..300 {
            let (x, y) = (random() * 1000.0, random() * 1000.0);
            let (w, h) = (random() * 100.0, random() * 100.0);
            sweep.line((x, y), (x + w, y + h));
            boxes.push((x, x + w, y, y + h));
        }
        let (rows, both) = brute(&boxes);
        let edges = sweep.finish();
        // Bins are finer than the boxes, so the count is exact up to the
        // pairs that share a bin or a column.
        assert!(edges.row_pairs >= rows && edges.row_pairs <= rows * 1.1 + 20.0);
        assert!(edges.box_pairs >= both && edges.box_pairs <= both * 1.2 + 20.0);
    }

    #[test]
    fn invisible_and_odd_edges_are_ignored() {
        let mut sweep = EdgeSweep::new((0.0, 10.0), WIDE, 0.0, 4);
        sweep.line((0.0, 20.0), (0.0, 30.0));
        sweep.line((0.0, -30.0), (0.0, -20.0));
        sweep.line((f32::NAN, f32::NAN), (1.0, 5.0));
        sweep.line((0.0, 1.0), (0.0, f32::INFINITY));
        // Two edges reach the window.
        let edges = sweep.finish();
        assert_eq!(edges.lines.parts, 1.0 + 1.0, "{edges:?}");
    }

    #[test]
    fn curves_count_with_their_exact_range_and_parts() {
        // y(t) = 3 t (1 - t) * 4 / 3 peaks at 1 in the middle (controls 4/3,
        // 4/3): range 0..1, two monotone parts of height 1.
        let mut sweep = EdgeSweep::new((-10.0, 10.0), WIDE, 0.0, 4);
        let c = 4.0 / 3.0;
        sweep.curve(&[(0.0, 0.0), (0.0, c), (0.0, c), (0.0, 0.0)]);
        let edges = sweep.finish();
        assert!((edges.curves.height - 2.0).abs() < 1e-4, "{edges:?}");
        assert_eq!(edges.curves.parts, 2.0);
        // A monotone cubic is one edge, however far its control points go
        // (those between 0 and 10 only): the range is the end values.
        let mut sweep = EdgeSweep::new((-10.0, 20.0), WIDE, 0.0, 4);
        sweep.curve(&[(0.0, 0.0), (0.0, 3.0), (0.0, 6.0), (0.0, 10.0)]);
        sweep.curve(&[(0.0, 0.0), (0.0, 5.0), (0.0, 10.0)]);
        let edges = sweep.finish();
        assert!((edges.curves.height - 20.0).abs() < 1e-4, "{edges:?}");
        assert_eq!(edges.curves.parts, 2.0);
        // A quadratic with a turn: 0 -> -1 -> 0 is two parts of height 0.5.
        let mut sweep = EdgeSweep::new((-10.0, 10.0), WIDE, 0.0, 4);
        sweep.curve(&[(0.0, 0.0), (0.0, -1.0), (0.0, 0.0)]);
        let edges = sweep.finish();
        assert!((edges.curves.height - 1.0).abs() < 1e-4, "{edges:?}");
        assert_eq!(edges.curves.parts, 2.0);
    }

    #[test]
    fn curves_use_their_exact_x_range() {
        // The control points reach x = 100, the curve only 75.
        let mut sweep = EdgeSweep::new((0.0, 10.0), WIDE, 0.0, 4);
        sweep.curve(&[(0.0, 0.0), (100.0, 3.0), (100.0, 6.0), (0.0, 10.0)]);
        sweep.line((80.0, 0.0), (80.0, 10.0));
        sweep.line((74.0, 0.0), (74.0, 10.0));
        assert_eq!(sweep.finish().box_pairs, 1.0);
    }

    #[test]
    fn paths_close_their_subpaths() {
        let mut sweep = EdgeSweep::new((0.0, 10.0), WIDE, 0.0, 8);
        let path = [
            Segment::Move((0.0, 0.0)),
            Segment::Line((5.0, 0.0)),
            Segment::Line((5.0, 5.0)),
            Segment::Move((1.0, 1.0)),
            Segment::Line((2.0, 2.0)),
            Segment::Close,
        ];
        sweep.path(path, true);
        // Two lines and the closing edge of the first subpath, the line
        // and the closing edge of the second.
        assert_eq!(sweep.finish().lines.parts, 5.0);
        // A stroke does not close the first subpath.
        let mut sweep = EdgeSweep::new((0.0, 10.0), WIDE, 0.0, 8);
        sweep.path(path, false);
        assert_eq!(sweep.finish().lines.parts, 4.0);
    }

    #[test]
    fn curves_with_odd_values_do_not_panic() {
        let mut sweep = EdgeSweep::new((-10.0, 10.0), WIDE, 1.0, 8);
        for y in [
            [f32::NAN, 0.0, 1.0, 2.0],
            [f32::INFINITY, 0.0, 1.0, f32::NEG_INFINITY],
            [1e30, -1e30, 1e30, -1e30],
            [0.0, 0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0, 2.0],
        ] {
            let p = |i: usize| (y[(i + 1) % 4], y[i]);
            sweep.curve(&[p(0), p(1), p(2), p(3)]);
            sweep.curve(&[p(0), p(1), p(3)]);
        }
        let edges = sweep.finish();
        assert!(!edges.curves.height.is_nan() && !edges.box_pairs.is_nan());
    }

    #[test]
    fn huge_windows_use_few_bins() {
        let mut sweep = EdgeSweep::new((-1e30, 1e30), WIDE, 0.0, 2);
        sweep.line((0.0, -1e29), (1e30, 1e29));
        sweep.line((0.0, -5.0), (0.0, 5.0));
        let edges = sweep.finish();
        assert!(edges.lines.height.is_finite() && edges.box_pairs.is_finite());
        let mut empty = EdgeSweep::new((5.0, 5.0), WIDE, 0.0, 0);
        empty.line((0.0, 5.0), (0.0, 5.0));
        assert_eq!(empty.finish().lines.parts, 1.0);
    }
}
