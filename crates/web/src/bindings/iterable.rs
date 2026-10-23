//! Iterable declarations (Web IDL §3.7.9,
//! <https://webidl.spec.whatwg.org/#es-iterable>).
//!
//! - A value iterator (`iterable<V>` on an interface with an indexed
//!   property getter) uses the functions of `%Array.prototype%`:
//!   `entries`, `keys`, `values` and `forEach` are those functions, and
//!   `@@iterator` of every interface that declares an indexed property
//!   getter is `%Array.prototype.values%` ([`define_value_iterator`],
//!   [`define_indexed_iterator`]).
//! - A pair iterator (`iterable<K, V>`) has `entries` (also `@@iterator`),
//!   `keys`, `values` and `forEach` of its own; the first three return a
//!   default iterator object ([`new_iterator`]), a host object of the
//!   iterator class (named "X Iterator") whose prototype is the iterator
//!   prototype object of the interface ([`iterator_prototype`]: `next` and
//!   `@@toStringTag`, `[[Prototype]]` `%Iterator.prototype%`).
//!
//! Chromium's property order and attributes (measured in
//! `tools/probes/web-legacy.json`): a value iterator's four functions come
//! first on the prototype (`entries`, `keys`, `values`, `forEach`), a pair
//! iterator's after the operations (`entries`, `forEach`, `keys`,
//! `values`); `@@iterator` is not enumerable. Chromium's iterator results
//! list `done` before `value` ([`iterator_result`]).

use swb_js::{
    Attributes, ClassId, Gc, Generic, GenericData, HostData, Object, Runtime, Tracer, Value,
    VmError, VmResult,
};

use super::{Host, Platform, glue_state};

/// The kind of a default iterator object (§3.7.9.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `keys()`.
    Key,
    /// `values()`.
    Value,
    /// `entries()` and `@@iterator`.
    KeyValue,
}

/// The internal values of a default iterator object.
struct IteratorState {
    target: Gc<Object>,
    kind: Kind,
    index: u32,
}

impl GenericData for IteratorState {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.object(self.target);
    }

    fn heap_size(&self) -> usize {
        0
    }
}

/// A function of `%Array.prototype%` of `realm` (read from the global
/// object before any script runs).
fn array_function(rt: &mut Runtime, realm: u32, name: &str) -> VmResult<Value> {
    let prototype = global_prototype(rt, realm, "Array")?;
    let key = rt.key_from_str(name)?;
    rt.get_key(prototype.into(), key)
}

/// `C.prototype` of the global constructor `C` of `realm`.
fn global_prototype(rt: &mut Runtime, realm: u32, name: &str) -> VmResult<Gc<Object>> {
    let global = rt.global(realm)?;
    let key = rt.key_from_str(name)?;
    let constructor = rt.get_key(global.into(), key)?;
    let key = rt.key_from_str("prototype")?;
    match rt.get_key(constructor, key)? {
        Value::Object(prototype) => Ok(prototype),
        _ => Err(VmError::type_error("a built-in prototype is missing")),
    }
}

/// The value iterator of an interface (§3.7.9 step 2): `entries`, `keys`,
/// `values` and `forEach` of `%Array.prototype%` on the prototype.
pub fn define_value_iterator(rt: &mut Runtime, realm: u32, prototype: Gc<Object>) -> VmResult<()> {
    for name in ["entries", "keys", "values", "forEach"] {
        let function = array_function(rt, realm, name)?;
        rt.define_value(prototype, name, function, Attributes::MEMBER)?;
    }
    Ok(())
}

/// `@@iterator` of an interface with an indexed property getter (§3.7.9
/// step 1): `%Array.prototype.values%`, not enumerable.
pub fn define_indexed_iterator(
    rt: &mut Runtime,
    realm: u32,
    prototype: Gc<Object>,
) -> VmResult<()> {
    let values = array_function(rt, realm, "values")?;
    let key = rt.iterator_key();
    rt.define_value_with_key(prototype, key, values, Attributes::HIDDEN)
}

/// `@@iterator` of a pair iterator: the `entries` function, not
/// enumerable.
pub fn define_pair_iterator(
    rt: &mut Runtime,
    prototype: Gc<Object>,
    entries: Gc<Object>,
) -> VmResult<()> {
    let key = rt.iterator_key();
    rt.define_value_with_key(prototype, key, entries.into(), Attributes::HIDDEN)
}

