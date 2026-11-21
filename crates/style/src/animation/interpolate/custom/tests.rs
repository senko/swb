//! Tests of the interpolation of registered custom properties against
//! Chromium 148: the rows are the values at 250 and 500 ms of linear 1 s
//! animations (so the progress is 0.25 and 0.5) in the cases `registered-*`
//! of `tools/probes/css-animations-3a.json`
//! (`crates/automation/tests/data/css-animations-3a.json`) and in
//! `interp-custom-properties` of `tools/probes/css-animations.json`.

use std::sync::Arc;

use swb_css::ComponentValue;

use super::super::interpolate_custom;
use crate::values::{LengthContext, Rgba};

/// A custom property map with the registrations of the `@property` rules
/// in `css`.
fn registered_map(css: &str) -> crate::CustomProperties {
    use crate::registered::{Registration, Registry};
    let sheet = swb_css::parse_stylesheet(css);
    let registrations: Vec<Registration> = sheet
        .rules
        .iter()
        .filter_map(|r| match r {
            swb_css::CssRule::Property(rule) => Registration::from_rule(rule),
            _ => None,
        })
        .collect();
    let registry = Arc::new(Registry::new(registrations.iter()));
    crate::CustomProperties::new(Some(&registry))
}

/// The text of custom property `name` from `a` to `b` at `p` for an element
/// with the color `current`, with the values computed by the registration
/// first (as the cascade does).
fn custom_at_color(
    map: &crate::CustomProperties,
    name: &str,
    (a, b): (&str, &str),
    current: Rgba,
    p: f64,
) -> String {
    let computed = |v: &str| -> Arc<[ComponentValue]> {
        let tokens = swb_css::parse_component_values(v);
        match map.registration(name) {
            Some(r) => r
                .compute(&tokens, &LengthContext::DEFAULT)
                .unwrap_or_else(|| panic!("{v} matches {name}")),
            None => tokens.into(),
        }
    };
    let (from, to) = (computed(a), computed(b));
    let value = interpolate_custom(map, name, Some(&from), Some(&to), current, p).expect("a value");
    swb_css::serialize_component_values(&value)
}

/// The text of each row `(name, from, to, progress, want)` for black text.
fn check(map: &crate::CustomProperties, rows: &[(&str, &str, &str, f64, &str)]) {
    for &(name, a, b, p, want) in rows {
        assert_eq!(
            custom_at_color(map, name, (a, b), Rgba::BLACK, p),
            want,
            "{name}: {a} to {b} at {p}"
        );
    }
}

/// The registrations of the rows below.
fn map() -> crate::CustomProperties {
    registered_map(
        "@property --len{syntax:'<length>';inherits:true;initial-value:0px}\
         @property --col{syntax:'<color>';inherits:false;initial-value:black}\
         @property --list{syntax:'<length>+';inherits:false;initial-value:0px}\
         @property --tl{syntax:'<transform-list>';inherits:false;initial-value:none}\
         @property --num{syntax:'<number>';inherits:false;initial-value:0}\
         @property --any{syntax:'*';inherits:false}\
         @property --ang{syntax:'<angle>';inherits:false;initial-value:0deg}\
         @property --pct{syntax:'<percentage>';inherits:false;initial-value:0%}\
         @property --lp{syntax:'<length-percentage>';inherits:false;initial-value:0px}\
         @property --int{syntax:'<integer>';inherits:false;initial-value:0}\
         @property --time{syntax:'<time>';inherits:false;initial-value:0s}\
         @property --res{syntax:'<resolution>';inherits:false;initial-value:1dppx}\
         @property --cl{syntax:'<color>#';inherits:false;initial-value:red}\
         @property --alt{syntax:'<length> | <color>';inherits:false;initial-value:0px}\
         @property --kw{syntax:'a | b';inherits:false;initial-value:a}\
         @property --lc{syntax:'<length>#';inherits:false;initial-value:0px}\
         @property --pctl{syntax:'<percentage>+';inherits:false;initial-value:0%}\
         @property --mixed{syntax:'<length>|<percentage>';inherits:false;initial-value:0px}\
         @property --sp{syntax:'<number>+';inherits:false;initial-value:0}",
    )
}

