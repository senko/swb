//! Tests of the interpolation against Chromium 148: each test builds the
//! two keyframe styles of a case of
//! `crates/automation/tests/data/css-animations.json` and compares the
//! `getComputedStyle` text of the interpolated value at the rows' seek
//! times (linear 1 s animations, so the progress is the time / 1000).

// The numbers are Chromium's values as it prints them.
#![allow(clippy::unreadable_literal)]

use swb_css::parse_style_attribute;

use super::*;
use crate::cssom::resolved::{Resolver, resolved_value};
use crate::parse::ParserContext;
use crate::properties::compute::{ComputeContext, apply};
use crate::properties::{DeclarationBlock, PropertyDeclaration};
use crate::values::LengthContext;

/// The style with the declarations `css` computed on the initial style
/// (font properties first; `color: rgb(0, 128, 0)` unless `css` sets one).
pub(super) fn style(css: &str) -> ComputedStyle {
    let block = DeclarationBlock::parse(
        &parse_style_attribute(&format!("color: rgb(0, 128, 0); {css}")),
        &ParserContext::author(None, false),
    );
    let initial = ComputedStyle::initial();
    let mut s = (*initial).clone();
    let mut cx = ComputeContext {
        parent: &initial,
        lengths: LengthContext::DEFAULT,
        element: None,
        quirks: false,
    };
    let is_font = |v: &crate::properties::ids::LonghandValue| v.id() == LonghandId::FontSize;
    for pass in [true, false] {
        for decl in &block.normal {
            if let PropertyDeclaration::Value(v) = decl
                && is_font(v) == pass
            {
                apply(v, &cx, &mut s);
            }
        }
        cx.lengths.font_size = s.font_size;
    }
    s
}

/// The `getComputedStyle` text of `name` between the declarations `a` and
/// `b` of `name` at progress `p`.
fn at(name: &str, a: &str, b: &str, p: f64) -> String {
    let id = LonghandId::from_name(name).expect("a longhand");
    let (from, to) = (
        style(&format!("{name}: {a}")),
        style(&format!("{name}: {b}")),
    );
    let mut out = style("");
    interpolate_longhand(id, &from, &to, p, &mut out);
    let resolver = Resolver {
        style: &out,
        used: None,
        pseudo: None,
    };
    resolved_value(name, &resolver)
}

/// Checks the rows `(progress, text)` of `name` from `a` to `b`.
fn rows(name: &str, a: &str, b: &str, rows: &[(f64, &str)]) {
    for (p, want) in rows {
        assert_eq!(at(name, a, b, *p), *want, "{name}: {a} to {b} at {p}");
    }
}

#[test]
fn lengths_and_numbers() {
    // `interp-lengths-numbers`, queries 0 to 3 and 6.
    rows(
        "text-indent",
        "10px",
        "50%",
        &[(0.25, "calc(12.5% + 7.5px)"), (0.5, "calc(25% + 5px)")],
    );
    rows("width", "auto", "100px", &[(0.4, "auto"), (0.6, "100px")]);
    rows(
        "line-height",
        "normal",
        "2",
        &[(0.4, "normal"), (0.6, "32px")],
    );
    rows("line-height", "1", "20px", &[(0.4, "16px"), (0.6, "20px")]);
    rows("font-weight", "400", "700", &[(0.25, "475"), (0.5, "550")]);
    rows(
        "z-index",
        "1",
        "10",
        &[(0.25, "3"), (0.5, "6"), (0.501, "6")],
    );
    // Query 4: `linear(0, 2 50%, 1)` overshoots to progress 2 at 250 and
    // 500 ms (opacity clamps to 1), `linear(0, -4 50%, 1)` to -4 (width
    // clamps to 0).
    rows("opacity", "0", "1", &[(2.0, "1"), (1.5, "1"), (-0.5, "0")]);
    rows("width", "20px", "40px", &[(-4.0, "0px"), (-1.5, "0px")]);
    // Query 9: border widths snap as at compute time (0.3px is 1px).
    rows(
        "border-top-width",
        "0px",
        "3px",
        &[(0.1, "1px"), (0.25, "1px"), (0.5, "1px"), (0.9, "2px")],
    );
    // Query 10: `inset` longhands, a percentage of the containing block
    // stays a percentage in the computed value.
    rows("top", "0px", "10px", &[(0.5, "5px")]);
    rows("left", "0px", "20%", &[(0.5, "10%")]);
    rows("right", "auto", "auto", &[(0.5, "auto")]);
    // Integers round half toward +infinity.
    rows("z-index", "-10", "1", &[(0.5, "-4")]);
    rows("order", "0", "1", &[(0.5, "1")]);
}