/// Creates the iterator prototype object of an interface with a pair
/// iterator in `realm` (§3.7.9.2) and keeps it in the glue state: `next`
/// (the native function `next`) and `@@toStringTag` (the name of the
/// iterator class, "X Iterator"), with `%Iterator.prototype%` as its
/// prototype.
pub fn iterator_prototype<H: Host>(
    rt: &mut Runtime,
    realm: u32,
    class: ClassId,
    next: swb_js::NativeFn,
) -> VmResult<Gc<Object>> {
    let prototype = rt.new_plain_object(realm)?;
    glue_state::<H>(rt)?
        .iterator_prototypes
        .push(((realm, class), prototype));
    let parent = global_prototype(rt, realm, "Iterator")?;
    rt.heap_mut()
        .set_prototype_of(prototype, Some(parent))
        .map_err(VmError::from)?;
    rt.define_method(realm, prototype, "next", 0, next, Attributes::MEMBER)?;
    let name = rt
        .class_name(class)
        .ok_or(VmError::type_error("an iterator class without a name"))?
        .to_owned();
    let tag = rt.string_value(&name)?;
    let key = rt.to_string_tag_key();
    let attributes = Attributes {
        writable: false,
        enumerable: false,
        configurable: true,
    };
    rt.define_value_with_key(prototype, key, tag, attributes)?;
    Ok(prototype)
}

/// A new default iterator object of the iterator class `class` for
/// `target` (§3.7.9.1), with the index 0.
pub fn new_iterator<H: Host>(
    rt: &mut Runtime,
    realm: u32,
    class: ClassId,
    target: Gc<Object>,
    kind: Kind,
) -> VmResult<Gc<Object>> {
    let prototype = glue_state::<H>(rt)?
        .iterator_prototypes
        .iter()
        .find(|(key, _)| *key == (realm, class))
        .map(|&(_, prototype)| prototype)
        .ok_or(VmError::type_error("the iterator prototype is missing"))?;
    let data = rt.alloc_host_data(IteratorState {
        target,
        kind,
        index: 0,
    })?;
    rt.new_host_object_with_proto(Some(prototype), class, data)
}

/// A step of `next` (§3.7.9.2): the brand check of `this` (a default
/// iterator object of `class`, else `TypeError: Illegal invocation`), its
/// target (with the target's host data), kind and index.
pub fn step(
    rt: &Runtime,
    call: &swb_js::NativeCall,
    class: ClassId,
) -> super::Result<(Gc<Generic>, Platform, Kind, u32)> {
    let HostData::Generic(handle) = rt.this_host_data(call, class)? else {
        return Err(VmError::type_error("Illegal invocation").into());
    };
    let state = rt
        .heap()
        .generic::<IteratorState>(handle)
        .map_err(VmError::from)?;
    let (target, kind, index) = (state.target, state.kind, state.index);
    let (_, data) = rt
        .host_object(target.into())
        .ok_or(VmError::type_error("Illegal invocation"))?;
    Ok((
        handle,
        Platform {
            object: target,
            data,
        },
        kind,
        index,
    ))
}

/// Moves a default iterator object to the next pair.
pub fn advance(rt: &mut Runtime, handle: Gc<Generic>) -> VmResult<()> {
    let state = rt
        .heap_mut()
        .generic_mut::<IteratorState>(handle)
        .map_err(VmError::from)?;
    state.index = state.index.saturating_add(1);
    Ok(())
}

/// The iterator result object (`CreateIteratorResultObject`): `{ done,
/// value }`, in Chromium's order (measured: `done` first); `None` is the
/// end (`value` undefined).
pub fn iterator_result(rt: &mut Runtime, realm: u32, value: Option<Value>) -> VmResult<Value> {
    let object = rt.new_plain_object(realm)?;
    let done = Value::Bool(value.is_none());
    super::to_js::dictionary_member(rt, object, "done", done)?;
    super::to_js::dictionary_member(rt, object, "value", value.unwrap_or(Value::Undefined))?;
    Ok(object.into())
}

/// The `[key, value]` array of an entry.
pub fn entry(rt: &mut Runtime, realm: u32, key: Value, value: Value) -> VmResult<Value> {
    super::keep(rt, key);
    super::keep(rt, value);
    Ok(rt.new_array_of(realm, &[key, value])?.into())
}

/// The callback of `forEach` (converted to `Function`: a callable value,
/// else "parameter 1 is not of type 'Function'.").
pub fn callback(rt: &Runtime, value: Value) -> super::Result<Value> {
    if rt.callable(value)? {
        Ok(value)
    } else {
        Err(super::Exception::NotOfType("Function".into()))
    }
}
