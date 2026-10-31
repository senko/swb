//! Computed values of the basic shapes of `clip-path` (CSS Shapes 1 §3,
//! <https://drafts.csswg.org/css-shapes-1/#basic-shape-functions>; CSS
//! Masking 1 §5, <https://drafts.fxtf.org/css-masking-1/#the-clip-path>).
//!
//! `rect()` and `xywh()` compute to [`BasicShape::Inset`], as in Chromium.
//! Lengths are in px; percentages and `calc()` with percentages stay until
//! paint knows the reference box. `shape()` is not supported (the
//! declaration is invalid).

use std::sync::Arc;

use super::length::LengthPercentage;
use super::{CornerRadius, FillRule, PositionComponent};

/// The most points of a `polygon()` (and the most segments of the data of
/// a `path()`) that swb keeps. A longer shape makes the declaration
/// invalid (ADR 0028 section 16). Chromium has no limit; real pages use a
/// few dozen points.
pub const MAX_SHAPE_POINTS: usize = 1024;

/// The most points of a `polygon()` that uses font-relative or viewport
/// units: the computed value of such a shape is made again for each
/// element, while the value of a shape in absolute units is shared.
pub const MAX_RELATIVE_SHAPE_POINTS: usize = 128;

/// The longest data string of a `path()` in bytes (about
/// [`MAX_SHAPE_POINTS`] segments).
pub const MAX_PATH_DATA_BYTES: usize = 16 * 1024;

/// A computed `<geometry-box>`: the reference box of a shape.
/// <https://drafts.fxtf.org/css-masking-1/#typedef-geometry-box>
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GeometryBox {
    /// `margin-box`.
    MarginBox,
    /// `border-box`: the initial reference box.
    BorderBox,
    /// `padding-box`.
    PaddingBox,
    /// `content-box`.
    ContentBox,
    /// `fill-box`: the object bounding box. For an element with a CSS box
    /// it is the content box.
    FillBox,
    /// `stroke-box`: the stroke bounding box. For an element with a CSS
    /// box it is the border box.
    StrokeBox,
    /// `view-box`: the nearest SVG viewport. For an element with a CSS box
    /// it is the border box.
    ViewBox,
}

impl GeometryBox {
    /// The keyword.
    pub fn as_str(self) -> &'static str {
        match self {
            GeometryBox::MarginBox => "margin-box",
            GeometryBox::BorderBox => "border-box",
            GeometryBox::PaddingBox => "padding-box",
            GeometryBox::ContentBox => "content-box",
            GeometryBox::FillBox => "fill-box",
            GeometryBox::StrokeBox => "stroke-box",
            GeometryBox::ViewBox => "view-box",
        }
    }

    /// Parses a keyword (ASCII case-insensitive).
    pub fn from_ident(ident: &str) -> Option<Self> {
        Some(match ident.to_ascii_lowercase().as_str() {
            "margin-box" => GeometryBox::MarginBox,
            "border-box" => GeometryBox::BorderBox,
            "padding-box" => GeometryBox::PaddingBox,
            "content-box" => GeometryBox::ContentBox,
            "fill-box" => GeometryBox::FillBox,
            "stroke-box" => GeometryBox::StrokeBox,
            "view-box" => GeometryBox::ViewBox,
            _ => return None,
        })
    }
}

/// The radius of a `circle()` or one axis of an `ellipse()`.
#[derive(Clone, Debug, PartialEq)]
pub enum ShapeRadius {
    /// `closest-side` (the initial value).
    ClosestSide,
    /// `farthest-side`.
    FarthestSide,
    /// A length, or a percentage of the reference box.
    Length(LengthPercentage),
}

/// The center of a `circle()` or `ellipse()`.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapePosition {
    /// The horizontal component.
    pub x: PositionComponent,
    /// The vertical component.
    pub y: PositionComponent,
}

/// The sides of an `inset()` and its corner radii.
#[derive(Clone, Debug, PartialEq)]
pub struct Inset {
    /// The distance of the top edge from the reference box's top edge.
    pub top: LengthPercentage,
    /// The distance of the right edge from the reference box's right edge.
    pub right: LengthPercentage,
    /// The distance of the bottom edge from the reference box's bottom
    /// edge.
    pub bottom: LengthPercentage,
    /// The distance of the left edge from the reference box's left edge.
    pub left: LengthPercentage,
    /// The corner radii (top left, top right, bottom right, bottom left),
    /// if any is not zero.
    pub radii: Option<Box<[CornerRadius; 4]>>,
}

/// A computed basic shape.
#[derive(Clone, Debug, PartialEq)]
pub enum BasicShape {
    /// `inset()`, `rect()` and `xywh()`.
    Inset(Inset),
    /// `circle()`. `position` is `None` for the center of the box.
    Circle {
        /// The radius.
        radius: ShapeRadius,
        /// The center, if the value gives one.
        position: Option<ShapePosition>,
    },
    /// `ellipse()`.
    Ellipse {
        /// The horizontal radius.
        rx: ShapeRadius,
        /// The vertical radius.
        ry: ShapeRadius,
        /// The center, if the value gives one.
        position: Option<ShapePosition>,
    },
    /// `polygon()`.
    Polygon {
        /// The fill rule.
        rule: FillRule,
        /// The points (x, y), at least one.
        points: Vec<(LengthPercentage, LengthPercentage)>,
    },
    /// `path()`.
    Path {
        /// The fill rule as written. Chromium fills the shape with the
        /// even-odd rule whatever the value says (measured), and so does
        /// swb.
        rule: FillRule,
        /// The path data in absolute commands, in the form that
        /// `getComputedStyle` gives (`M 0 0 L 10 0 Z`).
        data: Arc<str>,
    },
}

/// The computed `clip-path` of a basic shape or a geometry box:
/// `<basic-shape> || <geometry-box>`.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeClip {
    /// The shape; `None` for a geometry box alone (it clips to the box
    /// with its rounded corners).
    pub shape: Option<BasicShape>,
    /// The reference box; `None` is `border-box`. A shape with
    /// `border-box` is stored without it (it serializes as
    /// `circle(50%)`); a `border-box` alone is stored, because it
    /// serializes with its keyword.
    pub geometry_box: Option<GeometryBox>,
}

impl ShapeClip {
    /// The reference box that the value uses.
    pub fn reference_box(&self) -> GeometryBox {
        self.geometry_box.unwrap_or(GeometryBox::BorderBox)
    }
}
