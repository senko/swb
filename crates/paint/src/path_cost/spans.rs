//! The extra cost of anti-aliased fills whose spans are narrower than a
//! pixel (ADR 0023, part 3).
//!
//! The model is fitted to timings of tiny-skia's `Pixmap::fill_path` with
//! anti-aliasing (ADR 0023, part 3); it does not describe how tiny-skia
//! works. The timings show:
//!
//! - Coverage has a resolution of a quarter pixel in x and in y: the alpha
//!   of a thin span steps by 64 per quarter pixel, and a span whose ends
//!   round to the same quarter draws nothing.
//! - A row can take much longer than its edges explain when many of its
//!   *spans* (the runs of the row that are inside the path under the fill
//!   rule) lie inside one pixel and cover only a part of it (*inside
//!   spans*). A span that crosses a pixel boundary (a *break*) ends a
//!   *group* of inside spans. The time of a row grows with the inside
//!   spans of each group times the pixels that they touch: up to 5.3 ns per
//!   pair. A comb of 300 teeth 0.4 px wide and 4 px apart takes 0.48 ms per
//!   row; 3,000 teeth across 1,200 px take 7.7 ms per row (6 s for 780
//!   rows). Teeth of a whole pixel, or 0.5 px wide at random positions (so
//!   that some cross a pixel boundary), take 0.01 to 0.06 ms per row.
//! - Groups apart from each other in a row add up as one group; spans
//!   outside the pixmap cost nothing. A slanted span touches more pixels in
//!   a row: the model counts the pixels from its start minus the slope of
//!   its left edge to its end plus the slope of its right edge.
//! - Without anti-aliasing such fills are cheap, and so are strokes.
//!
//! Whether a span end rounds up or down to a quarter pixel, and where in
//! a row the samples lie, is not known to the model. A span counts as a
//! break only if it crosses a pixel boundary for every rounding of its ends
//! and every position of a sample in the row (a margin of an eighth of a
//! pixel, plus the slope of the edge over an eighth of a row); as empty or
//! as a whole pixel only if its ends round surely; else as inside.
//!
//! [`SpanCount`] finds the spans at sample rows: it computes where every
//! edge crosses the sample rows, sorts the crossings by x and follows the
//! winding. The samples are spaced so that the crossings stay within a
//! limit that is linear in the number of edges. An edge that crosses no
//! sample row (shorter than the spacing) is charged as if it added a span
//! to the largest group of the samples in every row that it crosses.
//!
//! A cheap bound comes first: a row with `a` edges has at most `a / 2`
//! spans, so the work is at most `(a / 2)²` per row
//! ([`Edges::span_bound`](super::edges::Edges)). The charge is the smaller
//! of the bound and the count; if the bound is small, or counting costs
//! more than half of it or more than the rest of the budget, the bound is
//! charged and nothing is counted. The count's own work is charged before
//! it runs.

use swb_layout::bezier::{at_f64, derivative_f64};

use super::edges::{Edges, PathEdge, Rows, Segment, Window, Xy, monotone_parts, walk_path};

/// The work of one pair of an inside span and a pixel of its group in a
/// row, in the units of `path_cost` (0.3 ns each; measured: up to 5.3 ns).
pub(crate) const SPAN: f64 = 20.0;
/// A bound on the work of the spans below which the spans are not counted:
/// the bound is charged instead (0.6 ms).
pub(crate) const SPAN_SKIP: f64 = 2.0e6;
/// The work of the count for one straight edge and for each of its
/// crossings with a sample row, in units (measured: 30 to 45 ns).
const LINE_COUNT: f64 = 160.0;
/// The same for a monotone part of a curve (measured: 75 to 190 ns).
const CURVE_COUNT: f64 = 700.0;
/// The crossings that a count may compute per edge, and in any case.
const CROSSINGS_PER_PART: f64 = 8.0;
const MIN_CROSSINGS: f64 = 4096.0;
/// The most crossings of a count (16 bytes each).
const MAX_CROSSINGS: f64 = 4.0e6;
/// The most sample rows of a count.
const MAX_SAMPLES: f64 = 65_536.0;
/// The finest spacing of sample rows: a quarter row.
const MIN_SPACING: f64 = 0.25;
/// How far rounding to quarter pixels moves a span end.
const ROUNDING: f64 = 0.125;
/// A margin for errors of floating-point arithmetic, in pixels.
const EPSILON: f64 = 1.0 / 64.0;
/// The steps of the search for the parameter of a curve at a sample row.
const BISECTION: usize = 22;
/// The slope of an edge with no height at a crossing.
const FLAT: f64 = 1.0e6;
/// The number of scales at which an SVG image's spans are counted: the
/// largest scale and its halves down to `2^-16` of it ([`scale`]).
pub(crate) const SCALES: usize = 17;

