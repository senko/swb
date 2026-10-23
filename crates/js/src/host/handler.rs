//! Event handler functions (ADR 0028 section 6, item 6; HTML "getting the
//! current value of the event handler",
//! <https://html.spec.whatwg.org/multipage/webappapis.html#getting-the-current-value-of-the-event-handler>):
//! a function compiled from the text of an event handler content
//! attribute, with a scope chain of object environments.
//!
//! The source is `(function NAME(PARAMS) {\n` body `\n})`, so that
//! `Function.prototype.toString` gives `function onclick(event) {\n...\n}`,
//! as in Chromium; the parse (`swb_js_syntax::parse_event_handler`) has the
//! early errors of a function body and binds no name `NAME` in the
//! function (the name `onclick` in the body is the element's property).
//! The free names of the function resolve like those of direct eval code
//! whose caller is inside `with` statements: the compile gives each such
//! reference a dynamic lookup through all records of a captured
//! environment chain, then the global environment (`compiler/eval.rs`).
//! The chain holds one object environment record (with `@@unscopables`, as
//! HTML's `NewObjectEnvironment(O, true, E)`) per scope object: the
//! element, then its form owner, then the document.
//!
//! The function is an ordinary function of the realm (`MakeConstructor`:
//! it has a `prototype`), sloppy unless its body has a `"use strict"`
//! directive; its `name` is `NAME` and its `length` the parameter count.
//! It owns its source text for the heap accounting.

use std::rc::Rc;

use swb_js_text::String16;

use crate::compiler::compile_event_handler;
use crate::heap::Gc;
use crate::object::Object;
use crate::runtime::{Runtime, ScriptError, compile_error, finish_error};
use crate::value::Value;
use crate::vm::{VmError, VmResult};

/// The text before the name.
const PREFIX: &str = "(function ";
/// The text between the parameters and the body.
const MIDDLE: &str = ") {\n";
/// The text after the body.
const SUFFIX: &str = "\n})";

/// The source and the scope of an event handler for
/// [`Runtime::compile_event_handler`].
#[derive(Clone, Copy, Debug)]
pub struct EventHandler<'a> {
    /// The realm of the function.
    pub realm: u32,
    /// The name of the handler (`onclick`): the function's `name`.
    pub name: &'a str,
    /// The parameter names (`event`; `event, source, lineno, colno, error`
    /// for `onerror` of the global object).
    pub params: &'a [&'a str],
    /// The text of the attribute: the function body.
    pub body: &'a str,
    /// The objects of the scope chain, the outermost first (HTML: the
    /// document, the form owner, the element); the global environment is
    /// outside them.
    pub scope: &'a [Gc<Object>],
    /// The name of the script in stack traces (the document's URL).
    pub script_name: &'a str,
}

/// The source text of a handler, the offset of the `)` after its
/// parameters and the offset of its body.
fn handler_source(handler: &EventHandler<'_>) -> (String16, u32, u32) {
    let mut text = String16::from(PREFIX);
    text.push_str16(String16::from(handler.name).as_str16());
    text.push(u16::from(b'('));
    for (index, param) in handler.params.iter().enumerate() {
        if index > 0 {
            text.push_str16(String16::from(", ").as_str16());
        }
        text.push_str16(String16::from(*param).as_str16());
    }
    let params_end = text.len() as u32;
    text.push_str16(String16::from(MIDDLE).as_str16());
    let body_start = text.len() as u32;
    text.push_str16(String16::from(handler.body).as_str16());
    text.push_str16(String16::from(SUFFIX).as_str16());
    (text, params_end, body_start)
}

impl Runtime {
    /// Compiles an event handler and returns its function object. An early
    /// error of the body is [`ScriptError::Compile`] with the offset in
    /// code units from the start of the body (the host reports it, as HTML
    /// says, and the handler's value becomes `null`). The function stays
    /// alive until the next call that runs script code
    /// ([`Runtime::last_value`]); the host stores it in its traced state.
    pub fn compile_event_handler(
        &mut self,
        handler: &EventHandler<'_>,
    ) -> Result<Gc<Object>, ScriptError> {
        let (text, params_end, body_start) = handler_source(handler);
        let body = (
            body_start,
            body_start.saturating_add(String16::from(handler.body).len() as u32),
        );
        swb_js_syntax::check_source_len(text.len()).map_err(ScriptError::from)?;
        let name = String16::from(handler.name);
        // The tree of the source takes some bytes per code unit (as for
        // eval code).
        let room = text.len().saturating_mul(crate::vm::PARSE_BYTES_PER_UNIT);
        self.heap
            .reserve(room, &self.vm)
            .map_err(|e| finish_error(e.into()))?;
        let script =
            swb_js_syntax::parse_event_handler(text.as_str16(), params_end, &mut self.budget)
                .map_err(|error| body_relative(ScriptError::from(error), body))?;
        let size = script.ast.heap_size() + script.scopes.heap_size();
        self.heap
            .reserve(size, &self.vm)
            .map_err(|e| finish_error(e.into()))?;
        let (code, compiled) = compile_event_handler(
            &mut self.heap,
            &script,
            (text.as_str16(), Rc::from(handler.script_name)),
            name,
            &mut self.budget,
        )
        .map_err(|error| body_relative(compile_error(error), body))?;
        // The code is not rooted until the closure exists; nothing between
        // collects (the allocations below are of fixed size).
        let function = self
            .scoped(|rt| {
                rt.heap.record(code);
                let record = rt.handler_records(handler.scope)?;
                let cell = rt.heap.alloc_cell(record)?;
                rt.heap.record(cell);
                let captures = compiled
                    .captures
                    .iter()
                    .map(|source| match source {
                        swb_js_syntax::CaptureSource::ParentCapture(0) => Ok(cell),
                        _ => Err(VmError::invariant(
                            "an event handler that captures more than its scope",
                        )),
                    })
                    .collect::<VmResult<Box<[_]>>>()?;
                rt.new_closure(code, &compiled, captures, handler.realm)
            })
            .map_err(finish_error)?;
        self.vm.last_value = function.into();
        Ok(function)
    }

    /// The chain of object environment records of `scope` (the outermost
    /// first); the innermost record, or `undefined` for an empty scope.
    fn handler_records(&mut self, scope: &[Gc<Object>]) -> VmResult<Value> {
        let mut record = Value::Undefined;
        for &object in scope {
            record = self.new_with_env(object.into(), record)?;
            self.heap.record(record);
        }
        Ok(record)
    }
}

/// A compile error with its offset counted from the body, at most the
/// body's end (an error in the text after the body, such as the `}` that
/// ends the function early, is at the end).
fn body_relative(error: ScriptError, (start, end): (u32, u32)) -> ScriptError {
    match error {
        ScriptError::Compile {
            kind,
            message,
            offset,
        } => ScriptError::Compile {
            kind,
            message,
            offset: offset.clamp(start, end) - start,
        },
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_source_has_the_form_of_chromium() {
        let handler = EventHandler {
            realm: 0,
            name: "onerror",
            params: &["event", "source", "lineno"],
            body: "return 1",
            scope: &[],
            script_name: "",
        };
        let (text, params_end, body_start) = handler_source(&handler);
        let text = text.as_str16().to_string_lossy();
        assert_eq!(
            text,
            "(function onerror(event, source, lineno) {\nreturn 1\n})"
        );
        assert_eq!(text.as_bytes().get(params_end as usize), Some(&b')'));
        assert_eq!(&text[body_start as usize..], "return 1\n})");
    }
}
