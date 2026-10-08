//! `@font-face` rules: their descriptors, parsed and validated.
//!
//! <https://drafts.csswg.org/css-fonts-4/#font-face-rule>
//!
//! A rule needs a valid `font-family` and `src`; without them it is
//! dropped. Each descriptor that does not parse is ignored, so an earlier
//! valid declaration of the same descriptor stays. Values that the
//! specification leaves open follow Chromium 148, measured with
//! `tools/probes/web-fonts.json` (`src-lists`, `family-descriptor`,
//! `descriptor-values`).

use std::sync::Arc;

use swb_css::{ComponentValue, Declaration, FontFaceRule, ParseError, Parser};

use crate::font_settings::{
    FontFeatureSettings, FontVariationSettings, parse_feature_settings, parse_variation_settings,
};
use crate::parse::length::{CalcKind, parse_math_function};
use crate::parse::{ParserContext, parse_angle, parse_number};
use crate::properties::CssWideKeyword;
use crate::properties::longhand::parse_font_stretch;
use crate::values::{GenericFamily, LengthContext};

/// An `@font-face` rule with valid `font-family` and `src` descriptors.
#[derive(Clone, Debug, PartialEq)]
pub struct FontFace {
    /// The family name that the face belongs to.
    pub family: Arc<str>,
    /// The sources in priority order: only the entries whose format and
    /// technologies swb supports. Never empty.
    pub sources: Vec<FontFaceSource>,
    /// `font-weight`: an inclusive range (`min <= max`), or `None` for
    /// `auto`.
    pub weight: Option<(f32, f32)>,
    /// `font-stretch` (`font-width`) in percent: an inclusive range, or
    /// `None` for `auto`.
    pub stretch: Option<(f32, f32)>,
    /// `font-style`.
    pub style: FontFaceStyle,
    /// `unicode-range`: inclusive code point ranges, as written. The
    /// initial value is `U+0-10FFFF`.
    pub unicode_range: Vec<(u32, u32)>,
    /// `font-display`.
    pub display: FontDisplay,
    /// `size-adjust` as a ratio (1 is 100%, the initial value): it scales
    /// the glyphs and all metrics of the face. At most [`MAX_RATIO`].
    pub size_adjust: f32,
    /// `ascent-override` as a ratio of the used font size (after
    /// `size-adjust`); `None` is `normal`. At most [`MAX_RATIO`].
    pub ascent_override: Option<f32>,
    /// `descent-override`, like `ascent_override`.
    pub descent_override: Option<f32>,
    /// `line-gap-override`, like `ascent_override`.
    pub line_gap_override: Option<f32>,
    /// `font-variation-settings`: applied to the face before the
    /// property of the same name (measured in Chromium 148).
    pub variation_settings: FontVariationSettings,
    /// `font-feature-settings`: applied before the property.
    pub feature_settings: FontFeatureSettings,
}

/// The largest ratio of `size-adjust` and of the metric overrides (a
/// million percent): larger values are clamped. The used font size is
/// limited anyway, and finite ratios keep the metrics finite.
pub const MAX_RATIO: f32 = 10_000.0;

/// One entry of the `src` descriptor.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FontFaceSource {
    /// `url()`, resolved against the style sheet's URL.
    Url(Arc<str>),
    /// `local()`: the full name or PostScript name of an installed face.
    Local(Arc<str>),
}

/// The `font-style` descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum FontFaceStyle {
    /// `auto` (the initial value).
    #[default]
    Auto,
    /// `normal`.
    Normal,
    /// `italic`.
    Italic,
    /// `oblique` with an inclusive angle range in degrees (`min <= max`);
    /// `oblique` alone is 14deg.
    Oblique(f32, f32),
}

/// The `font-display` descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FontDisplay {
    /// `auto`.
    #[default]
    Auto,
    /// `block`.
    Block,
    /// `swap`.
    Swap,
    /// `fallback`.
    Fallback,
    /// `optional`.
    Optional,
}

/// The angle of `oblique` without an angle.
/// <https://drafts.csswg.org/css-fonts-4/#valdef-font-style-oblique-angle--90deg-90deg>
const DEFAULT_OBLIQUE_ANGLE: f32 = 14.0;

