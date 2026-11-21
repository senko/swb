//! The resolved values of the properties of M13 feature 2a (`box-shadow`,
//! `text-shadow`, `translate`, `rotate`, `scale`, `corner-*-shape`),
//! written as Chromium 148 writes the computed values
//! (`tools/probes/css-videojs-2a.json`).

use std::fmt::Write as _;

use super::{number, px};
use crate::ComputedStyle;
use crate::values::{CornerShape, LengthPercentage, Rotate, Scale, Translate};

/// `box-shadow`: the color first, then the offsets, the blur and the
/// spread, `inset` last; `none` for an empty list.
pub(super) fn box_shadow(s: &ComputedStyle) -> String {
    list(
        s.box_shadow
            .iter()
            .map(|b| shadow(s, b.color, &[b.x, b.y, b.blur, b.spread], b.inset)),
    )
}

/// `text-shadow`: the color, the offsets and the blur.
pub(super) fn text_shadow(s: &ComputedStyle) -> String {
    list(
        s.text_shadow
            .iter()
            .map(|t| shadow(s, t.color, &[t.x, t.y, t.blur], false)),
    )
}

fn list(items: impl Iterator<Item = String>) -> String {
    let items: Vec<String> = items.collect();
    if items.is_empty() {
        "none".to_owned()
    } else {
        items.join(", ")
    }
}

fn shadow(s: &ComputedStyle, color: crate::values::Color, lengths: &[f32], inset: bool) -> String {
    let mut out = color.resolved_text(s.color);
    for &l in lengths {
        let _ = write!(out, " {}", px(l));
    }
    if inset {
        out.push_str(" inset");
    }
    out
}

fn lp(v: &LengthPercentage) -> String {
    super::longhand::lp(v)
}

/// `translate`: `none`, or the x offset, the y offset if it is not zero or
/// the z offset is, and the z offset if it is not zero.
pub(super) fn translate(t: Option<&Translate>) -> String {
    let Some(t) = t else {
        return "none".to_owned();
    };
    let y_zero = matches!(t.y, LengthPercentage::Px(v) if v == 0.0);
    let mut out = lp(&t.x);
    if !y_zero || t.z != 0.0 {
        let _ = write!(out, " {}", lp(&t.y));
    }
    if t.z != 0.0 {
        let _ = write!(out, " {}", px(t.z));
    }
    out
}

/// `rotate`: `none`, the angle for the z axis, `x`, `y` or `z` and the
/// angle for another unit axis, else the axis and the angle.
pub(super) fn rotate(r: Option<&Rotate>) -> String {
    let Some(r) = r else {
        return "none".to_owned();
    };
    let angle = format!("{}deg", number(r.angle));
    // A positive multiple of a unit vector is written as the axis name.
    let [x, y, z] = r.axis;
    let unit = |a: f32, b: f32, c: f32| a > 0.0 && b == 0.0 && c == 0.0;
    if unit(z, x, y) {
        angle
    } else if unit(x, y, z) {
        format!("x {angle}")
    } else if unit(y, x, z) {
        format!("y {angle}")
    } else {
        format!("{} {} {} {angle}", number(x), number(y), number(z))
    }
}

/// `scale`: `none`, the x factor, and the y factor if it differs or the z
/// factor is not 1, and the z factor if it is not 1.
pub(super) fn scale(s: Option<&Scale>) -> String {
    let Some(s) = s else {
        return "none".to_owned();
    };
    let mut out = number(s.x);
    if s.y != s.x || s.z != 1.0 {
        let _ = write!(out, " {}", number(s.y));
    }
    if s.z != 1.0 {
        let _ = write!(out, " {}", number(s.z));
    }
    out
}

/// A `corner-*-shape`: the keyword, or `superellipse(k)`.
pub(super) fn corner_shape(c: CornerShape) -> String {
    c.keyword()
        .map_or_else(|| format!("superellipse({})", number(c.0)), str::to_owned)
}
