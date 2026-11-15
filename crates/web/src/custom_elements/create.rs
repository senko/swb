//! DOM "create an element" (DOM §4.9,
//! <https://dom.spec.whatwg.org/#concept-create-element>; ADR 0035 section
//! 4) with the synchronous custom elements flag set: `createElement` and
//! `createElementNS` run the constructor of a defined name inside the call,
//! and so does the parser (HTML "create an element for a token" with "will
//! execute script", ADR 0035 section 8: [`parser_custom_element`]).
//!
//! A constructor that throws, or whose result fails a check, is reported
//! (not thrown), and the result is a new `HTMLUnknownElement` in the state
//! `failed` (measured, case `create-element-failures`). At the nesting limit
//! the constructor does not run and the result is that element, without a
//! report (measured, case `reactions-depth-limit`). The parser's reports of
//! a failed check have no operation in the message and the document's URL
//! and 0:0 as the place (measured, case `parser-sync-failures`).

use swb_dom::{
    Attribute, CustomElementState, ElementData, LocalName, NodeData, NodeId, QualName, ns,
};
use swb_js::{Runtime, ScriptError, SourceLocation, ThrowKind, Value, VmError, VmResult};

use super::reactions;
use crate::bindings::{self as b, Exception};
use crate::generated as g;
use crate::state::{dom, dom_mut, engine_mut, state_mut};

/// Who creates an element synchronously: the prefix and the place of the
/// reports of a failed construction.
#[derive(Clone, Copy)]
enum Operation<'a> {
    /// A method of `Document` (`createElement`, `createElementNS`).
    Method(&'a str),
    /// The parser.
    Parser,
}

/// "Create an element" of `name` in the document `document` (a document
/// node of the current arena) for the operation `operation` of `Document`
/// (`createElement`, `createElementNS`: the prefix of the reported
/// messages). `hyphen` is whether the local name has a hyphen, which the
/// caller knows from validating it (the cheap test of ADR 0035 "4a
/// implementation notes", performance).
#[inline]
pub(crate) fn create_element(
    rt: &mut Runtime,
    document: NodeId,
    name: QualName,
    hyphen: bool,
    operation: &str,
) -> b::Result<NodeId> {
    if super::any_definitions(rt) {
        return create_with_definitions(rt, document, name, hyphen, operation);
    }
    plain_element(rt, document, name, hyphen)
}

/// [`create_element`] while the agent has definitions (out of line: the
/// frame of the common case stays small).
#[inline(never)]
fn create_with_definitions(
    rt: &mut Runtime,
    document: NodeId,
    name: QualName,
    hyphen: bool,
    operation: &str,
) -> b::Result<NodeId> {
    if name.ns == ns!(html)
        && let Some(number) = super::look_up(rt, dom(rt)?.node_document(document), &name.local)
    {
        return construct(rt, document, name, number, Operation::Method(operation));
    }
    plain_element(rt, document, name, hyphen)
}

/// A new element that no definition constructs.
#[inline]
fn plain_element(
    rt: &mut Runtime,
    document: NodeId,
    name: QualName,
    hyphen: bool,
) -> b::Result<NodeId> {
    let data = NodeData::Element(ElementData::new_with_hyphen(name, Vec::new(), hyphen));
    let result = dom_mut(rt)?.create_node_in(document, data);
    crate::dom::dom_result(rt, result)
}

/// Step 6.1 of "create an element": `Construct(C)` and the checks of its
/// result.
#[inline(never)]
fn construct(
    rt: &mut Runtime,
    document: NodeId,
    name: QualName,
    number: u32,
    operation: Operation<'_>,
) -> b::Result<NodeId> {
    let reactions = reactions::shared(rt)?;
    if !reactions.borrow_mut().enter() {
        return failed_element(rt, document, name);
    }
    let constructor = super::definition(rt, number)?.constructor;
    let result = rt.construct_function(Value::Object(constructor), &[]);
    reactions.borrow_mut().leave();
    let value = match result {
        Ok(value) => value,
        Err(ScriptError::Terminated(termination)) => {
            return Err(VmError::Terminated(termination).into());
        }
        Err(ScriptError::Internal(error)) => return Err(VmError::Internal(error).into()),
        Err(error) => {
            let site = crate::errors::Site::named("a custom element constructor");
            match operation {
                Operation::Method(_) => {
                    crate::errors::report_callback(rt, constructor, &site, &error)?;
                }
                Operation::Parser => {
                    crate::errors::report_parser_constructor(rt, constructor, &site, &error)?;
                }
            }
            return failed_element(rt, document, name);
        }
    };
    match check_result(rt, value, document, &name.local) {
        Ok(element) => {
            if name.prefix.is_some() {
                dom_mut(rt)?.set_element_prefix(element, name.prefix);
            }
            Ok(element)
        }
        Err(exception) => {
            report_failure(rt, operation, exception)?;
            failed_element(rt, document, name)
        }
    }
}

