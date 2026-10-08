//! The viewport of an outer `<svg>` element: its natural size, `viewBox`
//! and `preserveAspectRatio`.
//!
//! - Natural size (measured in Chromium 148, `tools/probes/inline-svg.json`;
//!   SVG 2 §8.2,
//!   <https://svgwg.org/svg2-draft/coords.html#SizingSVGInCSS>): the
//!   `width` and `height` attributes in absolute units (and `em`, `ex`)
//!   are the natural width and height, also when CSS overrides the
//!   properties that the attributes set; percentages and invalid values
//!   give none. The ratio is `width / height` if both are positive, else
//!   the `viewBox` ratio.
//! - The `viewBox` transform: SVG 2 §8.2, "Equivalent transform of an SVG
//!   viewport", <https://svgwg.org/svg2-draft/coords.html#ComputingAViewportsTransform>.

use std::str::FromStr;

use swb_dom::ElementData;
use swb_style::ComputedStyle;

use crate::NaturalSize;
use crate::geom::{Matrix, Rect, Size, clamp_length};

/// Alignment of the view box along one axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Align {
    /// `xMin`, `YMin`.
    Min,
    /// `xMid`, `YMid`.
    Mid,
    /// `xMax`, `YMax`.
    Max,
}

/// A `preserveAspectRatio` value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PreserveAspectRatio {
    /// The alignment on both axes; `None` for `none` (scale each axis on
    /// its own).
    pub align: Option<(Align, Align)>,
    /// True for `slice` (cover the viewport), false for `meet` (fit).
    pub slice: bool,
}

impl Default for PreserveAspectRatio {
    /// `xMidYMid meet`.
    fn default() -> Self {
        PreserveAspectRatio {
            align: Some((Align::Mid, Align::Mid)),
            slice: false,
        }
    }
}

/// The `viewBox` of `e`, if it is valid (four numbers, width and height
/// positive). A width or height of 0 disables rendering in SVG 2; Chromium
/// sizes such an element as if it had no `viewBox` (measured) and draws
/// nothing, so it is invalid here too.
pub(crate) fn view_box(e: &ElementData) -> Option<Rect> {
    let vb = svgtypes::ViewBox::from_str(e.attr("viewBox")?).ok()?;
    let r = Rect::new(vb.x as f32, vb.y as f32, vb.w as f32, vb.h as f32);
    let finite = [r.x, r.y, r.width, r.height].iter().all(|v| v.is_finite());
    (finite && r.width > 0.0 && r.height > 0.0).then_some(r)
}

/// The `preserveAspectRatio` of `e`; the default for a missing or invalid
/// attribute.
pub(crate) fn preserve_aspect_ratio(e: &ElementData) -> PreserveAspectRatio {
    use svgtypes::Align as A;
    let Some(par) = e
        .attr("preserveAspectRatio")
        .and_then(|v| svgtypes::AspectRatio::from_str(v).ok())
    else {
        return PreserveAspectRatio::default();
    };
    let align = match par.align {
        A::None => None,
        A::XMinYMin => Some((Align::Min, Align::Min)),
        A::XMidYMin => Some((Align::Mid, Align::Min)),
        A::XMaxYMin => Some((Align::Max, Align::Min)),
        A::XMinYMid => Some((Align::Min, Align::Mid)),
        A::XMidYMid => Some((Align::Mid, Align::Mid)),
        A::XMaxYMid => Some((Align::Max, Align::Mid)),
        A::XMinYMax => Some((Align::Min, Align::Max)),
        A::XMidYMax => Some((Align::Mid, Align::Max)),
        A::XMaxYMax => Some((Align::Max, Align::Max)),
    };
    PreserveAspectRatio {
        align,
        slice: par.slice,
    }
}

/// The transform from the user space of a view box `view_box` to a
/// viewport of `size` at the origin (SVG 2 §8.2).
pub(crate) fn view_box_transform(view_box: Rect, par: PreserveAspectRatio, size: Size) -> Matrix {
    let mut sx = size.width / view_box.width;
    let mut sy = size.height / view_box.height;
    let Some((ax, ay)) = par.align else {
        return Matrix::new(sx, 0.0, 0.0, sy, -view_box.x * sx, -view_box.y * sy);
    };
    let s = if par.slice { sx.max(sy) } else { sx.min(sy) };
    sx = s;
    sy = s;
    let offset = |align: Align, viewport: f32, content: f32| match align {
        Align::Min => 0.0,
        Align::Mid => (viewport - content) / 2.0,
        Align::Max => viewport - content,
    };
    let tx = -view_box.x * sx + offset(ax, size.width, view_box.width * sx);
    let ty = -view_box.y * sy + offset(ay, size.height, view_box.height * sy);
    Matrix::new(sx, 0.0, 0.0, sy, tx, ty)
}