impl FontFace {
    /// Parses an `@font-face` rule. Returns `None` if it lacks a valid
    /// `font-family` or `src`. `cx` resolves URLs against the sheet.
    pub(crate) fn parse(rule: &FontFaceRule, cx: &ParserContext) -> Option<FontFace> {
        let mut family = None;
        let mut sources = None;
        let mut face = FontFace {
            family: Arc::from(""),
            sources: Vec::new(),
            weight: None,
            stretch: None,
            style: FontFaceStyle::Auto,
            unicode_range: vec![(0, 0x10_FFFF)],
            display: FontDisplay::Auto,
            size_adjust: 1.0,
            ascent_override: None,
            descent_override: None,
            line_gap_override: None,
            variation_settings: Arc::from([]),
            feature_settings: Arc::from([]),
        };
        for declaration in &rule.declarations {
            apply(declaration, cx, &mut face, &mut family, &mut sources);
        }
        face.family = family?;
        face.sources = sources?;
        Some(face)
    }
}

/// Parses one descriptor declaration into `face`. Invalid values are
/// ignored; unknown descriptors too.
fn apply(
    declaration: &Declaration,
    cx: &ParserContext,
    face: &mut FontFace,
    family: &mut Option<Arc<str>>,
    sources: &mut Option<Vec<FontFaceSource>>,
) {
    let value = &declaration.value;
    let parsed = match declaration.name.as_str() {
        "font-family" => entirely(value, parse_family_name).map(|f| *family = Some(f)),
        "src" => parse_sources(value, cx).map(|s| *sources = Some(s)),
        "font-weight" => entirely(value, |p| parse_weight(p).ok()).map(|w| face.weight = w),
        "font-stretch" | "font-width" => {
            entirely(value, |p| parse_stretch(p).ok()).map(|s| face.stretch = s)
        }
        "font-style" => entirely(value, parse_style).map(|s| face.style = s),
        "unicode-range" => parse_unicode_range(value).map(|r| face.unicode_range = r),
        "font-display" => entirely(value, parse_display).map(|d| face.display = d),
        "size-adjust" => entirely(value, parse_ratio).map(|r| face.size_adjust = r),
        "ascent-override" => {
            entirely(value, |p| parse_override(p).ok()).map(|r| face.ascent_override = r)
        }
        "descent-override" => {
            entirely(value, |p| parse_override(p).ok()).map(|r| face.descent_override = r)
        }
        "line-gap-override" => {
            entirely(value, |p| parse_override(p).ok()).map(|r| face.line_gap_override = r)
        }
        "font-variation-settings" => entirely(value, |p| parse_variation_settings(p).ok())
            .map(|s| face.variation_settings = s),
        "font-feature-settings" => {
            entirely(value, |p| parse_feature_settings(p).ok()).map(|s| face.feature_settings = s)
        }
        _ => Some(()),
    };
    if parsed.is_none() {
        log::debug!("ignored invalid @font-face descriptor {}", declaration.name);
    }
}

