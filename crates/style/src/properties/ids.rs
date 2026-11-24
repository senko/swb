//! The list of supported longhand properties.
//!
//! The `longhands!` macro generates [`LonghandId`] with its name lookup,
//! the "inherited" flag and a function that copies the computed value
//! between two styles (used for inheritance and for `inherit`/`initial`).
//! It also generates [`LonghandValue`], the parsed (specified) value of a
//! declaration of each longhand.

use std::sync::Arc;

use super::clip_path::SpecifiedClipPath;
use super::effects::{SpecifiedShadows, SpecifiedTranslate};
use super::filter::SpecifiedFilter;
use super::mask::SpecifiedMaskImage;
use super::specified::{
    SpecifiedBackgroundSize, SpecifiedContainIntrinsic, SpecifiedContent, SpecifiedFlexBasis,
    SpecifiedFontSize, SpecifiedFontWeight, SpecifiedLineHeight, SpecifiedPosition, SpecifiedSize,
    SpecifiedTextAlign, SpecifiedVerticalAlign,
};
use super::transform::{SpecifiedClip, SpecifiedTransform, SpecifiedTransformOrigin};
use crate::ComputedStyle;
use crate::animation::properties::{
    AnimationName, Composition, IterationCount, PlayState, RangeItem,
};
use crate::animation::transition_properties::TransitionName;
use crate::animation::{Easing, FillMode, PlaybackDirection, Time};
use crate::font_settings::{FontFeatureSettings, FontVariationSettings};
use crate::parse::image::SpecifiedImage;
use crate::values::{
    Alignment, Appearance, AspectRatio, BackgroundAttachment, BackgroundBox,
    BackgroundRepeatKeyword, BorderCollapse, BorderStyle, BoxOrient, BoxSizing, CaptionSide, Clear,
    Color, CompositeOperator, Contain, ContainerName, ContainerType, ContentVisibility,
    CornerShape, CounterList, Cursor, Direction, Display, EmptyCells, FieldSizing, FillRule,
    FlexDirection, FlexWrap, Float, FontFamily, FontSmoothing, FontStyle, FontVariantCaps,
    FontVariantLigatures, FontVariantNumeric, GridAutoFlow, GridLine, GridTemplateAreas, Hyphens,
    InterpolateSize, Isolation, LineBreak, ListStylePosition, ListStyleType, MaskClip, MaskMode,
    MixBlendMode, ObjectFit, OutlineStyle, Overflow, OverflowAnchor, OverflowWrap, Overlay,
    OverscrollBehavior, PointerEvents, Position, Resize, Rotate, Scale, ScrollbarWidth,
    ShapeRendering, SpaceCollapse, SpecifiedLengthPercentage as Lp, SpecifiedTrackList,
    SpecifiedTrackSize, StrokeLinecap, StrokeLinejoin, SvgPaint, TableLayout, TextDecorationLine,
    TextDecorationStyle, TextOverflow, TextSizeAdjust, TextTransform, TextWrapStyle, UnicodeBidi,
    UserSelect, Visibility, WordBreak, ZIndex,
};

/// The computed value of one longhand alone ([`LonghandId::value_of`]):
/// the field or fields of [`ComputedStyle`] that hold it, shared.
#[derive(Clone)]
pub(crate) struct ComputedSlot(Arc<dyn std::any::Any + Send + Sync>);

impl ComputedSlot {
    /// True if both are the same object (so the same value).
    pub(crate) fn ptr_eq(&self, other: &ComputedSlot) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for ComputedSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ComputedSlot")
    }
}

