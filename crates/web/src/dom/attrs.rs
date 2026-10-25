//! Attributes for scripts (DOM §4.9 "Interface `Element`" and §4.9.1
//! "Interface `NamedNodeMap`", §4.9.2 "Interface `Attr`",
//! <https://dom.spec.whatwg.org/#interface-element>): the algorithms
//! behind `getAttribute`, `setAttribute` and the others, `Attr` objects and
//! `NamedNodeMap`.
//!
//! The DOM keeps the attributes of an element in the element (`swb_dom`);
//! an `Attr` object is a view of one of them. The binding state keeps the
//! `Attr` objects that scripts have seen, by element and name, so that
//! `el.getAttributeNode('a') === el.attributes[0]`. An attribute that is
//! removed leaves its object behind, detached, with the last value.
//! Departure: `Attr` is not a `Node` (it extends `EventTarget`; ADR 0028
//! section 3 keeps attributes out of the arena).
//!
//! All documents of swb are HTML documents, so a name that a script gives
//! to an element of the HTML namespace is lowercased (DOM "get an
//! attribute by name"). Every attribute that a script adds goes through
//! [`set_attribute`], which runs the attribute change steps of script
//! elements; every removal through [`remove_attribute`], which detaches the
//! `Attr` object.

use std::borrow::Cow;

use swb_dom::{LocalName, Namespace, NodeId, QualName, ns};
use swb_js::{Gc, Generic, GenericData, HostData, Object, Runtime, Tracer, VmError, VmResult};

use super::dom_result;
use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::generated as g;
use crate::state::{WebState, dom, dom_mut, state, state_mut};

const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
const XMLNS_NS: &str = "http://www.w3.org/2000/xmlns/";

// ----- Names -----

/// True for a name that Chromium accepts for an attribute (measured in
/// `tools/probes/web-dom-mutation.json`): not empty, without ASCII
/// whitespace, NUL, `/`, `>` or `=`. (The DOM standard asks for an XML
/// Name; Chromium accepts `1a` and `[a]`.)
pub(super) fn is_valid_attribute_name(name: &str) -> bool {
    !name.is_empty()
        && !name.chars().any(|c| {
            matches!(
                c,
                ' ' | '\t' | '\n' | '\x0c' | '\r' | '\0' | '/' | '>' | '='
            )
        })
}

/// The exception for an invalid attribute name.
pub(super) fn invalid_attribute_name(name: &str) -> Exception {
    Exception::dom(
        "InvalidCharacterError",
        format!("'{name}' is not a valid attribute name."),
    )
}

/// The qualified name for an element in the HTML namespace is lowercased.
fn lowered<'a>(dom: &swb_dom::Document, element: NodeId, name: &'a str) -> Cow<'a, str> {
    let html = dom.element(element).is_some_and(|e| e.name.ns == ns!(html));
    if html && name.bytes().any(|c| c.is_ascii_uppercase()) {
        Cow::Owned(name.to_ascii_lowercase())
    } else {
        Cow::Borrowed(name)
    }
}

/// True if `c` may start a name that `createElementNS`, `setAttributeNS`
/// and `createAttributeNS` accept (Chromium: as an element's local name).
fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || matches!(c, ':' | '_') || c >= '\u{80}'
}

/// True if `c` may start a name without colons.
fn is_ncname_start(c: char) -> bool {
    c != ':' && is_name_start(c)
}

/// The parts of a validated qualified name.
pub(super) struct Extracted {
    pub(super) namespace: Option<String>,
    pub(super) prefix: Option<String>,
    pub(super) local: String,
}

