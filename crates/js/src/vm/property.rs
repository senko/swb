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
    TypedArrayData, length_write_message,
};
use crate::runtime::Runtime;
use crate::string::{JsString, PropertyKey};
use crate::value::Value;
use crate::vm::proxy::SetDone;
use crate::vm::{Intrinsic, VmError, VmResult, WellKnown};

impl From<SetDone> for SetOutcome {
    fn from(done: SetDone) -> Self {
        match done {
            SetDone::Done(_) => SetOutcome::Done,
            SetDone::Setter { setter, receiver } => SetOutcome::Setter { setter, receiver },
        }
    }
}

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

/// The lookup of an ordinary heap walk. Callers take the generic path for
/// [`GetResult::Generic`] before they convert, so a proxy that reaches this
/// is an internal error, never an empty object.
fn ordinary_lookup(result: GetResult) -> VmResult<Lookup> {
    match result {
        GetResult::Value(value) => Ok(Lookup::Value(value)),
        GetResult::CallGetter { getter, receiver } => Ok(Lookup::Getter { getter, receiver }),
        GetResult::Generic => Err(VmError::invariant("an ordinary lookup that met a proxy")),
    }
}

impl Runtime {
    /// The realm of the running code.
    pub(crate) fn current_realm(&self) -> u32 {
        self.vm.frames.last().map_or(0, |f| f.realm)
    }

    /// `[[OwnPropertyKeys]]` for a built-in function: charges the time
    /// countdown for the keys, then lists them (the heap checks that the
    /// list fits under the limit before it allocates).
    pub(crate) fn own_keys(&mut self, object: Gc<Object>) -> VmResult<Vec<PropertyKey>> {
        if self.heap.is_proxy(object)? {
            return self.proxy_own_keys(object);
        }
        if let Some(hooks) = self.host_hooks(object)? {
            return self.host_own_keys(object, &hooks);
        }
        let bound = self.heap.own_key_bound(object)?;
        self.charge(bound)?;
        Ok(self.heap.own_property_keys(object)?)
    }

    // --- Property reads ---

    /// `base[key]` for any base value.
    pub(crate) fn get_property(&mut self, base: Value, key: PropertyKey) -> VmResult<Lookup> {
        match base {
            Value::Object(object) => self.get_with_receiver(object, key, base),
            _ => self.get_primitive_property(base, key),
        }
    }

    /// `[[Get]]` of an object with a receiver, including the index
    /// properties of String objects (§10.4.3.5), also those on the
    /// prototype chain. The heap does not know the characters (each is a
    /// new string), so an index that the heap does not find gets a second
    /// look along the chain. An array index of a typed array reads its
    /// element here (§10.4.5: no walk of the chain; the interpreter's
    /// `ta[i]`).
    pub(crate) fn get_with_receiver(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        receiver: Value,
    ) -> VmResult<Lookup> {
        if let PropertyKey::Index(index) = key {
            let kind = &self.heap.object(object)?.kind;
            if kind.sets_exotic() {
                match *kind {
                    ObjectKind::StringWrapper(string) => {
                        if let Some(value) = self.string_own_property(string, key)? {
                            return Ok(Lookup::Value(value));
                        }
                    }
                    ObjectKind::TypedArray(ref data) => {
                        let data = **data;
                        return self.typed_index_lookup(&data, index);
                    }
                    _ => {}
                }
            }
        }
        let found = self.heap.get(object, key, receiver)?;
        if found == GetResult::Generic {
            return self.get_generic_scoped(object, key, receiver);
        }
        if let (PropertyKey::Index(_), GetResult::Value(Value::Undefined)) = (key, found)
            // An own property with the value `undefined` ends the lookup.
            && self.heap.get_own_property(object, key)?.is_none()
        {
            let mut current = self.heap.get_prototype_of(object)?;
            while let Some(proto) = current {
                // A typed array ends the walk for an index (§10.4.5).
                if self.heap.typed_array(proto)?.is_some()
                    || self.heap.get_own_property(proto, key)?.is_some()
                {
                    break;
                }
                if let Some(value) = self.string_object_index(proto, key)? {
                    return Ok(Lookup::Value(value));
                }
                current = self.heap.get_prototype_of(proto)?;
            }
        }
        ordinary_lookup(found)
    }

    /// The element `index` of a typed array (`undefined` if the
    /// index is not valid), for [`Runtime::get_with_receiver`]: out of
    /// line, so that the paths of other objects stay short.
    #[inline(never)]
    fn typed_index_lookup(&mut self, data: &TypedArrayData, index: u32) -> VmResult<Lookup> {
        Ok(Lookup::Value(self.typed_element(data, index as usize)?))
    }

    /// The value of [`Runtime::get_generic`], with a getter called through
    /// re-entry, in a handle scope of its own (for the interpreter's fast
    /// path of named reads, which has none open).
    #[cold]
    #[inline(never)]
    pub(crate) fn get_generic_value(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        receiver: Value,
    ) -> VmResult<Value> {
        self.with_scope(|rt| match rt.get_generic(object, key, receiver)? {
            Lookup::Value(value) => Ok(value),
            Lookup::Getter { getter, receiver } => rt.call(getter, receiver, &[]),
        })
    }

