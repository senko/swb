//! Paths of SVG shapes in user units: path data (`d`), and the equivalent
//! paths of the basic shapes.
//!
//! Path data: <https://svgwg.org/svg2-draft/paths.html#PathData>.
//! `svgtypes` tokenizes the data; this module makes coordinates absolute,
//! expands the shorthand commands and converts elliptical arcs to cubic
//! Béziers (SVG 2 Appendix B, "Elliptical arc implementation notes",
//! <https://svgwg.org/svg2-draft/implnote.html#ArcImplementationNotes>).
//! An error in the data ends the path: the segments before it are drawn
//! (SVG 2 §9.5.4, "Error handling in path data").
//!
//! Basic shapes: <https://svgwg.org/svg2-draft/shapes.html>. Ellipses and
//! rounded corners are drawn with one cubic Bézier per quarter.

use std::sync::{Arc, OnceLock};

use svgtypes::{PathParser, PathSegment as Token, PointsParser};

use crate::geom::{Point, Rect};

/// One segment of a path, in absolute user units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathSegment {
    /// Start a new subpath.
    MoveTo(Point),
    /// A straight line to the point.
    LineTo(Point),
    /// A quadratic Bézier: control point, end point.
    QuadTo(Point, Point),
    /// A cubic Bézier: two control points, end point.
    CubicTo(Point, Point, Point),
    /// Close the current subpath.
    Close,
}

/// A path in user units with the box of its points (control points
/// included, so it contains the curves).
#[derive(Clone, Debug, PartialEq)]
pub struct SvgPath {
    segments: Vec<PathSegment>,
    bounds: Rect,
    /// [`SvgPath::fill_bounds`], computed on first use: it walks all
    /// segments, and every element that refers to a clip path asks for it.
    fill_bounds: OnceLock<Option<Rect>>,
}

impl SvgPath {
    /// A path from segments, or `None` if a coordinate is not finite or
    /// no segment draws (only moves).
    pub fn from_segments(segments: &[PathSegment]) -> Option<SvgPath> {
        let mut budget = segments.len();
        let mut b = PathBuilder::new(&mut budget);
        for segment in segments {
            if !b.push(*segment) {
                return None;
            }
        }
        b.finish()
    }

    /// The segments.
    pub fn segments(&self) -> &[PathSegment] {
        &self.segments
    }

    /// The box of all points of the path, control points included.
    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    /// The bounding box of the path's geometry, as `getBBox()` reports it
    /// (SVG 2 §8.11, the fill bounding box): curves contribute their
    /// extrema, not their control points, and a move that no segment
    /// follows contributes nothing (measured in Chromium 148: `M 10 10 L
    /// 50 50 M 80 80` is `10 10 40 40`). `None` for a path without a
    /// point.
    pub fn fill_bounds(&self) -> Option<Rect> {
        *self.fill_bounds.get_or_init(|| self.compute_fill_bounds())
    }

    fn compute_fill_bounds(&self) -> Option<Rect> {
        let mut bounds = Bounds::default();
        let mut pending: Option<Point> = None;
        let mut current = Point::default();
        let mut start = Point::default();
        for segment in &self.segments {
            if let PathSegment::MoveTo(p) = *segment {
                pending = Some(p);
                current = p;
                start = p;
                continue;
            }
            if let Some(p) = pending.take() {
                bounds.add(p);
            }
            match *segment {
                PathSegment::LineTo(p) => bounds.add(p),
                PathSegment::QuadTo(c, p) => {
                    bounds.add(p);
                    bounds.quad_extrema(current, c, p);
                }
                PathSegment::CubicTo(c1, c2, p) => {
                    bounds.add(p);
                    bounds.cubic_extrema(current, c1, c2, p);
                }
                PathSegment::Close => bounds.add(start),
                PathSegment::MoveTo(_) => {}
            }
            current = match *segment {
                PathSegment::LineTo(p)
                | PathSegment::QuadTo(_, p)
                | PathSegment::CubicTo(_, _, p) => p,
                _ => start,
            };
        }
        bounds.rect()
    }

