//! Event handlers (HTML §8.1.8.1, "Event handlers",
//! <https://html.spec.whatwg.org/multipage/webappapis.html#event-handlers>):
//! the IDL attributes `onclick` and the others, the `on*` content
//! attributes, and the event handler processing algorithm.
//!
//! Each event target has a map from handler name to a value: `null`, a
//! function, or the source text of a content attribute that is not
//! compiled yet ("internal raw uncompiled handler"). A non-null value has
//! a listener in the target's list, added when the handler was first set
//! and removed when it becomes `null`; its callback is one marker object
//! of the runtime and its `handler` field names the handler, so that a
//! dispatch calls [`call`] for it. The handler keeps its place in the list
//! while its value changes.
//!
//! The compile happens when the value is first needed: the getter, or the
//! first dispatch ([`Runtime::compile_event_handler`](swb_js::Runtime), ADR
//! 0028 section 6, item 6). The scope chain is the document, the form
//! owner of a form-associated element, the element. A compile error is
//! reported like a script error (ADR 0028 section 8) and the value becomes
//! `null`.
//!
//! `body` and `frameset` forward the handlers of the window-reflecting set
//! (`onblur`, `onerror`, `onfocus`, `onload`, `onresize`, `onscroll` and
//! the `WindowEventHandlers`) to the window.
//!
//! Content attributes: the parser sets attributes without going through
//! the bindings, so an element's attributes are read the first time a
//! script touches its listeners (`sync`): by a dispatch through the
//! element, `addEventListener`, or the IDL attribute. Later changes of
//! `on*` attributes of such an element go through [`attribute_changed`].
//! Departure: an element that no script touched has no listener until
//! then, which no script can observe.

use std::borrow::Cow;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};

use swb_dom::{Document, NodeId, QualName, local_name, ns};
use swb_js::{
    EventHandler, Gc, Generic, Object, Runtime, ScriptError, Tracer, Value, VmError, VmResult,
};

use super::handler_names::NAMES;
use super::listeners::Listener;
use super::target::{MAX_DOCUMENT_LISTENERS, TargetKind, default_passive, remove, with_listeners};
use super::{Extra, data_mut};
use crate::bindings::{self as b, HandlerTarget, Host};
use crate::state::{WebState, dom, engine, state, state_mut};

/// Which interfaces have a handler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Class {
    /// `GlobalEventHandlers`.
    Global,
    /// `DocumentAndElementEventHandlers`.
    Element,
    /// `WindowEventHandlers`: also `body` and `frameset` elements.
    Window,
    /// Only `Document`.
    Document,
}

/// An event handler: the name of its IDL and content attribute, and the
/// type of the events it handles.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Name {
    pub(crate) attr: &'static str,
    pub(crate) event: &'static str,
    pub(crate) class: Class,
}

/// The handler with the IDL attribute `attr`.
pub(crate) fn lookup(attr: &str) -> Option<&'static Name> {
    if !attr.starts_with("on") {
        return None;
    }
    NAMES.iter().find(|n| n.attr == attr)
}

impl Name {
    /// True for the handlers that `body` and `frameset` forward to the
    /// window ("the Window-reflecting body element event handler set").
    fn reflects_to_window(&self) -> bool {
        self.class == Class::Window
            || matches!(
                self.attr,
                "onblur" | "onerror" | "onfocus" | "onload" | "onresize" | "onscroll"
            )
    }
}

/// The object that holds a handler map: a node or the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Key {
    Node(NodeId),
    Window,
}

impl Key {
    /// The key of a listener target; none for a constructed target.
    pub(crate) fn of(target: TargetKind) -> Option<Key> {
        match target {
            TargetKind::Node(node) => Some(Key::Node(node)),
            TargetKind::Window => Some(Key::Window),
            TargetKind::Constructed(_) => None,
        }
    }

    fn target(self) -> TargetKind {
        match self {
            Key::Node(node) => TargetKind::Node(node),
            Key::Window => TargetKind::Window,
        }
    }
}

/// The value of a handler.
#[derive(Debug)]
enum Value_ {
    Null,
    Function(Gc<Object>),
    /// The text of a content attribute, not compiled yet.
    Raw(String),
}

#[derive(Debug)]
struct Entry {
    value: Value_,
    /// True while the target's listener list holds the handler's listener.
    registered: bool,
}

