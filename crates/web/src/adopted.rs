//! `adoptedStyleSheets` of `Document` and `ShadowRoot` (CSSOM §6.1
//! <https://drafts.csswg.org/cssom/#dom-documentorshadowroot-adoptedstylesheets>,
//! ADR 0036 section 6): Web IDL's observable array
//! (<https://webidl.spec.whatwg.org/#es-observable-array>) of constructed
//! `CSSStyleSheet` objects.
//!
//! Each document and shadow root that a script asks gets one array: a
//! `Proxy` whose target is a real `Array` (so `Array.isArray` is true and
//! the array methods work) and whose handler has four natives as traps
//! (`set`, `defineProperty`, `deleteProperty`, `preventExtensions`). The
//! traps check each value, change the target, and change the engine's list
//! of the scope ([`crate::EngineHost::adopted_sheets`]), which the stylist
//! reads. The other internal methods of the proxy go to the target, which
//! always holds the backing list: Web IDL's `get`, `has`, `ownKeys` and
//! `getOwnPropertyDescriptor` traps give the same answers.
//!
//! The handler is a host object whose word names the scope, so that the
//! traps find the scope's document after a move (as a node's wrapper). The
//! arrays live as long as their document's binding state, as the wrappers
//! do; the sheets in an array live as long as it holds them.
//!
//! Measured in Chromium 148 (cases `adopted-array`, `adopted-host-moves`,
//! `constructable-sheets`): the trap errors have no context prefix, the
//! setter's have the setter's; a value is checked when it is put into the
//! list (a setter that fails on its second value keeps the first); a shadow
//! root that moves to another document (but its template contents owner)
//! loses its sheets ([`swb_dom::ShadowRootData::adoptions`]).

use std::collections::HashMap;

use swb_dom::NodeId;
use swb_js::{
    ClassId, ClassSpec, Gc, HostData, NativeCall, NativeReturn, Object, PropertyKey, Runtime,
    Tracer, Value, VmError, VmResult,
};
use swb_style::cssom::sheet::SheetRef;

use crate::bindings::{self as b, Cx};
use crate::generated as g;
use crate::state::{DocKey, dom, engine_mut, state, state_mut};

/// The most entries of one array (ADR 0036 section 12; Chromium has no
/// limit): a write past it throws `NotAllowedError`.
pub(crate) const MAX_ADOPTED: usize = 65_536;

/// The arrays of the scopes of one document.
#[derive(Default)]
pub(crate) struct AdoptedArrays {
    records: HashMap<NodeId, Record>,
    /// True after the warning about [`MAX_ADOPTED`].
    warned: bool,
}

/// The objects of one array, and the adoption count of its shadow root at
/// the last check (0 for a document).
#[derive(Clone, Copy)]
struct Record {
    proxy: Gc<Object>,
    target: Gc<Object>,
    handler: Gc<Object>,
    adoptions: u32,
}

impl AdoptedArrays {
    /// Reports the arrays to the collector.
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for record in self.records.values() {
            tracer.object(record.proxy);
            tracer.object(record.target);
            tracer.object(record.handler);
        }
    }

    /// The array of the shadow root `old` goes to the document `to` with the
    /// node's new id `new` (a move between arenas, `state::move_node_state`).
    /// Returns its handler, whose word the caller changes. The move counted
    /// as an adoption of the root: the caller empties the array then
    /// ([`forget_moved`]).
    pub(crate) fn move_to(
        &mut self,
        to: &mut AdoptedArrays,
        old: NodeId,
        new: NodeId,
    ) -> Option<Gc<Object>> {
        let record = self.records.remove(&old)?;
        to.records.insert(new, record);
        Some(record.handler)
    }
}

/// The traps of the arrays of one realm: `set`, `defineProperty`,
/// `deleteProperty`, `preventExtensions`.
pub(crate) type Traps = [Gc<Object>; 4];

/// Registers the class of the handlers (after the generated classes).
pub(crate) fn define_class(rt: &mut Runtime) -> VmResult<ClassId> {
    let class = rt.define_class(ClassSpec::new("ObservableArrayHandler"))?;
    state_mut(rt)?.adopted_handler = Some(class);
    Ok(class)
}

/// `adoptedStyleSheets` of `scope` (a document or a shadow root of the
/// current document): its array, made at the first use.
pub(crate) fn get(cx: &mut Cx<'_>, scope: NodeId) -> b::Result<Value> {
    Ok(record(cx.rt, scope)?.proxy.into())
}

