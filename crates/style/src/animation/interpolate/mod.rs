//! Interpolation of computed values (CSS Values 4 §3,
//! <https://www.w3.org/TR/css-values-4/#combining-values>; ADR 0044
//! section 4): the value of one longhand between two computed styles at a
//! progress, with the computed-value rules of the property applied
//! afterwards (the clamps of `opacity` and of non-negative lengths, the
//! rounding of integers, the snapping of border widths).
//!
//! A pair that cannot be combined is discrete: it flips from the first to
//! the second value at progress 0.5. Progress outside 0 to 1 (from
//! `cubic-bezier()` or `linear()` overshoot) extrapolates; the clamps
//! apply after.
//!
//! The two styles are the keyframe values computed with the element's
//! context (3b): `from` and `to`. `out` is the element's style; its `color`
//! is already the animated one when another property is interpolated, so
//! `currentcolor` resolves to it (ADR 0044 section 4, `c-current`).

// The functions take the two values `a` and `b` and the progress `p`, as the
// specifications write them.
#![allow(clippy::many_single_char_names)]

mod color;
mod custom;
mod effects;
pub mod transform;

use std::sync::Arc;

use swb_css::ComponentValue;

use super::keyframes::{AnimatedProperty, PropertyKeyframes};
use crate::ComputedStyle;
use crate::properties::LonghandId;
use crate::values::{
    BackgroundSize, ComputedCalc, ContentVisibility, CornerRadius, Display, FlexBasis, Gap,
    LengthPercentage, LengthPercentageOrAuto, LineHeight, MaxSize, Overlay, PositionComponent,
    Size, SvgPaint, VerticalAlign, Visibility, ZIndex,
};

/// Sets the value of the longhand of `keyframes` in `out` to the value
/// between `from` and `to` at `progress`. Returns false for a custom
/// property: its value enters the custom property map before `var()` is
/// substituted, so 3b calls [`interpolate_custom`] there (ADR 0044
/// section 3).
pub fn interpolate(
    keyframes: &PropertyKeyframes,
    from: &ComputedStyle,
    to: &ComputedStyle,
    progress: f64,
    out: &mut ComputedStyle,
) -> bool {
    match &keyframes.property {
        AnimatedProperty::Longhand(id) => {
            interpolate_longhand(*id, from, to, progress, out);
            true
        }
        AnimatedProperty::Custom(_) => false,
    }
}

/// The value of the custom property `name` between the computed values
/// `from` and `to` at `progress` (`None`: the guaranteed-invalid value).
/// `custom` is the element's map, which knows the registrations; `current`
/// is the element's color (for `currentcolor` in a `<color>`). A
/// registered property interpolates by its syntax (`interp-custom-properties`:
/// `--len`, `--col`, `--list`, `--tl`, `--num`, `--ang`); an unregistered
/// one, and a pair that its syntax cannot combine, flip at 0.5 (`un`,
/// `--any`, `list2`).
pub fn interpolate_custom(
    custom: &crate::CustomProperties,
    name: &str,
    from: Option<&Arc<[ComponentValue]>>,
    to: Option<&Arc<[ComponentValue]>>,
    current: crate::values::Rgba,
    progress: f64,
) -> Option<Arc<[ComponentValue]>> {
    if let (Some(registration), Some(a), Some(b)) = (custom.registration(name), from, to)
        && let Some(value) = custom::interpolate(&registration.syntax, a, b, current, progress)
    {
        return Some(value);
    }
    if progress < 0.5 { from } else { to }.cloned()
}

/// True if a transition can interpolate the longhand `id` from `a` to `b`
/// (the pair is not discrete; CSS Transitions 1 §3 "transitionable" without
/// `allow-discrete`). `display`, `content-visibility` and `overlay` are
/// discrete for transitions also where an animation keeps the other value
/// of `none` (CSS Transitions 2 §3).
pub(crate) fn is_interpolable(id: LonghandId, a: &ComputedStyle, b: &ComputedStyle) -> bool {
    if matches!(
        id,
        LonghandId::Display | LonghandId::ContentVisibility | LonghandId::Overlay
    ) {
        return false;
    }
    let mut scratch = b.clone();
    interpolate_typed(id, a, b, 0.5, &mut scratch)
}

