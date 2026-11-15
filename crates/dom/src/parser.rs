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
//! `document.write` (ADR 0028 section 8, `write.rs`) puts its
//! text at the insertion point, in front of the unparsed rest: while a
//! script runs ([`HtmlParser::enter_script`]), the first write moves the
//! queue behind the insertion point, and [`HtmlParser::resume_written`]
//! runs the tokenizer on the written text only, up to the insertion point
//! or a `script` end tag, without ending the input.
//! With the scripting flag on, the parser also finds where the text of each
//! script starts in the source ([`HtmlParser::script_position`],
//! `position.rs`), for the positions of errors in inline scripts; scripts
//! that come from written text have none
//! ([`HtmlParser::script_written`]).
//!
//! Custom elements (ADR 0035 section 8, `token_sink.rs`): the parser also
//! pauses after a token that creates an element whose name the window's
//! registry defines ([`ParseStep::CustomElement`]): the caller constructs the
//! element and gives it back ([`HtmlParser::finish_custom_element`]). And it
//! pauses after a token whose changes queued custom element reactions that
//! the caller must invoke before the next token ([`ParseStep::Reactions`]).
//! Neither happens while the page defines no custom element.
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
mod token_sink;
mod write;

use std::rc::Rc;

use html5ever::TokenizerResult;
use html5ever::tendril::StrTendril;
use html5ever::tokenizer::{BufferQueue, Tokenizer, TokenizerOpts};
use html5ever::tree_builder::{TreeBuilder, TreeBuilderOpts};

pub use fragment::{parse_document, parse_fragment};
use position::Positions;
pub use position::TextPosition;
use sink::Sink;
pub(crate) use token_sink::PAUSE_REACTIONS;
use token_sink::{PAUSE_CUSTOM, Wrapper};
pub use write::WrittenStep;

use crate::encoding::decode_html;
use crate::mutation::DomError;
use crate::tree::{Attribute, Document, Limits, MAX_NODES, NodeId};

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
    /// The tree builder created the element of this placeholder, whose name
    /// the window's registry defines, and asked for its insertion (ADR 0035
    /// section 8): the caller constructs the element (HTML "create an
    /// element for a token" with "will execute script"; the token's
    /// attributes are [`HtmlParser::custom_element_attributes`]) and calls
    /// [`HtmlParser::finish_custom_element`] before the next
    /// [`HtmlParser::resume`].
    CustomElement(NodeId),
    /// The changes of the last token queued custom element reactions in the
    /// backup element queue ([`Document::request_parser_pause`]): the caller
    /// invokes them, then resumes.
    Reactions,
    /// The input ended and the document is complete.
    Finished,
}

/// What a `TokenizerResult::Script` of the tokenizer was.
enum Pause {
    /// The end tag of a `script` element.
    Script(NodeId),
    /// A custom element waits for its construction.
    CustomElement(NodeId),
    /// Reactions wait.
    Reactions,
    /// Nothing to do (a custom element in template contents, or at a limit
    /// of the arena).
    None,
}

