//! The cross-origin objects: the `WindowProxy` objects without a target and
//! their `Location` objects (HTML §7.2.3.4 and §7.4.3, `CrossOriginProperties`,
//! `CrossOriginGetOwnPropertyHelper`, `CrossOriginGet`, `CrossOriginSet`,
//! `CrossOriginOwnPropertyKeys`,
//! <https://html.spec.whatwg.org/multipage/nav-history-apis.html#crossorigin-objects>;
//! ADR 0031 section 5, "Done (feature 13a)"). Measured in Chromium 148 with the
//! cases `xo-*` of `tools/probes/web-frames.json`; where the two differ this
//! follows Chromium, and each such place says so.
//!
//! A frame of another origin, a frame with an opaque origin, and a frame past
//! the budget of realms have a proxy without a target. Each internal method of
//! such a proxy comes here. No path of this module reads a document of the
//! frame: a value comes from the frame tree (`FrameHost`), or it is a proxy of
//! this runtime, a function of the caller's realm, or a `Location` of this
//! module. When a rule is unclear or an internal read fails, the access throws.
//!
//! *The `Location`.* A second use of `ObjectKind::WindowProxy`: a proxy without a
//! target whose frame id has [`LOCATION_BIT`]. So all rules stay in the hooks, and
//! the paths of the engine that treat a proxy as exotic need no new object kind.
//! `w.location` is one object per frame and runtime (measured: the same for
//! callers of two realms). It is the companion of the proxy
//! (`Runtime::set_window_proxy_companion`), so it dies with the proxy, and a new
//! document of the frame drops it. A kept one stays cross-origin; it navigates
//! nothing, and its errors have the text of a frame that is gone (measured).
//!
//! *The functions.* `w.focus` and the accessor functions are functions of the
//! caller's realm, one per (name, realm): measured, `w.focus === g.focus` for
//! two frames, and another function from another realm. Each is a native of the
//! generated bindings (`[SwbCrossOrigin]`), which acts on its `this`, so that
//! `w.focus.call(other)` behaves as measured. The table holds at most 16
//! functions per realm and 256 realms, and they stay for the life of the runtime.

use swb_js::{
    ClassId, Gc, Object, Property, PropertyDescriptor, PropertyKey, Runtime, Value, VmError,
    VmResult, WindowProxyHooks,
};

use crate::bindings::exception::throw_with_prefix;
use crate::bindings::{self as b, Exception};
use crate::generated as g;
use crate::state::{self, WebState, state, state_mut};

/// The bit of a frame id that names the `Location` of the frame. Frame numbers
/// stay below it (`FrameHost`).
const LOCATION_BIT: u32 = 1 << 30;

/// The message of an access to a frame that is gone, or through a `Location`
/// that its frame has left (measured).
const GONE: &str = "An attempt was made to break through the security policy of the user agent.";

/// The prefix of the errors of a write to `href` through the hooks (measured;
/// also for `w.location = v`, which forwards to it).
const SET_HREF: &str = "Failed to set a named property 'href' on 'Location': ";

/// The properties of a cross-origin `Window` in the order of
/// `CrossOriginOwnPropertyKeys`, without `then`, with their functions.
const WINDOW_ATTRIBUTES: [(&str, &str); 9] = [
    ("window", "get window"),
    ("self", "get self"),
    ("location", "get location"),
    ("closed", "get closed"),
    ("frames", "get frames"),
    ("length", "get length"),
    ("top", "get top"),
    ("opener", "get opener"),
    ("parent", "get parent"),
];

/// The methods of a cross-origin `Window`, in the same order.
const WINDOW_METHODS: [&str; 4] = ["blur", "close", "focus", "postMessage"];

/// What a cross-origin object is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Remote {
    /// The `WindowProxy` of the frame.
    Window(u32),
    /// A `Location` of the frame.
    Location(u32),
}

impl Remote {
    fn interface(self) -> &'static str {
        match self {
            Remote::Window(_) => "Window",
            Remote::Location(_) => "Location",
        }
    }
}

/// What a string key names on a cross-origin object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Member {
    /// An accessor: its name and the name of its getter.
    Attribute(&'static str, &'static str),
    /// A method.
    Method(&'static str),
    /// `href` of a `Location`: a setter only.
    Href,
    /// `then`, which reads `undefined`.
    Then,
}