/// DOM "validate and extract": checks the qualified name and the
/// namespace, with Chromium's messages. `attribute` selects the message of
/// a namespace error (`setAttributeNS`: "'NS' is an invalid namespace for
/// attributes."; the others name the namespace and the qualified name).
/// <https://dom.spec.whatwg.org/#validate-and-extract>
pub(super) fn validate_and_extract(
    namespace: Option<String>,
    qualified: &str,
    attribute: bool,
) -> Result<Extracted, Exception> {
    let namespace = namespace.filter(|n| !n.is_empty());
    let invalid = |message: String| Exception::dom("InvalidCharacterError", message);
    if qualified.is_empty() {
        return Err(invalid("The qualified name provided is empty.".to_owned()));
    }
    let bad = |c: char, first: bool| {
        matches!(c, ' ' | '\t' | '\n' | '\x0c' | '\r' | '\0' | '/' | '>')
            || (attribute && c == '=')
            || (first && !is_name_start(c) && !attribute)
    };
    for (i, c) in qualified.chars().enumerate() {
        if bad(c, i == 0) {
            return Err(invalid(format!(
                "The qualified name provided ('{qualified}') contains the invalid character '{c}'."
            )));
        }
    }
    let (prefix, local) = match qualified.split_once(':') {
        Some((prefix, local)) => (Some(prefix), local),
        None => (None, qualified),
    };
    if prefix == Some("") {
        return Err(invalid(format!(
            "The qualified name provided ('{qualified}') has an empty namespace prefix."
        )));
    }
    if local.is_empty() {
        return Err(invalid(format!(
            "The qualified name provided ('{qualified}') has an empty local name."
        )));
    }
    // Elements and `createAttributeNS`: each part starts like an NCName
    // (a local name that starts with another colon is refused).
    if !attribute {
        for part in [prefix, Some(local)].into_iter().flatten() {
            if let Some(c) = part.chars().next().filter(|&c| !is_ncname_start(c)) {
                return Err(invalid(format!(
                    "The qualified name provided ('{qualified}') contains the invalid character '{c}'."
                )));
            }
        }
    }
    let ns = namespace.as_deref();
    let wrong = (prefix.is_some() && ns.is_none())
        || (prefix == Some("xml") && ns != Some(XML_NS))
        || ((prefix == Some("xmlns") || qualified == "xmlns") && ns != Some(XMLNS_NS))
        || (ns == Some(XMLNS_NS) && prefix != Some("xmlns") && qualified != "xmlns");
    if wrong {
        let message = if attribute {
            format!(
                "'{}' is an invalid namespace for attributes.",
                ns.unwrap_or("")
            )
        } else {
            format!(
                "The namespace URI provided ('{}') is not valid for the qualified name provided ('{qualified}').",
                ns.unwrap_or("")
            )
        };
        return Err(Exception::dom("NamespaceError", message));
    }
    Ok(Extracted {
        prefix: prefix.map(str::to_owned),
        local: local.to_owned(),
        namespace,
    })
}

/// The qualified name of an attribute: `prefix:local` or `local`.
pub(super) fn qualified_name(name: &QualName) -> String {
    match &name.prefix {
        Some(prefix) => format!("{prefix}:{}", name.local),
        None => name.local.to_string(),
    }
}

/// The name (as the DOM stores it) of the attribute of `element` whose
/// qualified name is `qualified` (lowercased for an HTML element).
pub(super) fn find_by_qualified_name(
    rt: &Runtime,
    element: NodeId,
    qualified: &str,
) -> VmResult<Option<QualName>> {
    let dom = dom(rt)?;
    let wanted = lowered(dom, element, qualified);
    Ok(dom
        .element(element)
        .and_then(|e| e.attr_by_qualified_name(&wanted))
        .map(|a| a.name.clone()))
}

/// The name of the attribute of `element` with this namespace (`None` or
/// empty for none) and local name.
pub(super) fn find_by_namespace(
    rt: &Runtime,
    element: NodeId,
    namespace: Option<&str>,
    local: &str,
) -> VmResult<Option<QualName>> {
    Ok(dom(rt)?
        .element(element)
        .and_then(|e| e.attr_by_namespace(namespace.unwrap_or(""), local))
        .map(|a| a.name.clone()))
}

/// The value of the attribute `name` of `element`.
fn value_of(rt: &Runtime, element: NodeId, name: &QualName) -> VmResult<Option<String>> {
    Ok(dom(rt)?
        .element(element)
        .and_then(|e| e.attr_by_namespace(&name.ns, &name.local))
        .map(|a| a.value.clone()))
}

// ----- Changes -----

