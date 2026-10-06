//! The computed style of an element: one value per supported longhand
//! property.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use swb_css::ComponentValue;

use crate::values::{
    Alignment, BackgroundAttachment, BackgroundBox, BackgroundRepeatKeyword, BackgroundSize,
    BorderCollapse, BorderStyle, BoxSizing, CaptionSide, Clear, Color, CompositeOperator, Content,
    CornerRadius, Cursor, Direction, Display, EmptyCells, FlexBasis, FlexDirection, FlexWrap,
    Float, FontFamily, FontSizeOrigin, FontStyle, FontVariantCaps, Gap, GenericFamily,
    GridAutoFlow, GridLine, GridTemplateAreas, Hyphens, Image, LengthPercentage,
    LengthPercentageOrAuto, LineHeight, ListStylePosition, ListStyleType, MaskClip, MaskImage,
    MaskMode, MaxSize, ObjectFit, OutlineStyle, Overflow, OverflowWrap, PointerEvents, Position,
    PositionComponent, Rgba, Size, TableLayout, TextAlign, TextDecorationLine, TextDecorationStyle,
    TextOverflow, TextTransform, TrackBreadth, TrackList, TrackSize, UnicodeBidi, UserSelect,
    VerticalAlign, Visibility, WhiteSpace, WordBreak, ZIndex,
};

/// Custom properties (`--name: value`) of an element, after `var()`
/// substitution. Shared with the parent when unchanged.
pub type CustomProperties = HashMap<Arc<str>, Arc<[ComponentValue]>>;

/// The computed values of all supported properties for one element (or
/// pseudo-element).
///
/// Lengths are in CSS px. Percentages are kept where layout resolves them.
/// Each field is the CSS property of the same name (with `_` for `-`),
/// unless its documentation says otherwise.
#[derive(Clone, Debug, PartialEq)]
pub struct ComputedStyle {
    // ----- Inherited properties -----
    pub color: Rgba,
    pub font_family: Arc<[FontFamily]>,
    pub font_size: f32,
    /// How `font_size` was derived (not a CSS property). Inherited with
    /// `font-size`. It implements Chromium's monospace font size quirk:
    /// when the family changes between the single generic `monospace` and
    /// anything else, a keyword-derived size is recomputed for the default
    /// fixed font size (13px instead of 16px). See
    /// `crate::cascade::adjust_font_size_for_family`.
    pub font_size_origin: FontSizeOrigin,
    pub font_weight: f32,
    pub font_style: FontStyle,
    pub font_stretch: f32,
    pub font_variant_caps: FontVariantCaps,
    pub line_height: LineHeight,
    pub text_align: TextAlign,
    pub text_indent: LengthPercentage,
    pub text_transform: TextTransform,
    pub white_space: WhiteSpace,
    pub letter_spacing: f32,
    pub word_spacing: f32,
    pub word_break: WordBreak,
    pub overflow_wrap: OverflowWrap,
    pub hyphens: Hyphens,
    pub tab_size: f32,
    pub visibility: Visibility,
    pub list_style_type: ListStyleType,
    pub list_style_position: ListStylePosition,
    pub list_style_image: Option<Image>,
    pub cursor: Cursor,
    pub direction: Direction,
    pub border_collapse: BorderCollapse,
    /// The horizontal part of `border-spacing`.
    pub border_spacing_horizontal: f32,
    /// The vertical part of `border-spacing`.
    pub border_spacing_vertical: f32,
    pub caption_side: CaptionSide,
    pub empty_cells: EmptyCells,
    pub pointer_events: PointerEvents,
    /// The custom properties (`--*`), `None` if there are none.
    pub custom_properties: Option<Arc<CustomProperties>>,