    /// The rectangle if the path is one axis-aligned rectangle (`M L L L`
    /// with an optional last `L` back to the start and an optional `Z`),
    /// as in `M0 0h40v20H0z`.
    pub fn as_rect(&self) -> Option<Rect> {
        let mut points = Vec::with_capacity(5);
        let mut closed = false;
        for (i, segment) in self.segments.iter().enumerate() {
            match *segment {
                PathSegment::MoveTo(p) if i == 0 => points.push(p),
                PathSegment::LineTo(p) if i > 0 && !closed && points.len() < 5 => points.push(p),
                PathSegment::Close if i > 0 && !closed => closed = true,
                _ => return None,
            }
        }
        if points.len() == 5 && points.first() == points.last() {
            points.pop();
        }
        let [p0, p1, p2, p3] = points[..] else {
            return None;
        };
        let near = |a: f32, b: f32| (a - b).abs() <= 1e-4 * (1.0 + a.abs().max(b.abs()));
        let rectangular =
            (near(p0.x, p1.x) && near(p1.y, p2.y) && near(p2.x, p3.x) && near(p3.y, p0.y))
                || (near(p0.y, p1.y) && near(p1.x, p2.x) && near(p2.y, p3.y) && near(p3.x, p0.x));
        rectangular.then(|| {
            let x0 = p0.x.min(p1.x).min(p2.x).min(p3.x);
            let y0 = p0.y.min(p1.y).min(p2.y).min(p3.y);
            let x1 = p0.x.max(p1.x).max(p2.x).max(p3.x);
            let y1 = p0.y.max(p1.y).max(p2.y).max(p3.y);
            Rect::new(x0, y0, x1 - x0, y1 - y0)
        })
    }

    /// An upper bound of the path's length: the length of its control
    /// polygon.
    pub fn length_bound(&self) -> f32 {
        let mut length = 0.0_f64;
        let mut current = Point::default();
        let mut start = Point::default();
        let mut add = |from: Point, to: Point| {
            length += f64::from(to.x - from.x).hypot(f64::from(to.y - from.y));
        };
        for segment in &self.segments {
            match *segment {
                PathSegment::MoveTo(p) => {
                    current = p;
                    start = p;
                }
                PathSegment::LineTo(p) => {
                    add(current, p);
                    current = p;
                }
                PathSegment::QuadTo(c, p) => {
                    add(current, c);
                    add(c, p);
                    current = p;
                }
                PathSegment::CubicTo(c1, c2, p) => {
                    add(current, c1);
                    add(c1, c2);
                    add(c2, p);
                    current = p;
                }
                PathSegment::Close => {
                    add(current, start);
                    current = start;
                }
            }
        }
        length as f32
    }
}

/// The box of points, for [`SvgPath::fill_bounds`].
#[derive(Default)]
struct Bounds(Option<(f64, f64, f64, f64)>);

impl Bounds {
    fn add(&mut self, p: Point) {
        self.add_xy(f64::from(p.x), f64::from(p.y));
    }

    fn add_xy(&mut self, x: f64, y: f64) {
        let (x0, y0, x1, y1) = self.0.unwrap_or((x, y, x, y));
        self.0 = Some((x0.min(x), y0.min(y), x1.max(x), y1.max(y)));
    }

    /// Adds the points of the quadratic curve `p0 c p` where a coordinate
    /// has its extremum.
    #[allow(clippy::many_single_char_names)]
    fn quad_extrema(&mut self, p0: Point, c: Point, p: Point) {
        let at = |t: f64, a: f32, b: f32, e: f32| {
            let (a, b, e) = (f64::from(a), f64::from(b), f64::from(e));
            (1.0 - t) * (1.0 - t) * a + 2.0 * (1.0 - t) * t * b + t * t * e
        };
        for axis in 0..2 {
            let (a, b, e) = if axis == 0 {
                (p0.x, c.x, p.x)
            } else {
                (p0.y, c.y, p.y)
            };
            let denominator = f64::from(a) - 2.0 * f64::from(b) + f64::from(e);
            if denominator.abs() < 1e-12 {
                continue;
            }
            let t = (f64::from(a) - f64::from(b)) / denominator;
            if t > 0.0 && t < 1.0 {
                self.add_xy(at(t, p0.x, c.x, p.x), at(t, p0.y, c.y, p.y));
            }
        }
    }

