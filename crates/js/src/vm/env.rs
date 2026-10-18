//! Dynamic environments at run time (ADR 0026 "Dynamic scope", ECMA-262
//! §9.1.1): the object environment records of `with` statements
//! (§9.1.1.2) and the records of the vars that sloppy direct eval code
//! declares in the variable environment of a function call (§19.2.1.3).
//!
//! A record is an internal object ([`EnvRecord`]) that links to the next
//! outer record. The record of a `with` statement holds its binding
//! object; the record of eval vars is declarative: its bindings are its
//! own data properties (configurable, because a `delete` of such a var
//! succeeds), and it has no prototype. The scope analysis gives each
//! reference in the reach of records the number of records to check before
//! its static binding; [`Runtime::find_binding`] walks that many.
//!
//! Each record that a walk passes and each object of a binding object's
//! prototype chain cost a time step, so a long chain cannot outrun the
//! deadline.

use crate::heap::{Gc, Tracer};
use crate::object::{Object, ObjectKind, Property, PropertyDescriptor};
use crate::runtime::Runtime;
use crate::string::{JsString, PropertyKey};
use crate::value::Value;
use crate::vm::{Lookup, SetOutcome, VmError, VmResult, WellKnown};

/// The count of [`crate::bytecode::Insn::FindBinding`] that walks the
/// whole chain of records.
pub(crate) const ALL_RECORDS: u16 = u16::MAX;

/// A dynamic environment record.
#[derive(Clone, Copy, Debug)]
pub struct EnvRecord {
    /// The binding object of a `with` statement; `None` for the record of
    /// eval vars, whose bindings are its own properties.
    pub(crate) object: Option<Gc<Object>>,
    /// The next outer record.
    pub(crate) outer: Option<Gc<Object>>,
}

impl EnvRecord {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        if let Some(object) = self.object {
            tracer.object(object);
        }
        if let Some(outer) = self.outer {
            tracer.object(outer);
        }
    }
}

impl Runtime {
    /// The record of a `with` statement (§14.11.2 steps 3 and 4):
    /// `ToObject` of the value, linked to `outer` (a record or
    /// `undefined`).
    pub(crate) fn new_with_env(&mut self, value: Value, outer: Value) -> VmResult<Value> {
        let object = self.to_object(value)?;
        // `to_object` made a wrapper for a primitive: keep it until the
        // record holds it.
        self.heap.record(object);
        self.new_env(Some(object), outer)
    }

    /// The record of the vars that sloppy direct eval code declares in a
    /// function's variable environment, linked to `outer`.
    pub(crate) fn new_var_env(&mut self, outer: Value) -> VmResult<Value> {
        self.new_env(None, outer)
    }

    fn new_env(&mut self, object: Option<Gc<Object>>, outer: Value) -> VmResult<Value> {
        let outer = match outer {
            Value::Object(record) => Some(record),
            Value::Undefined => None,
            _ => return Err(VmError::invariant("an outer record that is not a record")),
        };
        let kind = ObjectKind::Environment(Box::new(EnvRecord { object, outer }));
        let record = self.heap.new_object_with_kind(None, kind)?;
        Ok(Value::Object(record))
    }

    /// The parts of a record: its binding object (`None` for eval vars) and
    /// the next outer record.
    fn env_parts(&self, record: Gc<Object>) -> VmResult<EnvRecord> {
        match &self.heap.object(record)?.kind {
            ObjectKind::Environment(env) => Ok(**env),
            _ => Err(VmError::invariant(
                "a dynamic environment that is not a record",
            )),
        }
    }

    /// The first of `count` records from `env` (all of them for
    /// [`ALL_RECORDS`]) that has a binding of `name` (`HasBinding`,
    /// §9.1.1.2.1 with the `@@unscopables` check, §9.1.1.1.1), or
    /// `undefined`. The check of an object record can run script code (a
    /// getter of `@@unscopables`), through re-entry.
    pub(crate) fn find_binding(
        &mut self,
        env: Value,
        count: u16,
        name: Gc<JsString>,
    ) -> VmResult<Value> {
        let key = PropertyKey::String(name);
        let mut current = match env {
            Value::Object(record) => Some(record),
            _ => None,
        };
        let mut left = count;
        while let Some(record) = current {
            if left == 0 {
                break;
            }
            if count != ALL_RECORDS {
                left -= 1;
            }
            self.tick()?;
            let EnvRecord { object, outer } = self.env_parts(record)?;
            let found = match object {
                None => self.heap.get_own_property(record, key)?.is_some(),
                Some(object) => self.with_scope(|rt| rt.object_has_binding(object, key))?,
            };
            if found {
                return Ok(Value::Object(record));
            }
            current = outer;
        }
        Ok(Value::Undefined)
    }

    /// `HasBinding` of an object environment record of a `with` statement
    /// (§9.1.1.2.1).
    fn object_has_binding(&mut self, object: Gc<Object>, key: PropertyKey) -> VmResult<bool> {
        if !self.has_property_charged(object, key)? {
            return Ok(false);
        }
        let unscopables = self.symbol_key(WellKnown::Unscopables);
        let blocked = self.get_value(Value::Object(object), unscopables)?;
        if let Value::Object(blocked) = blocked {
            let value = self.get_value(Value::Object(blocked), key)?;
            return Ok(!self.to_boolean(value)?);
        }
        Ok(true)
    }