/// `interp-custom-properties` and the case `registered-custom`.
#[test]
#[allow(clippy::too_many_lines)] // One row per case.
fn registered_custom_properties() {
    let rows: &[(&str, &str, &str, f64, &str)] = &[
        // `interp-custom-properties`.
        ("--len", "0px", "100px", 0.5, "50px"),
        (
            "--col",
            "rgb(255,0,0)",
            "rgb(0,0,255)",
            0.25,
            "rgb(191, 0, 64)",
        ),
        (
            "--col",
            "rgb(255,0,0)",
            "rgb(0,0,255)",
            0.5,
            "rgb(128, 0, 128)",
        ),
        ("--list", "1px 2px", "11px 22px", 0.25, "3.5px 7px"),
        ("--list", "1px 2px", "11px 22px", 0.5, "6px 12px"),
        ("--list", "1px", "11px 22px", 0.25, "1px"),
        ("--list", "1px", "11px 22px", 0.5, "11px 22px"),
        (
            "--tl",
            "rotate(0deg)",
            "rotate(90deg)",
            0.5,
            "rotate(45deg)",
        ),
        ("--num", "0", "10", 0.25, "2.5"),
        ("--any", "10px", "20px", 0.4, "10px"),
        ("--any", "10px", "20px", 0.6, "20px"),
        ("--ang", "0deg", "1turn", 0.5, "180deg"),
        ("--u", "a", "b", 0.499, "a"),
        ("--u", "a", "b", 0.5, "b"),
        // `registered-custom`: `pct`, `lp`, `int`, `int2`, `time`, `res`.
        ("--pct", "10%", "20%", 0.25, "12.5%"),
        ("--pct", "10%", "20%", 0.5, "15%"),
        ("--lp", "10%", "20px", 0.25, "calc(7.5% + 5px)"),
        ("--lp", "10%", "20px", 0.5, "calc(5% + 10px)"),
        ("--int", "0", "5", 0.25, "1"),
        ("--int", "0", "5", 0.5, "3"),
        ("--int", "0", "-5", 0.25, "-1"),
        // Half away from zero (`int2` at 500 ms).
        ("--int", "0", "-5", 0.5, "-3"),
        ("--time", "1s", "500ms", 0.25, "0.875s"),
        ("--time", "1s", "500ms", 0.5, "0.75s"),
        ("--res", "1dppx", "192dpi", 0.25, "1.25dppx"),
        ("--res", "1dppx", "192dpi", 0.5, "1.5dppx"),
        // `cl` and `cl2`: `<color>#` item by item; lists of different
        // lengths are discrete.
        (
            "--cl",
            "red, blue",
            "blue, red",
            0.25,
            "rgb(191, 0, 64), rgb(64, 0, 191)",
        ),
        (
            "--cl",
            "red, blue",
            "blue, red",
            0.5,
            "rgb(128, 0, 128), rgb(128, 0, 128)",
        ),
        ("--cl", "red", "blue, red", 0.25, "rgb(255, 0, 0)"),
        (
            "--cl",
            "red",
            "blue, red",
            0.5,
            "rgb(0, 0, 255), rgb(255, 0, 0)",
        ),
        // `alt`, `alt2`, `kw`: `<length> | <color>` and keywords.
        ("--alt", "0px", "red", 0.25, "0px"),
        ("--alt", "0px", "red", 0.5, "rgb(255, 0, 0)"),
        ("--alt", "0px", "10px", 0.25, "2.5px"),
        ("--alt", "0px", "10px", 0.5, "5px"),
        ("--kw", "a", "b", 0.25, "a"),
        ("--kw", "a", "b", 0.5, "b"),
        // `cur` (black text): `currentcolor` to `red`.
        ("--col", "currentcolor", "red", 0.25, "rgb(64, 0, 0)"),
        ("--col", "currentcolor", "red", 0.5, "rgb(128, 0, 0)"),
        // `tl2` to `tl4`.
        ("--tl", "none", "translateX(10px) scale(2)", 0.25, "none"),
        (
            "--tl",
            "none",
            "translateX(10px) scale(2)",
            0.5,
            "translateX(10px) scale(2)",
        ),
        (
            "--tl",
            "translate(-50%)",
            "rotate(45deg)",
            0.25,
            "translate(-50%)",
        ),
        (
            "--tl",
            "translate(-50%)",
            "rotate(45deg)",
            0.5,
            "rotate(45deg)",
        ),
        (
            "--tl",
            "rotate(45deg)",
            "scale(2)",
            0.25,
            "matrix3d(1.03934, 0.694463, 0, 0, -0.694463, 1.03934, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",
        ),
        (
            "--tl",
            "rotate(45deg)",
            "scale(2)",
            0.5,
            "matrix3d(1.38582, 0.574025, 0, 0, -0.574025, 1.38582, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",
        ),
        // `lp2` (`1em` is 10 px there), `num`, `ang`.
        ("--len", "10px", "2in", 0.25, "55.5px"),
        ("--len", "10px", "2in", 0.5, "101px"),
        ("--num", "1.5", "2.5e1", 0.25, "7.375"),
        ("--num", "1.5", "2.5e1", 0.5, "13.25"),
        ("--ang", "10deg", "1rad", 0.25, "21.8239deg"),
        ("--ang", "10deg", "1rad", 0.5, "33.6479deg"),
        // `lps`, `lc`, `pctl`, `mixed`, `sp`.
        (
            "--list",
            "1px 2px 3px",
            "4px 5px 6px",
            0.25,
            "1.75px 2.75px 3.75px",
        ),
        (
            "--list",
            "1px 2px 3px",
            "4px 5px 6px",
            0.5,
            "2.5px 3.5px 4.5px",
        ),
        ("--lc", "1px, 2px", "4px, 5px", 0.25, "1.75px, 2.75px"),
        ("--lc", "1px, 2px", "4px, 5px", 0.5, "2.5px, 3.5px"),
        ("--pctl", "1% 2%", "3% 4%", 0.25, "1.5% 2.5%"),
        ("--pctl", "1% 2%", "3% 4%", 0.5, "2% 3%"),
        ("--mixed", "10px", "20%", 0.25, "10px"),
        ("--mixed", "10px", "20%", 0.5, "20%"),
        ("--sp", "1 2", "3 4", 0.25, "1.5 2.5"),
        ("--sp", "1 2", "3 4", 0.5, "2 3"),
    ];
    check(&map(), rows);
    // A missing side is the guaranteed-invalid value, discrete.
    let map = map();
    let a: Arc<[ComponentValue]> = swb_css::parse_component_values("1px").into();
    let black = Rgba::BLACK;
    assert_eq!(
        interpolate_custom(&map, "--len", Some(&a), None, black, 0.7),
        None
    );
    assert_eq!(
        interpolate_custom(&map, "--len", Some(&a), None, black, 0.2).as_deref(),
        Some(&a[..])
    );
}

