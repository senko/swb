//! The properties of CSSOM declaration blocks: their names as Chromium
//! gives them (logical properties stay apart from the physical ones;
//! legacy and prefixed aliases map to the standard name), how each
//! longhand's value is written, which longhands each shorthand sets, and
//! the names of the IDL attributes of `CSSStyleDeclaration` (CSSOM §6.7.1,
//! the camel-cased, webkit-cased and dashed attributes).
//!
//! The values are parsed with swb's own property parsers (by name), so a
//! property exists here only if swb parses it.

use std::collections::HashMap;
use std::sync::LazyLock;

use crate::properties::{LonghandId, is_known_property};

/// How a longhand's value is written (see `longhand.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ser {
    /// Keywords and numbers.
    Plain,
    /// Lengths: a bare number is `px`.
    Length,
    /// A color.
    Color,
    /// `font-family`.
    FontFamily,
    /// `transform-origin`, `object-position`: two keywords in x, y order.
    Position,
    /// `background-repeat`, `mask-repeat`.
    Repeat,
    /// `background-size`, `mask-size`.
    BackgroundSize,
    /// `aspect-ratio`.
    AspectRatio,
    /// `counter-reset`, `counter-set` (default 0) and `counter-increment`
    /// (default 1).
    Counter(i32),
    /// `font-feature-settings`.
    FontFeatures,
    /// A corner radius.
    Radius,
    /// `grid-template-areas`.
    GridAreas,
    /// `stroke-dasharray`.
    Dasharray,
    /// `z-index`.
    Integer,
    /// `opacity` and the SVG opacities: a percentage is a number.
    Opacity,
    /// `display`.
    Display,
    /// `text-decoration-line`.
    DecorationLine,
    /// `contain`.
    Contain,
    /// `justify-items`.
    JustifyItems,
    /// `align-items`, `align-self`, `justify-self`, `align-content`.
    Alignment,
    /// A grid line (`grid-row-start` ...).
    GridLine,
    /// `grid-auto-flow`.
    GridAutoFlow,
    /// `content`: strings, `attr()` and counters.
    Content,
}

/// A longhand of a declaration block.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Longhand {
    /// The name.
    pub(crate) name: &'static str,
    /// How the value is written.
    pub(crate) ser: Ser,
    /// False for longhands that only a shorthand sets (swb does not parse
    /// them on their own: `border-image-*` from `border`, the spacings of
    /// `border-spacing`).
    pub(crate) settable: bool,
}

/// How a shorthand's value goes to its longhands (`shorthand.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// One to four values for the sides (top, right, bottom, left).
    Sides,
    /// One or two values (the second defaults to the first).
    Pair,
    /// `border-radius`.
    Radius,
    /// Width, style and color in any order, for one or more sides
    /// (`border-top`, `border`, `border-block`); `border` also resets
    /// `border-image`.
    BorderSide,
    /// Components in any order, each to the first longhand that takes it
    /// (`outline`, `list-style`, `text-decoration`, `flex-flow`,
    /// `columns`).
    AnyOrder,
    /// `flex`.
    Flex,
    /// `font`.
    Font,
    /// `background` (one layer) and `background-position`,
    /// `mask-position`.
    Background,
    /// The position shorthands: x and y.
    Position,
    /// `grid-row`, `grid-column`, `grid-area`.
    GridLines,
    /// One longhand under another name (`font-variant`,
    /// `-webkit-mask-origin`, ...).
    Single,
    /// `border-image` (only `initial` and the other CSS-wide keywords).
    BorderImage,
    /// Kept as written: the longhands read as empty strings, as for a
    /// value with `var()` (`grid`, `grid-template`, `mask`).
    Opaque,
}

/// A shorthand.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Shorthand {
    /// The name.
    pub(crate) name: &'static str,
    /// The longhands, in Chromium's order.
    pub(crate) longhands: &'static [&'static str],
    /// How values go to the longhands.
    pub(crate) kind: Kind,
}

/// What a name is.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Property {
    /// A longhand (index into the table).
    Longhand(usize),
    /// A shorthand (index into the table).
    Shorthand(usize),
}

