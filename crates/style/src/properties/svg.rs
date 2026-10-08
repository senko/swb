//! Parsing the SVG fill and stroke properties (SVG 2 §13,
//! <https://svgwg.org/svg2-draft/painting.html>): `fill`, `fill-rule`,
//! `fill-opacity`, `stroke`, `stroke-width`, `stroke-linecap`,
//! `stroke-linejoin`, `stroke-miterlimit`, `stroke-dasharray`,
//! `stroke-dashoffset` and `stroke-opacity`. All are inherited. Also the
//! `clip-path` value (`parse_clip_path`) and the shared opacity grammar
//! (`parse_alpha`, used by `opacity` too).
//!
//! Lengths of these properties also accept plain numbers (user units,
//! which are px in CSS).

use std::sync::Arc;

use swb_css::{ParseError, Parser};

use crate::parse::ParseResult;
use crate::parse::color::parse_color;
use crate::parse::length::{LengthOptions, parse_length_percentage};
use crate::parse::{parse_non_negative_number, parse_number};
use crate::values::{ClipPath, SpecifiedLengthPercentage as Lp, SvgPaint};

/// The most entries of a `stroke-dasharray`. A longer list is invalid, so
/// that a hostile page cannot make every element carry a huge list (real
/// pages use a few entries; Chromium has no limit).
pub(crate) const MAX_DASH_ENTRIES: usize = 256;

/// `<paint> = none | <color> | <url> [none | <color>]?`. `context-fill`
/// and `context-stroke` are not supported (invalid).
/// <https://svgwg.org/svg2-draft/painting.html#SpecifyingPaint>
pub(crate) fn parse_paint(p: &mut Parser<'_>) -> ParseResult<SvgPaint> {
    if p.expect_ident_matching("none").is_ok() {
        return Ok(SvgPaint::None);
    }
    if let Ok(url) = p.expect_url() {
        let url: Arc<str> = Arc::from(url.trim());
        if p.is_exhausted() {
            return Ok(SvgPaint::Url {
                url,
                fallback: None,
            });
        }
        let fallback = if p.expect_ident_matching("none").is_ok() {
            None
        } else {
            Some(parse_color(p)?)
        };
        return Ok(SvgPaint::Url { url, fallback });
    }
    parse_color(p).map(SvgPaint::Color)
}

/// `clip-path`: `none | <url>` (the basic shapes of CSS Masking 1 are not
/// supported). Chromium 148 accepts one `url()` only, so `url(#a) url(#b)`
/// is invalid.
pub(crate) fn parse_clip_path(p: &mut Parser<'_>) -> ParseResult<ClipPath> {
    if p.expect_ident_matching("none").is_ok() {
        return Ok(ClipPath::None);
    }
    let url = p.expect_url()?;
    Ok(ClipPath::Url(Arc::from(url.trim())))
}

/// `opacity`, `fill-opacity`, `stroke-opacity`: `<number> | <percentage>`,
/// clamped to 0..1 when computed.
pub(crate) fn parse_alpha(p: &mut Parser<'_>) -> ParseResult<f32> {
    if let Ok(v) = p.expect_percentage() {
        return Ok(v / 100.0);
    }
    parse_number(p)
}

/// `stroke-width`: `<length-percentage [0,∞]> | <number [0,∞]>`.
pub(crate) fn parse_stroke_width(p: &mut Parser<'_>) -> ParseResult<Lp> {
    parse_length_percentage(p, LengthOptions::NON_NEGATIVE.with_quirks(true))
}

/// `stroke-dashoffset`: `<length-percentage> | <number>`.
pub(crate) fn parse_dash_offset(p: &mut Parser<'_>) -> ParseResult<Lp> {
    parse_length_percentage(p, LengthOptions::LENGTH_PERCENTAGE.with_quirks(true))
}

/// `stroke-miterlimit`: `<number [0,∞]>`. A negative value is invalid.
/// Chromium keeps a value below 1 as it is (computed `0.5`, `0`) and draws
/// a bevel, as for 1 (measured, `tools/probes/inline-svg.json`, case
/// `paint-miterlimit-invalid`), so the stroke code clamps it to 1.
pub(crate) fn parse_miter_limit(p: &mut Parser<'_>) -> ParseResult<f32> {
    parse_non_negative_number(p)
}

