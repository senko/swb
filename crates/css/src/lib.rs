//! CSS syntax: tokenizer, stylesheet parser, selectors and media queries.
//!
//! This crate turns CSS text into data structures. It knows nothing about
//! the DOM or about individual properties; the `swb-style` crate parses
//! property values (with [`Parser`]) and matches selectors (through the
//! [`Element`] trait).
//!
//! Specifications:
//!
//! - CSS Syntax Level 3: <https://www.w3.org/TR/css-syntax-3/>
//!   ([`tokenize`], [`parse_stylesheet`], [`parse_style_attribute`],
//!   [`parse_component_values`], [`serialize_component_values`]).
//! - Selectors Level 4: <https://www.w3.org/TR/selectors-4/>
//!   ([`SelectorList`], [`matches()`]).
//! - Media Queries Level 4: <https://www.w3.org/TR/mediaqueries-4/>
//!   ([`MediaQueryList`]).
//! - The `sizes` attribute of HTML images, which uses media conditions:
//!   <https://html.spec.whatwg.org/multipage/images.html#sizes-attributes>
//!   ([`source_size`]).
//! - CSS Containment 3, container queries:
//!   <https://www.w3.org/TR/css-contain-3/#container-queries>
//!   ([`ContainerCondition`]).
//! - CSS Conditional Rules Level 3 and 4:
//!   <https://www.w3.org/TR/css-conditional-3/> ([`SupportsCondition`]).
//!
//! Content from the network never causes a panic: malformed CSS is handled
//! with the error recovery that the specifications define, and nesting
//! depth is limited so that hostile input cannot overflow the stack.

mod container;
mod cursor;
mod media;
mod parser;
mod selector;
mod serialize;
mod sizes;
mod stylesheet;
mod supports;
mod tokenizer;
mod values;

pub use container::{
    ContainerCondition, ContainerSize, QueryAxes, is_valid_name as is_valid_container_name,
};
pub use cursor::{ParseError, Parser};
pub use media::{ColorScheme, MediaEnvironment, MediaQueryList, MediaType};
pub use parser::{parse_component_values, parse_rule, parse_style_attribute, parse_stylesheet};
pub use selector::{
    AnB, BucketKey, CaseSensitivity, Element, ElementState, HostRelation, MATCH_BUDGET,
    MatchingContext, PseudoElement, QuirksMode, Selector, SelectorList, ShadowPseudoKind,
    Specificity, matches, matches_any, matches_in_tree, matches_part, matches_slotted,
    matches_with_scope,
};
pub use serialize::{serialize_component_values, serialize_identifier, serialize_string};
pub use sizes::{allows_auto, source_size};
pub use stylesheet::{
    ContainerRule, CssRule, Declaration, FontFaceRule, ImportLayer, ImportRule, Keyframe,
    KeyframesRule, LayerBlockRule, LayerName, LayerStatementRule, MediaRule, StyleRule, Stylesheet,
    SupportsRule,
};
pub use supports::SupportsCondition;
pub use tokenizer::{Number, Token, tokenize};
pub use values::{
    BlockKind, ComponentValue, Function, SimpleBlock, contains_function, split_on_commas,
    trim_whitespace,
};
