//! Arena-based DOM tree and HTML parsing.
//!
//! The [`Document`] owns all nodes in a `Vec`; nodes are addressed by
//! [`NodeId`]. HTML is parsed by html5ever directly into this tree. See
//! `docs/adr/0004-engine-structure.md` for the reasons, and ADR 0028
//! section 3 for the parts that scripts need: the resumable parser
//! ([`HtmlParser`]), the change functions with their journal ([`Journal`])
//! and mutation sink ([`MutationSink`]), the id index, fragment parsing,
//! more documents in one arena, and the limits ([`Limits`]).

mod activation;
mod algorithms;
mod custom;
mod dump;
mod encoding;
mod flat;
mod handler_attrs;
mod id_index;
mod iterator;
mod journal;
mod microsyntax;
mod mutation;
mod parser;
mod ranges;
mod serialize;
mod shadow;
mod slots;
mod subtree_search;
mod top_layer;
mod transplant;
mod tree;
mod work;

pub use activation::{
    ClickRole, ClickTarget, LabelMemo, click_target, swallows_invoker_click, with_invoker,
};
pub use algorithms::{Cloned, position};
pub use custom::{
    CustomElementState, element_local_name_hyphen, is_valid_custom_element_name,
    is_valid_element_local_name,
};
pub use dump::dump_tree;
pub use flat::{FlatChildren, FlatScopes};
pub use html5ever::{LocalName, Namespace, QualName, local_name, ns};
pub use iterator::{IteratorPosition, Spot};
pub use journal::{Change, Journal, MAX_JOURNAL_ENTRIES, MutationSink};
pub use microsyntax::{
    canvas_size, is_valid_float, is_valid_non_negative_integer, parse_integer,
    parse_non_negative_integer, parse_non_negative_u32,
};
pub use mutation::DomError;
pub use parser::{
    HtmlParser, MAX_TREE_DEPTH, ParseStep, TextPosition, WrittenStep, decode_html_bytes,
    parse_document, parse_fragment, parse_html, parse_html_bytes, parse_html_bytes_limited,
    parse_html_bytes_with, parse_html_limited, parse_html_with,
};
pub use ranges::{Boundary, Contents, Flow, HOST_ID_LIMIT, LiveRange, RangePiece, SelectionState};
pub use serialize::{inner_html, outer_html};
pub use shadow::{
    AttachError, ShadowIncludingDescendants, ShadowRootInit, is_valid_shadow_host_name,
};
pub use top_layer::{
    ClosedBy, DialogData, HideOptions, PendingToggle, PopoverError, PopoverErrorKind, PopoverState,
    TargetAction, ToggleState, TopLayerHost, TopLayerState, WatcherKind, can_invoke, close_dialog,
    dialog_form_target, hide_all_popovers_until, hide_popover, invoker_acts_on_form,
    light_dismiss_dialogs_down, light_dismiss_dialogs_up, light_dismiss_down, light_dismiss_up,
    popover_target_activation, popover_target_by_id, process_close_watchers, request_close_dialog,
    run_popover_attribute_steps, show_dialog, show_modal, show_popover, toggle_popover,
    topmost_clicked_popover, topmost_popover_ancestor,
};
pub use transplant::{Placed, Subtree, SubtreeSize};
pub use tree::{
    Attribute, Children, Descendants, Document, ElementData, Limits, MAX_NODES, MAX_TEXT_BYTES,
    Node, NodeData, NodeId, QuirksMode, ShadowRootData, ShadowRootMode, SlotAssignment,
    is_html_whitespace,
};
pub use work::{ArenaWork, DeadIds, WORK_PER_CHECK};
