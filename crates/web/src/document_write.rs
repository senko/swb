//! `document.write()`, `writeln()`, `open()` and `close()` (HTML "dynamic
//! markup insertion",
//! <https://html.spec.whatwg.org/multipage/dynamic-markup-insertion.html>;
//! ADR 0028 section 8).
//!
//! While a script that the parser started runs, the parser has an insertion
//! point ([`swb_dom::HtmlParser::enter_script`], `document_load.rs`).
//! `write` puts its text there, and the parser reads it before `write`
//! returns, up to the insertion point or up to a `script` end tag. A script
//! in the written text is prepared as the parser would: an inline one runs
//! inside `write` (nested, with an insertion point of its own: the script
//! nesting level), an external one becomes the pending parsing-blocking
//! script. Then `write` returns without reading more; the parse waits after
//! the current script ends (`document_load.rs`, `parse`), and the text that
//! the later calls write waits behind it, in order.
//!
//! Chromium measurements (Chromium 148, `tools/probes/web-document-write.json`)
//! and the departures:
//!
//! - A script in written text has the URL "" in errors and stack traces and
//!   counts lines and columns from the start of its own text. The scripts of
//!   the document keep the document's URL and positions.
//! - Nesting stops at level 21: a `write` of a script at level 22 or more
//!   does nothing and does not throw (a script that writes itself runs 22
//!   times). The HTML standard has no such limit; swb follows Chromium and
//!   logs a warning ([`MAX_NESTING`]).
//! - A write with no insertion point (from a timer, a listener, an async or
//!   deferred script, after the parse, or while the parser waits for a
//!   script) runs `document.open()` in the standard, and Chromium replaces
//!   the document. Departure: swb ignores the text with a warning, in the
//!   top-level document and in frame documents (no target calls `open()` on
//!   a frame; ADR 0031 "Done (feature 15)"). The same goes for a write to a
//!   document that has no parser (`createHTMLDocument()`), which Chromium
//!   replaces too.
//! - `open()` with a script nesting level above 0 returns the document and
//!   does nothing, as the standard says (step 8 of "document open steps").
//!   Otherwise it would replace the document: swb ignores it with a warning
//!   and returns the document. `close()` returns at once, as the standard
//!   says without a script-created parser (swb never makes one).
//! - Written text that waits behind a pending parsing-blocking script is
//!   bounded ([`MAX_WAITING_BYTES`]); past it `write` drops its text with a
//!   warning. Chromium has no bound.
//! - The inline scripts from written text that run are bounded per document
//!   ([`MAX_WRITTEN_SCRIPTS`]); past it `write` drops its text with a
//!   warning. Without it, scripts that each write two more scripts grow
//!   until the task time limit, to more than 1 GiB (hostile case
//!   `document-write-fork-js-on`). Chromium has no bound.
//! - Microtasks of a written script run when the outermost script ends, as
//!   in Chromium (measured), not at the end of the written script.
//!
//! Out of scope: XML documents (swb has none), Trusted Types, the
//! throw-on-dynamic-markup-insertion counter (swb has no custom elements),
//! and Chromium's intervention that blocks written cross-site scripts on
//! slow connections.

use swb_dom::{NodeId, WrittenStep};
use swb_js::{ScriptError, VmError, VmResult};

use crate::bindings::{self as b, Cx};
use crate::scripts::{ParserScript, Phase, Prepared, scripts, scripts_mut};
use crate::state::engine_mut;

/// The script nesting level above which `write` does nothing (Chromium
/// 148; measured with a script that writes itself).
pub(crate) const MAX_NESTING: usize = 21;

/// The most bytes of written text that wait for the parser per document,
/// also the largest single write (ADR 0028 section 16).
pub(crate) const MAX_WAITING_BYTES: usize = 64 << 20;

/// The most inline scripts from written text that run per document (ADR
/// 0028 section 16).
pub(crate) const MAX_WRITTEN_SCRIPTS: usize = 100_000;

/// `document.write(...text)` and `writeln`: HTML `document.write()` with
/// the text joined (and a newline for `writeln`).
pub(crate) fn write(
    cx: &mut Cx<'_>,
    this: NodeId,
    text: &[String],
    newline: bool,
) -> b::Result<()> {
    let mut input = text.concat();
    if newline {
        input.push('\n');
    }
    Ok(document_write_steps(cx.rt, this, &input)?)
}

/// `document.open()`: HTML "document open steps". Returns the document.
pub(crate) fn open(cx: &mut Cx<'_>, this: NodeId) -> b::Result<NodeId> {
    if this == NodeId::DOCUMENT && script_depth(cx.rt)? > 0 {
        // Step 8: an active parser whose script nesting level is above 0.
        return Ok(this);
    }
    ignored_without_point(cx.rt, "document.open()")?;
    Ok(this)
}

/// `document.close()`: HTML "document close steps" return when the document
/// has no script-created parser, and `open()` never makes one.
#[allow(
    clippy::unnecessary_wraps,
    reason = "the signature of the generated trait"
)]
pub(crate) fn close(_cx: &mut Cx<'_>, _this: NodeId) -> b::Result<()> {
    Ok(())
}