macro_rules! longhands {
    ($($id:ident $name:literal $kind:ident $field:ident: $spec:ty;)+
     ;
     $($iid:ident $iname:literal: $ispec:ty;)+
     ;
     $($aid:ident $aname:literal: $aspec:ty;)+) => {
        /// A longhand property.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub(crate) enum LonghandId {
            $($id,)+
            $($iid,)+
            $($aid),+
        }

        impl LonghandId {
            /// All longhands, in declaration order.
            pub(crate) const ALL: &'static [LonghandId] =
                &[$(LonghandId::$id,)+ $(LonghandId::$iid,)+ $(LonghandId::$aid),+];

            /// The number of longhands.
            pub(crate) const COUNT: usize = Self::ALL.len();

            /// Looks up a longhand by its CSS name (already lowercase).
            pub(crate) fn from_name(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(LonghandId::$id),)+
                    $($iname => Some(LonghandId::$iid),)+
                    $($aname => Some(LonghandId::$aid),)+
                    _ => None,
                }
            }

            /// The CSS name.
            pub(crate) fn name(self) -> &'static str {
                match self {
                    $(LonghandId::$id => $name,)+
                    $(LonghandId::$iid => $iname,)+
                    $(LonghandId::$aid => $aname),+
                }
            }

            /// True if the property is inherited by default.
            pub(crate) fn is_inherited(self) -> bool {
                match self {
                    $(LonghandId::$id => longhands!(@inherited $kind),)+
                    $(LonghandId::$iid => false,)+
                    $(LonghandId::$aid => false),+
                }
            }

            /// Copies this property's computed value from `from` to `to`.
            pub(crate) fn copy_value(self, from: &ComputedStyle, to: &mut ComputedStyle) {
                let white_space = to.white_space;
                match self {
                    $(LonghandId::$id => to.$field.clone_from(&from.$field),)+
                    $(LonghandId::$iid)|+ => super::inert::copy_value(self, from, to),
                    $(LonghandId::$aid)|+ => {
                        crate::animation::properties::copy_value(self, from, to);
                    }
                }
                // `white-space-collapse` and `text-wrap-mode` share one
                // computed field: each copies only its half.
                match self {
                    LonghandId::WhiteSpaceCollapse => {
                        to.white_space = white_space.with_collapse(from.white_space.collapse());
                    }
                    LonghandId::TextWrapMode => {
                        to.white_space = white_space.with_wrap(from.white_space.wraps());
                    }
                    _ => {}
                }
                if self == LonghandId::FontSize {
                    to.font_size_origin = from.font_size_origin;
                }
                if self == LonghandId::Color {
                    to.color_exact = from.color_exact;
                }
            }
        }

        impl LonghandId {
            /// True if `a` and `b` have the same computed value of this
            /// property (the comparison of the before- and after-change
            /// styles of a transition).
            pub(crate) fn same_value(self, a: &ComputedStyle, b: &ComputedStyle) -> bool {
                // `white-space-collapse` and `text-wrap-mode` share one
                // field; `color` keeps its exact form next to it.
                match self {
                    LonghandId::WhiteSpaceCollapse => {
                        return a.white_space.collapse() == b.white_space.collapse();
                    }
                    LonghandId::TextWrapMode => {
                        return a.white_space.wraps() == b.white_space.wraps();
                    }
                    LonghandId::Color => {
                        return a.color == b.color && a.color_exact == b.color_exact;
                    }
                    _ => {}
                }
                match self {
                    $(LonghandId::$id => a.$field == b.$field,)+
                    $(LonghandId::$iid)|+ => super::inert::same_value(self, a, b),
                    $(LonghandId::$aid)|+ => {
                        a.animations == b.animations && a.transitions == b.transitions
                    }
                }
            }
        }

        impl LonghandId {
            /// The computed value of this property in `style` alone: a copy
            /// of the field or fields that hold it (the start and end values
            /// of a transition, ADR 0044 section 20; a few dozen bytes where
            /// the style is a few kilobytes).
            pub(crate) fn value_of(self, style: &ComputedStyle) -> ComputedSlot {
                match self {
                    LonghandId::Color => {
                        return ComputedSlot(Arc::new((style.color, style.color_exact)));
                    }
                    LonghandId::FontSize => {
                        return ComputedSlot(Arc::new((style.font_size, style.font_size_origin)));
                    }
                    _ => {}
                }
                ComputedSlot(match self {
                    $(LonghandId::$id => Arc::new(style.$field.clone()),)+
                    $(LonghandId::$iid)|+ => Arc::new(style.inert.clone()),
                    $(LonghandId::$aid)|+ => {
                        Arc::new((style.animations.clone(), style.transitions.clone()))
                    }
                })
            }

            /// Stores the value `slot` of this property (from
            /// [`LonghandId::value_of`] of the same property) in `style`: the
            /// fields that hold it, whole (both halves of `white-space` too).
            /// A slot of another type is a bug: it changes nothing, and a
            /// debug build stops at it.
            pub(crate) fn put_value(self, slot: &ComputedSlot, style: &mut ComputedStyle) {
                let value: &(dyn std::any::Any + Send + Sync) = &*slot.0;
                let fits = |found: bool| {
                    debug_assert!(found, "a slot of another type for {}", self.name());
                };
                match self {
                    LonghandId::Color => {
                        let v = value.downcast_ref();
                        fits(v.is_some());
                        if let Some((color, exact)) = v {
                            style.color = *color;
                            style.color_exact = *exact;
                        }
                        return;
                    }
                    LonghandId::FontSize => {
                        let v = value.downcast_ref();
                        fits(v.is_some());
                        if let Some((size, origin)) = v {
                            style.font_size = *size;
                            style.font_size_origin = *origin;
                        }
                        return;
                    }
                    _ => {}
                }
                match self {
                    $(LonghandId::$id => {
                        let v = value.downcast_ref();
                        fits(v.is_some());
                        if let Some(v) = v {
                            style.$field.clone_from(v);
                        }
                    })+
                    $(LonghandId::$iid)|+ => {
                        let v = value.downcast_ref();
                        fits(v.is_some());
                        if let Some(v) = v {
                            style.inert.clone_from(v);
                        }
                    }
                    $(LonghandId::$aid)|+ => {
                        let v = value.downcast_ref();
                        fits(v.is_some());
                        if let Some((a, t)) = v {
                            style.animations.clone_from(a);
                            style.transitions.clone_from(t);
                        }
                    }
                }
            }
        }

        /// The specified value of a longhand declaration.
        #[derive(Clone, Debug, PartialEq)]
        pub(crate) enum LonghandValue {
            $($id($spec),)+
            $($iid($ispec),)+
            $($aid($aspec)),+
        }

        impl LonghandValue {
            /// The property this value belongs to.
            pub(crate) fn id(&self) -> LonghandId {
                match self {
                    $(LonghandValue::$id(_) => LonghandId::$id,)+
                    $(LonghandValue::$iid(_) => LonghandId::$iid,)+
                    $(LonghandValue::$aid(_) => LonghandId::$aid),+
                }
            }
        }
    };
    (@inherited inherited) => { true };
    (@inherited reset) => { false };
}