/// `object` as a cross-origin object.
fn remote(rt: &Runtime, object: Gc<Object>) -> Option<Remote> {
    let value = Value::Object(object);
    if rt.window_proxy_target(value).is_some() {
        return None;
    }
    let id = rt.window_proxy_frame(value)?;
    Some(if id & LOCATION_BIT == 0 {
        Remote::Window(id)
    } else {
        Remote::Location(id & !LOCATION_BIT)
    })
}

/// True if `object` is a cross-origin object.
pub(crate) fn is_cross_origin(rt: &Runtime, object: Gc<Object>) -> bool {
    remote(rt, object).is_some()
}

/// True if `object` is a cross-origin object of the interface `class`.
pub(crate) fn is_cross_origin_of(rt: &Runtime, object: Gc<Object>, class: ClassId) -> bool {
    match remote(rt, object) {
        Some(Remote::Window(_)) => class == g::WINDOW || class == g::EVENT_TARGET,
        Some(Remote::Location(_)) => class == g::LOCATION,
        None => false,
    }
}

/// True if `object` is a cross-origin `Location`.
pub(crate) fn is_cross_origin_location(rt: &Runtime, object: Gc<Object>) -> bool {
    matches!(remote(rt, object), Some(Remote::Location(_)))
}

/// The frame of the cross-origin `Window` `object`.
pub(crate) fn window_frame(rt: &Runtime, object: Gc<Object>) -> Option<u32> {
    match remote(rt, object) {
        Some(Remote::Window(frame)) => Some(frame),
        _ => None,
    }
}

/// True if `location` is the `Location` of the frame's current document.
fn location_is_current(rt: &Runtime, location: Gc<Object>, frame: u32) -> bool {
    let Ok(state) = state(rt) else {
        return false;
    };
    state
        .window_proxies
        .get(&frame)
        .and_then(|&proxy| rt.window_proxy_companion(proxy))
        == Some(location)
}

/// True if what `object` stands for is still there.
fn is_live(rt: &Runtime, object: Gc<Object>) -> bool {
    match remote(rt, object) {
        Some(Remote::Window(frame)) => crate::frames::frame_alive(rt, frame).unwrap_or(false),
        Some(Remote::Location(frame)) => location_is_current(rt, object, frame),
        None => false,
    }
}

// ----- The errors -----

/// The message of a `SecurityError` of the running code about `object`
/// (measured): `O` is the origin of the running code.
fn blocked_text(rt: &mut Runtime, object: Gc<Object>) -> String {
    if !is_live(rt, object) {
        return GONE.to_owned();
    }
    let key = state::running_key(rt).ok();
    let origin = key
        .and_then(|key| state::origin_of_document(rt, key).ok())
        .unwrap_or_else(|| "null".to_owned());
    let retired = key.is_some_and(|key| {
        state(rt)
            .ok()
            .and_then(|state| state::document_ref(state, key))
            .is_none_or(|record| record.retired)
    });
    if retired {
        format!("Blocked a restricted frame with origin \"{origin}\" from accessing another frame.")
    } else {
        format!("Blocked a frame with origin \"{origin}\" from accessing a cross-origin frame.")
    }
}

/// A `SecurityError` of the running code's realm with `prefix`.
fn security_error(rt: &mut Runtime, object: Gc<Object>, prefix: &str) -> VmError {
    let text = blocked_text(rt, object);
    let realm = rt.running_realm();
    throw_with_prefix(
        rt,
        realm,
        prefix,
        g::DOM_EXCEPTION,
        Exception::dom("SecurityError", text),
    )
}

/// The `SecurityError` of an access that names no property (`in`, `delete`,
/// `defineProperty`, `setPrototypeOf`, `preventExtensions`, a member that the
/// object does not have).
pub(crate) fn blocked(rt: &mut Runtime, object: Gc<Object>) -> VmError {
    security_error(rt, object, "")
}

/// The text of a key in a message; `None` if the message names no key
/// (measured: the empty string, a symbol without a description and
/// `@@toPrimitive`).
fn key_text(rt: &Runtime, key: PropertyKey) -> Option<String> {
    if let PropertyKey::Symbol(_) = key {
        if key == rt.to_primitive_key() {
            return None;
        }
        return rt
            .symbol_description(key)
            .map(|description| format!("[{description}]"));
    }
    rt.key_to_string(key).filter(|text| !text.is_empty())
}

