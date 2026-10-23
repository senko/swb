//! The conversions of IDL values to JavaScript values (Web IDL §3.2, the
//! second half of each type's section). Numbers need no function
//! ([`swb_js::Value::number`]); the rest allocate.

use swb_js::{Gc, Object, Runtime, Value, VmResult};

use super::{Host, NodeId};

/// A string.
pub fn string(rt: &mut Runtime, text: &str) -> VmResult<Value> {
    rt.string_value(text)
}

/// A nullable string (`null` for `None`).
pub fn nullable_string(rt: &mut Runtime, text: Option<&str>) -> VmResult<Value> {
    match text {
        Some(text) => rt.string_value(text),
        None => Ok(Value::Null),
    }
}

/// The wrapper of a node.
pub fn node<H: Host>(rt: &mut Runtime, realm: u32, node: NodeId) -> VmResult<Value> {
    Ok(H::wrap_node(rt, realm, node)?.into())
}

/// An object reference (another interface, `object`, a callback, a
/// promise).
pub fn object(object: Gc<Object>) -> Value {
    Value::Object(object)
}

/// A sequence: a new array of `realm` with the converted items. Each
/// converted item is recorded in the handle scope until the array holds
/// it.
pub fn sequence<T>(
    rt: &mut Runtime,
    realm: u32,
    items: &[T],
    mut f: impl FnMut(&mut Runtime, &T) -> VmResult<Value>,
) -> VmResult<Value> {
    let mut values = Vec::with_capacity(items.len());
    for item in items {
        let value = f(rt, item)?;
        super::keep(rt, value);
        values.push(value);
    }
    Ok(rt.new_array_of(realm, &values)?.into())
}

/// A record: a new object of `realm` with a data property per entry.
pub fn record<V>(
    rt: &mut Runtime,
    realm: u32,
    entries: &[(String, V)],
    mut f: impl FnMut(&mut Runtime, &V) -> VmResult<Value>,
) -> VmResult<Value> {
    let object = rt.new_plain_object(realm)?;
    for (key, value) in entries {
        let value = f(rt, value)?;
        super::keep(rt, value);
        let key = rt.key_from_str(key)?;
        rt.create_data_property(object, key, value)?;
    }
    Ok(object.into())
}

/// Sets the member `key` of a dictionary object that the glue builds.
pub fn dictionary_member(
    rt: &mut Runtime,
    object: Gc<Object>,
    key: &str,
    value: Value,
) -> VmResult<()> {
    super::keep(rt, value);
    let key = rt.key_from_str(key)?;
    rt.create_data_property(object, key, value)
}
