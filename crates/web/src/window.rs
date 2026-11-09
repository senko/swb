//! `Window` (HTML §7.2, <https://html.spec.whatwg.org/multipage/nav-history-apis.html#the-window-object>)
//! as the global object, and its named properties object
//! `WindowProperties` (Web IDL §3.7.4,
//! <https://webidl.spec.whatwg.org/#named-properties-object>).
//!
//! Script sees the `WindowProxy` of the window (ADR 0031 section 5): `this`
//! of a member is the proxy, also for a call with `undefined` as `this`, and
//! `window`, `self` and `frames` return it. `top`, `parent`, `frameElement`,
//! `length`, `name` and `closed` ask the frame registry (`frames.rs`).
//!
//! `WindowProperties` sits between `Window.prototype` and
//! `EventTarget.prototype` and answers `window[name]` for the ids of
//! elements and the `name` attributes of `embed`, `form`, `img` and
//! `object` elements (HTML §7.2.2.3, "named access on the Window object").
//! A name with one element gives the element; a name with several gives a
//! live `HTMLCollection` of them (`lists.rs`, as Chromium; measured in
//! `tools/probes/web-dom-query.json`). The name of a child frame comes before
//! the elements (measured). Ids come from the DOM's id index; the `name`
//! attributes are cached by DOM generation.

use std::collections::HashMap;

use swb_dom::{Document, NodeId};
use swb_js::{
    ClassId, ClassSpec, Gc, HostHooks, Object, Property, PropertyDescriptor, PropertyKey, Runtime,
    Value, VmResult,
};

use crate::bindings::{self as b, Cx, Platform};
use crate::dom::lists;
use crate::generated as g;
use crate::generated::types::{
    DomStringOrFunction, EventOrUndefined, ScrollToOptions, VoidFunction,
};
use crate::state::{WebState, state, state_mut};

/// The frame of `this` if it is a cross-origin `Window`.
#[inline]
fn remote_frame(cx: &Cx<'_>, this: Platform) -> Option<u32> {
    if b::glue::is_cross_origin_this(this) {
        crate::window_proxy::window_frame(cx.rt, this.object)
    } else {
        None
    }
}

/// `window`, `self` and `frames`: the window as a proxy.
fn self_window(cx: &Cx<'_>, this: Platform) -> b::Result<Option<Gc<Object>>> {
    if b::glue::is_cross_origin_this(this) {
        return Ok(crate::window_proxy::self_window(cx.rt, this.object)?);
    }
    Ok(Some(this.object))
}

/// `postMessage`: a cross-origin `Window` is in another runtime.
fn post_message(
    cx: &mut Cx<'_>,
    this: Platform,
    message: Value,
    target_origin: &str,
    transfer: &[Gc<Object>],
) -> b::Result<()> {
    if let Some(frame) = remote_frame(cx, this) {
        return crate::messaging::post_across_runtimes(cx, frame, message, target_origin, transfer);
    }
    crate::messaging::post(cx, message, target_origin, transfer)
}

