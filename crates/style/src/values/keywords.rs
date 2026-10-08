//! Properties whose values are single keywords.

/// Defines an enum of CSS keywords with `from_ident` and `as_str`.
///
/// Each variant lists its CSS keyword and optional aliases. The last
/// argument names the initial value (used for `Default`).
macro_rules! keyword_enum {
    (
        $(#[$meta:meta])*
        $name:ident {
            $($variant:ident = $css:literal $(| $alias:literal)*),+ $(,)?
        }
        default $default:ident
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $name {
            $(
                #[doc = concat!("`", $css, "`")]
                $variant
            ),+
        }

        impl Default for $name {
            fn default() -> Self {
                Self::$default
            }
        }

        impl $name {
            /// Parses the keyword (ASCII case-insensitive).
            pub fn from_ident(ident: &str) -> Option<Self> {
                $(
                    if ident.eq_ignore_ascii_case($css)
                        $(|| ident.eq_ignore_ascii_case($alias))*
                    {
                        return Some(Self::$variant);
                    }
                )+
                None
            }

            /// The CSS keyword.
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $css),+
                }
            }
        }
    };
}

keyword_enum! {
    /// The `display` property. Two-value syntax is mapped to these values.
    Display {
        None = "none",
        Contents = "contents",
        Block = "block",
        Inline = "inline",
        InlineBlock = "inline-block",
        FlowRoot = "flow-root",
        ListItem = "list-item",
        Flex = "flex",
        InlineFlex = "inline-flex",
        Grid = "grid",
        InlineGrid = "inline-grid",
        Table = "table",
        InlineTable = "inline-table",
        TableRowGroup = "table-row-group",
        TableHeaderGroup = "table-header-group",
        TableFooterGroup = "table-footer-group",
        TableRow = "table-row",
        TableCell = "table-cell",
        TableColumnGroup = "table-column-group",
        TableColumn = "table-column",
        TableCaption = "table-caption",
    }
    default Inline
}

impl Display {
    /// True if the outer display type is inline (the box takes part in an
    /// inline formatting context).
    pub fn is_inline_level(self) -> bool {
        matches!(
            self,
            Display::Inline
                | Display::InlineBlock
                | Display::InlineFlex
                | Display::InlineGrid
                | Display::InlineTable
        )
    }

    /// True for the internal table display types (rows, cells, groups,
    /// columns, captions).
    pub fn is_table_internal(self) -> bool {
        matches!(
            self,
            Display::TableRowGroup
                | Display::TableHeaderGroup
                | Display::TableFooterGroup
                | Display::TableRow
                | Display::TableCell
                | Display::TableColumnGroup
                | Display::TableColumn
                | Display::TableCaption
        )
    }

    /// The "blockified" value: the display of a box that must be
    /// block-level (floats, absolutely positioned boxes, flex items, the
    /// root). <https://www.w3.org/TR/css-display-3/#blockify>
    #[must_use]
    pub fn blockify(self) -> Display {
        match self {
            Display::Inline
            | Display::InlineBlock
            | Display::TableRowGroup
            | Display::TableHeaderGroup
            | Display::TableFooterGroup
            | Display::TableRow
            | Display::TableCell
            | Display::TableColumnGroup
            | Display::TableColumn
            | Display::TableCaption => Display::Block,
            Display::InlineFlex => Display::Flex,
            Display::InlineGrid => Display::Grid,
            Display::InlineTable => Display::Table,
            other => other,
        }
    }
}

keyword_enum! {
    /// The `position` property.
    Position {
        Static = "static",
        Relative = "relative",
        Absolute = "absolute",
        Fixed = "fixed",
        Sticky = "sticky" | "-webkit-sticky",
    }
    default Static
}

impl Position {
    /// True for `absolute` and `fixed`.
    pub fn is_absolutely_positioned(self) -> bool {
        matches!(self, Position::Absolute | Position::Fixed)
    }
}

keyword_enum! {
    /// The `float` property.
    Float {
        None = "none",
        Left = "left" | "inline-start",
        Right = "right" | "inline-end",
    }
    default None
}

keyword_enum! {
    /// The `clear` property.
    Clear {
        None = "none",
        Left = "left" | "inline-start",
        Right = "right" | "inline-end",
        Both = "both",
    }
    default None
}

keyword_enum! {
    /// Border and outline styles.
    BorderStyle {
        None = "none",
        Hidden = "hidden",
        Dotted = "dotted",
        Dashed = "dashed",
        Solid = "solid",
        Double = "double",
        Groove = "groove",
        Ridge = "ridge",
        Inset = "inset",
        Outset = "outset",
    }
    default None
}

