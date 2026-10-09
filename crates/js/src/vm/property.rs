//! Property access on values (ECMA-262 §6.2.5 `GetValue` and
//! `PutValue`). The global bindings are in `global.rs`, the texts of the
//! errors in `messages.rs`.
//!
//! Reads and writes return accessors to the caller instead of calling
//! them: the interpreter runs them as script-to-script calls (memo 1.2),
//! Rust code calls them through re-entry.
//!
//! Primitive bases: a string has its own `length` and index properties;
//! everything else comes from the prototype of the primitive's wrapper,
//! with the primitive as the receiver (no wrapper object is created).

use crate::heap::Gc;
use crate::object::{
    GetResult, INVALID_ARRAY_LENGTH, Object, ObjectKind, PropertyDescriptor, SetResult,
};
use crate::runtime::Runtime;
use crate::string::{JsString, PropertyKey};
use crate::value::Value;
use crate::vm::{Intrinsic, VmError, VmResult};

/// The result of a property read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Lookup {
    /// The value.
    Value(Value),
    /// An accessor: call `getter` with `this` = `receiver`.
    Getter { getter: Value, receiver: Value },
}

/// The result of a property write.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum SetOutcome {
    /// Done (or silently ignored in sloppy mode).
    Done,
    /// An accessor: call `setter` with `this` = `receiver` and the value.
    Setter { setter: Value, receiver: Value },
}

impl From<GetResult> for Lookup {
    fn from(result: GetResult) -> Self {
        match result {
            GetResult::Value(value) => Lookup::Value(value),
            GetResult::CallGetter { getter, receiver } => Lookup::Getter { getter, receiver },
        }
    }
}

impl Runtime {
    /// The realm of the running code.
    pub(crate) fn current_realm(&self) -> u32 {
        self.vm.frames.last().map_or(0, |f| f.realm)
    }

    // --- Property reads ---

    /// `base[key]` for any base value.
    pub(crate) fn get_property(&mut self, base: Value, key: PropertyKey) -> VmResult<Lookup> {
        match base {
            Value::Object(object) => {
                if let Some(value) = self.string_object_index(object, key)? {
                    return Ok(Lookup::Value(value));
                }
                Ok(self.heap.get(object, key, base)?.into())
            }
            _ => self.get_primitive_property(base, key),
        }
    }

    /// The index properties of a String object (§10.4.3.5
    /// `StringGetOwnProperty`), which the String object
    /// does not store: the code unit at `key` if the object is a String
    /// object and `key` an index below its length.
    pub(crate) fn string_object_index(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
    ) -> VmResult<Option<Value>> {
        let PropertyKey::Index(_) = key else {
            return Ok(None);
        };
        match self.heap.object(object)?.kind {
            ObjectKind::StringWrapper(string) => self.string_own_property(string, key),
            _ => Ok(None),
        }
    }

    /// `HasProperty` (§7.3.12) with the index properties of String
    /// objects.
    pub(crate) fn has_property(&mut self, object: Gc<Object>, key: PropertyKey) -> VmResult<bool> {
        if self.string_object_index(object, key)?.is_some() {
            return Ok(true);
        }
        Ok(self.heap.has_property(object, key)?)
    }

    /// `base[key]` for a primitive base (§6.2.5.5 `GetValue` with `ToObject`,
    /// without creating the wrapper).
    pub(crate) fn get_primitive_property(
        &mut self,
        base: Value,
        key: PropertyKey,
    ) -> VmResult<Lookup> {
        let realm = self.current_realm();
        let proto = match base {
            Value::Undefined | Value::Null => return Err(self.cannot_read(base, key)),
            Value::String(string) => {
                if let Some(value) = self.string_own_property(string, key)? {
                    return Ok(Lookup::Value(value));
                }
                Intrinsic::StringPrototype
            }
            Value::Int(_) | Value::Double(_) => Intrinsic::NumberPrototype,
            Value::Bool(_) => Intrinsic::BooleanPrototype,
            Value::Symbol(_) => Intrinsic::SymbolPrototype,
            Value::BigInt(_) => Intrinsic::ObjectPrototype,
            Value::Object(object) => return Ok(self.heap.get(object, key, base)?.into()),
            Value::Empty | Value::Cell(_) => {
                return Err(VmError::invariant("a property read of an internal value"));
            }
        };
        let proto = self.intrinsic(realm, proto)?;
        Ok(self.heap.get(proto, key, base)?.into())
    }