impl g::window::WindowImpl for WebState {
    fn window(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<Gc<Object>>> {
        self_window(cx, this)
    }

    fn self_(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<Gc<Object>>> {
        self_window(cx, this)
    }

    fn document(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<NodeId> {
        Ok(NodeId::DOCUMENT)
    }

    fn location(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        if b::glue::is_cross_origin_this(this) {
            return Ok(crate::window_proxy::location_of(cx.rt, this.object)?);
        }
        crate::location::location_object(cx)
    }

    fn history(cx: &mut Cx<'_>, _this: Platform) -> b::Result<Gc<Object>> {
        crate::location::history_object(cx)
    }

    /// `alert()` (HTML "simple dialogs"): dismissed at once.
    fn alert_1(cx: &mut Cx<'_>, _this: Platform) -> b::Result<()> {
        crate::dialogs::alert(cx, "");
        Ok(())
    }

    fn alert_2(cx: &mut Cx<'_>, _this: Platform, message: String) -> b::Result<()> {
        crate::dialogs::alert(cx, &message);
        Ok(())
    }

    fn confirm(cx: &mut Cx<'_>, _this: Platform, message: String) -> b::Result<bool> {
        Ok(crate::dialogs::confirm(cx, &message))
    }

    fn prompt(
        cx: &mut Cx<'_>,
        _this: Platform,
        message: String,
        _default: String,
    ) -> b::Result<Option<String>> {
        Ok(crate::dialogs::prompt(cx, &message))
    }

    fn print(cx: &mut Cx<'_>, _this: Platform) -> b::Result<()> {
        crate::dialogs::print(cx)
    }

    fn fetch(
        cx: &mut Cx<'_>,
        _this: Platform,
        input: Value,
        init: crate::generated::types::RequestInit,
    ) -> b::Result<Gc<Object>> {
        crate::fetch::window_fetch(cx, input, init)
    }

    fn structured_clone(
        cx: &mut Cx<'_>,
        _this: Platform,
        value: Value,
        options: crate::generated::types::StructuredSerializeOptions,
    ) -> b::Result<Value> {
        crate::structured::structured_clone(cx, value, options)
    }

    /// `window.originAgentCluster` (HTML "origin-keyed agent clusters"):
    /// true for a tuple origin, as in Chromium (`domain.rs`).
    fn origin_agent_cluster(cx: &mut Cx<'_>, _this: Platform) -> b::Result<bool> {
        crate::domain::origin_agent_cluster(cx)
    }

    /// `postMessage(message, targetOrigin, transfer)` (`messaging.rs`).
    fn post_message_1(
        cx: &mut Cx<'_>,
        this: Platform,
        message: Value,
        target_origin: String,
        transfer: Vec<Gc<Object>>,
    ) -> b::Result<()> {
        post_message(cx, this, message, &target_origin, &transfer)
    }

    /// `postMessage(message, options)`.
    fn post_message_2(
        cx: &mut Cx<'_>,
        this: Platform,
        message: Value,
        options: crate::generated::types::WindowPostMessageOptions,
    ) -> b::Result<()> {
        post_message(cx, this, message, &options.target_origin, &options.transfer)
    }

    fn frames(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<Gc<Object>>> {
        self_window(cx, this)
    }

    fn length(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        if let Some(frame) = remote_frame(cx, this) {
            return Ok(crate::frames::child_count_of_frame(cx.rt, frame)?);
        }
        Ok(crate::frames::length(cx.rt)?)
    }

    fn top(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<Gc<Object>>> {
        if let Some(frame) = remote_frame(cx, this) {
            return Ok(crate::frames::top_of_remote(cx.rt, this.object, frame)?);
        }
        Ok(crate::frames::top(cx.rt, this.object)?)
    }

    fn parent(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<Gc<Object>>> {
        if let Some(frame) = remote_frame(cx, this) {
            return Ok(crate::frames::parent_of_remote(cx.rt, this.object, frame)?);
        }
        Ok(crate::frames::parent(cx.rt, this.object)?)
    }

    /// `window.frameElement`.
    fn frame_element(cx: &mut Cx<'_>, _this: Platform) -> b::Result<Option<Gc<Object>>> {
        Ok(crate::frames::frame_element(cx.rt)?)
    }

    /// `window.opener`: swb has no auxiliary browsing contexts (ADR 0031
    /// section 13).
    fn opener(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<Value> {
        Ok(crate::frames::opener())
    }

    /// HTML: `null` disowns; another value becomes a data property of the
    /// window (measured: `window.opener = 7` reads 7).
    fn set_opener(cx: &mut Cx<'_>, _this: Platform, value: Value) -> b::Result<()> {
        if matches!(value, Value::Null) {
            return Ok(());
        }
        let global = crate::state::window_global(cx.rt)?;
        let key = cx.rt.key_from_str("opener")?;
        let desc = PropertyDescriptor::data(value, true, true, true);
        Ok(cx.rt.define_property(global, key, desc)?)
    }

    fn name(cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok(crate::frames::name(cx.rt)?)
    }

    fn set_name(cx: &mut Cx<'_>, _this: Platform, value: String) -> b::Result<()> {
        Ok(crate::frames::set_name(cx.rt, &value)?)
    }

    fn closed(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        if let Some(frame) = remote_frame(cx, this) {
            return Ok(!crate::frames::frame_alive(cx.rt, frame)?);
        }
        Ok(crate::frames::closed(cx.rt)?)
    }

    /// `window.close()`: does nothing, because no script opened a window (swb
    /// has no auxiliary browsing contexts; measured, also for a frame's window).
    fn close(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<()> {
        log::debug!("window.close(): only a window that a script opened can close");
        Ok(())
    }

    /// `self.origin`: the serialized origin of the document (`"null"` for an
    /// opaque one).
    fn origin(cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok(crate::state::engine(cx.rt)?.origin_text())
    }

    /// `window.event` (DOM §2.3): the window's current event, or
    /// `undefined`.
    fn event(cx: &mut Cx<'_>, _this: Platform) -> b::Result<EventOrUndefined> {
        let event = state(cx.rt)?.doc.current_event;
        Ok(
            match event.and_then(|e| Some((e, cx.rt.host_object(e.into())?.1))) {
                Some((object, data)) => EventOrUndefined::Event(Platform { object, data }),
                None => EventOrUndefined::Undefined(()),
            },
        )
    }

    /// `performance` (HR Time §5): one object per window.
    fn performance(cx: &mut Cx<'_>, _this: Platform) -> b::Result<Gc<Object>> {
        // An `EventTarget` keeps its listeners in its data.
        crate::state::window_object(
            cx,
            |d| &mut d.performance,
            g::PERFORMANCE,
            crate::state::target_data,
        )
    }

    /// `navigator` (HTML §8.9): one object per window.
    fn navigator(cx: &mut Cx<'_>, _this: Platform) -> b::Result<Gc<Object>> {
        crate::navigator::navigator_object(cx)
    }

    /// `customElements` (HTML §4.13.4): the window's registry.
    fn custom_elements(cx: &mut Cx<'_>, _this: Platform) -> b::Result<Gc<Object>> {
        crate::custom_elements::registry_object(cx)
    }

    /// `localStorage` (HTML §12.2.3): one object per window; a
    /// `SecurityError` for a document without storage.
    fn local_storage(cx: &mut Cx<'_>, _this: Platform) -> b::Result<Gc<Object>> {
        crate::storage::storage_object(cx, crate::storage::StorageKind::Local)
    }

    /// `sessionStorage` (HTML §12.2.2).
    fn session_storage(cx: &mut Cx<'_>, _this: Platform) -> b::Result<Gc<Object>> {
        crate::storage::storage_object(cx, crate::storage::StorageKind::Session)
    }

    /// `crypto` (Web Crypto §10): one object per window.
    fn crypto(cx: &mut Cx<'_>, _this: Platform) -> b::Result<Gc<Object>> {
        crate::crypto::crypto_object(cx)
    }

    fn btoa(_cx: &mut Cx<'_>, _this: Platform, data: String) -> b::Result<String> {
        crate::base64::encode(&data).ok_or_else(crate::base64::encode_error)
    }

    fn atob(_cx: &mut Cx<'_>, _this: Platform, data: String) -> b::Result<String> {
        crate::base64::decode(&data).map_err(crate::base64::decode_error)
    }

    fn request_idle_callback(
        cx: &mut Cx<'_>,
        _this: Platform,
        callback: crate::generated::types::IdleRequestCallback,
        options: crate::generated::types::IdleRequestOptions,
    ) -> b::Result<u32> {
        crate::idle_callbacks::request(cx, callback.0, &options)
    }

    fn cancel_idle_callback(cx: &mut Cx<'_>, _this: Platform, handle: u32) -> b::Result<()> {
        crate::idle_callbacks::cancel(cx.rt, handle)
    }

    /// `getComputedStyle`: the element can belong to another document of the
    /// agent (the document's layout answers; measured, case `geometry`).
    fn get_computed_style(
        cx: &mut Cx<'_>,
        _this: Platform,
        elt: Value,
        pseudo_elt: Option<Option<String>>,
    ) -> b::Result<Gc<Object>> {
        let data = cx
            .rt
            .host_object(elt)
            .filter(|_| cx.rt.implements(elt, g::ELEMENT))
            .map(|(_, data)| data);
        let Some(swb_js::HostData::Word(word)) = data else {
            return Err(b::Exception::type_error(
                "parameter 1 is not of type 'Element'.",
            ));
        };
        let data = swb_js::HostData::Word(word);
        // The declaration belongs to the realm of this window (measured), its
        // element to the element's document.
        let realm = cx.realm();
        let entered = crate::state::switch_if_other(cx.rt, b::node_document(word))?;
        let result = match b::node_here(cx.rt, data) {
            Some(element) => {
                crate::cssom::computed_style(cx, realm, element, pseudo_elt.flatten().as_deref())
            }
            None => Err(b::Exception::type_error(
                "parameter 1 is not of type 'Element'.",
            )),
        };
        crate::state::leave(cx.rt, entered);
        result
    }

    /// `window.focus()`: the page's focus comes to this window (measured).
    fn focus(cx: &mut Cx<'_>, this: Platform) -> b::Result<()> {
        if let Some(frame) = remote_frame(cx, this) {
            return Ok(crate::frames::focus_enters_remote(cx.rt, frame)?);
        }
        Ok(crate::html::focus::focus_window(cx.rt)?)
    }

    /// `window.blur()`: does nothing (measured, also for a frame's window).
    fn blur(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<()> {
        Ok(())
    }

    // ----- CSSOM View (`cssom_view.rs`) -----

    fn screen(cx: &mut Cx<'_>, _this: Platform) -> b::Result<Gc<Object>> {
        crate::cssom_view::screen(cx)
    }

    fn inner_width(cx: &mut Cx<'_>, _this: Platform) -> b::Result<i32> {
        Ok(crate::cssom_view::viewport_size(cx.rt)?.0)
    }

    fn inner_height(cx: &mut Cx<'_>, _this: Platform) -> b::Result<i32> {
        Ok(crate::cssom_view::viewport_size(cx.rt)?.1)
    }

    fn scroll_x(cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(crate::cssom_view::scroll_position(cx.rt)?.0)
    }

    fn page_x_offset(cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(crate::cssom_view::scroll_position(cx.rt)?.0)
    }

    fn scroll_y(cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(crate::cssom_view::scroll_position(cx.rt)?.1)
    }

    fn page_y_offset(cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        Ok(crate::cssom_view::scroll_position(cx.rt)?.1)
    }

    fn screen_x(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<i32> {
        Ok(0)
    }

    fn screen_left(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<i32> {
        Ok(0)
    }

    fn screen_y(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<i32> {
        Ok(0)
    }

    fn screen_top(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<i32> {
        Ok(0)
    }

    fn outer_width(cx: &mut Cx<'_>, _this: Platform) -> b::Result<i32> {
        Ok(crate::cssom_view::viewport_size(cx.rt)?.0)
    }

    fn outer_height(cx: &mut Cx<'_>, _this: Platform) -> b::Result<i32> {
        Ok(crate::cssom_view::viewport_size(cx.rt)?.1)
    }

    fn device_pixel_ratio(cx: &mut Cx<'_>, _this: Platform) -> b::Result<f64> {
        crate::cssom_view::device_pixel_ratio(cx.rt)
    }

    fn scroll_1(cx: &mut Cx<'_>, _this: Platform, options: ScrollToOptions) -> b::Result<()> {
        let request = crate::cssom_view::options_request(&options, false);
        crate::cssom_view::scroll(cx.rt, None, request)
    }

    fn scroll_2(cx: &mut Cx<'_>, _this: Platform, x: f64, y: f64) -> b::Result<()> {
        let request = crate::cssom_view::point_request(x, y, false);
        crate::cssom_view::scroll(cx.rt, None, request)
    }

    fn scroll_to_1(cx: &mut Cx<'_>, this: Platform, options: ScrollToOptions) -> b::Result<()> {
        Self::scroll_1(cx, this, options)
    }

    fn scroll_to_2(cx: &mut Cx<'_>, this: Platform, x: f64, y: f64) -> b::Result<()> {
        Self::scroll_2(cx, this, x, y)
    }

    fn scroll_by_1(cx: &mut Cx<'_>, _this: Platform, options: ScrollToOptions) -> b::Result<()> {
        let request = crate::cssom_view::options_request(&options, true);
        crate::cssom_view::scroll(cx.rt, None, request)
    }

    fn scroll_by_2(cx: &mut Cx<'_>, _this: Platform, x: f64, y: f64) -> b::Result<()> {
        let request = crate::cssom_view::point_request(x, y, true);
        crate::cssom_view::scroll(cx.rt, None, request)
    }

    fn match_media(cx: &mut Cx<'_>, _this: Platform, query: String) -> b::Result<Gc<Object>> {
        crate::media_queries::match_media(cx, &query)
    }

    fn request_animation_frame(
        cx: &mut Cx<'_>,
        _this: Platform,
        callback: crate::generated::types::FrameRequestCallback,
    ) -> b::Result<u32> {
        crate::animation_frames::request(cx.rt, callback.0)
    }

    fn cancel_animation_frame(cx: &mut Cx<'_>, _this: Platform, handle: u32) -> b::Result<()> {
        crate::animation_frames::cancel(cx.rt, handle)
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

/// Registers the class of `WindowProperties` (after the generated
/// classes) and remembers its number.
pub(crate) fn define_class(rt: &mut Runtime) -> VmResult<ClassId> {
    let hooks = HostHooks {
        get_own_property: Some(get_own_property_in_document),
        define_own_property: Some(define_own_property),
        delete: Some(delete_in_document),
        prevent_extensions: Some(b::legacy::prevent_extensions),
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
        let object = rt.new_host_object_with_proto(
            realm,
            Some(target.prototype),
            class,
            crate::state::NOT_A_NODE,
        )?;
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
    /// A child frame, by its number.
    Frame(u32),
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
    if name.is_empty() {
        return Ok(None);
    }
    if let Some(frame) = crate::frames::child_named(rt, name)? {
        return Ok(Some(Named::Frame(frame)));
    }
    let state = state(rt)?;
    let Some(host) = state.doc.host.as_deref() else {
        return Ok(None);
    };
    let dom = host.dom();
    let by_id = dom.count_elements_by_id(NodeId::DOCUMENT, name, 2);
    let first_id = if by_id > 0 {
        dom.element_by_id(name)
    } else {
        None
    };
    let generation = dom.generation();
    let (first_name, by_name) = match &state.doc.window_names {
        Some((built, names)) if *built == generation => names
            .get(name)
            .map_or((None, 0), |&(n, c)| (Some(n), usize::from(c))),
        _ => {
            let names = build_names(dom);
            let found = names
                .get(name)
                .map_or((None, 0), |&(n, c)| (Some(n), usize::from(c)));
            state_mut(rt)?.doc.window_names = Some((generation, names));
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
    let global = crate::state::window_global(rt)?;
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

/// [`get_own_property`] in the document of the named properties object.
fn get_own_property_in_document(
    rt: &mut Runtime,
    object: Gc<Object>,
    key: PropertyKey,
) -> VmResult<Option<Property>> {
    let entered = crate::state::enter_object(rt, object)?;
    let result = get_own_property(rt, object, key);
    crate::state::leave(rt, entered);
    result
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
    let realm = crate::state::doc_realm(rt)?;
    let value = match found {
        Named::Frame(frame) => match crate::frames::window_of_frame(rt, frame)? {
            Some(window) => window,
            None => return Ok(None),
        },
        Named::One(element) => <WebState as b::Host>::wrap_node(rt, realm, element)?,
        Named::Many => lists::window_name(rt, realm, &name)?,
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

/// [`delete`] in the document of the named properties object.
fn delete_in_document(
    rt: &mut Runtime,
    object: Gc<Object>,
    key: PropertyKey,
) -> VmResult<Option<bool>> {
    let entered = crate::state::enter_object(rt, object)?;
    let result = delete(rt, object, key);
    crate::state::leave(rt, entered);
    result
}

/// `[[Delete]]` (Web IDL §3.7.4.3): a visible name cannot be deleted;
/// other keys are ordinary.
fn delete(rt: &mut Runtime, object: Gc<Object>, key: PropertyKey) -> VmResult<Option<bool>> {
    Ok(visible(rt, object, key)?.map(|_| false))
}
