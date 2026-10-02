//! `<image>` values: `url()`, linear gradients and `image-set()`.
//!
//! <https://www.w3.org/TR/css-images-4/>
//!
//! Supported: `url()`, `linear-gradient()`, `repeating-linear-gradient()`
//! and their legacy `-webkit-` forms, and `image-set()` /
//! `-webkit-image-set()` (the 1x candidate is chosen). Other gradients
//! (radial, conic, `-webkit-gradient()`) and image functions
//! (`cross-fade()`, `element()`, `paint()`) parse as valid values but
//! produce no image. Gradient lines toward a corner (`to top right`) use a
//! 45° multiple instead of the box-dependent angle.

use std::sync::Arc;

use swb_css::{ComponentValue, ParseError, Parser};

use super::color::parse_color;
use super::length::{LengthOptions, parse_length_percentage};
use super::{ParseResult, ParserContext, parse_angle};
use crate::values::{Color, Image, LengthContext, LinearGradient, SpecifiedLengthPercentage};

/// A specified `<image>`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SpecifiedImage {
    /// An absolute (resolved) URL.
    Url(Arc<str>),
    /// A linear gradient.
    LinearGradient(Box<SpecifiedLinearGradient>),
}

/// A specified linear gradient.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SpecifiedLinearGradient {
    /// The gradient line angle in degrees (0 = to top, 90 = to right).
    pub(crate) angle_deg: f32,
    /// The color stops and their optional positions.
    pub(crate) stops: Vec<(Color, Option<SpecifiedLengthPercentage>)>,
    /// True for `repeating-linear-gradient()`.
    pub(crate) repeating: bool,
}

impl SpecifiedImage {
    /// Computes the image.
    pub(crate) fn compute(&self, ctx: &LengthContext) -> Image {
        match self {
            SpecifiedImage::Url(url) => Image::Url(Arc::clone(url)),
            SpecifiedImage::LinearGradient(g) => Image::LinearGradient(Arc::new(LinearGradient {
                angle_deg: g.angle_deg,
                stops: g
                    .stops
                    .iter()
                    .map(|(color, pos)| (*color, pos.as_ref().map(|p| p.compute(ctx))))
                    .collect(),
                repeating: g.repeating,
            })),
        }
    }
}

/// Consumes an `<image>`. Returns `Ok(None)` for valid images that swb
/// cannot render (see the module documentation).
pub(crate) fn parse_image(
    p: &mut Parser<'_>,
    cx: &ParserContext,
) -> ParseResult<Option<SpecifiedImage>> {
    if let Ok(url) = p.expect_url() {
        // An empty URL is an invalid resource, not the base URL.
        // <https://www.w3.org/TR/css-values-4/#urls>
        if url.trim().is_empty() {
            return Ok(None);
        }
        return Ok(Some(SpecifiedImage::Url(cx.resolve_url(url))));
    }
    p.try_parse(|p| {
        let (name, mut args) = p.expect_function()?;
        let lower = name.to_ascii_lowercase();
        match lower.as_str() {
            "linear-gradient" | "repeating-linear-gradient" => args
                .parse_entirely(|a| parse_linear_gradient(a, lower.starts_with("repeating"), false))
                .map(|g| Some(SpecifiedImage::LinearGradient(Box::new(g)))),
            "-webkit-linear-gradient" | "-webkit-repeating-linear-gradient" => args
                .parse_entirely(|a| parse_linear_gradient(a, lower.contains("repeating"), true))
                .map(|g| Some(SpecifiedImage::LinearGradient(Box::new(g)))),
            "image-set" | "-webkit-image-set" => parse_image_set(&mut args, cx),
            "radial-gradient"
            | "repeating-radial-gradient"
            | "conic-gradient"
            | "repeating-conic-gradient"
            | "-webkit-radial-gradient"
            | "-webkit-repeating-radial-gradient"
            | "-webkit-gradient"
            | "cross-fade"
            | "-webkit-cross-fade"
            | "element"
            | "-moz-element"
            | "paint" => Ok(None),
            _ => Err(ParseError::Unexpected),
        }
    })
}