    /// The own `length` and index properties of a string value (§10.4.3.5
    /// `StringGetOwnProperty`).
    fn string_own_property(
        &mut self,
        string: Gc<JsString>,
        key: PropertyKey,
    ) -> VmResult<Option<Value>> {
        let text = self.heap.string(string)?.as_str16();
        match key {
            PropertyKey::Index(index) => {
                let Some(unit) = text.get(index as usize) else {
                    return Ok(None);
                };
                let one = swb_js_text::String16::from_units(&[unit]);
                Ok(Some(Value::String(self.heap.alloc_string(one)?)))
            }
            PropertyKey::String(name) if name == self.heap.length_atom() => {
                Ok(Some(Value::number(text.len() as f64)))
            }
            _ => Ok(None),
        }
    }

    /// A property read from Rust code: a getter runs through re-entry.
    /// The result is recorded in the current handle scope.
    pub(crate) fn get_value(&mut self, base: Value, key: PropertyKey) -> VmResult<Value> {
        match self.get_property(base, key)? {
            Lookup::Value(value) => {
                self.heap.record(value);
                Ok(value)
            }
            Lookup::Getter { getter, receiver } => self.call(getter, receiver, &[]),
        }
    }

    // --- Property writes ---

    /// `PutValue` for `base[key] = value` (§6.2.5.6, §10.1.9 `OrdinarySet`).
    /// A failed write throws in strict mode code.
    pub(crate) fn put_value(
        &mut self,
        base: Value,
        key: PropertyKey,
        value: Value,
        strict: bool,
    ) -> VmResult<SetOutcome> {
        let value = self.array_length_operand(base, key, value)?;
        let target = match base {
            Value::Object(object) => object,
            Value::Undefined | Value::Null => {
                return Err(self.cannot_set(base, key));
            }
            _ => {
                // The setter search starts at the wrapper prototype; the
                // receiver is the primitive, so a data property cannot be
                // created (OrdinarySet step 2.a).
                let realm = self.current_realm();
                let proto = match base {
                    Value::String(_) => Intrinsic::StringPrototype,
                    Value::Int(_) | Value::Double(_) => Intrinsic::NumberPrototype,
                    Value::Bool(_) => Intrinsic::BooleanPrototype,
                    Value::Symbol(_) => Intrinsic::SymbolPrototype,
                    _ => Intrinsic::ObjectPrototype,
                };
                let proto = self.intrinsic(realm, proto)?;
                let result = self.heap.set(proto, key, value, base, &self.vm)?;
                return match result {
                    SetResult::CallSetter { setter, receiver } => {
                        Ok(SetOutcome::Setter { setter, receiver })
                    }
                    SetResult::Done(_) if strict => Err(self.cannot_create(base, key)),
                    SetResult::Done(_) => Ok(SetOutcome::Done),
                };
            }
        };
        match self.heap.set(target, key, value, base, &self.vm)? {
            SetResult::CallSetter { setter, receiver } => {
                Ok(SetOutcome::Setter { setter, receiver })
            }
            SetResult::Done(false) if strict => Err(self.failed_write(target, key)),
            SetResult::Done(_) => Ok(SetOutcome::Done),
        }
    }

    /// The conversion of `ArraySetLength` (§10.4.2.4 steps 3 to 5) for a
    /// string or object written to the `length` of a writable array:
    /// `ToUint32(V)` and `ToNumber(V)` (two conversions, as the
    /// specification says), a `RangeError` if they differ. The heap's
    /// define takes only primitives, and the conversion can run script.
    /// Other writes pass unchanged.
    fn array_length_operand(
        &mut self,
        base: Value,
        key: PropertyKey,
        value: Value,
    ) -> VmResult<Value> {
        let (Value::Object(array), Value::String(_) | Value::Object(_)) = (base, value) else {
            return Ok(value);
        };
        if key != PropertyKey::String(self.heap.length_atom()) {
            return Ok(value);
        }
        // A read-only `length` rejects the write before any conversion.
        if !matches!(self.heap.array_length(array), Ok((_, true))) {
            return Ok(value);
        }
        self.with_scope(|rt| {
            let integer = rt.to_number(value)?;
            let number = rt.to_number(value)?;
            if f64::from(super::convert::to_uint32(integer)) != number {
                return Err(VmError::range_error(INVALID_ARRAY_LENGTH));
            }
            Ok(Value::number(number))
        })
    }