/// The handler maps of the document's targets.
#[derive(Debug, Default)]
pub(crate) struct Handlers {
    entries: HashMap<(Key, &'static str), Entry>,
    /// The nodes whose content attributes were read.
    synced: HashSet<NodeId>,
    /// The callback of every handler listener.
    marker: Option<Gc<Object>>,
    /// The API call that Chromium names in the message of a compile error
    /// found during it (`Failed to execute 'dispatchEvent' on
    /// 'EventTarget'`).
    context: Option<Cow<'static, str>>,
}

impl Handlers {
    /// Reports the functions and the marker to the collector.
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for entry in self.entries.values() {
            if let Value_::Function(function) = entry.value {
                tracer.object(function);
            }
        }
        if let Some(marker) = self.marker {
            tracer.object(marker);
        }
    }
}

/// Runs `f` with `context` as the API call that a compile error names.
pub(crate) fn with_context<R>(
    rt: &mut Runtime,
    context: &'static str,
    f: impl FnOnce(&mut Runtime) -> VmResult<R>,
) -> VmResult<R> {
    let old = state_mut(rt)?
        .handlers
        .context
        .replace(Cow::Borrowed(context));
    let result = f(rt);
    if let Ok(state) = state_mut(rt) {
        state.handlers.context = old;
    }
    result
}

/// The key that holds the handler `name` of `target`: the window for the
/// window-reflecting handlers of `body` and `frameset`.
fn resolve(dom: &Document, target: HandlerTarget, name: &Name) -> Key {
    match target {
        HandlerTarget::Window => Key::Window,
        HandlerTarget::Node(node) => {
            if name.reflects_to_window() && is_body_or_frameset(dom, node) {
                Key::Window
            } else {
                Key::Node(node)
            }
        }
    }
}

fn is_body_or_frameset(dom: &Document, node: NodeId) -> bool {
    dom.is_html_element(node, &local_name!("body"))
        || dom.is_html_element(node, &local_name!("frameset"))
}

/// True if the content attribute `name` of `element` is an event handler
/// (HTML and SVG elements; the window handlers only on `body` and
/// `frameset`).
fn applies(dom: &Document, element: NodeId, name: &Name) -> bool {
    let Some(e) = dom.element(element) else {
        return false;
    };
    if e.name.ns != ns!(html) && e.name.ns != ns!(svg) {
        return false;
    }
    match name.class {
        Class::Global | Class::Element => true,
        Class::Window => is_body_or_frameset(dom, element),
        Class::Document => false,
    }
}

// ----- Reading content attributes -----

/// Reads the `on*` content attributes of `key` the first time (see the
/// module documentation).
pub(crate) fn sync(rt: &mut Runtime, target: TargetKind) -> VmResult<()> {
    match target {
        TargetKind::Node(node) => sync_node(rt, node),
        TargetKind::Window => sync_window(rt),
        TargetKind::Constructed(_) => Ok(()),
    }
}