/// Sets the attribute `name` of `element` and runs the attribute change
/// steps of script elements (`async` added: not "force async"; `src`
/// added: the post-connection steps).
pub(crate) fn set_attribute(
    rt: &mut Runtime,
    element: NodeId,
    name: &QualName,
    value: &str,
) -> b::Result<()> {
    let had = dom(rt)?
        .element(element)
        .is_some_and(|e| e.attr_by_namespace(&name.ns, &name.local).is_some());
    let type_change = crate::html::before_type_change(rt, element, name)?;
    let result = dom_mut(rt)?.set_attribute(element, name, value);
    dom_result(rt, result)?;
    if !had {
        selected_attribute_changed(rt, element, name)?;
    }
    crate::html::after_type_change(rt, element, type_change)?;
    crate::events::handler_attribute_changed(rt, element, name, Some(value))?;
    if name.ns == ns!() && &*name.local == "hidden" {
        crate::html::focus::hidden_set(rt, element)?;
    }
    Ok(crate::scripts::attribute_changed(rt, element, name, had)?)
}

/// Tells the engine that the `selected` attribute of an `option` was
/// added or removed: an option whose selectedness is not dirty follows
/// the attribute.
fn selected_attribute_changed(rt: &mut Runtime, element: NodeId, name: &QualName) -> VmResult<()> {
    if name.ns == ns!() && &*name.local == "selected" {
        let is_option = dom(rt)?
            .element(element)
            .is_some_and(|e| e.is_html() && &**e.local_name() == "option");
        if is_option {
            crate::state::engine_mut(rt)?.option_selected_attribute_changed(element);
        }
    }
    Ok(())
}

/// Removes the attribute `name` of `element`; its `Attr` object, if a
/// script has seen it, becomes detached with the old value. Returns the
/// old value.
pub(crate) fn remove_attribute(
    rt: &mut Runtime,
    element: NodeId,
    name: &QualName,
) -> VmResult<Option<String>> {
    let old = dom_mut(rt)?.remove_attribute(element, name);
    if old.is_some() {
        selected_attribute_changed(rt, element, name)?;
        crate::events::handler_attribute_changed(rt, element, name, None)?;
    }
    if let Some(old) = &old {
        let key = (element, name.ns.clone(), name.local.clone());
        if let Some(object) = state_mut(rt)?.attr_objects.remove(&key) {
            let data = attr_data_mut(rt, object)?;
            data.owner = None;
            data.value.clone_from(old);
        }
    }
    Ok(old)
}

// ----- Attr objects -----

/// The data of an `Attr` object.
pub(super) struct AttrData {
    /// The element that has the attribute, or none.
    pub(super) owner: Option<NodeId>,
    /// The name, with its prefix.
    pub(super) name: QualName,
    /// The value while there is no owner.
    pub(super) value: String,
}

impl GenericData for AttrData {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        self.value.capacity() + 64
    }
}

/// The generic data handle of an `Attr` object.
fn attr_handle(rt: &Runtime, object: Gc<Object>) -> VmResult<Gc<Generic>> {
    match rt.host_data_of(object.into(), g::ATTR)? {
        HostData::Generic(data) => Ok(data),
        HostData::Word(_) => Err(VmError::type_error("Illegal invocation")),
    }
}

fn attr_data_mut(rt: &mut Runtime, object: Gc<Object>) -> VmResult<&mut AttrData> {
    let handle = attr_handle(rt, object)?;
    Ok(rt.heap_mut().generic_mut::<AttrData>(handle)?)
}

/// The data of the `Attr` that a call received as an argument or `this`.
fn data_of(rt: &Runtime, this: Platform) -> b::Result<&AttrData> {
    match this.data {
        HostData::Generic(handle) => Ok(rt
            .heap()
            .generic::<AttrData>(handle)
            .map_err(VmError::from)?),
        HostData::Word(_) => Err(VmError::type_error("Illegal invocation").into()),
    }
}

fn data_of_mut(rt: &mut Runtime, this: Platform) -> b::Result<&mut AttrData> {
    match this.data {
        HostData::Generic(handle) => Ok(rt
            .heap_mut()
            .generic_mut::<AttrData>(handle)
            .map_err(VmError::from)?),
        HostData::Word(_) => Err(VmError::type_error("Illegal invocation").into()),
    }
}

/// A new `Attr` object.
pub(super) fn new_attr(
    rt: &mut Runtime,
    realm: u32,
    owner: Option<NodeId>,
    name: QualName,
    value: String,
) -> VmResult<Gc<Object>> {
    let data = rt.alloc_host_data(AttrData { owner, name, value })?;
    rt.new_host_object(realm, g::ATTR, data)
}

