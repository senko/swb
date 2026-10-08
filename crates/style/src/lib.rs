//! CSS properties, cascade, inheritance and computed values.
//!
//! The entry points are [`Stylist`] (the user-agent stylesheet, the
//! presentational hint rules and the author stylesheets of a document) and
//! [`compute_styles`], which runs the cascade for every element and
//! returns a [`StyleMap`] of [`ComputedStyle`]s.
//!
//! Module overview:
//!
//! - `parse`: value parsers (lengths and `calc()`, colors, images, grid
//!   track lists, lines and areas).
//! - `properties`: longhand and shorthand parsers, specified values,
//!   computing values.
//! - `custom`: custom properties and `var()` substitution.
//! - `stylist`: rule storage and lookup.
//! - `bloom`: the ancestor Bloom filter that rejects rules early.
//! - `cascade`: the cascade, inheritance and computed-value fixups.
//! - `hints`: presentational hints from HTML attributes.
//! - `element`: the DOM element adapter for selector matching, element
//!   states, disabled form controls and `querySelectorAll`;
//!   `element_kinds`: the kinds of boxes of HTML elements that style and
//!   layout share (replaced elements, possible list items).
//! - `values`, `computed`: computed value types.
//! - `font_face`: `@font-face` rules and their descriptors;
//!   `font_settings`: `font-variation-settings` and `font-feature-settings`.
//! - `counters`: CSS counters and list item ordinal values, resolved
//!   after the cascade; `counter_style`: the text of a counter value.
//! - `style_map`: the result for a document; `content`: the text of
//!   generated content.
//!
//! See `docs/adr/0007-style-system.md` for the design.

mod bloom;
mod cascade;
mod computed;
mod content;
mod counter_style;
mod counters;
mod custom;
mod element;
mod element_kinds;
mod font_face;
mod font_settings;
mod hints;
mod parse;
mod properties;
mod style_map;
mod stylist;
mod values;

pub use cascade::compute_styles;
pub use computed::{ComputedStyle, CustomProperties};
pub use content::content_text;
pub use element::{
    CONTROL_STATES, DisabledElements, ElementStates, is_actually_disabled, query_selector_all,
};
pub use element_kinds::is_replaced_element;
pub use font_face::{FontDisplay, FontFace, FontFaceSource, FontFaceStyle, MAX_RATIO};
pub use font_settings::{FontFeatureSettings, FontTag, FontVariationSettings, MAX_SETTINGS};
pub use style_map::{PseudoKind, StyleMap};
pub use stylist::{MAX_FONT_FACES, Stylist};
pub use values::*;
