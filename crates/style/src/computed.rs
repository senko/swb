//! The computed style of an element: one value per supported longhand
//! property.

use std::sync::{Arc, LazyLock};

use crate::custom::CustomProperties;

use crate::font_settings::{FontFeatureSettings, FontVariationSettings};

use crate::properties::inert::InertStyle;
use crate::values::{
    Alignment, Appearance, AspectRatio, BackgroundAttachment, BackgroundBox,
    BackgroundRepeatKeyword, BackgroundSize, BorderCollapse, BorderStyle, BoxSizing, CaptionSide,
    Clear, ClipPath, Color, ColorId, CompositeOperator, Contain, ContainIntrinsic, ContainerName,
    ContainerType, Content, ContentVisibility, CornerRadius, CounterList, Cursor, Direction,
    Display, EmptyCells, FieldSizing, FillRule, Filter, FlexBasis, FlexDirection, FlexWrap, Float,
    FontFamily, FontSizeOrigin, FontStyle, FontVariantCaps, Gap, GenericFamily, GridAutoFlow,
    GridLine, GridTemplateAreas, Hyphens, Image, LengthPercentage, LengthPercentageOrAuto,
    LineHeight, ListStylePosition, ListStyleType, MaskClip, MaskImage, MaskMode, MaxSize,
    ObjectFit, OutlineStyle, Overflow, OverflowWrap, Overlay, PointerEvents, Position,
    PositionComponent, Rgba, ScrollbarWidth, ShapeRendering, Size, StrokeLinecap, StrokeLinejoin,
    SvgPaint, TableLayout, TextAlign, TextDecorationLine, TextDecorationStyle, TextOverflow,
    TextTransform, TrackBreadth, TrackList, TrackSize, UnicodeBidi, UserSelect, VerticalAlign,
    Visibility, WhiteSpace, WordBreak, ZIndex,
};
use crate::values::{ClipRect, TransformFunction, TransformOrigin};

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
    /// The `color` as `getComputedStyle` shows it when it came from a
    /// `color-mix()` (float coordinates, not 8-bit); inherited with `color`.
    pub color_exact: Option<ColorId>,
    /// `color-scheme`: `None` for `normal`, else the words of the value. See
    /// [`crate::used_color_scheme`].
    pub color_scheme: Option<Arc<str>>,
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
    /// `font-variation-settings`: sorted by tag (see `font_settings`).
    pub font_variation_settings: FontVariationSettings,
    /// `font-feature-settings`: sorted by tag.
    pub font_feature_settings: FontFeatureSettings,
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
    /// `fill` (SVG). `currentColor` stays a keyword, as CSS Color 4 says,
    /// and is resolved by the element that paints.
    pub fill: SvgPaint,
    pub fill_rule: FillRule,
    /// `clip-rule` (SVG): the fill rule of the shapes in a `clipPath`.
    pub clip_rule: FillRule,
    /// `shape-rendering` (SVG).
    pub shape_rendering: ShapeRendering,
    /// `clip-path`: `none` or a reference to a `clipPath` element.
    pub clip_path: ClipPath,
    /// `filter`: `none` or a list of filter functions (Filter Effects 1).
    pub filter: Filter,
    /// `backdrop-filter`: `none` or a list of filter functions (Filter
    /// Effects 2).
    pub backdrop_filter: Filter,
    /// `fill-opacity`, 0 to 1.
    pub fill_opacity: f32,
    /// `stroke` (SVG).
    pub stroke: SvgPaint,
    /// `stroke-width`; percentages refer to the normalized diagonal of the
    /// SVG viewport.
    pub stroke_width: LengthPercentage,
    pub stroke_linecap: StrokeLinecap,
    pub stroke_linejoin: StrokeLinejoin,
    pub stroke_miterlimit: f32,
    /// `stroke-dasharray`; empty for `none`.
    pub stroke_dasharray: Arc<[LengthPercentage]>,
    pub stroke_dashoffset: LengthPercentage,
    /// `stroke-opacity`, 0 to 1.
    pub stroke_opacity: f32,
    /// The custom properties (`--*`), `None` if there are none.
    pub custom_properties: Option<Arc<CustomProperties>>,
    /// True if a modal dialog blocks the element's document and the element
    /// is not that dialog or one of its flat tree descendants (HTML "blocked
    /// by a modal dialog", ADR 0037 section 7). Not a CSS property:
    /// inherited, set at the root element and cleared at the dialog; the
    /// element gets no hit region, no focus and no `:hover`.
    pub modal_inert: bool,

    // ----- Non-inherited properties -----
    pub display: Display,
    /// The `display` value before blockification (not a CSS property).
    /// The static position of an absolutely positioned box depends on it
    /// (CSS Position 3 §4.1, Chromium's `IsOriginalDisplayInlineType`).
    pub original_display: Display,
    pub position: Position,
    pub float: Float,
    pub clear: Clear,
    pub top: LengthPercentageOrAuto,
    pub right: LengthPercentageOrAuto,
    pub bottom: LengthPercentageOrAuto,
    pub left: LengthPercentageOrAuto,
    pub z_index: ZIndex,
    /// True if the element is a flex or grid item (a child of a flex or
    /// grid container's box). `z-index` applies to such an item although
    /// it is not positioned (CSS Flexbox 1 §4.3, CSS Grid 2 §6.4; measured
    /// in Chromium 148, `tools/probes/svg-symbol.json`, cases `grid-item-*`
    /// and `flex-item-*`). Not a CSS property.
    pub flex_or_grid_item: bool,
    /// The transform functions in order; empty for `none`.
    pub transform: Arc<[TransformFunction]>,
    pub transform_origin: TransformOrigin,
    /// `None` for `auto`.
    pub clip: Option<ClipRect>,
    pub width: Size,
    pub height: Size,
    pub min_width: Size,
    pub min_height: Size,
    pub max_width: MaxSize,
    pub max_height: MaxSize,
    pub box_sizing: BoxSizing,
    pub aspect_ratio: AspectRatio,
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
    /// `column-width` in px; `None` for `auto`. Layout uses it only to find
    /// multi-column containers (no multi-column layout yet).
    pub column_width: Option<f32>,
    /// `column-count`; `None` for `auto`.
    pub column_count: Option<u32>,
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
    pub counter_reset: CounterList,
    pub counter_increment: CounterList,
    pub counter_set: CounterList,
    pub contain: Contain,
    /// `container-type` (not inherited; ADR 0038).
    pub container_type: ContainerType,
    pub field_sizing: FieldSizing,
    /// `appearance`: `none` removes the native look of form controls.
    pub appearance: Appearance,
    pub content_visibility: ContentVisibility,
    /// `container-name` (not inherited).
    pub container_name: ContainerName,
    /// `contain-intrinsic-width`.
    pub contain_intrinsic_width: ContainIntrinsic,
    /// `contain-intrinsic-height`.
    pub contain_intrinsic_height: ContainIntrinsic,
    pub object_fit: ObjectFit,
    /// `scrollbar-width` (not inherited).
    pub scrollbar_width: ScrollbarWidth,
    /// `overlay` (not inherited).
    pub overlay: Overlay,
    /// `object-position`: x and y.
    pub object_position: [PositionComponent; 2],
    pub user_select: UserSelect,
    pub unicode_bidi: UnicodeBidi,
    /// The properties that have no effect on layout or paint (`transition`
    /// and its longhands, `will-change`, `touch-action`, `perspective`,
    /// `perspective-origin`; `properties/inert.rs`). `None` while all are
    /// initial, so an element that sets none pays one pointer.
    pub inert: Option<Arc<InertStyle>>,
    /// True for the `::details-content` box of a closed `details`. Not a
    /// CSS property: it stands for Chromium's `content-visibility: hidden`
    /// on that box. The box has size, layout, paint and style
    /// containment, and its contents are laid out but not rendered (not
    /// painted, hit tested or selected, and not part of the scrollable
    /// overflow of the boxes around).
    pub contents_hidden: bool,
}