/// The longhands that swb parses besides its own (`LonghandId`): logical
/// properties, which Chromium keeps apart from the physical ones (swb's
/// cascade maps them), and properties that swb accepts and ignores.
const EXTRA_LONGHANDS: &[(&str, Ser)] = &[
    ("margin-block-start", Ser::Length),
    ("margin-block-end", Ser::Length),
    ("margin-inline-start", Ser::Length),
    ("margin-inline-end", Ser::Length),
    ("padding-block-start", Ser::Length),
    ("padding-block-end", Ser::Length),
    ("padding-inline-start", Ser::Length),
    ("padding-inline-end", Ser::Length),
    ("inset-block-start", Ser::Length),
    ("inset-block-end", Ser::Length),
    ("inset-inline-start", Ser::Length),
    ("inset-inline-end", Ser::Length),
    ("border-block-start-width", Ser::Length),
    ("border-block-end-width", Ser::Length),
    ("border-inline-start-width", Ser::Length),
    ("border-inline-end-width", Ser::Length),
    ("border-block-start-style", Ser::Plain),
    ("border-block-end-style", Ser::Plain),
    ("border-inline-start-style", Ser::Plain),
    ("border-inline-end-style", Ser::Plain),
    ("border-block-start-color", Ser::Color),
    ("border-block-end-color", Ser::Color),
    ("border-inline-start-color", Ser::Color),
    ("border-inline-end-color", Ser::Color),
    ("border-start-start-radius", Ser::Radius),
    ("border-start-end-radius", Ser::Radius),
    ("border-end-start-radius", Ser::Radius),
    ("border-end-end-radius", Ser::Radius),
    ("inline-size", Ser::Length),
    ("block-size", Ser::Length),
    ("min-inline-size", Ser::Length),
    ("min-block-size", Ser::Length),
    ("max-inline-size", Ser::Length),
    ("max-block-size", Ser::Length),
    ("contain-intrinsic-inline-size", Ser::Length),
    ("contain-intrinsic-block-size", Ser::Length),
    ("overflow-inline", Ser::Plain),
    ("overflow-block", Ser::Plain),
    ("text-decoration-thickness", Ser::Length),
    ("text-underline-offset", Ser::Length),
    ("text-underline-position", Ser::Plain),
    ("text-decoration-skip-ink", Ser::Plain),
    ("mask-type", Ser::Plain),
];

/// Longhands that only shorthands set.
const SHORTHAND_ONLY: &[(&str, Ser)] = &[
    ("-webkit-border-horizontal-spacing", Ser::Length),
    ("-webkit-border-vertical-spacing", Ser::Length),
    ("border-image-source", Ser::Plain),
    ("border-image-slice", Ser::Plain),
    ("border-image-width", Ser::Plain),
    ("border-image-outset", Ser::Plain),
    ("border-image-repeat", Ser::Plain),
];

/// Legacy and prefixed names: (alias, standard name).
const ALIASES: &[(&str, &str)] = &[
    ("word-wrap", "overflow-wrap"),
    ("font-width", "font-stretch"),
    ("-webkit-box-sizing", "box-sizing"),
    ("-moz-box-sizing", "box-sizing"),
    ("-webkit-user-select", "user-select"),
    ("-moz-user-select", "user-select"),
    ("-ms-user-select", "user-select"),
    ("-webkit-hyphens", "hyphens"),
    ("-ms-hyphens", "hyphens"),
    ("-moz-hyphens", "hyphens"),
    ("-webkit-flex-direction", "flex-direction"),
    ("-webkit-flex-wrap", "flex-wrap"),
    ("-webkit-flex-grow", "flex-grow"),
    ("-webkit-flex-shrink", "flex-shrink"),
    ("-webkit-flex-basis", "flex-basis"),
    ("-webkit-order", "order"),
    ("-webkit-justify-content", "justify-content"),
    ("-webkit-align-items", "align-items"),
    ("-webkit-align-self", "align-self"),
    ("-webkit-align-content", "align-content"),
    ("grid-row-gap", "row-gap"),
    ("grid-column-gap", "column-gap"),
    ("-webkit-border-top-left-radius", "border-top-left-radius"),
    ("-webkit-border-top-right-radius", "border-top-right-radius"),
    (
        "-webkit-border-bottom-left-radius",
        "border-bottom-left-radius",
    ),
    (
        "-webkit-border-bottom-right-radius",
        "border-bottom-right-radius",
    ),
    ("-webkit-mask-image", "mask-image"),
    ("-webkit-mask-size", "mask-size"),
    ("-webkit-mask-repeat", "mask-repeat"),
    ("-webkit-transform", "transform"),
    ("-webkit-transform-origin", "transform-origin"),
    ("grid-gap", "gap"),
    ("-webkit-flex", "flex"),
    ("-webkit-flex-flow", "flex-flow"),
    ("-webkit-border-radius", "border-radius"),
    ("-moz-border-radius", "border-radius"),
    ("-webkit-mask", "mask"),
    ("-webkit-mask-position", "mask-position"),
    ("-webkit-mask-origin", "mask-origin"),
    ("-webkit-mask-clip", "mask-clip"),
    ("-webkit-mask-composite", "mask-composite"),
];

