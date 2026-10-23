//! The reviver walk of `JSON.parse` (ECMA-262 §25.5.1.1
//! `InternalizeJSONProperty` with the context argument of the source text
//! access proposal, <https://tc39.es/proposal-json-parse-with-source/>).
//!
//! The walk is a loop over an explicit stack of the objects being walked,
//! so a deep text costs no Rust stack; the reviver itself runs through
//! re-entry, which the recursion budget limits. Chromium ends a deep walk
//! with a `RangeError` where its own stack runs out (about 3,000 levels);
//! swb ends it at [`MAX_DEPTH`] levels.
//!
//! Each walked object keeps a handle scope open until its walk ends;
//! the children that are not objects use a scope of their own, so a
//! long array does not grow the scope. The parse records
//! ([`Records`]) give each value its source text while it is still the
//! value that the parser made (`SameValue` with the current property).

use std::collections::HashMap;

use super::json_parse::{RecKind, Records};
use crate::heap::{Gc, HandleScope};
use crate::object::Object;
use crate::runtime::Runtime;
use crate::string::{JsString, PropertyKey};
use crate::value::Value;
use crate::vm::{Intrinsic, STACK_OVERFLOW, VmResult};

/// The deepest walk. Chromium recurses on its native stack and ends a
/// walk at about 3,000 levels (it depends on the stack).
const MAX_DEPTH: usize = 4_000;

/// An object that is being walked.
struct Level {
    /// The object (the value of the property).
    object: Gc<Object>,
    /// Its holder and its name in the holder, for the call of the reviver.
    holder: Gc<Object>,
    name: PropertyKey,
    context: Gc<Object>,
    scope: HandleScope,
    /// Its parse record.
    record: Option<u32>,
    members: Members,
    /// The member records by key, for an object with many members.
    index: Option<HashMap<PropertyKey, u32>>,
    /// The next member to visit.
    next: usize,
    /// The member that is being revived.
    current: PropertyKey,
    /// The record of the member that is being revived.
    current_record: Option<u32>,
}

/// What a level visits.
enum Members {
    /// The indices below a length.
    Elements(f64),
    /// The enumerable own string keys, listed when the walk entered.
    Keys(Vec<PropertyKey>),
}

/// What the loop does next.
enum Step {
    /// Revive the property `name` of `holder`.
    Enter {
        holder: Gc<Object>,
        name: PropertyKey,
        record: Option<u32>,
    },
    /// A property was revived to this value; give it to the level above.
    Return(Value),
}

/// `InternalizeJSONProperty(root, "", reviver, snapshot)`.
pub(super) fn internalize(
    rt: &mut Runtime,
    root: Gc<Object>,
    reviver: Value,
    records: Option<Records>,
    source: Gc<JsString>,
    realm: u32,
) -> VmResult<Value> {
    let source_key = rt.heap.key_from_str("source")?;
    rt.heap.record(source_key);
    let object_proto = rt.intrinsic(realm, Intrinsic::ObjectPrototype)?;
    let empty = PropertyKey::String(rt.vm.atoms.empty);
    let records = records.unwrap_or_default();
    let mut stack: Vec<Level> = Vec::new();
    let mut step = Step::Enter {
        holder: root,
        name: empty,
        record: records.root(),
    };
    loop {
        match step {
            Step::Enter {
                holder,
                name,
                record,
            } => {
                rt.tick()?;
                let scope = rt.heap.open_scope();
                let value = rt.get_value(holder.into(), name)?;
                let context = rt.heap.new_object(Some(object_proto))?;
                rt.heap.record(context);
                let record = record.filter(|&id| {
                    records
                        .node(id)
                        .is_some_and(|node| rt.heap.same_value(node.value, value).unwrap_or(false))
                });
                let Value::Object(object) = value else {
                    if let Some(node) = record.and_then(|id| records.node(id)) {
                        let text = rt.substring(source, node.start as usize, node.end as usize)?;
                        rt.create_data_property_or_throw(context, source_key, text.into())?;
                    }
                    let result = call_reviver(rt, reviver, holder, name, value, context)?;
                    rt.heap.close_scope(scope)?;
                    step = Step::Return(result);
                    continue;
                };
                if stack.len() >= MAX_DEPTH {
                    return Err(crate::vm::VmError::range_error(STACK_OVERFLOW));
                }
                let members = list_members(rt, object)?;
                let index = member_index(&records, record);
                stack.push(Level {
                    object,
                    holder,
                    name,
                    context,
                    scope,
                    record,
                    members,
                    index,
                    next: 0,
                    current: empty,
                    current_record: None,
                });
                step = next_member(rt, &mut stack, &records, reviver)?;
            }
            Step::Return(result) => {
                let Some(level) = stack.last_mut() else {
                    return Ok(result);
                };
                // §25.5.1.1 steps 2.d.iii and 2.e.iii: an `undefined`
                // result deletes the property, another one defines it;
                // neither can fail the walk.
                let (object, key) = (level.object, level.current);
                if result.is_undefined() {
                    rt.delete_generic(object, key, false)?;
                } else if rt.heap.needs_runtime(object)? || rt.typed_numeric(object, key)?.is_some()
                {
                    // A trap, or the conversion of a typed array element,
                    // can run script code.
                    let desc = crate::object::PropertyDescriptor::data(result, true, true, true);
                    rt.define_generic(object, key, desc, false)?;
                } else {
                    rt.heap.create_data_property(object, key, result, &rt.vm)?;
                }
                step = next_member(rt, &mut stack, &records, reviver)?;
            }
        }
    }
}