/// The prefix of a failed read or write of `key`.
fn prefix(rt: &Runtime, object: Gc<Object>, key: PropertyKey, write: bool) -> String {
    let interface = remote(rt, object).map_or("Window", Remote::interface);
    let (verb, place) = if write {
        ("set", "on")
    } else {
        ("read", "from")
    };
    if let PropertyKey::Index(index) = key {
        return format!("Failed to {verb} an indexed property [{index}] {place} '{interface}': ");
    }
    match key_text(rt, key) {
        Some(text) => format!("Failed to {verb} a named property '{text}' {place} '{interface}': "),
        None => format!("Failed to {verb} a named property {place} '{interface}': "),
    }
}

fn read_error(rt: &mut Runtime, object: Gc<Object>, key: PropertyKey) -> VmError {
    let prefix = prefix(rt, object, key, false);
    security_error(rt, object, &prefix)
}

fn write_error(rt: &mut Runtime, object: Gc<Object>, key: PropertyKey) -> VmError {
    let prefix = prefix(rt, object, key, true);
    security_error(rt, object, &prefix)
}

/// A line of the debug log for each read and write of a cross-origin object: what
/// the scripts of a page do with its frames.
fn note_access(rt: &Runtime, object: Remote, action: &str, key: PropertyKey) {
    if log::log_enabled!(log::Level::Debug) {
        let text = key_text(rt, key).unwrap_or_default();
        log::debug!(
            "a script: {action} {text} of a cross-origin {}",
            object.interface()
        );
    }
}

// ----- Members -----

/// What the string key `key` names on `object`.
fn member(rt: &Runtime, object: Remote, key: PropertyKey) -> Option<Member> {
    let PropertyKey::String(_) = key else {
        return None;
    };
    let text = rt.key_to_string(key)?;
    if text == "then" {
        return Some(Member::Then);
    }
    match object {
        Remote::Location(_) => match text.as_str() {
            "href" => Some(Member::Href),
            "replace" => Some(Member::Method("replace")),
            _ => None,
        },
        Remote::Window(_) => {
            if let Some(&(name, getter)) = WINDOW_ATTRIBUTES.iter().find(|(n, _)| *n == text) {
                return Some(Member::Attribute(name, getter));
            }
            WINDOW_METHODS
                .iter()
                .find(|name| **name == text)
                .map(|name| Member::Method(name))
        }
    }
}

/// The child frame named by `key` of the frame of a `Window`: not for an empty
/// name (measured).
fn named_child(rt: &mut Runtime, frame: u32, key: PropertyKey) -> VmResult<Option<Gc<Object>>> {
    let PropertyKey::String(_) = key else {
        return Ok(None);
    };
    let Some(name) = rt.key_to_string(key).filter(|name| !name.is_empty()) else {
        return Ok(None);
    };
    crate::frames::child_named_of_frame(rt, frame, &name)
}

/// The function `name` of the running code's realm: made once per realm, and
/// traced in the binding state.
fn function(rt: &mut Runtime, object: Gc<Object>, name: &'static str) -> VmResult<Value> {
    let realm = rt.running_realm();
    if let Some(&known) = state(rt)?.cross_origin_functions.get(&(realm, name)) {
        return Ok(known.into());
    }
    let native = g::window::cross_origin_native::<WebState>(name)
        .or_else(|| g::location::cross_origin_native::<WebState>(name));
    let Some((length, native)) = native else {
        return Err(blocked(rt, object));
    };
    let made = rt.new_host_method(realm, name, length, native)?;
    state_mut(rt)?
        .cross_origin_functions
        .insert((realm, name), made);
    Ok(made.into())
}

