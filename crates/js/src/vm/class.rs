//! The run-time parts of classes (ECMA-262 §15.7) and `super` (§13.3.7):
//! the heritage, the creation of a class's constructor and prototype,
//! method and field definitions, the internal slots of methods and
//! classes ([`crate::vm::function::ClassSlots`]), and `super` property
//! access with `this` as the receiver.
//!
//! The compiler emits one instruction per step of
//! `ClassDefinitionEvaluation` (§15.7.14); the instance fields of a class
//! are defined by one engine-made function per class (its `[[Fields]]`),
//! which `super(...)` or the base constructor calls as an ordinary frame.

use std::rc::Rc;

use crate::bytecode::FunctionCode;
use crate::heap::{Gc, Generic};
use crate::object::{Object, ObjectKind, PropertyDescriptor};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::{Value, ValueCell};
use crate::vm::function::{ClassSlots, ClosureParts};
use crate::vm::{Intrinsic, Lookup, SetOutcome, VmError, VmResult};

/// V8's message for `this` (or a return) before `super(...)` in a derived
/// constructor.
pub(crate) const THIS_BEFORE_SUPER: &str = "Must call super constructor in derived class before accessing 'this' or returning from derived constructor";

/// V8's message for a second `super(...)` call.
pub(crate) const SUPER_TWICE: &str = "Super constructor may only be called once";

/// V8's message for a derived constructor that returns a primitive.
pub(crate) const DERIVED_RESULT: &str = "Derived constructors may only return object or undefined";

/// V8's message for `delete super.x`.
pub(crate) const SUPER_DELETE: &str = "Unsupported reference to 'super'";

/// The kinds of class members that [`Runtime::define_class_member`]
/// defines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MemberKind {
    Method,
    Getter,
    Setter,
}

impl Runtime {
    /// The parents of a class with heritage (§15.7.14 step 8): the parent
    /// of the prototype (an object or `null`) and the parent of the
    /// constructor. Reads `prototype` with `[[Get]]` (a getter runs through
    /// re-entry). Runs inside a handle scope.
    pub(crate) fn class_heritage(&mut self, superclass: Value) -> VmResult<(Value, Value)> {
        if superclass == Value::Null {
            let realm = self.current_realm();
            let function_proto = self.intrinsic(realm, Intrinsic::FunctionPrototype)?;
            return Ok((Value::Null, function_proto.into()));
        }
        if !self.is_constructor(superclass)? {
            return Err(VmError::type_error(format!(
                "Class extends value {} is not a constructor or null",
                self.primitive_text(superclass)
            )));
        }
        let key = PropertyKey::String(self.vm.atoms.prototype);
        let proto_parent = self.get_value(superclass, key)?;
        match proto_parent {
            Value::Object(_) | Value::Null => Ok((proto_parent, superclass)),
            _ => Err(VmError::type_error(format!(
                "Class extends value does not have valid prototype property {}",
                self.primitive_text(proto_parent)
            ))),
        }
    }

