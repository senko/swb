//! DOM "create an element" (DOM §4.9,
//! <https://dom.spec.whatwg.org/#concept-create-element>; ADR 0035 section
//! 4) with the synchronous custom elements flag set: `createElement` and
//! `createElementNS` run the constructor of a defined name inside the call.
//!
//! A constructor that throws, or whose result fails a check, is reported
//! (not thrown), and the result is a new `HTMLUnknownElement` in the state
//! `failed` (measured, case `create-element-failures`). At the nesting limit
//! the constructor does not run and the result is that element, without a
//! report (measured, case `reactions-depth-limit`).

use swb_dom::{CustomElementState, ElementData, LocalName, NodeData, NodeId, QualName, ns};
use swb_js::{Runtime, ScriptError, ThrowKind, Value, VmError};

use super::reactions;
use crate::bindings::{self as b, Exception};
use crate::generated as g;
use crate::state::{dom, dom_mut};

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
        return construct(rt, document, name, number, operation);
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
    operation: &str,
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
            crate::errors::report_callback(rt, constructor, &site, &error)?;
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
fn report_failure(rt: &mut Runtime, operation: &str, exception: Exception) -> b::Result<()> {
    let prefix = format!("Failed to execute '{operation}' on 'Document': ");
    match exception {
        Exception::Dom { name, message } => {
            let realm = crate::state::doc_realm(rt)?;
            let message = format!("{prefix}{message}");
            let error = b::dom_exception::create(rt, realm, g::DOM_EXCEPTION, name, &message)?;
            crate::errors::report_host_error(rt, Value::Object(error), true)?;
        }
        Exception::Type(message) => {
            let error = rt.new_error(ThrowKind::TypeError, &format!("{prefix}{message}"))?;
            crate::errors::report_host_error(rt, Value::Object(error), false)?;
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