/// The `Attr` object of the attribute `name` of `element`: the same object
/// at each call.
pub(super) fn attr_object(
    rt: &mut Runtime,
    realm: u32,
    element: NodeId,
    name: &QualName,
) -> VmResult<Gc<Object>> {
    let key = (element, name.ns.clone(), name.local.clone());
    if let Some(object) = state(rt)?.attr_objects.get(&key) {
        return Ok(*object);
    }
    let object = new_attr(rt, realm, Some(element), name.clone(), String::new())?;
    state_mut(rt)?.attr_objects.insert(key, object);
    Ok(object)
}

/// The `Attr` of the attribute `name` of `element`, or none if it does not
/// exist.
fn existing_attr(
    cx: &mut Cx<'_>,
    element: NodeId,
    name: Option<QualName>,
) -> b::Result<Option<Gc<Object>>> {
    let Some(name) = name else {
        return Ok(None);
    };
    let realm = cx.realm();
    Ok(Some(attr_object(cx.rt, realm, element, &name)?))
}

/// The `Attr` object that an argument names (it is not a node, so the
/// glue passes it as a platform object).
fn attr_argument(
    rt: &Runtime,
    attr: Platform,
) -> b::Result<(Gc<Object>, QualName, Option<NodeId>)> {
    let data = data_of(rt, attr)?;
    Ok((attr.object, data.name.clone(), data.owner))
}

/// DOM "set an attribute": puts the detached `attr` on `element`. Returns
/// the `Attr` it replaced.
/// <https://dom.spec.whatwg.org/#concept-element-attributes-set>
pub(super) fn set_attribute_node(
    cx: &mut Cx<'_>,
    element: NodeId,
    attr: Platform,
) -> b::Result<Option<Gc<Object>>> {
    let (object, name, owner) = attr_argument(cx.rt, attr)?;
    if owner == Some(element) {
        return Ok(Some(object));
    }
    if owner.is_some() {
        return Err(Exception::dom(
            "InUseAttributeError",
            "The node provided is an attribute node that is already an attribute of another Element; attribute nodes must be explicitly cloned.",
        ));
    }
    let value = data_of(cx.rt, attr)?.value.clone();
    let existing = find_by_namespace(cx.rt, element, Some(&name.ns), &name.local)?;
    let old_value = match &existing {
        Some(n) => value_of(cx.rt, element, n)?,
        None => None,
    };
    let old = match &existing {
        Some(n) => {
            let realm = cx.realm();
            let old = attr_object(cx.rt, realm, element, n)?;
            // The old attribute leaves the element with its value.
            let key = (element, n.ns.clone(), n.local.clone());
            state_mut(cx.rt)?.attr_objects.remove(&key);
            let data = attr_data_mut(cx.rt, old)?;
            data.owner = None;
            data.value = old_value.unwrap_or_default();
            Some(old)
        }
        None => None,
    };
    let stored = existing.unwrap_or_else(|| name.clone());
    set_attribute(cx.rt, element, &stored, &value)?;
    let key = (element, stored.ns.clone(), stored.local.clone());
    state_mut(cx.rt)?.attr_objects.insert(key, object);
    let data = attr_data_mut(cx.rt, object)?;
    data.owner = Some(element);
    data.name = stored;
    data.value = String::new();
    Ok(old)
}

/// DOM `removeAttributeNode`: removes `attr` from `element`.
pub(super) fn remove_attribute_node(
    cx: &mut Cx<'_>,
    element: NodeId,
    attr: Platform,
) -> b::Result<Gc<Object>> {
    let (object, name, owner) = attr_argument(cx.rt, attr)?;
    if owner != Some(element) {
        return Err(not_found("The node provided is owned by another element."));
    }
    remove_attribute(cx.rt, element, &name)?;
    Ok(object)
}

fn not_found(message: &str) -> Exception {
    Exception::dom("NotFoundError", message.to_owned())
}

// ----- Element attribute methods -----