/// Runs `parse` on the whole value; `None` if it fails or leaves input.
fn entirely<T>(
    value: &[ComponentValue],
    parse: impl FnOnce(&mut Parser<'_>) -> Option<T>,
) -> Option<T> {
    let mut p = Parser::new(value);
    let result = parse(&mut p)?;
    p.expect_exhausted().ok()?;
    Some(result)
}

/// `<family-name>`: a string, or identifiers joined by single spaces. A
/// single identifier must not be a generic family, a CSS-wide keyword or
/// `default`. <https://drafts.csswg.org/css-fonts-4/#family-name-syntax>
fn parse_family_name(p: &mut Parser<'_>) -> Option<Arc<str>> {
    if let Ok(s) = p.expect_string() {
        return Some(Arc::from(s));
    }
    let mut words = vec![p.expect_ident().ok()?];
    while let Ok(word) = p.expect_ident() {
        words.push(word);
    }
    if let [single] = words.as_slice()
        && (GenericFamily::from_ident(single).is_some()
            || CssWideKeyword::from_ident(single).is_some()
            || single.eq_ignore_ascii_case("default"))
    {
        return None;
    }
    Some(Arc::from(words.join(" ")))
}

/// The `src` descriptor: entries whose syntax, format or technologies are
/// not supported are dropped; `None` if no entry is left.
/// <https://drafts.csswg.org/css-fonts-4/#font-face-src-parsing>
fn parse_sources(value: &[ComponentValue], cx: &ParserContext) -> Option<Vec<FontFaceSource>> {
    let sources: Vec<FontFaceSource> = value
        .split(ComponentValue::is_comma)
        .filter_map(|entry| entirely(entry, |p| parse_source(p, cx)))
        .collect();
    (!sources.is_empty()).then_some(sources)
}

/// One `src` entry: `<url> [format()]? [tech()]?` or `local()`. `None` if
/// it does not parse or is not supported.
fn parse_source(p: &mut Parser<'_>, cx: &ParserContext) -> Option<FontFaceSource> {
    if let Ok(mut args) = p.expect_function_matching("local") {
        let name = parse_family_name(&mut args)?;
        args.expect_exhausted().ok()?;
        return Some(FontFaceSource::Local(name));
    }
    let url = p.expect_url().ok()?;
    if url.trim().is_empty() {
        return None;
    }
    let mut supported = true;
    if let Ok(mut args) = p.expect_function_matching("format") {
        supported &= parse_format(&mut args)?;
        args.expect_exhausted().ok()?;
    }
    if let Ok(mut args) = p.expect_function_matching("tech") {
        let techs = args.parse_comma_separated(|p| parse_tech(p).ok_or(ParseError::Invalid));
        supported &= techs.ok()?.into_iter().all(|t| t);
    }
    supported.then(|| FontFaceSource::Url(cx.resolve_url(url)))
}

/// The argument of `format()`: `Some(true)` for a supported format,
/// `Some(false)` for a valid but unsupported one, `None` for a syntax
/// error. Measured in Chromium 148: a format string that is not in the
/// specification's table is skipped like an unsupported keyword.
fn parse_format(p: &mut Parser<'_>) -> Option<bool> {
    if let Ok(s) = p.expect_string() {
        return Some(matches!(
            s.to_ascii_lowercase().as_str(),
            "woff2"
                | "woff"
                | "truetype"
                | "opentype"
                | "collection"
                | "woff2-variations"
                | "woff-variations"
                | "truetype-variations"
                | "opentype-variations"
        ));
    }
    let keyword = p.expect_ident().ok()?.to_ascii_lowercase();
    match keyword.as_str() {
        "woff2" | "woff" | "truetype" | "opentype" | "collection" => Some(true),
        "embedded-opentype" | "svg" => Some(false),
        _ => None,
    }
}

/// One `tech()` keyword: `Some(true)` if supported, `Some(false)` if
/// valid but unsupported (as in Chromium 148: `color-SVG`,
/// `features-graphite` and `incremental`), `None` otherwise. Color
/// technologies count as supported, as in Chromium, although swb does not
/// draw color glyphs yet: the face still gives the layout its metrics.
fn parse_tech(p: &mut Parser<'_>) -> Option<bool> {
    let keyword = p.expect_ident().ok()?.to_ascii_lowercase();
    match keyword.as_str() {
        "variations" | "palettes" | "features-opentype" | "features-aat" | "color-colrv0"
        | "color-colrv1" | "color-sbix" | "color-cbdt" => Some(true),
        "features-graphite" | "color-svg" | "incremental" => Some(false),
        _ => None,
    }
}

/// `auto | <font-weight-absolute>{1,2}`. Measured in Chromium 148: the
/// keywords `normal` and `bold` are valid only alone.
fn parse_weight(p: &mut Parser<'_>) -> Result<Option<(f32, f32)>, ParseError> {
    if p.expect_ident_matching("auto").is_ok() {
        return Ok(None);
    }
    if let Ok(w) = p.expect_one_of(&[("normal", 400.0), ("bold", 700.0)]) {
        return Ok(Some((w, w)));
    }
    let weight = |p: &mut Parser<'_>| {
        p.try_parse(|p| {
            parse_number(p).and_then(|w| {
                if (1.0..=1000.0).contains(&w) {
                    Ok(w)
                } else {
                    Err(ParseError::Invalid)
                }
            })
        })
    };
    let first = weight(p)?;
    let second = weight(p).unwrap_or(first);
    Ok(Some(ordered(first, second)))
}

