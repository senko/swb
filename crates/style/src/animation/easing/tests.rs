//! Tests of the easing functions against Chromium 148: the rows of
//! `crates/automation/tests/data/css-animations.json` (cases `easings`,
//! `easing-precision-duration`, `easing-progress-precision`) and of
//! `tools/probes/css-animations-3a.json` (`longhand-values`,
//! `longhand-computed`).

// The numbers are Chromium's values as it prints them.
#![allow(clippy::unreadable_literal)]

use super::*;
use crate::parse::test_util::parse_all;

fn easing(css: &str) -> Easing {
    parse_all(css, parse_easing).unwrap_or_else(|_| panic!("{css} parses"))
}

/// Numbers as the probe rows compare them (ADR 0044 section 12).
fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-4 || (a - b).abs() <= 1e-5 * b.abs()
}

/// The easings of the case `easings`, in the order of [`EASINGS_LEFT`].
const EASINGS: [&str; 20] = [
    "ease",
    "ease-in",
    "ease-out",
    "ease-in-out",
    "linear",
    "cubic-bezier(.2, .8, .2, 1)",
    "cubic-bezier(.45,.15,.6,.9)",
    "cubic-bezier(.4, 0, 1, 1)",
    "cubic-bezier(.4, 0, .2, 1)",
    "cubic-bezier(0, 0, .2, 1)",
    "cubic-bezier(.25,.8,.4,1)",
    "cubic-bezier(.3, 1.6, .6, 1)",
    "steps(8, end)",
    "steps(4, start)",
    "steps(3, jump-both)",
    "steps(3, jump-none)",
    "linear(0, 0.25 25%, 1)",
    "linear(0, 0.5 30% 60%, 1)",
    "linear(0, -24 20%, 16 40%, -8 60%, 4 80%, 1)",
    "linear(0, 1, 0, 1)",
];

