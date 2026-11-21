//! Computed values of the properties that M13 feature 2a added: the
//! shadows (`box-shadow`, `text-shadow`; CSS Backgrounds 3 §7,
//! <https://www.w3.org/TR/css-backgrounds-3/#box-shadow>, CSS Text
//! Decoration 3 §4), the individual transform properties (`translate`,
//! `rotate`, `scale`; CSS Transforms 2,
//! <https://www.w3.org/TR/css-transforms-2/#individual-transforms>) and
//! `corner-shape` (CSS Borders 4,
//! <https://drafts.csswg.org/css-borders-4/#corner-shaping>).
//!
//! The values stay in the form that the specifications give as computed
//! values (a shadow list, an axis and an angle), so that animations can
//! interpolate them (M13 feature 3).

use super::{Color, LengthPercentage};

/// One computed `box-shadow`. Lengths are in px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxShadow {
    /// The color; `currentColor` is the element's `color`.
    pub color: Color,
    /// The horizontal offset.
    pub x: f32,
    /// The vertical offset.
    pub y: f32,
    /// The blur radius (not negative); the Gaussian's standard deviation is
    /// half of it.
    pub blur: f32,
    /// The spread distance (may be negative).
    pub spread: f32,
    /// True for an inner shadow.
    pub inset: bool,
}

/// One computed `text-shadow`. Lengths are in px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextShadow {
    /// The color; `currentColor` is the element's `color`.
    pub color: Color,
    /// The horizontal offset.
    pub x: f32,
    /// The vertical offset.
    pub y: f32,
    /// The blur radius (not negative).
    pub blur: f32,
}

/// The computed `translate` (not `none`): offsets that percentages of the
/// border box can give, and a z offset in px (not drawn: swb has no 3D).
#[derive(Clone, Debug, PartialEq)]
pub struct Translate {
    /// The horizontal offset.
    pub x: LengthPercentage,
    /// The vertical offset.
    pub y: LengthPercentage,
    /// The z offset in px.
    pub z: f32,
}

/// The computed `rotate` (not `none`): an axis and an angle in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rotate {
    /// The axis `x y z` as written (`rotate: 45deg` is the z axis).
    pub axis: [f32; 3],
    /// The angle in degrees.
    pub angle: f32,
}

/// The computed `scale` (not `none`): the factors of the three axes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale {
    /// The horizontal factor.
    pub x: f32,
    /// The vertical factor.
    pub y: f32,
    /// The z factor.
    pub z: f32,
}

/// A computed `corner-*-shape`: the parameter `k` of the superellipse
/// `superellipse(k)`; `round` is 1, `squircle` 2, `bevel` 0, `scoop` -1,
/// `square` positive infinity and `notch` negative infinity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CornerShape(pub f32);

impl CornerShape {
    /// `round`.
    pub const ROUND: CornerShape = CornerShape(1.0);

    /// The keyword for the value, if it has one.
    pub fn keyword(self) -> Option<&'static str> {
        Some(match self.0 {
            1.0 => "round",
            2.0 => "squircle",
            0.0 => "bevel",
            -1.0 => "scoop",
            v if v == f32::INFINITY => "square",
            v if v == f32::NEG_INFINITY => "notch",
            _ => return None,
        })
    }

    /// Parses a keyword.
    pub fn from_ident(ident: &str) -> Option<CornerShape> {
        Some(CornerShape(match ident.to_ascii_lowercase().as_str() {
            "round" => 1.0,
            "squircle" => 2.0,
            "bevel" => 0.0,
            "scoop" => -1.0,
            "square" => f32::INFINITY,
            "notch" => f32::NEG_INFINITY,
            _ => return None,
        }))
    }
}

impl Default for CornerShape {
    fn default() -> Self {
        CornerShape::ROUND
    }
}
