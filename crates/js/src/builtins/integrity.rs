//! Integrity levels (ECMA-262 §7.3.15 `SetIntegrityLevel`, §7.3.16
//! `TestIntegrityLevel`) and the functions of `Object` that use them:
//! `freeze`, `seal`, `isFrozen`, `isSealed`, `preventExtensions` and
//! `isExtensible`.
//!
//! The loops cost one step of the time countdown per own key. A frozen
//! array stores its elements in the sparse form, so the cost is bounded by
//! the number of stored elements, not by `length`.

use crate::heap::Gc;
use crate::object::{Object, Property, PropertyDescriptor};
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::{NativeCall, NativeReturn, VmError, VmResult};

/// The two levels.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    Sealed,
    Frozen,
}

impl Runtime {
    /// `SetIntegrityLevel` (§7.3.15) for the objects that exist: the
    /// `[[PreventExtensions]]` of these kinds cannot fail.
    fn set_integrity_level(&mut self, object: Gc<Object>, level: Level) -> VmResult<()> {
        self.heap.prevent_extensions(object)?;
        let keys = self.own_keys(object)?;
        self.heap.record_keys(&keys);
        for key in keys {
            self.tick()?;
            let desc = match level {
                Level::Sealed => PropertyDescriptor {
                    configurable: Some(false),
                    ..PropertyDescriptor::default()
                },
                Level::Frozen => {
                    let Some(current) = self.heap.get_own_property(object, key)? else {
                        // A character of a String object is already fixed.
                        continue;
                    };
                    let mut desc = PropertyDescriptor {
                        configurable: Some(false),
                        ..PropertyDescriptor::default()
                    };
                    if !current.is_accessor() {
                        desc.writable = Some(false);
                    }
                    desc
                }
            };
            self.define_or_throw(object, key, desc)?;
        }
        Ok(())
    }

    /// `TestIntegrityLevel` (§7.3.16).
    fn test_integrity_level(&mut self, object: Gc<Object>, level: Level) -> VmResult<bool> {
        if self.heap.is_extensible(object)? {
            return Ok(false);
        }
        let keys = self.own_keys(object)?;
        for key in keys {
            self.tick()?;
            let Some(property) = self.heap.get_own_property(object, key)? else {
                continue;
            };
            let writable = matches!(property, Property::Data { writable: true, .. });
            if property.configurable() || (level == Level::Frozen && writable) {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// `Object.freeze(O)` (§20.1.2.6).
pub(super) fn freeze(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    set_level(rt, call, Level::Frozen)
}

/// `Object.seal(O)` (§20.1.2.21).
pub(super) fn seal(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    set_level(rt, call, Level::Sealed)
}

fn set_level(rt: &mut Runtime, call: &NativeCall, level: Level) -> VmResult<NativeReturn> {
    let value = rt.arg(call, 0);
    if let Value::Object(object) = value {
        rt.set_integrity_level(object, level)?;
    }
    Ok(NativeReturn::Value(value))
}

/// `Object.isFrozen(O)` (§20.1.2.15).
pub(super) fn is_frozen(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    test_level(rt, call, Level::Frozen)
}

/// `Object.isSealed(O)` (§20.1.2.17).
pub(super) fn is_sealed(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    test_level(rt, call, Level::Sealed)
}

fn test_level(rt: &mut Runtime, call: &NativeCall, level: Level) -> VmResult<NativeReturn> {
    let result = match rt.arg(call, 0) {
        Value::Object(object) => rt.test_integrity_level(object, level)?,
        _ => true,
    };
    Ok(NativeReturn::Value(Value::Bool(result)))
}

/// `Object.preventExtensions(O)` (§20.1.2.20).
pub(super) fn prevent_extensions(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let value = rt.arg(call, 0);
    if let Value::Object(object) = value
        && !rt.heap.prevent_extensions(object)?
    {
        return Err(VmError::type_error("Cannot prevent extensions"));
    }
    Ok(NativeReturn::Value(value))
}

/// `Object.isExtensible(O)` (§20.1.2.16).
pub(super) fn is_extensible(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let result = match rt.arg(call, 0) {
        Value::Object(object) => rt.heap.is_extensible(object)?,
        _ => false,
    };
    Ok(NativeReturn::Value(Value::Bool(result)))
}