    // ----- Non-inherited properties -----
    pub display: Display,
    pub position: Position,
    pub float: Float,
    pub clear: Clear,
    pub top: LengthPercentageOrAuto,
    pub right: LengthPercentageOrAuto,
    pub bottom: LengthPercentageOrAuto,
    pub left: LengthPercentageOrAuto,
    pub z_index: ZIndex,
    pub width: Size,
    pub height: Size,
    pub min_width: Size,
    pub min_height: Size,
    pub max_width: MaxSize,
    pub max_height: MaxSize,
    pub box_sizing: BoxSizing,
    pub aspect_ratio: Option<f32>,
    pub margin_top: LengthPercentageOrAuto,
    pub margin_right: LengthPercentageOrAuto,
    pub margin_bottom: LengthPercentageOrAuto,
    pub margin_left: LengthPercentageOrAuto,
    pub padding_top: LengthPercentage,
    pub padding_right: LengthPercentage,
    pub padding_bottom: LengthPercentage,
    pub padding_left: LengthPercentage,
    pub border_top_width: f32,
    pub border_right_width: f32,
    pub border_bottom_width: f32,
    pub border_left_width: f32,
    pub border_top_style: BorderStyle,
    pub border_right_style: BorderStyle,
    pub border_bottom_style: BorderStyle,
    pub border_left_style: BorderStyle,
    pub border_top_color: Color,
    pub border_right_color: Color,
    pub border_bottom_color: Color,
    pub border_left_color: Color,
    pub border_top_left_radius: CornerRadius,
    pub border_top_right_radius: CornerRadius,
    pub border_bottom_right_radius: CornerRadius,
    pub border_bottom_left_radius: CornerRadius,
    pub outline_width: f32,
    pub outline_style: OutlineStyle,
    pub outline_color: Color,
    pub outline_offset: f32,
    pub background_color: Color,
    pub background_image: Arc<[Option<Image>]>,
    pub background_position_x: Arc<[PositionComponent]>,
    pub background_position_y: Arc<[PositionComponent]>,
    pub background_size: Arc<[BackgroundSize]>,
    pub background_repeat: Arc<[(BackgroundRepeatKeyword, BackgroundRepeatKeyword)]>,
    pub background_origin: Arc<[BackgroundBox]>,
    pub background_clip: Arc<[BackgroundBox]>,
    pub background_attachment: Arc<[BackgroundAttachment]>,
    pub mask_image: Arc<[MaskImage]>,
    pub mask_mode: Arc<[MaskMode]>,
    /// `-webkit-mask-position-x` (`mask-position` is a shorthand, as in
    /// Chromium).
    pub mask_position_x: Arc<[PositionComponent]>,
    /// `-webkit-mask-position-y`.
    pub mask_position_y: Arc<[PositionComponent]>,
    pub mask_size: Arc<[BackgroundSize]>,
    pub mask_repeat: Arc<[(BackgroundRepeatKeyword, BackgroundRepeatKeyword)]>,
    pub mask_origin: Arc<[BackgroundBox]>,
    pub mask_clip: Arc<[MaskClip]>,
    pub mask_composite: Arc<[CompositeOperator]>,
    pub overflow_x: Overflow,
    pub overflow_y: Overflow,
    pub text_overflow: TextOverflow,
    pub opacity: f32,
    pub vertical_align: VerticalAlign,
    pub text_decoration_line: TextDecorationLine,
    pub text_decoration_color: Color,
    pub text_decoration_style: TextDecorationStyle,
    pub flex_direction: FlexDirection,
    pub flex_wrap: FlexWrap,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub flex_basis: FlexBasis,
    pub order: i32,
    pub justify_content: Alignment,
    pub align_items: Alignment,
    pub align_self: Alignment,
    pub align_content: Alignment,
    pub row_gap: Gap,
    pub column_gap: Gap,
    /// `justify-items`. `legacy` computes to `normal` and `legacy center`
    /// to `center` (the inheritance of `legacy` is not supported).
    pub justify_items: Alignment,
    pub justify_self: Alignment,
    pub grid_template_columns: TrackList,
    pub grid_template_rows: TrackList,
    /// `grid-template-areas`; `None` for `none`.
    pub grid_template_areas: Option<Arc<GridTemplateAreas>>,
    pub grid_auto_columns: Arc<[TrackSize]>,
    pub grid_auto_rows: Arc<[TrackSize]>,
    pub grid_auto_flow: GridAutoFlow,
    pub grid_row_start: GridLine,
    pub grid_row_end: GridLine,
    pub grid_column_start: GridLine,
    pub grid_column_end: GridLine,
    pub table_layout: TableLayout,
    pub content: Content,
    pub object_fit: ObjectFit,
    pub user_select: UserSelect,
    pub unicode_bidi: UnicodeBidi,
}

static INITIAL: LazyLock<Arc<ComputedStyle>> =
    LazyLock::new(|| Arc::new(ComputedStyle::new_initial()));

impl ComputedStyle {
    /// The style with every property at its initial value (shared).
    pub fn initial() -> Arc<ComputedStyle> {
        Arc::clone(&INITIAL)
    }