    /// [`Runtime::delete_generic`] of a proxy or a host object with hooks
    /// in a handle scope of its own, out of line.
    #[cold]
    #[inline(never)]
    fn delete_generic_scoped(
        &mut self,
        proxy: Gc<Object>,
        key: PropertyKey,
        strict: bool,
    ) -> VmResult<bool> {
        self.with_scope(|rt| rt.delete_generic(proxy, key, strict))
    }

    /// [`Runtime::get_generic`] in a handle scope of its own (for the
    /// interpreter, which has none open).
    #[cold]
    #[inline(never)]
    pub(crate) fn get_generic_scoped(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        receiver: Value,
    ) -> VmResult<Lookup> {
        self.with_scope(|rt| rt.get_generic(object, key, receiver))
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

    /// `HasProperty` (§7.3.12); the heap knows the index properties of
    /// String objects, a chain with a proxy or a host object with hooks
    /// takes the generic path.
    pub(crate) fn has_property(&mut self, object: Gc<Object>, key: PropertyKey) -> VmResult<bool> {
        match self.heap.has_property_fast(object, key)? {
            Some(found) => Ok(found),
            None => self.has_generic_scoped(object, key),
        }
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
            Value::BigInt(_) => Intrinsic::BigIntPrototype,
            Value::Object(object) => return self.get_with_receiver(object, key, base),
            Value::Empty | Value::Cell(_) => {
                return Err(VmError::invariant("a property read of an internal value"));
            }
        };
        let proto = self.intrinsic(realm, proto)?;
        match self.heap.get(proto, key, base)? {
            GetResult::Generic => self.get_generic_scoped(proto, key, base),
            found => ordinary_lookup(found),
        }
    }

