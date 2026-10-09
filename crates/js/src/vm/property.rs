//! Property access on values and the global bindings (ECMA-262 §6.2.5
//! `GetValue` and `PutValue`, §9.1.1.4 the global environment record,
//! §16.1.7 `GlobalDeclarationInstantiation`).
//!
//! Reads and writes return accessors to the caller instead of calling
//! them: the interpreter runs them as script-to-script calls (memo 1.2),
//! Rust code calls them through re-entry.
//!
//! Primitive bases: a string has its own `length` and index properties;
//! everything else comes from the prototype of the primitive's wrapper,
//! with the primitive as the receiver (no wrapper object is created).

use std::borrow::Cow;

use crate::bytecode::{FunctionCode, GlobalKind};
use crate::heap::Gc;
use crate::object::{GetResult, Object, ObjectKind, Property, PropertyDescriptor, SetResult};
use crate::runtime::Runtime;
use crate::string::{JsString, PropertyKey};
use crate::value::Value;
use crate::vm::realm::LexicalBinding;
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
            Value::Object(object) => Ok(self.heap.get(object, key, base)?.into()),
            _ => self.get_primitive_property(base, key),
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

    /// The `TypeError` of a read from `undefined` or `null`.
    pub(crate) fn cannot_read(&self, base: Value, key: PropertyKey) -> VmError {
        let what = if base == Value::Null {
            "null"
        } else {
            "undefined"
        };
        VmError::type_error(format!(
            "Cannot read properties of {what} (reading '{}')",
            self.key_text(key)
        ))
    }

    /// The `TypeError` of a write to `undefined` or `null`.
    pub(crate) fn cannot_set(&self, base: Value, key: PropertyKey) -> VmError {
        let what = if base == Value::Null {
            "null"
        } else {
            "undefined"
        };
        VmError::type_error(format!(
            "Cannot set properties of {what} (setting '{}')",
            self.key_text(key)
        ))
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
            let integer = rt.to_numeric(value)?;
            let number = rt.to_numeric(value)?;
            if f64::from(super::convert::to_uint32(integer)) != number {
                return Err(VmError::range_error("Invalid array length"));
            }
            Ok(Value::number(number))
        })
    }

    /// The `TypeError` of a strict write to a primitive.
    fn cannot_create(&self, base: Value, key: PropertyKey) -> VmError {
        if let Value::String(string) = base {
            // An own property of the string (an index below its length, or
            // `length`) is read-only instead of missing.
            let own = self.heap.string(string).is_ok_and(|text| match key {
                PropertyKey::Index(index) => (index as usize) < text.len(),
                PropertyKey::String(name) => name == self.heap.length_atom(),
                PropertyKey::Symbol(_) => false,
            });
            if own {
                return VmError::type_error(format!(
                    "Cannot assign to read only property '{}' of string '{}'",
                    self.key_text(key),
                    self.name_text(string)
                ));
            }
        }
        let (kind, text) = match base {
            Value::String(string) => ("string", self.name_text(string)),
            Value::Int(_) | Value::Double(_) => ("number", self.primitive_text(base)),
            Value::Bool(_) => ("boolean", self.primitive_text(base)),
            _ => ("symbol", "Symbol()".to_owned()),
        };
        VmError::type_error(format!(
            "Cannot create property '{}' on {kind} '{text}'",
            self.key_text(key)
        ))
    }

    /// The `TypeError` of a failed strict write to an object: a read-only
    /// property on the chain, or a non-extensible object.
    fn failed_write(&self, target: Gc<Object>, key: PropertyKey) -> VmError {
        let mut current = Some(target);
        while let Some(object) = current {
            match self.heap.get_own_property(object, key) {
                Ok(Some(Property::Accessor { .. })) => {
                    return VmError::type_error(format!(
                        "Cannot set property {} of {} which has only a getter",
                        self.key_text(key),
                        self.object_text(target)
                    ));
                }
                Ok(Some(_)) => {
                    let kind = if self.is_function(target) {
                        "function"
                    } else {
                        "object"
                    };
                    return VmError::type_error(format!(
                        "Cannot assign to read only property '{}' of {kind} '{}'",
                        self.key_text(key),
                        self.object_text(target)
                    ));
                }
                _ => {}
            }
            current = self.heap.get_prototype_of(object).ok().flatten();
        }
        VmError::type_error(format!(
            "Cannot add property {}, object is not extensible",
            self.key_text(key)
        ))
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
        Ok(self.heap.has_property(object, key)?)
    }

    /// Appends a hole to an array literal: `length` + 1.
    pub(crate) fn append_hole(&mut self, array: Gc<Object>) -> VmResult<()> {
        let (length, _) = self.heap.array_length(array)?;
        let new_length = length
            .checked_add(1)
            .ok_or(VmError::range_error("Invalid array length"))?;
        let key = PropertyKey::String(self.heap.length_atom());
        let desc = PropertyDescriptor::value(Value::number(f64::from(new_length)));
        self.heap.define_own_property(array, key, desc, &self.vm)?;
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
        let object = self.heap.new_object(Some(proto))?;
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
        if !strict && let Some(function) = function {
            // Strict functions get a `callee` accessor that throws
            // (%ThrowTypeError%, §10.2.4); it comes with the built-ins.
            let callee = PropertyKey::String(self.vm.atoms.callee);
            self.define(object, callee, function.into(), true, false, true)?;
        }
        Ok(())
    }

    // --- Globals (§9.1.1.4) ---

    /// Looks up a global name: the declarative record, then the global
    /// object. `None` if neither has it.
    pub(crate) fn get_global(&mut self, name: Gc<JsString>) -> VmResult<Option<Lookup>> {
        let realm = self.current_realm();
        let record = self
            .vm
            .realms
            .get(realm as usize)
            .ok_or(VmError::invariant("no realm"))?;
        if !record.lexical.is_empty()
            && let Some(binding) = record.lexical.get(&name)
        {
            if binding.value.is_empty() {
                return Err(self.tdz_error(name));
            }
            return Ok(Some(Lookup::Value(binding.value)));
        }
        let global = record.global;
        let key = PropertyKey::String(name);
        // HasBinding and GetBindingValue of the object record (§9.1.1.2):
        // one walk that finds the property.
        let mut current = Some(global);
        while let Some(object) = current {
            if let Some(property) = self.heap.get_own_property(object, key)? {
                return Ok(Some(match property {
                    Property::Data { value, .. } => Lookup::Value(value),
                    Property::Accessor { get, .. } if get.is_undefined() => {
                        Lookup::Value(Value::Undefined)
                    }
                    Property::Accessor { get, .. } => Lookup::Getter {
                        getter: get,
                        receiver: global.into(),
                    },
                }));
            }
            current = self.heap.get_prototype_of(object)?;
        }
        Ok(None)
    }

    /// `PutValue` on a global name (§9.1.1.4.5 `SetMutableBinding`; an
    /// unresolvable name in sloppy mode creates a global property).
    pub(crate) fn set_global(
        &mut self,
        name: Gc<JsString>,
        value: Value,
        strict: bool,
    ) -> VmResult<SetOutcome> {
        let realm = self.current_realm();
        let record = self
            .vm
            .realms
            .get_mut(realm as usize)
            .ok_or(VmError::invariant("no realm"))?;
        if let Some(binding) = record.lexical.get_mut(&name) {
            if binding.value.is_empty() {
                return Err(self.tdz_error(name));
            }
            if !binding.mutable {
                return Err(VmError::type_error("Assignment to constant variable."));
            }
            binding.value = value;
            return Ok(SetOutcome::Done);
        }
        let global = record.global;
        let key = PropertyKey::String(name);
        if strict && !self.heap.has_property(global, key)? {
            let text = self.name_text(name);
            return Err(VmError::reference_error(format!("{text} is not defined")));
        }
        self.put_value(global.into(), key, value, strict)
    }

    /// The `ReferenceError` of a binding in its temporal dead zone.
    pub(crate) fn tdz_error(&self, name: Gc<JsString>) -> VmError {
        VmError::reference_error(format!(
            "Cannot access '{}' before initialization",
            self.name_text(name)
        ))
    }

    /// Initializes a global `let` or `const` binding at its declaration.
    pub(crate) fn init_global_lexical(&mut self, name: Gc<JsString>, value: Value) -> VmResult<()> {
        let realm = self.current_realm();
        let binding = self
            .vm
            .realms
            .get_mut(realm as usize)
            .and_then(|r| r.lexical.get_mut(&name))
            .ok_or(VmError::invariant(
                "a global lexical binding was not declared",
            ))?;
        binding.value = value;
        Ok(())
    }

    /// `CreateGlobalFunctionBinding` (§9.1.1.4.18).
    pub(crate) fn init_global_function(
        &mut self,
        name: Gc<JsString>,
        value: Value,
    ) -> VmResult<()> {
        let global = self.global_object(self.current_realm())?;
        let key = PropertyKey::String(name);
        let existing = self.heap.get_own_property(global, key)?;
        let desc = match existing {
            None => PropertyDescriptor::data(value, true, true, false),
            Some(property) if property.configurable() => {
                PropertyDescriptor::data(value, true, true, false)
            }
            Some(_) => PropertyDescriptor::value(value),
        };
        if !self.heap.define_own_property(global, key, desc, &self.vm)? {
            return Err(VmError::type_error(format!(
                "Cannot redefine property: {}",
                self.name_text(name)
            )));
        }
        // Step 6: Set(globalObject, N, V, false).
        let _ = self.heap.set(global, key, value, global.into(), &self.vm)?;
        Ok(())
    }

    /// `delete name` for a global name (§9.1.1.4.7 `DeleteBinding`).
    pub(crate) fn delete_global(&mut self, name: Gc<JsString>) -> VmResult<bool> {
        let realm = self.current_realm();
        let record = self
            .vm
            .realms
            .get(realm as usize)
            .ok_or(VmError::invariant("no realm"))?;
        if record.lexical.contains_key(&name) {
            return Ok(false);
        }
        let global = record.global;
        Ok(self.heap.delete(global, PropertyKey::String(name))?)
    }

    /// `GlobalDeclarationInstantiation` (§16.1.7) for the declarations of a
    /// script: the redeclaration checks, the lexical bindings in their
    /// dead zone, and the `var` properties. Function values are set by
    /// `InitGlobalFunction` after their closures are created.
    pub(crate) fn declare_globals(&mut self, code: &FunctionCode) -> VmResult<()> {
        let realm = self.current_realm();
        let global = self.global_object(realm)?;
        let names: Vec<(Gc<JsString>, GlobalKind)> = code
            .globals
            .iter()
            .map(|g| {
                code.string(g.name)
                    .map(|name| (name, g.kind))
                    .ok_or(VmError::invariant("a global name that is not a string"))
            })
            .collect::<VmResult<_>>()?;
        // Steps 3 to 6: conflicts with existing bindings.
        for &(name, kind) in &names {
            let lexical_exists = self
                .vm
                .realms
                .get(realm as usize)
                .is_some_and(|r| r.lexical.contains_key(&name));
            let redeclared = match kind {
                GlobalKind::Let | GlobalKind::Const => {
                    lexical_exists
                        || self
                            .heap
                            .get_own_property(global, PropertyKey::String(name))?
                            .is_some_and(|p| !p.configurable())
                }
                GlobalKind::Var | GlobalKind::Function => lexical_exists,
            };
            if redeclared {
                return Err(VmError::Raise {
                    kind: crate::error::ThrowKind::SyntaxError,
                    message: Cow::Owned(format!(
                        "Identifier '{}' has already been declared",
                        self.name_text(name)
                    )),
                });
            }
        }
        // Steps 11 and 12 (§9.1.1.4.15 `CanDeclareGlobalVar`, §9.1.1.4.16
        // `CanDeclareGlobalFunction`): every check runs before any binding
        // is created. V8 reports a function that cannot be declared as a
        // `SyntaxError` with the message of a redeclaration.
        for &(name, kind) in &names {
            let key = PropertyKey::String(name);
            let existing = self.heap.get_own_property(global, key)?;
            match kind {
                GlobalKind::Function => {
                    let allowed = match existing {
                        None => self.heap.is_extensible(global)?,
                        Some(property) if property.configurable() => true,
                        Some(Property::Data {
                            writable,
                            enumerable,
                            ..
                        }) => writable && enumerable,
                        Some(Property::Accessor { .. }) => false,
                    };
                    if !allowed {
                        return Err(VmError::Raise {
                            kind: crate::error::ThrowKind::SyntaxError,
                            message: Cow::Owned(format!(
                                "Identifier '{}' has already been declared",
                                self.name_text(name)
                            )),
                        });
                    }
                }
                GlobalKind::Var => {
                    if existing.is_none() && !self.heap.is_extensible(global)? {
                        return Err(VmError::type_error(format!(
                            "Cannot define global variable '{}', the global object is not extensible",
                            self.name_text(name)
                        )));
                    }
                }
                GlobalKind::Let | GlobalKind::Const => {}
            }
        }
        // Steps 15 and 17: lexical bindings, then `var` bindings
        // (§9.1.1.4.17 CreateGlobalVarBinding).
        for &(name, kind) in &names {
            match kind {
                GlobalKind::Let | GlobalKind::Const => {
                    if let Some(record) = self.vm.realms.get_mut(realm as usize) {
                        record.lexical.insert(
                            name,
                            LexicalBinding {
                                value: Value::Empty,
                                mutable: kind == GlobalKind::Let,
                            },
                        );
                    }
                }
                GlobalKind::Var => {
                    let key = PropertyKey::String(name);
                    if self.heap.get_own_property(global, key)?.is_none()
                        && self.heap.is_extensible(global)?
                    {
                        let desc = PropertyDescriptor::data(Value::Undefined, true, true, false);
                        self.heap.define_own_property(global, key, desc, &self.vm)?;
                    }
                }
                GlobalKind::Function => {}
            }
        }
        Ok(())
    }

    // --- Texts for messages (no script code runs) ---

    /// The text of a property key.
    pub(crate) fn key_text(&self, key: PropertyKey) -> String {
        match key {
            PropertyKey::Index(index) => index.to_string(),
            PropertyKey::String(name) => self.name_text(name),
            PropertyKey::Symbol(symbol) => {
                let description = self
                    .heap
                    .symbol(symbol)
                    .ok()
                    .and_then(|s| s.description)
                    .map(|d| self.name_text(d))
                    .unwrap_or_default();
                format!("Symbol({description})")
            }
        }
    }

    /// The text of an object in messages (V8 shows the constructor name;
    /// the global object as Node.js shows it).
    pub(crate) fn object_text(&self, object: Gc<Object>) -> String {
        if self.vm.realms.iter().any(|realm| realm.global == object) {
            return "[object Object]".to_owned();
        }
        match self.heap.object(object).map(|o| &o.kind) {
            Ok(ObjectKind::Array { .. }) => "[object Array]".to_owned(),
            Ok(ObjectKind::Function(closure)) => closure
                .compiled
                .source_text()
                .map_or_else(|| "#<Object>".to_owned(), |text| shorten_source(&text)),
            _ => "#<Object>".to_owned(),
        }
    }

    /// Whether the object is a function, for the "of function '...'" form
    /// of messages.
    fn is_function(&self, object: Gc<Object>) -> bool {
        matches!(
            self.heap.object(object).map(|o| &o.kind),
            Ok(ObjectKind::Function(_) | ObjectKind::Native(_))
        )
    }

    /// The text of a primitive value without running script code.
    pub(crate) fn primitive_text(&self, value: Value) -> String {
        match value {
            Value::Undefined => "undefined".to_owned(),
            Value::Null => "null".to_owned(),
            Value::Bool(b) => b.to_string(),
            Value::Int(i) => i.to_string(),
            Value::Double(d) => super::number::number_to_string(d),
            Value::String(s) => self.name_text(s),
            Value::Symbol(s) => self.key_text(PropertyKey::Symbol(s)),
            Value::BigInt(_) => "BigInt".to_owned(),
            Value::Object(o) => self.object_text(o),
            Value::Empty | Value::Cell(_) => "(internal)".to_owned(),
        }
    }

    /// A short description of a value for messages.
    pub(crate) fn describe_value(&self, value: Value) -> String {
        match value {
            Value::Object(object) => match self.heap.object(object).map(|o| &o.kind) {
                Ok(ObjectKind::Function(_) | ObjectKind::Native(_)) => "function".to_owned(),
                _ => self.object_text(object),
            },
            _ => self.primitive_text(value),
        }
    }
}

/// The source of a function as V8 shows it in messages: at most 128 code
/// units; a longer text keeps its first 111 units, "...<omitted>..." and
/// its last 2 (measured in Node.js 22).
fn shorten_source(text: &swb_js_text::String16) -> String {
    const LIMIT: usize = 128;
    const TAIL: usize = 2;
    const OMITTED: &str = "...<omitted>...";
    let text = text.as_str16();
    if text.len() <= LIMIT {
        return text.to_string_lossy();
    }
    let head = LIMIT - OMITTED.len() - TAIL;
    let part = |start, end| {
        text.slice(start, end)
            .map(swb_js_text::Str16::to_string_lossy)
            .unwrap_or_default()
    };
    format!(
        "{}{OMITTED}{}",
        part(0, head),
        part(text.len() - TAIL, text.len())
    )
}