/// The scale (device pixels per user unit) of index `i` of [`SCALES`] for
/// the `largest` scale.
pub(crate) fn scale(largest: f64, i: usize) -> f64 {
    largest * 2f64.powi(i as i32 - (SCALES as i32 - 1))
}

/// The fill rule of a path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Rule {
    NonZero,
    EvenOdd,
}

impl From<swb_style::FillRule> for Rule {
    fn from(rule: swb_style::FillRule) -> Rule {
        match rule {
            swb_style::FillRule::NonZero => Rule::NonZero,
            swb_style::FillRule::EvenOdd => Rule::EvenOdd,
        }
    }
}

impl From<usvg::FillRule> for Rule {
    fn from(rule: usvg::FillRule) -> Rule {
        match rule {
            usvg::FillRule::NonZero => Rule::NonZero,
            usvg::FillRule::EvenOdd => Rule::EvenOdd,
        }
    }
}

/// Where the pixel grid lies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Grid {
    /// The coordinates are device pixels.
    Device,
    /// The coordinates are scaled by one of the [`SCALES`] below this
    /// largest one and moved by an unknown offset (an SVG image, estimated
    /// before its size is known). A span is a break only if it is wide
    /// enough to cross a pixel boundary wherever it lies, and a group
    /// touches as many pixels as it has spans.
    Scaled(f64),
}

/// The work of the spans of an anti-aliased fill in device pixels, in units:
/// the bound of `edges` if it is small, else the count (its own work
/// included) if it fits into `room`, else the bound. `segments` gives the
/// path's segments again for the count; `rows` and `columns` are the window
/// of the sweep that made `edges`.
pub(crate) fn device_work<I: IntoIterator<Item = Segment>>(
    edges: &Edges,
    (rows, columns): Window,
    rule: Rule,
    room: f64,
    segments: impl FnOnce() -> I,
) -> f64 {
    let bound = SPAN * edges.span_bound;
    if bound.is_nan() || bound <= SPAN_SKIP {
        return bound;
    }
    let mut count = SpanCount::new(rows, columns, edges, rule, Grid::Device);
    let own = count.work();
    // Counting would save little against the bound, or not fit: the bound
    // is charged (and rejects the path if it does not fit either).
    if 2.0 * own >= bound || own > room {
        return bound;
    }
    count.path(segments());
    own + count
        .finish()
        .map_or(bound, |groups| (SPAN * groups[0]).min(bound))
}

/// What a span is, for the cost of its row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// It surely crosses a pixel boundary: it ends a group.
    Break,
    /// It surely draws nothing or covers whole pixels only.
    Neutral,
    /// It may lie inside one pixel and cover a part of it.
    Inside,
}

/// Where an edge crosses a sample row.
#[derive(Clone, Copy, Debug)]
struct Crossing {
    sample: u32,
    x: f32,
    /// The absolute slope `|dx/dy|` of the edge there.
    slope: f32,
    /// +1 for an edge downwards, -1 upwards.
    winding: i8,
}

/// The counts of a [`SpanCount`]: per scale (one for [`Grid::Device`]),
/// the sum over rows of the inside spans of each group times its pixels.
pub(crate) type Groups = [f64; SCALES];

/// Counts the groups of inside spans of a fill at sample rows.
#[derive(Debug)]
pub(crate) struct SpanCount {
    rule: Rule,
    grid: Grid,
    top: f64,
    bottom: f64,
    left: f64,
    right: f64,
    /// The rows between sample rows.
    spacing: f64,
    samples: usize,
    crossings: Vec<Crossing>,
    /// The most crossings; the count gives up beyond.
    limit: usize,
    overflow: bool,
    /// Per gap before sample `k` (and after the last one): the edges that
    /// cross no sample row in it, and the rows that they cross.
    unseen: Vec<(f64, f64)>,
    /// The work of the count, see [`SpanCount::work`].
    work: f64,
}