    /// `HasProperty` with a time step for each object of the prototype
    /// chain (a proxy on the chain takes the generic path).
    fn has_property_charged(&mut self, object: Gc<Object>, key: PropertyKey) -> VmResult<bool> {
        let mut current = object;
        loop {
            self.tick()?;
            if self.heap.is_proxy(current)? {
                return self.with_scope(|rt| rt.has_generic(current, key));
            }
            // A numeric key (`NaN`, `Infinity`) of a typed array ends the
            // walk (§10.4.5).
            if let Some((data, index)) = self.typed_numeric(current, key)? {
                return Ok(self.heap.valid_index(&data, index)?.is_some());
            }
            if self.heap.get_own_property(current, key)?.is_some()
                || self.heap.string_object_unit(current, key)?.is_some()
            {
                return Ok(true);
            }
            match self.heap.get_prototype_of(current)? {
                Some(proto) => current = proto,
                None => return Ok(false),
            }
        }
    }

    /// `GetBindingValue` of a record that [`Runtime::find_binding`] found
    /// (§9.1.1.2.6, §9.1.1.1.6). A getter of the binding object is returned
    /// for the caller to run.
    pub(crate) fn binding_value(
        &mut self,
        record: Value,
        name: Gc<JsString>,
        strict: bool,
    ) -> VmResult<Lookup> {
        let record = env_object(record)?;
        let key = PropertyKey::String(name);
        match self.env_parts(record)?.object {
            None => Ok(match self.heap.get_own_property(record, key)? {
                Some(Property::Data { value, .. }) => Lookup::Value(value),
                _ => Lookup::Value(Value::Undefined),
            }),
            Some(object) => {
                if !self.has_property_charged(object, key)? {
                    if strict {
                        return Err(self.not_defined(name));
                    }
                    return Ok(Lookup::Value(Value::Undefined));
                }
                self.get_with_receiver(object, key, Value::Object(object))
            }
        }
    }

    /// `SetMutableBinding` of a record that [`Runtime::find_binding`]
    /// found (§9.1.1.2.5, §9.1.1.1.5). A setter of the binding object is
    /// returned for the caller to run.
    pub(crate) fn set_binding_value(
        &mut self,
        record: Value,
        name: Gc<JsString>,
        value: Value,
        strict: bool,
    ) -> VmResult<SetOutcome> {
        let record = env_object(record)?;
        let key = PropertyKey::String(name);
        match self.env_parts(record)?.object {
            None => {
                let exists = self.heap.get_own_property(record, key)?.is_some();
                if !exists && strict {
                    return Err(self.not_defined(name));
                }
                self.set_var_binding(record, key, value)?;
                Ok(SetOutcome::Done)
            }
            Some(object) => {
                let exists = self.has_property_charged(object, key)?;
                if !exists && strict {
                    return Err(self.not_defined(name));
                }
                self.put_value(Value::Object(object), key, value, strict)
            }
        }
    }

    /// `DeleteBinding` of a record that [`Runtime::find_binding`] found
    /// (§9.1.1.2.7; the vars of eval code can be deleted, §9.1.1.1.7).
    pub(crate) fn delete_binding(&mut self, record: Value, name: Gc<JsString>) -> VmResult<bool> {
        let record = env_object(record)?;
        let key = PropertyKey::String(name);
        let object = self.env_parts(record)?.object.unwrap_or(record);
        self.with_scope(|rt| rt.delete_generic(object, key, false))
    }

    /// `WithBaseObject` (§9.1.1.2.10): the binding object of a `with`
    /// record, `undefined` for any other record.
    pub(crate) fn with_base_object(&self, record: Value) -> VmResult<Value> {
        let record = env_object(record)?;
        Ok(self
            .env_parts(record)?
            .object
            .map_or(Value::Undefined, Value::Object))
    }

    /// Creates or sets a binding of a record of eval vars (its bindings are
    /// writable, configurable data properties).
    pub(crate) fn set_var_binding(
        &mut self,
        record: Gc<Object>,
        key: PropertyKey,
        value: Value,
    ) -> VmResult<()> {
        let desc = match self.heap.get_own_property(record, key)? {
            Some(_) => PropertyDescriptor::value(value),
            None => PropertyDescriptor::data(value, true, false, true),
        };
        if self.heap.define_own_property(record, key, desc, &self.vm)? {
            Ok(())
        } else {
            Err(VmError::invariant(
                "a binding of eval vars could not be set",
            ))
        }
    }

    /// Whether a record of eval vars has a binding of `key`.
    pub(crate) fn var_env_has(&self, record: Gc<Object>, key: PropertyKey) -> VmResult<bool> {
        Ok(self.heap.get_own_property(record, key)?.is_some())
    }

    /// The `ReferenceError` of an unresolvable name in strict code.
    pub(crate) fn not_defined(&self, name: Gc<JsString>) -> VmError {
        VmError::reference_error(format!("{} is not defined", self.name_text(name)))
    }
}

/// The record object of a record value.
fn env_object(record: Value) -> VmResult<Gc<Object>> {
    match record {
        Value::Object(record) => Ok(record),
        _ => Err(VmError::invariant("a binding lookup without a record")),
    }
}
