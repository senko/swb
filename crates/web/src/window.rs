//! `Window` (HTML §7.2, <https://html.spec.whatwg.org/multipage/nav-history-apis.html#the-window-object>)
//! as the global object, and its named properties object
//! `WindowProperties` (Web IDL §3.7.4,
//! <https://webidl.spec.whatwg.org/#named-properties-object>).
//!
//! Without frames, the `WindowProxy` of the window is the global object
//! itself (ADR 0028 section 4): `window`, `self`, `frames`, `top` and
//! `parent` return it, and `length` is 0.
//!
//! `WindowProperties` sits between `Window.prototype` and
//! `EventTarget.prototype` and answers `window[name]` for the ids of
//! elements and the `name` attributes of `embed`, `form`, `img` and
//! `object` elements (HTML §7.2.2.3, "named access on the Window object").
//! A name with one element gives the element; a name with several gives a
//! live `HTMLCollection` of them (`lists.rs`, as Chromium; measured in
//! `tools/probes/web-dom-query.json`). Child navigables (frames) do not
//! exist. Ids come from the DOM's id index; the `name` attributes are
//! cached by DOM generation.

use std::collections::HashMap;

use swb_dom::{Document, NodeId};
use swb_js::{
    ClassId, ClassSpec, Gc, HostHooks, Object, Property, PropertyDescriptor, PropertyKey, Runtime,
    Value, VmResult,
};

use crate::bindings::{self as b, Cx, Platform};
use crate::dom::lists;
use crate::generated as g;
use crate::generated::types::{DomStringOrFunction, EventOrUndefined, VoidFunction};
use crate::state::{WebState, state, state_mut};

impl g::window::WindowImpl for WebState {
    fn window(_cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        Ok(this.object)
    }

    fn self_(_cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        Ok(this.object)
    }