/// `stroke-dasharray`: `none | [<length-percentage [0,∞]> | <number
/// [0,∞]>]+`, separated by commas, white space or both (SVG 2 §13.5.7).
/// `none` is an empty list. Negative values make the declaration
/// invalid, so the dashes of the parent apply (Chromium draws a solid
/// stroke for `10 -5`).
pub(crate) fn parse_dash_array(p: &mut Parser<'_>) -> ParseResult<Arc<[Lp]>> {
    if p.expect_ident_matching("none").is_ok() {
        return Ok(Arc::from([]));
    }
    let mut values = Vec::new();
    loop {
        values.push(parse_length_percentage(
            p,
            LengthOptions::NON_NEGATIVE.with_quirks(true),
        )?);
        if values.len() > MAX_DASH_ENTRIES {
            return Err(ParseError::Invalid);
        }
        if p.is_exhausted() {
            break;
        }
        // An optional comma; a comma at the end is invalid.
        let _ = p.expect_comma();
    }
    Ok(Arc::from(values))
}

#[cfg(test)]
mod tests {
    use swb_css::{ComponentValue, Parser, parse_component_values};

    use super::*;
    use crate::values::{Color, Length, Rgba};

    fn parse<T>(css: &str, f: impl FnOnce(&mut Parser<'_>) -> ParseResult<T>) -> Option<T> {
        let values: Vec<ComponentValue> = parse_component_values(css);
        Parser::new(&values).parse_entirely(f).ok()
    }

    #[test]
    fn paints() {
        assert_eq!(parse("none", parse_paint), Some(SvgPaint::None));
        assert_eq!(
            parse("red", parse_paint),
            Some(SvgPaint::Color(Color::Rgba(Rgba::rgb(255, 0, 0))))
        );
        assert_eq!(
            parse("currentcolor", parse_paint),
            Some(SvgPaint::Color(Color::CurrentColor))
        );
        assert_eq!(
            parse("url(#a)", parse_paint),
            Some(SvgPaint::Url {
                url: Arc::from("#a"),
                fallback: None
            })
        );
        assert_eq!(
            parse("url(#a) none", parse_paint),
            Some(SvgPaint::Url {
                url: Arc::from("#a"),
                fallback: None
            })
        );
        assert_eq!(
            parse("url('#a') blue", parse_paint),
            Some(SvgPaint::Url {
                url: Arc::from("#a"),
                fallback: Some(Color::Rgba(Rgba::rgb(0, 0, 255)))
            })
        );
        assert_eq!(parse("bogus", parse_paint), None);
        assert_eq!(parse("context-fill", parse_paint), None);
        assert_eq!(parse("red blue", parse_paint), None);
    }

    #[test]
    fn clip_paths() {
        let url = |s: &str| Some(ClipPath::Url(Arc::from(s)));
        assert_eq!(parse("none", parse_clip_path), Some(ClipPath::None));
        assert_eq!(parse("url(#a)", parse_clip_path), url("#a"));
        assert_eq!(parse("url( '#a' )", parse_clip_path), url("#a"));
        assert_eq!(parse("url(a.svg#b)", parse_clip_path), url("a.svg#b"));
        // Chromium accepts one `url()`; basic shapes are not supported.
        assert_eq!(parse("url(#a) url(#b)", parse_clip_path), None);
        assert_eq!(parse("circle(5px)", parse_clip_path), None);
        assert_eq!(parse("#a", parse_clip_path), None);
    }

    #[test]
    fn dash_arrays() {
        let px = |v| Lp::Length(Length::px(v));
        assert_eq!(parse("none", parse_dash_array).as_deref(), Some(&[][..]));
        assert_eq!(
            parse("10 5,2 , 1", parse_dash_array).as_deref(),
            Some(&[px(10.0), px(5.0), px(2.0), px(1.0)][..])
        );
        assert_eq!(
            parse("10%", parse_dash_array).as_deref(),
            Some(&[Lp::Percentage(0.1)][..])
        );
        assert_eq!(parse("10 -5", parse_dash_array), None);
        assert_eq!(parse("10,", parse_dash_array), None);
        assert_eq!(parse("10,,5", parse_dash_array), None);
        let long = vec!["1"; MAX_DASH_ENTRIES + 1].join(" ");
        assert_eq!(parse(&long, parse_dash_array), None);
        let ok = vec!["1"; MAX_DASH_ENTRIES].join(" ");
        assert!(parse(&ok, parse_dash_array).is_some());
    }

    #[test]
    fn numbers_and_lengths() {
        assert_eq!(
            parse("2", parse_stroke_width),
            Some(Lp::Length(Length::px(2.0)))
        );
        assert_eq!(parse("-2", parse_stroke_width), None);
        assert_eq!(
            parse("-2", parse_dash_offset),
            Some(Lp::Length(Length::px(-2.0)))
        );
        assert_eq!(parse("50%", parse_alpha), Some(0.5));
        assert_eq!(parse("0.25", parse_alpha), Some(0.25));
        assert_eq!(parse("-1", parse_miter_limit), None);
    }
}