impl SpanCount {
    /// A count over the rows from `top` to `bottom` and the columns from
    /// `left` to `right` (in device pixels, or user units for
    /// [`Grid::Scaled`]) of the edges that `edges` describes.
    pub(crate) fn new(
        (top, bottom): (f32, f32),
        (left, right): (f32, f32),
        edges: &Edges,
        rule: Rule,
        grid: Grid,
    ) -> SpanCount {
        let (top, bottom) = (f64::from(top), f64::from(bottom).max(f64::from(top)));
        let parts = edges.lines.parts + edges.curves.parts;
        let height = edges.lines.height + edges.curves.height;
        let target = CROSSINGS_PER_PART * parts + MIN_CROSSINGS;
        // Each edge crosses about `height / spacing` sample rows, and one
        // more at most.
        let spacing = (height / target)
            .max((bottom - top) / MAX_SAMPLES)
            .max(MIN_SPACING);
        let spacing = if spacing.is_finite() {
            spacing
        } else {
            f64::MAX
        };
        // Samples in the middle of equal parts of the window. A window that
        // is not finite (an image at `scale(1e20)`) gets none: every edge
        // is then unseen and charged for all its rows.
        let samples = if (bottom - top).is_finite() {
            ((bottom - top) / spacing).ceil().min(MAX_SAMPLES) as usize
        } else {
            0
        };
        let spacing = if samples > 0 {
            (bottom - top) / samples as f64
        } else {
            1.0
        };
        // An edge of height `h` crosses at most `h / spacing + 1` sample
        // rows, one more for the rounding of the rows to quarters.
        let crossings = |rows: &Rows| rows.height / spacing + 2.0 * rows.parts;
        let limit = (crossings(&edges.lines) + crossings(&edges.curves)).min(MAX_CROSSINGS);
        // The work of an edge and of each of its crossings.
        let work = LINE_COUNT * (edges.lines.parts + crossings(&edges.lines))
            + CURVE_COUNT * (edges.curves.parts + crossings(&edges.curves));
        SpanCount {
            rule,
            grid,
            top,
            bottom,
            left: f64::from(left),
            right: f64::from(right).max(f64::from(left)),
            spacing,
            samples,
            crossings: Vec::new(),
            limit: limit as usize,
            overflow: false,
            unseen: vec![(0.0, 0.0); samples + 1],
            work,
        }
    }

    /// The work of running the count, in units, before it runs: the
    /// edges, and the crossings that the spacing allows.
    pub(crate) fn work(&self) -> f64 {
        self.work
    }

    /// The y of sample row `k`: the middle of a quarter row for device
    /// pixels.
    fn sample_y(&self, k: usize) -> f64 {
        let y = self.top + (k as f64 + 0.5) * self.spacing;
        match self.grid {
            Grid::Device => ((y * 4.0).floor() + 0.5) / 4.0,
            Grid::Scaled(_) => y,
        }
    }

    /// Adds the edges of a fill of `segments` (each open subpath closed).
    pub(crate) fn path(&mut self, segments: impl IntoIterator<Item = Segment>) {
        walk_path(segments, true, |edge| match edge {
            PathEdge::Line(a, b) => self.line(a, b),
            PathEdge::Curve(points) => self.curve(points),
        });
    }

    /// The sample rows in `[lo, hi)`.
    fn samples_in(&self, lo: f64, hi: f64) -> std::ops::Range<usize> {
        let first = ((lo - self.top) / self.spacing - 1.5).floor().max(0.0);
        let mut k = (first as usize).min(self.samples);
        while k < self.samples && self.sample_y(k) < lo {
            k += 1;
        }
        let mut end = k;
        while end < self.samples && self.sample_y(end) < hi {
            end += 1;
        }
        k..end
    }

    /// An edge with this y range (`lo < hi`) that crosses no sample row.
    fn unseen(&mut self, lo: f64, hi: f64, samples: &std::ops::Range<usize>) {
        let rows = hi.min(self.bottom) - lo.max(self.top);
        if rows < 0.0 {
            return;
        }
        let gap = &mut self.unseen[samples.start.min(self.samples)];
        gap.0 += 1.0;
        gap.1 += rows + MIN_SPACING;
    }

    fn push(&mut self, sample: usize, x: f64, slope: f64, winding: i8) {
        if self.crossings.len() >= self.limit {
            self.overflow = true;
            return;
        }
        // The columns outside the window draw nothing; a crossing there
        // only keeps its side.
        let x = x.clamp(self.left - 1.0, self.right + 1.0);
        self.crossings.push(Crossing {
            sample: sample as u32,
            x: x as f32,
            slope: slope.min(FLAT) as f32,
            winding,
        });
    }

