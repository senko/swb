//! Arena-based DOM tree and HTML parsing.
//!
//! The [`Document`] owns all nodes in a `Vec`; nodes are addressed by
//! [`NodeId`]. HTML is parsed by html5ever directly into this tree. See
//! `docs/adr/0004-engine-structure.md` for the reasons.

mod dump;
mod encoding;
mod microsyntax;
mod parser;
mod serialize;
mod tree;

pub use dump::dump_tree;
pub use html5ever::{LocalName, Namespace, QualName, local_name, ns};
pub use microsyntax::{
    is_valid_float, is_valid_non_negative_integer, parse_integer, parse_non_negative_integer,
    parse_non_negative_u32,
};
pub use parser::{MAX_TREE_DEPTH, parse_html, parse_html_bytes};
pub use serialize::outer_html;
pub use tree::{
    Attribute, Children, Descendants, Document, ElementData, Node, NodeData, NodeId, QuirksMode,
    is_html_whitespace,
};