/// True if a transition can interpolate the custom property `name` from
/// `a` to `b`: it is registered and its syntax interpolates the pair.
pub(crate) fn custom_is_interpolable(
    custom: &crate::CustomProperties,
    name: &str,
    a: Option<&Arc<[ComponentValue]>>,
    b: Option<&Arc<[ComponentValue]>>,
    current: crate::values::Rgba,
) -> bool {
    match (custom.registration(name), a, b) {
        (Some(registration), Some(a), Some(b)) => {
            custom::interpolate(&registration.syntax, a, b, current, 0.5).is_some()
        }
        _ => false,
    }
}

/// Sets longhand `id` of `out` to its value between `from` and `to` at
/// `progress`.
pub(crate) fn interpolate_longhand(
    id: LonghandId,
    from: &ComputedStyle,
    to: &ComputedStyle,
    p: f64,
    out: &mut ComputedStyle,
) {
    if !interpolate_typed(id, from, to, p, out) {
        discrete(id, from, to, p, out);
    }
}

/// The discrete step: the first value below progress 0.5, else the second.
fn discrete(
    id: LonghandId,
    from: &ComputedStyle,
    to: &ComputedStyle,
    p: f64,
    out: &mut ComputedStyle,
) {
    id.copy_value(if p < 0.5 { from } else { to }, out);
}