    /// `delete base[key]` (§13.5.1.2). A failure throws in strict mode
    /// code.
    pub(crate) fn delete_property(
        &mut self,
        base: Value,
        key: PropertyKey,
        strict: bool,
    ) -> VmResult<bool> {
        let deleted = match base {
            Value::Object(object) => {
                let deleted = self.heap.delete(object, key)?;
                if !deleted && strict {
                    return Err(VmError::type_error(format!(
                        "Cannot delete property '{}' of {}",
                        self.key_text(key),
                        self.object_text(object)
                    )));
                }
                deleted
            }
            Value::Undefined | Value::Null => {
                return Err(VmError::type_error(
                    "Cannot convert undefined or null to object",
                ));
            }
            Value::String(string) => {
                let deleted = self.string_own_property(string, key)?.is_none();
                if !deleted && strict {
                    return Err(VmError::type_error(format!(
                        "Cannot delete property '{}' of [object String]",
                        self.key_text(key)
                    )));
                }
                deleted
            }
            _ => true,
        };
        Ok(deleted)
    }

    /// `key in target` (§13.10.1).
    pub(crate) fn has_property_op(&mut self, key: Value, target: Value) -> VmResult<bool> {
        let Value::Object(object) = target else {
            let key_text = self.describe_value(key);
            let target_text = self.describe_value(target);
            return Err(VmError::type_error(format!(
                "Cannot use 'in' operator to search for '{key_text}' in {target_text}"
            )));
        };
        let key = self.to_property_key(key)?;
        self.has_property(object, key)
    }

    /// Appends a hole to an array literal: `length` + 1.
    pub(crate) fn append_hole(&mut self, array: Gc<Object>) -> VmResult<()> {
        let (length, writable) = self.heap.array_length(array)?;
        let new_length = length
            .checked_add(1)
            .ok_or(VmError::range_error(INVALID_ARRAY_LENGTH))?;
        self.heap
            .set_array_length_field(array, new_length, writable)?;
        Ok(())
    }

    /// Creates the (unmapped) arguments object of the running frame
    /// (§10.4.4.6 `CreateUnmappedArgumentsObject`; sloppy functions also get
    /// `callee` as a data property) and stores it in the stack slot
    /// `slot`. Called at function entry, before the parameters become
    /// cells. The mapped object (§10.4.4.7) comes in M7.
    pub(crate) fn create_arguments(&mut self, slot: usize) -> VmResult<()> {
        let frame = self
            .vm
            .frames
            .last()
            .ok_or(VmError::invariant("arguments without a frame"))?;
        let (base, argc, realm, strict, function) = (
            frame.base as usize,
            frame.argc as usize,
            frame.realm,
            frame.compiled.strict,
            frame.function,
        );
        let params = frame.compiled.param_count as usize;
        let proto = self.intrinsic(realm, Intrinsic::ObjectPrototype)?;
        let object = self
            .heap
            .new_object_with_kind(Some(proto), ObjectKind::Arguments)?;
        // The register roots the object while its properties are added.
        *self
            .vm
            .stack
            .get_mut(slot)
            .ok_or(VmError::invariant("a register outside the stack"))? = object.into();
        for index in 0..argc {
            let value = if index < params {
                self.stack_value(base + index)?
            } else {
                self.vm
                    .frames
                    .last()
                    .and_then(|f| f.extra_args.get(index - params))
                    .copied()
                    .unwrap_or_default()
            };
            let key = PropertyKey::Index(index as u32);
            self.heap
                .create_data_property(object, key, value, &self.vm)?;
        }
        let length = PropertyKey::String(self.heap.length_atom());
        self.define(
            object,
            length,
            Value::number(argc as f64),
            true,
            false,
            true,
        )?;
        let callee = PropertyKey::String(self.vm.atoms.callee);
        if strict {
            // §10.4.4.6 step 8: an accessor that throws.
            let thrower: Value = self.intrinsic(realm, Intrinsic::ThrowTypeError)?.into();
            let desc = PropertyDescriptor::accessor(thrower, thrower, false, false);
            self.heap
                .define_own_property(object, callee, desc, &self.vm)?;
        } else if let Some(function) = function {
            self.define(object, callee, function.into(), true, false, true)?;
        }
        Ok(())
    }
}
