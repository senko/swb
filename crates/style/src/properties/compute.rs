//! Computing specified values: [`apply`] stores the computed value of one
//! longhand declaration in a [`ComputedStyle`].
//!
//! <https://www.w3.org/TR/css-cascade-4/#computed>

use std::sync::Arc;

use swb_dom::ElementData;

use super::ids::LonghandValue;
use super::specified::clamp_non_negative;
use super::transform;
use crate::ComputedStyle;
use crate::values::{
    CornerRadius, FontFamily, Gap, GenericFamily, LengthContext, LengthPercentage,
    LengthPercentageOrAuto, SpecifiedLengthPercentage as Lp, SpecifiedTrackSize, TrackSize,
};

/// What computing a value depends on.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ComputeContext<'a> {
    /// The parent's computed style (the initial style for the root).
    pub(crate) parent: &'a ComputedStyle,
    /// Font sizes and viewport for relative lengths. While `font-size` is
    /// computed, `font_size` is the parent's font size; afterwards it is
    /// the element's.
    pub(crate) lengths: LengthContext,
    /// The element, for `attr()`. `None` in tests.
    pub(crate) element: Option<&'a ElementData>,
    /// True in quirks mode.
    pub(crate) quirks: bool,
}

impl ComputeContext<'_> {
    /// Computes a length-percentage.
    pub(crate) fn lp(&self, value: &Lp) -> LengthPercentage {
        value.compute(&self.lengths)
    }

    /// Computes a length-percentage that cannot be negative.
    pub(crate) fn non_negative(&self, value: &Lp) -> LengthPercentage {
        clamp_non_negative(value.compute(&self.lengths))
    }

    /// Computes a length (no percentages) to px.
    pub(crate) fn px(&self, value: &Lp) -> f32 {
        value.compute(&self.lengths).resolve(0.0)
    }

    fn lp_or_auto(&self, value: Option<&Lp>) -> LengthPercentageOrAuto {
        match value {
            None => LengthPercentageOrAuto::Auto,
            Some(lp) => LengthPercentageOrAuto::LengthPercentage(self.lp(lp)),
        }
    }
}

/// True if the font family list is exactly the generic `monospace`. This
/// selects the 13px default font size (Blink's `IsMonospace()`).
pub(crate) fn is_monospace(families: &[FontFamily]) -> bool {
    matches!(families, [FontFamily::Generic(GenericFamily::Monospace)])
}

/// Snaps a border or outline width as Chromium does at a device pixel ratio
/// of 1: widths between 0 and 1 become 1, larger widths are floored.
fn snap_border_width(px: f32) -> f32 {
    if px <= 0.0 || !px.is_finite() {
        0.0
    } else if px < 1.0 {
        1.0
    } else {
        px.floor()
    }
}