static INITIAL: LazyLock<Arc<ComputedStyle>> =
    LazyLock::new(|| Arc::new(ComputedStyle::new_initial()));

impl ComputedStyle {
    /// The style with every property at its initial value (shared).
    pub fn initial() -> Arc<ComputedStyle> {
        Arc::clone(&INITIAL)
    }

    /// The initial style of a document whose root has the dark used color
    /// scheme: the text is white.
    pub(crate) fn initial_dark() -> Arc<ComputedStyle> {
        Arc::new(ComputedStyle {
            color: Rgba::WHITE,
            ..ComputedStyle::new_initial()
        })
    }

    /// True if `z-index` applies to the element: it is positioned, or it is
    /// a flex or grid item (CSS Positioned Layout 3, `z-index`).
    pub fn z_index_applies(&self) -> bool {
        self.position != Position::Static || self.flex_or_grid_item
    }

    #[allow(clippy::too_many_lines)] // One line per property.
    fn new_initial() -> Self {
        let medium_border = 3.0;
        ComputedStyle {
            color: Rgba::BLACK,
            color_exact: None,
            color_scheme: None,
            font_family: Arc::from([FontFamily::Generic(GenericFamily::Serif)]),
            font_size: 16.0,
            font_size_origin: FontSizeOrigin::default(),
            font_weight: 400.0,
            font_style: FontStyle::Normal,
            font_stretch: 100.0,
            font_variant_caps: FontVariantCaps::Normal,
            font_variation_settings: Arc::from([]),
            font_feature_settings: Arc::from([]),
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
            fill: SvgPaint::BLACK,
            fill_rule: FillRule::NonZero,
            clip_rule: FillRule::NonZero,
            shape_rendering: ShapeRendering::Auto,
            clip_path: ClipPath::None,
            filter: Filter::NONE,
            backdrop_filter: Filter::NONE,
            fill_opacity: 1.0,
            stroke: SvgPaint::None,
            stroke_width: LengthPercentage::Px(1.0),
            stroke_linecap: StrokeLinecap::Butt,
            stroke_linejoin: StrokeLinejoin::Miter,
            stroke_miterlimit: 4.0,
            stroke_dasharray: Arc::from([]),
            stroke_dashoffset: LengthPercentage::ZERO,
            stroke_opacity: 1.0,
            custom_properties: None,
            modal_inert: false,

            display: Display::Inline,
            original_display: Display::Inline,
            position: Position::Static,
            float: Float::None,
            clear: Clear::None,
            top: LengthPercentageOrAuto::Auto,
            right: LengthPercentageOrAuto::Auto,
            bottom: LengthPercentageOrAuto::Auto,
            left: LengthPercentageOrAuto::Auto,
            z_index: ZIndex::Auto,
            flex_or_grid_item: false,
            transform: Arc::from([]),
            transform_origin: TransformOrigin::default(),
            clip: None,
            width: Size::Auto,
            height: Size::Auto,
            min_width: Size::Auto,
            min_height: Size::Auto,
            max_width: MaxSize::None,
            max_height: MaxSize::None,
            box_sizing: BoxSizing::ContentBox,
            aspect_ratio: AspectRatio::AUTO,
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
            column_width: None,
            column_count: None,
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
            counter_reset: CounterList::default(),
            counter_increment: CounterList::default(),
            counter_set: CounterList::default(),
            contain: Contain::NONE,
            container_type: ContainerType::Normal,
            field_sizing: FieldSizing::Fixed,
            appearance: Appearance::None,
            content_visibility: ContentVisibility::Visible,
            container_name: None,
            contain_intrinsic_width: ContainIntrinsic::NONE,
            contain_intrinsic_height: ContainIntrinsic::NONE,
            object_fit: ObjectFit::Fill,
            scrollbar_width: ScrollbarWidth::Auto,
            overlay: Overlay::None,
            object_position: [PositionComponent::CENTER, PositionComponent::CENTER],
            user_select: UserSelect::Auto,
            unicode_bidi: UnicodeBidi::Normal,
            inert: None,
            contents_hidden: false,
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
        self.modal_inert = parent.modal_inert;
    }

    /// True if the user can select the element's text: `user-select` is not
    /// `none` and a modal dialog does not make the element inert (HTML
    /// "inert nodes": the user cannot select them).
    pub fn is_user_selectable(&self) -> bool {
        self.user_select != UserSelect::None && !self.modal_inert
    }

    /// True if the box can be the target of a pointer event: `pointer-events`
    /// is not `none` and a modal dialog does not make the element inert.
    pub fn receives_pointer_events(&self) -> bool {
        self.pointer_events != PointerEvents::None && !self.modal_inert
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

    /// True if `filter` is not `none`, whatever the functions are (also a
    /// `url()` and functions that change nothing). A filtered element is
    /// a stacking context and the containing block of its absolutely
    /// positioned and fixed descendants, like a transformed one, but not
    /// when it is an inline box (Filter Effects 1 §5, measured in
    /// Chromium 148).
    pub fn has_filter(&self) -> bool {
        !self.filter.is_none()
    }

    /// True if the element is a candidate in the query container lookup
    /// (ADR 0038): its `container-type` establishes a size container, or it
    /// has a `container-name` (a name alone matches `@container NAME`).
    pub fn is_query_container(&self) -> bool {
        self.size_container_axes().0 || self.container_name.is_some()
    }

    /// The axes (inline, block) that the element's `container-type` lets
    /// size queries and container units read: `inline-size` the inline
    /// axis, `size` both; `normal` and `scroll-state` none.
    pub fn size_container_axes(&self) -> (bool, bool) {
        match self.container_type {
            ContainerType::Size => (true, true),
            ContainerType::InlineSize => (true, false),
            ContainerType::Normal | ContainerType::ScrollState => (false, false),
        }
    }

    /// True if `container-name` lists `name` (case-sensitive).
    pub fn has_container_name(&self, name: &str) -> bool {
        self.container_name
            .as_ref()
            .is_some_and(|names| names.iter().any(|n| &**n == name))
    }

    /// True if size containment can apply to the element's box (CSS
    /// Containment 2 §3.1): it has a principal box that is not a
    /// non-atomic inline, a table (inner display `table`) or an internal
    /// table or ruby box. The container units skip containers without such
    /// a box, and their queries are unknown (measured in Chromium 148,
    /// `odd-container-boxes`, `units-odd-containers`).
    pub fn size_containment_applies(&self) -> bool {
        !matches!(
            self.display,
            Display::None
                | Display::Contents
                | Display::Inline
                | Display::Table
                | Display::InlineTable
                | Display::TableRowGroup
                | Display::TableHeaderGroup
                | Display::TableFooterGroup
                | Display::TableRow
                | Display::TableCell
                | Display::TableColumnGroup
                | Display::TableColumn
                | Display::TableCaption
        )
    }

    /// The effective size containment of the box: `contain: size` (also
    /// `strict`) or `container-type: size`, if it applies to the box
    /// ([`Self::size_containment_applies`]).
    pub fn has_size_containment(&self) -> bool {
        (self.contain.size || self.container_type == ContainerType::Size || self.skips_contents())
            && self.size_containment_applies()
    }

    /// True if the box skips its contents: `content-visibility: hidden`
    /// where containment applies, and the content of a closed `details`
    /// ([`ComputedStyle::contents_hidden`]). The contents are laid out when
    /// a script asks for their boxes, but not painted, hit, selected,
    /// focused or in `innerText` (CSS Containment 2 §4, measured in
    /// Chromium 148, `cv-hidden-*`). On a non-atomic inline box and on
    /// `display: contents` the value has no effect (measured, `cv-inline-*`
    /// and `cv-contents-*`). Every site that asks whether contents are
    /// skipped uses this method.
    pub fn skips_contents(&self) -> bool {
        self.contents_hidden
            || (self.content_visibility == ContentVisibility::Hidden
                && self.layout_containment_applies())
    }

    /// True if layout and paint containment can apply to the box (CSS
    /// Containment 2 §3.2, §3.3): not to non-atomic inline boxes and
    /// internal table or ruby boxes except table cells.
    pub fn layout_containment_applies(&self) -> bool {
        !matches!(
            self.display,
            Display::None
                | Display::Contents
                | Display::Inline
                | Display::TableRowGroup
                | Display::TableHeaderGroup
                | Display::TableFooterGroup
                | Display::TableRow
                | Display::TableColumnGroup
                | Display::TableColumn
                | Display::TableCaption
        )
    }

    /// The effective layout containment: `contain: layout` (also `content`
    /// and `strict`), or a `content-visibility` other than `visible`. The
    /// box is an independent formatting context, the containing block of
    /// absolutely positioned and fixed boxes, a stacking context and
    /// has no baseline (measured in Chromium 148, `ct-*`).
    pub fn has_layout_containment(&self) -> bool {
        (self.contain.layout
            || self.content_visibility != ContentVisibility::Visible
            || self.contents_hidden)
            && self.layout_containment_applies()
    }

    /// True if the box clips its content in the inline axis: `overflow-x`
    /// other than `visible`, or paint containment (the padding box).
    pub fn clips_x(&self) -> bool {
        self.overflow_x.clips() || self.has_paint_containment()
    }

    /// True if the box clips its content in the block axis (see
    /// [`ComputedStyle::clips_x`]).
    pub fn clips_y(&self) -> bool {
        self.overflow_y.clips() || self.has_paint_containment()
    }

    /// The effective paint containment: `contain: paint` (also `content`
    /// and `strict`), or a `content-visibility` other than `visible`. The
    /// box clips its content to its padding box and has the same effects
    /// as layout containment on formatting and positioning.
    pub fn has_paint_containment(&self) -> bool {
        (self.contain.paint
            || self.content_visibility != ContentVisibility::Visible
            || self.contents_hidden)
            && self.layout_containment_applies()
    }

    /// The effective inline-size containment (without size containment):
    /// `contain: inline-size` or `container-type: inline-size`, if it
    /// applies to the box. A box with size containment has both.
    pub fn has_inline_size_containment(&self) -> bool {
        self.has_size_containment()
            || ((self.contain.inline_size || self.container_type == ContainerType::InlineSize)
                && self.size_containment_applies())
    }

    /// True if `filter` or `backdrop-filter` is not `none`: the element is
    /// a stacking context and the containing block of its absolutely
    /// positioned and fixed descendants (unless it is an inline box or the
    /// root element).
    pub fn has_filter_effect(&self) -> bool {
        self.has_filter() || self.has_backdrop_filter()
    }

    /// True if the element is the containing block of its absolutely
    /// positioned and fixed descendants because of a filter: `filter` or
    /// `backdrop-filter` is not `none` ([`Self::has_filter_effect`]), or
    /// `will-change` names one of them (CSS Will Change 1 §3, measured in
    /// Chromium 148). The caller excludes inline boxes and the root
    /// element.
    pub fn is_filter_containing_block(&self) -> bool {
        self.has_filter_effect() || self.will_change_makes_containing_block()
    }

    /// True if `backdrop-filter` is not `none`, whatever the functions are.
    /// Such an element is a stacking context, the containing block of its
    /// absolutely positioned and fixed descendants (not when it is an
    /// inline box), and a backdrop root for its descendants (Filter
    /// Effects 2 §2, measured in Chromium 148).
    pub fn has_backdrop_filter(&self) -> bool {
        !self.backdrop_filter.is_none()
    }

    /// True if the element is a backdrop root (Filter Effects 2 §2,
    /// <https://drafts.fxtf.org/filter-effects-2/#BackdropRoot>): the
    /// `backdrop-filter` of a descendant sees only what is painted inside
    /// the element. Measured in Chromium 148: `opacity` below 1, a `filter`
    /// (any, also an identity or a missing `url()`), a mask, a `clip-path`
    /// shape, a `backdrop-filter`, and `will-change` with one of these
    /// properties. (`mix-blend-mode` also makes a root in Chromium; swb
    /// does not have the property.) The root element is the root of the
    /// page.
    pub fn is_backdrop_root(&self) -> bool {
        self.opacity < 1.0
            || self.has_filter()
            || self.has_backdrop_filter()
            || self.has_mask()
            || self.clip_path.is_shape()
            || self.will_change_makes_stacking_context()
    }

    /// True if `will-change` names a property that makes a stacking
    /// context and a backdrop root: `opacity`, `filter`,
    /// `backdrop-filter`, a mask or `clip-path` (CSS Will Change 1 §3: a
    /// property that would make a stacking context does so in
    /// `will-change` too; measured for the backdrop root in Chromium 148).
    /// `will-change: transform` does neither in swb (inert).
    pub fn will_change_makes_stacking_context(&self) -> bool {
        InertStyle::will_change_effects(self).stacking_context
    }

    /// True if `will-change` names `filter` or `backdrop-filter`, which
    /// makes the element the containing block of its absolutely positioned
    /// and fixed descendants (CSS Will Change 1 §3; `opacity`, `clip-path`
    /// and masks do not, measured in Chromium 148).
    pub fn will_change_makes_containing_block(&self) -> bool {
        InertStyle::will_change_effects(self).containing_block
    }

    /// True if `transform` is not `none`. Transforms do not apply to
    /// inline boxes (only to block-level and atomic boxes); layout and
    /// paint check that.
    pub fn has_transform(&self) -> bool {
        !self.transform.is_empty()
    }
}