    /// A straight edge between two points.
    fn line(&mut self, a: Xy, b: Xy) {
        let (a, b) = (to64(a), to64(b));
        if self.overflow || !(a.1 != b.1 && a.0.is_finite() && b.0.is_finite()) {
            return;
        }
        let (lo, hi, winding) = if a.1 < b.1 { (a, b, 1) } else { (b, a, -1) };
        if !(hi.1 > self.top && lo.1 < self.bottom) {
            return;
        }
        let slope = (hi.0 - lo.0) / (hi.1 - lo.1);
        let samples = self.samples_in(lo.1, hi.1);
        if samples.is_empty() {
            self.unseen(lo.1, hi.1, &samples);
        }
        for k in samples {
            let x = lo.0 + (self.sample_y(k) - lo.1) * slope;
            self.push(k, x, slope.abs(), winding);
        }
    }

    /// A quadratic or cubic curve with these control points: its monotone
    /// parts.
    fn curve(&mut self, points: &[Xy]) {
        if self.overflow || !points.iter().all(|p| p.0.is_finite() && p.1.is_finite()) {
            return;
        }
        let curve = Curve::new(points);
        let mut ys = [0.0f32; 4];
        for (y, p) in ys.iter_mut().zip(points) {
            *y = p.1;
        }
        for (t0, t1) in monotone_parts(&ys[..curve.n]) {
            self.curve_part(&curve, f64::from(t0), f64::from(t1));
        }
    }

    /// The part of `curve` from `t0` to `t1`, monotone in y.
    fn curve_part(&mut self, curve: &Curve, t0: f64, t1: f64) {
        let (y0, y1) = (curve.y(t0), curve.y(t1));
        if y0 == y1 {
            return;
        }
        let (lo, hi, winding) = if y0 < y1 { (y0, y1, 1) } else { (y1, y0, -1) };
        if !(hi > self.top && lo < self.bottom) {
            return;
        }
        let samples = self.samples_in(lo, hi);
        if samples.is_empty() {
            self.unseen(lo, hi, &samples);
        }
        for k in samples {
            let y = self.sample_y(k);
            // Bisection: y(t) runs from y0 to y1.
            let (mut a, mut b) = (t0, t1);
            for _ in 0..BISECTION {
                let middle = f64::midpoint(a, b);
                if (curve.y(middle) < y) == (y0 < y1) {
                    a = middle;
                } else {
                    b = middle;
                }
            }
            let t = f64::midpoint(a, b);
            let (dx, dy) = curve.derivative(t);
            let slope = if dy.abs() > 0.0 {
                (dx / dy).abs()
            } else {
                FLAT
            };
            self.push(k, curve.x(t), slope, winding);
        }
    }

    /// The groups, or `None` if the crossings exceeded the limit.
    pub(crate) fn finish(mut self) -> Option<Groups> {
        if self.overflow {
            return None;
        }
        if self.samples == 0 {
            return Some([0.0; SCALES]);
        }
        self.crossings
            .sort_unstable_by(|a, b| a.sample.cmp(&b.sample).then(a.x.total_cmp(&b.x)));
        let scales = match self.grid {
            Grid::Device => 1,
            Grid::Scaled(_) => SCALES,
        };
        let mut groups = [0.0; SCALES];
        // The most inside spans of a sample, per scale.
        let mut most = [0.0f64; SCALES];
        for row in self.crossings.chunk_by(|a, b| a.sample == b.sample) {
            for (i, total) in groups.iter_mut().enumerate().take(scales) {
                let s = match self.grid {
                    Grid::Device => 1.0,
                    Grid::Scaled(largest) => scale(largest, i),
                };
                let (work, inside) = self.row(row, s);
                *total += work * self.spacing;
                most[i] = most[i].max(inside);
            }
        }
        // An edge between sample rows can add a span to a group in every
        // row that it crosses: at most the largest group of the samples,
        // plus the other unseen edges of its gap (each pair forms a span).
        for &(edges, rows) in &self.unseen {
            for i in 0..scales {
                groups[i] += (most[i] + edges / 4.0) * rows;
            }
        }
        Some(groups)
    }