/// Stores the computed value of `value` in `s`.
#[allow(clippy::too_many_lines)] // One arm per longhand.
pub(crate) fn apply(value: &LonghandValue, cx: &ComputeContext<'_>, s: &mut ComputedStyle) {
    use LonghandValue as V;
    match value {
        // `color: currentColor` is `color: inherit`.
        V::Color(c) => s.color = c.resolve(cx.parent.color),
        V::FontFamily(f) => s.font_family = Arc::clone(f),
        V::FontSize(v) => {
            let (size, origin) = v.compute(cx, is_monospace(&s.font_family));
            s.font_size = size;
            s.font_size_origin = origin;
        }
        V::FontWeight(v) => s.font_weight = v.compute(cx.parent.font_weight),
        V::FontStyle(v) => s.font_style = *v,
        V::FontStretch(v) => s.font_stretch = *v,
        V::FontVariantCaps(v) => s.font_variant_caps = *v,
        V::LineHeight(v) => s.line_height = v.compute(cx),
        V::TextAlign(v) => s.text_align = v.compute(cx),
        V::TextIndent(v) => s.text_indent = cx.lp(v),
        V::TextTransform(v) => s.text_transform = *v,
        V::WhiteSpace(v) => s.white_space = *v,
        V::LetterSpacing(v) => s.letter_spacing = cx.lp(v).resolve(cx.lengths.font_size),
        V::WordSpacing(v) => s.word_spacing = cx.px(v),
        V::WordBreak(v) => s.word_break = *v,
        V::OverflowWrap(v) => s.overflow_wrap = *v,
        V::Hyphens(v) => s.hyphens = *v,
        V::TabSize(v) => s.tab_size = *v,
        V::Visibility(v) => s.visibility = *v,
        V::ListStyleType(v) => s.list_style_type = *v,
        V::ListStylePosition(v) => s.list_style_position = *v,
        V::ListStyleImage(v) => {
            s.list_style_image = v.as_ref().map(|i| i.compute(&cx.lengths));
        }
        V::Cursor(v) => s.cursor = *v,
        V::Direction(v) => s.direction = *v,
        V::BorderCollapse(v) => s.border_collapse = *v,
        V::BorderSpacingHorizontal(v) => s.border_spacing_horizontal = cx.px(v).max(0.0),
        V::BorderSpacingVertical(v) => s.border_spacing_vertical = cx.px(v).max(0.0),
        V::CaptionSide(v) => s.caption_side = *v,
        V::EmptyCells(v) => s.empty_cells = *v,
        V::PointerEvents(v) => s.pointer_events = *v,
        V::Display(v) => s.display = *v,
        V::Position(v) => s.position = *v,
        V::Float(v) => s.float = *v,
        V::Clear(v) => s.clear = *v,
        V::Top(v) => s.top = cx.lp_or_auto(v.as_ref()),
        V::Right(v) => s.right = cx.lp_or_auto(v.as_ref()),
        V::Bottom(v) => s.bottom = cx.lp_or_auto(v.as_ref()),
        V::Left(v) => s.left = cx.lp_or_auto(v.as_ref()),
        V::ZIndex(v) => s.z_index = *v,
        V::Transform(v) => s.transform = transform::compute_transform(v, cx),
        V::TransformOrigin(v) => s.transform_origin = transform::compute_transform_origin(v, cx),
        V::Clip(v) => s.clip = transform::compute_clip(v, cx),
        V::Width(v) => s.width = v.compute_size(cx),
        V::Height(v) => s.height = v.compute_size(cx),
        V::MinWidth(v) => s.min_width = v.compute_size(cx),
        V::MinHeight(v) => s.min_height = v.compute_size(cx),
        V::MaxWidth(v) => s.max_width = v.compute_max_size(cx),
        V::MaxHeight(v) => s.max_height = v.compute_max_size(cx),
        V::BoxSizing(v) => s.box_sizing = *v,
        V::AspectRatio(v) => s.aspect_ratio = *v,
        V::MarginTop(v) => s.margin_top = cx.lp_or_auto(v.as_ref()),
        V::MarginRight(v) => s.margin_right = cx.lp_or_auto(v.as_ref()),
        V::MarginBottom(v) => s.margin_bottom = cx.lp_or_auto(v.as_ref()),
        V::MarginLeft(v) => s.margin_left = cx.lp_or_auto(v.as_ref()),
        V::PaddingTop(v) => s.padding_top = cx.non_negative(v),
        V::PaddingRight(v) => s.padding_right = cx.non_negative(v),
        V::PaddingBottom(v) => s.padding_bottom = cx.non_negative(v),
        V::PaddingLeft(v) => s.padding_left = cx.non_negative(v),
        V::BorderTopWidth(v) => s.border_top_width = snap_border_width(cx.px(v)),
        V::BorderRightWidth(v) => s.border_right_width = snap_border_width(cx.px(v)),
        V::BorderBottomWidth(v) => s.border_bottom_width = snap_border_width(cx.px(v)),
        V::BorderLeftWidth(v) => s.border_left_width = snap_border_width(cx.px(v)),
        V::BorderTopStyle(v) => s.border_top_style = *v,
        V::BorderRightStyle(v) => s.border_right_style = *v,
        V::BorderBottomStyle(v) => s.border_bottom_style = *v,
        V::BorderLeftStyle(v) => s.border_left_style = *v,
        V::BorderTopColor(v) => s.border_top_color = *v,
        V::BorderRightColor(v) => s.border_right_color = *v,
        V::BorderBottomColor(v) => s.border_bottom_color = *v,
        V::BorderLeftColor(v) => s.border_left_color = *v,
        V::BorderTopLeftRadius(v) => s.border_top_left_radius = radius(cx, v),
        V::BorderTopRightRadius(v) => s.border_top_right_radius = radius(cx, v),
        V::BorderBottomRightRadius(v) => s.border_bottom_right_radius = radius(cx, v),
        V::BorderBottomLeftRadius(v) => s.border_bottom_left_radius = radius(cx, v),
        V::OutlineWidth(v) => s.outline_width = snap_border_width(cx.px(v)),
        V::OutlineStyle(v) => s.outline_style = *v,
        V::OutlineColor(v) => s.outline_color = *v,
        V::OutlineOffset(v) => s.outline_offset = cx.px(v),
        V::BackgroundColor(v) => s.background_color = *v,
        V::BackgroundImage(v) => {
            s.background_image = v
                .iter()
                .map(|i| i.as_ref().map(|i| i.compute(&cx.lengths)))
                .collect();
        }
        V::BackgroundPositionX(v) => {
            s.background_position_x = v.iter().map(|p| p.compute(cx)).collect();
        }
        V::BackgroundPositionY(v) => {
            s.background_position_y = v.iter().map(|p| p.compute(cx)).collect();
        }
        V::BackgroundSize(v) => s.background_size = v.iter().map(|b| b.compute(cx)).collect(),
        V::BackgroundRepeat(v) => s.background_repeat = Arc::clone(v),
        V::BackgroundOrigin(v) => s.background_origin = Arc::clone(v),
        V::BackgroundClip(v) => s.background_clip = Arc::clone(v),
        V::BackgroundAttachment(v) => s.background_attachment = Arc::clone(v),
        V::MaskImage(_)
        | V::MaskMode(_)
        | V::MaskPositionX(_)
        | V::MaskPositionY(_)
        | V::MaskSize(_)
        | V::MaskRepeat(_)
        | V::MaskOrigin(_)
        | V::MaskClip(_)
        | V::MaskComposite(_) => super::mask::apply(value, cx, s),
        V::OverflowX(v) => s.overflow_x = *v,
        V::OverflowY(v) => s.overflow_y = *v,
        V::TextOverflow(v) => s.text_overflow = *v,
        V::Opacity(v) => s.opacity = v.clamp(0.0, 1.0),
        V::VerticalAlign(v) => s.vertical_align = v.compute(cx),
        V::TextDecorationLine(v) => s.text_decoration_line = *v,
        V::TextDecorationColor(v) => s.text_decoration_color = *v,
        V::TextDecorationStyle(v) => s.text_decoration_style = *v,
        V::FlexDirection(v) => s.flex_direction = *v,
        V::FlexWrap(v) => s.flex_wrap = *v,
        V::FlexGrow(v) => s.flex_grow = *v,
        V::FlexShrink(v) => s.flex_shrink = *v,
        V::FlexBasis(v) => s.flex_basis = v.compute(cx),
        V::Order(v) => s.order = *v,
        V::JustifyContent(v) => s.justify_content = *v,
        V::AlignItems(v) => s.align_items = *v,
        V::AlignSelf(v) => s.align_self = *v,
        V::AlignContent(v) => s.align_content = *v,
        V::RowGap(v) => s.row_gap = gap(cx, v.as_ref()),
        V::ColumnGap(v) => s.column_gap = gap(cx, v.as_ref()),
        V::JustifyItems(v) => s.justify_items = *v,
        V::JustifySelf(v) => s.justify_self = *v,
        V::GridTemplateColumns(v) => s.grid_template_columns = v.map(&|l| cx.non_negative(l)),
        V::GridTemplateRows(v) => s.grid_template_rows = v.map(&|l| cx.non_negative(l)),
        V::GridTemplateAreas(v) => s.grid_template_areas.clone_from(v),
        V::GridAutoColumns(v) => s.grid_auto_columns = track_sizes(cx, v),
        V::GridAutoRows(v) => s.grid_auto_rows = track_sizes(cx, v),
        V::GridAutoFlow(v) => s.grid_auto_flow = *v,
        V::GridRowStart(v) => s.grid_row_start.clone_from(v),
        V::GridRowEnd(v) => s.grid_row_end.clone_from(v),
        V::GridColumnStart(v) => s.grid_column_start.clone_from(v),
        V::GridColumnEnd(v) => s.grid_column_end.clone_from(v),
        V::TableLayout(v) => s.table_layout = *v,
        V::Content(v) => s.content = v.compute(cx),
        V::ObjectFit(v) => s.object_fit = *v,
        V::ObjectPosition([x, y]) => s.object_position = [x.compute(cx), y.compute(cx)],
        V::UserSelect(v) => s.user_select = *v,
        V::UnicodeBidi(v) => s.unicode_bidi = *v,
    }
}

