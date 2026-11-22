//! Paint properties and the classification of style changes (ADR 0044
//! section 9).
//!
//! A *paint property* changes only what paint draws: layout reads none of
//! its values (or reads them only through the checks of
//! [`paint_change`]). When a restyle changes only paint properties, the
//! layout stays and only the display list is built again. The table
//! [`PAINT`] lists them; the check mode of the engine compares each
//! paint-only frame with a full layout and removes a property from the
//! table when it finds a difference.
//!
//! [`paint_change`] decides for one element. A change is a paint change if
//!
//! - the old style with the paint properties of the new one equals the new
//!   style (every other field is the same, also those that the table does
//!   not list; only the custom properties may differ, no layout code reads
//!   them);
//! - the box is the containing block of fixed and absolute descendants as
//!   before for a transform ([`ComputedStyle::has_transform`]: a change
//!   between `none` and a value of `transform`, `translate`, `rotate` or
//!   `scale` is not a paint change);
//! - and the same for `filter` and `backdrop-filter`
//!   ([`ComputedStyle::is_filter_containing_block`]).

use std::sync::Arc;

use crate::ComputedStyle;
use crate::properties::LonghandId;

/// The paint properties (ADR 0044 section 9), by longhand.
///
/// The outline (no layout code reads it: it takes no space and adds no
/// scrollable overflow) and the decoration lines of text are in it; the
/// hover and focus rules of most pages change them.
///
/// Not in the table, although paint-only in principle: `visibility`
/// (hit testing and focus read it), `background-image` and the other
/// background layers (image loads), the border styles and radii (collapsed
/// borders and the native look of controls read them), the stroke width
/// and the opacities of SVG.
pub(crate) const PAINT: &[LonghandId] = &[
    LonghandId::Opacity,
    LonghandId::Transform,
    LonghandId::Translate,
    LonghandId::Rotate,
    LonghandId::Scale,
    LonghandId::TransformOrigin,
    LonghandId::Filter,
    LonghandId::BackdropFilter,
    LonghandId::Color,
    LonghandId::BackgroundColor,
    LonghandId::BorderTopColor,
    LonghandId::BorderRightColor,
    LonghandId::BorderBottomColor,
    LonghandId::BorderLeftColor,
    LonghandId::OutlineColor,
    LonghandId::OutlineStyle,
    LonghandId::OutlineWidth,
    LonghandId::OutlineOffset,
    LonghandId::TextDecorationColor,
    LonghandId::TextDecorationLine,
    LonghandId::TextDecorationStyle,
    LonghandId::Fill,
    LonghandId::Stroke,
    LonghandId::TextStrokeColor,
    LonghandId::TextFillColor,
    LonghandId::BoxShadow,
    LonghandId::TextShadow,
    LonghandId::BackgroundPositionX,
    LonghandId::BackgroundPositionY,
    LonghandId::BackgroundSize,
    LonghandId::ClipPath,
    LonghandId::MaskPositionX,
    LonghandId::MaskPositionY,
];

/// What a paint-only change of one style touches, for the refresh of the
/// fragment tree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PaintDelta {
    /// `transform`, `translate`, `rotate`, `scale` or `transform-origin`
    /// changed: the transformed bounds in the scrollable overflow and the
    /// geometry that scripts read change.
    pub geometry: bool,
    /// An inherited paint property (or a custom property) changed:
    /// anonymous boxes and text that derive their style from this one
    /// derive it again.
    pub inherited: bool,
    /// A color that the fragments of tables and controls bake in changed
    /// (`color`, a border color).
    pub colors: bool,
}

/// Copies every paint property (and the custom properties) of `from` to
/// `to`.
pub fn copy_paint_properties(from: &ComputedStyle, to: &mut ComputedStyle) {
    for &id in PAINT {
        id.copy_value(from, to);
    }
    to.custom_properties.clone_from(&from.custom_properties);
}

/// Copies the inherited paint properties (and the custom properties) of
/// `from`, the style of a parent box, to `to`, the style of an anonymous
/// box or of the text of a control, as `ComputedStyle::anonymous_from`
/// gave it.
pub fn copy_inherited_paint_properties(from: &ComputedStyle, to: &mut ComputedStyle) {
    for &id in PAINT {
        if id.is_inherited() {
            id.copy_value(from, to);
        }
    }
    to.custom_properties.clone_from(&from.custom_properties);
}

/// If `old` and `new` differ only in paint properties (and in no way that
/// the layout reads, see the module documentation), what the change
/// touches; `None` if the change can change the layout.
pub fn paint_change(old: &ComputedStyle, new: &ComputedStyle) -> Option<PaintDelta> {
    if old.has_transform() != new.has_transform()
        || old.is_filter_containing_block() != new.is_filter_containing_block()
    {
        return None;
    }
    let mut probe = old.clone();
    copy_paint_properties(new, &mut probe);
    if probe != *new {
        return None;
    }
    Some(PaintDelta {
        geometry: old.transform != new.transform
            || old.translate != new.translate
            || old.rotate != new.rotate
            || old.scale != new.scale
            || old.transform_origin != new.transform_origin,
        inherited: old.color != new.color
            || old.fill != new.fill
            || old.stroke != new.stroke
            || old.text_stroke_color != new.text_stroke_color
            || old.text_fill_color != new.text_fill_color
            || old.text_shadow != new.text_shadow
            || !same_custom_properties(old, new),
        colors: old.color != new.color
            || old.border_top_color != new.border_top_color
            || old.border_right_color != new.border_right_color
            || old.border_bottom_color != new.border_bottom_color
            || old.border_left_color != new.border_left_color,
    })
}

/// True if both styles have the same custom properties.
fn same_custom_properties(a: &ComputedStyle, b: &ComputedStyle) -> bool {
    match (&a.custom_properties, &b.custom_properties) {
        (None, None) => true,
        (Some(x), Some(y)) => Arc::ptr_eq(x, y) || **x == **y,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(f: impl FnOnce(&mut ComputedStyle)) -> ComputedStyle {
        let mut s = (*ComputedStyle::initial()).clone();
        f(&mut s);
        s
    }

    #[test]
    fn a_color_change_is_a_paint_change() {
        let old = style(|_| {});
        let new = style(|s| s.background_color = crate::Color::Rgba(crate::Rgba::rgb(1, 2, 3)));
        let delta = paint_change(&old, &new).expect("paint-only");
        assert!(!delta.geometry && !delta.inherited);
    }

    #[test]
    fn a_layout_property_is_not_a_paint_change() {
        let old = style(|_| {});
        let new = style(|s| s.padding_left = crate::LengthPercentage::Px(3.0));
        assert!(paint_change(&old, &new).is_none());
        let new = style(|s| {
            s.background_color = crate::Color::Rgba(crate::Rgba::rgb(1, 2, 3));
            s.display = crate::Display::Flex;
        });
        assert!(paint_change(&old, &new).is_none());
    }

    #[test]
    fn opacity_changes_are_paint_changes() {
        let old = style(|_| {});
        let new = style(|s| s.opacity = 0.5);
        assert!(paint_change(&old, &new).is_some());
    }

    #[test]
    fn the_color_is_inherited_and_a_color() {
        let old = style(|_| {});
        let new = style(|s| s.color = crate::Rgba::rgb(9, 9, 9));
        let delta = paint_change(&old, &new).expect("paint-only");
        assert!(delta.inherited && delta.colors);
    }
}
