//! SVG presentation attributes: attributes of SVG elements that set CSS
//! properties (SVG 2 §6.6,
//! <https://svgwg.org/svg2-draft/styling.html#PresentationAttributes>).
//!
//! Like the presentational hints of HTML, they are author-level
//! declarations with specificity zero that come before all author rules,
//! so style sheets and `style` attributes override them and inheritance
//! works as for any property. The value is parsed with the property's CSS
//! grammar; an invalid value is ignored.
//!
//! Supported: the fill and stroke properties (`fill`, `fill-opacity`,
//! `fill-rule`, `stroke`, `stroke-*`), `clip-path`, `clip-rule`,
//! `opacity`, `display`, `visibility`, `pointer-events`,
//! `shape-rendering`, `color`, `overflow`, `transform` (in the SVG syntax of
//! the attribute, parsed by `svgtypes`), and `width` and `height` of
//! `svg` elements. The geometry attributes of shapes (`x`, `r`, `d`, ...)
//! are read by layout, not mapped to properties.

use std::str::FromStr;
use std::sync::Arc;

use swb_css::{ComponentValue, Parser, parse_component_values};
use swb_dom::{ElementData, is_html_whitespace, local_name};

use crate::parse::ParserContext;
use crate::parse::length::{LengthOptions, parse_length_percentage};
use crate::properties::ids::LonghandValue;
use crate::properties::specified::SpecifiedSize;
use crate::properties::transform::SpecifiedTransformFunction;
use crate::properties::{PropertyDeclaration, parse_declaration};
use crate::values::{Length, SpecifiedLengthPercentage as Lp};

/// The presentation attributes that map to the property of the same name.
const PROPERTY_ATTRIBUTES: &[&str] = &[
    "clip-path",
    "clip-rule",
    "color",
    "display",
    "fill",
    "fill-opacity",
    "fill-rule",
    "opacity",
    "overflow",
    "pointer-events",
    "shape-rendering",
    "stroke",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-opacity",
    "stroke-width",
    "visibility",
];

/// Appends the declarations of the presentation attributes of SVG element
/// `e` to `out`. `cx` is the author parser context of the document.
pub(crate) fn collect_svg_hints(
    e: &ElementData,
    cx: &ParserContext,
    out: &mut Vec<PropertyDeclaration>,
) {
    let cx = ParserContext {
        quirks: false,
        ..cx.clone()
    };
    let is_svg = *e.local_name() == local_name!("svg");
    for attr in e.attributes() {
        if attr.name.ns != swb_dom::ns!() {
            continue;
        }
        let name = &*attr.name.local;
        let value = &*attr.value;
        if PROPERTY_ATTRIBUTES.contains(&name) {
            let tokens: Vec<ComponentValue> = parse_component_values(value);
            parse_declaration(name, &tokens, &cx, out);
        } else if name == "transform" {
            if let Some(matrix) = parse_transform_attribute(value) {
                out.push(PropertyDeclaration::Value(LonghandValue::Transform(
                    Arc::from([SpecifiedTransformFunction::Matrix(matrix)]),
                )));
            }
        } else if is_svg
            && (name == "width" || name == "height")
            && let Some(size) = svg_size(value)
        {
            out.push(PropertyDeclaration::Value(if name == "width" {
                LonghandValue::Width(size)
            } else {
                LonghandValue::Height(size)
            }));
        }
    }
}

/// The `transform` attribute (SVG 1.1 syntax: unitless numbers, angles in
/// degrees, `rotate(a cx cy)`, white space or commas between arguments)
/// as one matrix, or `None` if it is invalid (Chromium 148 then ignores
/// the whole attribute) or not finite.
/// <https://svgwg.org/svg2-draft/coords.html#TransformProperty>
fn parse_transform_attribute(value: &str) -> Option<[f32; 6]> {
    let t = svgtypes::Transform::from_str(value).ok()?;
    let m = [t.a, t.b, t.c, t.d, t.e, t.f].map(|v| v as f32);
    m.iter().all(|v| v.is_finite()).then_some(m)
}

