//! Geometry types in CSS pixels.

use std::ops::Add;

/// Clamps a length or coordinate in px to the range that layout produces
/// (see [`swb_style::Length::MAX_PX`]); NaN becomes 0. Lengths from styles
/// are already clamped; this is for values that layout derives from them
/// (line heights, offsets, the scroll size).
pub(crate) fn clamp_length(v: f32) -> f32 {
    swb_style::Length::clamp_px(v)
}

/// A point.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Point {
    /// Horizontal coordinate.
    pub x: f32,
    /// Vertical coordinate (down is positive).
    pub y: f32,
}

impl Point {
    /// Creates a point.
    pub const fn new(x: f32, y: f32) -> Self {
        Point { x, y }
    }
}

impl Add for Point {
    type Output = Point;
    fn add(self, o: Point) -> Point {
        Point::new(self.x + o.x, self.y + o.y)
    }
}

/// A size.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Size {
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
}

impl Size {
    /// Creates a size.
    pub const fn new(width: f32, height: f32) -> Self {
        Size { width, height }
    }
}

/// An axis-aligned rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
}

impl Rect {
    /// Creates a rectangle.
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// The top-left corner.
    pub fn origin(&self) -> Point {
        Point::new(self.x, self.y)
    }

    /// The right edge.
    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    /// The bottom edge.
    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    /// This rectangle moved by `offset`.
    #[must_use]
    pub fn translate(&self, offset: Point) -> Rect {
        Rect::new(
            self.x + offset.x,
            self.y + offset.y,
            self.width,
            self.height,
        )
    }

    /// True if the point is inside (left/top edges inclusive).
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }

    /// The smallest rectangle that contains both.
    #[must_use]
    pub fn union(&self, other: &Rect) -> Rect {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Rect::new(
            x,
            y,
            self.right().max(other.right()) - x,
            self.bottom().max(other.bottom()) - y,
        )
    }

    /// The intersection, or `None` if the rectangles do not overlap.
    #[must_use]
    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        (right > x && bottom > y).then(|| Rect::new(x, y, right - x, bottom - y))
    }

    /// This rectangle shrunk by `sides`.
    #[must_use]
    pub fn inset(&self, sides: &Edges) -> Rect {
        Rect::new(
            self.x + sides.left,
            self.y + sides.top,
            (self.width - sides.left - sides.right).max(0.0),
            (self.height - sides.top - sides.bottom).max(0.0),
        )
    }
}

/// A 2D affine transform. It maps (x, y) to (a·x + c·y + e, b·x + d·y +
/// f), as CSS `matrix(a, b, c, d, e, f)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix {
    /// Horizontal scale.
    pub a: f32,
    /// Vertical shear.
    pub b: f32,
    /// Horizontal shear.
    pub c: f32,
    /// Vertical scale.
    pub d: f32,
    /// Horizontal translation.
    pub e: f32,
    /// Vertical translation.
    pub f: f32,
}

// The names of the entries are those of CSS `matrix(a, b, c, d, e, f)`.
#[allow(clippy::many_single_char_names)]
impl Matrix {
    /// The identity.
    pub const IDENTITY: Matrix = Matrix::new(1.0, 0.0, 0.0, 1.0, 0.0, 0.0);

    /// Creates a matrix.
    pub const fn new(a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) -> Self {
        Matrix { a, b, c, d, e, f }
    }

    /// A translation.
    pub const fn translate(x: f32, y: f32) -> Self {
        Matrix::new(1.0, 0.0, 0.0, 1.0, x, y)
    }

    /// The product `self × other`: `other` is applied first.
    #[must_use]
    pub fn multiply(&self, other: &Matrix) -> Matrix {
        // f64, so that large factors do not lose the small ones.
        let (a, b, c, d, e, f) = (
            f64::from(self.a),
            f64::from(self.b),
            f64::from(self.c),
            f64::from(self.d),
            f64::from(self.e),
            f64::from(self.f),
        );
        let (oa, ob, oc, od, oe, of) = (
            f64::from(other.a),
            f64::from(other.b),
            f64::from(other.c),
            f64::from(other.d),
            f64::from(other.e),
            f64::from(other.f),
        );
        Matrix::new(
            (a * oa + c * ob) as f32,
            (b * oa + d * ob) as f32,
            (a * oc + c * od) as f32,
            (b * oc + d * od) as f32,
            (a * oe + c * of + e) as f32,
            (b * oe + d * of + f) as f32,
        )
    }

    /// The image of a point.
    pub fn apply(&self, p: Point) -> Point {
        Point::new(
            self.a * p.x + self.c * p.y + self.e,
            self.b * p.x + self.d * p.y + self.f,
        )
    }