    /// Adds the points of the cubic curve `p0 c1 c2 p` where a coordinate
    /// has its extremum (the roots of the derivative, a quadratic).
    #[allow(clippy::many_single_char_names)]
    fn cubic_extrema(&mut self, p0: Point, c1: Point, c2: Point, p: Point) {
        let at = |t: f64, v: [f32; 4]| {
            let u = 1.0 - t;
            let v = v.map(f64::from);
            u * u * u * v[0] + 3.0 * u * u * t * v[1] + 3.0 * u * t * t * v[2] + t * t * t * v[3]
        };
        let xs = [p0.x, c1.x, c2.x, p.x];
        let ys = [p0.y, c1.y, c2.y, p.y];
        for values in [xs, ys] {
            let [a, b, c, d] = values.map(f64::from);
            let (d0, d1, d2) = (b - a, c - b, d - c);
            let qa = d0 - 2.0 * d1 + d2;
            let qb = 2.0 * (d1 - d0);
            let qc = d0;
            let mut roots = [None, None];
            if qa.abs() < 1e-12 {
                if qb.abs() >= 1e-12 {
                    roots[0] = Some(-qc / qb);
                }
            } else {
                let discriminant = qb * qb - 4.0 * qa * qc;
                if discriminant >= 0.0 {
                    let s = discriminant.sqrt();
                    roots = [Some((-qb + s) / (2.0 * qa)), Some((-qb - s) / (2.0 * qa))];
                }
            }
            for t in roots.into_iter().flatten() {
                if t > 0.0 && t < 1.0 {
                    self.add_xy(at(t, xs), at(t, ys));
                }
            }
        }
    }

    fn rect(&self) -> Option<Rect> {
        let (x0, y0, x1, y1) = self.0?;
        Some(Rect::new(
            x0 as f32,
            y0 as f32,
            (x1 - x0) as f32,
            (y1 - y0) as f32,
        ))
    }
}

/// Collects segments while counting them against a budget.
pub(crate) struct PathBuilder<'a> {
    segments: Vec<PathSegment>,
    /// The segments that the document may still create.
    budget: &'a mut usize,
    /// True once the budget ran out.
    pub(crate) exhausted: bool,
}

impl<'a> PathBuilder<'a> {
    pub(crate) fn new(budget: &'a mut usize) -> Self {
        PathBuilder {
            segments: Vec::new(),
            budget,
            exhausted: false,
        }
    }

    /// Adds a segment. Returns false (and adds nothing) if a coordinate is
    /// not finite or the budget is used up.
    fn push(&mut self, segment: PathSegment) -> bool {
        let finite = |p: &Point| p.x.is_finite() && p.y.is_finite();
        let ok = match &segment {
            PathSegment::MoveTo(p) | PathSegment::LineTo(p) => finite(p),
            PathSegment::QuadTo(c, p) => finite(c) && finite(p),
            PathSegment::CubicTo(c1, c2, p) => finite(c1) && finite(c2) && finite(p),
            PathSegment::Close => true,
        };
        if !ok {
            return false;
        }
        if *self.budget == 0 {
            self.exhausted = true;
            return false;
        }
        *self.budget -= 1;
        self.segments.push(segment);
        true
    }

    pub(crate) fn move_to(&mut self, p: Point) -> bool {
        self.push(PathSegment::MoveTo(p))
    }

    pub(crate) fn line_to(&mut self, p: Point) -> bool {
        self.push(PathSegment::LineTo(p))
    }

    pub(crate) fn cubic_to(&mut self, c1: Point, c2: Point, p: Point) -> bool {
        self.push(PathSegment::CubicTo(c1, c2, p))
    }

    pub(crate) fn close(&mut self) -> bool {
        self.push(PathSegment::Close)
    }

    /// The last move, if the path has no segment that draws (only
    /// moves): the one point of its bounding box.
    pub(crate) fn lone_point(&self) -> Option<Point> {
        let mut last = None;
        for segment in &self.segments {
            match segment {
                PathSegment::MoveTo(p) => last = Some(*p),
                _ => return None,
            }
        }
        last
    }

    /// The path, or `None` if it has no segment that draws (only moves).
    pub(crate) fn finish(self) -> Option<SvgPath> {
        if !self
            .segments
            .iter()
            .any(|s| !matches!(s, PathSegment::MoveTo(_)))
        {
            return None;
        }
        let mut bounds: Option<(f32, f32, f32, f32)> = None;
        let mut add = |p: Point| {
            let (x0, y0, x1, y1) = bounds.unwrap_or((p.x, p.y, p.x, p.y));
            bounds = Some((x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y)));
        };
        for segment in &self.segments {
            match *segment {
                PathSegment::MoveTo(p) | PathSegment::LineTo(p) => add(p),
                PathSegment::QuadTo(c, p) => {
                    add(c);
                    add(p);
                }
                PathSegment::CubicTo(c1, c2, p) => {
                    add(c1);
                    add(c2);
                    add(p);
                }
                PathSegment::Close => {}
            }
        }
        let (x0, y0, x1, y1) = bounds?;
        Some(SvgPath {
            fill_bounds: OnceLock::new(),
            segments: self.segments,
            bounds: Rect::new(x0, y0, x1 - x0, y1 - y0),
        })
    }
}