/// The checks of step 6.1.3 of "create an element", with Chromium's
/// messages (measured, case `create-element-failures`).
fn check_result(
    rt: &Runtime,
    value: Value,
    document: NodeId,
    local: &LocalName,
) -> Result<NodeId, Exception> {
    let not_supported = |message: &'static str| Exception::dom("NotSupportedError", message);
    let element = rt
        .host_data_of(value, g::HTML_ELEMENT)
        .ok()
        .and_then(|data| b::node_here(rt, data))
        .ok_or_else(|| Exception::type_error("The result must implement HTMLElement interface"))?;
    let dom = dom(rt)?;
    let Some(data) = dom.element(element) else {
        return Err(Exception::type_error(
            "The result must implement HTMLElement interface",
        ));
    };
    if !data.attributes().is_empty() {
        return Err(not_supported("The result must not have attributes"));
    }
    if dom.child_count(element) > 0 {
        return Err(not_supported("The result must not have children"));
    }
    if dom.parent(element).is_some() {
        return Err(not_supported("The result must not have a parent"));
    }
    if dom.node_document(element) != document {
        // Measured, case `create-element-messages`: a constructor that
        // returns an element of `createHTMLDocument()`.
        return Err(not_supported("The result must be in the same document"));
    }
    if data.name.local != *local {
        return Err(not_supported("The result must have the same localName"));
    }
    Ok(element)
}

/// Reports a failed check: the error of `exception` with the prefix of the
/// operation, made here and not thrown.
fn report_failure(
    rt: &mut Runtime,
    operation: Operation<'_>,
    exception: Exception,
) -> b::Result<()> {
    let (prefix, location) = match operation {
        Operation::Method(name) => (
            format!("Failed to execute '{name}' on 'Document': "),
            rt.script_location(),
        ),
        Operation::Parser => (
            String::new(),
            Some(SourceLocation {
                name: crate::scripts::document_url(rt),
                line: 0,
                column: 0,
            }),
        ),
    };
    match exception {
        Exception::Dom { name, message } => {
            let realm = crate::state::doc_realm(rt)?;
            let message = format!("{prefix}{message}");
            let error = b::dom_exception::create(rt, realm, g::DOM_EXCEPTION, name, &message)?;
            crate::errors::report_host_error_at(rt, Value::Object(error), true, location)?;
        }
        Exception::Type(message) => {
            let error = rt.new_error(ThrowKind::TypeError, &format!("{prefix}{message}"))?;
            crate::errors::report_host_error_at(rt, Value::Object(error), false, location)?;
        }
        other => return Err(other),
    }
    Ok(())
}

/// The result of a failed synchronous construction: a new
/// `HTMLUnknownElement` in the state `failed`, without a definition.
fn failed_element(rt: &mut Runtime, document: NodeId, name: QualName) -> b::Result<NodeId> {
    let mut data = ElementData::new(name, Vec::new());
    data.set_custom_element_state(CustomElementState::Failed, 0);
    let result = dom_mut(rt)?.create_node_in(document, NodeData::Element(data));
    let element = crate::dom::dom_result(rt, result)?;
    crate::state::wrap_node_as(rt, element, g::HTML_UNKNOWN_ELEMENT)?;
    Ok(element)
}

/// The parser's pause for a custom element (`swb_dom::ParseStep::CustomElement`,
/// `WrittenStep::CustomElement`; ADR 0035 section 8): the reactions that
/// the token queued before, "create an element for a token" for the
/// element of `placeholder`, then its insertion in an element queue whose
/// reactions (`connectedCallback`) run before the parser goes on (HTML
/// "insert an element at the adjusted insertion location"). Without an
/// element (a termination, a limit of the arena) the placeholder is
/// inserted with the token's attributes.
pub(crate) fn parser_custom_element(rt: &mut Runtime, placeholder: NodeId) -> VmResult<()> {
    let limit = reactions::start_parser_level(rt)?;
    let result = parser_custom_element_scope(rt, placeholder);
    reactions::end_parser_level(rt, limit);
    result
}