impl BorderStyle {
    /// True if a border with this style has zero computed width.
    pub fn has_no_width(self) -> bool {
        matches!(self, BorderStyle::None | BorderStyle::Hidden)
    }
}

keyword_enum! {
    /// The `outline-style` property: `auto` or a border style other than
    /// `hidden`.
    OutlineStyle {
        Auto = "auto",
        None = "none",
        Dotted = "dotted",
        Dashed = "dashed",
        Solid = "solid",
        Double = "double",
        Groove = "groove",
        Ridge = "ridge",
        Inset = "inset",
        Outset = "outset",
    }
    default None
}

impl OutlineStyle {
    /// The border style that draws the outline. `auto` (a focus ring) has
    /// none.
    pub fn border_style(self) -> Option<BorderStyle> {
        Some(match self {
            OutlineStyle::Auto => return None,
            OutlineStyle::None => BorderStyle::None,
            OutlineStyle::Dotted => BorderStyle::Dotted,
            OutlineStyle::Dashed => BorderStyle::Dashed,
            OutlineStyle::Solid => BorderStyle::Solid,
            OutlineStyle::Double => BorderStyle::Double,
            OutlineStyle::Groove => BorderStyle::Groove,
            OutlineStyle::Ridge => BorderStyle::Ridge,
            OutlineStyle::Inset => BorderStyle::Inset,
            OutlineStyle::Outset => BorderStyle::Outset,
        })
    }
}

keyword_enum! {
    /// The `text-align` property.
    TextAlign {
        Start = "start",
        End = "end",
        Left = "left",
        Right = "right",
        Center = "center",
        Justify = "justify",
        MatchParent = "match-parent",
        WebkitLeft = "-webkit-left",
        WebkitRight = "-webkit-right",
        WebkitCenter = "-webkit-center",
    }
    default Start
}

keyword_enum! {
    /// The `white-space` property (as a single keyword).
    WhiteSpace {
        Normal = "normal",
        Pre = "pre",
        Nowrap = "nowrap",
        PreWrap = "pre-wrap",
        PreLine = "pre-line",
        BreakSpaces = "break-spaces",
    }
    default Normal
}

impl WhiteSpace {
    /// True if sequences of white space collapse.
    pub fn collapses_spaces(self) -> bool {
        matches!(
            self,
            WhiteSpace::Normal | WhiteSpace::Nowrap | WhiteSpace::PreLine
        )
    }

    /// True if lines may wrap at soft wrap opportunities.
    pub fn wraps(self) -> bool {
        !matches!(self, WhiteSpace::Pre | WhiteSpace::Nowrap)
    }
}

keyword_enum! {
    /// The `font-style` property.
    FontStyle {
        Normal = "normal",
        Italic = "italic",
        Oblique = "oblique",
    }
    default Normal
}

keyword_enum! {
    /// The `visibility` property.
    Visibility {
        Visible = "visible",
        Hidden = "hidden",
        Collapse = "collapse",
    }
    default Visible
}

keyword_enum! {
    /// The `overflow-x` and `overflow-y` properties.
    Overflow {
        Visible = "visible",
        Hidden = "hidden",
        Clip = "clip",
        Scroll = "scroll",
        Auto = "auto" | "overlay",
    }
    default Visible
}

impl Overflow {
    /// True if the box clips its content (anything but `visible`).
    pub fn clips(self) -> bool {
        self != Overflow::Visible
    }

    /// True if a box with this overflow value is a scroll container
    /// (`hidden`, `scroll`, `auto`; not `clip`). Scroll containers
    /// establish a block formatting context and have no automatic minimum
    /// size or line-box baseline.
    /// <https://www.w3.org/TR/css-overflow-3/#scroll-container>
    pub fn is_scroll_container(self) -> bool {
        matches!(self, Overflow::Hidden | Overflow::Scroll | Overflow::Auto)
    }
}

keyword_enum! {
    /// The `box-sizing` property.
    BoxSizing {
        ContentBox = "content-box",
        BorderBox = "border-box",
    }
    default ContentBox
}

keyword_enum! {
    /// The `text-transform` property.
    TextTransform {
        None = "none",
        Capitalize = "capitalize",
        Uppercase = "uppercase",
        Lowercase = "lowercase",
        FullWidth = "full-width",
    }
    default None
}