    /// The groups of one sample row at scale `s`: the sum over groups of
    /// inside spans times pixels, and the number of inside spans.
    fn row(&self, crossings: &[Crossing], s: f64) -> (f64, f64) {
        let (left, right) = (self.left * s, self.right * s);
        let mut winding = 0i32;
        let mut start = (0.0, 0.0);
        let (mut work, mut inside) = (0.0, 0.0);
        let mut group = Group::default();
        for c in crossings {
            let was = self.is_inside(winding);
            winding += i32::from(c.winding);
            let x = f64::from(c.x) * s;
            let slope = f64::from(c.slope);
            if !was && self.is_inside(winding) {
                start = (x, slope);
            } else if was && !self.is_inside(winding) {
                // The window cuts the span exactly.
                let (a, da) = if start.0 < left { (left, 0.0) } else { start };
                let (b, db) = if x > right { (right, 0.0) } else { (x, slope) };
                if b <= a {
                    continue;
                }
                match self.kind((a, da), (b, db)) {
                    Kind::Break => work += group.take(),
                    Kind::Neutral => {}
                    Kind::Inside => {
                        inside += 1.0;
                        // A slanted edge moves across the row: the span
                        // touches the pixels from `a - da` to `b + db`.
                        let pixels = ((a - da).max(left).floor(), (b + db).min(right).floor());
                        group.add(pixels, self.grid);
                    }
                }
            }
        }
        (work + group.take(), inside)
    }

    fn is_inside(&self, winding: i32) -> bool {
        match self.rule {
            Rule::NonZero => winding != 0,
            Rule::EvenOdd => winding % 2 != 0,
        }
    }

    /// What a span from `a` to `b` is, with the slopes of its edges.
    fn kind(&self, (a, da): (f64, f64), (b, db): (f64, f64)) -> Kind {
        // How far a sample in the row and the rounding can move each end.
        let (ma, mb) = (ROUNDING + EPSILON + da / 8.0, ROUNDING + EPSILON + db / 8.0);
        match self.grid {
            Grid::Device => {
                // A pixel boundary `k` with `a + ma < k < b - mb`.
                if (a + ma).floor() + 1.0 < b - mb {
                    return Kind::Break;
                }
                match (sure_quarter(a, da), sure_quarter(b, db)) {
                    (Some(qa), Some(qb)) if qa == qb => Kind::Neutral,
                    (Some(qa), Some(qb)) if qa.rem_euclid(4) == 0 && qb == qa + 4 => Kind::Neutral,
                    _ => Kind::Inside,
                }
            }
            Grid::Scaled(_) => {
                if b - a > 1.0 + ma + mb {
                    Kind::Break
                } else {
                    Kind::Inside
                }
            }
        }
    }
}

/// The quarter pixel to which `v` rounds for every position within the
/// margins, if there is one.
fn sure_quarter(v: f64, slope: f64) -> Option<i64> {
    let q = 4.0 * v;
    let nearest = q.round();
    let margin = 4.0 * (EPSILON + slope / 8.0);
    ((q - nearest).abs() + margin < 0.5).then_some(nearest as i64)
}

/// The inside spans of one group so far.
#[derive(Debug, Default)]
struct Group {
    spans: f64,
    /// The pixels that they touch ([`Grid::Device`]), and the last of them.
    pixels: f64,
    last: Option<f64>,
    /// [`Grid::Scaled`]: the pixels that the spans touch one by one, and
    /// the first and last pixel of all.
    each: f64,
    from: Option<f64>,
    to: f64,
}

impl Group {
    /// Adds an inside span that touches the pixels `first..=last`.
    fn add(&mut self, (first, last): (f64, f64), grid: Grid) {
        self.spans += 1.0;
        let count = (last - first + 1.0).max(1.0);
        match grid {
            Grid::Scaled(_) => {
                self.each += count;
                self.from.get_or_insert(first);
                self.to = self.to.max(last);
            }
            Grid::Device => {
                // The pixels after the last one that the group touched.
                let new = match self.last {
                    Some(previous) => (last - previous.max(first - 1.0)).max(0.0),
                    None => count,
                };
                self.pixels += new;
                self.last = Some(self.last.map_or(last, |previous| previous.max(last)));
            }
        }
    }

