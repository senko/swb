//! Tests of the resolved values. The values of each property group as
//! Chromium gives them are tested with the cascade and layout in
//! `crates/engine/tests/computed_style.rs` (the probe
//! `tools/probes/web-computed-style.json`); these tests cover the rules
//! that do not need a document.

use std::sync::Arc;

use super::*;
use crate::values::{LengthPercentage, LengthPercentageOrAuto, Position, TransformFunction};

fn initial() -> Arc<ComputedStyle> {
    ComputedStyle::initial()
}

fn value(style: &ComputedStyle, used: Option<&UsedBox>, property: &str) -> String {
    resolved_value(
        property,
        &Resolver {
            style,
            used,
            pseudo: None,
        },
    )
}

#[test]
fn every_listed_property_has_a_value() {
    let style = initial();
    for name in property_names() {
        assert!(!value(&style, None, name).is_empty(), "{name}");
    }
    let names = property_names();
    let standard = names.iter().take_while(|n| !n.starts_with('-'));
    assert!(standard.clone().is_sorted());
    assert!(
        names
            .iter()
            .skip(standard.count())
            .all(|n| n.starts_with('-'))
    );
}

#[test]
fn names_aliases_and_unknown_properties() {
    let style = initial();
    assert_eq!(value(&style, None, "COLOR"), "rgb(0, 0, 0)");
    assert_eq!(value(&style, None, "word-wrap"), "normal");
    assert_eq!(value(&style, None, "-webkit-transform"), "none");
    assert_eq!(value(&style, None, "font-family"), "\"Times New Roman\"");
    assert_eq!(value(&style, None, "foo"), "");
    assert_eq!(value(&style, None, "--x"), "");
    assert_eq!(value(&style, None, "border-image"), "none");
    assert!(needs_layout("margin-inline-start"));
    assert!(needs_layout("padding"));
    assert!(!needs_layout("color"));
}

#[test]
fn layout_units_truncate_to_a_sixty_fourth() {
    assert_eq!(layout_px(133.333_33), "133.328px");
    assert_eq!(layout_px(0.1), "0.09375px");
    assert_eq!(layout_px(1e21), "3.35544e+07px");
    // Just below a unit because of `f32`: the unit.
    assert_eq!(layout_px(109.999_98), "110px");
}

/// A block of 200x100 at (10, 20) in a containing block of 400x300, with
/// 5 px padding and 1 px borders.
fn block() -> UsedBox {
    UsedBox {
        kind: BoxKind::Block,
        x: 10.0,
        y: 20.0,
        width: 200.0,
        height: 100.0,
        padding: [5.0; 4],
        border: [1.0; 4],
        cb_width: 400.0,
        cb_height: 300.0,
        scrollport: (800.0, 600.0),
    }
}

#[test]
fn sizes_margins_and_paddings_from_layout() {
    let mut style = (*initial()).clone();
    style.margin_left = LengthPercentageOrAuto::Auto;
    style.margin_right = LengthPercentageOrAuto::LengthPercentage(LengthPercentage::Percent(0.1));
    let used = block();
    assert_eq!(value(&style, Some(&used), "width"), "188px");
    assert_eq!(value(&style, Some(&used), "height"), "88px");
    assert_eq!(value(&style, Some(&used), "margin-left"), "10px");
    assert_eq!(value(&style, Some(&used), "margin-right"), "40px");
    // A percentage padding comes from layout; a fixed one is the computed
    // value, not snapped (Chromium).
    let percent = LengthPercentage::Percent(0.0125);
    style.padding_top = percent.clone();
    style.padding_right = percent.clone();
    style.padding_bottom = percent.clone();
    style.padding_left = percent;
    assert_eq!(value(&style, Some(&used), "padding"), "5px");
    style.padding_top = LengthPercentage::Px(5.3);
    assert_eq!(value(&style, Some(&used), "padding-top"), "5.3px");
    style.margin_top = LengthPercentageOrAuto::LengthPercentage(LengthPercentage::Px(1.1));
    assert_eq!(value(&style, Some(&used), "margin-top"), "1.1px");
    style.box_sizing = crate::values::BoxSizing::BorderBox;
    assert_eq!(value(&style, Some(&used), "width"), "200px");
    // A part of an inline box keeps its computed values.
    let inline = UsedBox {
        kind: BoxKind::Inline,
        ..block()
    };
    assert_eq!(value(&style, Some(&inline), "width"), "auto");
    assert_eq!(value(&style, Some(&inline), "margin-right"), "10%");
}