/// The natural size of an outer `svg` element `e` with computed style
/// `style` (see the module documentation).
pub(crate) fn natural_size(e: &ElementData, style: &ComputedStyle) -> NaturalSize {
    let dimension = |name: &str| {
        e.attr(name)
            .and_then(|v| absolute_length(v, style.font_size))
            .map(|v| clamp_length(v.max(0.0)))
    };
    let width = dimension("width");
    let height = dimension("height");
    let ratio = match (width, height) {
        (Some(w), Some(h)) if w > 0.0 && h > 0.0 => Some(w / h),
        _ => view_box(e).map(|vb| vb.width / vb.height),
    };
    NaturalSize {
        width,
        height,
        ratio: ratio.map(clamp_ratio).filter(|r| r.is_finite() && *r > 0.0),
    }
}

/// A ratio within [1/L, L] for the largest layout length L, so that sizes
/// derived from it stay finite.
fn clamp_ratio(ratio: f32) -> f32 {
    let max = swb_style::Length::MAX_PX;
    ratio.clamp(1.0 / max, max)
}

/// An attribute length in px: a number with an absolute unit, `em` or
/// `ex` (half an `em`, as elsewhere in swb), or no unit; `None` for a
/// percentage or an invalid value.
fn absolute_length(value: &str, font_size: f32) -> Option<f32> {
    let length = svgtypes::Length::from_str(value.trim()).ok()?;
    length_px(&length, font_size)
}

/// `length` in px (see [`absolute_length`]); `None` for a percentage or a
/// value that is not finite.
pub(super) fn length_px(length: &svgtypes::Length, font_size: f32) -> Option<f32> {
    use svgtypes::LengthUnit as U;
    let n = length.number as f32;
    let px = match length.unit {
        U::None | U::Px => n,
        U::Em => n * font_size,
        U::Ex => n * font_size / 2.0,
        U::In => n * 96.0,
        U::Cm => n * 96.0 / 2.54,
        U::Mm => n * 96.0 / 25.4,
        U::Pt => n * 4.0 / 3.0,
        U::Pc => n * 16.0,
        U::Percent => return None,
    };
    px.is_finite().then_some(px)
}

#[cfg(test)]
mod tests {
    use swb_dom::parse_html;

    use super::*;

    fn svg_element(attrs: &str) -> ElementData {
        let doc = parse_html(&format!("<svg {attrs}></svg>"));
        let body = doc.body().expect("a body");
        let svg = doc.element_children(body).next().expect("an svg element");
        doc.element(svg).expect("an element").clone()
    }

    fn natural(attrs: &str) -> NaturalSize {
        natural_size(&svg_element(attrs), &ComputedStyle::initial())
    }

    #[test]
    fn natural_sizes() {
        assert_eq!(natural(""), NaturalSize::default());
        assert_eq!(
            natural("viewBox='0 0 100 50'"),
            NaturalSize {
                width: None,
                height: None,
                ratio: Some(2.0)
            }
        );
        assert_eq!(
            natural("width=100 height=50"),
            NaturalSize::fixed(100.0, 50.0)
        );
        // The ratio of the attributes wins over the view box.
        assert_eq!(
            natural("width=100 height=50 viewBox='0 0 10 40'").ratio,
            Some(2.0)
        );
        assert_eq!(
            natural("width=100 viewBox='0 0 10 40'"),
            NaturalSize {
                width: Some(100.0),
                height: None,
                ratio: Some(0.25)
            }
        );
        assert_eq!(natural("width=50% height=abc"), NaturalSize::default());
        assert_eq!(natural("width=1em").width, Some(16.0));
        assert_eq!(natural("width=1in").width, Some(96.0));
        assert_eq!(natural("width=-10").width, Some(0.0));
        assert_eq!(natural("viewBox='0 0 -1 5'").ratio, None);
        assert_eq!(natural("viewBox='0 0 0 5'").ratio, None);
        assert_eq!(
            natural("viewBox='0 0 1e-30 1e30'").ratio.map(|r| r > 0.0),
            Some(true)
        );
    }

    #[test]
    fn view_box_transforms() {
        let vb = Rect::new(0.0, 0.0, 10.0, 10.0);
        let size = Size::new(100.0, 50.0);
        let par =
            |v: &str| preserve_aspect_ratio(&svg_element(&format!("preserveAspectRatio='{v}'")));
        let t = view_box_transform(vb, par("xMidYMid"), size);
        assert_eq!(t, Matrix::new(5.0, 0.0, 0.0, 5.0, 25.0, 0.0));
        let t = view_box_transform(vb, par("none"), size);
        assert_eq!(t, Matrix::new(10.0, 0.0, 0.0, 5.0, 0.0, 0.0));
        let t = view_box_transform(vb, par("xMaxYMax meet"), size);
        assert_eq!(t, Matrix::new(5.0, 0.0, 0.0, 5.0, 50.0, 0.0));
        let t = view_box_transform(vb, par("xMidYMin slice"), size);
        assert_eq!(t, Matrix::new(10.0, 0.0, 0.0, 10.0, 0.0, 0.0));
        assert_eq!(par("bogus"), PreserveAspectRatio::default());
        let t = view_box_transform(
            Rect::new(10.0, 10.0, 20.0, 20.0),
            par(""),
            Size::new(50.0, 50.0),
        );
        assert_eq!(t, Matrix::new(2.5, 0.0, 0.0, 2.5, -25.0, -25.0));
    }
}