/// The value of an attribute of the cross-origin `Window` `proxy` of `frame`.
fn attribute(rt: &mut Runtime, proxy: Gc<Object>, frame: u32, name: &str) -> VmResult<Value> {
    let or_null = |window: Option<Gc<Object>>| window.map_or(Value::Null, Value::Object);
    Ok(match name {
        "window" | "self" | "frames" => or_null(self_window(rt, proxy)?),
        "location" => location_of(rt, proxy)?.into(),
        "closed" => Value::Bool(!crate::frames::frame_alive(rt, frame)?),
        "length" => {
            Value::Int(i32::try_from(crate::frames::child_count_of_frame(rt, frame)?).unwrap_or(0))
        }
        "top" => or_null(crate::frames::top_of_remote(rt, proxy, frame)?),
        "parent" => or_null(crate::frames::parent_of_remote(rt, proxy, frame)?),
        "opener" => crate::frames::opener(),
        _ => return Err(blocked(rt, proxy)),
    })
}

/// `window`, `self` and `frames` of a cross-origin `Window`: the proxy, and
/// `null` when its frame is gone (measured).
pub(crate) fn self_window(rt: &Runtime, proxy: Gc<Object>) -> VmResult<Option<Gc<Object>>> {
    let frame = window_frame(rt, proxy).ok_or(VmError::type_error("Illegal invocation"))?;
    Ok(crate::frames::frame_alive(rt, frame)?.then_some(proxy))
}

/// The `Location` of a cross-origin `Window`. It is made at its first read and
/// stays until the frame gets another document.
pub(crate) fn location_of(rt: &mut Runtime, proxy: Gc<Object>) -> VmResult<Gc<Object>> {
    if let Some(known) = rt.window_proxy_companion(proxy) {
        return Ok(known);
    }
    let frame = window_frame(rt, proxy).ok_or(VmError::type_error("Illegal invocation"))?;
    let location = rt.new_window_proxy(frame | LOCATION_BIT, None)?;
    rt.set_window_proxy_companion(proxy, location)?;
    Ok(location)
}

// ----- The navigation -----

/// A write to `href` of the cross-origin `Location` `location`, or `replace`
/// (measured, cases `xo-navigate`, `xo-navigate-rules`, `xo-location-this`): any
/// origin may ask, under `may_navigate`. A `Location` that its frame has left and
/// a caller whose frame is gone do nothing, without an error. The URL resolves
/// against the entry document, and the page navigates after the task.
pub(crate) fn navigate(rt: &mut Runtime, location: Gc<Object>, text: &str) -> b::Result<()> {
    let Some(Remote::Location(frame)) = remote(rt, location) else {
        return Err(Exception::Blocked(location));
    };
    if !location_is_current(rt, location, frame) {
        return Ok(());
    }
    let (source, source_retired) = {
        let caller = state::caller_document(rt)?;
        (caller.frame, caller.retired)
    };
    if source_retired {
        return Ok(());
    }
    let allowed = crate::frames::may_navigate(rt, source, frame)?;
    let url = crate::location::parse_against_base(rt, text)?;
    // Measured: a refusal comes before the check of the URL. A `javascript:` URL
    // needs one origin (HTML "navigate to a javascript: URL"), so it is refused.
    let scripted = url.as_ref().is_some_and(|url| url.scheme() == "javascript");
    if !allowed || scripted {
        let shown = url.as_ref().map_or(text, url::Url::as_str);
        return Err(Exception::dom(
            "SecurityError",
            format!(
                "The current window does not have permission to navigate the target frame to '{shown}'."
            ),
        ));
    }
    let Some(url) = url else {
        return Err(Exception::dom(
            "SyntaxError",
            format!("'{text}' is not a valid URL."),
        ));
    };
    crate::frames::request_navigation(rt, frame, url.as_str(), source)?;
    Ok(())
}

/// `location.href = value` through the hooks: the conversion and the navigation
/// have the prefix of a named property.
fn set_href(rt: &mut Runtime, location: Gc<Object>, value: Value) -> VmResult<()> {
    let result = b::convert::usv_string(rt, value).and_then(|text| navigate(rt, location, &text));
    result.map_err(|exception| {
        let realm = rt.running_realm();
        throw_with_prefix(rt, realm, SET_HREF, g::DOM_EXCEPTION, exception)
    })
}

// ----- The hooks -----

/// Installs the internal methods of the proxies without a target.
pub(crate) fn install_hooks(rt: &mut Runtime) {
    rt.set_window_proxy_hooks(WindowProxyHooks {
        get_prototype_of,
        set_prototype_of,
        prevent_extensions,
        get_own_property,
        define_own_property,
        has,
        get,
        set,
        delete,
        own_keys,
        child_count,
        child_frame,
        blocked,
    });
}