keyword_enum! {
    /// The `list-style-type` property (keyword values only).
    ListStyleType {
        Disc = "disc",
        Circle = "circle",
        Square = "square",
        Decimal = "decimal",
        DecimalLeadingZero = "decimal-leading-zero",
        LowerRoman = "lower-roman",
        UpperRoman = "upper-roman",
        LowerAlpha = "lower-alpha" | "lower-latin",
        UpperAlpha = "upper-alpha" | "upper-latin",
        LowerGreek = "lower-greek",
        DisclosureOpen = "disclosure-open",
        DisclosureClosed = "disclosure-closed",
        None = "none",
    }
    default Disc
}

keyword_enum! {
    /// The `list-style-position` property.
    ListStylePosition {
        Outside = "outside",
        Inside = "inside",
    }
    default Outside
}

keyword_enum! {
    /// The `direction` property.
    Direction {
        Ltr = "ltr",
        Rtl = "rtl",
    }
    default Ltr
}

keyword_enum! {
    /// The `border-collapse` property.
    BorderCollapse {
        Separate = "separate",
        Collapse = "collapse",
    }
    default Separate
}

keyword_enum! {
    /// The `table-layout` property.
    TableLayout {
        Auto = "auto",
        Fixed = "fixed",
    }
    default Auto
}

keyword_enum! {
    /// The `caption-side` property.
    CaptionSide {
        Top = "top",
        Bottom = "bottom",
    }
    default Top
}

keyword_enum! {
    /// The `empty-cells` property.
    EmptyCells {
        Show = "show",
        Hide = "hide",
    }
    default Show
}

keyword_enum! {
    /// The `flex-direction` property.
    FlexDirection {
        Row = "row",
        RowReverse = "row-reverse",
        Column = "column",
        ColumnReverse = "column-reverse",
    }
    default Row
}

impl FlexDirection {
    /// True for `row` and `row-reverse`.
    pub fn is_row(self) -> bool {
        matches!(self, FlexDirection::Row | FlexDirection::RowReverse)
    }

    /// True for the reversed directions.
    pub fn is_reverse(self) -> bool {
        matches!(
            self,
            FlexDirection::RowReverse | FlexDirection::ColumnReverse
        )
    }
}

keyword_enum! {
    /// The `flex-wrap` property.
    FlexWrap {
        Nowrap = "nowrap",
        Wrap = "wrap",
        WrapReverse = "wrap-reverse",
    }
    default Nowrap
}

keyword_enum! {
    /// Box alignment keywords, shared by `justify-content`, `align-items`,
    /// `align-self`, `align-content`, `justify-items` and `justify-self`.
    /// `first baseline` and `last baseline` are mapped to `baseline`.
    Alignment {
        Auto = "auto",
        Normal = "normal",
        Stretch = "stretch",
        Start = "start",
        End = "end",
        FlexStart = "flex-start",
        FlexEnd = "flex-end",
        SelfStart = "self-start",
        SelfEnd = "self-end",
        Center = "center",
        Left = "left",
        Right = "right",
        Baseline = "baseline",
        SpaceBetween = "space-between",
        SpaceAround = "space-around",
        SpaceEvenly = "space-evenly",
    }
    default Normal
}

keyword_enum! {
    /// The `text-decoration-style` property.
    TextDecorationStyle {
        Solid = "solid",
        Double = "double",
        Dotted = "dotted",
        Dashed = "dashed",
        Wavy = "wavy",
    }
    default Solid
}

keyword_enum! {
    /// The `cursor` property (keyword values only).
    Cursor {
        Auto = "auto",
        Default = "default",
        None = "none",
        Pointer = "pointer",
        Text = "text",
        VerticalText = "vertical-text",
        Help = "help",
        Wait = "wait",
        Progress = "progress",
        Crosshair = "crosshair",
        Move = "move",
        Grab = "grab",
        Grabbing = "grabbing",
        NotAllowed = "not-allowed",
        NoDrop = "no-drop",
        ContextMenu = "context-menu",
        Cell = "cell",
        Copy = "copy",
        Alias = "alias",
        ColResize = "col-resize",
        RowResize = "row-resize",
        EwResize = "ew-resize",
        NsResize = "ns-resize",
        NeswResize = "nesw-resize",
        NwseResize = "nwse-resize",
        NResize = "n-resize",
        EResize = "e-resize",
        SResize = "s-resize",
        WResize = "w-resize",
        NeResize = "ne-resize",
        NwResize = "nw-resize",
        SeResize = "se-resize",
        SwResize = "sw-resize",
        AllScroll = "all-scroll",
        ZoomIn = "zoom-in",
        ZoomOut = "zoom-out",
    }
    default Auto
}