/// The cases `registered-tl-1` to `registered-tl-4` (pairs of `--tl`): the
/// names that the custom property keeps, the families that pair, `none`,
/// the padding and the `matrix3d()` rest.
#[test]
#[allow(clippy::too_many_lines)] // One row per case.
fn registered_transform_lists() {
    let rows: &[(&str, &str, f64, &str)] = &[
        // `registered-tl-1` (Chromium 148).
        ("none", "rotate(90deg)", 0.25, "none"),
        ("none", "rotate(90deg)", 0.5, "rotate(90deg)"),
        (
            "translateX(0px)",
            "translateX(10px) scale(2)",
            0.25,
            "translateX(2.5px) scale(1.25)",
        ),
        (
            "translateX(0px)",
            "translateX(10px) scale(2)",
            0.5,
            "translateX(5px) scale(1.5)",
        ),
        (
            "translateX(10px)",
            "translateY(10px)",
            0.25,
            "translate(7.5px, 2.5px)",
        ),
        (
            "translateX(10px)",
            "translateY(10px)",
            0.5,
            "translate(5px, 5px)",
        ),
        (
            "rotate(10deg) scale(2)",
            "rotate(30deg) scale(4)",
            0.25,
            "rotate(15deg) scale(2.5)",
        ),
        (
            "rotate(10deg) scale(2)",
            "rotate(30deg) scale(4)",
            0.5,
            "rotate(20deg) scale(3)",
        ),
        (
            "translateX(10px) rotate(0deg)",
            "translateX(30px)",
            0.25,
            "translateX(15px) rotate(0deg)",
        ),
        (
            "translateX(10px) rotate(0deg)",
            "translateX(30px)",
            0.5,
            "translateX(20px) rotate(0deg)",
        ),
        ("rotate(0deg)", "none", 0.25, "rotate(0deg)"),
        ("rotate(0deg)", "none", 0.5, "none"),
        ("scale(2)", "none", 0.25, "scale(2)"),
        ("scale(2)", "none", 0.5, "none"),
        // `registered-tl-2` (Chromium 148).
        (
            "translate(10px, 20px)",
            "translateX(30px)",
            0.25,
            "translate(15px, 15px)",
        ),
        (
            "translate(10px, 20px)",
            "translateX(30px)",
            0.5,
            "translate(20px, 10px)",
        ),
        (
            "translate(10px)",
            "translate(30px, 40px)",
            0.25,
            "translate(15px, 10px)",
        ),
        (
            "translate(10px)",
            "translate(30px, 40px)",
            0.5,
            "translate(20px, 20px)",
        ),
        ("scale(2)", "scale(4,6)", 0.25, "scale(2.5, 3)"),
        ("scale(2)", "scale(4,6)", 0.5, "scale(3, 4)"),
        ("rotate(0deg)", "rotate(720deg)", 0.25, "rotate(180deg)"),
        ("rotate(0deg)", "rotate(720deg)", 0.5, "rotate(360deg)"),
        ("skewX(0deg)", "skewX(30deg)", 0.25, "skewX(7.5deg)"),
        ("skewX(0deg)", "skewX(30deg)", 0.5, "skewX(15deg)"),
        (
            "skew(10deg,5deg)",
            "skew(30deg)",
            0.25,
            "skew(15deg, 3.75deg)",
        ),
        (
            "skew(10deg,5deg)",
            "skew(30deg)",
            0.5,
            "skew(20deg, 2.5deg)",
        ),
        (
            "matrix(1,0,0,1,0,0)",
            "matrix(2,0,0,2,10,10)",
            0.25,
            "matrix(1.25, 0, 0, 1.25, 2.5, 2.5)",
        ),
        (
            "matrix(1,0,0,1,0,0)",
            "matrix(2,0,0,2,10,10)",
            0.5,
            "matrix(1.5, 0, 0, 1.5, 5, 5)",
        ),
        (
            "matrix(1,0,0,1,0,0)",
            "matrix(1,0,0,1,10,10)",
            0.25,
            "matrix(1, 0, 0, 1, 2.5, 2.5)",
        ),
        (
            "matrix(1,0,0,1,0,0)",
            "matrix(1,0,0,1,10,10)",
            0.5,
            "matrix(1, 0, 0, 1, 5, 5)",
        ),
        (
            "matrix(1,0,0,1,0,0)",
            "rotate(90deg)",
            0.25,
            "matrix3d(0.92388, 0.382683, 0, 0, -0.382683, 0.92388, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",
        ),
        (
            "matrix(1,0,0,1,0,0)",
            "rotate(90deg)",
            0.5,
            "matrix3d(0.707107, 0.707107, 0, 0, -0.707107, 0.707107, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",
        ),
        ("rotate(10deg)", "rotate(10deg)", 0.25, "rotate(10deg)"),
        ("rotate(10deg)", "rotate(10deg)", 0.5, "rotate(10deg)"),
        ("rotate(0.5turn)", "rotate(1turn)", 0.25, "rotate(225deg)"),
        ("rotate(0.5turn)", "rotate(1turn)", 0.5, "rotate(270deg)"),
        (
            "translateX(0px) translateY(0px)",
            "translateX(10px) translateY(20px)",
            0.25,
            "translateX(2.5px) translateY(5px)",
        ),
        (
            "translateX(0px) translateY(0px)",
            "translateX(10px) translateY(20px)",
            0.5,
            "translateX(5px) translateY(10px)",
        ),
        (
            "translate(0px,0px)",
            "translateX(10px)",
            0.25,
            "translate(2.5px)",
        ),
        (
            "translate(0px,0px)",
            "translateX(10px)",
            0.5,
            "translate(5px)",
        ),
        ("rotate(45deg)", "rotate(45deg)", 0.25, "rotate(45deg)"),
        ("rotate(45deg)", "rotate(45deg)", 0.5, "rotate(45deg)"),
        (
            "translateX(10px) rotate(0deg)",
            "translateX(10px) rotate(90deg)",
            0.25,
            "translateX(10px) rotate(22.5deg)",
        ),
        (
            "translateX(10px) rotate(0deg)",
            "translateX(10px) rotate(90deg)",
            0.5,
            "translateX(10px) rotate(45deg)",
        ),
        (
            "scale(1) rotate(0deg)",
            "rotate(90deg) scale(3)",
            0.25,
            "matrix3d(1.38582, 0.574025, 0, 0, -0.574025, 1.38582, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",
        ),
        (
            "scale(1) rotate(0deg)",
            "rotate(90deg) scale(3)",
            0.5,
            "matrix3d(1.41421, 1.41421, 0, 0, -1.41421, 1.41421, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",
        ),
        (
            "translateY(10px)",
            "translateX(10px) translateY(20px)",
            0.25,
            "translate(2.5px, 7.5px) translateY(5px)",
        ),
        (
            "translateY(10px)",
            "translateX(10px) translateY(20px)",
            0.5,
            "translate(5px, 5px) translateY(10px)",
        ),
        ("scaleX(1)", "scaleX(3)", 0.25, "scaleX(1.5)"),
        ("scaleX(1)", "scaleX(3)", 0.5, "scaleX(2)"),
        ("scaleX(1)", "scale(3)", 0.25, "scale(1.5)"),
        ("scaleX(1)", "scale(3)", 0.5, "scale(2)"),
        ("rotate(0deg)", "none", 0.25, "rotate(0deg)"),
        ("rotate(0deg)", "none", 0.5, "none"),
        ("none", "none", 0.25, "none"),
        ("none", "none", 0.5, "none"),
        ("scale(0)", "scale(1)", 0.25, "scale(0.25)"),
        ("scale(0)", "scale(1)", 0.5, "scale(0.5)"),
        (
            "rotate(1deg)",
            "rotate(90deg) translateX(10px)",
            0.25,
            "rotate(23.25deg) translateX(2.5px)",
        ),
        (
            "rotate(1deg)",
            "rotate(90deg) translateX(10px)",
            0.5,
            "rotate(45.5deg) translateX(5px)",
        ),
        (
            "matrix(1,0,0,1,0,0)",
            "matrix(1.333333333,0.1,0.1,1.333333333,0.333333,0.7777777)",
            0.25,
            "matrix(1.08408, 0.0202909, 0.0205211, 1.0811, 0.0833332, 0.194444)",
        ),
        (
            "matrix(1,0,0,1,0,0)",
            "matrix(1.333333333,0.1,0.1,1.333333333,0.333333,0.7777777)",
            0.5,
            "matrix(1.16772, 0.0437281, 0.0440625, 1.16352, 0.166666, 0.388889)",
        ),
        (
            "translateX(1px)",
            "matrix(1,0,0,1,0,0)",
            0.25,
            "matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0.75, 0, 0, 1)",
        ),
        (
            "translateX(1px)",
            "matrix(1,0,0,1,0,0)",
            0.5,
            "matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0.5, 0, 0, 1)",
        ),
        // `registered-tl-3` (Chromium 148).
        (
            "translateY(0px)",
            "translateY(10px)",
            0.25,
            "translateY(2.5px)",
        ),
        (
            "translateY(0px)",
            "translateY(10px)",
            0.5,
            "translateY(5px)",
        ),
        ("scaleY(1)", "scaleY(3)", 0.25, "scaleY(1.5)"),
        ("scaleY(1)", "scaleY(3)", 0.5, "scaleY(2)"),
        ("skewY(0deg)", "skewY(30deg)", 0.25, "skewY(7.5deg)"),
        ("skewY(0deg)", "skewY(30deg)", 0.5, "skewY(15deg)"),
        (
            "skewX(0deg)",
            "skewY(30deg)",
            0.25,
            "matrix3d(1.02979, 0.135574, 0, 0, 0.0121553, 0.976447, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",
        ),
        (
            "skewX(0deg)",
            "skewY(30deg)",
            0.5,
            "matrix3d(1.04064, 0.278839, 0, 0, 0.0186787, 0.970931, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",
        ),
        (
            "skew(10deg)",
            "skewX(30deg)",
            0.25,
            "matrix3d(1, 0, 0, 0, 0.276583, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",
        ),
        (
            "skew(10deg)",
            "skewX(30deg)",
            0.5,
            "matrix3d(1, 0, 0, 0, 0.376839, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",
        ),
        (
            "skewX(10deg)",
            "skew(30deg, 10deg)",
            0.25,
            "matrix3d(1.0029, 0.0437876, 0, 0, 0.289472, 0.984702, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",
        ),
        (
            "skewX(10deg)",
            "skew(30deg, 10deg)",
            0.5,
            "matrix3d(1.00388, 0.087828, 0, 0, 0.394461, 0.980386, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",
        ),
        (
            "translate(10px,0px)",
            "translate(30px,0px)",
            0.25,
            "translate(15px)",
        ),
        (
            "translate(10px,0px)",
            "translate(30px,0px)",
            0.5,
            "translate(20px)",
        ),
        (
            "translate(0px,10px)",
            "translate(0px,30px)",
            0.25,
            "translate(0px, 15px)",
        ),
        (
            "translate(0px,10px)",
            "translate(0px,30px)",
            0.5,
            "translate(0px, 20px)",
        ),
        ("scale(1,1)", "scale(3,3)", 0.25, "scale(1.5)"),
        ("scale(1,1)", "scale(3,3)", 0.5, "scale(2)"),
        ("scale(1,2)", "scale(3,2)", 0.25, "scale(1.5, 2)"),
        ("scale(1,2)", "scale(3,2)", 0.5, "scale(2)"),
        (
            "matrix(1,0,0,1,0,0) translateX(0px)",
            "matrix(2,0,0,2,10,10) translateX(10px)",
            0.25,
            "matrix(1.25, 0, 0, 1.25, 2.5, 2.5) translateX(2.5px)",
        ),
        (
            "matrix(1,0,0,1,0,0) translateX(0px)",
            "matrix(2,0,0,2,10,10) translateX(10px)",
            0.5,
            "matrix(1.5, 0, 0, 1.5, 5, 5) translateX(5px)",
        ),
        (
            "translateX(10px) matrix(1,0,0,1,0,0)",
            "translateX(30px) rotate(90deg)",
            0.25,
            "translateX(15px) matrix3d(0.92388, 0.382683, 0, 0, -0.382683, 0.92388, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",
        ),
        (
            "translateX(10px) matrix(1,0,0,1,0,0)",
            "translateX(30px) rotate(90deg)",
            0.5,
            "translateX(20px) matrix3d(0.707107, 0.707107, 0, 0, -0.707107, 0.707107, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",
        ),
        (
            "translateX(0px) rotate(0deg)",
            "translateX(10px) scale(2)",
            0.25,
            "translateX(2.5px) matrix3d(1.25, 0, 0, 0, 0, 1.25, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",
        ),
        (
            "translateX(0px) rotate(0deg)",
            "translateX(10px) scale(2)",
            0.5,
            "translateX(5px) matrix3d(1.5, 0, 0, 0, 0, 1.5, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",
        ),
        (
            "translateX(0px)",
            "translateX(10%)",
            0.25,
            "translateX(2.5%)",
        ),
        ("translateX(0px)", "translateX(10%)", 0.5, "translateX(5%)"),
        (
            "translate(0px)",
            "translate(10%, 20px)",
            0.25,
            "translate(2.5%, 5px)",
        ),
        (
            "translate(0px)",
            "translate(10%, 20px)",
            0.5,
            "translate(5%, 10px)",
        ),
        (
            "translateX(0px)",
            "translate(10px, 20px)",
            0.25,
            "translate(2.5px, 5px)",
        ),
        (
            "translateX(0px)",
            "translate(10px, 20px)",
            0.5,
            "translate(5px, 10px)",
        ),
        ("rotate(10deg)", "rotate(350deg)", 0.25, "rotate(95deg)"),
        ("rotate(10deg)", "rotate(350deg)", 0.5, "rotate(180deg)"),
        (
            "scaleX(2) scaleY(3)",
            "scaleX(4) scaleY(5)",
            0.25,
            "scaleX(2.5) scaleY(3.5)",
        ),
        (
            "scaleX(2) scaleY(3)",
            "scaleX(4) scaleY(5)",
            0.5,
            "scaleX(3) scaleY(4)",
        ),
        (
            "rotate(10deg) translateX(1px)",
            "rotate(10deg)",
            0.25,
            "rotate(10deg) translateX(0.75px)",
        ),
        (
            "rotate(10deg) translateX(1px)",
            "rotate(10deg)",
            0.5,
            "rotate(10deg) translateX(0.5px)",
        ),
        (
            "matrix(1,0,0,1,0,0)",
            "matrix(-1,0,0,1,0,0)",
            0.25,
            "matrix(0.5, 0, 0, 1, 0, 0)",
        ),
        (
            "matrix(1,0,0,1,0,0)",
            "matrix(-1,0,0,1,0,0)",
            0.5,
            "matrix(0, 0, 0, 1, 0, 0)",
        ),
        (
            "matrix(1,0,0,1,0,0)",
            "matrix(1,0,0,1,0,0)",
            0.25,
            "matrix(1, 0, 0, 1, 0, 0)",
        ),
        (
            "matrix(1,0,0,1,0,0)",
            "matrix(1,0,0,1,0,0)",
            0.5,
            "matrix(1, 0, 0, 1, 0, 0)",
        ),
        (
            "translateX(10px) translateY(10px)",
            "translate(30px, 30px)",
            0.25,
            "translate(15px, 7.5px) translateY(7.5px)",
        ),
        (
            "translateX(10px) translateY(10px)",
            "translate(30px, 30px)",
            0.5,
            "translate(20px, 15px) translateY(5px)",
        ),
        ("translate(10%)", "rotate(90deg)", 0.25, "translate(10%)"),
        ("translate(10%)", "rotate(90deg)", 0.5, "rotate(90deg)"),
        (
            "translateX(10px)",
            "translateX(10px) rotate(90deg)",
            0.25,
            "translateX(10px) rotate(22.5deg)",
        ),
        (
            "translateX(10px)",
            "translateX(10px) rotate(90deg)",
            0.5,
            "translateX(10px) rotate(45deg)",
        ),
        (
            "rotate(90deg) translateX(10px)",
            "translateX(10px)",
            0.25,
            "matrix3d(0.382683, 0.92388, 0, 0, -0.92388, 0.382683, 0, 0, 0, 0, 1, 0, 2.5, 7.5, 0, 1)",
        ),
        (
            "rotate(90deg) translateX(10px)",
            "translateX(10px)",
            0.5,
            "matrix3d(0.707107, 0.707107, 0, 0, -0.707107, 0.707107, 0, 0, 0, 0, 1, 0, 5, 5, 0, 1)",
        ),
        (
            "matrix(1,0,0,1,0,0)",
            "rotate(180deg)",
            0.25,
            "matrix3d(0.707107, 0.707107, 0, 0, -0.707107, 0.707107, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",
        ),
        // Not here: this pair at 500 ms. Chromium's cos(90deg) noise is
        // 2.22045e-16, swb's 6.12323e-17 (and the token text of a number
        // has no exponent).
        (
            "scale(2) translateX(10px)",
            "scale(4) translateX(20px)",
            0.25,
            "scale(2.5) translateX(12.5px)",
        ),
        (
            "scale(2) translateX(10px)",
            "scale(4) translateX(20px)",
            0.5,
            "scale(3) translateX(15px)",
        ),
        (
            "skewX(10deg) rotate(0deg)",
            "skewY(30deg) rotate(90deg)",
            0.25,
            "matrix3d(0.92388, 0.382683, 0, 0, -0.393855, 0.919252, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",
        ),
        (
            "skewX(10deg) rotate(0deg)",
            "skewY(30deg) rotate(90deg)",
            0.5,
            "matrix3d(0.707107, 0.707107, 0, 0, -0.84889, 0.565324, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",
        ),
        // `registered-tl-4` (Chromium 148).
        ("scaleX(1)", "scaleY(3)", 0.25, "scale(1, 1.5)"),
        ("scaleX(1)", "scaleY(3)", 0.5, "scale(1, 2)"),
        ("scaleY(1)", "scale(3, 5)", 0.25, "scale(1.5, 2)"),
        ("scaleY(1)", "scale(3, 5)", 0.5, "scale(2, 3)"),
        (
            "skew(10deg,0deg)",
            "skew(30deg,0deg)",
            0.25,
            "skew(15deg, 0deg)",
        ),
        (
            "skew(10deg,0deg)",
            "skew(30deg,0deg)",
            0.5,
            "skew(20deg, 0deg)",
        ),
        (
            "skew(10deg,5deg)",
            "skew(10deg,5deg)",
            0.25,
            "skew(10deg, 5deg)",
        ),
        (
            "skew(10deg,5deg)",
            "skew(10deg,5deg)",
            0.5,
            "skew(10deg, 5deg)",
        ),
        (
            "skewX(10deg) skewY(5deg)",
            "skewX(20deg) skewY(15deg)",
            0.25,
            "skewX(12.5deg) skewY(7.5deg)",
        ),
        (
            "skewX(10deg) skewY(5deg)",
            "skewX(20deg) skewY(15deg)",
            0.5,
            "skewX(15deg) skewY(10deg)",
        ),
        (
            "translateX(10%) rotate(0deg)",
            "translateX(20%) scale(2)",
            0.25,
            "translateX(12.5%) matrix3d(1.25, 0, 0, 0, 0, 1.25, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",
        ),
        (
            "translateX(10%) rotate(0deg)",
            "translateX(20%) scale(2)",
            0.5,
            "translateX(15%) matrix3d(1.5, 0, 0, 0, 0, 1.5, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",
        ),
        (
            "rotate(0deg) translateX(10%)",
            "scale(2) translateX(20%)",
            0.25,
            "rotate(0deg) translateX(10%)",
        ),
        (
            "rotate(0deg) translateX(10%)",
            "scale(2) translateX(20%)",
            0.5,
            "scale(2) translateX(20%)",
        ),
        (
            "translateX(10px)",
            "translateX(10px)",
            0.25,
            "translateX(10px)",
        ),
        (
            "translateX(10px)",
            "translateX(10px)",
            0.5,
            "translateX(10px)",
        ),
        (
            "translateY(10px) translateX(10px)",
            "translateX(30px) translateY(30px)",
            0.25,
            "translate(7.5px, 7.5px) translate(7.5px, 7.5px)",
        ),
        (
            "translateY(10px) translateX(10px)",
            "translateX(30px) translateY(30px)",
            0.5,
            "translate(15px, 5px) translate(5px, 15px)",
        ),
        (
            "rotate(0deg) translateX(10px)",
            "rotate(90deg) translate(30px, 20px)",
            0.25,
            "rotate(22.5deg) translate(15px, 5px)",
        ),
        (
            "rotate(0deg) translateX(10px)",
            "rotate(90deg) translate(30px, 20px)",
            0.5,
            "rotate(45deg) translate(20px, 10px)",
        ),
        (
            "rotate(0deg)",
            "rotate(90deg) rotate(90deg)",
            0.25,
            "rotate(22.5deg) rotate(22.5deg)",
        ),
        (
            "rotate(0deg)",
            "rotate(90deg) rotate(90deg)",
            0.5,
            "rotate(45deg) rotate(45deg)",
        ),
        (
            "scale(1, 1) rotate(0deg)",
            "scale(2) skewX(10deg)",
            0.25,
            "scale(1.25) matrix3d(1, 0, 0, 0, 0.0440817, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",
        ),
        (
            "scale(1, 1) rotate(0deg)",
            "scale(2) skewX(10deg)",
            0.5,
            "scale(1.5) matrix3d(1, 0, 0, 0, 0.0881635, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",
        ),
    ];
    let rows: Vec<(&str, &str, &str, f64, &str)> = rows
        .iter()
        .map(|&(a, b, p, want)| ("--tl", a, b, p, want))
        .collect();
    check(&map(), &rows);
}