#[test]
fn keywords() {
    // `interp-lengths-numbers`, queries 5 and 6.
    let vis = [
        (0.0, "visible"),
        (0.001, "visible"),
        (0.5, "visible"),
        (0.999, "visible"),
        (1.0, "hidden"),
    ];
    rows("visibility", "visible", "hidden", &vis);
    let vis2 = [
        (0.0, "hidden"),
        (0.001, "visible"),
        (0.5, "visible"),
        (0.999, "visible"),
        (1.0, "visible"),
    ];
    rows("visibility", "hidden", "visible", &vis2);
    rows(
        "display",
        "block",
        "none",
        &[
            (0.0, "block"),
            (0.4, "block"),
            (0.6, "block"),
            (1.0, "none"),
        ],
    );
    rows(
        "content-visibility",
        "visible",
        "hidden",
        &[
            (0.0, "visible"),
            (0.4, "visible"),
            (0.6, "visible"),
            (1.0, "hidden"),
        ],
    );
    // Other keyword pairs flip at 0.5.
    rows(
        "display",
        "block",
        "flex",
        &[(0.49, "block"), (0.5, "flex")],
    );
    rows(
        "position",
        "static",
        "absolute",
        &[(0.49, "static"), (0.5, "absolute")],
    );
}

#[test]
fn colors() {
    // `interp-lengths-numbers`, query 8: `color` 0,0,0 to 200,100,0.
    rows(
        "color",
        "rgb(0,0,0)",
        "rgb(200,100,0)",
        &[(0.5, "rgb(100, 50, 0)")],
    );
    rows(
        "background-color",
        "rgb(255, 0, 0)",
        "rgb(0, 0, 255)",
        &[(0.25, "rgb(191, 0, 64)"), (0.5, "rgb(128, 0, 128)")],
    );
}

#[test]
fn filters() {
    // `interp-filters`.
    rows(
        "filter",
        "blur(0px)",
        "blur(10px) brightness(2)",
        &[
            (0.25, "blur(2.5px) brightness(1.25)"),
            (0.5, "blur(5px) brightness(1.5)"),
        ],
    );
    rows(
        "filter",
        "none",
        "drop-shadow(4px 4px 2px rgb(255, 0, 0))",
        &[
            (0.25, "drop-shadow(rgba(255, 0, 0, 0.25) 1px 1px 0.5px)"),
            (0.5, "drop-shadow(rgba(255, 0, 0, 0.5) 2px 2px 1px)"),
        ],
    );
    rows(
        "filter",
        "grayscale(1)",
        "blur(4px)",
        &[(0.25, "grayscale(1)"), (0.5, "blur(4px)")],
    );
    rows(
        "filter",
        "none",
        "blur(10px) sepia(1)",
        &[
            (0.25, "blur(2.5px) sepia(0.25)"),
            (0.5, "blur(5px) sepia(0.5)"),
        ],
    );
    rows(
        "filter",
        "hue-rotate(0deg)",
        "hue-rotate(360deg)",
        &[(0.25, "hue-rotate(90deg)"), (0.5, "hue-rotate(180deg)")],
    );
}