/// A resumable HTML parser for one document. See the module documentation.
pub struct HtmlParser {
    tokenizer: Tokenizer<Wrapper>,
    input: Rc<BufferQueue>,
    arena_id: u64,
    finished: bool,
    /// The source text for the positions of scripts (scripting on).
    positions: Option<Positions>,
    /// The position of the text of the script of the last step.
    script_position: TextPosition,
    /// True if the script of the last step came from `document.write`.
    script_written: bool,
    /// The runs of scripts that have an insertion point (`write.rs`).
    frames: Vec<write::Frame>,
    /// Written text that the queue does not hold yet (`write.rs`).
    written: String,
    /// The bytes of written text at the front of the queue.
    queue_written: usize,
    /// The bytes of document text that the parser had not read at the last
    /// script from the document: a script whose end tag leaves at least
    /// that much input came from written text.
    doc_remaining: usize,
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
        doc.parser_pause = Some(Rc::clone(&sink.pause));
        let tree_builder = TreeBuilder::new(sink, opts);
        HtmlParser {
            tokenizer: Tokenizer::new(Wrapper::new(tree_builder), TokenizerOpts::default()),
            input,
            arena_id: doc.arena_id(),
            finished: false,
            positions: scripting.then(|| Positions::new(text)),
            script_position: TextPosition::default(),
            script_written: false,
            frames: Vec::new(),
            written: String::new(),
            queue_written: 0,
            doc_remaining: text.len(),
        }
    }

    /// Parses until the next `script` end tag, the next pause for a custom
    /// element or for reactions, or the end of the input.
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
        // The runs of scripts ended: the input is whole again.
        while !self.frames.is_empty() {
            self.leave_script();
        }
        self.flush_written();
        let sink = &self.tokenizer.sink.tree_builder.sink;
        // A request of the document between the steps has no change of the
        // parser behind it (the reactions of scripts run in their scopes).
        sink.pause.set(sink.pause.get() & !PAUSE_REACTIONS);
        std::mem::swap(&mut *sink.doc.borrow_mut(), doc);
        let step = loop {
            let result = match self.tokenizer.feed(&self.input) {
                TokenizerResult::Script(node) => match pause_of(&self.tokenizer.sink, node) {
                    Pause::Script(node) => TokenizerResult::Script(node),
                    Pause::CustomElement(node) => break ParseStep::CustomElement(node),
                    Pause::Reactions => break ParseStep::Reactions,
                    Pause::None => continue,
                },
                other => other,
            };
            match result {
                TokenizerResult::Script(node) if !sink.stopped.get() => {
                    let remaining = queue_len(&self.input);
                    self.script_written = remaining >= self.doc_remaining;
                    if self.script_written {
                        self.queue_written = remaining - self.doc_remaining;
                    } else {
                        self.doc_remaining = remaining;
                        self.queue_written = 0;
                        self.script_position = script_start(
                            self.positions.as_mut(),
                            remaining,
                            &sink.doc.borrow(),
                            node,
                        );
                    }
                    break ParseStep::Script(node);
                }
                // The input was decoded before the parse (`encoding.rs`),
                // so an encoding declaration changes nothing.
                TokenizerResult::Script(_) | TokenizerResult::EncodingIndicator(_) => {}
                TokenizerResult::Done => {
                    self.tokenizer.sink.stop_pausing();
                    self.tokenizer.end();
                    self.finished = true;
                    self.queue_written = 0;
                    break ParseStep::Finished;
                }
            }
        };
        std::mem::swap(&mut *sink.doc.borrow_mut(), doc);
        if step == ParseStep::Finished {
            self.positions = None;
            doc.parser_pause = None;
        }
        Ok(step)
    }

    /// The tree builder's sink.
    fn sink(&self) -> &Sink {
        &self.tokenizer.sink.tree_builder.sink
    }

    /// The token's attributes of the custom element of the last
    /// [`ParseStep::CustomElement`] or [`WrittenStep::CustomElement`] (a
    /// copy: the parser keeps them for a
    /// [`finish_custom_element`](Self::finish_custom_element) without an
    /// element).
    pub fn custom_element_attributes(&self) -> Vec<Attribute> {
        self.sink().pending_attributes()
    }

    /// Ends the pause of [`ParseStep::CustomElement`] (or
    /// [`WrittenStep::CustomElement`]) for `placeholder`: `element` is the
    /// element that the caller made (HTML "create an element for a token",
    /// with the token's attributes), which takes the placeholder's place in
    /// the parser and is inserted where the tree builder asked; `None`
    /// inserts the placeholder itself with the token's attributes (no
    /// constructor ran). The caller pushes an element queue before and pops
    /// and invokes it after (HTML "insert an element at the adjusted
    /// insertion location"). Another document than the parser's, or a
    /// placeholder that does not wait, changes nothing.
    pub fn finish_custom_element(
        &mut self,
        doc: &mut Document,
        placeholder: NodeId,
        element: Option<NodeId>,
    ) {
        if doc.arena_id() != self.arena_id {
            return;
        }
        self.sink().finish_pending(doc, placeholder, element);
    }

    /// Runs the parser to the end without a script runtime: scripts do not
    /// run, and custom elements are inserted as the parser made them.
    pub fn run_to_end(&mut self, doc: &mut Document) {
        loop {
            match self.resume(doc) {
                Ok(ParseStep::Script(_) | ParseStep::Reactions) => {}
                Ok(ParseStep::CustomElement(placeholder)) => {
                    self.finish_custom_element(doc, placeholder, None);
                }
                Ok(ParseStep::Finished) | Err(_) => return,
            }
        }
    }

    /// Where the text of the script of the last [`ParseStep::Script`]
    /// starts in the source text (line and column from 0, the column in
    /// UTF-16 code units). Zero for a parser without the scripting flag.
    pub fn script_position(&self) -> TextPosition {
        self.script_position
    }

    /// True if the script of the last [`ParseStep::Script`] or
    /// [`WrittenStep::Script`] came from `document.write`: its text has no
    /// place in the source, and [`script_position`](Self::script_position)
    /// does not apply.
    pub fn script_written(&self) -> bool {
        self.script_written
    }

    /// True after the parser returned [`ParseStep::Finished`].
    #[cfg(test)]
    pub(crate) fn is_finished(&self) -> bool {
        self.finished
    }

    /// True if the parser stopped at a limit of the arena.
    pub fn stopped_at_limit(&self) -> bool {
        self.sink().stopped.get()
    }
}

