//! HTML parsing: the resumable parser and the parse functions.
//!
//! [`HtmlParser`] drives html5ever's tokenizer directly (ADR 0028 section
//! 3): a `BufferQueue` holds the decoded input, and each
//! [`resume`](HtmlParser::resume) feeds it until the tree builder reaches
//! the end tag of a `script` element or the input ends. The parser keeps
//! the tokenizer, the tree builder and its sink between steps; the caller
//! keeps the [`Document`] and lends it to each step. The stack of open
//! elements holds `NodeId`s, which stay valid while scripts and the engine
//! use the document between steps (also for nodes they removed).
//! `document.write` (later work, ADR 0028 section 8) will put its text into
//! the queue in front of the unparsed rest (`BufferQueue::push_front`).
//! With the scripting flag on, the parser also finds where the text of each
//! script starts in the source ([`HtmlParser::script_position`],
//! `position.rs`), for the positions of errors in inline scripts.
//!
//! The tree builder gets the scripting flag of the document (the JavaScript
//! setting when the document was created, ADR 0028 section 15). With the
//! flag off, `<noscript>` content is parsed as normal markup and rendered,
//! as in a browser with JavaScript turned off. With it on, `<noscript>` in
//! `head` and `body` holds raw text, and the document remembers the flag
//! ([`Document::scripting`]).
//!
//! The tree depth is limited to about [`MAX_TREE_DEPTH`], as in Blink:
//! elements that would be deeper are attached to the ancestor at the limit.
//! The limit is not strict: the tree builder can move existing subtrees
//! (adoption agency, reparenting), which can make a few nodes deeper, and
//! template contents are counted from their own fragment. Later stages do
//! not rely on it: style is iterative and box construction has its own
//! depth limit.
//!
//! At a limit of the arena ([`Limits`](crate::Limits)), the parser drops the
//! rest of its input with a warning and finishes the document: the page
//! shows what was parsed up to that point.

mod fragment;
mod position;
mod sink;

use std::rc::Rc;

use html5ever::TokenizerResult;
use html5ever::tendril::StrTendril;
use html5ever::tokenizer::{BufferQueue, Tokenizer, TokenizerOpts};
use html5ever::tree_builder::{TreeBuilder, TreeBuilderOpts};

pub use fragment::{parse_document, parse_fragment};
use position::Positions;
pub use position::TextPosition;
use sink::Sink;

use crate::encoding::decode_html;
use crate::mutation::DomError;
use crate::tree::{Document, NodeId};

/// The maximum depth of a node below the root of its tree (the document,
/// or a template's content fragment). Blink uses the same limit
/// (`kMaximumHTMLParserDOMTreeDepth`).
pub const MAX_TREE_DEPTH: usize = 512;

/// The result of one step of the parser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseStep {
    /// The tree builder popped this `script` element at its end tag: the
    /// script may run now (HTML "prepare the script element"), then the
    /// parse continues with the next [`HtmlParser::resume`].
    Script(NodeId),
    /// The input ended and the document is complete.
    Finished,
}

/// A resumable HTML parser for one document. See the module documentation.
pub struct HtmlParser {
    tokenizer: Tokenizer<TreeBuilder<NodeId, Sink>>,
    input: Rc<BufferQueue>,
    arena_id: u64,
    finished: bool,
    /// The source text for the positions of scripts (scripting on).
    positions: Option<Positions>,
    /// The position of the text of the script of the last step.
    script_position: TextPosition,
}

/// The largest input chunk: tendrils hold at most 4 GiB.
const MAX_CHUNK: usize = 1 << 30;

/// Appends `text` to `queue` in chunks that a tendril can hold.
fn push_text(queue: &BufferQueue, mut text: &str) {
    while !text.is_empty() {
        let mut end = text.len().min(MAX_CHUNK);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let (chunk, rest) = text.split_at(end);
        queue.push_back(StrTendril::from_slice(chunk));
        text = rest;
    }
}

impl HtmlParser {
    /// Starts the parse of `text` into `doc`, which must be a new document
    /// ([`Document::new`]). `scripting` becomes the scripting flag of the
    /// document and of the parser. Nothing is parsed before the first
    /// [`resume`](Self::resume).
    pub fn new(doc: &mut Document, text: &str, scripting: bool) -> HtmlParser {
        debug_assert_eq!(doc.len(), 1, "the parser starts with a new document");
        doc.set_scripting(scripting);
        let input = Rc::new(BufferQueue::default());
        push_text(&input, text);
        let opts = TreeBuilderOpts {
            scripting_enabled: scripting,
            ..Default::default()
        };
        // The tree builder asks the sink for the document node only; the
        // placeholder is never changed.
        let sink = Sink::new(Document::new(), Rc::clone(&input));
        let tree_builder = TreeBuilder::new(sink, opts);
        HtmlParser {
            tokenizer: Tokenizer::new(tree_builder, TokenizerOpts::default()),
            input,
            arena_id: doc.arena_id(),
            finished: false,
            positions: scripting.then(|| Positions::new(text)),
            script_position: TextPosition::default(),
        }
    }