keyword_enum! {
    /// The `word-break` property.
    WordBreak {
        Normal = "normal",
        BreakAll = "break-all",
        KeepAll = "keep-all",
        BreakWord = "break-word",
    }
    default Normal
}

keyword_enum! {
    /// The `overflow-wrap` (and legacy `word-wrap`) property.
    OverflowWrap {
        Normal = "normal",
        BreakWord = "break-word",
        Anywhere = "anywhere",
    }
    default Normal
}

keyword_enum! {
    /// The `text-overflow` property (single-value form).
    TextOverflow {
        Clip = "clip",
        Ellipsis = "ellipsis",
    }
    default Clip
}

keyword_enum! {
    /// The `object-fit` property.
    ObjectFit {
        Fill = "fill",
        Contain = "contain",
        Cover = "cover",
        None = "none",
        ScaleDown = "scale-down",
    }
    default Fill
}

keyword_enum! {
    /// The `pointer-events` property. The values other than `auto` and
    /// `none` are for SVG shapes (SVG 2 §16.4); on HTML content they
    /// behave as `auto` (swb does not use the property for HTML yet).
    PointerEvents {
        Auto = "auto",
        None = "none",
        VisiblePainted = "visiblepainted",
        VisibleFill = "visiblefill",
        VisibleStroke = "visiblestroke",
        Visible = "visible",
        Painted = "painted",
        Fill = "fill",
        Stroke = "stroke",
        All = "all",
    }
    default Auto
}

keyword_enum! {
    /// The `shape-rendering` property (SVG 2 §13.4.1). `optimizeSpeed`
    /// and `crispEdges` draw without anti-aliasing (measured in
    /// Chromium 148); `auto` and `geometricPrecision` are anti-aliased.
    ShapeRendering {
        Auto = "auto",
        OptimizeSpeed = "optimizespeed",
        CrispEdges = "crispedges",
        GeometricPrecision = "geometricprecision",
    }
    default Auto
}

keyword_enum! {
    /// The `user-select` property.
    UserSelect {
        Auto = "auto",
        Text = "text",
        None = "none",
        All = "all",
        Contain = "contain",
    }
    default Auto
}

keyword_enum! {
    /// A `background-repeat` keyword for one axis, or a shorthand form.
    BackgroundRepeatKeyword {
        Repeat = "repeat",
        RepeatX = "repeat-x",
        RepeatY = "repeat-y",
        NoRepeat = "no-repeat",
        Space = "space",
        Round = "round",
    }
    default Repeat
}

keyword_enum! {
    /// `background-clip` and `background-origin` boxes.
    BackgroundBox {
        BorderBox = "border-box",
        PaddingBox = "padding-box",
        ContentBox = "content-box",
    }
    default BorderBox
}

keyword_enum! {
    /// The `background-attachment` property.
    BackgroundAttachment {
        Scroll = "scroll",
        Fixed = "fixed",
        Local = "local",
    }
    default Scroll
}

keyword_enum! {
    /// The `font-variant-caps` property (and the CSS 2 `font-variant`).
    FontVariantCaps {
        Normal = "normal",
        SmallCaps = "small-caps",
        AllSmallCaps = "all-small-caps",
        PetiteCaps = "petite-caps",
        AllPetiteCaps = "all-petite-caps",
        Unicase = "unicase",
        TitlingCaps = "titling-caps",
    }
    default Normal
}

keyword_enum! {
    /// The `unicode-bidi` property.
    UnicodeBidi {
        Normal = "normal",
        Embed = "embed",
        Isolate = "isolate",
        BidiOverride = "bidi-override",
        IsolateOverride = "isolate-override",
        Plaintext = "plaintext",
    }
    default Normal
}

keyword_enum! {
    /// The `hyphens` property.
    Hyphens {
        None = "none",
        Manual = "manual",
        Auto = "auto",
    }
    default Manual
}

keyword_enum! {
    /// The `vertical-align` keyword values.
    VerticalAlignKeyword {
        Baseline = "baseline",
        Sub = "sub",
        Super = "super",
        TextTop = "text-top",
        TextBottom = "text-bottom",
        Middle = "middle",
        Top = "top",
        Bottom = "bottom",
    }
    default Baseline
}

keyword_enum! {
    /// An absolute-size keyword of `font-size`.
    /// <https://www.w3.org/TR/css-fonts-4/#absolute-size-value>
    FontSizeKeyword {
        XxSmall = "xx-small",
        XSmall = "x-small",
        Small = "small",
        Medium = "medium",
        Large = "large",
        XLarge = "x-large",
        XxLarge = "xx-large",
        XxxLarge = "xxx-large" | "-webkit-xxx-large",
    }
    default Medium
}