    /// The work of the group, which then starts again.
    fn take(&mut self) -> f64 {
        let pixels = match self.from {
            // At unknown positions the pixels of the spans one by one, and
            // at most those of their extent plus two at up to twice the
            // scale (the next scale of a [`Grid::Scaled`]).
            Some(from) => self.each.min(2.0 * (self.to - from) + 3.0),
            None => self.pixels,
        };
        let work = self.spans * pixels;
        *self = Group::default();
        work
    }
}

fn to64(p: Xy) -> (f64, f64) {
    (f64::from(p.0), f64::from(p.1))
}

/// A quadratic or cubic Bézier curve in f64.
struct Curve {
    xs: [f64; 4],
    ys: [f64; 4],
    n: usize,
}

impl Curve {
    fn new(points: &[Xy]) -> Curve {
        let (mut xs, mut ys) = ([0.0; 4], [0.0; 4]);
        let n = points.len().min(4);
        for (i, p) in points.iter().take(4).enumerate() {
            (xs[i], ys[i]) = to64(*p);
        }
        Curve { xs, ys, n }
    }

    fn x(&self, t: f64) -> f64 {
        at_f64(&self.xs[..self.n], t)
    }

    fn y(&self, t: f64) -> f64 {
        at_f64(&self.ys[..self.n], t)
    }