longhands! {
    // Inherited.
    Color "color" inherited color: Color;
    ColorScheme "color-scheme" inherited color_scheme: Option<Arc<str>>;
    FontFamily "font-family" inherited font_family: Arc<[FontFamily]>;
    FontSize "font-size" inherited font_size: SpecifiedFontSize;
    FontWeight "font-weight" inherited font_weight: SpecifiedFontWeight;
    FontStyle "font-style" inherited font_style: FontStyle;
    FontStretch "font-stretch" inherited font_stretch: f32;
    FontVariantCaps "font-variant-caps" inherited font_variant_caps: FontVariantCaps;
    FontVariantLigatures "font-variant-ligatures" inherited font_variant_ligatures: FontVariantLigatures;
    FontVariantNumeric "font-variant-numeric" inherited font_variant_numeric: FontVariantNumeric;
    TapHighlightColor "-webkit-tap-highlight-color" inherited tap_highlight_color: Color;
    TextSizeAdjust "text-size-adjust" inherited text_size_adjust: TextSizeAdjust;
    FontSmoothing "-webkit-font-smoothing" inherited font_smoothing: FontSmoothing;
    LineBreak "line-break" inherited line_break: LineBreak;
    InterpolateSize "interpolate-size" inherited interpolate_size: InterpolateSize;
    FontVariationSettings "font-variation-settings" inherited font_variation_settings: FontVariationSettings;
    FontFeatureSettings "font-feature-settings" inherited font_feature_settings: FontFeatureSettings;
    LineHeight "line-height" inherited line_height: SpecifiedLineHeight;
    TextAlign "text-align" inherited text_align: SpecifiedTextAlign;
    TextIndent "text-indent" inherited text_indent: Lp;
    TextTransform "text-transform" inherited text_transform: TextTransform;
    WhiteSpaceCollapse "white-space-collapse" inherited white_space: SpaceCollapse;
    LetterSpacing "letter-spacing" inherited letter_spacing: Lp;
    WordSpacing "word-spacing" inherited word_spacing: Lp;
    WordBreak "word-break" inherited word_break: WordBreak;
    OverflowWrap "overflow-wrap" inherited overflow_wrap: OverflowWrap;
    Hyphens "hyphens" inherited hyphens: Hyphens;
    TabSize "tab-size" inherited tab_size: f32;
    Visibility "visibility" inherited visibility: Visibility;
    ListStyleType "list-style-type" inherited list_style_type: ListStyleType;
    ListStylePosition "list-style-position" inherited list_style_position: ListStylePosition;
    ListStyleImage "list-style-image" inherited list_style_image: Option<SpecifiedImage>;
    Cursor "cursor" inherited cursor: Cursor;
    Direction "direction" inherited direction: Direction;
    BorderCollapse "border-collapse" inherited border_collapse: BorderCollapse;
    BorderSpacingHorizontal "-swb-border-spacing-horizontal" inherited border_spacing_horizontal: Lp;
    BorderSpacingVertical "-swb-border-spacing-vertical" inherited border_spacing_vertical: Lp;
    CaptionSide "caption-side" inherited caption_side: CaptionSide;
    EmptyCells "empty-cells" inherited empty_cells: EmptyCells;
    PointerEvents "pointer-events" inherited pointer_events: PointerEvents;
    Fill "fill" inherited fill: SvgPaint;
    FillRule "fill-rule" inherited fill_rule: FillRule;
    ClipRule "clip-rule" inherited clip_rule: FillRule;
    ShapeRendering "shape-rendering" inherited shape_rendering: ShapeRendering;
    FillOpacity "fill-opacity" inherited fill_opacity: f32;
    Stroke "stroke" inherited stroke: SvgPaint;
    StrokeWidth "stroke-width" inherited stroke_width: Lp;
    StrokeLinecap "stroke-linecap" inherited stroke_linecap: StrokeLinecap;
    StrokeLinejoin "stroke-linejoin" inherited stroke_linejoin: StrokeLinejoin;
    StrokeMiterlimit "stroke-miterlimit" inherited stroke_miterlimit: f32;
    StrokeDasharray "stroke-dasharray" inherited stroke_dasharray: Arc<[Lp]>;
    StrokeDashoffset "stroke-dashoffset" inherited stroke_dashoffset: Lp;
    StrokeOpacity "stroke-opacity" inherited stroke_opacity: f32;
    // `white-space` is the shorthand of `white-space-collapse` and
    // `text-wrap-mode`. `ComputedStyle::white_space` holds both halves;
    // each longhand sets and copies only its half (`copy_value`, `compute.rs`).
    TextWrapMode "text-wrap-mode" inherited white_space: bool;
    TextWrapStyle "text-wrap-style" inherited text_wrap_style: TextWrapStyle;
    TextStrokeWidth "-webkit-text-stroke-width" inherited text_stroke_width: Lp;
    TextStrokeColor "-webkit-text-stroke-color" inherited text_stroke_color: Color;
    TextFillColor "-webkit-text-fill-color" inherited text_fill_color: Color;
    TextShadow "text-shadow" inherited text_shadow: SpecifiedShadows;

    // Not inherited.
    Display "display" reset display: Display;
    Position "position" reset position: Position;
    Float "float" reset float: Float;
    Clear "clear" reset clear: Clear;
    Top "top" reset top: Option<Lp>;
    Right "right" reset right: Option<Lp>;
    Bottom "bottom" reset bottom: Option<Lp>;
    Left "left" reset left: Option<Lp>;
    ZIndex "z-index" reset z_index: ZIndex;
    Transform "transform" reset transform: SpecifiedTransform;
    TransformOrigin "transform-origin" reset transform_origin: SpecifiedTransformOrigin;
    Clip "clip" reset clip: SpecifiedClip;
    ClipPath "clip-path" reset clip_path: SpecifiedClipPath;
    Width "width" reset width: SpecifiedSize;
    Height "height" reset height: SpecifiedSize;
    MinWidth "min-width" reset min_width: SpecifiedSize;
    MinHeight "min-height" reset min_height: SpecifiedSize;
    MaxWidth "max-width" reset max_width: SpecifiedSize;
    MaxHeight "max-height" reset max_height: SpecifiedSize;
    BoxSizing "box-sizing" reset box_sizing: BoxSizing;
    AspectRatio "aspect-ratio" reset aspect_ratio: AspectRatio;
    MarginTop "margin-top" reset margin_top: Option<Lp>;
    MarginRight "margin-right" reset margin_right: Option<Lp>;
    MarginBottom "margin-bottom" reset margin_bottom: Option<Lp>;
    MarginLeft "margin-left" reset margin_left: Option<Lp>;
    PaddingTop "padding-top" reset padding_top: Lp;
    PaddingRight "padding-right" reset padding_right: Lp;
    PaddingBottom "padding-bottom" reset padding_bottom: Lp;
    PaddingLeft "padding-left" reset padding_left: Lp;
    BorderTopWidth "border-top-width" reset border_top_width: Lp;
    BorderRightWidth "border-right-width" reset border_right_width: Lp;
    BorderBottomWidth "border-bottom-width" reset border_bottom_width: Lp;
    BorderLeftWidth "border-left-width" reset border_left_width: Lp;
    BorderTopStyle "border-top-style" reset border_top_style: BorderStyle;
    BorderRightStyle "border-right-style" reset border_right_style: BorderStyle;
    BorderBottomStyle "border-bottom-style" reset border_bottom_style: BorderStyle;
    BorderLeftStyle "border-left-style" reset border_left_style: BorderStyle;
    BorderTopColor "border-top-color" reset border_top_color: Color;
    BorderRightColor "border-right-color" reset border_right_color: Color;
    BorderBottomColor "border-bottom-color" reset border_bottom_color: Color;
    BorderLeftColor "border-left-color" reset border_left_color: Color;
    BorderTopLeftRadius "border-top-left-radius" reset border_top_left_radius: (Lp, Lp);
    BorderTopRightRadius "border-top-right-radius" reset border_top_right_radius: (Lp, Lp);
    BorderBottomRightRadius "border-bottom-right-radius" reset border_bottom_right_radius: (Lp, Lp);
    BorderBottomLeftRadius "border-bottom-left-radius" reset border_bottom_left_radius: (Lp, Lp);
    OutlineWidth "outline-width" reset outline_width: Lp;
    OutlineStyle "outline-style" reset outline_style: OutlineStyle;
    OutlineColor "outline-color" reset outline_color: Color;
    OutlineOffset "outline-offset" reset outline_offset: Lp;
    BackgroundColor "background-color" reset background_color: Color;
    BackgroundImage "background-image" reset background_image: Arc<[Option<SpecifiedImage>]>;
    BackgroundPositionX "background-position-x" reset background_position_x: Arc<[SpecifiedPosition]>;
    BackgroundPositionY "background-position-y" reset background_position_y: Arc<[SpecifiedPosition]>;
    BackgroundSize "background-size" reset background_size: Arc<[SpecifiedBackgroundSize]>;
    BackgroundRepeat "background-repeat" reset background_repeat: Arc<[(BackgroundRepeatKeyword, BackgroundRepeatKeyword)]>;
    BackgroundOrigin "background-origin" reset background_origin: Arc<[BackgroundBox]>;
    BackgroundClip "background-clip" reset background_clip: Arc<[BackgroundBox]>;
    BackgroundAttachment "background-attachment" reset background_attachment: Arc<[BackgroundAttachment]>;
    MaskImage "mask-image" reset mask_image: Arc<[SpecifiedMaskImage]>;
    MaskMode "mask-mode" reset mask_mode: Arc<[MaskMode]>;
    MaskPositionX "-webkit-mask-position-x" reset mask_position_x: Arc<[SpecifiedPosition]>;
    MaskPositionY "-webkit-mask-position-y" reset mask_position_y: Arc<[SpecifiedPosition]>;
    MaskSize "mask-size" reset mask_size: Arc<[SpecifiedBackgroundSize]>;
    MaskRepeat "mask-repeat" reset mask_repeat: Arc<[(BackgroundRepeatKeyword, BackgroundRepeatKeyword)]>;
    MaskOrigin "mask-origin" reset mask_origin: Arc<[BackgroundBox]>;
    MaskClip "mask-clip" reset mask_clip: Arc<[MaskClip]>;
    MaskComposite "mask-composite" reset mask_composite: Arc<[CompositeOperator]>;
    OverflowX "overflow-x" reset overflow_x: Overflow;
    OverflowY "overflow-y" reset overflow_y: Overflow;
    TextOverflow "text-overflow" reset text_overflow: TextOverflow;
    Opacity "opacity" reset opacity: f32;
    VerticalAlign "vertical-align" reset vertical_align: SpecifiedVerticalAlign;
    TextDecorationLine "text-decoration-line" reset text_decoration_line: TextDecorationLine;
    TextDecorationColor "text-decoration-color" reset text_decoration_color: Color;
    TextDecorationStyle "text-decoration-style" reset text_decoration_style: TextDecorationStyle;
    FlexDirection "flex-direction" reset flex_direction: FlexDirection;
    FlexWrap "flex-wrap" reset flex_wrap: FlexWrap;
    FlexGrow "flex-grow" reset flex_grow: f32;
    FlexShrink "flex-shrink" reset flex_shrink: f32;
    FlexBasis "flex-basis" reset flex_basis: SpecifiedFlexBasis;
    Order "order" reset order: i32;
    JustifyContent "justify-content" reset justify_content: Alignment;
    AlignItems "align-items" reset align_items: Alignment;
    AlignSelf "align-self" reset align_self: Alignment;
    AlignContent "align-content" reset align_content: Alignment;
    RowGap "row-gap" reset row_gap: Option<Lp>;
    ColumnGap "column-gap" reset column_gap: Option<Lp>;
    ColumnWidth "column-width" reset column_width: Option<Lp>;
    ColumnCount "column-count" reset column_count: Option<u32>;
    JustifyItems "justify-items" reset justify_items: Alignment;
    JustifySelf "justify-self" reset justify_self: Alignment;
    GridTemplateColumns "grid-template-columns" reset grid_template_columns: SpecifiedTrackList;
    GridTemplateRows "grid-template-rows" reset grid_template_rows: SpecifiedTrackList;
    GridTemplateAreas "grid-template-areas" reset grid_template_areas: Option<Arc<GridTemplateAreas>>;
    GridAutoColumns "grid-auto-columns" reset grid_auto_columns: Arc<[SpecifiedTrackSize]>;
    GridAutoRows "grid-auto-rows" reset grid_auto_rows: Arc<[SpecifiedTrackSize]>;
    GridAutoFlow "grid-auto-flow" reset grid_auto_flow: GridAutoFlow;
    GridRowStart "grid-row-start" reset grid_row_start: GridLine;
    GridRowEnd "grid-row-end" reset grid_row_end: GridLine;
    GridColumnStart "grid-column-start" reset grid_column_start: GridLine;
    GridColumnEnd "grid-column-end" reset grid_column_end: GridLine;
    TableLayout "table-layout" reset table_layout: TableLayout;
    Content "content" reset content: SpecifiedContent;
    CounterReset "counter-reset" reset counter_reset: CounterList;
    CounterIncrement "counter-increment" reset counter_increment: CounterList;
    CounterSet "counter-set" reset counter_set: CounterList;
    Contain "contain" reset contain: Contain;
    ContainerType "container-type" reset container_type: ContainerType;
    Appearance "appearance" reset appearance: Appearance;
    FieldSizing "field-sizing" reset field_sizing: FieldSizing;
    ContentVisibility "content-visibility" reset content_visibility: ContentVisibility;
    ContainerName "container-name" reset container_name: ContainerName;
    ContainIntrinsicWidth "contain-intrinsic-width" reset contain_intrinsic_width: SpecifiedContainIntrinsic;
    ContainIntrinsicHeight "contain-intrinsic-height" reset contain_intrinsic_height: SpecifiedContainIntrinsic;
    ObjectFit "object-fit" reset object_fit: ObjectFit;
    MixBlendMode "mix-blend-mode" reset mix_blend_mode: MixBlendMode;
    Resize "resize" reset resize: Resize;
    ScrollbarWidth "scrollbar-width" reset scrollbar_width: ScrollbarWidth;
    Overlay "overlay" reset overlay: Overlay;
    ObjectPosition "object-position" reset object_position: [SpecifiedPosition; 2];
    UserSelect "user-select" reset user_select: UserSelect;
    UnicodeBidi "unicode-bidi" reset unicode_bidi: UnicodeBidi;
    Filter "filter" reset filter: SpecifiedFilter;
    BackdropFilter "backdrop-filter" reset backdrop_filter: SpecifiedFilter;
    BoxShadow "box-shadow" reset box_shadow: SpecifiedShadows;
    Translate "translate" reset translate: SpecifiedTranslate;
    Rotate "rotate" reset rotate: Option<Rotate>;
    Scale "scale" reset scale: Option<Scale>;
    Isolation "isolation" reset isolation: Isolation;
    OverflowAnchor "overflow-anchor" reset overflow_anchor: OverflowAnchor;
    OverscrollBehaviorX "overscroll-behavior-x" reset overscroll_behavior_x: OverscrollBehavior;
    OverscrollBehaviorY "overscroll-behavior-y" reset overscroll_behavior_y: OverscrollBehavior;
    OverscrollBehaviorInline "overscroll-behavior-inline" reset overscroll_behavior_inline: OverscrollBehavior;
    OverscrollBehaviorBlock "overscroll-behavior-block" reset overscroll_behavior_block: OverscrollBehavior;
    WebkitLineClamp "-webkit-line-clamp" reset line_clamp: Option<i32>;
    WebkitBoxOrient "-webkit-box-orient" reset box_orient: BoxOrient;
    CornerTopLeftShape "corner-top-left-shape" reset corner_top_left_shape: CornerShape;
    CornerTopRightShape "corner-top-right-shape" reset corner_top_right_shape: CornerShape;
    CornerBottomRightShape "corner-bottom-right-shape" reset corner_bottom_right_shape: CornerShape;
    CornerBottomLeftShape "corner-bottom-left-shape" reset corner_bottom_left_shape: CornerShape;

    // Not inherited, without effect on layout or paint (the scroll margin
    // and padding only on scrolling to an element): the computed values are
    // in `ComputedStyle::inert` (see `inert.rs`).
    ;
    WillChange "will-change": Arc<str>;
    TouchAction "touch-action": Arc<str>;
    Perspective "perspective": Option<Lp>;
    PerspectiveOrigin "perspective-origin": [SpecifiedPosition; 2];
    ScrollMarginTop "scroll-margin-top": Lp;
    ScrollMarginRight "scroll-margin-right": Lp;
    ScrollMarginBottom "scroll-margin-bottom": Lp;
    ScrollMarginLeft "scroll-margin-left": Lp;
    ScrollPaddingTop "scroll-padding-top": Option<Lp>;
    ScrollPaddingRight "scroll-padding-right": Option<Lp>;
    ScrollPaddingBottom "scroll-padding-bottom": Option<Lp>;
    ScrollPaddingLeft "scroll-padding-left": Option<Lp>;

    // Not inherited: the animation properties, in one shared record
    // (`ComputedStyle::animations`, `animation/properties.rs`), and the
    // transition properties.
    ;
    AnimationName "animation-name": Arc<[AnimationName]>;
    AnimationDuration "animation-duration": Arc<[Option<Time>]>;
    AnimationTimingFunction "animation-timing-function": Arc<[Easing]>;
    AnimationDelay "animation-delay": Arc<[Time]>;
    AnimationIterationCount "animation-iteration-count": Arc<[IterationCount]>;
    AnimationDirection "animation-direction": Arc<[PlaybackDirection]>;
    AnimationFillMode "animation-fill-mode": Arc<[FillMode]>;
    AnimationPlayState "animation-play-state": Arc<[PlayState]>;
    AnimationComposition "animation-composition": Arc<[Composition]>;
    AnimationTimeline "animation-timeline": Arc<[Arc<str>]>;
    AnimationRangeStart "animation-range-start": Arc<[RangeItem]>;
    AnimationRangeEnd "animation-range-end": Arc<[RangeItem]>;
    // The transition properties, in their own shared record
    // (`ComputedStyle::transitions`, `animation/transition_properties.rs`).
    TransitionProperty "transition-property": Arc<[TransitionName]>;
    TransitionDuration "transition-duration": Arc<[Time]>;
    TransitionDelay "transition-delay": Arc<[Time]>;
    TransitionTimingFunction "transition-timing-function": Arc<[Easing]>;
    TransitionBehavior "transition-behavior": Arc<[bool]>;
}

