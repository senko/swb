//! The computed value of `clip-path` as `getComputedStyle()` writes it for
//! a basic shape or a geometry box (measured in Chromium 148,
//! `tools/probes/clip-path.json`): the sides of `inset()` and the corner
//! radii written as short as the box shorthands, a default radius and
//! reference box left out, a position of keywords as lengths, `rect()` and
//! `xywh()` as `inset()`, and `path()` with absolute commands.

use std::sync::Arc;

use super::longhand::lp;
use crate::cssom::clip_path::short_list;
use crate::values::{
    BasicShape, CornerRadius, FillRule, Inset, PositionComponent, ShapeClip, ShapePosition,
    ShapeRadius,
};

/// A basic shape and/or a geometry box.
pub(super) fn shape_clip(clip: &ShapeClip) -> String {
    let mut out = clip.shape.as_ref().map(shape).unwrap_or_default();
    if let Some(b) = clip.geometry_box {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(b.as_str());
    }
    out
}

fn shape(s: &BasicShape) -> String {
    match s {
        BasicShape::Inset(i) => inset(i),
        BasicShape::Circle { radius, position } => {
            let mut parts = Vec::new();
            parts.extend(radius_text(radius));
            parts.extend(position_text(position.as_ref()));
            format!("circle({})", parts.join(" "))
        }
        BasicShape::Ellipse { rx, ry, position } => {
            let mut parts = Vec::new();
            if *rx != ShapeRadius::ClosestSide || *ry != ShapeRadius::ClosestSide {
                parts.push(radius_keyword(rx));
                parts.push(radius_keyword(ry));
            }
            parts.extend(position_text(position.as_ref()));
            format!("ellipse({})", parts.join(" "))
        }
        BasicShape::Polygon { rule, points } => {
            let points: Vec<String> = points
                .iter()
                .map(|(x, y)| format!("{} {}", lp(x), lp(y)))
                .collect();
            format!("polygon({}{})", rule_prefix(*rule), points.join(", "))
        }
        BasicShape::Path { rule, data } => {
            format!("path({}{})", rule_prefix(*rule), quoted(data))
        }
    }
}

fn rule_prefix(rule: FillRule) -> &'static str {
    match rule {
        FillRule::NonZero => "",
        FillRule::EvenOdd => "evenodd, ",
    }
}

fn quoted(s: &Arc<str>) -> String {
    let mut out = String::new();
    swb_css::serialize_string(s, &mut out);
    out
}

fn inset(i: &Inset) -> String {
    let sides = [&i.top, &i.right, &i.bottom, &i.left].map(lp);
    let mut text = format!("inset({}", short_list(&sides).join(" "));
    if let Some(radii) = &i.radii {
        text.push_str(" round ");
        text.push_str(&radii_text(radii));
    }
    text.push(')');
    text
}

/// `h0 h1 h2 h3 / v0 v1 v2 v3` with each list short, and without the
/// vertical list if it is the horizontal one.
fn radii_text(radii: &[CornerRadius; 4]) -> String {
    let h = short_list(&radii.each_ref().map(|r| lp(&r.horizontal))).join(" ");
    let v = short_list(&radii.each_ref().map(|r| lp(&r.vertical))).join(" ");
    if h == v { h } else { format!("{h} / {v}") }
}

fn radius_keyword(r: &ShapeRadius) -> String {
    match r {
        ShapeRadius::ClosestSide => "closest-side".to_owned(),
        ShapeRadius::FarthestSide => "farthest-side".to_owned(),
        ShapeRadius::Length(l) => lp(l),
    }
}

/// The radius of a circle; `closest-side` is left out.
fn radius_text(r: &ShapeRadius) -> Option<String> {
    (*r != ShapeRadius::ClosestSide).then(|| radius_keyword(r))
}

fn position_text(p: Option<&ShapePosition>) -> Option<String> {
    let p = p?;
    let offset = |c: &PositionComponent| lp(&c.offset);
    let text = if p.x.from_end || p.y.from_end {
        format!(
            "{} {} {} {}",
            if p.x.from_end { "right" } else { "left" },
            offset(&p.x),
            if p.y.from_end { "bottom" } else { "top" },
            offset(&p.y)
        )
    } else {
        format!("{} {}", offset(&p.x), offset(&p.y))
    };
    Some(format!("at {text}"))
}