/// Interpolates the properties that have a typed interpolation; false if
/// the property or the pair is discrete.
#[allow(clippy::too_many_lines)] // One arm per property.
fn interpolate_typed(
    id: LonghandId,
    a: &ComputedStyle,
    b: &ComputedStyle,
    p: f64,
    s: &mut ComputedStyle,
) -> bool {
    use LonghandId as L;
    let current = s.color;
    macro_rules! color {
        ($field:ident) => {{
            s.$field = color::mix(a.$field, b.$field, current, p);
            true
        }};
    }
    macro_rules! number {
        ($field:ident, $lo:expr, $hi:expr) => {{
            s.$field = lerp(a.$field, b.$field, p).clamp($lo, $hi);
            true
        }};
    }
    macro_rules! length {
        ($field:ident, $non_negative:expr) => {{
            s.$field = lp(&a.$field, &b.$field, p, $non_negative);
            true
        }};
    }
    macro_rules! option {
        ($field:ident, $f:expr) => {
            match $f(&a.$field, &b.$field, p) {
                Some(v) => {
                    s.$field = v;
                    true
                }
                None => false,
            }
        };
    }
    // The same for `Copy` values passed by value.
    macro_rules! copied {
        ($field:ident, $f:expr) => {
            match $f(a.$field, b.$field, p) {
                Some(v) => {
                    s.$field = v;
                    true
                }
                None => false,
            }
        };
    }
    match id {
        L::Color => {
            s.color = color::mix_color_property(a, b, p);
            s.color_exact = None;
            true
        }
        L::TapHighlightColor => color!(tap_highlight_color),
        L::TextStrokeColor => color!(text_stroke_color),
        L::TextFillColor => color!(text_fill_color),
        L::BorderTopColor => color!(border_top_color),
        L::BorderRightColor => color!(border_right_color),
        L::BorderBottomColor => color!(border_bottom_color),
        L::BorderLeftColor => color!(border_left_color),
        L::OutlineColor => color!(outline_color),
        L::BackgroundColor => color!(background_color),
        L::TextDecorationColor => color!(text_decoration_color),
        L::Fill => option!(fill, |x, y, p| paint(x, y, current, p)),
        L::Stroke => option!(stroke, |x, y, p| paint(x, y, current, p)),
        L::FontSize => number!(font_size, 0.0, f32::MAX),
        L::FontWeight => number!(font_weight, 1.0, 1000.0),
        L::FontStretch => number!(font_stretch, 0.0, f32::MAX),
        L::Opacity => number!(opacity, 0.0, 1.0),
        L::FillOpacity => number!(fill_opacity, 0.0, 1.0),
        L::StrokeOpacity => number!(stroke_opacity, 0.0, 1.0),
        L::FlexGrow => number!(flex_grow, 0.0, f32::MAX),
        L::FlexShrink => number!(flex_shrink, 0.0, f32::MAX),
        L::StrokeMiterlimit => number!(stroke_miterlimit, 1.0, f32::MAX),
        L::TabSize => number!(tab_size, 0.0, f32::MAX),
        L::LetterSpacing => number!(letter_spacing, f32::MIN, f32::MAX),
        L::WordSpacing => number!(word_spacing, f32::MIN, f32::MAX),
        L::TextStrokeWidth => number!(text_stroke_width, 0.0, f32::MAX),
        L::BorderSpacingHorizontal => number!(border_spacing_horizontal, 0.0, f32::MAX),
        L::BorderSpacingVertical => number!(border_spacing_vertical, 0.0, f32::MAX),
        L::OutlineOffset => {
            s.outline_offset = crate::properties::compute::snap_outline_offset(lerp(
                a.outline_offset,
                b.outline_offset,
                p,
            ));
            true
        }
        L::BorderTopWidth => border_width(
            &mut s.border_top_width,
            a.border_top_width,
            b.border_top_width,
            p,
        ),
        L::BorderRightWidth => border_width(
            &mut s.border_right_width,
            a.border_right_width,
            b.border_right_width,
            p,
        ),
        L::BorderBottomWidth => border_width(
            &mut s.border_bottom_width,
            a.border_bottom_width,
            b.border_bottom_width,
            p,
        ),
        L::BorderLeftWidth => border_width(
            &mut s.border_left_width,
            a.border_left_width,
            b.border_left_width,
            p,
        ),
        L::OutlineWidth => border_width(&mut s.outline_width, a.outline_width, b.outline_width, p),
        L::TextIndent => length!(text_indent, false),
        L::PaddingTop => length!(padding_top, true),
        L::PaddingRight => length!(padding_right, true),
        L::PaddingBottom => length!(padding_bottom, true),
        L::PaddingLeft => length!(padding_left, true),
        L::StrokeWidth => length!(stroke_width, true),
        L::StrokeDashoffset => length!(stroke_dashoffset, false),
        L::Top => option!(top, |x, y, p| lp_or_auto(x, y, p, false)),
        L::Right => option!(right, |x, y, p| lp_or_auto(x, y, p, false)),
        L::Bottom => option!(bottom, |x, y, p| lp_or_auto(x, y, p, false)),
        L::Left => option!(left, |x, y, p| lp_or_auto(x, y, p, false)),
        L::MarginTop => option!(margin_top, |x, y, p| lp_or_auto(x, y, p, false)),
        L::MarginRight => option!(margin_right, |x, y, p| lp_or_auto(x, y, p, false)),
        L::MarginBottom => option!(margin_bottom, |x, y, p| lp_or_auto(x, y, p, false)),
        L::MarginLeft => option!(margin_left, |x, y, p| lp_or_auto(x, y, p, false)),
        L::Width => option!(width, size),
        L::Height => option!(height, size),
        L::MinWidth => option!(min_width, size),
        L::MinHeight => option!(min_height, size),
        L::MaxWidth => option!(max_width, max_size),
        L::MaxHeight => option!(max_height, max_size),
        L::LineHeight => copied!(line_height, line_height),
        L::ZIndex => copied!(z_index, z_index),
        L::Order => {
            s.order = round_integer(lerp(a.order as f32, b.order as f32, p));
            true
        }
        L::ColumnCount => option!(column_count, |x: &Option<u32>, y: &Option<u32>, p| {
            match (x, y) {
                (Some(x), Some(y)) => Some(Some(
                    round_integer(lerp(*x as f32, *y as f32, p))
                        .max(1)
                        .unsigned_abs(),
                )),
                _ => None,
            }
        }),
        L::ColumnWidth => option!(column_width, |x: &Option<f32>, y: &Option<f32>, p| {
            match (x, y) {
                (Some(x), Some(y)) => Some(Some(lerp(*x, *y, p).max(0.0))),
                _ => None,
            }
        }),
        L::RowGap => option!(row_gap, gap),
        L::ColumnGap => option!(column_gap, gap),
        L::FlexBasis => option!(flex_basis, |x: &FlexBasis, y: &FlexBasis, p| match (x, y) {
            (FlexBasis::Size(x), FlexBasis::Size(y)) => size(x, y, p).map(FlexBasis::Size),
            _ => None,
        }),
        L::VerticalAlign => option!(vertical_align, |x: &VerticalAlign, y: &VerticalAlign, p| {
            match (x, y) {
                (VerticalAlign::LengthPercentage(x), VerticalAlign::LengthPercentage(y)) => {
                    Some(VerticalAlign::LengthPercentage(lp(x, y, p, false)))
                }
                _ => None,
            }
        }),
        L::BorderTopLeftRadius => {
            s.border_top_left_radius =
                radius(&a.border_top_left_radius, &b.border_top_left_radius, p);
            true
        }
        L::BorderTopRightRadius => {
            s.border_top_right_radius =
                radius(&a.border_top_right_radius, &b.border_top_right_radius, p);
            true
        }
        L::BorderBottomRightRadius => {
            s.border_bottom_right_radius = radius(
                &a.border_bottom_right_radius,
                &b.border_bottom_right_radius,
                p,
            );
            true
        }
        L::BorderBottomLeftRadius => {
            s.border_bottom_left_radius = radius(
                &a.border_bottom_left_radius,
                &b.border_bottom_left_radius,
                p,
            );
            true
        }
        L::TransformOrigin => {
            let x = lp(&a.transform_origin.x, &b.transform_origin.x, p, false);
            let y = lp(&a.transform_origin.y, &b.transform_origin.y, p, false);
            s.transform_origin = crate::values::TransformOrigin { x, y };
            true
        }
        L::ObjectPosition => {
            let x = position(&a.object_position[0], &b.object_position[0], p);
            let y = position(&a.object_position[1], &b.object_position[1], p);
            s.object_position = [x, y];
            true
        }
        L::BackgroundPositionX => option!(background_position_x, positions),
        L::BackgroundPositionY => option!(background_position_y, positions),
        L::MaskPositionX => option!(mask_position_x, positions),
        L::MaskPositionY => option!(mask_position_y, positions),
        L::BackgroundSize => option!(background_size, sizes),
        L::MaskSize => option!(mask_size, sizes),
        L::Visibility => {
            s.visibility = visibility(a.visibility, b.visibility, p);
            true
        }
        L::Display => {
            let none = |d: Display| d == Display::None;
            if none(a.display) == none(b.display) {
                false
            } else {
                s.display = not_none(a.display, b.display, none, p);
                true
            }
        }
        L::ContentVisibility => {
            let hidden = |v: ContentVisibility| v == ContentVisibility::Hidden;
            if hidden(a.content_visibility) == hidden(b.content_visibility) {
                false
            } else {
                s.content_visibility =
                    not_none(a.content_visibility, b.content_visibility, hidden, p);
                true
            }
        }
        L::Overlay => {
            let none = |v: Overlay| v == Overlay::None;
            if none(a.overlay) == none(b.overlay) {
                false
            } else {
                s.overlay = not_none(a.overlay, b.overlay, none, p);
                true
            }
        }
        L::Transform => option!(transform, transform::interpolate_lists),
        // `none` to `none` stays `none` for the individual transforms.
        L::Translate => {
            s.translate = (a.translate.is_some() || b.translate.is_some())
                .then(|| effects::translate(a.translate.as_ref(), b.translate.as_ref(), p));
            true
        }
        L::Rotate if a.rotate.is_none() && b.rotate.is_none() => {
            s.rotate = None;
            true
        }
        L::Rotate => match effects::rotate(a.rotate, b.rotate, p) {
            Some(r) => {
                s.rotate = Some(r);
                true
            }
            None => false,
        },
        L::Scale => {
            s.scale = (a.scale.is_some() || b.scale.is_some())
                .then(|| effects::scale(a.scale, b.scale, p));
            true
        }
        L::Filter => option!(filter, |x, y, p| effects::filter(x, y, current, p)),
        L::BackdropFilter => option!(backdrop_filter, |x, y, p| effects::filter(x, y, current, p)),
        L::BoxShadow => option!(box_shadow, |x, y, p| effects::box_shadows(x, y, current, p)),
        L::TextShadow => option!(text_shadow, |x, y, p| effects::text_shadows(
            x, y, current, p
        )),
        _ => false,
    }
}

