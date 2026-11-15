//! The token sink between html5ever's tokenizer and its tree builder, which
//! lets the parser pause after a token (ADR 0035 section 8).
//!
//! The tree builder can pause the tokenizer only at the end tag of a
//! `script` element. Synchronous custom element construction (HTML "create
//! an element for a token" with "will execute script") needs a pause after
//! any token that creates an element whose name the window's registry
//! defines, and the reactions of the parser's other changes need one after
//! the token that queued them. [`Wrapper`] forwards each token to the tree
//! builder and then reads one cell: while it is 0 (always, while the page
//! has no custom element definition), the tree builder's answer passes
//! unchanged. Otherwise a token whose answer is "continue" ends with
//! `TokenSinkResult::Script`, so that the tokenizer returns from `feed`
//! after the token, and the parser reports the pause ([`Wrapper::take_paused`]).
//!
//! The tokenizer's state after the token stays what the tree builder's
//! answer made it: only a "continue" answer to a start or end tag becomes a
//! pause, and that answer leaves the tokenizer in its state (the tests in
//! `tests/custom.rs` check the text that follows a pause in raw text, script
//! data, `plaintext` and foreign content, since swb does not read
//! html5ever's source). A token
//! whose answer changes the tokenizer's state never creates a custom element
//! (those are the start tags of `textarea`, `script`, `style`, `plaintext`
//! and similar, and the end tag of `script`); if a pending element meets
//! such an answer anyway, the sink inserts its placeholder as an ordinary
//! element ([`Sink::abandon_pending`]).

use std::cell::Cell;
use std::rc::Rc;

use html5ever::tokenizer::{Token, TokenSink, TokenSinkResult};
use html5ever::tree_builder::TreeBuilder;

use super::sink::Sink;
use crate::tree::NodeId;

/// The bit of the pause cell for an element that waits for its
/// construction.
pub(crate) const PAUSE_CUSTOM: u8 = 1;
/// The bit of the pause cell for reactions that the mutation sink queued
/// (`Document::request_parser_pause`).
pub(crate) const PAUSE_REACTIONS: u8 = 2;

/// The token sink of the document parser. See the module documentation.
pub(super) struct Wrapper {
    pub(super) tree_builder: TreeBuilder<NodeId, Sink>,
    /// The pause cell, shared with the sink and the document.
    pause: Rc<Cell<u8>>,
    /// The bits of the cell that the last pause took.
    paused: Cell<u8>,
    /// False while the tokenizer ends the input: no pause then.
    pausing: Cell<bool>,
}

impl Wrapper {
    pub(super) fn new(tree_builder: TreeBuilder<NodeId, Sink>) -> Wrapper {
        let pause = Rc::clone(&tree_builder.sink.pause);
        Wrapper {
            tree_builder,
            pause,
            paused: Cell::new(0),
            pausing: Cell::new(true),
        }
    }

    /// The bits of the pause that ended the last `feed` with a
    /// `TokenizerResult::Script`, and clears them; 0 if the tree builder
    /// reached a `script` end tag.
    pub(super) fn take_paused(&self) -> u8 {
        self.paused.replace(0)
    }

    /// Turns the pauses off (before the tokenizer ends the input, which may
    /// process tokens and does not expect a pause).
    pub(super) fn stop_pausing(&self) {
        self.pausing.set(false);
    }

    /// The rare path of [`TokenSink::process_token`]: the cell asks for a
    /// pause after a token (`tag`: a start or end tag).
    #[cold]
    #[inline(never)]
    fn pause_after(&self, result: TokenSinkResult<NodeId>, tag: bool) -> TokenSinkResult<NodeId> {
        if !self.pausing.get() {
            return result;
        }
        if !tag || !matches!(result, TokenSinkResult::Continue) {
            // Only a tag can end with a pause (only tags create elements or
            // move them), and only with a "continue" answer: another one
            // changes the tokenizer's state or ends a script. A pending
            // element is inserted as it is; queued reactions wait for a
            // later token.
            if self.pause.get() & PAUSE_CUSTOM != 0 {
                self.tree_builder.sink.abandon_pending();
                self.pause.set(self.pause.get() & !PAUSE_CUSTOM);
            }
            return result;
        }
        self.paused.set(self.pause.replace(0));
        TokenSinkResult::Script(NodeId::DOCUMENT)
    }
}

impl TokenSink for Wrapper {
    type Handle = NodeId;

    #[inline]
    fn process_token(&self, token: Token, line_number: u64) -> TokenSinkResult<NodeId> {
        let tag = matches!(token, Token::TagToken(_));
        let result = self.tree_builder.process_token(token, line_number);
        if self.pause.get() == 0 {
            return result;
        }
        self.pause_after(result, tag)
    }

    fn end(&self) {
        self.tree_builder.end();
    }

    fn adjusted_current_node_present_but_not_in_html_namespace(&self) -> bool {
        self.tree_builder
            .adjusted_current_node_present_but_not_in_html_namespace()
    }
}
