//! Tests of the keyframe sets: the rules of `keyframes-resolution`
//! (`tools/probes/css-animations-3a.json`, Chromium 148) and the intervals
//! of `easing-in-keyframes` and `spin-implicit-from`
//! (`crates/automation/tests/data/css-animations.json`).

// The numbers are Chromium's values as it prints them.
#![allow(clippy::unreadable_literal)]

use std::fmt::Write as _;

use swb_css::{CssRule, MediaEnvironment, parse_stylesheet};
use url::Url;

use super::*;
use crate::Stylist;
use crate::properties::ids::LonghandValue;
use crate::values::SpecifiedLengthPercentage as Lp;

/// The keyframe set of `name` in a stylist with the author sheet `css`.
fn set(css: &str, name: &str) -> Option<Arc<KeyframeSet>> {
    let mut stylist = Stylist::new(swb_dom::QuirksMode::NoQuirks);
    let url = Url::parse("https://example.com/").expect("valid URL");
    stylist.add_author_sheet(&parse_stylesheet(css), &url);
    stylist.keyframes_in(name, &MediaEnvironment::default())
}

/// The px value of a `left`, `top` or other length declaration.
fn px(k: &Keyframe) -> Option<f32> {
    match &k.declaration {
        PropertyDeclaration::Value(
            LonghandValue::Left(Some(Lp::Length(l))) | LonghandValue::Top(Some(Lp::Length(l))),
        ) => Some(l.value),
        _ => None,
    }
}

/// The (offset, px, easing) of each keyframe of `property`.
fn frames(set: &KeyframeSet, property: LonghandId) -> Vec<(f64, Option<f32>, Option<Easing>)> {
    set.properties
        .iter()
        .find(|p| p.property == AnimatedProperty::Longhand(property))
        .map(|p| {
            p.keyframes
                .iter()
                .map(|k| (k.offset, px(k), k.easing.clone()))
                .collect()
        })
        .unwrap_or_default()
}

const LEFT: LonghandId = LonghandId::Left;
const TOP: LonghandId = LonghandId::Top;

#[test]
fn duplicates_merge_and_offsets_sort() {
    let s = set(
        "@keyframes dup{50%{left:10px}50%{top:5px;left:20px}to{left:30px}}",
        "dup",
    )
    .expect("a set");
    assert_eq!(
        frames(&s, LEFT),
        [(0.5, Some(20.0), None), (1.0, Some(30.0), None)]
    );
    assert_eq!(frames(&s, TOP), [(0.5, Some(5.0), None)]);
    let s = set("@keyframes o{to{left:1px}from{left:2px}50%{left:3px}}", "o").expect("a set");
    assert_eq!(
        frames(&s, LEFT),
        [
            (0.0, Some(2.0), None),
            (0.5, Some(3.0), None),
            (1.0, Some(1.0), None)
        ]
    );
    let s = set("@keyframes m{0%,100%{left:1px}50%{left:2px}}", "m").expect("a set");
    assert_eq!(frames(&s, LEFT).len(), 3);
}

#[test]
fn dropped_declarations() {
    // `!important`, invalid values, animation and transition properties,
    // `all`.
    let s = set(
        "@keyframes i{from{left:0 !important;top:1px}to{left:10px}}",
        "i",
    )
    .expect("set");
    assert_eq!(frames(&s, LEFT), [(1.0, Some(10.0), None)]);
    let s = set(
        "@keyframes v{from{left:foo;top:1px}to{left:2px;top:bar}}",
        "v",
    )
    .expect("set");
    assert_eq!(frames(&s, LEFT), [(1.0, Some(2.0), None)]);
    assert_eq!(frames(&s, TOP), [(0.0, Some(1.0), None)]);
    let s = set(
        "@keyframes a{from{left:0;animation-duration:5s;animation-name:x;\
         animation-timing-function:steps(2)}to{left:10px;animation-delay:1s;transition:left 1s}}",
        "a",
    )
    .expect("set");
    assert_eq!(s.properties.len(), 1);
    let steps = Easing::Steps {
        count: 2,
        position: super::super::easing::StepPosition::End,
        math: None,
    };
    assert_eq!(
        frames(&s, LEFT),
        [(0.0, Some(0.0), Some(steps)), (1.0, Some(10.0), None)]
    );
    let s = set("@keyframes k{to{all:initial}}", "k").expect("set");
    assert!(s.is_empty());
    let s = set("@keyframes d{to{left:1px !important}to{left:2px}}", "d").expect("set");
    assert_eq!(frames(&s, LEFT), [(1.0, Some(2.0), None)]);
    // Shorthands expand; custom properties stay (they animate, ADR 0044).
    let s = set("@keyframes sh{to{margin:1px 2px;--v:10px}}", "sh").expect("set");
    assert_eq!(s.properties.len(), 5);
}