/// Font sizes in px for the absolute-size keywords, `xx-small` to
/// `xxx-large`, for a medium size of 16px (proportional fonts) and 13px
/// (monospace), in standards mode and in quirks mode. The values are the
/// computed font sizes in Chromium 148 (`swbtools measure
/// font-size-keywords`, docs/testing.md). With 16px, quirks mode has the
/// same sizes as standards mode. They differ from the scale factors in
/// CSS Fonts 4 (Chromium computes `small` as 13px, not 14.2px).
const KEYWORD_SIZES_16: [f32; 8] = [9.0, 10.0, 13.0, 16.0, 18.0, 24.0, 32.0, 48.0];
const KEYWORD_SIZES_13_STRICT: [f32; 8] = [9.0, 10.0, 12.0, 13.0, 16.0, 20.0, 26.0, 39.0];
const KEYWORD_SIZES_13_QUIRKS: [f32; 8] = [9.0, 9.0, 10.0, 13.0, 16.0, 20.0, 26.0, 40.0];

impl FontSizeKeyword {
    /// The keyword for a legacy HTML font size (`<font size>`), 1 to 7.
    /// <https://html.spec.whatwg.org/multipage/rendering.html#rules-for-parsing-a-legacy-font-size>
    pub fn from_legacy_size(size: i32) -> Self {
        match size.clamp(1, 7) {
            1 => Self::XSmall,
            2 => Self::Small,
            3 => Self::Medium,
            4 => Self::Large,
            5 => Self::XLarge,
            6 => Self::XxLarge,
            _ => Self::XxxLarge,
        }
    }

    /// The font size in px. `monospace` selects the 13px medium size used
    /// for the `monospace` generic family; `quirks` selects the quirks mode
    /// table.
    pub fn to_px(self, monospace: bool, quirks: bool) -> f32 {
        let index = self as usize;
        let table = match (monospace, quirks) {
            (false, _) => &KEYWORD_SIZES_16,
            (true, false) => &KEYWORD_SIZES_13_STRICT,
            (true, true) => &KEYWORD_SIZES_13_QUIRKS,
        };
        table.get(index).copied().unwrap_or(16.0)
    }
}

/// How a computed font size was derived. The monospace font size quirk
/// needs it: see [`crate::ComputedStyle::font_size_origin`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontSizeOrigin {
    /// An absolute-size keyword (`medium` is the initial value).
    Keyword(FontSizeKeyword),
    /// A size relative (`em`, `%`, `larger`, `smaller`) to a size that
    /// came from a keyword.
    RelativeToKeyword,
    /// An absolute length (`px`, `pt`, `rem`, `calc()`), or a size relative
    /// to one.
    Absolute,
}

impl Default for FontSizeOrigin {
    fn default() -> Self {
        FontSizeOrigin::Keyword(FontSizeKeyword::Medium)
    }
}

keyword_enum! {
    /// The `fill-rule` property (SVG 2 §13.4.2).
    FillRule {
        NonZero = "nonzero",
        EvenOdd = "evenodd",
    }
    default NonZero
}

keyword_enum! {
    /// The `stroke-linecap` property (SVG 2 §13.5.4).
    StrokeLinecap {
        Butt = "butt",
        Round = "round",
        Square = "square",
    }
    default Butt
}

keyword_enum! {
    /// The `stroke-linejoin` property (SVG 2 §13.5.5). `miter-clip` and
    /// `arcs` are not supported (invalid), as in Chromium 148.
    StrokeLinejoin {
        Miter = "miter",
        Round = "round",
        Bevel = "bevel",
    }
    default Miter
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_print() {
        assert_eq!(
            Display::from_ident("INLINE-block"),
            Some(Display::InlineBlock)
        );
        assert_eq!(Display::InlineBlock.as_str(), "inline-block");
        assert_eq!(Float::from_ident("inline-start"), Some(Float::Left));
        assert_eq!(Overflow::from_ident("overlay"), Some(Overflow::Auto));
        assert_eq!(Display::from_ident("bogus"), None);
        assert_eq!(Display::default(), Display::Inline);
    }

    #[test]
    fn blockify() {
        assert_eq!(Display::Inline.blockify(), Display::Block);
        assert_eq!(Display::InlineFlex.blockify(), Display::Flex);
        assert_eq!(Display::TableCell.blockify(), Display::Block);
        assert_eq!(Display::ListItem.blockify(), Display::ListItem);
    }
}