/// Sets `adoptedStyleSheets` of `scope` (Web IDL "set the value of an
/// observable array attribute"): the sequence is converted first; then the
/// list is emptied and each value is checked and appended in turn.
pub(crate) fn set(cx: &mut Cx<'_>, scope: NodeId, value: Value) -> b::Result<()> {
    let values = b::convert::sequence(cx.rt, value, |rt, item| {
        crate::cssom_sheets::constructed_sheet(rt, item).map(|sheet| (item, sheet))
    })?;
    let record = record(cx.rt, scope)?;
    truncate(cx.rt, scope, record.target, 0)?;
    let mut result = Ok(());
    for (index, (item, sheet)) in values.into_iter().enumerate() {
        let checked = check(cx.rt, scope, index, sheet);
        match checked {
            Ok(sheet) => put(cx.rt, scope, record.target, index, item, sheet)?,
            Err(error) => {
                result = Err(error);
                break;
            }
        }
    }
    changed(cx.rt)?;
    result
}

/// Empties the arrays of the shadow roots `moved` of the current document
/// that moved to another document since their last check (the arena's
/// report, [`swb_dom::Document::take_moved_roots`], or a move between
/// arenas), so that a script that holds such an array sees it empty at once
/// (measured in Chromium 148). Roots without an array cost one lookup.
pub(crate) fn forget_moved(rt: &mut Runtime, moved: &[NodeId]) -> VmResult<()> {
    for &scope in moved {
        if scope == NodeId::DOCUMENT || !state(rt)?.doc.adopted.records.contains_key(&scope) {
            continue;
        }
        // `record` makes no exception of its own here (the record exists);
        // an engine error, such as a termination, passes.
        if let Err(b::Exception::Js(error)) = record(rt, scope) {
            return Err(error);
        }
    }
    Ok(())
}

/// The record of `scope`, made if it has none; the sheets of a shadow root
/// that moved to another document since the last check go.
fn record(rt: &mut Runtime, scope: NodeId) -> b::Result<Record> {
    let adoptions = adoptions_of(rt, scope)?;
    if let Some(record) = state(rt)?.doc.adopted.records.get(&scope).copied() {
        if record.adoptions != adoptions {
            truncate(rt, scope, record.target, 0)?;
            changed(rt)?;
            if let Some(record) = state_mut(rt)?.doc.adopted.records.get_mut(&scope) {
                record.adoptions = adoptions;
            }
        }
        return Ok(record);
    }
    let realm = crate::state::objects_realm(rt)?;
    let traps = traps(rt, realm)?;
    let class = state(rt)?.adopted_handler.ok_or(VmError::invariant(
        "the class of the array handlers is not registered",
    ))?;
    let word = b::word_of(crate::state::document_key(rt)?, scope);
    let handler = rt.new_host_object_with_proto(realm, None, class, HostData::Word(word))?;
    for (name, trap) in [
        "set",
        "defineProperty",
        "deleteProperty",
        "preventExtensions",
    ]
    .into_iter()
    .zip(traps)
    {
        let key = rt.key_from_str(name)?;
        rt.create_data_property(handler, key, trap.into())?;
    }
    let target = rt.new_array_of(realm, &[])?;
    let proxy = rt.new_proxy(target, handler)?;
    let record = Record {
        proxy,
        target,
        handler,
        adoptions,
    };
    state_mut(rt)?.doc.adopted.records.insert(scope, record);
    Ok(record)
}

/// The adoption count of the shadow root `scope` (0 for a document).
fn adoptions_of(rt: &Runtime, scope: NodeId) -> VmResult<u32> {
    Ok(dom(rt)?.shadow_root_data(scope).map_or(0, |d| d.adoptions))
}

/// The traps of `realm`, made at the first use.
fn traps(rt: &mut Runtime, realm: u32) -> VmResult<Traps> {
    if let Some(traps) = state(rt)?.adopted_traps.get(&realm) {
        return Ok(*traps);
    }
    let traps = [
        rt.new_host_method(realm, "set", 4, trap_set)?,
        rt.new_host_method(realm, "defineProperty", 3, trap_define_property)?,
        rt.new_host_method(realm, "deleteProperty", 2, trap_delete_property)?,
        rt.new_host_method(realm, "preventExtensions", 1, trap_prevent_extensions)?,
    ];
    state_mut(rt)?.adopted_traps.insert(realm, traps);
    Ok(traps)
}