#[test]
fn keyframes_of_different_easings_stay_apart() {
    let s = set(
        "@keyframes e{50%{left:1px;animation-timing-function:linear}\
         50%{top:1px;left:5px;animation-timing-function:ease-in}to{left:2px;top:2px}}",
        "e",
    )
    .expect("set");
    assert_eq!(
        frames(&s, LEFT),
        [
            (0.5, Some(1.0), Some(Easing::Linear)),
            (0.5, Some(5.0), Some(Easing::EaseIn)),
            (1.0, Some(2.0), None)
        ]
    );
}

#[test]
fn the_winning_rule() {
    let css = "@keyframes twice{to{left:1px}}@keyframes twice{to{top:2px}}\
               @layer la{@keyframes lay{to{left:3px}}}@keyframes lay{to{top:4px}}\
               @layer lb{@keyframes lay2{to{left:5px}}}@layer lc{@keyframes lay2{to{top:6px}}}\
               @layer x2,x1;@layer x1{@keyframes lay3{to{left:7px}}}\
               @layer x2{@keyframes lay3{to{top:8px}}}\
               @keyframes lay4{to{left:9px}}@layer ly{@keyframes lay4{to{top:9px}}}\
               @media (max-width:1px){@keyframes med{to{top:10px}}}\
               @keyframes med2{to{left:1px}}@media (max-width:1px){@keyframes med2{to{top:10px}}}\
               @keyframes Case{to{left:1px}}@keyframes \"str\"{to{left:2px}}";
    let only = |name: &str| -> Option<LonghandId> {
        let s = set(css, name)?;
        match s.properties.as_slice() {
            [p] => match p.property {
                AnimatedProperty::Longhand(id) => Some(id),
                AnimatedProperty::Custom(_) => None,
            },
            _ => None,
        }
    };
    assert_eq!(only("twice"), Some(TOP));
    assert_eq!(only("lay"), Some(TOP));
    assert_eq!(only("lay2"), Some(TOP));
    assert_eq!(only("lay3"), Some(LEFT));
    assert_eq!(only("lay4"), Some(LEFT));
    assert!(set(css, "med").is_none());
    assert_eq!(only("med2"), Some(LEFT));
    assert_eq!(only("Case"), Some(LEFT));
    assert!(set(css, "case").is_none());
    assert_eq!(only("str"), Some(LEFT));
    assert!(set("@keyframes empty{}", "empty").is_some_and(|s| s.is_empty()));
}

/// `keyframes-conditions` (css-animations-3a.json, Chromium 148).
#[test]
fn conditions_and_prefixed_rules() {
    let css = "@-webkit-keyframes wk{to{left:5px}}\
               @supports (display:grid){@keyframes sup{to{left:6px}}}\
               @supports not (display:grid){@keyframes nsup{to{left:6px}}}\
               @container (min-width:1px){@keyframes cont{to{left:7px}}}\
               @container (max-width:1px){@keyframes cont2{to{left:8px}}}\
               @keyframes both{to{left:1px}}@-webkit-keyframes both{to{top:2px}}\
               @-webkit-keyframes both2{to{left:1px}}@keyframes both2{to{top:2px}}\
               @layer la{@keyframes lp{to{left:1px}}}@-webkit-keyframes lp{to{top:2px}}\
               @-webkit-keyframes lp2{to{left:1px}}@layer lb{@keyframes lp2{to{top:2px}}}";
    let first = |name: &str| -> Option<LonghandId> {
        match set(css, name)?.properties.first()?.property {
            AnimatedProperty::Longhand(id) => Some(id),
            AnimatedProperty::Custom(_) => None,
        }
    };
    assert_eq!(first("wk"), Some(LEFT));
    assert_eq!(first("sup"), Some(LEFT));
    assert!(set(css, "nsup").is_none());
    // `@container` does not decide: the rule counts.
    assert_eq!(first("cont"), Some(LEFT));
    assert_eq!(first("cont2"), Some(LEFT));
    // `@keyframes` beats `@-webkit-keyframes`, in any order and layer.
    assert_eq!(first("both"), Some(LEFT));
    assert_eq!(first("both2"), Some(TOP));
    assert_eq!(first("lp"), Some(LEFT));
    assert_eq!(first("lp2"), Some(TOP));
}

