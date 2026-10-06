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
//!   states, disabled form controls and `querySelectorAll`.
//! - `values`, `computed`: computed value types.
//! - `style_map`: the result for a document; `content`: the text of
//!   generated content.
//!
//! See `docs/adr/0007-style-system.md` for the design.

mod bloom;
mod cascade;
mod computed;
mod content;
mod custom;
mod element;
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
pub use style_map::{PseudoKind, StyleMap};
pub use stylist::Stylist;
pub use values::*;