/// `auto | <'font-width'>{1,2}`. Measured in Chromium 148: keywords are
/// valid only alone.
fn parse_stretch(p: &mut Parser<'_>) -> Result<Option<(f32, f32)>, ParseError> {
    if p.expect_ident_matching("auto").is_ok() {
        return Ok(None);
    }
    let first_is_keyword = matches!(p.peek(), Some(ComponentValue::Ident(_)));
    let first = parse_font_stretch(p)?;
    if first_is_keyword {
        return Ok(Some((first, first)));
    }
    let second = p
        .try_parse(|p| {
            p.expect_percentage().and_then(|v| {
                if v >= 0.0 {
                    Ok(v)
                } else {
                    Err(ParseError::Invalid)
                }
            })
        })
        .unwrap_or(first);
    Ok(Some(ordered(first, second)))
}

/// `auto | normal | italic | oblique [<angle [-90deg,90deg]>{1,2}]?`.
fn parse_style(p: &mut Parser<'_>) -> Option<FontFaceStyle> {
    let keyword = p.expect_ident().ok()?.to_ascii_lowercase();
    match keyword.as_str() {
        "auto" => Some(FontFaceStyle::Auto),
        "normal" => Some(FontFaceStyle::Normal),
        "italic" => Some(FontFaceStyle::Italic),
        "oblique" => {
            let angle = |p: &mut Parser<'_>| {
                parse_angle(p, true)
                    .ok()
                    .filter(|a| (-90.0..=90.0).contains(a))
                    .ok_or(())
            };
            let Ok(first) = p.try_parse(angle) else {
                if p.is_exhausted() {
                    return Some(FontFaceStyle::Oblique(
                        DEFAULT_OBLIQUE_ANGLE,
                        DEFAULT_OBLIQUE_ANGLE,
                    ));
                }
                return None;
            };
            let second = p.try_parse(angle).unwrap_or(first);
            let (min, max) = ordered(first, second);
            Some(FontFaceStyle::Oblique(min, max))
        }
        _ => None,
    }
}

/// `<unicode-range-token>#`; any invalid range makes the whole value
/// invalid (the CSS parser leaves invalid ranges as other tokens).
fn parse_unicode_range(value: &[ComponentValue]) -> Option<Vec<(u32, u32)>> {
    let mut ranges = Vec::new();
    for entry in value.split(ComponentValue::is_comma) {
        let mut items = entry.iter().filter(|v| !v.is_whitespace());
        match (items.next(), items.next()) {
            (Some(ComponentValue::UnicodeRange { start, end }), None) => {
                ranges.push((*start, *end));
            }
            _ => return None,
        }
    }
    (!ranges.is_empty()).then_some(ranges)
}

/// `auto | block | swap | fallback | optional`.
fn parse_display(p: &mut Parser<'_>) -> Option<FontDisplay> {
    p.expect_one_of(&[
        ("auto", FontDisplay::Auto),
        ("block", FontDisplay::Block),
        ("swap", FontDisplay::Swap),
        ("fallback", FontDisplay::Fallback),
        ("optional", FontDisplay::Optional),
    ])
    .ok()
}

/// `<percentage [0,∞]>` (also `calc()`) as a ratio, clamped to
/// [`MAX_RATIO`]. Measured in Chromium 148: `0%` is valid (the face then
/// has size 0), a negative value or a number is not.
fn parse_ratio(p: &mut Parser<'_>) -> Option<f32> {
    let percent = if let Ok(v) = p.expect_percentage() {
        v
    } else {
        let expr = parse_math_function(p).ok()?;
        if expr.kind != CalcKind::Percentage {
            return None;
        }
        expr.node.compute(&LengthContext::DEFAULT).resolve(100.0)
    };
    (percent >= 0.0).then(|| (percent / 100.0).min(MAX_RATIO))
}

/// `normal | <percentage [0,∞]>`: `normal` is `None`.
fn parse_override(p: &mut Parser<'_>) -> Result<Option<f32>, ParseError> {
    if p.expect_ident_matching("normal").is_ok() {
        return Ok(None);
    }
    parse_ratio(p).map(Some).ok_or(ParseError::Invalid)
}