/// Offsets are `f64`: `getKeyframes()` shows 0.33299999999999996 for
/// `33.3%` (`keyframes-conditions`).
#[test]
fn offsets_are_exact() {
    let s = set(
        "@keyframes off{33.3%{left:1px}12.345678%{left:2px}0.0000001%{left:3px}}",
        "off",
    )
    .expect("a set");
    let offsets: Vec<f64> = frames(&s, LEFT).iter().map(|f| f.0).collect();
    assert_eq!(
        offsets,
        [
            9.999999999999999e-10,
            0.12345677999999999,
            0.33299999999999996
        ]
    );
}

#[test]
fn intervals_with_keyframe_easing() {
    // `easing-in-keyframes`: `k 1s linear both`, keyframes 0% (left 0,
    // `steps(2,end)`), 50% (50px, `ease-in`), 100% (100px).
    let s = set(
        "@keyframes k{0%{left:0;animation-timing-function:steps(2,end)}\
         50%{left:50px;animation-timing-function:ease-in}100%{left:100px}}",
        "k",
    )
    .expect("set");
    let rows = [
        (0.0, 0.0),
        (100.0, 0.0),
        (250.0, 25.0),
        (499.0, 25.0),
        (500.0, 50.0),
        (600.0, 53.1141),
        (750.0, 65.7678),
        (900.0, 84.5817),
        (1000.0, 100.0),
    ];
    for (time, want) in rows {
        let i = s
            .interval(0, time / 1000.0, &Easing::Linear, false)
            .expect("left");
        let a = f64::from(i.from.and_then(px).expect("explicit"));
        let b = f64::from(i.to.and_then(px).expect("explicit"));
        let got = a + (b - a) * i.progress;
        assert!((got - want).abs() < 1e-3, "{time}: {got} != {want}");
    }
}

#[test]
fn implicit_keyframes_are_the_underlying_value() {
    // `spin-implicit-from`: only a `to` keyframe.
    let s = set("@keyframes spin{to{left:10px}}", "spin").expect("set");
    let i = s.interval(0, 0.25, &Easing::Linear, false).expect("left");
    assert!(i.from.is_none());
    assert_eq!(i.to.and_then(px), Some(10.0));
    assert!((i.progress - 0.25).abs() < 1e-12);
    // `onlymid`: implicit keyframes on both sides.
    let s = set("@keyframes m{50%{left:5px}}", "m").expect("set");
    let i = s.interval(0, 0.75, &Easing::Linear, false).expect("left");
    assert_eq!(i.from.and_then(px), Some(5.0));
    assert!(i.to.is_none());
    assert!((i.progress - 0.5).abs() < 1e-12);
    // Before 0 with two keyframes at 0: the first alone.
    let s = set(
        "@keyframes z{0%{left:1px}0%{left:2px;animation-timing-function:linear}}",
        "z",
    )
    .expect("set");
    let i = s.interval(0, -0.5, &Easing::Linear, false).expect("left");
    assert_eq!(i.from.and_then(px), Some(1.0));
    assert_eq!(i.progress, 0.0);
}

#[test]
fn many_keyframes() {
    // ADR 0044 section 11: one rule with 100,000 keyframes.
    let mut frames = String::new();
    for i in 0..100_000 {
        let _ = write!(frames, "{}%{{left:{}px}}", f64::from(i) / 1000.0, i % 7);
    }
    let css = format!("@keyframes big{{{frames}}}");
    let sheet = parse_stylesheet(&css);
    let Some(CssRule::Keyframes(rule)) = sheet.rules.first() else {
        panic!("one rule");
    };
    let started = std::time::Instant::now();
    let s = KeyframeSet::new(rule, &ParserContext::author(None, false));
    assert!(started.elapsed().as_secs() < 10);
    assert_eq!(
        s.properties.first().map(|p| p.keyframes.len()),
        Some(100_000)
    );
    assert!(s.interval(0, 0.5, &Easing::Linear, false).is_some());
}