// ----- Numbers -----

/// `a + (b − a) × p` in `f64`, as a finite `f32`.
pub(crate) fn lerp(a: f32, b: f32, p: f64) -> f32 {
    let (a, b) = (f64::from(a), f64::from(b));
    to_f32(a + (b - a) * p)
}

/// `v` as a finite `f32`: saturated at the largest `f32`, NaN as 0. A
/// progress far outside 0 to 1 (`linear(1e300 0%, 0 100%)`) must not make
/// an infinite length or matrix.
pub(crate) fn to_f32(v: f64) -> f32 {
    if v.is_nan() {
        0.0
    } else {
        v.clamp(-f64::from(f32::MAX), f64::from(f32::MAX)) as f32
    }
}

/// A percentage as a fraction (`0.5` is 50%), saturated at 1e36 so that
/// the percentage (the fraction × 100 in `f32`) is finite too.
fn to_fraction(v: f64) -> f32 {
    to_f32(v.clamp(-1e36, 1e36))
}

/// An interpolated integer: rounded half toward +∞ (5.5 is 6, −5.5 is −5;
/// CSS Values 4 §3.1, `fw`, `zi`).
fn round_integer(v: f32) -> i32 {
    let rounded = (f64::from(v) + 0.5).floor();
    rounded.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

/// A border or outline width: not negative, snapped as at compute time.
fn border_width(out: &mut f32, a: f32, b: f32, p: f64) -> bool {
    *out = crate::properties::compute::snap_border_width(lerp(a, b, p).max(0.0));
    true
}

fn z_index(a: ZIndex, b: ZIndex, p: f64) -> Option<ZIndex> {
    match (a, b) {
        (ZIndex::Integer(x), ZIndex::Integer(y)) => {
            Some(ZIndex::Integer(round_integer(lerp(x as f32, y as f32, p))))
        }
        _ => None,
    }
}

fn line_height(a: LineHeight, b: LineHeight, p: f64) -> Option<LineHeight> {
    match (a, b) {
        (LineHeight::Number(x), LineHeight::Number(y)) => {
            Some(LineHeight::Number(lerp(x, y, p).max(0.0)))
        }
        (LineHeight::Px(x), LineHeight::Px(y)) => Some(LineHeight::Px(lerp(x, y, p).max(0.0))),
        _ => None,
    }
}

// ----- Lengths and percentages -----

/// The px and percentage parts of a length-percentage whose expression is
/// linear (no `min()`, `max()`, `clamp()`).
fn linear_parts(v: &LengthPercentage) -> Option<(f64, f64)> {
    match v {
        LengthPercentage::Px(px) => Some((f64::from(*px), 0.0)),
        LengthPercentage::Percent(pct) => Some((0.0, f64::from(*pct))),
        LengthPercentage::Calc(c) => calc_parts(c),
    }
}

fn calc_parts(c: &ComputedCalc) -> Option<(f64, f64)> {
    match c {
        ComputedCalc::Px(v) => Some((f64::from(*v), 0.0)),
        ComputedCalc::Percent(v) => Some((0.0, f64::from(*v))),
        ComputedCalc::Sum(items) => items.iter().try_fold((0.0, 0.0), |(px, pct), item| {
            let (a, b) = calc_parts(item)?;
            Some((px + a, pct + b))
        }),
        ComputedCalc::Product(x, y) => match (x.as_ref(), y.as_ref()) {
            (ComputedCalc::Number(n), other) | (other, ComputedCalc::Number(n)) => {
                let (px, pct) = calc_parts(other)?;
                Some((px * f64::from(*n), pct * f64::from(*n)))
            }
            _ => None,
        },
        ComputedCalc::Quotient(x, y) => match y.as_ref() {
            ComputedCalc::Number(n) if *n != 0.0 => {
                let (px, pct) = calc_parts(x)?;
                Some((px / f64::from(*n), pct / f64::from(*n)))
            }
            _ => None,
        },
        _ => None,
    }
}

/// The interpolated length-percentage: a length if both are lengths, a
/// percentage if both are percentages, else `calc(a% + bpx)` (ADR 0044
/// section 4). A non-negative property clamps the result: a length or
/// percentage at 0, a mix with `max(0px, …)`. A value with `min()` or
/// `max()` interpolates as the weighted sum of the two expressions.
fn lp(a: &LengthPercentage, b: &LengthPercentage, p: f64, non_negative: bool) -> LengthPercentage {
    let has_pct = a.has_percentage() || b.has_percentage();
    let has_px =
        !matches!(a, LengthPercentage::Percent(_)) || !matches!(b, LengthPercentage::Percent(_));
    let value = if let (Some((pa, ca)), Some((pb, cb))) = (linear_parts(a), linear_parts(b)) {
        let px = to_f32(pa + (pb - pa) * p);
        let pct = to_fraction(ca + (cb - ca) * p);
        if !has_pct {
            LengthPercentage::Px(px)
        } else if has_px {
            LengthPercentage::Calc(Arc::new(ComputedCalc::Sum(vec![
                ComputedCalc::Percent(pct),
                ComputedCalc::Px(px),
            ])))
        } else {
            LengthPercentage::Percent(pct)
        }
    } else {
        let weight = |w: f64, v: &LengthPercentage| {
            ComputedCalc::Product(
                Box::new(ComputedCalc::Number(to_f32(w))),
                Box::new(calc_of(v)),
            )
        };
        LengthPercentage::Calc(Arc::new(ComputedCalc::Sum(vec![
            weight(1.0 - p, a),
            weight(p, b),
        ])))
    };
    if non_negative {
        clamp_non_negative(value)
    } else {
        value
    }
}

/// A length-percentage as a calc expression.
fn calc_of(v: &LengthPercentage) -> ComputedCalc {
    match v {
        LengthPercentage::Px(px) => ComputedCalc::Px(*px),
        LengthPercentage::Percent(pct) => ComputedCalc::Percent(*pct),
        LengthPercentage::Calc(c) => (**c).clone(),
    }
}

/// The clamp of a non-negative length-percentage.
fn clamp_non_negative(v: LengthPercentage) -> LengthPercentage {
    match v {
        LengthPercentage::Px(px) => LengthPercentage::Px(px.max(0.0)),
        LengthPercentage::Percent(pct) => LengthPercentage::Percent(pct.max(0.0)),
        LengthPercentage::Calc(c) => LengthPercentage::Calc(Arc::new(ComputedCalc::Max(vec![
            ComputedCalc::Px(0.0),
            (*c).clone(),
        ]))),
    }
}

fn lp_or_auto(
    a: &LengthPercentageOrAuto,
    b: &LengthPercentageOrAuto,
    p: f64,
    non_negative: bool,
) -> Option<LengthPercentageOrAuto> {
    match (a, b) {
        (
            LengthPercentageOrAuto::LengthPercentage(x),
            LengthPercentageOrAuto::LengthPercentage(y),
        ) => Some(LengthPercentageOrAuto::LengthPercentage(lp(
            x,
            y,
            p,
            non_negative,
        ))),
        _ => None,
    }
}

fn size(a: &Size, b: &Size, p: f64) -> Option<Size> {
    match (a, b) {
        (Size::LengthPercentage(x), Size::LengthPercentage(y)) => {
            Some(Size::LengthPercentage(lp(x, y, p, true)))
        }
        _ => None,
    }
}

fn max_size(a: &MaxSize, b: &MaxSize, p: f64) -> Option<MaxSize> {
    match (a, b) {
        (MaxSize::LengthPercentage(x), MaxSize::LengthPercentage(y)) => {
            Some(MaxSize::LengthPercentage(lp(x, y, p, true)))
        }
        _ => None,
    }
}

fn gap(a: &Gap, b: &Gap, p: f64) -> Option<Gap> {
    match (a, b) {
        (Gap::LengthPercentage(x), Gap::LengthPercentage(y)) => {
            Some(Gap::LengthPercentage(lp(x, y, p, true)))
        }
        _ => None,
    }
}

fn radius(a: &CornerRadius, b: &CornerRadius, p: f64) -> CornerRadius {
    CornerRadius {
        horizontal: lp(&a.horizontal, &b.horizontal, p, true),
        vertical: lp(&a.vertical, &b.vertical, p, true),
    }
}

/// A position component with the offset from the start edge: `right 10px`
/// is `calc(100% - 10px)`.
fn from_start(v: &PositionComponent) -> LengthPercentage {
    if !v.from_end {
        return v.offset.clone();
    }
    match linear_parts(&v.offset) {
        Some((0.0, pct)) => LengthPercentage::Percent(to_fraction(1.0 - pct)),
        Some((px, pct)) => LengthPercentage::Calc(Arc::new(ComputedCalc::Sum(vec![
            ComputedCalc::Percent(to_fraction(1.0 - pct)),
            ComputedCalc::Px(to_f32(-px)),
        ]))),
        None => LengthPercentage::Calc(Arc::new(ComputedCalc::Sum(vec![
            ComputedCalc::Percent(1.0),
            ComputedCalc::Product(
                Box::new(ComputedCalc::Number(-1.0)),
                Box::new(calc_of(&v.offset)),
            ),
        ]))),
    }
}

fn position(a: &PositionComponent, b: &PositionComponent, p: f64) -> PositionComponent {
    if a.from_end == b.from_end {
        return PositionComponent {
            offset: lp(&a.offset, &b.offset, p, false),
            from_end: a.from_end,
        };
    }
    PositionComponent {
        offset: lp(&from_start(a), &from_start(b), p, false),
        from_end: false,
    }
}

/// The greatest common divisor, for the least common multiple of two list
/// lengths.
fn gcd(a: usize, b: usize) -> usize {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// The longest list that a repeated-list interpolation makes (two lists
/// of different lengths repeat to their least common multiple, CSS Values
/// 4 §3.3); longer pairs are discrete, so that a hostile pair of lists of
/// 9,973 and 9,967 layers does not make 10^8 entries.
const MAX_REPEATED_LIST: usize = 1024;

/// Interpolates two lists item by item, repeated to the least common
/// multiple of their lengths; `None` if an item pair is discrete or the
/// result would be too long.
fn repeated<T>(a: &[T], b: &[T], f: impl Fn(&T, &T) -> Option<T>) -> Option<Arc<[T]>> {
    if a.is_empty() || b.is_empty() {
        return None;
    }
    let lcm = a.len() / gcd(a.len(), b.len()) * b.len();
    if lcm > MAX_REPEATED_LIST.max(a.len()).max(b.len()) {
        return None;
    }
    (0..lcm)
        .map(|i| f(a.get(i % a.len())?, b.get(i % b.len())?))
        .collect()
}

fn positions(
    a: &Arc<[PositionComponent]>,
    b: &Arc<[PositionComponent]>,
    p: f64,
) -> Option<Arc<[PositionComponent]>> {
    repeated(a, b, |x, y| Some(position(x, y, p)))
}

fn sizes(
    a: &Arc<[BackgroundSize]>,
    b: &Arc<[BackgroundSize]>,
    p: f64,
) -> Option<Arc<[BackgroundSize]>> {
    repeated(a, b, |x, y| background_size(x, y, p))
}

/// One layer of `background-size`: per component as lengths; `auto`
/// matches only `auto`; `cover` and `contain` only themselves.
fn background_size(a: &BackgroundSize, b: &BackgroundSize, p: f64) -> Option<BackgroundSize> {
    use BackgroundSize as B;
    let explicit = |v: &BackgroundSize| match v {
        B::Explicit(w, h) => Some((w.clone(), h.clone())),
        B::Auto => Some((LengthPercentageOrAuto::Auto, LengthPercentageOrAuto::Auto)),
        _ => None,
    };
    match (explicit(a), explicit(b)) {
        (Some((wa, ha)), Some((wb, hb))) => {
            let component = |x: &LengthPercentageOrAuto, y: &LengthPercentageOrAuto| match (x, y) {
                (LengthPercentageOrAuto::Auto, LengthPercentageOrAuto::Auto) => {
                    Some(LengthPercentageOrAuto::Auto)
                }
                _ => lp_or_auto(x, y, p, true),
            };
            let w = component(&wa, &wb)?;
            let h = component(&ha, &hb)?;
            Some(if w.is_auto() && h.is_auto() {
                B::Auto
            } else {
                B::Explicit(w, h)
            })
        }
        _ => (a == b).then(|| a.clone()),
    }
}

// ----- Colors and paints -----

fn paint(a: &SvgPaint, b: &SvgPaint, current: crate::values::Rgba, p: f64) -> Option<SvgPaint> {
    match (a, b) {
        (SvgPaint::Color(x), SvgPaint::Color(y)) => {
            Some(SvgPaint::Color(color::mix(*x, *y, current, p)))
        }
        _ => None,
    }
}

// ----- Keywords -----

/// `visibility`: with `visible` at one end, `visible` for every progress
/// strictly between 0 and 1; else discrete (CSS Values 4 §3,
/// `vi`, `vi2`).
fn visibility(a: Visibility, b: Visibility, p: f64) -> Visibility {
    let visible = Visibility::Visible;
    if a != b && (a == visible || b == visible) {
        if p <= 0.0 {
            a
        } else if p >= 1.0 {
            b
        } else {
            visible
        }
    } else if p < 0.5 {
        a
    } else {
        b
    }
}

/// `display`, `content-visibility` and `overlay` with `none` (`hidden`)
/// at one end: the other value strictly between 0 and 1 (CSS Display 4,
/// <https://drafts.csswg.org/css-display-4/#display-animation>; `ds`, `cv`).
fn not_none<T: Copy>(a: T, b: T, is_none: impl Fn(T) -> bool, p: f64) -> T {
    if p >= 1.0 || (p > 0.0 && is_none(a)) {
        b
    } else {
        a
    }
}

#[cfg(test)]
mod tests;