const BORDER_IMAGE: &[&str] = &[
    "border-image-source",
    "border-image-slice",
    "border-image-width",
    "border-image-outset",
    "border-image-repeat",
];

/// The shorthands, with Chromium's longhands where swb parses them.
const SHORTHANDS: &[Shorthand] = &[
    sh(
        "margin",
        &["margin-top", "margin-right", "margin-bottom", "margin-left"],
        Kind::Sides,
    ),
    sh(
        "margin-block",
        &["margin-block-start", "margin-block-end"],
        Kind::Pair,
    ),
    sh(
        "margin-inline",
        &["margin-inline-start", "margin-inline-end"],
        Kind::Pair,
    ),
    sh(
        "padding",
        &[
            "padding-top",
            "padding-right",
            "padding-bottom",
            "padding-left",
        ],
        Kind::Sides,
    ),
    sh(
        "padding-block",
        &["padding-block-start", "padding-block-end"],
        Kind::Pair,
    ),
    sh(
        "padding-inline",
        &["padding-inline-start", "padding-inline-end"],
        Kind::Pair,
    ),
    sh("inset", &["top", "right", "bottom", "left"], Kind::Sides),
    sh(
        "inset-block",
        &["inset-block-start", "inset-block-end"],
        Kind::Pair,
    ),
    sh(
        "inset-inline",
        &["inset-inline-start", "inset-inline-end"],
        Kind::Pair,
    ),
    sh(
        "border",
        &[
            "border-top-width",
            "border-right-width",
            "border-bottom-width",
            "border-left-width",
            "border-top-style",
            "border-right-style",
            "border-bottom-style",
            "border-left-style",
            "border-top-color",
            "border-right-color",
            "border-bottom-color",
            "border-left-color",
            "border-image-source",
            "border-image-slice",
            "border-image-width",
            "border-image-outset",
            "border-image-repeat",
        ],
        Kind::BorderSide,
    ),
    sh(
        "border-top",
        &["border-top-width", "border-top-style", "border-top-color"],
        Kind::BorderSide,
    ),
    sh(
        "border-right",
        &[
            "border-right-width",
            "border-right-style",
            "border-right-color",
        ],
        Kind::BorderSide,
    ),
    sh(
        "border-bottom",
        &[
            "border-bottom-width",
            "border-bottom-style",
            "border-bottom-color",
        ],
        Kind::BorderSide,
    ),
    sh(
        "border-left",
        &[
            "border-left-width",
            "border-left-style",
            "border-left-color",
        ],
        Kind::BorderSide,
    ),
    sh(
        "border-block-start",
        &[
            "border-block-start-width",
            "border-block-start-style",
            "border-block-start-color",
        ],
        Kind::BorderSide,
    ),
    sh(
        "border-block-end",
        &[
            "border-block-end-width",
            "border-block-end-style",
            "border-block-end-color",
        ],
        Kind::BorderSide,
    ),
    sh(
        "border-inline-start",
        &[
            "border-inline-start-width",
            "border-inline-start-style",
            "border-inline-start-color",
        ],
        Kind::BorderSide,
    ),
    sh(
        "border-inline-end",
        &[
            "border-inline-end-width",
            "border-inline-end-style",
            "border-inline-end-color",
        ],
        Kind::BorderSide,
    ),
    sh(
        "border-block",
        &[
            "border-block-start-width",
            "border-block-end-width",
            "border-block-start-style",
            "border-block-end-style",
            "border-block-start-color",
            "border-block-end-color",
        ],
        Kind::BorderSide,
    ),
    sh(
        "border-inline",
        &[
            "border-inline-start-width",
            "border-inline-end-width",
            "border-inline-start-style",
            "border-inline-end-style",
            "border-inline-start-color",
            "border-inline-end-color",
        ],
        Kind::BorderSide,
    ),
    sh(
        "border-width",
        &[
            "border-top-width",
            "border-right-width",
            "border-bottom-width",
            "border-left-width",
        ],
        Kind::Sides,
    ),
    sh(
        "border-style",
        &[
            "border-top-style",
            "border-right-style",
            "border-bottom-style",
            "border-left-style",
        ],
        Kind::Sides,
    ),
    sh(
        "border-color",
        &[
            "border-top-color",
            "border-right-color",
            "border-bottom-color",
            "border-left-color",
        ],
        Kind::Sides,
    ),
    sh(
        "border-block-width",
        &["border-block-start-width", "border-block-end-width"],
        Kind::Pair,
    ),
    sh(
        "border-block-style",
        &["border-block-start-style", "border-block-end-style"],
        Kind::Pair,
    ),
    sh(
        "border-block-color",
        &["border-block-start-color", "border-block-end-color"],
        Kind::Pair,
    ),
    sh(
        "border-inline-width",
        &["border-inline-start-width", "border-inline-end-width"],
        Kind::Pair,
    ),
    sh(
        "border-inline-style",
        &["border-inline-start-style", "border-inline-end-style"],
        Kind::Pair,
    ),
    sh(
        "border-inline-color",
        &["border-inline-start-color", "border-inline-end-color"],
        Kind::Pair,
    ),
    sh(
        "border-radius",
        &[
            "border-top-left-radius",
            "border-top-right-radius",
            "border-bottom-right-radius",
            "border-bottom-left-radius",
        ],
        Kind::Radius,
    ),
    sh("border-image", BORDER_IMAGE, Kind::BorderImage),
    sh(
        "border-spacing",
        &[
            "-webkit-border-horizontal-spacing",
            "-webkit-border-vertical-spacing",
        ],
        Kind::Pair,
    ),
    sh(
        "outline",
        &["outline-color", "outline-style", "outline-width"],
        Kind::AnyOrder,
    ),
    sh(
        "background",
        &[
            "background-image",
            "background-position-x",
            "background-position-y",
            "background-size",
            "background-repeat",
            "background-attachment",
            "background-origin",
            "background-clip",
            "background-color",
        ],
        Kind::Background,
    ),
    sh(
        "background-position",
        &["background-position-x", "background-position-y"],
        Kind::Position,
    ),
    sh(
        "mask",
        &[
            "mask-image",
            "-webkit-mask-position-x",
            "-webkit-mask-position-y",
            "mask-size",
            "mask-repeat",
            "mask-origin",
            "mask-clip",
            "mask-composite",
            "mask-mode",
        ],
        Kind::Opaque,
    ),
    sh(
        "mask-position",
        &["-webkit-mask-position-x", "-webkit-mask-position-y"],
        Kind::Position,
    ),
    sh(
        "font",
        &[
            "font-style",
            "font-variant-caps",
            "font-feature-settings",
            "font-variation-settings",
            "font-weight",
            "font-stretch",
            "font-size",
            "line-height",
            "font-family",
        ],
        Kind::Font,
    ),
    sh("font-variant", &["font-variant-caps"], Kind::Single),
    sh(
        "list-style",
        &["list-style-position", "list-style-image", "list-style-type"],
        Kind::AnyOrder,
    ),
    sh(
        "text-decoration",
        &[
            "text-decoration-line",
            "text-decoration-style",
            "text-decoration-color",
        ],
        Kind::AnyOrder,
    ),
    sh(
        "flex",
        &["flex-grow", "flex-shrink", "flex-basis"],
        Kind::Flex,
    ),
    sh(
        "flex-flow",
        &["flex-direction", "flex-wrap"],
        Kind::AnyOrder,
    ),
    sh("gap", &["row-gap", "column-gap"], Kind::Pair),
    sh("columns", &["column-width", "column-count"], Kind::AnyOrder),
    sh("overflow", &["overflow-x", "overflow-y"], Kind::Pair),
    sh(
        "contain-intrinsic-size",
        &["contain-intrinsic-width", "contain-intrinsic-height"],
        Kind::Pair,
    ),
    sh(
        "place-content",
        &["align-content", "justify-content"],
        Kind::Pair,
    ),
    sh("place-items", &["align-items", "justify-items"], Kind::Pair),
    sh("place-self", &["align-self", "justify-self"], Kind::Pair),
    sh(
        "grid-row",
        &["grid-row-start", "grid-row-end"],
        Kind::GridLines,
    ),
    sh(
        "grid-column",
        &["grid-column-start", "grid-column-end"],
        Kind::GridLines,
    ),
    sh(
        "grid-area",
        &[
            "grid-row-start",
            "grid-column-start",
            "grid-row-end",
            "grid-column-end",
        ],
        Kind::GridLines,
    ),
    sh(
        "grid-template",
        &[
            "grid-template-rows",
            "grid-template-columns",
            "grid-template-areas",
        ],
        Kind::Opaque,
    ),
    sh(
        "grid",
        &[
            "grid-template-rows",
            "grid-template-columns",
            "grid-template-areas",
            "grid-auto-flow",
            "grid-auto-columns",
            "grid-auto-rows",
        ],
        Kind::Opaque,
    ),
];