fn sync_node(rt: &mut Runtime, node: NodeId) -> VmResult<()> {
    if !state_mut(rt)?.handlers.synced.insert(node) {
        return Ok(());
    }
    let document = dom(rt)?;
    let Some(element) = document.element(node) else {
        return Ok(());
    };
    let found: Vec<(&'static Name, String)> = element
        .attributes()
        .iter()
        .filter(|a| a.name.ns == ns!())
        .filter_map(|a| Some((lookup(&a.name.local)?, a.value.clone())))
        .filter(|(name, _)| applies(document, node, name))
        .collect();
    for (name, value) in found {
        let key = resolve(dom(rt)?, HandlerTarget::Node(node), name);
        set_raw(rt, key, name, value)?;
    }
    Ok(())
}

/// The window: also reads the attributes of the `body` or `frameset` of
/// the document, which forward their handlers to it.
fn sync_window(rt: &mut Runtime) -> VmResult<()> {
    // Not once: the body may come after the first listener of the window
    // (a script in the head), and a script may replace it.
    let dom = dom(rt)?;
    let body = dom
        .element_children(NodeId::DOCUMENT)
        .find(|&e| dom.is_html_element(e, &local_name!("html")))
        .and_then(|html| {
            dom.element_children(html)
                .find(|&c| is_body_or_frameset(dom, c))
        });
    match body {
        Some(body) => sync_node(rt, body),
        None => Ok(()),
    }
}

/// The attribute change steps of event handler content attributes (HTML
/// "event handler content attributes"): `value` is the new value, or none
/// if the attribute was removed. Elements that were not read yet are left
/// to [`sync`].
pub(crate) fn attribute_changed(
    rt: &mut Runtime,
    element: NodeId,
    attribute: &QualName,
    value: Option<&str>,
) -> VmResult<()> {
    if attribute.ns != ns!() || !attribute.local.starts_with("on") {
        return Ok(());
    }
    if !state(rt)?.handlers.synced.contains(&element) {
        return Ok(());
    }
    let dom = dom(rt)?;
    let Some(name) = lookup(&attribute.local).filter(|n| applies(dom, element, n)) else {
        return Ok(());
    };
    let key = resolve(dom, HandlerTarget::Node(element), name);
    match value {
        Some(value) => set_raw(rt, key, name, value.to_owned()),
        None => deactivate(rt, key, name),
    }
}

// ----- The map -----

/// The callback of handler listeners.
fn marker(rt: &mut Runtime) -> VmResult<Gc<Object>> {
    if let Some(marker) = state(rt)?.handlers.marker {
        return Ok(marker);
    }
    let marker = rt.new_plain_object(0)?;
    state_mut(rt)?.handlers.marker = Some(marker);
    Ok(marker)
}

/// Sets the value of a handler and makes sure that its listener is in the
/// list (HTML "activate an event handler").
fn set_value(rt: &mut Runtime, key: Key, name: &'static Name, value: Value_) -> VmResult<()> {
    let handlers = &mut state_mut(rt)?.handlers;
    let entry = handlers.entries.entry((key, name.attr)).or_insert(Entry {
        value: Value_::Null,
        registered: false,
    });
    entry.value = value;
    if entry.registered {
        return Ok(());
    }
    if state(rt)?.listener_count >= MAX_DOCUMENT_LISTENERS {
        log::warn!("a document has more than {MAX_DOCUMENT_LISTENERS} event listeners");
        return Err(rt.terminate("too many event listeners in the document"));
    }
    let callback = marker(rt)?;
    let target = key.target();
    let passive = default_passive(rt, target, name.event)?;
    let listener = Listener {
        callback,
        capture: false,
        passive,
        once: false,
        removed: Cell::new(false),
        handler: Some(name),
    };
    with_listeners(rt, target, |list| list.add(name.event, listener))?;
    state_mut(rt)?.listener_count += 1;
    if let Some(entry) = state_mut(rt)?.handlers.entries.get_mut(&(key, name.attr)) {
        entry.registered = true;
    }
    Ok(())
}

/// A content attribute sets the raw handler.
fn set_raw(rt: &mut Runtime, key: Key, name: &'static Name, text: String) -> VmResult<()> {
    set_value(rt, key, name, Value_::Raw(text))
}

/// HTML "deactivate an event handler": the value becomes `null` and the
/// listener leaves the list.
fn deactivate(rt: &mut Runtime, key: Key, name: &'static Name) -> VmResult<()> {
    let Some(entry) = state_mut(rt)?.handlers.entries.get_mut(&(key, name.attr)) else {
        return Ok(());
    };
    entry.value = Value_::Null;
    if !std::mem::replace(&mut entry.registered, false) {
        return Ok(());
    }
    let callback = marker(rt)?;
    remove(rt, key.target(), |list| {
        list.remove(name.event, callback, false)
    })?;
    Ok(())
}

// ----- The IDL attributes -----

/// The getter of an event handler IDL attribute.
pub(crate) fn get(rt: &mut Runtime, target: HandlerTarget, attr: &str) -> VmResult<Value> {
    let Some(name) = lookup(attr) else {
        return Ok(Value::Null);
    };
    let key = resolve(dom(rt)?, target, name);
    sync(rt, key.target())?;
    let context = getter_context(dom(rt)?, target, name);
    let saved = state_mut(rt)?.handlers.context.replace(context);
    let value = current_value(rt, key, name);
    state_mut(rt)?.handlers.context = saved;
    Ok(value?.map_or(Value::Null, Value::Object))
}

/// The setter of an event handler IDL attribute: an object is the handler
/// (the getter returns it, also if it is not callable; a dispatch ignores
/// it), any other value makes it `null` (`[LegacyTreatNonObjectAsNull]`,
/// measured in Chromium 148).
pub(crate) fn set(
    rt: &mut Runtime,
    target: HandlerTarget,
    attr: &str,
    value: Value,
) -> VmResult<()> {
    let Some(name) = lookup(attr) else {
        return Ok(());
    };
    let key = resolve(dom(rt)?, target, name);
    sync(rt, key.target())?;
    match value {
        Value::Object(object) => set_value(rt, key, name, Value_::Function(object)),
        _ => deactivate(rt, key, name),
    }
}

/// HTML "getting the current value of the event handler": the function,
/// compiled now if the value is still the text of a content attribute.
fn current_value(rt: &mut Runtime, key: Key, name: &'static Name) -> VmResult<Option<Gc<Object>>> {
    let text = match state(rt)?.handlers.entries.get(&(key, name.attr)) {
        None => return Ok(None),
        Some(entry) => match &entry.value {
            Value_::Null => return Ok(None),
            Value_::Function(function) => return Ok(Some(*function)),
            Value_::Raw(text) => text.clone(),
        },
    };
    compile(rt, key, name, &text)
}

/// The text that Chromium puts in front of the message of a compile error
/// that the getter finds.
fn getter_context(dom: &Document, target: HandlerTarget, name: &Name) -> Cow<'static, str> {
    let interface = match target {
        HandlerTarget::Window => "Window",
        HandlerTarget::Node(NodeId::DOCUMENT) => "Document",
        HandlerTarget::Node(node) => {
            if dom.element(node).is_some_and(|e| e.name.ns == ns!(svg)) {
                "SVGElement"
            } else if name.class == Class::Window && is_body_or_frameset(dom, node) {
                "HTMLBodyElement"
            } else {
                "HTMLElement"
            }
        }
    };
    Cow::Owned(format!(
        "Failed to read the '{}' property from '{interface}'",
        name.attr
    ))
}