/// The script nesting level of the window's document: the number of scripts
/// that the parser started and that run now (0 when none does).
fn script_depth(rt: &mut swb_js::Runtime) -> VmResult<usize> {
    if scripts(rt)?.phase != Phase::Parsing {
        return Ok(0);
    }
    Ok(engine_mut(rt)?.parser().map_or(0, |p| p.script_depth()))
}

/// Why swb did not run a call. Each reason logs one warning per document
/// and debug lines after it (a page can call `write` a million times).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reason {
    /// No insertion point: the standard replaces the document.
    NoInsertionPoint = 1,
    /// The script nesting level is above [`MAX_NESTING`].
    TooDeep = 2,
    /// The text would pass [`MAX_WAITING_BYTES`].
    TooMuch = 4,
    /// [`MAX_WRITTEN_SCRIPTS`] written scripts ran.
    TooMany = 8,
}

/// Logs a call that swb does not run, with `detail` for the warning.
fn ignored(
    rt: &mut swb_js::Runtime,
    reason: Reason,
    detail: std::fmt::Arguments<'_>,
) -> VmResult<()> {
    let scripts = scripts_mut(rt)?;
    if scripts.ignored_write_logged & reason as u8 == 0 {
        scripts.ignored_write_logged |= reason as u8;
        log::warn!("{detail}");
    } else {
        log::debug!("{detail}");
    }
    Ok(())
}

/// [`ignored`] for a call without an insertion point.
fn ignored_without_point(rt: &mut swb_js::Runtime, what: &str) -> VmResult<()> {
    ignored(
        rt,
        Reason::NoInsertionPoint,
        format_args!(
            "{what} ignored: the document has no insertion point (a call from a timer, a \
             listener, an async script or after the parse replaces the document in other \
             browsers; swb does not)"
        ),
    )
}

/// HTML "document write steps": puts `input` at the insertion point and
/// lets the parser read it.
fn document_write_steps(rt: &mut swb_js::Runtime, document: NodeId, input: &str) -> VmResult<()> {
    let depth = if document == NodeId::DOCUMENT {
        script_depth(rt)?
    } else {
        0
    };
    if depth == 0 {
        return ignored_without_point(rt, "document.write()");
    }
    if depth > MAX_NESTING {
        return ignored(
            rt,
            Reason::TooDeep,
            format_args!(
                "document.write() ignored: the script nesting level {depth} is above \
                 {MAX_NESTING}"
            ),
        );
    }
    if scripts(rt)?.written_scripts_run >= MAX_WRITTEN_SCRIPTS {
        return ignored(
            rt,
            Reason::TooMany,
            format_args!(
                "document.write() ignored: {MAX_WRITTEN_SCRIPTS} scripts from written text ran \
                 in this document"
            ),
        );
    }
    let Some(parser) = engine_mut(rt)?.parser() else {
        return ignored_without_point(rt, "document.write()");
    };
    let waiting = parser.written_bytes().saturating_add(input.len());
    if waiting > MAX_WAITING_BYTES {
        return ignored(
            rt,
            Reason::TooMuch,
            format_args!(
                "document.write() ignored: {waiting} bytes would wait for the parser (the limit \
                 is {MAX_WAITING_BYTES})"
            ),
        );
    }
    if !parser.write(input) {
        log::debug!("document.write() ignored: the parser stopped at a limit");
        return Ok(());
    }
    // Step 8: with a pending parsing-blocking script the parser waits, and
    // the text stays in the input.
    if scripts(rt)?.pending_parsing_blocking.is_some() {
        return Ok(());
    }
    read_written_text(rt)
}

/// Lets the parser read the written text, and runs the scripts in it (each
/// nested in the call of `write`), until the insertion point or until a
/// script holds the parser.
fn read_written_text(rt: &mut swb_js::Runtime) -> VmResult<()> {
    loop {
        let step = engine_mut(rt)?.parse_written_step();
        let inserted = engine_mut(rt)?.take_inserted_frames();
        crate::frames::parser_inserted(rt, inserted)?;
        crate::scripts::written_links(rt)?;
        let Some(WrittenStep::Script(node)) = step else {
            return Ok(());
        };
        run_written_script(rt, node)?;
        if scripts(rt)?.pending_parsing_blocking.is_some() {
            return Ok(());
        }
    }
}

/// The `script` end tag of the written text: "prepare the script element",
/// then, for an inline script, run it with an insertion point of its own.
/// The microtask checkpoint before the script (HTML 13.2.6.4.8) does not
/// run: a script is running, as in Chromium (measured).
fn run_written_script(rt: &mut swb_js::Runtime, node: NodeId) -> VmResult<()> {
    match crate::scripts::prepare(rt, node, Some(ParserScript::Written))? {
        Prepared::ExecuteNow => {
            scripts_mut(rt)?.written_scripts_run += 1;
            if let Some(parser) = engine_mut(rt)?.parser() {
                parser.enter_script();
            }
            let result = crate::scripts::execute(rt, node);
            if let Some(parser) = engine_mut(rt)?.parser() {
                parser.leave_script();
            }
            match result {
                Err(ScriptError::Terminated(termination)) => Err(VmError::Terminated(termination)),
                Err(ScriptError::Internal(error)) => Err(VmError::Internal(error)),
                _ => Ok(()),
            }
        }
        Prepared::Blocking => {
            scripts_mut(rt)?.parser_waited = true;
            Ok(())
        }
        Prepared::Nothing => Ok(()),
    }
}