const fn sh(name: &'static str, longhands: &'static [&'static str], kind: Kind) -> Shorthand {
    Shorthand {
        name,
        longhands,
        kind,
    }
}

/// How each of swb's longhands is written.
fn ser_of(name: &str) -> Ser {
    match name {
        "color"
        | "background-color"
        | "outline-color"
        | "text-decoration-color"
        | "border-top-color"
        | "border-right-color"
        | "border-bottom-color"
        | "border-left-color" => Ser::Color,
        "font-family" => Ser::FontFamily,
        "transform-origin" | "object-position" => Ser::Position,
        "background-repeat" | "mask-repeat" => Ser::Repeat,
        "background-size" | "mask-size" => Ser::BackgroundSize,
        "aspect-ratio" => Ser::AspectRatio,
        "counter-reset" | "counter-set" => Ser::Counter(0),
        "counter-increment" => Ser::Counter(1),
        "font-feature-settings" => Ser::FontFeatures,
        "border-top-left-radius"
        | "border-top-right-radius"
        | "border-bottom-right-radius"
        | "border-bottom-left-radius" => Ser::Radius,
        "grid-template-areas" => Ser::GridAreas,
        "stroke-dasharray" => Ser::Dasharray,
        "z-index" => Ser::Integer,
        "opacity" | "fill-opacity" | "stroke-opacity" => Ser::Opacity,
        "display" => Ser::Display,
        "text-decoration-line" => Ser::DecorationLine,
        "contain" => Ser::Contain,
        "justify-items" => Ser::JustifyItems,
        "align-items" | "align-self" | "justify-self" | "align-content" => Ser::Alignment,
        "grid-row-start" | "grid-row-end" | "grid-column-start" | "grid-column-end" => {
            Ser::GridLine
        }
        "grid-auto-flow" => Ser::GridAutoFlow,
        "content" => Ser::Content,
        "font-size"
        | "text-indent"
        | "letter-spacing"
        | "word-spacing"
        | "top"
        | "right"
        | "bottom"
        | "left"
        | "width"
        | "height"
        | "min-width"
        | "min-height"
        | "max-width"
        | "max-height"
        | "margin-top"
        | "margin-right"
        | "margin-bottom"
        | "margin-left"
        | "padding-top"
        | "padding-right"
        | "padding-bottom"
        | "padding-left"
        | "border-top-width"
        | "border-right-width"
        | "border-bottom-width"
        | "border-left-width"
        | "outline-width"
        | "outline-offset"
        | "background-position-x"
        | "background-position-y"
        | "-webkit-mask-position-x"
        | "-webkit-mask-position-y"
        | "vertical-align"
        | "flex-basis"
        | "row-gap"
        | "column-gap"
        | "column-width"
        | "grid-template-columns"
        | "grid-template-rows"
        | "grid-auto-columns"
        | "grid-auto-rows"
        | "contain-intrinsic-width"
        | "contain-intrinsic-height" => Ser::Length,
        _ => Ser::Plain,
    }
}

/// The table of properties.
pub(crate) struct Table {
    /// The longhands.
    pub(crate) longhands: Vec<Longhand>,
    /// The shorthands.
    pub(crate) shorthands: &'static [Shorthand],
    /// Every supported name (also aliases) to its property.
    names: HashMap<&'static str, Property>,
    /// The longhand names to their index (also those that only shorthands
    /// set).
    longhand_index: HashMap<&'static str, usize>,
    /// The camel-cased and webkit-cased attribute names, sorted (the named
    /// properties of a `CSSStyleDeclaration` as Chromium lists them).
    pub(crate) attribute_names: Vec<String>,
    /// For each longhand, the shorthands that contain it, in the order of
    /// the serialization of a block (CSSOM "preferred order": the
    /// shorthands with more longhands first).
    pub(crate) shorthands_of: Vec<Vec<usize>>,
}

/// The table, built at its first use.
pub(crate) static TABLE: LazyLock<Table> = LazyLock::new(build);

fn build() -> Table {
    let mut longhands: Vec<Longhand> = LonghandId::ALL
        .iter()
        .map(|id| id.name())
        .filter(|name| !name.starts_with("-swb-"))
        .map(|name| Longhand {
            name,
            ser: ser_of(name),
            settable: true,
        })
        .collect();
    longhands.extend(EXTRA_LONGHANDS.iter().map(|&(name, ser)| Longhand {
        name,
        ser,
        settable: true,
    }));
    longhands.extend(SHORTHAND_ONLY.iter().map(|&(name, ser)| Longhand {
        name,
        ser,
        settable: false,
    }));
    let longhand_index: HashMap<&'static str, usize> = longhands
        .iter()
        .enumerate()
        .map(|(i, l)| (l.name, i))
        .collect();
    let mut names: HashMap<&'static str, Property> = longhands
        .iter()
        .enumerate()
        .filter(|(_, l)| l.settable)
        .map(|(i, l)| (l.name, Property::Longhand(i)))
        .collect();
    for (i, s) in SHORTHANDS.iter().enumerate() {
        // `border-image` only resets with `border`: swb does not parse it.
        if is_known_property(s.name) {
            names.insert(s.name, Property::Shorthand(i));
        }
    }
    for &(alias, standard) in ALIASES {
        if let Some(&p) = names.get(standard) {
            names.insert(alias, p);
        }
    }
    let mut attribute_names: Vec<String> = names.keys().map(|n| camel_case(n)).collect();
    attribute_names.sort_unstable();
    attribute_names.dedup();
    let mut shorthands_of = vec![Vec::new(); longhands.len()];
    let mut order: Vec<usize> = (0..SHORTHANDS.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(SHORTHANDS.get(i).map_or(0, |s| s.longhands.len())));
    for i in order {
        let Some(s) = SHORTHANDS.get(i) else {
            continue;
        };
        for l in s.longhands {
            if let Some(list) = longhand_index
                .get(l)
                .and_then(|&j| shorthands_of.get_mut(j))
            {
                list.push(i);
            }
        }
    }
    Table {
        longhands,
        shorthands: SHORTHANDS,
        names,
        longhand_index,
        attribute_names,
        shorthands_of,
    }
}

impl Table {
    /// The property of a name (already lower case).
    pub(crate) fn lookup(&self, name: &str) -> Option<Property> {
        self.names.get(name).copied()
    }

    /// The index of a longhand by name.
    pub(crate) fn longhand(&self, name: &str) -> Option<usize> {
        self.longhand_index.get(name).copied()
    }

    /// The name of a longhand.
    pub(crate) fn longhand_name(&self, index: usize) -> &'static str {
        self.longhands.get(index).map_or("", |l| l.name)
    }

    /// A shorthand by index.
    pub(crate) fn shorthand(&self, index: usize) -> Option<&'static Shorthand> {
        self.shorthands.get(index)
    }
}

