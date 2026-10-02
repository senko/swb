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
//! - CSS Conditional Rules Level 3 and 4:
//!   <https://www.w3.org/TR/css-conditional-3/> ([`SupportsCondition`]).
//!
//! Content from the network never causes a panic: malformed CSS is handled
//! with the error recovery that the specifications define, and nesting
//! depth is limited so that hostile input cannot overflow the stack.

mod cursor;
mod media;
mod parser;
mod selector;
mod serialize;
mod stylesheet;
mod supports;
mod tokenizer;
mod values;

pub use cursor::{ParseError, Parser};
pub use media::{ColorScheme, MediaEnvironment, MediaQueryList, MediaType};
pub use parser::{
    parse_component_values, parse_rule_list, parse_style_attribute, parse_stylesheet,
};
pub use selector::{
    AnB, BucketKey, CaseSensitivity, Element, ElementState, MATCH_BUDGET, MatchingContext,
    PseudoElement, QuirksMode, Selector, SelectorList, Specificity, matches, matches_any,
    matches_with_scope,
};
pub use serialize::{serialize_component_values, serialize_identifier, serialize_string};
pub use stylesheet::{
    CssRule, Declaration, FontFaceRule, ImportRule, MediaRule, StyleRule, Stylesheet, SupportsRule,
};
pub use supports::SupportsCondition;
pub use tokenizer::{Number, Token, preprocess, tokenize};
pub use values::{
    BlockKind, ComponentValue, Function, SimpleBlock, contains_function, split_on_commas,
    trim_whitespace,
};