/// The parameter names of the compiled function (HTML "getting the current
/// value of the event handler", step 9).
fn parameters(dom: &Document, key: Key, name: &Name) -> &'static [&'static str] {
    if name.attr == "onerror" && key == Key::Window {
        return &["event", "source", "lineno", "colno", "error"];
    }
    match key {
        Key::Node(node) if dom.element(node).is_some_and(|e| e.name.ns == ns!(svg)) => &["evt"],
        _ => &["event"],
    }
}

/// Compiles the text of a content attribute and stores the function; a
/// compile error is reported and the value becomes `null`.
fn compile(
    rt: &mut Runtime,
    key: Key,
    name: &'static Name,
    text: &str,
) -> VmResult<Option<Gc<Object>>> {
    let (document, element, form, params) = {
        let dom = dom(rt)?;
        let element = match key {
            Key::Node(node) if dom.element(node).is_some() => Some(node),
            _ => None,
        };
        let document = element.map_or(NodeId::DOCUMENT, |e| dom.node_document(e));
        let form = element.and_then(|e| crate::html::form_owner(dom, e));
        (document, element, form, parameters(dom, key, name))
    };
    // A handler that forwards to the window (`body`, `frameset`) has no
    // element and no document in its scope (HTML: the scope is for an
    // element target only).
    let mut scope = Vec::new();
    if key != Key::Window {
        scope.push(WebState::wrap_node(rt, 0, document)?);
    }
    for node in form.into_iter().chain(element) {
        scope.push(WebState::wrap_node(rt, 0, node)?);
    }
    let url = engine(rt)?.url();
    let handler = EventHandler {
        realm: 0,
        name: name.attr,
        params,
        body: text,
        scope: &scope,
        script_name: &url,
    };
    match rt.compile_event_handler(&handler) {
        Ok(function) => {
            // Nothing between the compile and the store collects.
            if let Some(entry) = state_mut(rt)?.handlers.entries.get_mut(&(key, name.attr))
                && matches!(entry.value, Value_::Raw(_))
            {
                entry.value = Value_::Function(function);
            }
            Ok(Some(function))
        }
        Err(ScriptError::Terminated(termination)) => Err(VmError::Terminated(termination)),
        Err(ScriptError::Internal(error)) => Err(VmError::Internal(error)),
        Err(error) => {
            if let Some(entry) = state_mut(rt)?.handlers.entries.get_mut(&(key, name.attr)) {
                entry.value = Value_::Null;
            }
            let context = state(rt)?.handlers.context.clone();
            let error = with_context_message(error, context.as_deref());
            let site = crate::errors::Site::script(&url, text);
            crate::errors::report(rt, &site, &error)?;
            Ok(None)
        }
    }
}

/// Chromium words the message of a compile error with the API call that
/// found it.
fn with_context_message(error: ScriptError, context: Option<&str>) -> ScriptError {
    match (error, context) {
        (
            ScriptError::Compile {
                kind,
                message,
                offset,
            },
            Some(context),
        ) => ScriptError::Compile {
            kind,
            message: format!("{context}: {message}"),
            offset,
        },
        (error, _) => error,
    }
}

// ----- Dispatch -----