/// The case `registered-currentcolor`: an element with the color
/// `rgb(0, 0, 255)`; `currentcolor` interpolates with it, a `color-mix()`
/// is discrete.
#[test]
#[allow(clippy::too_many_lines)] // One row per case.
fn registered_currentcolor() {
    let map = registered_map(
        "@property --c{syntax:'<color>';inherits:false;initial-value:black}\
         @property --l{syntax:'<color>#';inherits:false;initial-value:black}\
         @property --a{syntax:'<color> | <length>';inherits:false;initial-value:black}",
    );
    let blue = Rgba::from_f32(0.0, 0.0, 255.0, 1.0);
    let rows: &[(&str, (&str, &str), f64, &str)] = &[
        ("--c", ("currentcolor", "red"), 0.25, "rgb(64, 0, 191)"),
        ("--c", ("currentcolor", "red"), 0.5, "rgb(128, 0, 128)"),
        ("--c", ("red", "currentcolor"), 0.25, "rgb(191, 0, 64)"),
        ("--c", ("red", "currentcolor"), 0.5, "rgb(128, 0, 128)"),
        (
            "--l",
            ("currentcolor, red", "blue, currentcolor"),
            0.25,
            "rgb(0, 0, 255), rgb(191, 0, 64)",
        ),
        (
            "--l",
            ("currentcolor, red", "blue, currentcolor"),
            0.5,
            "rgb(0, 0, 255), rgb(128, 0, 128)",
        ),
        (
            "--c",
            ("currentcolor", "currentcolor"),
            0.25,
            "rgb(0, 0, 255)",
        ),
        (
            "--c",
            ("currentcolor", "currentcolor"),
            0.5,
            "rgb(0, 0, 255)",
        ),
        ("--a", ("currentcolor", "red"), 0.25, "rgb(64, 0, 191)"),
        ("--a", ("currentcolor", "red"), 0.5, "rgb(128, 0, 128)"),
        (
            "--c",
            ("color-mix(in srgb, currentcolor, red)", "blue"),
            0.25,
            "color-mix(in srgb, currentcolor, rgb(255, 0, 0))",
        ),
        (
            "--c",
            ("color-mix(in srgb, currentcolor, red)", "blue"),
            0.5,
            "rgb(0, 0, 255)",
        ),
        (
            "--c",
            ("CurrentColor", "rgb(0 255 0 / 0.5)"),
            0.25,
            "rgba(0, 37, 218, 0.875)",
        ),
        (
            "--c",
            ("CurrentColor", "rgb(0 255 0 / 0.5)"),
            0.5,
            "rgba(0, 85, 170, 0.753)",
        ),
        (
            "--c",
            ("transparent", "currentcolor"),
            0.25,
            "rgba(0, 0, 255, 0.25)",
        ),
        (
            "--c",
            ("transparent", "currentcolor"),
            0.5,
            "rgba(0, 0, 255, 0.5)",
        ),
    ];
    for &(name, pair, p, want) in rows {
        assert_eq!(
            custom_at_color(&map, name, pair, blue, p),
            want,
            "{name}: {pair:?} at {p}"
        );
    }
}
