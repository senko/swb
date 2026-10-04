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
}
