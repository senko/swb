//! `document.write()` for the resumable parser (HTML "document write
//! steps", <https://html.spec.whatwg.org/multipage/dynamic-markup-insertion.html#document-write-steps>;
//! ADR 0028 section 8).
//!
//! While a script that the parser started runs, the parser has an
//! *insertion point* (HTML "insertion point"): the place in the input where
//! the text of `write` goes, just in front of the text that the parser has
//! not read. The caller marks the run of such a script with
//! [`HtmlParser::enter_script`] and [`HtmlParser::leave_script`]; a script in
//! the written text runs nested, so the runs form a stack, and its depth is
//! the script nesting level.
//!
//! The queue of the tokenizer holds the text in front of the insertion
//! point, and a [`Frame`] holds the text behind it. The first
//! [`write`](HtmlParser::write) of a run moves the whole queue into the
//! frame; later writes of the run add to `written` (a `String`, so a flood
//! of small writes costs no chunks). [`resume_written`](HtmlParser::resume_written)
//! gives `written` to the tokenizer and runs it up to the insertion point
//! (the end of the queue), or to a `script` end tag: the caller then runs the
//! script and calls it again. It never ends the input: a token that
//! the written text leaves open (`<di`, `</scr`) waits for the next text.
//! Writes that the caller does not process (a pending parsing-blocking
//! script holds the parser) stay in `written`, behind the text of earlier
//! writes that is still in the queue. [`HtmlParser::leave_script`] puts the
//! text behind the insertion point back, after all the written text.
//!
//! Positions of inline scripts count the bytes of document text that the
//! parser has not read. Written text has no place in the document source,
//! so the parser tells the scripts that come from it
//! ([`HtmlParser::script_written`]): errors in them count from the start of
//! their own text.

use html5ever::TokenizerResult;
use html5ever::tokenizer::BufferQueue;

use super::{HtmlParser, push_text};
use crate::mutation::DomError;
use crate::tree::{Document, NodeId};

/// The text behind the insertion point of one running script.
#[derive(Default)]
pub(super) struct Frame {
    /// The text that was in the queue at the first write of the run.
    held: Option<BufferQueue>,
    /// The bytes of `held` that are written text (not document text).
    held_written: usize,
}

/// The result of [`HtmlParser::resume_written`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WrittenStep {
    /// The tree builder popped this `script` element at its end tag
    /// (inside the written text): the script may run now, then the caller
    /// calls [`HtmlParser::resume_written`] again.
    Script(NodeId),
    /// The parser read all the written text, up to the insertion point.
    Parsed,
}

impl HtmlParser {
    /// Starts the run of a script that the parser reached: from now on
    /// [`write`](Self::write) has an insertion point, just in front of the
    /// text that the parser has not read. Pair it with
    /// [`leave_script`](Self::leave_script).
    pub fn enter_script(&mut self) {
        self.flush_written();
        self.frames.push(Frame::default());
    }

    /// Ends the run of the script that [`enter_script`](Self::enter_script)
    /// started. The text that the parser has not read, written text first,
    /// is in front of the queue again. A call without a run does nothing.
    pub fn leave_script(&mut self) {
        let Some(frame) = self.frames.pop() else {
            return;
        };
        self.flush_written();
        if let Some(held) = frame.held {
            // At a limit of the arena the parser is stopped and the held
            // text is dropped, and so is its count.
            if !self.stopped_at_limit() {
                while let Some(chunk) = held.pop_front() {
                    self.input.push_back(chunk);
                }
                self.queue_written += frame.held_written;
            }
        }
    }

    /// The script nesting level (HTML "script nesting level"): the number
    /// of runs of scripts that the parser started and that have not ended.
    /// There is an insertion point when it is above 0.
    pub fn script_depth(&self) -> usize {
        self.frames.len()
    }

    /// Puts `text` into the input at the insertion point (HTML "insert
    /// input immediately before the insertion point"): after the written
    /// text that the parser has not read, in front of the rest of the
    /// document. The parser reads it at the next
    /// [`resume_written`](Self::resume_written) or
    /// [`resume`](Self::resume). Returns false, and drops the text, when
    /// there is no insertion point or the parser stopped at a limit of the
    /// arena.
    pub fn write(&mut self, text: &str) -> bool {
        if self.stopped_at_limit() {
            return false;
        }
        let Some(frame) = self.frames.last_mut() else {
            return false;
        };
        if frame.held.is_none() {
            let held = BufferQueue::default();
            self.input.swap_with(&held);
            frame.held = Some(held);
            frame.held_written = std::mem::take(&mut self.queue_written);
        }
        self.written.push_str(text);
        true
    }

    /// The bytes of written text that the parser has not read, at all
    /// insertion points. The caller bounds it (ADR 0028 section 16).
    pub fn written_bytes(&self) -> usize {
        self.frames
            .iter()
            .map(|f| f.held_written)
            .sum::<usize>()
            .saturating_add(self.queue_written)
            .saturating_add(self.written.len())
    }

    /// Reads the written text up to the insertion point, or up to the next
    /// `script` end tag. `doc` must be the document that the parser started
    /// with. Without a [`write`](Self::write) in the current run it does
    /// nothing. It does not end the input.
    pub fn resume_written(&mut self, doc: &mut Document) -> Result<WrittenStep, DomError> {
        if doc.arena_id() != self.arena_id {
            return Err(DomError::WrongDocument);
        }
        if self.finished || self.frames.last().is_none_or(|f| f.held.is_none()) {
            return Ok(WrittenStep::Parsed);
        }
        self.flush_written();
        let sink = &self.tokenizer.sink.sink;
        std::mem::swap(&mut *sink.doc.borrow_mut(), doc);
        let step = loop {
            match self.tokenizer.feed(&self.input) {
                TokenizerResult::Script(node) if !sink.stopped.get() => {
                    break WrittenStep::Script(node);
                }
                TokenizerResult::Script(_) | TokenizerResult::EncodingIndicator(_) => {}
                TokenizerResult::Done => break WrittenStep::Parsed,
            }
        };
        std::mem::swap(&mut *sink.doc.borrow_mut(), doc);
        // The queue holds written text only: the document text is behind
        // the insertion point.
        self.queue_written = super::queue_len(&self.input);
        if matches!(step, WrittenStep::Script(_)) {
            self.script_written = true;
        }
        Ok(step)
    }

    /// Gives the written text that waits in `written` to the queue.
    pub(super) fn flush_written(&mut self) {
        if self.written.is_empty() {
            return;
        }
        if !self.stopped_at_limit() {
            push_text(&self.input, &self.written);
            self.queue_written = self.queue_written.saturating_add(self.written.len());
        }
        self.written = String::new();
    }
}
