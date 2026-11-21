//! Tests of the animation properties: parsing, computed values and the
//! list entries. The texts of `element.style` and `getComputedStyle` are
//! checked against Chromium in `crates/automation/tests/css_animations.rs`
//! (`tools/probes/css-animations-3a.json`).

// The numbers are Chromium's values as it prints them.
#![allow(clippy::unreadable_literal)]

use swb_css::parse_style_attribute;

use super::*;
use crate::animation::easing::StepPosition;
use crate::parse::ParserContext;
use crate::properties::compute::apply as apply_value;
use crate::properties::{DeclarationBlock, PropertyDeclaration};
use crate::values::LengthContext;

/// The style with the declarations `css` computed on the initial style.
fn style(css: &str) -> ComputedStyle {
    let block = DeclarationBlock::parse(
        &parse_style_attribute(css),
        &ParserContext::author(None, false),
    );
    let initial = ComputedStyle::initial();
    let mut s = (*initial).clone();
    let cx = ComputeContext {
        parent: &initial,
        lengths: LengthContext::DEFAULT,
        element: None,
        quirks: false,
    };
    for decl in &block.normal {
        if let PropertyDeclaration::Value(v) = decl {
            apply_value(v, &cx, &mut s);
        }
    }
    s
}

fn text(css: &str, name: &str) -> String {
    computed_text(name, &style(css)).unwrap_or_default()
}

#[test]
fn initial_record_is_shared() {
    let s = style("color: red");
    assert!(s.animations.is_none());
    assert_eq!(text("color: red", "animation-name"), "none");
    assert_eq!(text("color: red", "animation-duration"), "0s");
    assert_eq!(text("color: red", "animation-timing-function"), "ease");
    // Setting the initial value again drops the record.
    assert!(
        style("animation-name: none; animation-delay: 0s")
            .animations
            .is_none()
    );
    assert!(style("animation-name: a").animations.is_some());
}

#[test]
fn shorthand_sets_the_layers() {
    let css = "animation: hero-video-in 1.1s cubic-bezier(.2, .8, .2, 1) .9s both paused";
    let s = style(css);
    let a = AnimationStyle::of(&s);
    let e = a.entry(0).expect("one animation");
    assert_eq!(e.name, Some("hero-video-in"));
    assert!((e.timing.duration - 1100.0).abs() < 1e-9);
    assert!((e.timing.delay - 900.0).abs() < 1e-9);
    assert_eq!(e.timing.fill, FillMode::Both);
    assert_eq!(e.play_state, PlayState::Paused);
    assert_eq!(
        *e.keyframe_easing,
        Easing::CubicBezier([0.2, 0.8, 0.2, 1.0])
    );
    assert_eq!(e.timing.easing, Easing::Linear);
    assert!(a.entry(1).is_none());
    assert_eq!(text(css, "animation-delay"), "0.9s");
    assert_eq!(text(css, "animation-timeline"), "auto");
}

#[test]
fn lists_repeat_to_the_names() {
    // `inherit-and-lists` (css-animations-3a.json): three names, two
    // durations and one easing.
    let css = "animation-name: a, b, c; animation-duration: 1s, 2s; \
               animation-timing-function: linear";
    let s = style(css);
    let a = AnimationStyle::of(&s);
    let durations: Vec<f64> = (0..3)
        .filter_map(|i| a.entry(i))
        .map(|e| e.timing.duration)
        .collect();
    assert_eq!(durations, [1000.0, 2000.0, 1000.0]);
    assert_eq!(text(css, "animation-duration"), "1s, 2s");
    assert_eq!(
        computed_shorthand(&s),
        "1s linear a, 2s linear b, 1s linear c"
    );
    // More durations than names: cut to the names.
    let s = style("animation-name: a; animation-duration: 1s, 2s, 3s");
    assert_eq!(computed_shorthand(&s), "1s a");
    let s = style("animation-name: none, a; animation-duration: 1s");
    assert_eq!(computed_shorthand(&s), "1s, 1s a");
}