/// The next step of the level on top of the stack: its next member, or,
/// when it has none, the call of the reviver for the object itself (which
/// pops the level).
fn next_member(
    rt: &mut Runtime,
    stack: &mut Vec<Level>,
    records: &Records,
    reviver: Value,
) -> VmResult<Step> {
    let Some(level) = stack.last_mut() else {
        return Err(crate::vm::VmError::invariant("a walk without a level"));
    };
    let key = match &level.members {
        Members::Elements(length) => {
            if (level.next as f64) < *length {
                let key = rt.index_key(level.next as f64)?;
                level.current_record = element_record(records, level.record, level.next);
                Some(key)
            } else {
                None
            }
        }
        Members::Keys(keys) => {
            let key = keys.get(level.next).copied();
            if let Some(key) = key {
                level.current_record = match &level.index {
                    Some(index) => index.get(&key).copied(),
                    None => member_record(records, level.record, key),
                };
            }
            key
        }
    };
    if let Some(key) = key {
        level.next += 1;
        level.current = key;
        return Ok(Step::Enter {
            holder: level.object,
            name: key,
            record: level.current_record,
        });
    }
    let Some(level) = stack.pop() else {
        return Err(crate::vm::VmError::invariant("a walk without a level"));
    };
    let result = call_reviver(
        rt,
        reviver,
        level.holder,
        level.name,
        Value::Object(level.object),
        level.context,
    )?;
    rt.heap.close_scope(level.scope)?;
    Ok(Step::Return(result))
}

/// `Call(reviver, holder, « name, value, context »)`.
fn call_reviver(
    rt: &mut Runtime,
    reviver: Value,
    holder: Gc<Object>,
    name: PropertyKey,
    value: Value,
    context: Gc<Object>,
) -> VmResult<Value> {
    let name = rt.heap.key_to_value(name)?;
    rt.heap.record(name);
    rt.call(reviver, holder.into(), &[name, value, context.into()])
}

/// The members of an object that the walk visits: for an array, its
/// indices below the length it has now; for another object, its
/// enumerable own string keys (`EnumerableOwnProperties(object, key)`).
fn list_members(rt: &mut Runtime, object: Gc<Object>) -> VmResult<Members> {
    if rt.is_array(object.into())? {
        let length = rt.length_of_array_like(object)?;
        return Ok(Members::Elements(length));
    }
    if rt.heap.needs_runtime(object)? {
        // `[[OwnPropertyKeys]]` and `[[GetOwnProperty]]` through the traps
        // (or the hooks of a host object).
        let own = rt.own_keys(object)?;
        rt.heap.record_keys(&own);
        let mut keys = Vec::new();
        for key in own {
            rt.tick()?;
            if !key.is_symbol() && rt.own_enumerable(object, key)? {
                keys.push(key);
            }
        }
        return Ok(Members::Keys(keys));
    }
    let bound = rt.heap.own_key_bound(object)?;
    rt.charge(bound)?;
    rt.heap
        .check_room(bound.saturating_mul(size_of::<PropertyKey>()))?;
    let mut keys = Vec::new();
    keys.try_reserve(bound).map_err(crate::error::Error::from)?;
    rt.heap.each_string_key(object, |key, enumerable| {
        if enumerable {
            keys.push(key);
        }
    })?;
    rt.heap.record_keys(&keys);
    Ok(Members::Keys(keys))
}

/// The record of element `index` of a container record.
fn element_record(records: &Records, parent: Option<u32>, index: usize) -> Option<u32> {
    let parent = parent?;
    if records.node(parent)?.kind != RecKind::Array {
        return None;
    }
    records.children(parent).get(index).copied()
}

/// The record of the member `key` of a container record: the last one
/// with that key (a later member replaces an earlier one).
fn member_record(records: &Records, parent: Option<u32>, key: PropertyKey) -> Option<u32> {
    let parent = parent?;
    if records.node(parent)?.kind != RecKind::Object {
        return None;
    }
    records
        .children(parent)
        .iter()
        .rev()
        .find(|&&id| records.node(id).is_some_and(|node| node.key == Some(key)))
        .copied()
}

/// An index of the member records by key (the last record of a key
/// wins), for an object record with more than [`LINEAR_MEMBERS`] members.
fn member_index(records: &Records, parent: Option<u32>) -> Option<HashMap<PropertyKey, u32>> {
    let parent = parent?;
    if records.node(parent)?.kind != RecKind::Object {
        return None;
    }
    let children = records.children(parent);
    if children.len() <= LINEAR_MEMBERS {
        return None;
    }
    let mut index = HashMap::with_capacity(children.len());
    for &id in children {
        if let Some(key) = records.node(id).and_then(|node| node.key) {
            index.insert(key, id);
        }
    }
    Some(index)
}

/// The most members whose record is found by a scan.
const LINEAR_MEMBERS: usize = 16;