impl LonghandId {
    /// True for the `animation-*` longhands.
    pub(crate) fn is_animation(self) -> bool {
        use LonghandId as L;
        matches!(
            self,
            L::AnimationName
                | L::AnimationDuration
                | L::AnimationTimingFunction
                | L::AnimationDelay
                | L::AnimationIterationCount
                | L::AnimationDirection
                | L::AnimationFillMode
                | L::AnimationPlayState
                | L::AnimationComposition
                | L::AnimationTimeline
                | L::AnimationRangeStart
                | L::AnimationRangeEnd
        )
    }

    /// True for the `animation-*` and `transition-*` longhands.
    pub(crate) fn is_animation_or_transition(self) -> bool {
        self.is_animation() || self.is_transition()
    }

    /// True for the `transition-*` longhands.
    pub(crate) fn is_transition(self) -> bool {
        use LonghandId as L;
        matches!(
            self,
            L::TransitionProperty
                | L::TransitionDuration
                | L::TransitionDelay
                | L::TransitionTimingFunction
                | L::TransitionBehavior
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for &id in LonghandId::ALL {
            assert_eq!(LonghandId::from_name(id.name()), Some(id));
        }
        assert_eq!(LonghandId::from_name("color"), Some(LonghandId::Color));
        assert!(LonghandId::Color.is_inherited());
        assert!(LonghandId::FontVariationSettings.is_inherited());
        assert!(LonghandId::FontFeatureSettings.is_inherited());
        assert!(!LonghandId::Display.is_inherited());
    }

    #[test]
    fn animation_and_transition_longhands_match_their_names() {
        for &id in LonghandId::ALL {
            assert_eq!(id.is_animation(), id.name().starts_with("animation-"));
            assert_eq!(id.is_transition(), id.name().starts_with("transition-"));
        }
    }
}
