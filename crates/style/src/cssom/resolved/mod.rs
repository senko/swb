//! Resolved values for `getComputedStyle` (CSSOM §9 "Resolved values",
//! <https://drafts.csswg.org/cssom/#resolved-values>, ADR 0028 section
//! 10): the computed values of a [`ComputedStyle`] serialized as Chromium
//! serializes them, with the values that layout resolves taken from the
//! used box of the element ([`UsedBox`], which the engine finds in the
//! fragment tree).
//!
//! Chromium's rules (measured with `tools/probes/web-computed-style.json`):
//! colors are `rgb()`/`rgba()` with `currentcolor` resolved; lengths are in
//! `px` with six significant digits; values from layout are in Chromium's
//! layout units (multiples of 1/64 px, truncated); `width`, `height`, the
//! margins and the paddings come from layout for elements with a box that
//! is not a part of an inline box; the insets of positioned boxes come from
//! layout (`auto` stays for sticky boxes); `transform` is a `matrix()` with
//! percentages resolved against the border box (`none` without a box);
//! `line-height` numbers are `px`; `min-width: auto` is `0px` except for
//! flex and grid items. Shorthands serialize from their longhands
//! (`shorthand.rs`).
//!
//! Deliberate departures (the values swb does not keep): the track sizes of
//! grid containers stay as specified (Chromium gives the used sizes);
//! `aspect-ratio` gives the ratio as one number over 1; `oblique` with an
//! angle, `cursor` images, `safe`/`unsafe` alignment and the alternative
//! text of `content` are lost; `-apple-system` and `BlinkMacSystemFont` are
//! `system-ui`; the auto margins of flex items in the main axis and of grid
//! items are `0px`.

mod longhand;
mod shorthand;
#[cfg(test)]
mod tests;

use std::sync::LazyLock;

use crate::ComputedStyle;
use crate::values::Rgba;

use super::props::{Property, TABLE};

/// What layout gave the box of an element (or pseudo-element), for the
/// values that layout resolves. Lengths in CSS px.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsedBox {
    /// What kind of box it is.
    pub kind: BoxKind,
    /// The position of the border box relative to the origin of the
    /// containing block (`containing_block`).
    pub x: f32,
    /// See `x`.
    pub y: f32,
    /// The width of the border box.
    pub width: f32,
    /// The height of the border box.
    pub height: f32,
    /// The used paddings: top, right, bottom, left.
    pub padding: [f32; 4],
    /// The used border widths: top, right, bottom, left.
    pub border: [f32; 4],
    /// The width of the containing block: the content box of the parent
    /// box for boxes in flow, the padding box of the containing block for
    /// absolutely positioned boxes, the viewport for fixed ones.
    pub cb_width: f32,
    /// The height of the containing block.
    pub cb_height: f32,
    /// The size of the scrollport that a sticky box sticks in (the
    /// nearest scroll container's padding box, or the viewport).
    pub scrollport: (f32, f32),
}

/// The kinds of boxes whose resolved values differ.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BoxKind {
    /// A part of an inline box: `width`, `height`, margins and paddings
    /// stay computed (Chromium resolves them only for boxes that are not
    /// inline).
    Inline,
    /// A block-level box in the flow of a block container.
    #[default]
    Block,
    /// A flex item; `row` if the main axis of the container is
    /// horizontal.
    FlexItem {
        /// The main axis is horizontal.
        row: bool,
    },
    /// A grid item.
    GridItem,
    /// An absolutely positioned box (`absolute` or `fixed`).
    Absolute,
    /// Any other box: floats, atomic inlines, table parts.
    Other,
}

/// The pseudo-element of a `getComputedStyle` call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PseudoElement {
    /// One that swb styles: `::before`, `::after`, `::marker`,
    /// `::placeholder`.
    Styled(crate::PseudoKind),
    /// One that Chromium knows and swb does not style (`::first-line`,
    /// `::selection`, `::backdrop`, ...): its style inherits from the
    /// element and has no box (departure: its rules do not apply).
    Unstyled,
}

impl PseudoElement {
    /// The kind of a styled pseudo-element.
    pub fn kind(self) -> Option<crate::PseudoKind> {
        match self {
            PseudoElement::Styled(kind) => Some(kind),
            PseudoElement::Unstyled => None,
        }
    }
}

/// What the resolved values of one element depend on.
#[derive(Clone, Copy, Debug)]
pub struct Resolver<'a> {
    /// The computed style of the element or pseudo-element.
    pub style: &'a ComputedStyle,
    /// The box from layout; `None` if the element has no box (`display:
    /// none` or `contents`, not rendered) or layout did not run.
    pub used: Option<&'a UsedBox>,
    /// The pseudo-element, if the style is one's (`content` differs).
    pub pseudo: Option<crate::PseudoKind>,
}