/// The `left` of the case `easings` (query 1): the time in ms and the
/// value in px of each easing; `left` goes from 0 to 100px in 1 s.
#[rustfmt::skip]
const EASINGS_LEFT: &[(f64, [f64; 20])] = &[
    (0.0, [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 25.0, 25.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
    (50.0, [3.29515, 0.447462, 8.22422, 0.482975, 5.0, 20.2241, 1.96168, 0.487612, 0.577021, 17.4678, 15.5648, 24.6699, 0.0, 25.0, 25.0, 0.0, 5.0, 8.33333, -600.0, 15.0]),
    (100.0, [9.47963, 1.70266, 16.0572, 1.97225, 10.0, 39.6694, 4.54084, 1.83729, 2.5863, 30.3848, 30.0087, 45.5171, 0.0, 25.0, 25.0, 0.0, 10.0, 16.6667, -1200.0, 30.0]),
    (200.0, [29.5244, 6.2282, 30.8366, 8.16599, 20.0, 68.1792, 11.7024, 6.61514, 13.3947, 50.0, 54.2675, 76.8724, 12.5, 25.0, 25.0, 0.0, 20.0, 33.3333, -2400.0, 60.0]),
    (250.0, [40.8511, 9.34647, 37.8138, 12.9162, 25.0, 76.7284, 16.3398, 9.86266, 23.6587, 57.7573, 63.7808, 87.9928, 25.0, 50.0, 25.0, 0.0, 25.0, 41.6667, -1400.0, 75.0]),
    (333.0, [57.5268, 15.5889, 48.7924, 23.1311, 33.3, 85.7875, 25.6016, 16.2969, 45.6377, 68.471, 75.9552, 100.845, 25.0, 50.0, 25.0, 0.0, 33.3, 50.0, 260.0, 99.9]),
    (500.0, [80.2403, 31.5357, 68.4643, 50.0, 50.0, 94.6079, 49.0257, 32.4815, 77.5561, 83.9245, 90.2843, 110.812, 50.0, 75.0, 50.0, 50.0, 50.0, 50.0, 400.0, 50.0]),
    (667.0, [92.4995, 51.2076, 84.4111, 76.8689, 66.7, 98.216, 73.5017, 52.14, 92.0965, 93.4209, 96.7573, 108.556, 62.5, 75.0, 75.0, 100.0, 66.7, 58.375, -398.0, 0.1]),
    (750.0, [96.0459, 62.1862, 90.6535, 87.0838, 75.0, 99.1108, 83.5572, 63.0085, 95.9368, 96.4216, 98.3885, 105.711, 75.0, 100.0, 75.0, 100.0, 75.0, 68.75, 100.0, 25.0]),
    (800.0, [97.5625, 69.1634, 93.7718, 91.834, 80.0, 99.4687, 88.5506, 69.8843, 97.5267, 97.7559, 99.0399, 103.966, 75.0, 100.0, 75.0, 100.0, 80.0, 75.0, 400.0, 40.0]),
    (900.0, [99.4316, 83.9428, 98.2973, 98.0278, 90.0, 99.883, 95.9446, 84.375, 99.4354, 99.4601, 99.7902, 101.131, 87.5, 100.0, 75.0, 100.0, 90.0, 87.5, 250.0, 70.0]),
    (950.0, [99.8625, 91.7758, 99.5525, 99.517, 95.0, 99.9724, 98.3698, 92.0148, 99.8646, 99.8675, 99.9508, 100.298, 87.5, 100.0, 75.0, 100.0, 95.0, 93.75, 175.0, 85.0]),
    (1000.0, [100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0]),
];

#[test]
fn easings_match_chromium() {
    for (time, values) in EASINGS_LEFT {
        for (css, want) in EASINGS.iter().zip(values) {
            let got = 100.0 * easing(css).evaluate(time / 1000.0, false);
            assert!(close(got, *want), "{css} at {time} ms: {got} != {want}");
        }
    }
}

#[test]
fn precision_does_not_depend_on_the_duration() {
    // `easing-precision-duration`: the opacity at fractions of the
    // duration (the same for 100 ms, 1 s and 100 s).
    let fractions = [0.01, 0.05, 0.3, 0.45, 0.5, 0.55, 0.9];
    let rows: [(&str, [f64; 7]); 3] = [
        (
            "cubic-bezier(.2,.8,.2,1)",
            [
                0.0401548, 0.202241, 0.827631, 0.927591, 0.946079, 0.960307, 0.99883,
            ],
        ),
        (
            "cubic-bezier(.9,0,.1,1)",
            [
                4.16331e-05,
                0.00109249,
                0.0598132,
                0.233302,
                0.5,
                0.766698,
                0.995334,
            ],
        ),
        (
            "ease",
            [
                0.00448782, 0.0329515, 0.513315, 0.747853, 0.802403, 0.847752, 0.994316,
            ],
        ),
    ];
    for (css, values) in rows {
        for (f, want) in fractions.iter().zip(values) {
            let got = easing(css).evaluate(*f, false);
            assert!(close(got, want), "{css} at {f}: {got} != {want}");
        }
    }
    // `easing-progress-precision`, query 1: the opacity of a 1000 s
    // animation at 1 s, 50 s and 99.9 s.
    let rows: [(&str, [f64; 3]); 3] = [
        ("ease", [0.000404803, 0.0329515, 0.0946424]),
        ("cubic-bezier(.2,.8,.2,1)", [0.0040016, 0.202241, 0.396328]),
        ("cubic-bezier(.3,1.6,.6,1)", [0.00532519, 0.246699, 0.45479]),
    ];
    for (css, values) in rows {
        for (t, want) in [0.001, 0.05, 0.0999].iter().zip(values) {
            let got = easing(css).evaluate(*t, false);
            assert!(close(got, want), "{css} at {t}: {got} != {want}");
        }
    }
    // `e4`: `cubic-bezier(0,0,1,-0.5)` goes below 0, which `opacity`
    // clamps to 0.
    assert!(easing("cubic-bezier(0,0,1,-0.5)").evaluate(0.5, false) < 0.0);
}

#[test]
fn steps_and_the_before_flag() {
    // CSS Easing 2 §4: at an integral step position the before flag
    // lowers the step by one, and progress 0 never goes below 0.
    let start = easing("steps(4, start)");
    assert_eq!(start.evaluate(0.0, false), 0.25);
    assert_eq!(start.evaluate(0.0, true), 0.0);
    assert_eq!(start.evaluate(0.5, true), 0.5);
    assert_eq!(start.evaluate(-0.1, true), 0.0);
    let end = easing("steps(4)");
    assert_eq!(end.evaluate(1.0, false), 1.0);
    assert_eq!(end.evaluate(0.999, false), 0.75);
    assert_eq!(easing("steps(3, jump-none)").evaluate(0.5, false), 0.5);
    assert_eq!(easing("steps(3, jump-both)").evaluate(0.0, false), 0.25);
    assert_eq!(easing("step-start").evaluate(0.0, false), 1.0);
    assert_eq!(easing("step-end").evaluate(0.99, false), 0.0);
    // Beyond the interval the output keeps going (iteration progress
    // stays in 0 to 1, but an easing can be called outside).
    assert_eq!(end.evaluate(1.5, false), 1.5);
    // The largest count: O(1).
    let many = easing("steps(2147483648)");
    assert_eq!(
        many,
        Easing::Steps {
            count: MAX_STEPS,
            position: StepPosition::End,
            math: None
        }
    );
    assert!((many.evaluate(0.5, false) - 0.5).abs() < 1e-9);
}

#[test]
fn linear_points_fill_and_extend() {
    let e = easing("linear(0, 0.25 25%, 1)");
    assert!(close(e.evaluate(-0.5, false), -0.5));
    assert!(close(e.evaluate(1.5, false), 1.5));
    // Equal inputs: the output of the later point.
    let jump = easing("linear(0 50%, 1 50%)");
    assert_eq!(jump.evaluate(0.5, false), 1.0);
    // Before the first point too (the spec takes the later point).
    assert_eq!(jump.evaluate(0.49, false), 1.0);
    // 100,000 points (ADR 0044 section 11): binary search.
    let points: Vec<String> = (0..100_000).map(|i| format!("{}", i % 2)).collect();
    let big = easing(&format!("linear({})", points.join(", ")));
    let Easing::Points(list) = &big else {
        panic!("linear() points");
    };
    assert_eq!(list.len(), 100_000);
    assert!((0.0..=1.0).contains(&big.evaluate(0.123_456, false)));
}

/// (value, specified text, computed text); `None` for invalid values. The
/// rows of `longhand-values` and `longhand-computed` for
/// `animation-timing-function` (css-animations-3a.json).
const TEXTS: &[(&str, Option<(&str, &str)>)] = &[
    ("ease", Some(("ease", "ease"))),
    ("EASE-IN", Some(("ease-in", "ease-in"))),
    ("step-start", Some(("step-start", "steps(1, start)"))),
    ("step-end", Some(("step-end", "steps(1)"))),
    ("steps(4, end)", Some(("steps(4)", "steps(4)"))),
    ("steps(4, jump-end)", Some(("steps(4)", "steps(4)"))),
    (
        "steps(4,start)",
        Some(("steps(4, start)", "steps(4, start)")),
    ),
    (
        "steps(4, jump-start)",
        Some(("steps(4, jump-start)", "steps(4, jump-start)")),
    ),
    ("steps(1, jump-none)", None),
    (
        "steps(2, jump-none)",
        Some(("steps(2, jump-none)", "steps(2, jump-none)")),
    ),
    (
        "steps(3, jump-both)",
        Some(("steps(3, jump-both)", "steps(3, jump-both)")),
    ),
    ("steps(0)", None),
    (
        "steps(2147483648)",
        Some(("steps(2147483647)", "steps(2.14748e+09)")),
    ),
    ("steps(1000000)", Some(("steps(1000000)", "steps(1e+06)"))),
    ("steps(calc(2 + 1))", Some(("steps(calc(3))", "steps(3)"))),
    ("steps(2.5)", None),
    ("steps(4, END)", Some(("steps(4)", "steps(4)"))),
    (
        "cubic-bezier(0.1, 0.7, 1, 0.1)",
        Some((
            "cubic-bezier(0.1, 0.7, 1, 0.1)",
            "cubic-bezier(0.1, 0.7, 1, 0.1)",
        )),
    ),
    ("cubic-bezier(1.1, 0, 0, 1)", None),
    (
        "cubic-bezier(0, -2, 1, 3)",
        Some(("cubic-bezier(0, -2, 1, 3)", "cubic-bezier(0, -2, 1, 3)")),
    ),
    (
        "cubic-bezier(calc(0.5), 0, 1, 1)",
        Some(("cubic-bezier(0.5, 0, 1, 1)", "cubic-bezier(0.5, 0, 1, 1)")),
    ),
    (
        "linear(0, 1)",
        Some(("linear(0 0%, 1 100%)", "linear(0 0%, 1 100%)")),
    ),
    (
        "linear(0 0% 50%, 1)",
        Some(("linear(0 0%, 0 50%, 1 100%)", "linear(0 0%, 0 50%, 1 100%)")),
    ),
    (
        "linear(0, 0.5 30% 60%, 1)",
        Some((
            "linear(0 0%, 0.5 30%, 0.5 60%, 1 100%)",
            "linear(0 0%, 0.5 30%, 0.5 60%, 1 100%)",
        )),
    ),
    ("linear(0.5)", None),
    ("linear()", None),
    (
        "linear(0, 1 50%, 0 25%, 1)",
        Some((
            "linear(0 0%, 1 50%, 0 50%, 1 100%)",
            "linear(0 0%, 1 50%, 0 50%, 1 100%)",
        )),
    ),
    (
        "linear(0 -10%, 1 110%)",
        Some(("linear(0 -10%, 1 110%)", "linear(0 -10%, 1 110%)")),
    ),
    (
        "linear(0, calc(0.5) 50%, 1)",
        Some((
            "linear(0 0%, 0.5 50%, 1 100%)",
            "linear(0 0%, 0.5 50%, 1 100%)",
        )),
    ),
    (
        "linear(0 20%, 1)",
        Some(("linear(0 20%, 1 100%)", "linear(0 20%, 1 100%)")),
    ),
    (
        "linear(0, 0, 1)",
        Some(("linear(0 0%, 0 50%, 1 100%)", "linear(0 0%, 0 50%, 1 100%)")),
    ),
    (
        "linear(0, 0.3, 0.6, 1)",
        Some((
            "linear(0 0%, 0.3 33.3333%, 0.6 66.6667%, 1 100%)",
            "linear(0 0%, 0.3 33.3333%, 0.6 66.6667%, 1 100%)",
        )),
    ),
    (
        "linear(0, 1 calc(50% + 10%), 1)",
        Some(("linear(0 0%, 1 60%, 1 100%)", "linear(0 0%, 1 60%, 1 100%)")),
    ),
    ("linear(0 10% 20% 30%, 1)", None),
    ("linear", Some(("linear", "linear"))),
    (
        "LINEAR(0, 1)",
        Some(("linear(0 0%, 1 100%)", "linear(0 0%, 1 100%)")),
    ),
];

fn check_texts(rows: &[(&str, Option<(&str, &str)>)]) {
    for &(css, want) in rows {
        let got = parse_all(css, parse_easing)
            .ok()
            .map(|e| (e.declared_text(), e.computed_text()));
        let want = want.map(|(a, b)| (a.to_owned(), b.to_owned()));
        assert_eq!(got, want, "{css}");
    }
}

#[test]
fn specified_and_computed_texts() {
    check_texts(TEXTS);
}

/// The easing rows of `number-ranges` (css-animations-3a.json): literals
/// saturate to the `f32` range; an infinite math function is the largest
/// `f64`, NaN is 0; the count of `steps()` keeps its math function.
#[rustfmt::skip]
const RANGE_TEXTS: &[(&str, Option<(&str, &str)>)] = &[
    ("cubic-bezier(0, calc(1/0), 1, 1)", Some(("cubic-bezier(0, 1.79769e+308, 1, 1)", "cubic-bezier(0, 1.79769e+308, 1, 1)"))),
    ("cubic-bezier(0, calc(-1/0), 1, 1)", Some(("cubic-bezier(0, -1.79769e+308, 1, 1)", "cubic-bezier(0, -1.79769e+308, 1, 1)"))),
    ("cubic-bezier(0, calc(0/0), 1, 1)", Some(("cubic-bezier(0, 0, 1, 1)", "cubic-bezier(0, 0, 1, 1)"))),
    ("cubic-bezier(calc(0/0), 0, 1, 1)", Some(("cubic-bezier(0, 0, 1, 1)", "cubic-bezier(0, 0, 1, 1)"))),
    ("cubic-bezier(calc(1/0), 0, 1, 1)", None),
    ("cubic-bezier(0, 1e999, 1, 1)", Some(("cubic-bezier(0, 3.40282e+38, 1, 1)", "cubic-bezier(0, 3.40282e+38, 1, 1)"))),
    ("cubic-bezier(0, 1e40, 1, -1e40)", Some(("cubic-bezier(0, 3.40282e+38, 1, -3.40282e+38)", "cubic-bezier(0, 3.40282e+38, 1, -3.40282e+38)"))),
    ("steps(calc(2.5))", Some(("steps(calc(2.5))", "steps(3)"))),
    ("steps(calc(2.5), end)", Some(("steps(calc(2.5))", "steps(3)"))),
    ("steps(calc(-1))", Some(("steps(calc(-1))", "steps(1)"))),
    ("steps(calc(0))", Some(("steps(calc(0))", "steps(1)"))),
    ("steps(calc(1/0))", Some(("steps(calc(infinity))", "steps(2.14748e+09)"))),
    ("steps(calc(-1/0))", Some(("steps(calc(-infinity))", "steps(1)"))),
    ("steps(calc(0/0))", Some(("steps(calc(NaN))", "steps(0)"))),
    ("steps(calc(1), jump-none)", None),
    ("steps(calc(1.5), jump-none)", Some(("steps(calc(1.5), jump-none)", "steps(2, jump-none)"))),
    ("steps(calc(-1), jump-none)", None),
    ("steps(calc(1e10))", Some(("steps(calc(1e+10))", "steps(2.14748e+09)"))),
    ("linear(calc(1/0), 1)", Some(("linear(1.79769e+308 0%, 1 100%)", "linear(1.79769e+308 0%, 1 100%)"))),
    ("linear(calc(0/0), 1)", Some(("linear(0 0%, 1 100%)", "linear(0 0%, 1 100%)"))),
    ("linear(calc(-1/0), 1)", Some(("linear(-1.79769e+308 0%, 1 100%)", "linear(-1.79769e+308 0%, 1 100%)"))),
    ("linear(1e999, 0)", Some(("linear(3.40282e+38 0%, 0 100%)", "linear(3.40282e+38 0%, 0 100%)"))),
    ("linear(1e300 0%, 0 100%)", Some(("linear(3.40282e+38 0%, 0 100%)", "linear(3.40282e+38 0%, 0 100%)"))),
    ("linear(0, 1 calc(1%/0))", Some(("linear(0 0%, 1 1.79769e+308%)", "linear(0 0%, 1 1.79769e+308%)"))),
    ("linear(0, 1 calc(0%/0))", Some(("linear(0 0%, 1 0%)", "linear(0 0%, 1 0%)"))),
    ("linear(0 calc(-1%/0), 1)", Some(("linear(0 -1.79769e+308%, 1 100%)", "linear(0 -1.79769e+308%, 1 100%)"))),
    ("linear(0 1e999%, 1)", Some(("linear(0 3.40282e+38%, 1 3.40282e+38%)", "linear(0 3.40282e+38%, 1 3.40282e+38%)"))),
];

#[test]
fn the_range_of_arguments() {
    check_texts(RANGE_TEXTS);
}

/// Huge control points and outputs, a NaN count and extreme inputs give
/// finite outputs (the interpolation casts them to `f32` after).
#[test]
fn outputs_are_finite() {
    for css in [
        "cubic-bezier(0, calc(1/0), 1, calc(-1/0))",
        "cubic-bezier(0.5, 1e999, 0.5, -1e999)",
        "linear(calc(-1/0), calc(1/0))",
        "linear(1e300 0%, 0 100%)",
        "linear(0 calc(-1%/0), 1 calc(1%/0))",
        "steps(calc(0/0))",
        "steps(calc(0/0), jump-both)",
    ] {
        let e = easing(css).computed();
        for input in [
            f64::NEG_INFINITY,
            -f64::MAX,
            -1e300,
            -1.0,
            0.0,
            0.25,
            0.5,
            1.0,
            2.0,
            1e300,
            f64::MAX,
            f64::INFINITY,
            f64::NAN,
        ] {
            for before in [false, true] {
                let v = e.evaluate(input, before);
                assert!(v.is_finite(), "{css} at {input}: {v}");
            }
        }
    }
}