#[test]
fn insets_of_positioned_boxes() {
    let mut style = (*initial()).clone();
    style.position = Position::Relative;
    style.top = LengthPercentageOrAuto::LengthPercentage(LengthPercentage::Percent(0.05));
    let used = UsedBox {
        cb_height: 463.0,
        ..block()
    };
    assert_eq!(value(&style, Some(&used), "top"), "23.1406px");
    assert_eq!(value(&style, Some(&used), "bottom"), "-23.15px");
    assert_eq!(value(&style, Some(&used), "left"), "0px");
    style.position = Position::Absolute;
    style.top = LengthPercentageOrAuto::Auto;
    style.right = LengthPercentageOrAuto::LengthPercentage(LengthPercentage::Px(5.0));
    let used = block();
    assert_eq!(value(&style, Some(&used), "top"), "20px");
    assert_eq!(value(&style, Some(&used), "bottom"), "180px");
    assert_eq!(value(&style, Some(&used), "left"), "195px");
    assert_eq!(value(&style, Some(&used), "right"), "5px");
    style.position = Position::Sticky;
    assert_eq!(value(&style, Some(&used), "top"), "auto");
    assert_eq!(value(&style, None, "right"), "5px");
}

#[test]
fn transforms_as_matrices() {
    let mut style = (*initial()).clone();
    style.transform = Arc::from([
        TransformFunction::Translate(LengthPercentage::Percent(0.5), LengthPercentage::Px(10.0)),
        TransformFunction::Rotate(90.0),
    ]);
    let used = block();
    assert_eq!(
        value(&style, Some(&used), "transform"),
        "matrix(0, 1, -1, 0, 100, 10)"
    );
    assert_eq!(value(&style, Some(&used), "transform-origin"), "100px 50px");
    // Without a box: `none`, and the origin as computed.
    assert_eq!(value(&style, None, "transform"), "none");
    assert_eq!(value(&style, None, "transform-origin"), "50% 50%");
}

#[test]
fn shorthands_from_longhands() {
    let mut style = (*initial()).clone();
    style.padding_top = LengthPercentage::Px(1.0);
    style.padding_bottom = LengthPercentage::Px(1.0);
    style.padding_left = LengthPercentage::Px(2.0);
    style.padding_right = LengthPercentage::Px(2.0);
    assert_eq!(value(&style, None, "padding"), "1px 2px");
    assert_eq!(value(&style, None, "outline"), "rgb(0, 0, 0) none 3px");
    assert_eq!(value(&style, None, "flex"), "0 1 auto");
    assert_eq!(value(&style, None, "font"), "16px \"Times New Roman\"");
    assert_eq!(value(&style, None, "grid-area"), "auto");
    assert_eq!(value(&style, None, "list-style"), "outside none disc");
    style.border_left_width = 2.0;
    assert_eq!(value(&style, None, "border"), "");
    assert_eq!(value(&style, None, "border-width"), "3px 3px 3px 2px");
}

#[test]
fn content_of_elements_and_pseudo_elements() {
    let style = initial();
    let of = |pseudo| {
        resolved_value(
            "content",
            &Resolver {
                style: &style,
                used: None,
                pseudo,
            },
        )
    };
    assert_eq!(of(None), "normal");
    assert_eq!(of(Some(crate::PseudoKind::Before)), "none");
    assert_eq!(of(Some(crate::PseudoKind::Marker)), "normal");
}