fn parser_custom_element_scope(rt: &mut Runtime, placeholder: NodeId) -> VmResult<()> {
    let outermost = rt.stack_depths().0 == 0;
    let checkpoints = std::mem::replace(
        &mut state_mut(rt)?.event_loop.listener_checkpoints,
        outermost,
    );
    let result = parser_custom_element_steps(rt, placeholder);
    state_mut(rt)?.event_loop.listener_checkpoints = checkpoints;
    if result.is_err() {
        engine_mut(rt)?.finish_custom_element(placeholder, None);
    }
    result
}

fn parser_custom_element_steps(rt: &mut Runtime, placeholder: NodeId) -> VmResult<()> {
    reactions::flush_backup(rt)?;
    let name = dom(rt)?.element(placeholder).map(|e| e.name.clone());
    let attrs = engine_mut(rt)?
        .parser()
        .map(|p| p.custom_element_attributes())
        .unwrap_or_default();
    let element = match name {
        Some(name) => create_for_token(rt, name, &attrs)?,
        None => None,
    };
    reactions::push_queue(rt);
    engine_mut(rt)?.finish_custom_element(placeholder, element);
    reactions::pop_and_invoke(rt)
}

/// The parser's pause for the reactions of its changes
/// (`swb_dom::ParseStep::Reactions`): they run before the next token.
pub(crate) fn parser_reactions(rt: &mut Runtime) -> VmResult<()> {
    let limit = reactions::start_parser_level(rt)?;
    let outermost = rt.stack_depths().0 == 0;
    let checkpoints = std::mem::replace(
        &mut state_mut(rt)?.event_loop.listener_checkpoints,
        outermost,
    );
    let result = reactions::flush_backup(rt);
    state_mut(rt)?.event_loop.listener_checkpoints = checkpoints;
    reactions::end_parser_level(rt, limit);
    result
}

/// HTML "create an element for a token", steps 7 to 12, for an element whose
/// name the window's registry defines ("will execute script"): the
/// throw-on-dynamic-markup-insertion counter of the document goes up, the
/// microtasks run if no script runs, and "create an element" with the
/// synchronous custom elements flag and the appending of the token's
/// attributes (through the attribute funnel: `attributeChangedCallback`,
/// mutation records) run in an element queue that is invoked at the end.
/// `None` if no element could be made (a limit of the arena).
/// <https://html.spec.whatwg.org/multipage/parsing.html#create-an-element-for-the-token>
fn create_for_token(
    rt: &mut Runtime,
    name: QualName,
    attrs: &[Attribute],
) -> VmResult<Option<NodeId>> {
    state_mut(rt)?.doc.custom.throw_on_dynamic_markup += 1;
    let result = create_for_token_steps(rt, name, attrs);
    if let Ok(state) = state_mut(rt) {
        let counter = &mut state.doc.custom.throw_on_dynamic_markup;
        *counter = counter.saturating_sub(1);
    }
    result
}

fn create_for_token_steps(
    rt: &mut Runtime,
    name: QualName,
    attrs: &[Attribute],
) -> VmResult<Option<NodeId>> {
    // Step 9: the checkpoint tests that no script runs (not inside
    // `document.write`).
    match crate::event_loop::microtask_checkpoint(rt) {
        Err(ScriptError::Terminated(termination)) => return Err(VmError::Terminated(termination)),
        Err(ScriptError::Internal(error)) => return Err(VmError::Internal(error)),
        _ => {}
    }
    reactions::push_queue(rt);
    // The registry defines the name (the document's set of defined names
    // follows it); a plain element is the safe answer if it does not.
    let created = if let Some(number) = super::look_up(rt, NodeId::DOCUMENT, &name.local) {
        construct(rt, NodeId::DOCUMENT, name, number, Operation::Parser)
    } else {
        let hyphen = name.local.contains('-');
        plain_element(rt, NodeId::DOCUMENT, name, hyphen)
    };
    let element = match created {
        Ok(element) => Some(element),
        Err(Exception::Js(error)) => {
            reactions::pop_and_drop(rt)?;
            return Err(error);
        }
        Err(other) => {
            log::debug!("the parser made no custom element: {other:?}");
            None
        }
    };
    if let Some(element) = element {
        for attr in attrs {
            match crate::dom::attrs::set_attribute(rt, element, &attr.name, &attr.value) {
                Ok(()) => {}
                Err(Exception::Js(error)) => {
                    reactions::pop_and_drop(rt)?;
                    return Err(error);
                }
                Err(other) => {
                    log::debug!("an attribute of a custom element was not set: {other:?}");
                    break;
                }
            }
        }
    }
    reactions::pop_and_invoke(rt)?;
    Ok(element)
}