    /// Creates the constructor `F` and the prototype object of a class
    /// (§15.7.14 steps 9 to 17): `parents` are the values of
    /// [`Runtime::class_heritage`], or `Empty` and anything for a class
    /// without heritage. `F` is a closure of `compiled` whose home object
    /// is the prototype; `F.prototype` is not writable, not enumerable
    /// and not configurable, `prototype.constructor` is `F` (writable,
    /// configurable). Both are recorded in the current handle scope.
    pub(crate) fn new_class(
        &mut self,
        parents: (Value, Value),
        (code, compiled): (Gc<Generic>, &Rc<FunctionCode>),
        captures: Box<[Gc<ValueCell>]>,
        realm: u32,
    ) -> VmResult<(Gc<Object>, Gc<Object>)> {
        let (proto_parent, function_proto) = match parents {
            (Value::Empty, _) => (
                Some(self.intrinsic(realm, Intrinsic::ObjectPrototype)?),
                self.intrinsic(realm, Intrinsic::FunctionPrototype)?,
            ),
            (Value::Null, Value::Object(constructor_parent)) => (None, constructor_parent),
            (Value::Object(proto), Value::Object(constructor_parent)) => {
                (Some(proto), constructor_parent)
            }
            _ => return Err(VmError::invariant("class parents of the wrong kinds")),
        };
        let prototype = self.heap.new_object(proto_parent)?;
        self.heap.record(prototype);
        let parts = ClosureParts {
            function_proto: Some(function_proto),
            home: Some(prototype),
        };
        let function = self.new_closure_with(code, compiled, captures, realm, parts)?;
        let key = PropertyKey::String(self.vm.atoms.prototype);
        self.define(function, key, prototype.into(), false, false, false)?;
        let key = PropertyKey::String(self.vm.atoms.constructor);
        self.define(prototype, key, function.into(), true, false, true)?;
        Ok((function, prototype))
    }

    /// Defines a method, getter or setter of a class on `object` (the
    /// prototype or the constructor) under the key in `key` (a value of
    /// `ToKey` or a string): not enumerable, configurable (§10.2.8
    /// `DefineMethodProperty`, §15.4.5). Runs inside a handle scope.
    pub(crate) fn define_class_member(
        &mut self,
        object: Value,
        key: Value,
        function: Value,
        kind: MemberKind,
    ) -> VmResult<()> {
        let Value::Object(object) = object else {
            return Err(VmError::invariant(
                "a class member on a value that is not an object",
            ));
        };
        let key = self.key_from_converted(key)?;
        let desc = match kind {
            MemberKind::Method => PropertyDescriptor::data(function, true, false, true),
            MemberKind::Getter => PropertyDescriptor {
                get: Some(function),
                enumerable: Some(false),
                configurable: Some(true),
                ..PropertyDescriptor::default()
            },
            MemberKind::Setter => PropertyDescriptor {
                set: Some(function),
                enumerable: Some(false),
                configurable: Some(true),
                ..PropertyDescriptor::default()
            },
        };
        if self.heap.define_own_property(object, key, desc, &self.vm)? {
            return Ok(());
        }
        Err(self.class_define_failure(object, key))
    }

    /// `DefineField` (§7.3.33) of a public field: `CreateDataPropertyOrThrow`
    /// with V8's messages.
    pub(crate) fn define_field(
        &mut self,
        object: Value,
        key: PropertyKey,
        value: Value,
    ) -> VmResult<()> {
        let Value::Object(object) = object else {
            return Err(VmError::invariant(
                "a field of a value that is not an object",
            ));
        };
        if self
            .heap
            .create_data_property(object, key, value, &self.vm)?
        {
            return Ok(());
        }
        Err(self.class_define_failure(object, key))
    }

    /// The `TypeError` of a refused member or field definition: V8 words
    /// a static `prototype` of a class (the only property of a class
    /// constructor that cannot be redefined) in its own way.
    fn class_define_failure(&mut self, object: Gc<Object>, key: PropertyKey) -> VmError {
        if key == PropertyKey::String(self.vm.atoms.prototype) && self.is_class_constructor(object)
        {
            return VmError::type_error("Classes may not have a static property named 'prototype'");
        }
        self.define_failure(object, key)
    }

    /// Whether `object` is a class constructor.
    fn is_class_constructor(&self, object: Gc<Object>) -> bool {
        matches!(
            self.heap.object(object).map(|o| &o.kind),
            Ok(ObjectKind::Function(closure)) if closure.compiled.kind.is_class_constructor()
        )
    }

    /// The class slots of a function value, if it has them.
    pub(crate) fn class_slots(&self, function: Value) -> VmResult<Option<&ClassSlots>> {
        let Value::Object(function) = function else {
            return Ok(None);
        };
        Ok(match &self.heap.object(function)?.kind {
            ObjectKind::Function(closure) => closure.class.as_deref(),
            _ => None,
        })
    }