    fn document(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<NodeId> {
        Ok(NodeId::DOCUMENT)
    }

    fn frames(_cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        Ok(this.object)
    }

    fn length(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<u32> {
        Ok(0)
    }

    fn top(_cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<Gc<Object>>> {
        Ok(Some(this.object))
    }

    fn parent(_cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<Gc<Object>>> {
        Ok(Some(this.object))
    }

    /// `window.event` (DOM §2.3): the window's current event, or
    /// `undefined`.
    fn event(cx: &mut Cx<'_>, _this: Platform) -> b::Result<EventOrUndefined> {
        let event = state(cx.rt)?.current_event;
        Ok(
            match event.and_then(|e| Some((e, cx.rt.host_object(e.into())?.1))) {
                Some((object, data)) => EventOrUndefined::Event(Platform { object, data }),
                None => EventOrUndefined::Undefined(()),
            },
        )
    }

    /// `performance` (HR Time §5): one object per window.
    fn performance(cx: &mut Cx<'_>, _this: Platform) -> b::Result<Gc<Object>> {
        if let Some(object) = state(cx.rt)?.performance {
            return Ok(object);
        }
        // An `EventTarget` keeps its listeners in its data.
        let data = cx
            .rt
            .alloc_host_data(crate::events::TargetData::default())?;
        let object = cx.rt.new_host_object(0, g::PERFORMANCE, data)?;
        state_mut(cx.rt)?.performance = Some(object);
        Ok(object)
    }

    fn report_error(cx: &mut Cx<'_>, this: Platform, e: Value) -> b::Result<()> {
        crate::timers::report_error(cx, this, e)
    }

    fn set_timeout(
        cx: &mut Cx<'_>,
        _this: Platform,
        handler: DomStringOrFunction,
        timeout: i32,
        arguments: Vec<Value>,
    ) -> b::Result<i32> {
        crate::timers::set_timeout(cx, handler, timeout, arguments)
    }

    fn clear_timeout(cx: &mut Cx<'_>, _this: Platform, id: i32) -> b::Result<()> {
        crate::timers::clear_timer(cx, id)
    }

    fn set_interval(
        cx: &mut Cx<'_>,
        _this: Platform,
        handler: DomStringOrFunction,
        timeout: i32,
        arguments: Vec<Value>,
    ) -> b::Result<i32> {
        crate::timers::set_interval(cx, handler, timeout, arguments)
    }

    fn clear_interval(cx: &mut Cx<'_>, _this: Platform, id: i32) -> b::Result<()> {
        crate::timers::clear_timer(cx, id)
    }

    fn queue_microtask(cx: &mut Cx<'_>, _this: Platform, callback: VoidFunction) -> b::Result<()> {
        crate::timers::queue_microtask(cx, callback);
        Ok(())
    }
}

/// `Performance` (HR Time §5): the time of the document's clock.
impl g::performance::PerformanceImpl for WebState {
    fn time_origin(cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(state(cx.rt)?.clock.time_origin())
    }

    fn now(cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(state(cx.rt)?.clock.now())
    }
}

/// Registers the class of `WindowProperties` (after the generated
/// classes) and remembers its number.
pub(crate) fn define_class(rt: &mut Runtime) -> VmResult<ClassId> {
    let hooks = HostHooks {
        get_own_property: Some(get_own_property),
        define_own_property: Some(define_own_property),
        delete: Some(delete),
        prevent_extensions: Some(prevent_extensions),
        ..HostHooks::default()
    };
    let class = rt.define_class(ClassSpec::new("WindowProperties").hooks(hooks))?;
    state_mut(rt)?.window_properties = Some(class);
    Ok(class)
}

/// Creates the named properties object of `realm` and puts it between
/// `Window.prototype` and `EventTarget.prototype` (before the global
/// object gets its class).
pub(crate) fn install(rt: &mut Runtime, realm: u32, class: ClassId) -> VmResult<()> {
    let (Some(window), Some(target)) = (
        rt.interface(realm, g::WINDOW),
        rt.interface(realm, g::EVENT_TARGET),
    ) else {
        return Err(swb_js::VmError::type_error(
            "the Window interface is missing",
        ));
    };
    // The handle scope roots the new values until the prototype chain
    // holds them.
    let scope = rt.heap_mut().open_scope();
    let result = (|| {
        let object =
            rt.new_host_object_with_proto(Some(target.prototype), class, crate::state::NOT_A_NODE)?;
        rt.heap_mut().record(object);
        rt.heap_mut()
            .set_prototype_of(window.prototype, Some(object))
            .map_err(swb_js::VmError::from)?;
        let tag = rt.string_value("WindowProperties")?;
        rt.heap_mut().record(tag);
        let key = rt.to_string_tag_key();
        rt.define_value_with_key(
            object,
            key,
            tag,
            swb_js::Attributes {
                writable: false,
                enumerable: false,
                configurable: true,
            },
        )?;
        rt.set_immutable_prototype(object)
    })();
    rt.heap_mut()
        .close_scope(scope)
        .map_err(swb_js::VmError::from)?;
    result
}

/// The `embed`, `form`, `img` and `object` elements that have each `name`
/// of the window's named properties: the first one in tree order and how
/// many there are (counted up to 2). An element whose `id` is its `name`
/// counts as an element of the id.
fn build_names(dom: &Document) -> HashMap<String, (NodeId, u8)> {
    let mut names: HashMap<String, (NodeId, u8)> = HashMap::new();
    for node in dom.descendants(NodeId::DOCUMENT) {
        let Some(element) = dom.element(node) else {
            continue;
        };
        let Some(name) = element.attr("name").filter(|n| !n.is_empty()) else {
            continue;
        };
        if element.id() != Some(name) && lists::named_by_window(element, name) {
            let entry = names.entry(name.to_owned()).or_insert((node, 0));
            entry.1 = entry.1.saturating_add(1).min(2);
        }
    }
    names
}

/// What a name of the window's named properties stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Named {
    /// One element.
    One(NodeId),
    /// Several elements: a collection.
    Many,
}

/// The element of a supported property name of the window, or `Many` if
/// several elements have it, or `None`: the elements with that id (the
/// DOM's id index, which changes keep up to date) and the elements with
/// that `name` (cached by generation). One element of each kind gives the
/// one with the id.
fn named(rt: &mut Runtime, name: &str) -> VmResult<Option<Named>> {
    let state = state(rt)?;
    let Some(host) = state.host.as_deref() else {
        return Ok(None);
    };
    if name.is_empty() {
        return Ok(None);
    }
    let dom = host.dom();
    let by_id = dom.count_elements_by_id(NodeId::DOCUMENT, name, 2);
    let first_id = if by_id > 0 {
        dom.element_by_id(name)
    } else {
        None
    };
    let generation = dom.generation();
    let (first_name, by_name) = match &state.window_names {
        Some((built, names)) if *built == generation => names
            .get(name)
            .map_or((None, 0), |&(n, c)| (Some(n), usize::from(c))),
        _ => {
            let names = build_names(dom);
            let found = names
                .get(name)
                .map_or((None, 0), |&(n, c)| (Some(n), usize::from(c)));
            state_mut(rt)?.window_names = Some((generation, names));
            found
        }
    };
    Ok(match by_id + by_name {
        0 => None,
        1 => first_id.or(first_name).map(Named::One),
        _ => Some(Named::Many),
    })
}

/// The named property visibility algorithm (Web IDL §3.9.7) for the global
/// object: the name is supported, and neither the global object nor a
/// prototype other than the named properties object has an own property
/// of that name. Returns what the name stands for.
fn visible(
    rt: &mut Runtime,
    wp: Gc<Object>,
    key: PropertyKey,
) -> VmResult<Option<(String, Named)>> {
    let Some(name) = string_key(rt, key) else {
        return Ok(None);
    };
    let Some(found) = named(rt, &name)? else {
        return Ok(None);
    };
    let global = rt.global(0)?;
    if rt.own_property_of(global, key)?.is_some() {
        return Ok(None);
    }
    let mut prototype = rt.prototype_of(global)?;
    while let Some(object) = prototype {
        if object != wp && rt.own_property_of(object, key)?.is_some() {
            return Ok(None);
        }
        prototype = rt.prototype_of(object)?;
    }
    Ok(Some((name, found)))
}

/// The text of a string or index key; `None` for a symbol.
fn string_key(rt: &Runtime, key: PropertyKey) -> Option<String> {
    match key {
        PropertyKey::Symbol(_) => None,
        key => rt.key_to_string(key),
    }
}

/// `[[GetOwnProperty]]` (Web IDL §3.7.4.1): a visible name is a data
/// property (writable, not enumerable because `Window` has
/// `[LegacyUnenumerableNamedProperties]`, configurable), as Chromium shows.
fn get_own_property(
    rt: &mut Runtime,
    object: Gc<Object>,
    key: PropertyKey,
) -> VmResult<Option<Property>> {
    let Some((name, found)) = visible(rt, object, key)? else {
        return Ok(None);
    };
    let value = match found {
        Named::One(element) => <WebState as b::Host>::wrap_node(rt, 0, element)?,
        Named::Many => lists::window_name(rt, 0, &name)?,
    };
    Ok(Some(Property::Data {
        value: Value::Object(value),
        writable: true,
        enumerable: false,
        configurable: true,
    }))
}

/// `[[DefineOwnProperty]]` (Web IDL §3.7.4.2): refused.
#[allow(clippy::unnecessary_wraps, reason = "the signature of the hook")]
fn define_own_property(
    _rt: &mut Runtime,
    _object: Gc<Object>,
    _key: PropertyKey,
    _desc: &PropertyDescriptor,
) -> VmResult<Option<bool>> {
    Ok(Some(false))
}

/// `[[Delete]]` (Web IDL §3.7.4.3): a visible name cannot be deleted;
/// other keys are ordinary.
fn delete(rt: &mut Runtime, object: Gc<Object>, key: PropertyKey) -> VmResult<Option<bool>> {
    Ok(visible(rt, object, key)?.map(|_| false))
}

/// `[[PreventExtensions]]` (Web IDL §3.7.4.5): refused.
#[allow(clippy::unnecessary_wraps, reason = "the signature of the hook")]
fn prevent_extensions(_rt: &mut Runtime, _object: Gc<Object>) -> VmResult<Option<bool>> {
    Ok(Some(false))
}