/// The camel-cased attribute of a property name (CSSOM §6.7.1): a hyphen
/// before a letter is dropped and the letter upper cased; `-webkit-` names
/// get the webkit-cased attribute (`webkitTransform`).
pub(crate) fn camel_case(name: &str) -> String {
    let name = name
        .strip_prefix("-webkit-")
        .map_or_else(|| name.to_owned(), |rest| format!("webkit-{rest}"));
    let mut out = String::with_capacity(name.len());
    let mut upper = false;
    for c in name.chars() {
        if c == '-' {
            upper = true;
        } else if upper {
            out.push(c.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// The property name of an IDL attribute name: the dashed attribute is the
/// name itself; a camel-cased one (`backgroundColor`, `WebkitTransform`,
/// `webkitTransform`) maps back by putting a hyphen before each upper case
/// letter. `None` if swb does not support the property. `cssFloat` is an
/// attribute of the prototype; the named property is `float`.
pub fn property_of_attribute(attribute: &str) -> Option<String> {
    let table = &*TABLE;
    if attribute.contains('-') {
        return table.lookup(attribute).map(|_| attribute.to_owned());
    }
    if attribute.bytes().any(|b| !b.is_ascii_alphanumeric()) {
        return None;
    }
    let mut dashed = String::with_capacity(attribute.len() + 4);
    if let Some(rest) = attribute.strip_prefix("webkit")
        && rest.starts_with(|c: char| c.is_ascii_uppercase())
    {
        dashed.push_str("-webkit");
        push_dashed(rest, &mut dashed);
    } else {
        push_dashed(attribute, &mut dashed);
    }
    table.lookup(&dashed).map(|_| dashed)
}

fn push_dashed(camel: &str, out: &mut String) {
    for c in camel.chars() {
        if c.is_ascii_uppercase() {
            out.push('-');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
}

/// The camel-cased and webkit-cased attribute names of the supported
/// properties, sorted (the named properties that a `CSSStyleDeclaration`
/// lists).
pub fn attribute_names() -> &'static [String] {
    &TABLE.attribute_names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribute_names_map_to_properties() {
        let cases = [
            ("backgroundColor", Some("background-color")),
            ("background-color", Some("background-color")),
            ("webkitTransform", Some("-webkit-transform")),
            ("WebkitTransform", Some("-webkit-transform")),
            ("float", Some("float")),
            ("cssFloat", None),
            ("MozTransform", None),
            ("transition", None),
            ("borderTopLeftRadius", Some("border-top-left-radius")),
            ("marginInlineStart", Some("margin-inline-start")),
            ("gridGap", Some("grid-gap")),
            ("all", None),
            ("borderImage", None),
            ("BACKGROUND", None),
            ("--x", None),
        ];
        for (attribute, property) in cases {
            assert_eq!(
                property_of_attribute(attribute).as_deref(),
                property,
                "{attribute}"
            );
        }
        assert_eq!(camel_case("-webkit-transform"), "webkitTransform");
        assert_eq!(camel_case("border-top-width"), "borderTopWidth");
        assert_eq!(camel_case("-moz-box-sizing"), "MozBoxSizing");
        let names = attribute_names();
        assert!(names.iter().any(|n| n == "webkitTransform"));
        assert!(!names.iter().any(|n| n.contains('-')));
        assert!(names.is_sorted());
    }

    #[test]
    fn swb_parses_every_settable_longhand() {
        for l in &TABLE.longhands {
            assert_eq!(is_known_property(l.name), l.settable, "{}", l.name);
        }
    }

    #[test]
    fn every_longhand_of_a_shorthand_is_in_the_table() {
        for s in TABLE.shorthands {
            for l in s.longhands {
                assert!(TABLE.longhand(l).is_some(), "{} of {}", l, s.name);
            }
        }
    }
}