    /// Parses until the next `script` end tag or the end of the input.
    /// `doc` must be the document that the parser started with; another one
    /// gives [`DomError::WrongDocument`]. After [`ParseStep::Finished`],
    /// further calls return it again.
    pub fn resume(&mut self, doc: &mut Document) -> Result<ParseStep, DomError> {
        if doc.arena_id() != self.arena_id {
            return Err(DomError::WrongDocument);
        }
        if self.finished {
            return Ok(ParseStep::Finished);
        }
        let sink = &self.tokenizer.sink.sink;
        std::mem::swap(&mut *sink.doc.borrow_mut(), doc);
        let step = loop {
            match self.tokenizer.feed(&self.input) {
                TokenizerResult::Script(node) if !sink.stopped.get() => {
                    self.script_position = script_start(
                        self.positions.as_mut(),
                        &self.input,
                        &sink.doc.borrow(),
                        node,
                    );
                    break ParseStep::Script(node);
                }
                // The input was decoded before the parse (`encoding.rs`),
                // so an encoding declaration changes nothing.
                TokenizerResult::Script(_) | TokenizerResult::EncodingIndicator(_) => {}
                TokenizerResult::Done => {
                    self.tokenizer.end();
                    self.finished = true;
                    break ParseStep::Finished;
                }
            }
        };
        std::mem::swap(&mut *sink.doc.borrow_mut(), doc);
        if step == ParseStep::Finished {
            self.positions = None;
        }
        Ok(step)
    }

    /// Where the text of the script of the last [`ParseStep::Script`]
    /// starts in the source text (line and column from 0, the column in
    /// UTF-16 code units). Zero for a parser without the scripting flag.
    pub fn script_position(&self) -> TextPosition {
        self.script_position
    }

    /// True after the parser returned [`ParseStep::Finished`].
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// True if the parser stopped at a limit of the arena.
    pub fn stopped_at_limit(&self) -> bool {
        self.tokenizer.sink.sink.stopped.get()
    }
}

/// The start of the text of `script`, whose end tag the tokenizer just
/// consumed: the input that is left in `input` tells where the end tag
/// ends. Zero without `positions` (scripting off).
fn script_start(
    positions: Option<&mut Positions>,
    input: &BufferQueue,
    doc: &Document,
    script: NodeId,
) -> TextPosition {
    let Some(positions) = positions else {
        return TextPosition::default();
    };
    // The queue has one chunk per pushed text and per write; taking them
    // out and back costs a few reference counts.
    let mut remaining = 0;
    let mut chunks = Vec::new();
    while let Some(chunk) = input.pop_front() {
        remaining += chunk.len();
        chunks.push(chunk);
    }
    for chunk in chunks {
        input.push_back(chunk);
    }
    let text_chars = doc
        .children(script)
        .filter_map(|c| doc.node(c).as_text())
        .map(|t| t.chars().count())
        .sum();
    let end = positions.len().saturating_sub(remaining);
    positions.script_start(end, text_chars)
}

/// Parses an HTML document from text, with scripting disabled.
pub fn parse_html(text: &str) -> Document {
    parse_html_with(text, false)
}

/// Parses an HTML document from text to the end, without pauses at
/// scripts. `scripting` is the scripting flag of the new document.
///
/// The parse is not journaled: the journal of the new document is
/// [incomplete](crate::Journal::incomplete) (the whole document counts as
/// changed), and it records the changes after the parse. With
/// [`HtmlParser`], the parse is journaled.
pub fn parse_html_with(text: &str, scripting: bool) -> Document {
    let mut doc = Document::new();
    doc.set_journaling(false);
    let mut parser = HtmlParser::new(&mut doc, text, scripting);
    while let Ok(ParseStep::Script(_)) = parser.resume(&mut doc) {}
    doc.set_journaling(true);
    doc
}

/// Parses an HTML document from bytes. The character encoding is determined
/// from the byte order mark, the transport-layer charset (from the
/// `Content-Type` header), `<meta>` tags, and content sniffing, in that
/// order. Returns the document and the name of the encoding used.
pub fn parse_html_bytes(bytes: &[u8], transport_charset: Option<&str>) -> (Document, &'static str) {
    parse_html_bytes_with(bytes, transport_charset, false)
}

/// [`parse_html_bytes`] with the scripting flag `scripting`.
pub fn parse_html_bytes_with(
    bytes: &[u8],
    transport_charset: Option<&str>,
    scripting: bool,
) -> (Document, &'static str) {
    let (text, encoding) = decode_html(bytes, transport_charset);
    (parse_html_with(&text, scripting), encoding.name())
}

/// Decodes HTML bytes as [`parse_html_bytes`] does, for a parse with
/// [`HtmlParser`]. Returns the text and the name of the encoding.
pub fn decode_html_bytes(bytes: &[u8], transport_charset: Option<&str>) -> (String, &'static str) {
    let (text, encoding) = decode_html(bytes, transport_charset);
    (text.into_owned(), encoding.name())
}

#[cfg(test)]
mod tests;