    #[allow(clippy::too_many_lines)] // One line per property.
    fn new_initial() -> Self {
        let medium_border = 3.0;
        ComputedStyle {
            color: Rgba::BLACK,
            font_family: Arc::from([FontFamily::Generic(GenericFamily::Serif)]),
            font_size: 16.0,
            font_size_origin: FontSizeOrigin::default(),
            font_weight: 400.0,
            font_style: FontStyle::Normal,
            font_stretch: 100.0,
            font_variant_caps: FontVariantCaps::Normal,
            line_height: LineHeight::Normal,
            text_align: TextAlign::Start,
            text_indent: LengthPercentage::ZERO,
            text_transform: TextTransform::None,
            white_space: WhiteSpace::Normal,
            letter_spacing: 0.0,
            word_spacing: 0.0,
            word_break: WordBreak::Normal,
            overflow_wrap: OverflowWrap::Normal,
            hyphens: Hyphens::Manual,
            tab_size: 8.0,
            visibility: Visibility::Visible,
            list_style_type: ListStyleType::Disc,
            list_style_position: ListStylePosition::Outside,
            list_style_image: None,
            cursor: Cursor::Auto,
            direction: Direction::Ltr,
            border_collapse: BorderCollapse::Separate,
            border_spacing_horizontal: 0.0,
            border_spacing_vertical: 0.0,
            caption_side: CaptionSide::Top,
            empty_cells: EmptyCells::Show,
            pointer_events: PointerEvents::Auto,
            custom_properties: None,

            display: Display::Inline,
            position: Position::Static,
            float: Float::None,
            clear: Clear::None,
            top: LengthPercentageOrAuto::Auto,
            right: LengthPercentageOrAuto::Auto,
            bottom: LengthPercentageOrAuto::Auto,
            left: LengthPercentageOrAuto::Auto,
            z_index: ZIndex::Auto,
            width: Size::Auto,
            height: Size::Auto,
            min_width: Size::Auto,
            min_height: Size::Auto,
            max_width: MaxSize::None,
            max_height: MaxSize::None,
            box_sizing: BoxSizing::ContentBox,
            aspect_ratio: None,
            margin_top: LengthPercentageOrAuto::ZERO,
            margin_right: LengthPercentageOrAuto::ZERO,
            margin_bottom: LengthPercentageOrAuto::ZERO,
            margin_left: LengthPercentageOrAuto::ZERO,
            padding_top: LengthPercentage::ZERO,
            padding_right: LengthPercentage::ZERO,
            padding_bottom: LengthPercentage::ZERO,
            padding_left: LengthPercentage::ZERO,
            border_top_width: medium_border,
            border_right_width: medium_border,
            border_bottom_width: medium_border,
            border_left_width: medium_border,
            border_top_style: BorderStyle::None,
            border_right_style: BorderStyle::None,
            border_bottom_style: BorderStyle::None,
            border_left_style: BorderStyle::None,
            border_top_color: Color::CurrentColor,
            border_right_color: Color::CurrentColor,
            border_bottom_color: Color::CurrentColor,
            border_left_color: Color::CurrentColor,
            border_top_left_radius: CornerRadius::default(),
            border_top_right_radius: CornerRadius::default(),
            border_bottom_right_radius: CornerRadius::default(),
            border_bottom_left_radius: CornerRadius::default(),
            outline_width: medium_border,
            outline_style: OutlineStyle::None,
            outline_color: Color::CurrentColor,
            outline_offset: 0.0,
            background_color: Color::TRANSPARENT,
            background_image: Arc::from([None]),
            background_position_x: Arc::from([PositionComponent::default()]),
            background_position_y: Arc::from([PositionComponent::default()]),
            background_size: Arc::from([BackgroundSize::Auto]),
            background_repeat: Arc::from([(
                BackgroundRepeatKeyword::Repeat,
                BackgroundRepeatKeyword::Repeat,
            )]),
            background_origin: Arc::from([BackgroundBox::PaddingBox]),
            background_clip: Arc::from([BackgroundBox::BorderBox]),
            background_attachment: Arc::from([BackgroundAttachment::Scroll]),
            mask_image: Arc::from([MaskImage::None]),
            mask_mode: Arc::from([MaskMode::MatchSource]),
            mask_position_x: Arc::from([PositionComponent::default()]),
            mask_position_y: Arc::from([PositionComponent::default()]),
            mask_size: Arc::from([BackgroundSize::Auto]),
            mask_repeat: Arc::from([(
                BackgroundRepeatKeyword::Repeat,
                BackgroundRepeatKeyword::Repeat,
            )]),
            mask_origin: Arc::from([BackgroundBox::BorderBox]),
            mask_clip: Arc::from([MaskClip::BorderBox]),
            mask_composite: Arc::from([CompositeOperator::SourceOver]),
            overflow_x: Overflow::Visible,
            overflow_y: Overflow::Visible,
            text_overflow: TextOverflow::Clip,
            opacity: 1.0,
            vertical_align: VerticalAlign::default(),
            text_decoration_line: TextDecorationLine::empty(),
            text_decoration_color: Color::CurrentColor,
            text_decoration_style: TextDecorationStyle::Solid,
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::Nowrap,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_basis: FlexBasis::default(),
            order: 0,
            justify_content: Alignment::Normal,
            align_items: Alignment::Normal,
            align_self: Alignment::Auto,
            align_content: Alignment::Normal,
            row_gap: Gap::Normal,
            column_gap: Gap::Normal,
            justify_items: Alignment::Normal,
            justify_self: Alignment::Auto,
            grid_template_columns: TrackList::default(),
            grid_template_rows: TrackList::default(),
            grid_template_areas: None,
            grid_auto_columns: Arc::from([TrackSize::Breadth(TrackBreadth::Auto)]),
            grid_auto_rows: Arc::from([TrackSize::Breadth(TrackBreadth::Auto)]),
            grid_auto_flow: GridAutoFlow::default(),
            grid_row_start: GridLine::Auto,
            grid_row_end: GridLine::Auto,
            grid_column_start: GridLine::Auto,
            grid_column_end: GridLine::Auto,
            table_layout: TableLayout::Auto,
            content: Content::Normal,
            object_fit: ObjectFit::Fill,
            user_select: UserSelect::Auto,
            unicode_bidi: UnicodeBidi::Normal,
        }
    }