fn point(x: f64, y: f64) -> Point {
    Point::new(x as f32, y as f32)
}

/// Parses path data into `builder`, up to the first error.
pub(crate) fn parse_path_data(data: &str, builder: &mut PathBuilder<'_>) {
    let mut state = PathState::default();
    for token in PathParser::from(data) {
        let Ok(token) = token else {
            break;
        };
        if !state.segment(token, builder) {
            break;
        }
    }
}

/// The state of the path data parser between segments.
#[derive(Default)]
struct PathState {
    /// The current point.
    current: (f64, f64),
    /// The start of the current subpath.
    start: (f64, f64),
    /// The second control point of the previous cubic segment, for `S`.
    last_cubic: Option<(f64, f64)>,
    /// The control point of the previous quadratic segment, for `T`.
    last_quad: Option<(f64, f64)>,
    /// True after a close command: a segment other than a move starts at
    /// the subpath's start with an implicit move.
    closed: bool,
}

impl PathState {
    /// Adds one path data segment. Returns false if the path ends here.
    fn segment(&mut self, token: Token, b: &mut PathBuilder<'_>) -> bool {
        let (cx, cy) = self.current;
        let abs = |abs: bool, x: f64, y: f64| if abs { (x, y) } else { (cx + x, cy + y) };
        if self.closed && !matches!(token, Token::MoveTo { .. } | Token::ClosePath { .. }) {
            self.closed = false;
            if !b.move_to(point(self.start.0, self.start.1)) {
                return false;
            }
        }
        let mut last_cubic = None;
        let mut last_quad = None;
        let ok = match token {
            Token::MoveTo { abs: a, x, y } => {
                let p = abs(a, x, y);
                self.start = p;
                self.current = p;
                self.closed = false;
                b.move_to(point(p.0, p.1))
            }
            Token::LineTo { abs: a, x, y } => self.line(abs(a, x, y), b),
            Token::HorizontalLineTo { abs: a, x } => {
                let x = if a { x } else { cx + x };
                self.line((x, cy), b)
            }
            Token::VerticalLineTo { abs: a, y } => {
                let y = if a { y } else { cy + y };
                self.line((cx, y), b)
            }
            Token::CurveTo {
                abs: a,
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => {
                let (c1, c2, p) = (abs(a, x1, y1), abs(a, x2, y2), abs(a, x, y));
                last_cubic = Some(c2);
                self.cubic(c1, c2, p, b)
            }
            Token::SmoothCurveTo {
                abs: a,
                x2,
                y2,
                x,
                y,
            } => {
                let c1 = reflect(self.last_cubic, self.current);
                let (c2, p) = (abs(a, x2, y2), abs(a, x, y));
                last_cubic = Some(c2);
                self.cubic(c1, c2, p, b)
            }
            Token::Quadratic {
                abs: a,
                x1,
                y1,
                x,
                y,
            } => {
                let (c, p) = (abs(a, x1, y1), abs(a, x, y));
                last_quad = Some(c);
                self.quad(c, p, b)
            }
            Token::SmoothQuadratic { abs: a, x, y } => {
                let c = reflect(self.last_quad, self.current);
                last_quad = Some(c);
                self.quad(c, abs(a, x, y), b)
            }
            Token::EllipticalArc {
                abs: a,
                rx,
                ry,
                x_axis_rotation,
                large_arc,
                sweep,
                x,
                y,
            } => {
                let to = abs(a, x, y);
                let arc = EllipticalArc {
                    from: self.current,
                    to,
                    radii: (rx, ry),
                    rotation: x_axis_rotation,
                    large_arc,
                    sweep,
                };
                self.current = to;
                arc.append(b)
            }
            Token::ClosePath { .. } => {
                self.current = self.start;
                self.closed = true;
                b.close()
            }
        };
        self.last_cubic = last_cubic;
        self.last_quad = last_quad;
        ok
    }

    fn line(&mut self, p: (f64, f64), b: &mut PathBuilder<'_>) -> bool {
        self.current = p;
        b.line_to(point(p.0, p.1))
    }

    fn cubic(
        &mut self,
        c1: (f64, f64),
        c2: (f64, f64),
        p: (f64, f64),
        b: &mut PathBuilder<'_>,
    ) -> bool {
        self.current = p;
        b.cubic_to(point(c1.0, c1.1), point(c2.0, c2.1), point(p.0, p.1))
    }

    fn quad(&mut self, c: (f64, f64), p: (f64, f64), b: &mut PathBuilder<'_>) -> bool {
        self.current = p;
        b.push(PathSegment::QuadTo(point(c.0, c.1), point(p.0, p.1)))
    }
}

/// The reflection of the previous control point `control` about the
/// current point, or the current point if there is none (SVG 2 §9.5.6).
fn reflect(control: Option<(f64, f64)>, current: (f64, f64)) -> (f64, f64) {
    match control {
        Some((x, y)) => (2.0 * current.0 - x, 2.0 * current.1 - y),
        None => current,
    }
}

/// An elliptical arc in endpoint parameterization.
struct EllipticalArc {
    from: (f64, f64),
    to: (f64, f64),
    radii: (f64, f64),
    /// The rotation of the x-axis of the ellipse in degrees.
    rotation: f64,
    large_arc: bool,
    sweep: bool,
}

impl EllipticalArc {
    /// Appends the arc as at most four cubic Béziers (one per quarter
    /// turn or less): SVG 2 B.2.4 (conversion to center parameterization)
    /// and B.2.5 (out-of-range radii).
    fn append(&self, b: &mut PathBuilder<'_>) -> bool {
        let (x1, y1) = self.from;
        let (x2, y2) = self.to;
        if x1 == x2 && y1 == y2 {
            // B.2.5 step 1: the arc is omitted.
            return true;
        }
        let (mut rx, mut ry) = (self.radii.0.abs(), self.radii.1.abs());
        if rx == 0.0 || ry == 0.0 || !rx.is_finite() || !ry.is_finite() {
            // B.2.5 step 2: a straight line.
            return b.line_to(point(x2, y2));
        }
        let (sin, cos) = self.rotation.to_radians().sin_cos();
        // Step 1: the midpoint in the rotated frame.
        let dx = (x1 - x2) / 2.0;
        let dy = (y1 - y2) / 2.0;
        let x1p = cos * dx + sin * dy;
        let y1p = -sin * dx + cos * dy;
        // B.2.5 step 3: scale the radii up if they are too small.
        let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
        if lambda > 1.0 {
            let s = lambda.sqrt();
            rx *= s;
            ry *= s;
        }
        // Step 2: the center in the rotated frame.
        let num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
        let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
        let mut coef = if den > 0.0 {
            (num / den).max(0.0).sqrt()
        } else {
            0.0
        };
        if self.large_arc == self.sweep {
            coef = -coef;
        }
        let cxp = coef * rx * y1p / ry;
        let cyp = -coef * ry * x1p / rx;
        // Step 3: the center.
        let cx = cos * cxp - sin * cyp + f64::midpoint(x1, x2);
        let cy = sin * cxp + cos * cyp + f64::midpoint(y1, y2);
        // Step 4: the start angle and the sweep.
        let ux = (x1p - cxp) / rx;
        let uy = (y1p - cyp) / ry;
        let vx = (-x1p - cxp) / rx;
        let vy = (-y1p - cyp) / ry;
        let theta1 = uy.atan2(ux);
        let mut delta = (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
        if !self.sweep && delta > 0.0 {
            delta -= std::f64::consts::TAU;
        } else if self.sweep && delta < 0.0 {
            delta += std::f64::consts::TAU;
        }
        if !(cx.is_finite() && cy.is_finite() && delta.is_finite()) {
            return b.line_to(point(x2, y2));
        }
        let pieces = (delta.abs() / std::f64::consts::FRAC_PI_2)
            .ceil()
            .clamp(1.0, 4.0);
        let step = delta / pieces;
        // The distance of the control points along the tangent, for an
        // arc of `step` radians on the unit circle.
        let k = 4.0 / 3.0 * (step / 4.0).tan();
        let on_ellipse = |t: f64| {
            let (st, ct) = t.sin_cos();
            let (ex, ey) = (rx * ct, ry * st);
            (cx + cos * ex - sin * ey, cy + sin * ex + cos * ey)
        };
        let derivative = |t: f64| {
            let (st, ct) = t.sin_cos();
            let (ex, ey) = (-rx * st, ry * ct);
            (cos * ex - sin * ey, sin * ex + cos * ey)
        };
        let mut t = theta1;
        for i in 0..pieces as usize {
            let t2 = t + step;
            let p0 = on_ellipse(t);
            let d0 = derivative(t);
            // The last end point is exact.
            let p3 = if i + 1 == pieces as usize {
                (x2, y2)
            } else {
                on_ellipse(t2)
            };
            let d3 = derivative(t2);
            let c1 = (p0.0 + k * d0.0, p0.1 + k * d0.1);
            let c2 = (p3.0 - k * d3.0, p3.1 - k * d3.1);
            if !b.cubic_to(point(c1.0, c1.1), point(c2.0, c2.1), point(p3.0, p3.1)) {
                return false;
            }
            t = t2;
        }
        true
    }
}

/// The cubic Bézier factor for a quarter of a circle: the control points
/// are this fraction of the radius away from the end points along the
/// tangents (4/3 · tan(π/8)).
const KAPPA: f32 = 0.552_284_8;

/// The path of an ellipse with center `(cx, cy)` and radii `rx`, `ry`
/// (both positive): four cubic quarters, clockwise from the right end
/// (SVG 2 §10.4: it starts at `cx + rx, cy`).
pub(crate) fn ellipse_path(
    cx: f32,
    cy: f32,
    rx: f32,
    ry: f32,
    budget: &mut usize,
) -> Option<SvgPath> {
    let (kx, ky) = (rx * KAPPA, ry * KAPPA);
    let mut b = PathBuilder::new(budget);
    let p = Point::new;
    let ok = b.move_to(p(cx + rx, cy))
        && b.cubic_to(p(cx + rx, cy + ky), p(cx + kx, cy + ry), p(cx, cy + ry))
        && b.cubic_to(p(cx - kx, cy + ry), p(cx - rx, cy + ky), p(cx - rx, cy))
        && b.cubic_to(p(cx - rx, cy - ky), p(cx - kx, cy - ry), p(cx, cy - ry))
        && b.cubic_to(p(cx + kx, cy - ry), p(cx + rx, cy - ky), p(cx + rx, cy))
        && b.close();
    if ok { b.finish() } else { None }
}

/// The path of a rectangle with corner radii `rx`, `ry` (already clamped
/// to half the width and height; both zero for square corners). SVG 2
/// §10.2: clockwise from `(x + rx, y)`.
pub(crate) fn rect_path(rect: Rect, rx: f32, ry: f32, budget: &mut usize) -> Option<SvgPath> {
    let (left, top) = (rect.x, rect.y);
    let (right, bottom) = (rect.right(), rect.bottom());
    let mut pb = PathBuilder::new(budget);
    let pt = Point::new;
    let ok = if rx > 0.0 && ry > 0.0 {
        let (kx, ky) = (rx * KAPPA, ry * KAPPA);
        pb.move_to(pt(left + rx, top))
            && pb.line_to(pt(right - rx, top))
            && pb.cubic_to(
                pt(right - rx + kx, top),
                pt(right, top + ry - ky),
                pt(right, top + ry),
            )
            && pb.line_to(pt(right, bottom - ry))
            && pb.cubic_to(
                pt(right, bottom - ry + ky),
                pt(right - rx + kx, bottom),
                pt(right - rx, bottom),
            )
            && pb.line_to(pt(left + rx, bottom))
            && pb.cubic_to(
                pt(left + rx - kx, bottom),
                pt(left, bottom - ry + ky),
                pt(left, bottom - ry),
            )
            && pb.line_to(pt(left, top + ry))
            && pb.cubic_to(
                pt(left, top + ry - ky),
                pt(left + rx - kx, top),
                pt(left + rx, top),
            )
            && pb.close()
    } else {
        pb.move_to(pt(left, top))
            && pb.line_to(pt(right, top))
            && pb.line_to(pt(right, bottom))
            && pb.line_to(pt(left, bottom))
            && pb.close()
    };
    if ok { pb.finish() } else { None }
}

/// The path of a `line` element.
pub(crate) fn line_path(from: Point, to: Point, budget: &mut usize) -> Option<SvgPath> {
    let mut b = PathBuilder::new(budget);
    if b.move_to(from) && b.line_to(to) {
        b.finish()
    } else {
        None
    }
}

/// The path of the `points` of a `polyline` or `polygon` (closed). A
/// parse error or an odd number of coordinates ends the list, and the
/// points before it are drawn (SVG 2 §10.6).
pub(crate) fn points_path(
    points: &str,
    close: bool,
    budget: &mut usize,
) -> (Option<Arc<SvgPath>>, Option<Point>, bool) {
    let mut b = PathBuilder::new(budget);
    let mut first = true;
    for (x, y) in PointsParser::from(points) {
        let p = point(x, y);
        let ok = if first { b.move_to(p) } else { b.line_to(p) };
        first = false;
        if !ok {
            break;
        }
    }
    if close && !first {
        b.close();
    }
    let exhausted = b.exhausted;
    let point = b.lone_point();
    (b.finish().map(Arc::new), point, exhausted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(d: &str) -> Vec<PathSegment> {
        let mut budget = 1000;
        let mut b = PathBuilder::new(&mut budget);
        parse_path_data(d, &mut b);
        b.finish().map(|p| p.segments).unwrap_or_default()
    }

    fn p(x: f32, y: f32) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn relative_and_shorthand_commands() {
        use PathSegment::*;
        assert_eq!(
            path("m10 10 h30 v30 h-30 z m40 40 l30 0 0 30"),
            vec![
                MoveTo(p(10.0, 10.0)),
                LineTo(p(40.0, 10.0)),
                LineTo(p(40.0, 40.0)),
                LineTo(p(10.0, 40.0)),
                Close,
                MoveTo(p(50.0, 50.0)),
                LineTo(p(80.0, 50.0)),
                LineTo(p(80.0, 80.0)),
            ]
        );
        // `S` reflects the previous control point; `T` too.
        assert_eq!(
            path("M0 0 C10 0 20 10 20 20 S30 40 40 40"),
            vec![
                MoveTo(p(0.0, 0.0)),
                CubicTo(p(10.0, 0.0), p(20.0, 10.0), p(20.0, 20.0)),
                CubicTo(p(20.0, 30.0), p(30.0, 40.0), p(40.0, 40.0)),
            ]
        );
        assert_eq!(
            path("M0 0 Q10 0 10 10 T10 20 L0 0 T5 5"),
            vec![
                MoveTo(p(0.0, 0.0)),
                QuadTo(p(10.0, 0.0), p(10.0, 10.0)),
                QuadTo(p(10.0, 20.0), p(10.0, 20.0)),
                LineTo(p(0.0, 0.0)),
                QuadTo(p(0.0, 0.0), p(5.0, 5.0)),
            ]
        );
        // A segment after `z` starts at the subpath's start.
        assert_eq!(
            path("M5 5 L10 5 z l5 0"),
            vec![
                MoveTo(p(5.0, 5.0)),
                LineTo(p(10.0, 5.0)),
                Close,
                MoveTo(p(5.0, 5.0)),
                LineTo(p(10.0, 5.0)),
            ]
        );
    }

    #[test]
    fn errors_end_the_path() {
        assert_eq!(path("M0 0 L50 0 L50 50 Z garbage L0 50").len(), 4);
        // A path must start with a move.
        assert_eq!(path("L10 10"), []);
        // Only moves draw nothing.
        assert_eq!(path("M10 10 M20 20"), []);
        assert_eq!(path("M0 0 L1e39 0"), []);
    }

    #[test]
    fn arcs_become_cubics() {
        // A half circle of radius 10 from (0, 0) to (20, 0): two quarters.
        let segments = path("M0 0 A10 10 0 0 1 20 0");
        assert_eq!(segments.len(), 3);
        let PathSegment::CubicTo(_, _, mid) = segments[1] else {
            panic!("not a cubic: {segments:?}");
        };
        // The sweep flag 1 goes through the top (negative y).
        assert!((mid.x - 10.0).abs() < 1e-4 && (mid.y + 10.0).abs() < 1e-4);
        assert_eq!(segments[2], {
            let PathSegment::CubicTo(c1, c2, _) = segments[2] else {
                panic!("not a cubic");
            };
            PathSegment::CubicTo(c1, c2, p(20.0, 0.0))
        });
        // Radii that are too small are scaled up; zero radii give a line.
        assert_eq!(path("M0 0 A1 1 0 0 1 20 0").len(), 3);
        assert_eq!(
            path("M0 0 A0 5 0 0 1 20 0"),
            vec![
                PathSegment::MoveTo(p(0.0, 0.0)),
                PathSegment::LineTo(p(20.0, 0.0))
            ]
        );
        // A full-sweep large arc has at most four pieces.
        assert!(path("M0 0 A10 10 0 1 0 0.001 0").len() <= 5);
    }

    #[test]
    fn budget_limits_segments() {
        let mut budget = 3;
        let mut b = PathBuilder::new(&mut budget);
        parse_path_data("M0 0 L1 1 L2 2 L3 3 L4 4", &mut b);
        assert!(b.exhausted);
        assert_eq!(b.finish().map(|p| p.segments.len()), Some(3));
        assert_eq!(budget, 0);
    }

    #[test]
    fn shapes() {
        let mut budget = 100;
        let r = rect_path(Rect::new(0.0, 0.0, 10.0, 20.0), 0.0, 0.0, &mut budget).expect("a path");
        assert_eq!(r.bounds(), Rect::new(0.0, 0.0, 10.0, 20.0));
        assert_eq!(r.segments().len(), 5);
        let e = ellipse_path(10.0, 10.0, 5.0, 2.0, &mut budget).expect("a path");
        assert_eq!(e.bounds(), Rect::new(5.0, 8.0, 10.0, 4.0));
        let (poly, _, _) = points_path("0,0 10,0 10", true, &mut budget);
        assert_eq!(
            poly.map(|p| p.segments().to_vec()),
            Some(vec![
                PathSegment::MoveTo(p(0.0, 0.0)),
                PathSegment::LineTo(p(10.0, 0.0)),
                PathSegment::Close
            ])
        );
        assert!((r.length_bound() - 60.0).abs() < 1e-4);
    }

    fn parsed(d: &str) -> (Option<SvgPath>, Option<Point>) {
        let mut budget = 1000;
        let mut b = PathBuilder::new(&mut budget);
        parse_path_data(d, &mut b);
        let point = b.lone_point();
        (b.finish(), point)
    }

    fn fill_bounds(d: &str) -> Option<Rect> {
        parsed(d).0.and_then(|p| p.fill_bounds())
    }

    #[test]
    fn fill_bounds_use_the_extrema_of_curves() {
        // Measured in Chromium 148: `M100 100 C 100 50 150 50 150 100` has
        // the top at 62.5 (not at the control points, 50), a quadratic
        // `M200 100 Q 225 40 250 100` at 70, and a half circle as an arc.
        assert_eq!(
            fill_bounds("M100 100 C 100 50 150 50 150 100"),
            Some(Rect::new(100.0, 62.5, 50.0, 37.5))
        );
        assert_eq!(
            fill_bounds("M200 100 Q 225 40 250 100"),
            Some(Rect::new(200.0, 70.0, 50.0, 30.0))
        );
        let arc = fill_bounds("M10 200 A 30 30 0 0 1 70 200").expect("bounds");
        assert!(
            (arc.y - 170.0).abs() < 0.01 && (arc.width - 60.0).abs() < 0.01,
            "{arc:?}"
        );
    }

    #[test]
    fn fill_bounds_skip_moves_that_nothing_follows() {
        // `M 10 10 L 50 50 M 80 80` is 10 10 40 40; two moves in a row
        // count once (the second); a move and a close count.
        assert_eq!(
            fill_bounds("M 10 10 L 50 50 M 80 80"),
            Some(Rect::new(10.0, 10.0, 40.0, 40.0))
        );
        assert_eq!(
            fill_bounds("M 0 0 M 10 10 L 20 20"),
            Some(Rect::new(10.0, 10.0, 10.0, 10.0))
        );
        assert_eq!(
            fill_bounds("M 10 10 z"),
            Some(Rect::new(10.0, 10.0, 0.0, 0.0))
        );
        // A path that only moves has no segment that draws and one point.
        let (path, point) = parsed("M 10 10 M 50 50");
        assert!(path.is_none());
        assert_eq!(point, Some(p(50.0, 50.0)));
        assert_eq!(parsed("x"), (None, None));
    }

    #[test]
    fn rectangles_are_found_in_path_data() {
        let rect = |d: &str| parsed(d).0.and_then(|p| p.as_rect());
        let expected = Some(Rect::new(0.0, 0.0, 436.0, 144.1));
        assert_eq!(rect("M0 0h436v144.1H0z"), expected);
        assert_eq!(rect("M0 0L436 0L436 144.1L0 144.1L0 0z"), expected);
        assert_eq!(rect("M0 0V144.1H436V0Z"), expected);
        // Not a rectangle: a triangle, a curve, two subpaths, extra points.
        assert_eq!(rect("M0 0L10 0L5 10z"), None);
        assert_eq!(rect("M0 0h10v10h-10v-10h5z"), None);
        assert_eq!(rect("M0 0h10v10h-10zM20 20h5v5h-5z"), None);
        assert_eq!(rect("M0 0h10v10C0 10 0 5 0 0z"), None);
        assert_eq!(rect("M0 0L10 10L20 0L10 -10z"), None);
    }
}