/// What a `TokenizerResult::Script(node)` of the tokenizer means: the end
/// tag of a script, or a pause of the token sink. A custom element that the
/// tree builder did not insert, or one at a limit of the arena, is not
/// constructed: its placeholder stays as an ordinary element.
fn pause_of(wrapper: &Wrapper, node: NodeId) -> Pause {
    let bits = wrapper.take_paused();
    if bits == 0 {
        return Pause::Script(node);
    }
    let sink = &wrapper.tree_builder.sink;
    if bits & PAUSE_CUSTOM != 0 {
        match sink.pending_inserted() {
            // Reactions of the same token wait in the backup queue; the
            // caller runs them before the construction.
            Some(placeholder) if !sink.stopped.get() => {
                return Pause::CustomElement(placeholder);
            }
            _ => sink.abandon_pending(),
        }
    }
    if bits & PAUSE_REACTIONS != 0 && !sink.stopped.get() {
        return Pause::Reactions;
    }
    Pause::None
}

/// The number of bytes in `input`.
fn queue_len(input: &BufferQueue) -> usize {
    // The queue has one chunk per pushed text; taking them out and back
    // costs a few reference counts. The callers run this once per script,
    // so the number of chunks must stay small: the waiting writes of a
    // script join into one text (`write.rs`), not one chunk per write.
    let mut total = 0;
    let mut chunks = Vec::new();
    while let Some(chunk) = input.pop_front() {
        total += chunk.len();
        chunks.push(chunk);
    }
    for chunk in chunks {
        input.push_back(chunk);
    }
    total
}

/// The start of the text of `script`, whose end tag the tokenizer just
/// consumed: the `remaining` bytes of input tell where the end tag ends.
/// Zero without `positions` (scripting off).
fn script_start(
    positions: Option<&mut Positions>,
    remaining: usize,
    doc: &Document,
    script: NodeId,
) -> TextPosition {
    let Some(positions) = positions else {
        return TextPosition::default();
    };
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
    parse_html_limited(text, scripting, MAX_NODES)
}

/// [`parse_html_with`] for a document that may hold at most `max_nodes`
/// nodes: the parser stops there (a caller that shares a node budget uses
/// it to stop early and see that the document does not fit).
pub fn parse_html_limited(text: &str, scripting: bool, max_nodes: usize) -> Document {
    let mut doc = Document::with_limits(Limits {
        max_nodes,
        ..Limits::default()
    });
    doc.set_journaling(false);
    let mut parser = HtmlParser::new(&mut doc, text, scripting);
    parser.run_to_end(&mut doc);
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
    parse_html_bytes_limited(bytes, transport_charset, scripting, MAX_NODES)
}

/// [`parse_html_bytes_with`] for a document of at most `max_nodes` nodes
/// (see [`parse_html_limited`]).
pub fn parse_html_bytes_limited(
    bytes: &[u8],
    transport_charset: Option<&str>,
    scripting: bool,
    max_nodes: usize,
) -> (Document, &'static str) {
    let (text, encoding) = decode_html(bytes, transport_charset);
    (
        parse_html_limited(&text, scripting, max_nodes),
        encoding.name(),
    )
}

/// Decodes HTML bytes as [`parse_html_bytes`] does, for a parse with
/// [`HtmlParser`]. Returns the text and the name of the encoding.
pub fn decode_html_bytes(bytes: &[u8], transport_charset: Option<&str>) -> (String, &'static str) {
    let (text, encoding) = decode_html(bytes, transport_charset);
    (text.into_owned(), encoding.name())
}

#[cfg(test)]
mod tests;