/// `[[GetPrototypeOf]]`: `null`.
#[allow(clippy::unnecessary_wraps, reason = "the signature of the hook")]
fn get_prototype_of(_rt: &mut Runtime, _: Gc<Object>) -> VmResult<Option<Gc<Object>>> {
    Ok(None)
}

/// `[[SetPrototypeOf]]`. HTML has `SetImmutablePrototype`, which accepts `null`;
/// measured: an error for every value.
fn set_prototype_of(rt: &mut Runtime, object: Gc<Object>, _: Option<Gc<Object>>) -> VmResult<bool> {
    Err(blocked(rt, object))
}

fn prevent_extensions(rt: &mut Runtime, object: Gc<Object>) -> VmResult<bool> {
    Err(blocked(rt, object))
}

/// A property that a cross-origin object has and that reads `undefined`, or a
/// method: not writable, not enumerable, configurable.
fn data_property(value: Value) -> Property {
    Property::Data {
        value,
        writable: false,
        enumerable: false,
        configurable: true,
    }
}

/// `[[GetOwnProperty]]`: `CrossOriginGetOwnPropertyHelper` and the frames.
fn get_own_property(
    rt: &mut Runtime,
    object: Gc<Object>,
    key: PropertyKey,
) -> VmResult<Option<Property>> {
    let Some(kind) = remote(rt, object) else {
        return Err(blocked(rt, object));
    };
    if let (PropertyKey::Index(index), Remote::Window(_)) = (key, kind)
        && let Some(child) = crate::frames::child_of_proxy(rt, object, index)?
    {
        return Ok(Some(Property::Data {
            value: child.into(),
            writable: false,
            enumerable: true,
            configurable: true,
        }));
    }
    let found = member(rt, kind, key);
    if let (Some(Member::Then) | None, Remote::Window(frame)) = (found, kind)
        && let Some(child) = named_child(rt, frame, key)?
    {
        return Ok(Some(data_property(child.into())));
    }
    Ok(Some(match found {
        Some(Member::Method(name)) => data_property(function(rt, object, name)?),
        Some(Member::Attribute(name, getter)) => {
            let get = function(rt, object, getter)?;
            let set = if name == "location" {
                function(rt, object, "set location")?
            } else {
                Value::Undefined
            };
            Property::Accessor {
                get,
                set,
                enumerable: false,
                configurable: true,
            }
        }
        Some(Member::Href) => Property::Accessor {
            get: Value::Undefined,
            set: function(rt, object, "set href")?,
            enumerable: false,
            configurable: true,
        },
        Some(Member::Then) => data_property(Value::Undefined),
        None if rt.cross_origin_symbol_keys().contains(&key) => data_property(Value::Undefined),
        None => return Err(read_error(rt, object, key)),
    }))
}

fn define_own_property(
    rt: &mut Runtime,
    object: Gc<Object>,
    _: PropertyKey,
    _: &PropertyDescriptor,
) -> VmResult<bool> {
    Err(blocked(rt, object))
}

/// `[[HasProperty]]`, as measured: an index reads the frame; the names and the
/// three symbols are true, except `then` and the name of a frame, which throw.
fn has(rt: &mut Runtime, object: Gc<Object>, key: PropertyKey) -> VmResult<bool> {
    let Some(kind) = remote(rt, object) else {
        return Err(blocked(rt, object));
    };
    if let PropertyKey::Index(index) = key {
        if let Remote::Window(_) = kind
            && crate::frames::child_of_proxy(rt, object, index)?.is_some()
        {
            return Ok(true);
        }
        return Err(read_error(rt, object, key));
    }
    match member(rt, kind, key) {
        Some(Member::Then) => Err(blocked(rt, object)),
        Some(_) => Ok(true),
        None if rt.cross_origin_symbol_keys().contains(&key) => Ok(true),
        None => Err(blocked(rt, object)),
    }
}