#[test]
fn computed_values_clamp() {
    // `longhand-computed`: a negative math function computes to 0 for the
    // duration and the iteration count, not for the delay.
    assert_eq!(
        text("animation-duration: calc(1s - 2s)", "animation-duration"),
        "0s"
    );
    assert_eq!(text("animation-delay: calc(-1s)", "animation-delay"), "-1s");
    assert_eq!(
        text(
            "animation-iteration-count: calc(-1)",
            "animation-iteration-count"
        ),
        "0"
    );
    assert_eq!(
        text("animation-duration: auto, 1s", "animation-duration"),
        "0s, 1s"
    );
    assert_eq!(
        text(
            "animation-timing-function: step-start",
            "animation-timing-function"
        ),
        "steps(1, start)"
    );
    // Invalid values leave the initial value.
    assert_eq!(text("animation-duration: -1s", "animation-duration"), "0s");
    assert_eq!(text("animation-name: a, inherit", "animation-name"), "none");
    assert_eq!(
        text("animation-iteration-count: -1", "animation-iteration-count"),
        "1"
    );
}

#[test]
fn the_range_of_computed_values() {
    // `number-ranges`: literals saturate to the `f32` range; an infinite
    // math function is the largest `f64`, clamped like other values.
    let rows = [
        ("animation-duration", "1e999ms", "3.40282e+35s"),
        ("animation-duration", "calc(1s/0)", "1.79769e+308s"),
        ("animation-duration", "calc(-1s/0)", "0s"),
        ("animation-duration", "calc(NaN * 1s)", "0s"),
        ("animation-delay", "calc(-1s/0)", "-1.79769e+308s"),
        ("animation-delay", "-1e999s", "-3.40282e+38s"),
        ("animation-delay", "calc(1s*NaN)", "0s"),
        ("animation-iteration-count", "1e999", "3.40282e+38"),
        ("animation-iteration-count", "calc(1/0)", "1.79769e+308"),
        ("animation-iteration-count", "calc(-1/0)", "0"),
        ("animation-iteration-count", "calc(NaN)", "0"),
        (
            "animation-iteration-count",
            "infinite, calc(1/0)",
            "infinite, 1.79769e+308",
        ),
    ];
    for (name, value, want) in rows {
        assert_eq!(text(&format!("{name}: {value}"), name), want, "{value}");
    }
    let s = style("animation: a calc(1s/0)");
    assert_eq!(computed_shorthand(&s), "1.79769e+308s a");
    // The times of the timing model stay finite in ms.
    let entry = AnimationStyle::of(&s).entry(0).expect("one animation");
    assert_eq!(entry.timing.duration, f64::MAX);
}

#[test]
fn names() {
    assert_eq!(text("animation-name: \"foo\"", "animation-name"), "foo");
    assert_eq!(
        text("animation-name: 'foo bar'", "animation-name"),
        "foo\\ bar"
    );
    assert_eq!(
        text("animation-name: \"none\"", "animation-name"),
        "\"none\""
    );
    assert_eq!(
        text("animation-name: Foo, NONE", "animation-name"),
        "Foo, none"
    );
    assert_eq!(text("animation-name: \"\"", "animation-name"), "none");
    assert_eq!(text("animation-name: default", "animation-name"), "none");
    let s = style("animation-name: \"none\"");
    assert_eq!(
        AnimationStyle::of(&s).entry(0).and_then(|e| e.name),
        Some("none")
    );
}