    /// A new style for a child of `parent`: inherited properties come from
    /// the parent, all others are initial.
    pub fn inherit_from(parent: &ComputedStyle) -> ComputedStyle {
        let mut style = (*INITIAL).as_ref().clone();
        style.copy_inherited_from(parent);
        style
    }

    /// The style of an anonymous box (generated by layout, not by an
    /// element) with the given parent: inherited properties from the
    /// parent, all others at their initial computed values. Unlike
    /// [`ComputedStyle::inherit_from`], border widths are zero, because the
    /// initial `border-style` is `none` (the cascade applies that rule for
    /// elements).
    pub fn anonymous_from(parent: &ComputedStyle) -> ComputedStyle {
        let mut style = Self::inherit_from(parent);
        style.border_top_width = 0.0;
        style.border_right_width = 0.0;
        style.border_bottom_width = 0.0;
        style.border_left_width = 0.0;
        style
    }

    /// Copies all inherited properties from `parent`.
    fn copy_inherited_from(&mut self, parent: &ComputedStyle) {
        for &id in crate::properties::LonghandId::ALL {
            if id.is_inherited() {
                id.copy_value(parent, self);
            }
        }
        self.custom_properties.clone_from(&parent.custom_properties);
    }

    /// The border widths (top, right, bottom, left), for test assertions.
    #[cfg(test)]
    pub(crate) fn border_widths(&self) -> [f32; 4] {
        [
            self.border_top_width,
            self.border_right_width,
            self.border_bottom_width,
            self.border_left_width,
        ]
    }

    /// The used line height in px. `normal` uses `normal_line_height`,
    /// which the caller computes from font metrics.
    pub fn used_line_height(&self, normal_line_height: f32) -> f32 {
        self.line_height
            .resolve(self.font_size)
            .unwrap_or(normal_line_height)
    }

    /// True if the box is a float.
    pub fn is_floating(&self) -> bool {
        self.float != Float::None
    }

    /// True if the box is absolutely positioned (`absolute` or `fixed`).
    pub fn is_absolutely_positioned(&self) -> bool {
        self.position.is_absolutely_positioned()
    }

    /// True if the element is masked: a `mask-image` layer is not `none`
    /// (CSS Masking 1 §7.1). A masked element is a stacking context, as
    /// with `opacity` below 1; unlike in the specification of filters, it
    /// is not a containing block for positioned descendants (as in
    /// Chromium).
    pub fn has_mask(&self) -> bool {
        self.mask_image.iter().any(|i| *i != MaskImage::None)
    }
}