    /// The class slots of a closure for writing.
    fn class_slots_mut(&mut self, function: Value) -> VmResult<&mut ClassSlots> {
        let Value::Object(function) = function else {
            return Err(VmError::invariant(
                "class slots of a value that is not a function",
            ));
        };
        match &mut self.heap.object_mut(function)?.kind {
            ObjectKind::Function(closure) => closure
                .class
                .as_deref_mut()
                .ok_or(VmError::invariant("a function without class slots")),
            _ => Err(VmError::invariant(
                "class slots of a value that is not a closure",
            )),
        }
    }

    /// The function of the running frame.
    fn running_function(&self) -> Value {
        self.vm
            .frames
            .last()
            .and_then(|frame| frame.function)
            .map_or(Value::Undefined, Value::Object)
    }

    /// `[[HomeObject]]` of the running function (`undefined` if it has
    /// none).
    pub(crate) fn home_object(&self) -> VmResult<Value> {
        let home = self
            .class_slots(self.running_function())?
            .and_then(|slots| slots.home);
        Ok(home.map_or(Value::Undefined, Value::Object))
    }

    /// Computed field key `index` of the running initializer function.
    pub(crate) fn field_key(&self, index: u32) -> VmResult<Value> {
        let keys = self
            .class_slots(self.running_function())?
            .and_then(|slots| slots.field_keys)
            .ok_or(VmError::invariant("a field key without the keys"))?;
        match self
            .heap
            .get_own_property(keys, PropertyKey::Index(index))?
        {
            Some(crate::object::Property::Data { value, .. }) => Ok(value),
            _ => Err(VmError::invariant("a field key outside the keys")),
        }
    }

    /// Sets the computed field keys of an initializer function.
    pub(crate) fn set_field_keys(&mut self, function: Value, keys: Value) -> VmResult<()> {
        let Value::Object(keys) = keys else {
            return Err(VmError::invariant("field keys that are not an array"));
        };
        self.class_slots_mut(function)?.field_keys = Some(keys);
        Ok(())
    }

    /// Sets `[[Fields]]` of a class constructor.
    pub(crate) fn set_class_fields(&mut self, function: Value, init: Value) -> VmResult<()> {
        let Value::Object(init) = init else {
            return Err(VmError::invariant(
                "a field initializer that is not a function",
            ));
        };
        self.class_slots_mut(function)?.fields = Some(init);
        Ok(())
    }

    /// Sets the private environment of a class constructor or field
    /// initializer function.
    pub(crate) fn set_private_env(&mut self, function: Value, env: Value) -> VmResult<()> {
        let Value::Object(env) = env else {
            return Err(VmError::invariant(
                "a private environment that is not an object",
            ));
        };
        self.class_slots_mut(function)?.private = Some(env);
        Ok(())
    }

    /// The private environment of a class constructor or field initializer
    /// function, if it has one.
    pub(crate) fn private_env_of(&self, function: Value) -> VmResult<Option<Gc<Object>>> {
        Ok(self.class_slots(function)?.and_then(|slots| slots.private))
    }

    /// The private environment of the running field initializer function.
    pub(crate) fn running_private_env(&self) -> VmResult<Value> {
        self.private_env_of(self.running_function())?
            .map(Value::Object)
            .ok_or(VmError::invariant(
                "a field initializer without its private environment",
            ))
    }

    /// The instance field initializer of a class constructor, if any.
    pub(crate) fn fields_initializer(&self, function: Value) -> VmResult<Option<Gc<Object>>> {
        Ok(self.class_slots(function)?.and_then(|slots| slots.fields))
    }