#[test]
fn shorthand_keywords_and_names() {
    // `shorthand-values`: a keyword fills its part first, then the name.
    let cases = [
        ("ease ease", "ease ease"),
        ("forwards forwards", "forwards forwards"),
        ("paused running", "paused running"),
        ("normal normal", "normal normal"),
        ("infinite infinite", "infinite infinite"),
        ("2 infinite", "2 infinite"),
        ("linear linear 1s", "1s linear linear"),
        ("a -1s", "0s -1s a"),
        ("none 1s", "1s"),
        ("reverse", "reverse"),
        ("none", "none"),
        // A string name that is a keyword of a part (`p-animation` of the
        // review probe: Chromium `none "none"`).
        ("'none'", "none \"none\""),
        ("a 1s, none", "1s a, none"),
        (
            "a 1s steps(4, end) 2 alternate both paused",
            "1s steps(4) 2 alternate both paused a",
        ),
    ];
    for (value, want) in cases {
        let s = style(&format!("animation: {value}"));
        assert_eq!(computed_shorthand(&s), want, "{value}");
    }
    for invalid in [
        "a infinite 2",
        "a forwards backwards",
        "1s 2s 3s",
        "a 1s, , b",
        "a 1s add",
    ] {
        let s = style(&format!("animation: {invalid}"));
        assert!(s.animations.is_none(), "{invalid}");
    }
    // A timeline or range that is not initial empties the shorthand.
    let s = style("animation: a 1s; animation-timeline: none");
    assert_eq!(computed_shorthand(&s), "");
    let s = style("animation: a 1s; animation-composition: add");
    assert_eq!(computed_shorthand(&s), "1s a");
}

#[test]
fn timelines_and_ranges() {
    let t = |v: &str| text(&format!("animation-timeline: {v}"), "animation-timeline");
    assert_eq!(t("scroll(root block)"), "scroll(root)");
    assert_eq!(t("--foo, auto"), "--foo, auto");
    assert_eq!(t("view(x 10px 10px)"), "view(x 10px)");
    assert_eq!(t("foo"), "auto");
    let r = |v: &str| {
        let s = style(&format!("animation-range: {v}"));
        computed_range(&s)
    };
    assert_eq!(r("cover 0% cover 100%"), "cover");
    assert_eq!(r("entry 10% exit 90%"), "entry 10% exit 90%");
    assert_eq!(r("10% 90%"), "10% 90%");
    assert_eq!(r("normal normal"), "normal");
}

#[test]
fn element_style_texts() {
    let d = |name: &str, v: &str| declared_text(name, &swb_css::parse_component_values(v));
    assert_eq!(d("animation-duration", "1e3ms").as_deref(), Some("1000ms"));
    assert_eq!(
        d("animation-duration", "calc(1s + 500ms)").as_deref(),
        Some("calc(1.5s)")
    );
    assert_eq!(
        d("animation-iteration-count", "calc(1 + 1)").as_deref(),
        Some("calc(2)")
    );
    assert_eq!(
        d("animation-timing-function", "steps(4, jump-end)").as_deref(),
        Some("steps(4)")
    );
    assert_eq!(d("animation-duration", "0"), None);
    let expanded = expand_animation(&swb_css::parse_component_values("a 1s, b")).expect("valid");
    assert_eq!(
        expanded,
        [
            "1s, auto",
            "ease, ease",
            "0s, 0s",
            "1, 1",
            "normal, normal",
            "none, none",
            "running, running",
            "a, b",
            "auto",
            "normal",
            "normal"
        ]
    );
    let texts: Vec<&str> = expanded.iter().map(String::as_str).collect();
    assert_eq!(
        animation_value(&texts),
        "1s ease 0s 1 normal none running a, auto ease 0s 1 normal none running b"
    );
    // `animation: a 1s; animation-delay: 1s, 2s` (`shorthand-blocks`).
    let mut texts = texts;
    texts[0] = "1s";
    texts[1] = "ease";
    texts[2] = "1s, 2s";
    texts[3] = "1";
    texts[4] = "normal";
    texts[5] = "none";
    texts[6] = "running";
    texts[7] = "a";
    assert_eq!(
        animation_value(&texts),
        "1s ease 1s 1 normal none running a, 2s"
    );
    texts[8] = "none";
    assert_eq!(animation_value(&texts), "");
    let steps = Easing::Steps {
        count: 4,
        position: StepPosition::End,
        math: None,
    };
    assert_eq!(steps.declared_text(), "steps(4)");
}
