//! Arena-based DOM tree and HTML parsing.
//!
//! The [`Document`] owns all nodes in a `Vec`; nodes are addressed by
//! [`NodeId`]. HTML is parsed by html5ever directly into this tree. See
//! `docs/adr/0004-engine-structure.md` for the reasons, and ADR 0028
//! section 3 for the parts that scripts need: the resumable parser
//! ([`HtmlParser`]), the change functions with their journal ([`Journal`])
//! and mutation sink ([`MutationSink`]), the id index, fragment parsing,
//! more documents in one arena, and the limits ([`Limits`]).

mod dump;
mod encoding;
mod id_index;
mod journal;
mod microsyntax;
mod mutation;
mod parser;
mod serialize;
mod tree;

pub use dump::dump_tree;
pub use html5ever::{LocalName, Namespace, QualName, local_name, ns};
pub use journal::{Change, Journal, MAX_JOURNAL_ENTRIES, MutationSink};
pub use microsyntax::{
    canvas_size, is_valid_float, is_valid_non_negative_integer, parse_integer,
    parse_non_negative_integer, parse_non_negative_u32,
};
pub use mutation::DomError;
pub use parser::{
    HtmlParser, MAX_TREE_DEPTH, ParseStep, TextPosition, decode_html_bytes, parse_fragment,
    parse_html, parse_html_bytes, parse_html_bytes_with, parse_html_with,
};
pub use serialize::outer_html;
pub use tree::{
    Attribute, Children, Descendants, Document, ElementData, Limits, MAX_NODES, MAX_TEXT_BYTES,
    Node, NodeData, NodeId, QuirksMode, is_html_whitespace,
};