/// Parses the arguments of a linear gradient.
/// <https://www.w3.org/TR/css-images-4/#linear-gradients>
///
/// `legacy` selects the `-webkit-` syntax: a bare side means where the
/// gradient starts (`top` is `to bottom`), and angles are measured
/// counter-clockwise from the right.
fn parse_linear_gradient(
    p: &mut Parser<'_>,
    repeating: bool,
    legacy: bool,
) -> ParseResult<SpecifiedLinearGradient> {
    let direction = p.try_parse(|p| {
        let angle = if let Ok(a) = parse_angle(p, true) {
            if legacy { 90.0 - a } else { a }
        } else if legacy {
            side_angle(p)? + 180.0
        } else {
            p.expect_ident_matching("to")?;
            side_angle(p)?
        };
        p.expect_comma()?;
        Ok::<_, ParseError>(angle)
    });
    let angle_deg = direction.unwrap_or(180.0).rem_euclid(360.0);
    let mut stops = Vec::new();
    let items = p.parse_comma_separated(|item| {
        // A transition hint: a lone length-percentage.
        if item
            .parse_entirely(|i| parse_length_percentage(i, LengthOptions::LENGTH_PERCENTAGE))
            .is_ok()
        {
            return Ok(GradientItem::Hint);
        }
        // `<color> <length-percentage>{0,2}`.
        let color = parse_color(item)?;
        let mut positions = Vec::new();
        while positions.len() < 2 {
            match parse_length_percentage(item, LengthOptions::LENGTH_PERCENTAGE) {
                Ok(p) => positions.push(p),
                Err(_) => break,
            }
        }
        Ok(GradientItem::Stop(color, positions))
    })?;
    let mut previous_was_stop = false;
    for item in items {
        match item {
            GradientItem::Hint => {
                // Hints must sit between two stops; they are ignored.
                if !previous_was_stop {
                    return Err(ParseError::Invalid);
                }
                previous_was_stop = false;
            }
            GradientItem::Stop(color, positions) => {
                if positions.is_empty() {
                    stops.push((color, None));
                }
                for pos in positions {
                    stops.push((color, Some(pos)));
                }
                previous_was_stop = true;
            }
        }
    }
    if stops.len() < 2 || !previous_was_stop {
        return Err(ParseError::Invalid);
    }
    Ok(SpecifiedLinearGradient {
        angle_deg,
        stops,
        repeating,
    })
}

enum GradientItem {
    /// A transition hint (ignored).
    Hint,
    Stop(Color, Vec<SpecifiedLengthPercentage>),
}

/// `<side-or-corner>`: one or two of `left`/`right` and `top`/`bottom`,
/// as the angle of the direction toward that side or corner.
fn side_angle(p: &mut Parser<'_>) -> ParseResult<f32> {
    #[derive(Clone, Copy, PartialEq)]
    enum Side {
        Top,
        Right,
        Bottom,
        Left,
    }
    let sides = [
        ("top", Side::Top),
        ("right", Side::Right),
        ("bottom", Side::Bottom),
        ("left", Side::Left),
    ];
    let first = p.expect_one_of(&sides)?;
    let second = p.expect_one_of(&sides).ok();
    let vertical = |side: Side| matches!(side, Side::Top | Side::Bottom);
    let angle = match (first, second) {
        (side, None) => match side {
            Side::Top => 0.0,
            Side::Right => 90.0,
            Side::Bottom => 180.0,
            Side::Left => 270.0,
        },
        (one, Some(other)) if vertical(one) == vertical(other) => {
            return Err(ParseError::Invalid);
        }
        (one, Some(other)) => {
            let (vert, horiz) = if vertical(one) {
                (one, other)
            } else {
                (other, one)
            };
            match (vert, horiz) {
                (Side::Top, Side::Right) => 45.0,
                (Side::Bottom, Side::Right) => 135.0,
                (Side::Bottom, _) => 225.0,
                _ => 315.0,
            }
        }
    };
    Ok(angle)
}

/// `image-set()`: chooses the candidate with resolution 1x (or the lowest
/// resolution if there is none).
/// <https://www.w3.org/TR/css-images-4/#image-set-notation>
fn parse_image_set(
    args: &mut Parser<'_>,
    cx: &ParserContext,
) -> ParseResult<Option<SpecifiedImage>> {
    let options = args.parse_comma_separated(|p| {
        let image = if let Ok(s) = p.expect_string() {
            Some(SpecifiedImage::Url(cx.resolve_url(s)))
        } else {
            parse_image(p, cx)?
        };
        let mut resolution = 1.0;
        for _ in 0..2 {
            if let Ok(r) = p.try_parse(parse_resolution) {
                resolution = r;
            } else if p.expect_function_matching("type").is_err() {
                break;
            }
        }
        Ok::<_, ParseError>((image, resolution))
    })?;
    let best = options
        .into_iter()
        .min_by(|a, b| {
            let key = |r: f32| (r - 1.0).abs() + if r < 1.0 { 0.5 } else { 0.0 };
            key(a.1).total_cmp(&key(b.1))
        })
        .ok_or(ParseError::Invalid)?;
    Ok(best.0)
}

/// `<resolution>` in dppx.
fn parse_resolution(p: &mut Parser<'_>) -> ParseResult<f32> {
    let (value, unit) = p.expect_dimension()?;
    let dppx = match unit.to_ascii_lowercase().as_str() {
        "x" | "dppx" => value,
        "dpi" => value / 96.0,
        "dpcm" => value * 2.54 / 96.0,
        _ => return Err(ParseError::Unexpected),
    };
    if dppx > 0.0 {
        Ok(dppx)
    } else {
        Err(ParseError::Invalid)
    }
}