/// A range with its ends in increasing order: the specification swaps a
/// decreasing range.
fn ordered(a: f32, b: f32) -> (f32, f32) {
    if a <= b { (a, b) } else { (b, a) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swb_css::{CssRule, parse_stylesheet};
    use url::Url;

    fn faces(css: &str) -> Vec<FontFace> {
        let base = Arc::new(Url::parse("https://example.com/css/site.css").unwrap());
        let cx = ParserContext::author(Some(base), false);
        parse_stylesheet(css)
            .rules
            .iter()
            .filter_map(|r| match r {
                CssRule::FontFace(f) => FontFace::parse(f, &cx),
                _ => None,
            })
            .collect()
    }

    fn face(descriptors: &str) -> Option<FontFace> {
        faces(&format!(
            "@font-face {{ font-family: F; src: url(f.woff2); {descriptors} }}"
        ))
        .pop()
    }

    #[test]
    fn defaults_and_url_resolution() {
        let f = face("").unwrap();
        assert_eq!(&*f.family, "F");
        assert_eq!(
            f.sources,
            vec![FontFaceSource::Url(
                "https://example.com/css/f.woff2".into()
            )]
        );
        assert_eq!(f.weight, None);
        assert_eq!(f.stretch, None);
        assert_eq!(f.style, FontFaceStyle::Auto);
        assert_eq!(f.unicode_range, vec![(0, 0x10_FFFF)]);
        assert_eq!(f.display, FontDisplay::Auto);
    }

    #[test]
    fn family_names() {
        let family = |v: &str| {
            faces(&format!("@font-face {{ font-family: {v}; src: url(x) }}"))
                .pop()
                .map(|f| f.family.to_string())
        };
        assert_eq!(family("Two   Words").as_deref(), Some("Two Words"));
        assert_eq!(family("'serif'").as_deref(), Some("serif"));
        assert_eq!(family("Source Sans\\ 3").as_deref(), Some("Source Sans 3"));
        for invalid in ["serif", "'a', 'b'", "inherit", "default", "a 1", "initial"] {
            assert_eq!(family(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn rules_without_family_or_src_are_dropped() {
        assert_eq!(faces("@font-face { src: url(x) }").len(), 0);
        assert_eq!(faces("@font-face { font-family: A }").len(), 0);
        assert_eq!(
            faces("@font-face { font-family: A; src: url(x) format(svg) }").len(),
            0
        );
    }

    #[test]
    fn src_entries_and_formats() {
        let sources = |v: &str| {
            faces(&format!("@font-face {{ font-family: A; src: {v} }}"))
                .pop()
                .map(|f| f.sources)
                .unwrap_or_default()
        };
        let url = |s: &str| FontFaceSource::Url(format!("https://example.com/css/{s}").into());
        assert_eq!(
            sources(
                "url(a) format('embedded-opentype'), url(b) format(woff2), url('c') format('truetype-variations')"
            ),
            vec![url("b"), url("c")]
        );
        assert_eq!(
            sources(
                "url(a) format('bogus'), url(b) format(bogus), url(c) format(collection), url(d) format('collection-variations')"
            ),
            vec![url("c")]
        );
        assert_eq!(
            sources(
                "url(a) tech(color-SVG), url(b) tech(variations, color-COLRv1), url(c) tech('variations'), url(d) tech(incremental)"
            ),
            vec![url("b")]
        );
        assert_eq!(
            sources(
                "url(a) format(woff2) junk, local(Liberation Sans), local('X Y'), local(serif)"
            ),
            vec![
                FontFaceSource::Local("Liberation Sans".into()),
                FontFaceSource::Local("X Y".into())
            ]
        );
        assert_eq!(sources("url(a) tech(variations) format(woff2)"), vec![]);
    }

    #[test]
    fn weight_style_stretch() {
        assert_eq!(
            face("font-weight: 700 300").unwrap().weight,
            Some((300.0, 700.0))
        );
        assert_eq!(
            face("font-weight: bold").unwrap().weight,
            Some((700.0, 700.0))
        );
        assert_eq!(
            face("font-weight: calc(100 + 50)").unwrap().weight,
            Some((150.0, 150.0))
        );
        for invalid in ["0", "1001", "bolder", "normal bold", "100 200 300"] {
            assert_eq!(
                face(&format!("font-weight: {invalid}")).unwrap().weight,
                None
            );
        }
        assert_eq!(
            face("font-weight: 400; font-weight: junk").unwrap().weight,
            Some((400.0, 400.0))
        );
        assert_eq!(
            face("font-style: oblique 30deg 10deg").unwrap().style,
            FontFaceStyle::Oblique(10.0, 30.0)
        );
        assert_eq!(
            face("font-style: oblique").unwrap().style,
            FontFaceStyle::Oblique(14.0, 14.0)
        );
        assert_eq!(
            face("font-style: italic").unwrap().style,
            FontFaceStyle::Italic
        );
        for invalid in ["oblique 100deg", "italic 10deg", "bogus"] {
            assert_eq!(
                face(&format!("font-style: {invalid}")).unwrap().style,
                FontFaceStyle::Auto
            );
        }
        assert_eq!(
            face("font-stretch: 150% 50%").unwrap().stretch,
            Some((50.0, 150.0))
        );
        assert_eq!(
            face("font-stretch: condensed").unwrap().stretch,
            Some((75.0, 75.0))
        );
        assert_eq!(
            face("font-width: 300%").unwrap().stretch,
            Some((300.0, 300.0))
        );
        assert_eq!(
            face("font-stretch: condensed expanded").unwrap().stretch,
            None
        );
    }

    #[test]
    fn unicode_range_and_display() {
        assert_eq!(
            face("unicode-range: U+0-7F, u+4??").unwrap().unicode_range,
            vec![(0, 0x7F), (0x400, 0x4FF)]
        );
        for invalid in ["U+110000", "U+50-40", "U+0-7F, foo", "U+??????"] {
            assert_eq!(
                face(&format!("unicode-range: {invalid}"))
                    .unwrap()
                    .unicode_range,
                vec![(0, 0x10_FFFF)],
                "{invalid}"
            );
        }
        assert_eq!(
            face("font-display: optional").unwrap().display,
            FontDisplay::Optional
        );
        assert_eq!(
            face("font-display: bogus").unwrap().display,
            FontDisplay::Auto
        );
    }

    #[test]
    fn metric_descriptors() {
        let f = face("").unwrap();
        assert_eq!(f.size_adjust, 1.0);
        assert_eq!(
            (f.ascent_override, f.descent_override, f.line_gap_override),
            (None, None, None)
        );
        let f = face(
            "size-adjust: 93.75%; ascent-override: 98%; descent-override: 0%; line-gap-override: normal",
        )
        .unwrap();
        assert_eq!(f.size_adjust, 0.9375);
        assert_eq!(f.ascent_override, Some(0.98));
        assert_eq!(f.descent_override, Some(0.0));
        assert_eq!(f.line_gap_override, None);
        // Measured in Chromium 148: 0% is valid, calc() works, and the
        // last valid declaration stays.
        assert_eq!(face("size-adjust: 0%").unwrap().size_adjust, 0.0);
        assert_eq!(
            face("size-adjust: calc(25% + 25%)").unwrap().size_adjust,
            0.5
        );
        assert_eq!(
            face("size-adjust: 50%; size-adjust: bogus")
                .unwrap()
                .size_adjust,
            0.5
        );
        // Ratios are clamped.
        assert_eq!(face("size-adjust: 1e30%").unwrap().size_adjust, MAX_RATIO);
        for invalid in ["-50%", "100", "50% 60%", "1px", "calc(1px)", "auto"] {
            let f = face(&format!("size-adjust: {invalid}")).unwrap();
            assert_eq!(f.size_adjust, 1.0, "{invalid}");
        }
        for invalid in ["-10%", "90% 80%", "10", "bogus"] {
            let f = face(&format!("ascent-override: {invalid}")).unwrap();
            assert_eq!(f.ascent_override, None, "{invalid}");
        }
    }

    #[test]
    fn settings_descriptors() {
        let f = face("font-variation-settings: 'wght' 900, 'wdth' 125; font-feature-settings: 'liga' 0, 'kern' off").unwrap();
        assert_eq!(
            &*f.variation_settings,
            &[(*b"wdth", 125.0), (*b"wght", 900.0)]
        );
        assert_eq!(&*f.feature_settings, &[(*b"kern", 0), (*b"liga", 0)]);
        // An invalid declaration is ignored; the earlier one stays.
        let f =
            face("font-variation-settings: 'wght' 900; font-variation-settings: bogus").unwrap();
        assert_eq!(&*f.variation_settings, &[(*b"wght", 900.0)]);
        let f =
            face("font-variation-settings: 'wght' 900; font-variation-settings: normal").unwrap();
        assert!(f.variation_settings.is_empty());
    }
}