    /// `[[GetPrototypeOf]]` of an object value as a value (`null` when
    /// there is none).
    pub(crate) fn prototype_value(&self, value: Value) -> VmResult<Value> {
        let Value::Object(object) = value else {
            return Err(VmError::invariant(
                "the prototype of a value that is not an object",
            ));
        };
        Ok(self
            .heap
            .get_prototype_of(object)?
            .map_or(Value::Null, Value::Object))
    }

    /// The `TypeError` of `super(...)` when the parent is not a
    /// constructor (V8's wording: the parent's name, `null` for one
    /// without a name, and the class's name).
    pub(crate) fn super_not_constructor(&self, parent: Value, active: Value) -> VmError {
        let parent = match parent {
            Value::Object(object) if self.is_callable(parent).unwrap_or(false) => self
                .own_data_string(object, self.vm.atoms.name)
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| "null".to_owned()),
            _ => self.primitive_text(parent),
        };
        let class = match active {
            Value::Object(object) => self
                .own_data_string(object, self.vm.atoms.name)
                .filter(|name| !name.is_empty()),
            _ => None,
        }
        .unwrap_or_else(|| "anonymous class".to_owned());
        VmError::type_error(format!(
            "Super constructor {parent} of {class} is not a constructor"
        ))
    }

    /// Charges the time countdown for the prototype chain from `object`
    /// that a `super` property access walks (a step per 16 objects).
    fn charge_chain(&mut self, object: Gc<Object>) -> VmResult<()> {
        let mut steps = 0usize;
        let mut current = self.heap.get_prototype_of(object)?;
        while let Some(proto) = current {
            steps += 1;
            current = self.heap.get_prototype_of(proto)?;
        }
        self.charge(steps / 16)
    }

    /// `GetValue` of a `super` property (§13.3.7.3): `base.[[Get]](key,
    /// this)`; `base` is `null` when the home object has no prototype.
    pub(crate) fn super_get(
        &mut self,
        base: Value,
        key: PropertyKey,
        this: Value,
    ) -> VmResult<Lookup> {
        match base {
            Value::Object(object) => {
                self.charge_chain(object)?;
                self.get_with_receiver(object, key, this)
            }
            Value::Null => Err(self.cannot_read(base, key)),
            _ => Err(VmError::invariant("a super base that is not an object")),
        }
    }

    /// `PutValue` of a `super` property: `base.[[Set]](key, value,
    /// this)`; a failure throws in strict mode code.
    pub(crate) fn super_set(
        &mut self,
        base: Value,
        key: PropertyKey,
        value: Value,
        (this, strict): (Value, bool),
    ) -> VmResult<SetOutcome> {
        let object = match base {
            Value::Object(object) => object,
            Value::Null => return Err(self.cannot_set(base, key)),
            _ => return Err(VmError::invariant("a super base that is not an object")),
        };
        self.charge_chain(object)?;
        let value = self.array_length_operand(this, key, value)?;
        match self.heap.set(object, key, value, this, &self.vm)? {
            crate::object::SetResult::CallSetter { setter, receiver } => {
                Ok(SetOutcome::Setter { setter, receiver })
            }
            crate::object::SetResult::Done(false) if strict => Err(match this {
                Value::Object(receiver) => self.failed_write(receiver, key),
                _ => VmError::type_error(format!(
                    "Cannot assign to read only property '{}' of {}",
                    self.key_text(key),
                    self.quoted_primitive(this)
                )),
            }),
            crate::object::SetResult::Done(_) => Ok(SetOutcome::Done),
        }
    }

    /// A primitive with its type as V8 shows it in messages (`number
    /// '5'`).
    fn quoted_primitive(&self, value: Value) -> String {
        let kind = match value {
            Value::Bool(_) => "boolean",
            Value::Int(_) | Value::Double(_) => "number",
            Value::String(_) => "string",
            Value::Symbol(_) => "symbol",
            Value::BigInt(_) => "bigint",
            _ => "object",
        };
        format!("{kind} '{}'", self.primitive_text(value))
    }
}