/// True if `value` starts an `<image>` (for the `background` shorthand,
/// which must tell images from colors).
pub(crate) fn looks_like_image(value: &ComponentValue) -> bool {
    match value {
        ComponentValue::Url(_) => true,
        ComponentValue::Function(f) => {
            let name = f.name.to_ascii_lowercase();
            name == "url" || name.contains("gradient") || name.ends_with("image-set")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::test_util::parse_all;
    use crate::values::{LengthPercentage, Rgba};
    use url::Url;

    fn cx() -> ParserContext {
        let base = Url::parse("https://example.com/css/site.css").expect("valid URL");
        ParserContext::author(Some(Arc::new(base)), false)
    }

    #[allow(clippy::option_option)] // Invalid, valid without image, image.
    fn image(css: &str) -> Option<Option<Image>> {
        let cx = cx();
        parse_all(css, |p| parse_image(p, &cx))
            .ok()
            .map(|i| i.map(|i| i.compute(&LengthContext::DEFAULT)))
    }

    fn gradient(css: &str) -> Arc<LinearGradient> {
        match image(css) {
            Some(Some(Image::LinearGradient(g))) => g,
            other => panic!("{css}: expected a gradient, got {other:?}"),
        }
    }

    #[test]
    fn urls() {
        assert_eq!(
            image("url(a.png)"),
            Some(Some(Image::Url("https://example.com/css/a.png".into())))
        );
        assert_eq!(
            image("url('/b.png')"),
            Some(Some(Image::Url("https://example.com/b.png".into())))
        );
        assert_eq!(image("nope"), None);
        assert_eq!(image("url('')"), Some(None));
    }

    #[test]
    fn linear_gradients() {
        let red = Color::Rgba(Rgba::rgb(255, 0, 0));
        let blue = Color::Rgba(Rgba::rgb(0, 0, 255));
        let g = gradient("linear-gradient(red, blue)");
        assert_eq!(g.angle_deg, 180.0);
        assert_eq!(g.stops, vec![(red, None), (blue, None)]);
        assert!(!g.repeating);
        assert_eq!(
            gradient("linear-gradient(to right, red, blue)").angle_deg,
            90.0
        );
        assert_eq!(
            gradient("linear-gradient(to top left, red, blue)").angle_deg,
            315.0
        );
        assert_eq!(
            gradient("linear-gradient(45deg, red, blue)").angle_deg,
            45.0
        );
        assert_eq!(
            gradient("linear-gradient(-90deg, red, blue)").angle_deg,
            270.0
        );
        let g = gradient("repeating-linear-gradient(0deg, red 10px, blue 20%)");
        assert!(g.repeating);
        assert_eq!(g.stops[0].1, Some(LengthPercentage::Px(10.0)));
        assert_eq!(g.stops[1].1, Some(LengthPercentage::Percent(0.2)));
        let g = gradient("linear-gradient(red 0 50%, blue 50% 100%)");
        assert_eq!(g.stops.len(), 4);
        let g = gradient("linear-gradient(red, 30%, blue)");
        assert_eq!(g.stops.len(), 2);
        assert_eq!(
            gradient("-webkit-linear-gradient(top, red, blue)").angle_deg,
            180.0
        );
        assert_eq!(
            gradient("-webkit-linear-gradient(left, red, blue)").angle_deg,
            90.0
        );
        assert_eq!(
            gradient("linear-gradient(transparent, transparent)")
                .stops
                .len(),
            2
        );
        assert_eq!(image("linear-gradient(red)"), None);
        assert_eq!(image("linear-gradient(to middle, red, blue)"), None);
        assert_eq!(image("linear-gradient(red, 10%, 20%, blue)"), None);
        assert_eq!(image("linear-gradient(to top bottom, red, blue)"), None);
        assert_eq!(image("linear-gradient(10px red, blue)"), None);
    }

    #[test]
    fn unsupported_and_image_set() {
        assert_eq!(image("radial-gradient(red, blue)"), Some(None));
        assert_eq!(
            image("image-set('a.png' 1x, 'b.png' 2x)"),
            Some(Some(Image::Url("https://example.com/css/a.png".into())))
        );
        assert_eq!(
            image("-webkit-image-set(url(b.png) 2x, url(a.png) 1x)"),
            Some(Some(Image::Url("https://example.com/css/a.png".into())))
        );
        assert_eq!(
            image("image-set(url(c.png) type('image/png'))"),
            Some(Some(Image::Url("https://example.com/css/c.png".into())))
        );
    }
}