#[test]
fn shadows() {
    // `interp-box-shadows` (the element's color is rgb(0, 128, 0)).
    rows(
        "box-shadow",
        "0 0 0 rgb(255, 0, 0)",
        "10px 10px 10px 5px rgb(0, 0, 255)",
        &[
            (0.25, "rgb(191, 0, 64) 2.5px 2.5px 2.5px 1.25px"),
            (0.5, "rgb(128, 0, 128) 5px 5px 5px 2.5px"),
        ],
    );
    rows(
        "box-shadow",
        "none",
        "0 4px 6px rgba(0, 0, 0, 0.5), 0 2px 4px rgb(255, 0, 0)",
        &[
            (
                0.25,
                "rgba(0, 0, 0, 0.125) 0px 1px 1.5px 0px, rgba(255, 0, 0, 0.25) 0px 0.5px 1px 0px",
            ),
            (
                0.5,
                "rgba(0, 0, 0, 0.25) 0px 2px 3px 0px, rgba(255, 0, 0, 0.5) 0px 1px 2px 0px",
            ),
        ],
    );
    rows(
        "box-shadow",
        "inset 0 0 5px rgb(255, 0, 0)",
        "0 0 5px rgb(255, 0, 0)",
        &[
            (0.25, "rgb(255, 0, 0) 0px 0px 5px 0px inset"),
            (0.5, "rgb(255, 0, 0) 0px 0px 5px 0px"),
        ],
    );
    rows(
        "box-shadow",
        "1px 1px rgb(255, 0, 0)",
        "2px 2px rgb(0, 0, 255), 3px 3px rgb(0, 128, 0)",
        &[
            (
                0.25,
                "rgb(191, 0, 64) 1.25px 1.25px 0px 0px, rgba(0, 128, 0, 0.25) 0.75px 0.75px 0px 0px",
            ),
            (
                0.5,
                "rgb(128, 0, 128) 1.5px 1.5px 0px 0px, rgba(0, 128, 0, 0.5) 1.5px 1.5px 0px 0px",
            ),
        ],
    );
    rows(
        "box-shadow",
        "0 0 4px",
        "0 0 4px rgb(255, 0, 0)",
        &[
            (0.25, "rgb(64, 96, 0) 0px 0px 4px 0px"),
            (0.5, "rgb(128, 64, 0) 0px 0px 4px 0px"),
        ],
    );
    // `interp-text-shadows`.
    rows(
        "text-shadow",
        "0 0 rgb(255, 0, 0)",
        "4px 4px 2px rgb(0, 0, 255)",
        &[(0.5, "rgb(128, 0, 128) 2px 2px 1px")],
    );
    rows(
        "text-shadow",
        "none",
        "2px 2px rgb(0, 0, 0)",
        &[(0.5, "rgba(0, 0, 0, 0.5) 1px 1px 0px")],
    );
}

#[test]
fn backgrounds() {
    // `interp-background-position` (`background-position` is the pair of
    // its longhands).
    rows(
        "background-position-x",
        "0%",
        "100%",
        &[(0.25, "25%"), (0.5, "50%")],
    );
    rows(
        "background-position-x",
        "left",
        "right 10px",
        &[(0.25, "calc(25% - 2.5px)"), (0.5, "calc(50% - 5px)")],
    );
    rows(
        "background-position-y",
        "top",
        "bottom 20px",
        &[(0.25, "calc(25% - 5px)"), (0.5, "calc(50% - 10px)")],
    );
    rows(
        "background-position-x",
        "10px",
        "50%",
        &[(0.25, "calc(12.5% + 7.5px)"), (0.5, "calc(25% + 5px)")],
    );
    // `bp-list`: two layers against one; `getComputedStyle` shows as many
    // layers as `background-image` has (one).
    rows(
        "background-position-x",
        "0px, 0px",
        "10px",
        &[(0.25, "2.5px"), (0.5, "5px")],
    );
    // `interp-background-size`.
    rows(
        "background-size",
        "10px 10px",
        "50% 20px",
        &[
            (0.25, "calc(12.5% + 7.5px) 12.5px"),
            (0.5, "calc(25% + 5px) 15px"),
        ],
    );
    rows(
        "background-size",
        "auto",
        "100px",
        &[(0.25, "auto"), (0.5, "100px")],
    );
    rows(
        "background-size",
        "10px auto",
        "30px auto",
        &[(0.25, "15px"), (0.5, "20px")],
    );
    rows(
        "background-size",
        "cover",
        "contain",
        &[(0.25, "cover"), (0.5, "contain")],
    );
    rows(
        "background-size",
        "20px 100%",
        "50% 100%",
        &[
            (0.25, "calc(12.5% + 15px) 100%"),
            (0.5, "calc(25% + 10px) 100%"),
        ],
    );
}