/// `[[Get]]`: `CrossOriginGet`. The receiver does not count: measured,
/// `Object.create(w).closed` reads the frame's.
fn get(rt: &mut Runtime, object: Gc<Object>, key: PropertyKey, _: Value) -> VmResult<Value> {
    let Some(kind) = remote(rt, object) else {
        return Err(blocked(rt, object));
    };
    note_access(rt, kind, "read", key);
    if let (PropertyKey::Index(index), Remote::Window(_)) = (key, kind)
        && let Some(child) = crate::frames::child_of_proxy(rt, object, index)?
    {
        return Ok(child.into());
    }
    let found = member(rt, kind, key);
    if let (Some(Member::Then) | None, Remote::Window(frame)) = (found, kind)
        && let Some(child) = named_child(rt, frame, key)?
    {
        return Ok(child.into());
    }
    match (found, kind) {
        (Some(Member::Method(name)), _) => function(rt, object, name),
        (Some(Member::Attribute(name, _)), Remote::Window(frame)) => {
            attribute(rt, object, frame, name)
        }
        (Some(Member::Then), _) => Ok(Value::Undefined),
        (None, _) if rt.cross_origin_symbol_keys().contains(&key) => Ok(Value::Undefined),
        _ => Err(read_error(rt, object, key)),
    }
}

/// `[[Set]]`: `CrossOriginSet`. Only `location` of a `Window` and `href` of a
/// `Location`; the receiver does not count (measured).
fn set(
    rt: &mut Runtime,
    object: Gc<Object>,
    key: PropertyKey,
    value: Value,
    _: Value,
) -> VmResult<bool> {
    let Some(kind) = remote(rt, object) else {
        return Err(blocked(rt, object));
    };
    note_access(rt, kind, "write", key);
    match (member(rt, kind, key), kind) {
        (Some(Member::Attribute("location", _)), Remote::Window(_)) => {
            let location = location_of(rt, object)?;
            set_href(rt, location, value)?;
            Ok(true)
        }
        (Some(Member::Href), Remote::Location(_)) => {
            set_href(rt, object, value)?;
            Ok(true)
        }
        _ => Err(write_error(rt, object, key)),
    }
}

fn delete(rt: &mut Runtime, object: Gc<Object>, _: PropertyKey) -> VmResult<bool> {
    Err(blocked(rt, object))
}

/// `[[OwnPropertyKeys]]`: the indices of the frames and
/// `CrossOriginOwnPropertyKeys`. Measured: no names of frames.
fn own_keys(rt: &mut Runtime, object: Gc<Object>) -> VmResult<Vec<PropertyKey>> {
    let mut keys = Vec::new();
    match remote(rt, object) {
        Some(Remote::Window(frame)) => {
            let count = crate::frames::child_count_of_frame(rt, frame)?;
            keys.extend((0..count).map(PropertyKey::Index));
            for (name, _) in WINDOW_ATTRIBUTES {
                keys.push(rt.key_from_str(name)?);
            }
            for name in WINDOW_METHODS {
                keys.push(rt.key_from_str(name)?);
            }
        }
        Some(Remote::Location(_)) => {
            keys.push(rt.key_from_str("href")?);
            keys.push(rt.key_from_str("replace")?);
        }
        None => return Err(blocked(rt, object)),
    }
    keys.push(rt.key_from_str("then")?);
    keys.extend(rt.cross_origin_symbol_keys());
    Ok(keys)
}

fn child_count(rt: &mut Runtime, proxy: Gc<Object>) -> VmResult<u32> {
    crate::frames::child_count_of_proxy(rt, proxy)
}

fn child_frame(rt: &mut Runtime, proxy: Gc<Object>, index: u32) -> VmResult<Option<Gc<Object>>> {
    crate::frames::child_of_proxy(rt, proxy, index)
}

#[cfg(test)]
mod tests {
    use swb_js::PropertyDescriptor;

    use crate::clock::{ClockMode, PageClock};
    use crate::runtime::{Evaluation, JsonForm, ScriptRuntime};

    /// A runtime without a page: the frame of the proxy does not exist.
    fn with_remote(source: &str) -> Evaluation {
        let mut runtime = ScriptRuntime::new(PageClock::new(ClockMode::Virtual, None)).unwrap();
        let proxy = runtime.rt.new_window_proxy(9, None).unwrap();
        let global = runtime.rt.global(0).unwrap();
        let key = runtime.rt.key_from_str("remote").unwrap();
        let desc = PropertyDescriptor::data(proxy.into(), true, false, true);
        runtime.rt.define_property(global, key, desc).unwrap();
        runtime.evaluate(source, JsonForm::Probe)
    }