/// CSSOM "set an indexed value" of `adoptedStyleSheets`: a constructed
/// sheet of the scope's document (the window's document of the arena),
/// within [`MAX_ADOPTED`] entries. `sheet` is the conversion's answer
/// (`None` for a sheet of an element).
fn check(
    rt: &mut Runtime,
    scope: NodeId,
    index: usize,
    sheet: Option<(SheetRef, DocKey)>,
) -> b::Result<SheetRef> {
    let Some((sheet, constructor)) = sheet else {
        return Err(b::Exception::dom(
            "NotAllowedError",
            "Can't adopt non-constructed stylesheets.",
        ));
    };
    let dom = dom(rt)?;
    let document = if dom.shadow_root_data(scope).is_some() {
        dom.node_document(scope)
    } else {
        scope
    };
    if document != NodeId::DOCUMENT || constructor != crate::state::document_key(rt)? {
        return Err(b::Exception::dom(
            "NotAllowedError",
            "Sharing constructed stylesheets in multiple documents is not allowed",
        ));
    }
    if index >= MAX_ADOPTED {
        let arrays = &mut state_mut(rt)?.doc.adopted;
        if !std::mem::replace(&mut arrays.warned, true) {
            log::warn!("adoptedStyleSheets: more than {MAX_ADOPTED} entries; refused");
        }
        return Err(b::Exception::dom(
            "NotAllowedError",
            "swb: too many adopted style sheets",
        ));
    }
    Ok(sheet)
}

/// Puts `item` (the wrapper of `sheet`) at `index` (at most the length) of
/// the list of `scope`, in the target and in the engine's list.
fn put(
    rt: &mut Runtime,
    scope: NodeId,
    target: Gc<Object>,
    index: usize,
    item: Value,
    sheet: SheetRef,
) -> b::Result<()> {
    let key = PropertyKey::Index(u32::try_from(index).unwrap_or(u32::MAX));
    rt.create_data_property(target, key, item)?;
    if let Some(list) = engine_mut(rt)?.adopted_sheets(scope) {
        match list.get_mut(index) {
            Some(slot) => *slot = sheet,
            None => list.push(sheet),
        }
    }
    Ok(())
}

/// Shortens the list of `scope` to `length` entries, in the target and in
/// the engine's list.
fn truncate(rt: &mut Runtime, scope: NodeId, target: Gc<Object>, length: usize) -> b::Result<()> {
    let key = rt.key_from_str("length")?;
    rt.set_key(target.into(), key, Value::number(length as f64), true)?;
    if let Some(list) = engine_mut(rt)?.adopted_sheets(scope) {
        list.truncate(length);
    }
    Ok(())
}

/// The length of the list (the target's `length`).
fn length_of(rt: &mut Runtime, target: Gc<Object>) -> b::Result<usize> {
    let key = rt.key_from_str("length")?;
    let length = rt.get_key(target.into(), key)?;
    Ok(rt.number_of(length)? as usize)
}

/// The rendering changes with the list.
fn changed(rt: &mut Runtime) -> b::Result<()> {
    engine_mut(rt)?.style_sheets_changed();
    Ok(())
}

// ----- The traps (Web IDL §3.11 "observable array exotic objects") -----

/// The part of a trap's work that knows its scope: the scope of the handler
/// (`this`), its document current, and the target. Errors become exceptions
/// without a context prefix (measured), in the trap's realm.
fn run_trap(
    rt: &mut Runtime,
    call: &NativeCall,
    f: impl FnOnce(&mut Runtime, NodeId, Gc<Object>) -> b::Result<bool>,
) -> VmResult<NativeReturn> {
    let Value::Object(handler) = call.this() else {
        return Err(VmError::type_error("Illegal invocation"));
    };
    let entered = crate::state::enter_object(rt, handler)?;
    let result = (|| -> b::Result<bool> {
        let scope = rt
            .host_object(handler.into())
            .and_then(|(_, data)| b::node_of(data))
            .ok_or_else(|| b::Exception::type_error("Illegal invocation"))?;
        let target = record(rt, scope)?.target;
        f(rt, scope, target)
    })();
    crate::state::leave(rt, entered);
    match result {
        Ok(done) => Ok(NativeReturn::Value(Value::Bool(done))),
        Err(error) => Err(b::exception::throw_with_prefix(
            rt,
            call.realm(),
            "",
            g::DOM_EXCEPTION,
            error,
        )),
    }
}

/// What a trap's key is.
enum Key {
    Length,
    Index(usize),
    Other(PropertyKey),
}