/// HTML "the event handler processing algorithm" for the handler `name` of
/// `target`, called by the dispatch for its listener; `this` is the event's
/// current target.
pub(crate) fn call(
    rt: &mut Runtime,
    target: TargetKind,
    name: &'static Name,
    event: Gc<Object>,
    handle: Gc<Generic>,
    this: Gc<Object>,
) -> VmResult<()> {
    let Some(key) = Key::of(target) else {
        return Ok(());
    };
    let Some(function) = current_value(rt, key, name)? else {
        return Ok(());
    };
    if !rt.callable(function.into())? {
        return Ok(());
    }
    let special_error = key == Key::Window && is_error_event(rt, handle)?;
    let args = if special_error {
        error_arguments(rt, handle)?
    } else {
        vec![Value::Object(event)]
    };
    match rt.call_function(function.into(), this.into(), &args) {
        Ok(result) => process_result(rt, handle, name, special_error, result),
        Err(ScriptError::Terminated(termination)) => Err(VmError::Terminated(termination)),
        Err(ScriptError::Internal(error)) => Err(VmError::Internal(error)),
        Err(error) => {
            let site_name = format!("a listener of '{}'", name.event);
            crate::errors::report(rt, &crate::errors::Site::named(&site_name), &error)
        }
    }
}

/// True if the event is an `ErrorEvent` of type `error`.
fn is_error_event(rt: &Runtime, handle: Gc<Generic>) -> VmResult<bool> {
    let event = super::data(rt, handle)?;
    Ok(event.kind == "error" && matches!(event.extra, Extra::Error { .. }))
}

/// The five arguments of `window.onerror`: the message, the file name, the
/// line, the column and the error.
fn error_arguments(rt: &mut Runtime, handle: Gc<Generic>) -> VmResult<Vec<Value>> {
    let Extra::Error {
        message,
        filename,
        lineno,
        colno,
        error,
    } = &super::data(rt, handle)?.extra
    else {
        return Ok(Vec::new());
    };
    let (message, filename, lineno, colno, error) =
        (message.clone(), filename.clone(), *lineno, *colno, *error);
    let message = b::to_js::string(rt, &message)?;
    let filename = b::to_js::string(rt, &filename)?;
    let args = vec![
        message,
        filename,
        Value::Double(f64::from(lineno)),
        Value::Double(f64::from(colno)),
        error,
    ];
    for value in &args {
        b::keep(rt, *value);
    }
    Ok(args)
}

/// Steps 6 to 9 of the processing algorithm: the return value cancels the
/// event.
fn process_result(
    rt: &mut Runtime,
    handle: Gc<Generic>,
    name: &Name,
    special_error: bool,
    result: Value,
) -> VmResult<()> {
    let cancel = if special_error {
        matches!(result, Value::Bool(true))
    } else if name.attr == "onbeforeunload" {
        !matches!(result, Value::Null | Value::Undefined)
    } else {
        matches!(result, Value::Bool(false))
    };
    if cancel {
        data_mut(rt, handle)?.set_canceled();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The names in `idl` that are attributes of one of the three handler
    /// types.
    fn idl_names(idl: &str) -> Vec<&str> {
        idl.lines()
            .filter_map(|line| {
                let line = line.trim();
                let rest = line
                    .strip_prefix("attribute EventHandler ")
                    .or_else(|| line.strip_prefix("attribute OnErrorEventHandler "))
                    .or_else(|| line.strip_prefix("attribute OnBeforeUnloadEventHandler "))?;
                rest.split(';').next()
            })
            .collect()
    }

    #[test]
    fn the_table_is_the_idl() {
        let mut idl: Vec<&str> = idl_names(include_str!("../../idl/html.idl"));
        idl.extend(idl_names(include_str!("../../idl/pointer-events.idl")));
        let mut table: Vec<&str> = NAMES.iter().map(|n| n.attr).collect();
        idl.sort_unstable();
        table.sort_unstable();
        assert_eq!(table, idl);
        let mut unique = table.clone();
        unique.dedup();
        assert_eq!(unique, table, "a name twice in the table");
    }

    #[test]
    fn lookup_finds_names() {
        assert_eq!(lookup("onclick").map(|n| n.event), Some("click"));
        assert_eq!(
            lookup("onwebkitanimationend").map(|n| n.event),
            Some("webkitAnimationEnd")
        );
        assert!(lookup("on").is_none());
        assert!(lookup("onclickx").is_none());
        assert!(lookup("click").is_none());
        assert!(lookup("ontouchstart").is_none());
    }
}
