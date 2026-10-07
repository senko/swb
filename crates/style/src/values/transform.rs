//! Computed values of `transform` and `transform-origin` (CSS Transforms 1,
//! <https://www.w3.org/TR/css-transforms-1/>) and of `clip` (CSS 2.2
//! §11.1.2, <https://www.w3.org/TR/CSS22/visufx.html#clipping>).

use super::LengthPercentage;

/// A 2D transform function
/// (<https://www.w3.org/TR/css-transforms-1/#two-d-transform-functions>).
/// Lengths are in px; percentages of `translate()` refer to the size of
/// the border box.
#[derive(Clone, Debug, PartialEq)]
pub enum TransformFunction {
    /// `matrix(a, b, c, d, e, f)`: the matrix `[a c e; b d f; 0 0 1]`.
    Matrix([f32; 6]),
    /// `translate()`, `translateX()`, `translateY()`.
    Translate(LengthPercentage, LengthPercentage),
    /// `scale()`, `scaleX()`, `scaleY()`.
    Scale(f32, f32),
    /// `rotate()`, clockwise, in degrees.
    Rotate(f32),
    /// `skew()`, `skewX()`, `skewY()`: the angles in degrees.
    Skew(f32, f32),
}

/// The computed `transform-origin`: the point the transform is applied
/// around, relative to the border box (the z component is not
/// supported).
#[derive(Clone, Debug, PartialEq)]
pub struct TransformOrigin {
    /// Horizontal offset from the left border edge.
    pub x: LengthPercentage,
    /// Vertical offset from the top border edge.
    pub y: LengthPercentage,
}

impl Default for TransformOrigin {
    /// `50% 50%`.
    fn default() -> Self {
        TransformOrigin {
            x: LengthPercentage::Percent(0.5),
            y: LengthPercentage::Percent(0.5),
        }
    }
}

/// The computed `clip: rect(top, right, bottom, left)`: offsets in px
/// from the top-left corner of the border box; `None` for `auto`, which
/// is the matching edge of the border box.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ClipRect {
    /// The top edge.
    pub top: Option<f32>,
    /// The right edge, measured from the left border edge.
    pub right: Option<f32>,
    /// The bottom edge, measured from the top border edge.
    pub bottom: Option<f32>,
    /// The left edge.
    pub left: Option<f32>,
}