/// The `width` or `height` attribute of an `svg` element as a size, as
/// Chromium 148 maps it (measured, `tools/probes/inline-svg.json`): a
/// `<length-percentage>` where numbers are px, `calc()` included, white
/// space around it ignored; `auto` maps to nothing; a negative length is
/// 0; any other value is `100%` (the initial value of the attribute in
/// SVG 1.1).
fn svg_size(value: &str) -> Option<SpecifiedSize> {
    let value = value.trim_matches(is_html_whitespace);
    if value.eq_ignore_ascii_case("auto") {
        return None;
    }
    let tokens: Vec<ComponentValue> = parse_component_values(value);
    let parsed = Parser::new(&tokens).parse_entirely(|p| {
        parse_length_percentage(p, LengthOptions::LENGTH_PERCENTAGE.with_quirks(true))
    });
    let lp = match parsed {
        Ok(Lp::Length(l)) if l.value < 0.0 => Lp::Length(Length::px(0.0)),
        Ok(Lp::Percentage(p)) if p < 0.0 => Lp::Percentage(0.0),
        Ok(lp) => lp,
        Err(_) => Lp::Percentage(1.0),
    };
    Some(SpecifiedSize::LengthPercentage(lp))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::values::LengthUnit;

    fn size(v: &str) -> Option<SpecifiedSize> {
        svg_size(v)
    }

    #[test]
    fn svg_sizes() {
        let px = |v| Some(SpecifiedSize::LengthPercentage(Lp::Length(Length::px(v))));
        assert_eq!(size("10"), px(10.0));
        assert_eq!(size(" 12 "), px(12.0));
        assert_eq!(size("10px"), px(10.0));
        assert_eq!(size("-10"), px(0.0));
        assert_eq!(size("auto"), None);
        assert_eq!(
            size("50%"),
            Some(SpecifiedSize::LengthPercentage(Lp::Percentage(0.5)))
        );
        assert_eq!(
            size("abc"),
            Some(SpecifiedSize::LengthPercentage(Lp::Percentage(1.0)))
        );
        assert_eq!(
            size("10 px"),
            Some(SpecifiedSize::LengthPercentage(Lp::Percentage(1.0)))
        );
        assert_eq!(
            size("1em"),
            Some(SpecifiedSize::LengthPercentage(Lp::Length(Length {
                value: 1.0,
                unit: LengthUnit::Em
            })))
        );
    }

    #[test]
    fn presentation_attributes_cascade() {
        use swb_css::MediaEnvironment;
        use swb_dom::parse_html;

        use crate::values::{Color, LengthPercentage, Rgba, SvgPaint, TransformFunction};
        use crate::{ElementStates, Stylist, compute_styles};

        let doc = parse_html(
            "<!DOCTYPE html><svg id=s fill=red stroke-width=3 width=10 height=abc \
             transform='translate(1 2)' opacity=0.5><rect id=a class=c fill=green />\
             <rect id=b /><rect id=c style='fill: yellow' fill=green /><rect id=d fill=inherit \
             stroke-width=-1 /><rect id=e fill=bogus /></svg><div id=h fill=red></div><svg id=n display=none></svg>",
        );
        let base = url::Url::parse("https://example.com/").expect("valid URL");
        let mut stylist = Stylist::new(doc.quirks_mode);
        stylist.add_author_sheet(
            &swb_css::parse_stylesheet(".c { fill: blue } svg { stroke-width: 2 }"),
            &base,
        );
        let map = compute_styles(
            &doc,
            &stylist,
            &MediaEnvironment::default(),
            &ElementStates::default(),
            &base,
        );
        let style = |id: &str| {
            let node = doc.element_by_id(id).expect("an element");
            map.get(node).expect("a style").clone()
        };
        let color = |r, g, b| SvgPaint::Color(Color::Rgba(Rgba::rgb(r, g, b)));
        let s = style("s");
        // A style sheet beats the attribute; the attribute beats inheritance.
        assert_eq!(s.stroke_width, LengthPercentage::Px(2.0));
        assert_eq!(s.fill, color(255, 0, 0));
        assert_eq!(s.opacity, 0.5);
        assert_eq!(style("n").display, crate::values::Display::None);
        assert_eq!(
            &*s.transform,
            &[TransformFunction::Matrix([1.0, 0.0, 0.0, 1.0, 1.0, 2.0])]
        );
        assert_eq!(style("a").fill, color(0, 0, 255));
        assert_eq!(style("b").fill, color(255, 0, 0));
        assert_eq!(style("c").fill, color(255, 255, 0));
        assert_eq!(style("d").fill, color(255, 0, 0));
        assert_eq!(style("d").stroke_width, LengthPercentage::Px(2.0));
        assert_eq!(style("e").fill, color(255, 0, 0));
        // SVG content transforms about the origin of its user space.
        assert_eq!(style("b").transform_origin.x, LengthPercentage::ZERO);
        // HTML elements have no presentation attributes.
        assert_eq!(style("h").fill, SvgPaint::BLACK);
    }

    #[test]
    fn transform_attributes() {
        assert_eq!(
            parse_transform_attribute("translate(10 30)"),
            Some([1.0, 0.0, 0.0, 1.0, 10.0, 30.0])
        );
        assert_eq!(
            parse_transform_attribute("translate(10,30) scale(2)"),
            Some([2.0, 0.0, 0.0, 2.0, 10.0, 30.0])
        );
        assert_eq!(parse_transform_attribute("translate(30 30) bogus"), None);
        assert_eq!(parse_transform_attribute("scale(1e39)"), None);
        let r = parse_transform_attribute("rotate(90 50 50)").expect("valid");
        assert!((r[1] - 1.0).abs() < 1e-6 && (r[4] - 100.0).abs() < 1e-4 && r[5].abs() < 1e-4);
    }
}