/// The value of `property` (any case; aliases, logical longhands,
/// shorthands and custom properties) as `getPropertyValue` of a computed
/// style declaration gives it, or the empty string for a property that swb
/// does not know.
pub fn resolved_value(property: &str, resolver: &Resolver<'_>) -> String {
    if property.starts_with("--") {
        return custom_value(property, resolver.style);
    }
    let name = property.to_ascii_lowercase();
    match canonical(&name) {
        Some(Canonical::Longhand(name)) => longhand::value(name, resolver),
        Some(Canonical::Shorthand(name)) => shorthand::value(name, resolver),
        None => String::new(),
    }
}

/// True if the value of `property` can depend on layout (so a query needs
/// the layout of the document, not only the styles).
pub fn needs_layout(property: &str) -> bool {
    let name = property.to_ascii_lowercase();
    match canonical(&name) {
        Some(Canonical::Longhand(name)) => longhand::needs_layout(name),
        Some(Canonical::Shorthand(name)) => shorthand::needs_layout(name),
        None => false,
    }
}

/// The longhands that a computed style declaration lists (`length`,
/// `item()`), in Chromium's order: the standard names in alphabetical
/// order, then the `-webkit-` names. The custom properties of the element
/// follow them (Chromium lists them last).
pub fn property_names() -> &'static [&'static str] {
    &NAMES
}

static NAMES: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    let mut names: Vec<&'static str> = TABLE.longhands.iter().map(|l| l.name).collect();
    names.sort_by_key(|n| (n.starts_with('-'), *n));
    names.dedup();
    names
});

/// A property name, after its aliases.
enum Canonical {
    Longhand(&'static str),
    Shorthand(&'static str),
}

fn canonical(name: &str) -> Option<Canonical> {
    let table = &*TABLE;
    match table.lookup(name) {
        Some(Property::Longhand(i)) => Some(Canonical::Longhand(table.longhand_name(i))),
        Some(Property::Shorthand(i)) => table.shorthand(i).map(|s| Canonical::Shorthand(s.name)),
        // The longhands that only shorthands set, and `border-image`
        // (swb does not parse it: its value is always the initial one).
        None => match table.longhand(name) {
            Some(i) => Some(Canonical::Longhand(table.longhand_name(i))),
            None => (name == "border-image").then_some(Canonical::Shorthand("border-image")),
        },
    }
}

/// The value of a custom property: its tokens as written, without the
/// white space around them (Chromium keeps runs of white space inside;
/// swb's tokens have them collapsed).
fn custom_value(name: &str, style: &ComputedStyle) -> String {
    let Some(value) = style
        .custom_properties
        .as_ref()
        .and_then(|props| props.get(name))
    else {
        return String::new();
    };
    swb_css::serialize_component_values(value)
        .trim_matches(|c: char| c.is_ascii_whitespace())
        .to_owned()
}

/// The names of the custom properties of `style`, sorted (Chromium lists
/// them after the other properties).
pub fn custom_property_names(style: &ComputedStyle) -> Vec<String> {
    let mut names: Vec<String> = style
        .custom_properties
        .as_ref()
        .map(|props| props.keys().map(ToString::to_string).collect())
        .unwrap_or_default();
    names.sort_unstable();
    names
}

// ----- Number forms -----

/// A number as Chromium writes it (six significant digits).
fn number(value: f32) -> String {
    super::value::format_number(f64::from(value))
}

/// A computed length.
fn px(value: f32) -> String {
    format!("{}px", number(value))
}

/// The largest value of Chromium's layout units, (2^31 - 1) / 64.
const LAYOUT_MAX: f64 = 33_554_431.984_375;

/// A length from layout, in Chromium's layout units: a multiple of 1/64
/// px, truncated, within the range of those units.
fn layout_px(value: f32) -> String {
    format!("{}px", super::value::format_number(snap(f64::from(value))))
}

/// `value` in layout units (see [`layout_px`]). A value within 1/100 of a
/// unit of a multiple is that multiple: swb lays out in `f32`, so a value
/// that is exact in Chromium's units can come out just below it.
fn snap(value: f64) -> f64 {
    let units = value * 64.0;
    let near = units.round();
    let units = if (units - near).abs() < 0.01 {
        near
    } else {
        units.trunc()
    };
    (units / 64.0).clamp(-LAYOUT_MAX - 1.0 / 64.0, LAYOUT_MAX)
}

/// A color.
fn rgba(c: Rgba) -> String {
    let mut out = String::new();
    super::value::write_rgba(c, &mut out);
    out
}