    /// `(dx/dt, dy/dt)` at `t`.
    fn derivative(&self, t: f64) -> (f64, f64) {
        (
            derivative_f64(&self.xs[..self.n], t),
            derivative_f64(&self.ys[..self.n], t),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path_cost::edges::EdgeSweep;

    /// Rectangles `(x, y, width, height)` as the segments of one path.
    fn rects(rects: &[(f32, f32, f32, f32)]) -> Vec<Segment> {
        let mut segments = vec![];
        for &(x, y, w, h) in rects {
            segments.extend([
                Segment::Move((x, y)),
                Segment::Line((x, y + h)),
                Segment::Line((x + w, y + h)),
                Segment::Line((x + w, y)),
                Segment::Close,
            ]);
        }
        segments
    }

    /// A comb of `n` teeth `width` wide, `pitch` apart from `x0`, `height`
    /// tall.
    fn comb(n: usize, x0: f32, pitch: f32, width: f32, height: f32) -> Vec<Segment> {
        let teeth: Vec<_> = (0..n)
            .map(|i| (x0 + i as f32 * pitch, 0.0, width, height))
            .collect();
        rects(&teeth)
    }

    /// The edges and the groups of `segments` in a window of `rows` and
    /// `columns`.
    fn count(
        segments: &[Segment],
        rows: (f32, f32),
        columns: (f32, f32),
        rule: Rule,
        grid: Grid,
    ) -> (Edges, Option<Groups>) {
        let mut sweep = EdgeSweep::new(rows, columns, 0.0, segments.len() + 1);
        sweep.path(segments.iter().copied(), true);
        let edges = sweep.finish();
        let mut count = SpanCount::new(rows, columns, &edges, rule, grid);
        count.path(segments.iter().copied());
        (edges, count.finish())
    }

    fn device(segments: &[Segment], rows: f32, columns: f32) -> f64 {
        count(
            segments,
            (0.0, rows),
            (0.0, columns),
            Rule::NonZero,
            Grid::Device,
        )
        .1
        .expect("within the limit")[0]
    }

    #[test]
    fn thin_teeth_form_one_group() {
        // 300 teeth 0.4 px wide, 4 px apart, 100 rows: one group of 300
        // spans in 300 pixels per row.
        let groups = device(&comb(300, 0.0, 4.0, 0.4, 100.0), 100.0, 1200.0);
        let expected = 100.0 * 300.0 * 300.0;
        assert!(
            groups >= expected && groups <= expected * 1.05,
            "{groups} {expected}"
        );
        // Two teeth in a pixel: 600 spans in 300 pixels.
        let mut pairs = comb(300, 0.0, 4.0, 0.25, 100.0);
        pairs.extend(comb(300, 0.5, 4.0, 0.25, 100.0));
        let groups = device(&pairs, 100.0, 1200.0);
        assert!(groups >= 100.0 * 600.0 * 300.0, "{groups}");
    }

    #[test]
    fn whole_pixels_and_wide_spans_cost_nothing() {
        // Teeth of exactly one pixel, and teeth 2 px wide.
        assert_eq!(device(&comb(300, 0.0, 4.0, 1.0, 100.0), 100.0, 1200.0), 0.0);
        assert_eq!(device(&comb(300, 0.3, 4.0, 2.0, 100.0), 100.0, 1200.0), 0.0);
        // A span narrower than a quarter that rounds to nothing.
        assert_eq!(
            device(&comb(300, 0.0, 4.0, 0.05, 100.0), 100.0, 1200.0),
            0.0
        );
    }

    #[test]
    fn spans_that_cross_a_pixel_boundary_end_a_group() {
        // Thin teeth, each followed by a bar across a pixel boundary:
        // groups of one span.
        let mut segments = comb(300, 0.0, 4.0, 0.4, 100.0);
        segments.extend(comb(300, 1.5, 4.0, 1.0, 100.0));
        let groups = device(&segments, 100.0, 1200.0);
        assert!(groups <= 100.0 * 300.0 * 1.05, "{groups}");
        // A span that ends near a pixel boundary may round either way: it
        // does not end a group.
        let mut segments = comb(300, 0.0, 4.0, 0.4, 100.0);
        segments.extend(comb(300, 1.5, 4.0, 0.6, 100.0));
        let groups = device(&segments, 100.0, 1200.0);
        assert!(groups >= 100.0 * 600.0 * 600.0, "{groups}");
    }

    #[test]
    fn overlapping_shapes_merge_under_the_fill_rule() {
        // Each tooth twice, the second 0.25 px to the right: one span of
        // 0.75 px under the non-zero rule, two spans of 0.25 px under the
        // even-odd rule.
        let mut segments = comb(300, 0.0, 4.0, 0.5, 100.0);
        segments.extend(comb(300, 0.25, 4.0, 0.5, 100.0));
        let (rows, columns) = ((0.0, 100.0), (0.0, 1200.0));
        let nonzero = count(&segments, rows, columns, Rule::NonZero, Grid::Device);
        let evenodd = count(&segments, rows, columns, Rule::EvenOdd, Grid::Device);
        let (nonzero, evenodd) = (nonzero.1.unwrap()[0], evenodd.1.unwrap()[0]);
        assert!(evenodd >= 1.9 * nonzero, "{nonzero} {evenodd}");
    }

    #[test]
    fn edges_between_sample_rows_are_charged() {
        // 20 stacked combs of 3,000 thin teeth, 4 rows each, and one tall
        // zigzag that makes the sample rows far apart: most teeth cross no
        // sample row.
        let mut teeth = vec![];
        for j in 0..20 {
            for i in 0..3_000 {
                teeth.push((i as f32 * 0.4, j as f32 * 5.0, 0.2, 4.0));
            }
        }
        let mut segments = rects(&teeth);
        segments.push(Segment::Move((0.0, 0.0)));
        for i in 0..20_000 {
            let y = if i % 2 == 0 { 100.0 } else { 0.0 };
            segments.push(Segment::Line((i as f32 * 0.06, y)));
        }
        let window = ((0.0, 100.0), (0.0, 1200.0));
        let (edges, groups) = count(&segments, window.0, window.1, Rule::NonZero, Grid::Device);
        let groups = groups.unwrap()[0];
        // 80 rows with 3,000 teeth: at least 3,000 x 1,200 per row.
        assert!(groups >= 80.0 * 3_000.0 * 1_200.0, "{groups}");
        assert!(
            groups <= edges.span_bound * 1.01,
            "{groups} {}",
            edges.span_bound
        );
    }

    #[test]
    fn curves_are_cut_at_the_sample_rows() {
        // Teeth with curved sides, about 0.4 px wide.
        let mut segments = vec![];
        for i in 0..300 {
            let x = i as f32 * 4.0;
            segments.extend([
                Segment::Move((x, 0.0)),
                Segment::Cubic((x + 0.1, 30.0), (x - 0.1, 60.0), (x, 100.0)),
                Segment::Line((x + 0.4, 100.0)),
                Segment::Quad((x + 0.5, 50.0), (x + 0.4, 0.0)),
                Segment::Close,
            ]);
        }
        let groups = device(&segments, 100.0, 1200.0);
        assert!(groups >= 0.9 * 100.0 * 300.0 * 300.0, "{groups}");
    }

    #[test]
    fn the_count_gives_up_beyond_its_limit() {
        // A window of no rows counts nothing.
        let segments = comb(3, 0.0, 4.0, 0.4, 100.0);
        let (_, groups) = count(
            &segments,
            (50.0, 50.0),
            (0.0, 20.0),
            Rule::NonZero,
            Grid::Device,
        );
        assert_eq!(groups.map(|g| g[0]), Some(0.0));
        // The limit follows from the edges of the sweep: far more edges
        // exceed it.
        let mut sweep = EdgeSweep::new((0.0, 100.0), (0.0, 20.0), 0.0, 1);
        sweep.path(comb(1, 0.0, 4.0, 0.4, 100.0), true);
        let edges = sweep.finish();
        let mut count = SpanCount::new(
            (0.0, 100.0),
            (0.0, 20.0),
            &edges,
            Rule::NonZero,
            Grid::Device,
        );
        count.path(comb(10_000, 0.0, 0.1, 0.05, 100.0));
        assert!(count.finish().is_none());
    }

    #[test]
    fn scaled_spans_depend_on_the_scale() {
        // Teeth 0.5 units wide, 4 units apart: inside a pixel at a scale
        // of 1, wider than two pixels at 8.
        let segments = comb(300, 0.0, 4.0, 0.5, 100.0);
        let (rows, columns) = ((0.0, 100.0), (0.0, 1200.0));
        let (_, groups) = count(&segments, rows, columns, Rule::NonZero, Grid::Scaled(256.0));
        let groups = groups.unwrap();
        let at = |s: f64| groups[(s.log2() + 8.0) as usize];
        assert!(at(1.0) >= 100.0 * 300.0 * 300.0, "{groups:?}");
        assert_eq!(at(8.0), 0.0);
        // At 1/256 the comb is 5 px wide: its group touches few pixels.
        let small = at(1.0 / 256.0);
        assert!(small > 0.0 && small <= 100.0 * 300.0 * 12.0, "{groups:?}");
    }

    #[test]
    fn device_work_charges_the_bound_or_the_count() {
        let window = ((0.0, 100.0), (0.0, 1200.0));
        let work = |segments: &[Segment], room: f64| {
            let mut sweep = EdgeSweep::new(window.0, window.1, 0.0, segments.len() + 1);
            sweep.path(segments.iter().copied(), true);
            let edges = sweep.finish();
            let segments = || segments.iter().copied();
            let work = device_work(&edges, window, Rule::NonZero, room, segments);
            (work, SPAN * edges.span_bound)
        };
        // A small path: the bound.
        let (small, bound) = work(&comb(3, 0.0, 4.0, 0.4, 100.0), f64::INFINITY);
        assert_eq!(small, bound);
        assert!(small <= SPAN_SKIP);
        // Wide teeth: the count, far below the bound.
        let (wide, bound) = work(&comb(300, 0.3, 4.0, 2.0, 100.0), f64::INFINITY);
        assert!(wide < bound / 10.0, "{wide} {bound}");
        // No room for the count: the bound.
        let (no_room, bound) = work(&comb(300, 0.3, 4.0, 2.0, 100.0), 0.0);
        assert_eq!(no_room, bound);
    }

    #[test]
    fn odd_values_do_not_panic() {
        let segments = [
            Segment::Move((f32::NAN, 0.0)),
            Segment::Line((1.0, f32::INFINITY)),
            Segment::Cubic((1e30, -1e30), (f32::NAN, 1.0), (0.0, 5.0)),
            Segment::Quad((0.0, 0.0), (0.0, 0.0)),
            Segment::Line((-1e30, 3.0)),
            Segment::Close,
        ];
        for grid in [Grid::Device, Grid::Scaled(1.0)] {
            let window = ((-10.0, 10.0), (-1e30, 1e30));
            let (_, groups) = count(&segments, window.0, window.1, Rule::EvenOdd, grid);
            assert!(groups.is_none_or(|g| g.iter().all(|v| v.is_finite())));
            // Windows that are not finite (a dashed stroke in an image at
            // `scale(1e20)`) have no sample rows.
            for rows in [
                (f32::NEG_INFINITY, f32::INFINITY),
                (0.0, f32::INFINITY),
                (-3e38, 3e38),
            ] {
                count(&segments, rows, window.1, Rule::NonZero, grid);
                count(
                    &comb(10, 0.0, 1e20, 0.4, 1e21),
                    rows,
                    (0.0, 1e30),
                    Rule::NonZero,
                    grid,
                );
            }
        }
        assert_eq!(sure_quarter(0.5, 0.0), Some(2));
        assert_eq!(sure_quarter(0.125, 0.0), None);
        assert_eq!(sure_quarter(0.24, 1.0), None);
    }
}