fn radius(cx: &ComputeContext<'_>, (h, v): &(Lp, Lp)) -> CornerRadius {
    CornerRadius {
        horizontal: cx.non_negative(h),
        vertical: cx.non_negative(v),
    }
}

/// Computes `grid-auto-rows` or `grid-auto-columns`.
fn track_sizes(cx: &ComputeContext<'_>, sizes: &[SpecifiedTrackSize]) -> Arc<[TrackSize]> {
    sizes
        .iter()
        .map(|t| t.map(&|l| cx.non_negative(l)))
        .collect()
}

fn gap(cx: &ComputeContext<'_>, value: Option<&Lp>) -> Gap {
    match value {
        None => Gap::Normal,
        Some(lp) => Gap::LengthPercentage(cx.non_negative(lp)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn border_width_snapping() {
        assert_eq!(snap_border_width(0.0), 0.0);
        assert_eq!(snap_border_width(-1.0), 0.0);
        assert_eq!(snap_border_width(0.3), 1.0);
        assert_eq!(snap_border_width(1.0), 1.0);
        assert_eq!(snap_border_width(2.7), 2.0);
        assert_eq!(snap_border_width(f32::NAN), 0.0);
    }

    #[test]
    fn monospace_detection() {
        let mono = [FontFamily::Generic(GenericFamily::Monospace)];
        assert!(is_monospace(&mono));
        let two = [
            FontFamily::Generic(GenericFamily::Monospace),
            FontFamily::Generic(GenericFamily::Monospace),
        ];
        assert!(!is_monospace(&two));
        assert!(!is_monospace(&[FontFamily::Named("Courier".into())]));
    }
}