    /// The own `length` and index properties of a string value (§10.4.3.5
    /// `StringGetOwnProperty`). `length` reads a rope as it is; an index
    /// flattens it (without a collection: the base is not always rooted),
    /// once, so that a loop over the indices reads flat text.
    fn string_own_property(
        &mut self,
        string: Gc<JsString>,
        key: PropertyKey,
    ) -> VmResult<Option<Value>> {
        let len = self.heap.string(string)?.len();
        match key {
            PropertyKey::Index(index) => {
                if index as usize >= len {
                    return Ok(None);
                }
                self.flatten_no_gc(string)?;
                let Some(unit) = self.heap.text(string)?.get(index as usize) else {
                    return Ok(None);
                };
                Ok(Some(Value::String(self.heap.unit_string(unit)?)))
            }
            PropertyKey::String(name) if name == self.heap.length_atom() => {
                Ok(Some(Value::number(len as f64)))
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
                    Value::BigInt(_) => Intrinsic::BigIntPrototype,
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
                    SetResult::Generic => self.set_generic_scoped(proto, key, value, base, strict),
                };
            }
        };
        match self.heap.set(target, key, value, base, &self.vm)? {
            SetResult::CallSetter { setter, receiver } => {
                Ok(SetOutcome::Setter { setter, receiver })
            }
            SetResult::Done(false) if strict => Err(self.failed_write(target, key)),
            SetResult::Done(_) => Ok(SetOutcome::Done),
            SetResult::Generic => self.set_generic_scoped(target, key, value, base, strict),
        }
    }

    /// [`Runtime::set_generic`] in a handle scope of its own, out of line
    /// (the interpreter has no scope open).
    #[cold]
    #[inline(never)]
    fn set_generic_scoped(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        value: Value,
        receiver: Value,
        strict: bool,
    ) -> VmResult<SetOutcome> {
        self.with_scope(|rt| Ok(rt.set_generic(object, key, value, receiver, strict)?.into()))
    }

    /// [`Runtime::has_generic`] in a handle scope of its own, out of line.
    #[cold]
    #[inline(never)]
    fn has_generic_scoped(&mut self, object: Gc<Object>, key: PropertyKey) -> VmResult<bool> {
        self.with_scope(|rt| rt.has_generic(object, key))
    }

    /// `O.[[Set]](P, V, Receiver)` for any object: the heap's walk, or the
    /// generic path when the chain or the receiver has a proxy. With
    /// `throw`, a `false` result is a `TypeError` with V8's wording.
    pub(crate) fn set_with_receiver(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        value: Value,
        receiver: Value,
        throw: bool,
    ) -> VmResult<SetDone> {
        let value = self.array_length_operand(receiver, key, value)?;
        match self.heap.set(object, key, value, receiver, &self.vm)? {
            SetResult::CallSetter { setter, receiver } => Ok(SetDone::Setter { setter, receiver }),
            // The walk from `object` found the read-only property.
            SetResult::Done(false) if throw => match receiver {
                Value::Object(_) => Err(self.failed_write(object, key)),
                _ => Err(self.cannot_create(receiver, key)),
            },
            SetResult::Done(done) => Ok(SetDone::Done(done)),
            SetResult::Generic => self.set_generic(object, key, value, receiver, throw),
        }
    }

    /// The conversion of `ArraySetLength` (§10.4.2.4 steps 3 to 5) for a
    /// string or object written to the `length` of a writable array:
    /// `ToUint32(V)` and `ToNumber(V)` (two conversions, as the
    /// specification says), a `RangeError` if they differ. The heap's
    /// define takes only primitives, and the conversion can run script.
    /// Other writes pass unchanged.
    pub(crate) fn array_length_operand(
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
        self.convert_array_length(value)
            .map_err(|error| match error {
                VmError::Raise { kind, message } => match length_write_message(&message) {
                    Some(longer) => VmError::Raise {
                        kind,
                        message: longer.into(),
                    },
                    None => VmError::Raise { kind, message },
                },
                error => error,
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
            Value::Object(object) if self.heap.needs_runtime(object)? => {
                self.delete_generic_scoped(object, key, strict)?
            }
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
                return Err(VmError::type_error(crate::vm::messages::NULLISH_TO_OBJECT));
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

    /// Creates the arguments object of the running frame and stores it in
    /// the stack slot `slot`: unmapped (§10.4.4.6
    /// `CreateUnmappedArgumentsObject`; sloppy functions also get `callee`
    /// as a data property, as the mapped form has it), or `mapped`
    /// (§10.4.4.7 `CreateMappedArgumentsObject`). The unmapped object is
    /// made at function entry before the parameters become cells; the
    /// mapped one after, and it maps each index below the argument count
    /// whose parameter register holds a cell (an earlier position of a
    /// repeated name holds a plain value).
    ///
    /// After the first object of a realm, the objects are made in the
    /// template's shape with all elements at once (`object/arguments.rs`).
    #[inline(never)]
    pub(crate) fn create_arguments(&mut self, slot: usize, mapped: bool) -> VmResult<()> {
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
        // A sloppy object needs its callee: only strict objects and sloppy
        // ones of a function use or make a template.
        let templated = strict || function.is_some();
        if let Some(elements) = self.argument_elements(base, params, argc, mapped)? {
            let template = self
                .vm
                .realms
                .get(realm as usize)
                .and_then(|record| record.arguments[usize::from(strict)].as_ref());
            if let Some(template) = template
                && templated
            {
                let callee = function.map_or(Value::Undefined, Value::Object);
                let object = self.heap.new_arguments_object(template, callee, elements)?;
                *self
                    .vm
                    .stack
                    .get_mut(slot)
                    .ok_or(VmError::invariant("a register outside the stack"))? = object.into();
                return Ok(());
            }
        }
        let object = self.create_arguments_general(slot, mapped)?;
        let callee = PropertyKey::String(self.vm.atoms.callee);
        if templated
            && let Some(record) = self.vm.realms.get(realm as usize)
            && record.arguments[usize::from(strict)].is_none()
            && let Some(template) = self.heap.arguments_template(object, callee)?
            && let Some(record) = self.vm.realms.get_mut(realm as usize)
        {
            record.arguments[usize::from(strict)] = Some(template);
        }
        Ok(())
    }

    /// The elements of a new arguments object: the arguments, with the
    /// cell of each mapped parameter (`mapped`) or its value. `None` if an
    /// argument is a hole (`Empty`), which the dense vector cannot hold.
    fn argument_elements(
        &self,
        base: usize,
        params: usize,
        argc: usize,
        mapped: bool,
    ) -> VmResult<Option<Vec<Value>>> {
        let passed = self
            .vm
            .stack
            .get(base..base + argc.min(params))
            .ok_or(VmError::invariant("arguments outside the stack"))?;
        let extra = match self.vm.frames.last() {
            Some(frame) if argc > params => frame.extra_args.as_slice(),
            _ => &[],
        };
        let mut elements = Vec::with_capacity(argc);
        for &value in passed.iter().chain(extra) {
            elements.push(match value {
                Value::Empty => return Ok(None),
                Value::Cell(cell) if !mapped => self.heap.cell(cell)?.value,
                value => value,
            });
        }
        if elements.len() != argc {
            return Err(VmError::invariant("arguments without their values"));
        }
        Ok(Some(elements))
    }

    /// [`Runtime::create_arguments`] with the general definitions; returns
    /// the object.
    fn create_arguments_general(&mut self, slot: usize, mapped: bool) -> VmResult<Gc<Object>> {
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
            match value {
                // A mapped parameter (§10.4.4.7 step 17): the element holds
                // its cell. (An unmapped object is made before the
                // parameters become cells; it would take the value.)
                Value::Cell(cell) => {
                    let current = self.heap.cell(cell)?.value;
                    self.heap
                        .create_data_property(object, key, current, &self.vm)?;
                    if mapped {
                        self.heap.map_argument(object, index as u32, cell)?;
                    }
                }
                value => {
                    self.heap
                        .create_data_property(object, key, value, &self.vm)?;
                }
            }
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
        // §10.4.4.6 step 7: `@@iterator` is `%Array.prototype.values%`.
        let iterator = self.symbol_key(WellKnown::Iterator);
        let values = self.intrinsic(realm, Intrinsic::ArrayPrototypeValues)?;
        self.define(object, iterator, values.into(), true, false, true)?;
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
        Ok(object)
    }
}