    /// The bounding box of the image of a rectangle.
    pub fn map_rect(&self, r: &Rect) -> Rect {
        if self.is_translation() {
            return r.translate(Point::new(self.e, self.f));
        }
        let corners = [
            self.apply(Point::new(r.x, r.y)),
            self.apply(Point::new(r.right(), r.y)),
            self.apply(Point::new(r.x, r.bottom())),
            self.apply(Point::new(r.right(), r.bottom())),
        ];
        let (mut x0, mut y0) = (corners[0].x, corners[0].y);
        let (mut x1, mut y1) = (x0, y0);
        for p in &corners[1..] {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }

    /// The inverse, if the matrix is invertible and its entries are
    /// finite.
    pub fn invert(&self) -> Option<Matrix> {
        let (a, b, c, d, e, f) = (
            f64::from(self.a),
            f64::from(self.b),
            f64::from(self.c),
            f64::from(self.d),
            f64::from(self.e),
            f64::from(self.f),
        );
        let det = a * d - b * c;
        if !self.is_finite() || det.abs() < 1e-12 {
            return None;
        }
        let inv = Matrix::new(
            (d / det) as f32,
            (-b / det) as f32,
            (-c / det) as f32,
            (a / det) as f32,
            ((c * f - d * e) / det) as f32,
            ((b * e - a * f) / det) as f32,
        );
        inv.is_finite().then_some(inv)
    }

    /// True if the matrix only translates.
    pub fn is_translation(&self) -> bool {
        self.a == 1.0 && self.b == 0.0 && self.c == 0.0 && self.d == 1.0
    }

    /// True if all entries are finite.
    pub fn is_finite(&self) -> bool {
        [self.a, self.b, self.c, self.d, self.e, self.f]
            .iter()
            .all(|v| v.is_finite())
    }

    /// The largest factor by which the matrix stretches a length: the
    /// largest singular value of its linear part.
    pub fn max_scale(&self) -> f32 {
        let (a, b, c, d) = (
            f64::from(self.a),
            f64::from(self.b),
            f64::from(self.c),
            f64::from(self.d),
        );
        let s = a * a + b * b + c * c + d * d;
        let det = a * d - b * c;
        let root = (s * s - 4.0 * det * det).max(0.0).sqrt();
        f64::midpoint(s, root).sqrt() as f32
    }
}

/// Widths of the four edges of a box (margins, borders or padding).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Edges {
    /// Top.
    pub top: f32,
    /// Right.
    pub right: f32,
    /// Bottom.
    pub bottom: f32,
    /// Left.
    pub left: f32,
}

impl Edges {
    /// All edges zero.
    pub const ZERO: Edges = Edges::new(0.0, 0.0, 0.0, 0.0);

    /// Creates edges.
    pub const fn new(top: f32, right: f32, bottom: f32, left: f32) -> Self {
        Edges {
            top,
            right,
            bottom,
            left,
        }
    }

    /// Left + right.
    pub fn horizontal(&self) -> f32 {
        self.left + self.right
    }

    /// Top + bottom.
    pub fn vertical(&self) -> f32 {
        self.top + self.bottom
    }
}

impl Add for Edges {
    type Output = Edges;
    fn add(self, o: Edges) -> Edges {
        Edges::new(
            self.top + o.top,
            self.right + o.right,
            self.bottom + o.bottom,
            self.left + o.left,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_ops() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(5.0, 5.0, 10.0, 10.0);
        assert_eq!(a.union(&b), Rect::new(0.0, 0.0, 15.0, 15.0));
        assert_eq!(a.intersection(&b), Some(Rect::new(5.0, 5.0, 5.0, 5.0)));
        assert_eq!(a.intersection(&Rect::new(20.0, 0.0, 1.0, 1.0)), None);
        assert!(a.contains(Point::new(0.0, 9.9)));
        assert!(!a.contains(Point::new(10.0, 0.0)));
        let e = Edges::new(1.0, 2.0, 3.0, 4.0);
        assert_eq!(a.inset(&e), Rect::new(4.0, 1.0, 4.0, 6.0));
        // Insets larger than the rectangle give zero size.
        let all = Edges::new(6.0, 6.0, 6.0, 6.0);
        assert_eq!(a.inset(&all), Rect::new(6.0, 6.0, 0.0, 0.0));
    }

    #[test]
    fn matrix_ops() {
        let rotate = Matrix::new(0.0, 1.0, -1.0, 0.0, 0.0, 0.0); // 90° clockwise
        let t = Matrix::translate(10.0, 0.0);
        // Translate first, then rotate.
        let m = rotate.multiply(&t);
        assert_eq!(m.apply(Point::new(0.0, 0.0)), Point::new(0.0, 10.0));
        let inv = m.invert().expect("invertible");
        assert_eq!(inv.apply(Point::new(0.0, 10.0)), Point::new(0.0, 0.0));
        assert_eq!(
            rotate.map_rect(&Rect::new(0.0, 0.0, 10.0, 20.0)),
            Rect::new(-20.0, 0.0, 20.0, 10.0)
        );
        assert!(Matrix::new(0.0, 0.0, 0.0, 0.0, 1.0, 1.0).invert().is_none());
        assert!(
            Matrix::new(f32::INFINITY, 0.0, 0.0, 1.0, 0.0, 0.0)
                .invert()
                .is_none()
        );
        assert!((Matrix::new(2.0, 0.0, 0.0, 0.5, 0.0, 0.0).max_scale() - 2.0).abs() < 1e-6);
        assert!((rotate.max_scale() - 1.0).abs() < 1e-6);
        assert!(t.is_translation() && !rotate.is_translation());
    }
}