    #[test]
    fn access_to_a_proxy_without_a_target_follows_the_rules() {
        let source = "
            const E = g => { try { g(); return 'no error'; } catch (e) { return e.name; } };
            [
              () => remote.x, () => { remote.x = 1; }, () => 'x' in remote, () => delete remote.x,
              () => Object.setPrototypeOf(remote, null), () => Object.preventExtensions(remote),
              () => Object.defineProperty(remote, 'x', {value: 1}),
              () => Object.getOwnPropertyDescriptor(remote, 'x'),
            ].map(E).join() + ' ' + Object.isExtensible(remote) + ' '
              + Object.getPrototypeOf(remote) + ' ' + Object.keys(remote).length;";
        assert_eq!(
            with_remote(source),
            Evaluation::Value(format!(
                "\"{} true null 0\"",
                ["SecurityError"; 8].join(",")
            ))
        );
    }

    /// The cost of the table (ADR 0031 "Done (feature 13a)"): all functions of one
    /// realm, the budget of 256 realms times this.
    #[test]
    fn the_functions_of_a_realm_are_bounded() {
        let mut runtime = ScriptRuntime::new(PageClock::new(ClockMode::Virtual, None)).unwrap();
        let proxy = runtime.rt.new_window_proxy(9, None).unwrap();
        let global = runtime.rt.global(0).unwrap();
        let key = runtime.rt.key_from_str("remote").unwrap();
        let desc = PropertyDescriptor::data(proxy.into(), true, false, true);
        runtime.rt.define_property(global, key, desc).unwrap();
        let before = runtime.heap_size();
        let source =
            "for (const k of Reflect.ownKeys(remote)) Object.getOwnPropertyDescriptor(remote, k);
             Object.getOwnPropertyDescriptor(remote.location, 'href');
             Object.getOwnPropertyDescriptor(remote.location, 'replace'); 1";
        assert_eq!(
            runtime.evaluate(source, JsonForm::Probe),
            Evaluation::Value("1".to_owned())
        );
        let after = runtime.heap_size();
        let table = crate::state::state(&runtime.rt)
            .unwrap()
            .cross_origin_functions
            .len();
        assert_eq!(table, 16);
        assert!(after - before < 16_000, "{} bytes", after - before);
    }

    /// No script holds a function between the two reads: only the table of the
    /// runtime keeps it, and a mark on it tells that it is the same object.
    #[test]
    fn the_table_of_functions_keeps_them_across_a_collection() {
        let mut runtime = ScriptRuntime::new(PageClock::new(ClockMode::Virtual, None)).unwrap();
        let proxy = runtime.rt.new_window_proxy(9, None).unwrap();
        let global = runtime.rt.global(0).unwrap();
        let key = runtime.rt.key_from_str("remote").unwrap();
        let desc = PropertyDescriptor::data(proxy.into(), true, false, true);
        runtime.rt.define_property(global, key, desc).unwrap();
        let closed = "Object.getOwnPropertyDescriptor(remote, 'closed').get";
        let mark = format!("remote.focus.mark = 1; {closed}.mark = 2; 1");
        assert_eq!(
            runtime.evaluate(&mark, JsonForm::Probe),
            Evaluation::Value("1".to_owned())
        );
        runtime.collect();
        let read = format!("[remote.focus.mark, {closed}.mark, remote.focus.name].join()");
        assert_eq!(
            runtime.evaluate(&read, JsonForm::Probe),
            Evaluation::Value("\"1,2,focus\"".to_owned())
        );
    }

    #[test]
    fn functions_are_one_per_name_and_the_cache_survives_a_collection() {
        let source = "
            const f = remote.focus; gc(); const d = Object.getOwnPropertyDescriptor(remote, 'closed');
            [f === remote.focus, f.name, d.get.name, remote.postMessage.length,
             Object.getOwnPropertyDescriptor(remote, 'closed').get === d.get].join();";
        let source = source.replace("gc();", "for (let i = 0; i < 200000; i++) ({});");
        assert_eq!(
            with_remote(&source),
            Evaluation::Value("\"true,focus,get closed,1,true\"".to_owned())
        );
    }
}