#[test]
fn individual_transforms() {
    // `interp-individual-transforms` at 500 ms.
    rows("rotate", "0deg", "90deg", &[(0.5, "45deg")]);
    rows("rotate", "none", "45deg", &[(0.5, "22.5deg")]);
    rows("scale", "1", "2 3", &[(0.5, "1.5 2")]);
    rows("scale", "none", "2", &[(0.5, "1.5")]);
    rows("translate", "0", "10px 50%", &[(0.5, "5px 25%")]);
    rows("translate", "none", "20px", &[(0.5, "10px")]);
    // `none` to `none` stays `none`.
    for name in ["translate", "rotate", "scale"] {
        rows(name, "none", "none", &[(0.5, "none")]);
    }
}

/// A progress far outside 0 to 1 (an easing with huge outputs, such as
/// `linear(1e300 0%, 0 100%)`) gives finite lengths, numbers and matrices.
#[test]
fn huge_progress_stays_finite() {
    let cases = [
        ("left", "0px", "10px"),
        ("width", "10%", "20px"),
        ("opacity", "0", "1"),
        ("z-index", "0", "5"),
        (
            "transform",
            "translateX(10px) rotate(0deg)",
            "scale(2) skewX(10deg)",
        ),
        ("transform", "rotate(45deg)", "translate(-50%)"),
        ("translate", "0", "10px 50%"),
        ("background-position-x", "0px", "calc(10% + 2px)"),
        ("box-shadow", "1px 1px red", "2px 3px 4px blue"),
        ("filter", "blur(1px)", "blur(2px)"),
        ("color", "red", "oklch(0.7 0.1 200)"),
    ];
    for (name, a, b) in cases {
        for p in [1e300, -1e300, f64::MAX, -f64::MAX] {
            let text = at(name, a, b, p);
            assert!(
                !text.contains("inf") && !text.contains("NaN"),
                "{name}: {a} to {b} at {p}: {text}"
            );
        }
    }
}

#[test]
fn hostile_repeated_lists_are_discrete() {
    // Two lists of coprime lengths would repeat to their product.
    let a: Vec<&str> = std::iter::repeat_n("1px", 997).collect();
    let b: Vec<&str> = std::iter::repeat_n("2px", 991).collect();
    let from = style(&format!("background-position-x: {}", a.join(", ")));
    let to = style(&format!("background-position-x: {}", b.join(", ")));
    let mut out = style("");
    interpolate_longhand(LonghandId::BackgroundPositionX, &from, &to, 0.25, &mut out);
    assert_eq!(out.background_position_x.len(), 997);
    // Lengths whose least common multiple is small repeat.
    let from = style("background-position-x: 0px, 0px");
    let to = style("background-position-x: 10px, 20px, 30px");
    interpolate_longhand(LonghandId::BackgroundPositionX, &from, &to, 0.5, &mut out);
    assert_eq!(out.background_position_x.len(), 6);
}

#[test]
fn the_api_of_feature_3b() {
    // A keyframe set from the stylist, its interval and the value: the
    // `hero-reveal` opacity at 850 ms (progress 0.5 of 1.1 s after a
    // 300 ms delay, keyframe easing `cubic-bezier(.2,.8,.2,1)`).
    let mut stylist = crate::Stylist::new(swb_dom::QuirksMode::NoQuirks);
    let url = url::Url::parse("https://example.com/").expect("valid URL");
    stylist.add_author_sheet(
        &swb_css::parse_stylesheet("@keyframes v{0%{opacity:0}to{opacity:1}}"),
        &url,
    );
    let set = stylist
        .keyframes_in("v", &swb_css::MediaEnvironment::default())
        .expect("a set");
    let timing = crate::animation::Timing {
        delay: 300.0,
        duration: 1100.0,
        fill: crate::animation::FillMode::Both,
        ..crate::animation::Timing::default()
    };
    let t = timing.compute(Some(850.0));
    let easing = crate::animation::Easing::CubicBezier([0.2, 0.8, 0.2, 1.0]);
    let i = set
        .interval(0, t.progress.expect("active"), &easing, t.before_flag)
        .expect("opacity");
    let (from, to) = (style("opacity: 0"), style("opacity: 1"));
    let mut out = style("");
    let keyframes = set.properties.first().expect("one property");
    assert!(interpolate(keyframes, &from, &to, i.progress, &mut out));
    assert!((f64::from(out.opacity) - 0.946_079).abs() < 1e-5);
}