fn key_of(rt: &mut Runtime, value: Value) -> b::Result<Key> {
    let key = rt.property_key_of(value)?;
    Ok(match key {
        PropertyKey::Index(index) => Key::Index(index as usize),
        _ if key == rt.key_from_str("length")? => Key::Length,
        _ => Key::Other(key),
    })
}

/// The `set` trap.
fn trap_set(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    run_trap(rt, call, |rt, scope, target| {
        let (key, value, receiver) = (rt.arg(call, 1), rt.arg(call, 2), rt.arg(call, 3));
        match key_of(rt, key)? {
            Key::Length => set_length(rt, scope, target, value),
            Key::Index(index) => set_indexed(rt, scope, target, index, value),
            Key::Other(key) => Ok(rt.set_with_receiver_of(target, key, value, receiver)?),
        }
    })
}

/// The `defineProperty` trap: data descriptors that keep the attributes of
/// an array's element or `length`.
fn trap_define_property(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    run_trap(rt, call, |rt, scope, target| {
        let (key, descriptor) = (rt.arg(call, 1), rt.arg(call, 2));
        let key = key_of(rt, key)?;
        let desc = rt.property_descriptor_of(descriptor)?;
        let element = matches!(key, Key::Index(_));
        let fits = !desc.is_accessor()
            && desc.writable != Some(false)
            && desc.configurable != Some(!element)
            && desc.enumerable != Some(!element);
        match key {
            Key::Other(key) => Ok(rt.define_own(target, key, desc)?),
            _ if !fits => Ok(false),
            Key::Length => match desc.value {
                Some(value) => set_length(rt, scope, target, value),
                None => Ok(true),
            },
            Key::Index(index) => match desc.value {
                Some(value) => set_indexed(rt, scope, target, index, value),
                None => Ok(true),
            },
        }
    })
}

/// The `deleteProperty` trap: only the last element goes.
fn trap_delete_property(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    run_trap(rt, call, |rt, scope, target| {
        let key = rt.arg(call, 1);
        match key_of(rt, key)? {
            Key::Length => Ok(false),
            Key::Index(index) => {
                let length = length_of(rt, target)?;
                if index.checked_add(1) != Some(length) {
                    return Ok(false);
                }
                truncate(rt, scope, target, index)?;
                changed(rt)?;
                Ok(true)
            }
            Key::Other(key) => Ok(rt.delete_key(target, key)?),
        }
    })
}

/// The `preventExtensions` trap: never.
fn trap_prevent_extensions(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    run_trap(rt, call, |_, _, _| Ok(false))
}

/// Web IDL "set the length": shrinks the list; a longer length is refused.
fn set_length(
    rt: &mut Runtime,
    scope: NodeId,
    target: Gc<Object>,
    value: Value,
) -> b::Result<bool> {
    let integer = rt.number_of(value)?;
    let number = rt.number_of(value)?;
    let length = to_uint32(integer);
    if f64::from(length) != number {
        return Err(b::Exception::range_error("The provided length is invalid."));
    }
    let old = length_of(rt, target)?;
    let length = length as usize;
    if length > old {
        return Ok(false);
    }
    if length < old {
        truncate(rt, scope, target, length)?;
        changed(rt)?;
    }
    Ok(true)
}

/// Web IDL "set the indexed value": replaces an entry or appends one; an
/// index past the end is refused.
fn set_indexed(
    rt: &mut Runtime,
    scope: NodeId,
    target: Gc<Object>,
    index: usize,
    value: Value,
) -> b::Result<bool> {
    let length = length_of(rt, target)?;
    if index > length {
        return Ok(false);
    }
    let sheet = crate::cssom_sheets::constructed_sheet(rt, value).map_err(b::Exception::nested)?;
    let sheet = check(rt, scope, index, sheet)?;
    put(rt, scope, target, index, value, sheet)?;
    changed(rt)?;
    Ok(true)
}

/// `ToUint32` of a number (ECMA-262 §7.1.7).
fn to_uint32(number: f64) -> u32 {
    if !number.is_finite() {
        return 0;
    }
    let integer = number.trunc().rem_euclid(4_294_967_296.0);
    integer as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uint32() {
        assert_eq!(to_uint32(1.5), 1);
        assert_eq!(to_uint32(-1.0), u32::MAX);
        assert_eq!(to_uint32(f64::NAN), 0);
        assert_eq!(to_uint32(4_294_967_296.0), 0);
        assert_eq!(to_uint32(4_294_967_297.0), 1);
    }
}
