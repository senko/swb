//! The list of supported longhand properties.
//!
//! The `longhands!` macro generates [`LonghandId`] with its name lookup,
//! the "inherited" flag and a function that copies the computed value
//! between two styles (used for inheritance and for `inherit`/`initial`).
//! It also generates [`LonghandValue`], the parsed (specified) value of a
//! declaration of each longhand.

use std::sync::Arc;

use super::mask::SpecifiedMaskImage;
use super::specified::{
    SpecifiedBackgroundSize, SpecifiedContent, SpecifiedFlexBasis, SpecifiedFontSize,
    SpecifiedFontWeight, SpecifiedLineHeight, SpecifiedPosition, SpecifiedSize, SpecifiedTextAlign,
    SpecifiedVerticalAlign,
};
use super::transform::{SpecifiedClip, SpecifiedTransform, SpecifiedTransformOrigin};
use crate::ComputedStyle;
use crate::parse::image::SpecifiedImage;
use crate::values::{
    Alignment, AspectRatio, BackgroundAttachment, BackgroundBox, BackgroundRepeatKeyword,
    BorderCollapse, BorderStyle, BoxSizing, CaptionSide, Clear, Color, CompositeOperator,
    CounterList, Cursor, Direction, Display, EmptyCells, FlexDirection, FlexWrap, Float,
    FontFamily, FontStyle, FontVariantCaps, GridAutoFlow, GridLine, GridTemplateAreas, Hyphens,
    ListStylePosition, ListStyleType, MaskClip, MaskMode, ObjectFit, OutlineStyle, Overflow,
    OverflowWrap, PointerEvents, Position, SpecifiedLengthPercentage as Lp, SpecifiedTrackList,
    SpecifiedTrackSize, TableLayout, TextDecorationLine, TextDecorationStyle, TextOverflow,
    TextTransform, UnicodeBidi, UserSelect, Visibility, WhiteSpace, WordBreak, ZIndex,
};

macro_rules! longhands {
    ($($id:ident $name:literal $kind:ident $field:ident: $spec:ty;)+) => {
        /// A longhand property.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub(crate) enum LonghandId {
            $($id),+
        }

        impl LonghandId {
            /// All longhands, in declaration order.
            pub(crate) const ALL: &'static [LonghandId] = &[$(LonghandId::$id),+];

            /// The number of longhands.
            pub(crate) const COUNT: usize = Self::ALL.len();

            /// Looks up a longhand by its CSS name (already lowercase).
            pub(crate) fn from_name(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(LonghandId::$id),)+
                    _ => None,
                }
            }

            /// The CSS name.
            #[cfg(test)]
            fn name(self) -> &'static str {
                match self {
                    $(LonghandId::$id => $name),+
                }
            }

            /// True if the property is inherited by default.
            pub(crate) fn is_inherited(self) -> bool {
                match self {
                    $(LonghandId::$id => longhands!(@inherited $kind)),+
                }
            }

            /// Copies this property's computed value from `from` to `to`.
            pub(crate) fn copy_value(self, from: &ComputedStyle, to: &mut ComputedStyle) {
                match self {
                    $(LonghandId::$id => to.$field.clone_from(&from.$field)),+
                }
                if self == LonghandId::FontSize {
                    to.font_size_origin = from.font_size_origin;
                }
            }
        }

        /// The specified value of a longhand declaration.
        #[derive(Clone, Debug, PartialEq)]
        pub(crate) enum LonghandValue {
            $($id($spec)),+
        }

        impl LonghandValue {
            /// The property this value belongs to.
            pub(crate) fn id(&self) -> LonghandId {
                match self {
                    $(LonghandValue::$id(_) => LonghandId::$id),+
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
    FontFamily "font-family" inherited font_family: Arc<[FontFamily]>;
    FontSize "font-size" inherited font_size: SpecifiedFontSize;
    FontWeight "font-weight" inherited font_weight: SpecifiedFontWeight;
    FontStyle "font-style" inherited font_style: FontStyle;
    FontStretch "font-stretch" inherited font_stretch: f32;
    FontVariantCaps "font-variant-caps" inherited font_variant_caps: FontVariantCaps;
    LineHeight "line-height" inherited line_height: SpecifiedLineHeight;
    TextAlign "text-align" inherited text_align: SpecifiedTextAlign;
    TextIndent "text-indent" inherited text_indent: Lp;
    TextTransform "text-transform" inherited text_transform: TextTransform;
    WhiteSpace "white-space" inherited white_space: WhiteSpace;
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
    ObjectFit "object-fit" reset object_fit: ObjectFit;
    ObjectPosition "object-position" reset object_position: [SpecifiedPosition; 2];
    UserSelect "user-select" reset user_select: UserSelect;
    UnicodeBidi "unicode-bidi" reset unicode_bidi: UnicodeBidi;
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
        assert!(!LonghandId::Display.is_inherited());
    }
}