/// `Element.getAttributeNames`.
pub(super) fn attribute_names(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Vec<String>> {
    Ok(dom(cx.rt)?
        .element(this)
        .map(|e| {
            e.attributes()
                .iter()
                .map(|a| qualified_name(&a.name))
                .collect()
        })
        .unwrap_or_default())
}

/// `Element.getAttribute`.
pub(super) fn get_attribute(
    cx: &mut Cx<'_>,
    this: NodeId,
    qualified: &str,
) -> b::Result<Option<String>> {
    let Some(name) = find_by_qualified_name(cx.rt, this, qualified)? else {
        return Ok(None);
    };
    Ok(value_of(cx.rt, this, &name)?)
}

/// `Element.getAttributeNS`.
pub(super) fn get_attribute_ns(
    cx: &mut Cx<'_>,
    this: NodeId,
    namespace: Option<&str>,
    local: &str,
) -> b::Result<Option<String>> {
    let Some(name) = find_by_namespace(cx.rt, this, namespace, local)? else {
        return Ok(None);
    };
    Ok(value_of(cx.rt, this, &name)?)
}

/// `Element.setAttribute`.
pub(super) fn set_attribute_by_name(
    cx: &mut Cx<'_>,
    this: NodeId,
    qualified: &str,
    value: &str,
) -> b::Result<()> {
    if !is_valid_attribute_name(qualified) {
        return Err(invalid_attribute_name(qualified));
    }
    let name = if let Some(name) = find_by_qualified_name(cx.rt, this, qualified)? {
        name
    } else {
        let lowered = lowered(dom(cx.rt)?, this, qualified).into_owned();
        QualName::new(None, ns!(), LocalName::from(lowered))
    };
    set_attribute(cx.rt, this, &name, value)
}

/// `Element.setAttributeNS`.
pub(super) fn set_attribute_ns(
    cx: &mut Cx<'_>,
    this: NodeId,
    namespace: Option<String>,
    qualified: &str,
    value: &str,
) -> b::Result<()> {
    let parts = validate_and_extract(namespace, qualified, true)?;
    let existing = find_by_namespace(cx.rt, this, parts.namespace.as_deref(), &parts.local)?;
    let name = existing.unwrap_or_else(|| {
        QualName::new(
            parts.prefix.map(Into::into),
            Namespace::from(parts.namespace.unwrap_or_default()),
            LocalName::from(parts.local),
        )
    });
    set_attribute(cx.rt, this, &name, value)
}

/// `Element.removeAttribute`.
pub(super) fn remove_attribute_by_name(
    cx: &mut Cx<'_>,
    this: NodeId,
    qualified: &str,
) -> b::Result<()> {
    if let Some(name) = find_by_qualified_name(cx.rt, this, qualified)? {
        remove_attribute(cx.rt, this, &name)?;
    }
    Ok(())
}

/// `Element.removeAttributeNS`.
pub(super) fn remove_attribute_ns(
    cx: &mut Cx<'_>,
    this: NodeId,
    namespace: Option<&str>,
    local: &str,
) -> b::Result<()> {
    if let Some(name) = find_by_namespace(cx.rt, this, namespace, local)? {
        remove_attribute(cx.rt, this, &name)?;
    }
    Ok(())
}

/// `Element.toggleAttribute`.
pub(super) fn toggle_attribute(
    cx: &mut Cx<'_>,
    this: NodeId,
    qualified: &str,
    force: Option<bool>,
) -> b::Result<bool> {
    if !is_valid_attribute_name(qualified) {
        return Err(invalid_attribute_name(qualified));
    }
    if let Some(name) = find_by_qualified_name(cx.rt, this, qualified)? {
        if force.unwrap_or(false) {
            return Ok(true);
        }
        remove_attribute(cx.rt, this, &name)?;
        Ok(false)
    } else {
        if force == Some(false) {
            return Ok(false);
        }
        let lowered = lowered(dom(cx.rt)?, this, qualified).into_owned();
        let name = QualName::new(None, ns!(), LocalName::from(lowered));
        set_attribute(cx.rt, this, &name, "")?;
        Ok(true)
    }
}

/// `Element.getAttributeNode`.
pub(super) fn get_attribute_node(
    cx: &mut Cx<'_>,
    this: NodeId,
    qualified: &str,
) -> b::Result<Option<Gc<Object>>> {
    let name = find_by_qualified_name(cx.rt, this, qualified)?;
    existing_attr(cx, this, name)
}

/// `Element.getAttributeNodeNS`.
pub(super) fn get_attribute_node_ns(
    cx: &mut Cx<'_>,
    this: NodeId,
    namespace: Option<&str>,
    local: &str,
) -> b::Result<Option<Gc<Object>>> {
    let name = find_by_namespace(cx.rt, this, namespace, local)?;
    existing_attr(cx, this, name)
}

/// `Element.attributes`: the one `NamedNodeMap` of the element.
pub(super) fn new_named_node_map(
    rt: &mut Runtime,
    realm: u32,
    element: NodeId,
) -> VmResult<Gc<Object>> {
    let data = rt.alloc_host_data(MapData { element })?;
    rt.new_host_object(realm, g::NAMED_NODE_MAP, data)
}

// ----- Document: createAttribute -----

/// `Document.createAttribute`: a detached `Attr` with a lowercased local
/// name.
pub(super) fn create_attribute(cx: &mut Cx<'_>, local: &str) -> b::Result<Gc<Object>> {
    if !is_valid_attribute_name(local) {
        return Err(Exception::dom(
            "InvalidCharacterError",
            format!("The localName provided ('{local}') contains an invalid character."),
        ));
    }
    let name = QualName::new(None, ns!(), LocalName::from(local.to_ascii_lowercase()));
    let realm = cx.realm();
    Ok(new_attr(cx.rt, realm, None, name, String::new())?)
}

/// `Document.createAttributeNS`.
pub(super) fn create_attribute_ns(
    cx: &mut Cx<'_>,
    namespace: Option<String>,
    qualified: &str,
) -> b::Result<Gc<Object>> {
    let parts = validate_and_extract(namespace, qualified, false)?;
    let name = QualName::new(
        parts.prefix.map(Into::into),
        Namespace::from(parts.namespace.unwrap_or_default()),
        LocalName::from(parts.local),
    );
    let realm = cx.realm();
    Ok(new_attr(cx.rt, realm, None, name, String::new())?)
}

// ----- Attr -----

impl g::attr::AttrImpl for WebState {
    fn namespace_uri(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<String>> {
        let ns = data_of(cx.rt, this)?.name.ns.to_string();
        Ok(Some(ns).filter(|ns| !ns.is_empty()))
    }

    fn prefix(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<String>> {
        Ok(data_of(cx.rt, this)?
            .name
            .prefix
            .as_ref()
            .map(ToString::to_string))
    }

    fn local_name(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(data_of(cx.rt, this)?.name.local.to_string())
    }

    fn name(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(qualified_name(&data_of(cx.rt, this)?.name))
    }

    fn value(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        let data = data_of(cx.rt, this)?;
        let (owner, name) = (data.owner, data.name.clone());
        match owner {
            Some(element) => Ok(value_of(cx.rt, element, &name)?.unwrap_or_default()),
            None => Ok(data_of(cx.rt, this)?.value.clone()),
        }
    }

    fn set_value(cx: &mut Cx<'_>, this: Platform, value: String) -> b::Result<()> {
        let data = data_of(cx.rt, this)?;
        let (owner, name) = (data.owner, data.name.clone());
        if let Some(element) = owner {
            set_attribute(cx.rt, element, &name, &value)
        } else {
            data_of_mut(cx.rt, this)?.value = value;
            Ok(())
        }
    }

    fn owner_element(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<NodeId>> {
        Ok(data_of(cx.rt, this)?.owner)
    }

    fn specified(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<bool> {
        Ok(true)
    }
}

// ----- NamedNodeMap -----

/// The data of a `NamedNodeMap`: the element.
struct MapData {
    element: NodeId,
}

impl GenericData for MapData {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        0
    }
}

fn map_element(rt: &Runtime, this: Platform) -> b::Result<NodeId> {
    match this.data {
        HostData::Generic(handle) => Ok(rt
            .heap()
            .generic::<MapData>(handle)
            .map_err(VmError::from)?
            .element),
        HostData::Word(_) => Err(VmError::type_error("Illegal invocation").into()),
    }
}

impl g::named_node_map::NamedNodeMapImpl for WebState {
    fn length(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        let element = map_element(cx.rt, this)?;
        let count = dom(cx.rt)?
            .element(element)
            .map_or(0, |e| e.attributes().len());
        u32::try_from(count).map_err(|_| Exception::range_error("too many attributes"))
    }

    fn item(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<Option<Gc<Object>>> {
        let element = map_element(cx.rt, this)?;
        let name = dom(cx.rt)?
            .element(element)
            .and_then(|e| e.attributes().get(index as usize))
            .map(|a| a.name.clone());
        existing_attr(cx, element, name)
    }

    fn get_named_item(
        cx: &mut Cx<'_>,
        this: Platform,
        qualified_name: String,
    ) -> b::Result<Option<Gc<Object>>> {
        let element = map_element(cx.rt, this)?;
        get_attribute_node(cx, element, &qualified_name)
    }

    fn get_named_item_ns(
        cx: &mut Cx<'_>,
        this: Platform,
        namespace: Option<String>,
        local_name: String,
    ) -> b::Result<Option<Gc<Object>>> {
        let element = map_element(cx.rt, this)?;
        get_attribute_node_ns(cx, element, namespace.as_deref(), &local_name)
    }

    fn set_named_item(
        cx: &mut Cx<'_>,
        this: Platform,
        attr: Platform,
    ) -> b::Result<Option<Gc<Object>>> {
        let element = map_element(cx.rt, this)?;
        set_attribute_node(cx, element, attr)
    }

    fn set_named_item_ns(
        cx: &mut Cx<'_>,
        this: Platform,
        attr: Platform,
    ) -> b::Result<Option<Gc<Object>>> {
        let element = map_element(cx.rt, this)?;
        set_attribute_node(cx, element, attr)
    }

    fn remove_named_item(
        cx: &mut Cx<'_>,
        this: Platform,
        qualified_name: String,
    ) -> b::Result<Gc<Object>> {
        let element = map_element(cx.rt, this)?;
        let Some(name) = find_by_qualified_name(cx.rt, element, &qualified_name)? else {
            return Err(not_found(&format!(
                "No item with name '{qualified_name}' was found."
            )));
        };
        remove_named(cx, element, &name)
    }

    fn remove_named_item_ns(
        cx: &mut Cx<'_>,
        this: Platform,
        namespace: Option<String>,
        local_name: String,
    ) -> b::Result<Gc<Object>> {
        let element = map_element(cx.rt, this)?;
        let Some(name) = find_by_namespace(cx.rt, element, namespace.as_deref(), &local_name)?
        else {
            return Err(not_found(&format!(
                "No item with name '{}::{local_name}' was found.",
                namespace.unwrap_or_default()
            )));
        };
        remove_named(cx, element, &name)
    }

    fn index_count(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        Self::length(cx, this)
    }

    /// The qualified names of the attributes, without duplicates; an
    /// element of the HTML namespace leaves out the names that have ASCII
    /// upper case letters.
    fn supported_names(cx: &mut Cx<'_>, this: Platform) -> b::Result<Vec<String>> {
        let element = map_element(cx.rt, this)?;
        let dom = dom(cx.rt)?;
        let html = dom.element(element).is_some_and(|e| e.name.ns == ns!(html));
        let mut names: Vec<String> = Vec::new();
        for attr in dom.element(element).map_or(&[][..], |e| e.attributes()) {
            let name = qualified_name(&attr.name);
            if html && name.bytes().any(|c| c.is_ascii_uppercase()) {
                continue;
            }
            if !names.contains(&name) {
                names.push(name);
            }
        }
        Ok(names)
    }

    fn is_supported_name(cx: &mut Cx<'_>, this: Platform, name: &str) -> b::Result<bool> {
        let element = map_element(cx.rt, this)?;
        let dom = dom(cx.rt)?;
        let html = dom.element(element).is_some_and(|e| e.name.ns == ns!(html));
        if html && name.bytes().any(|c| c.is_ascii_uppercase()) {
            return Ok(false);
        }
        Ok(dom
            .element(element)
            .and_then(|e| e.attr_by_qualified_name(name))
            .is_some())
    }
}

/// Removes the attribute `name` and returns its `Attr` (detached).
fn remove_named(cx: &mut Cx<'_>, element: NodeId, name: &QualName) -> b::Result<Gc<Object>> {
    let realm = cx.realm();
    let object = attr_object(cx.rt, realm, element, name)?;
    remove_attribute(cx.rt, element, name)?;
    Ok(object)
}
