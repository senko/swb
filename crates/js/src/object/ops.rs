//! The internal methods of ordinary objects that do not run script code
//! (ECMA-262 §10.1), with the dispatch to array exotic objects
//! (§10.4.2).
//!
//! [[Get]] and [[Set]] walk the prototype chain in a loop. Where they
//! meet an accessor, they return a request to call the getter or the
//! setter with the receiver; the VM runs it (ADR 0026 section 5).
//! Methods that can grow a buffer of script-controlled size take a
//! [`RootSource`]: they can collect. They keep their own arguments alive
//! during that collection; everything else that the caller holds must be
//! rooted (VM registers, handle scopes).

use crate::error::{Error, Result};
use crate::heap::{Gc, Heap, RootSource};
use crate::object::property::{Apply, validate_and_apply};
use crate::object::{Object, ObjectKind, Property, PropertyDescriptor, ShapeKind};
use crate::string::PropertyKey;
use crate::value::Value;

/// The result of [[Get]].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GetResult {
    /// The value of a data property, or `undefined`.
    Value(Value),
    /// The property is an accessor: call `getter` with `this` = `receiver`
    /// and no arguments; its result is the value.
    CallGetter {
        /// The getter function.
        getter: Value,
        /// The `this` value of the call.
        receiver: Value,
    },
}

/// The result of [[Set]].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SetResult {
    /// Done; the boolean result of [[Set]].
    Done(bool),
    /// The property is an accessor with a setter: call `setter` with
    /// `this` = `receiver` and the value as its argument; then [[Set]]
    /// returns true.
    CallSetter {
        /// The setter function.
        setter: Value,
        /// The `this` value of the call.
        receiver: Value,
    },
}

/// The Number value of an array length.
pub(crate) fn length_value(length: u32) -> Value {
    Value::number(f64::from(length))
}

impl Heap {
    /// `[[GetPrototypeOf]]` (§10.1.1).
    pub fn get_prototype_of(&self, object: Gc<Object>) -> Result<Option<Gc<Object>>> {
        let record = self.object(object)?;
        Ok(self.arenas.shapes.get(record.shape)?.proto)
    }

    /// `[[SetPrototypeOf]]` (§10.1.2, `OrdinarySetPrototypeOf`). An object
    /// without own named properties moves to the root shape of the new
    /// prototype; any other object goes to dictionary mode.
    pub fn set_prototype_of(
        &mut self,
        object: Gc<Object>,
        proto: Option<Gc<Object>>,
    ) -> Result<bool> {
        Ok(self.set_prototype_of_counted(object, proto)?.0)
    }

    /// [`Heap::set_prototype_of`] that also returns the number of
    /// prototypes the cycle check visited, so that a caller can charge the
    /// time countdown for it.
    pub fn set_prototype_of_counted(
        &mut self,
        object: Gc<Object>,
        proto: Option<Gc<Object>>,
    ) -> Result<(bool, usize)> {
        let record = self.object(object)?;
        let shape = self.arenas.shapes.get(record.shape)?;
        if shape.proto == proto {
            return Ok((true, 0));
        }
        if !record.extensible {
            return Ok((false, 0));
        }
        // Step 8: no cycles. All kinds so far use the ordinary
        // [[GetPrototypeOf]], so the walk goes to the end of the chain.
        let mut current = proto;
        let mut steps = 0;
        while let Some(p) = current {
            steps += 1;
            if p == object {
                return Ok((false, steps));
            }
            current = self.get_prototype_of(p)?;
        }
        let empty_shared = matches!(&shape.kind, ShapeKind::Shared(shared) if shared.len == 0);
        if empty_shared {
            let root = self.root_shape(proto)?;
            self.object_mut(object)?.shape = root;
        } else {
            self.make_dictionary(object)?;
            let shape = self.object(object)?.shape;
            self.arenas.shapes.get_mut(shape)?.proto = proto;
        }
        Ok((true, steps))
    }

    /// `[[IsExtensible]]` (§10.1.3).
    pub fn is_extensible(&self, object: Gc<Object>) -> Result<bool> {
        Ok(self.object(object)?.extensible)
    }

    /// `[[PreventExtensions]]` (§10.1.4).
    pub fn prevent_extensions(&mut self, object: Gc<Object>) -> Result<bool> {
        self.object_mut(object)?.extensible = false;
        Ok(true)
    }

    /// `[[GetOwnProperty]]` (§10.1.5; for arrays also the `length`
    /// property, §10.4.2).
    pub fn get_own_property(
        &self,
        object: Gc<Object>,
        key: PropertyKey,
    ) -> Result<Option<Property>> {
        let record = self.object(object)?;
        match key {
            PropertyKey::Index(index) => Ok(record.elements.get(index)),
            PropertyKey::String(name) if name == self.names.length => {
                if let ObjectKind::Array {
                    length,
                    length_writable,
                } = record.kind
                {
                    return Ok(Some(Property::Data {
                        value: length_value(length),
                        writable: length_writable,
                        enumerable: false,
                        configurable: false,
                    }));
                }
                self.named_property(record, key)
            }
            _ => self.named_property(record, key),
        }
    }

    /// `[[DefineOwnProperty]]` (§10.1.6; §10.4.2.1 for arrays). Can collect.
    pub fn define_own_property(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        desc: PropertyDescriptor,
        roots: &dyn RootSource,
    ) -> Result<bool> {
        desc.check()?;
        if let Some(unit) = self.string_object_unit(object, key)? {
            return self.string_define_own_property(unit, &desc);
        }
        match self.object(object)?.kind {
            ObjectKind::Array { .. } => self.array_define_own_property(object, key, desc, roots),
            _ => self.ordinary_define_own_property(object, key, desc, roots),
        }
    }

    /// The code unit that a String object's index property `key` has
    /// (§10.4.3.5 `StringGetOwnProperty`): `Some` if `object` is a String
    /// object and `key` an index below the length of its string. These
    /// properties are not stored: they are enumerable, not writable and
    /// not configurable data properties.
    pub(crate) fn string_object_unit(
        &self,
        object: Gc<Object>,
        key: PropertyKey,
    ) -> Result<Option<u16>> {
        let PropertyKey::Index(index) = key else {
            return Ok(None);
        };
        match self.object(object)?.kind {
            ObjectKind::StringWrapper(string) => {
                Ok(self.string(string)?.as_str16().get(index as usize))
            }
            _ => Ok(None),
        }
    }

    /// `[[DefineOwnProperty]]` of a String object for an index property
    /// of its string (§10.4.3.2 step 2): `IsCompatiblePropertyDescriptor`
    /// against the data property of the character. Nothing is stored.
    fn string_define_own_property(&self, unit: u16, desc: &PropertyDescriptor) -> Result<bool> {
        if desc.is_accessor()
            || desc.configurable == Some(true)
            || desc.enumerable == Some(false)
            || desc.writable == Some(true)
        {
            return Ok(false);
        }
        match desc.value {
            None => Ok(true),
            Some(Value::String(string)) => Ok(self.string(string)?.as_str16().get(0) == Some(unit)
                && self.string(string)?.len() == 1),
            Some(_) => Ok(false),
        }
    }

    /// `OrdinaryDefineOwnProperty` (§10.1.6.1). Not for `length` of an array.
    pub(crate) fn ordinary_define_own_property(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        desc: PropertyDescriptor,
        roots: &dyn RootSource,
    ) -> Result<bool> {
        let current = self.get_own_property(object, key)?;
        let extensible = self.object(object)?.extensible;
        match validate_and_apply(self, extensible, &desc, current)? {
            Apply::Reject => Ok(false),
            Apply::Unchanged => Ok(true),
            Apply::Create(property) => {
                self.add_own_property(object, key, property, roots)?;
                Ok(true)
            }
            Apply::Update(property) => {
                let old = current.ok_or(Error::invariant("an update without a property"))?;
                self.replace_own_property(object, key, old, property, roots)?;
                Ok(true)
            }
        }
    }

    /// Stores a new own property. Can collect.
    fn add_own_property(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        property: Property,
        roots: &dyn RootSource,
    ) -> Result<()> {
        let PropertyKey::Index(index) = key else {
            return self.add_named(object, key, property, roots);
        };
        let estimate = self
            .object(object)?
            .elements
            .growth_estimate(index, &property);
        if estimate > 0 {
            let (first, second) = property.slot_values();
            let extra = [object.into(), first, second.unwrap_or_default()];
            self.reserve_with(estimate, roots, &extra)?;
        }
        let before = self.object_bytes(object)?;
        self.object_mut(object)?.elements.insert(index, property)?;
        self.charge_growth(object, before)
    }

    /// Replaces an own property. Can collect.
    fn replace_own_property(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        old: Property,
        new: Property,
        roots: &dyn RootSource,
    ) -> Result<()> {
        let PropertyKey::Index(index) = key else {
            return self.replace_named(object, key, old, new);
        };
        if !new.is_default_data() {
            let elements = &self.object(object)?.elements;
            if matches!(elements, crate::object::Elements::Dense { .. }) {
                let estimate = elements.stored() * crate::object::elements::SPARSE_ENTRY_BYTES;
                let (first, second) = new.slot_values();
                let extra = [object.into(), first, second.unwrap_or_default()];
                self.reserve_with(estimate, roots, &extra)?;
            }
        }
        let before = self.object_bytes(object)?;
        self.object_mut(object)?.elements.replace(index, new)?;
        self.charge_growth(object, before)
    }

    /// `[[HasProperty]]` (§10.1.7).
    pub fn has_property(&self, object: Gc<Object>, key: PropertyKey) -> Result<bool> {
        let mut current = object;
        loop {
            if self.get_own_property(current, key)?.is_some()
                || self.string_object_unit(current, key)?.is_some()
            {
                return Ok(true);
            }
            match self.get_prototype_of(current)? {
                Some(proto) => current = proto,
                None => return Ok(false),
            }
        }
    }

    /// [[Get]] (§10.1.8). Returns a request to call the getter where it
    /// meets an accessor.
    pub fn get(&self, object: Gc<Object>, key: PropertyKey, receiver: Value) -> Result<GetResult> {
        let mut current = object;
        loop {
            match self.get_own_property(current, key)? {
                Some(Property::Data { value, .. }) => return Ok(GetResult::Value(value)),
                Some(Property::Accessor { get, .. }) => {
                    if get.is_undefined() {
                        return Ok(GetResult::Value(Value::Undefined));
                    }
                    return Ok(GetResult::CallGetter {
                        getter: get,
                        receiver,
                    });
                }
                None => match self.get_prototype_of(current)? {
                    Some(proto) => current = proto,
                    None => return Ok(GetResult::Value(Value::Undefined)),
                },
            }
        }
    }

    /// [[Set]] (§10.1.9, `OrdinarySet`). Returns a request to call the setter
    /// where it meets an accessor. Can collect.
    pub fn set(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        value: Value,
        receiver: Value,
        roots: &dyn RootSource,
    ) -> Result<SetResult> {
        if value.is_internal() {
            return Err(Error::invariant("[[Set]] with an internal value"));
        }
        if receiver == Value::Object(object) && self.set_own_fast(object, key, value)? {
            return Ok(SetResult::Done(true));
        }
        // OrdinarySetWithOwnDescriptor (§10.1.9.2), steps 1 to 2 as a loop
        // over the prototype chain.
        let mut current = object;
        loop {
            if self.string_object_unit(current, key)?.is_some() {
                // A character of a String object: not writable.
                return Ok(SetResult::Done(false));
            }
            match self.get_own_property(current, key)? {
                Some(Property::Data { writable, .. }) => {
                    if !writable {
                        return Ok(SetResult::Done(false));
                    }
                    break;
                }
                Some(Property::Accessor { set, .. }) => {
                    if set.is_undefined() {
                        return Ok(SetResult::Done(false));
                    }
                    return Ok(SetResult::CallSetter {
                        setter: set,
                        receiver,
                    });
                }
                None => match self.get_prototype_of(current)? {
                    Some(proto) => current = proto,
                    None => break,
                },
            }
        }
        // Step 3: a writable data property (or none) was found.
        let Value::Object(target) = receiver else {
            return Ok(SetResult::Done(false));
        };
        if self.string_object_unit(target, key)?.is_some() {
            return Ok(SetResult::Done(false));
        }
        let done = match self.get_own_property(target, key)? {
            Some(
                Property::Accessor { .. }
                | Property::Data {
                    writable: false, ..
                },
            ) => false,
            Some(Property::Data { .. }) => {
                self.define_own_property(target, key, PropertyDescriptor::value(value), roots)?
            }
            None => self.create_data_property(target, key, value, roots)?,
        };
        Ok(SetResult::Done(done))
    }

    /// Writes an existing own writable data property without the general
    /// path: an element of the dense vector or a named slot. Returns false
    /// if the fast path does not apply.
    fn set_own_fast(&mut self, object: Gc<Object>, key: PropertyKey, value: Value) -> Result<bool> {
        match key {
            PropertyKey::Index(index) => {
                Ok(self.object_mut(object)?.elements.write_dense(index, value))
            }
            PropertyKey::String(name)
                if name == self.names.length
                    && matches!(self.object(object)?.kind, ObjectKind::Array { .. }) =>
            {
                Ok(false)
            }
            _ => {
                let record = self.object(object)?;
                let shape = self.arenas.shapes.get(record.shape)?;
                let Some(entry) = shape.find(key, &self.arenas.key_lists)? else {
                    return Ok(false);
                };
                if entry.flags.accessor() || !entry.flags.writable() {
                    return Ok(false);
                }
                let slot = self
                    .object_mut(object)?
                    .slots
                    .get_mut(entry.slot as usize)
                    .ok_or(Error::invariant("a shape entry outside the slots"))?;
                *slot = value;
                Ok(true)
            }
        }
    }

    /// `CreateDataProperty` (§7.3.5). Can collect.
    pub fn create_data_property(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        value: Value,
        roots: &dyn RootSource,
    ) -> Result<bool> {
        self.define_own_property(
            object,
            key,
            PropertyDescriptor::data(value, true, true, true),
            roots,
        )
    }

    /// [[Delete]] (§10.1.10).
    pub fn delete(&mut self, object: Gc<Object>, key: PropertyKey) -> Result<bool> {
        if self.string_object_unit(object, key)?.is_some() {
            return Ok(false);
        }
        let Some(current) = self.get_own_property(object, key)? else {
            return Ok(true);
        };
        if !current.configurable() {
            return Ok(false);
        }
        match key {
            PropertyKey::Index(index) => self.object_mut(object)?.elements.remove(index),
            _ => self.remove_named(object, key)?,
        }
        Ok(true)
    }

    /// Appends to `found` the string keys of the own named properties
    /// of `object` that hold `function`, as a data value or as a getter
    /// or setter (for the method name in stack traces). It looks at no
    /// more than `*budget` properties and lowers the budget by the number
    /// it looked at; it allocates nothing in proportion to the object.
    pub(crate) fn keys_holding(
        &self,
        object: Gc<Object>,
        function: Gc<Object>,
        budget: &mut usize,
        found: &mut Vec<PropertyKey>,
    ) -> Result<()> {
        let record = self.object(object)?;
        let shape = self.arenas.shapes.get(record.shape)?;
        for entry in shape.entries(&self.arenas.key_lists)? {
            if *budget == 0 {
                break;
            }
            *budget -= 1;
            if entry.flags.deleted() || entry.key.is_symbol() {
                continue;
            }
            let slot = entry.slot as usize;
            let holds =
                |i: usize| matches!(record.slots.get(i), Some(Value::Object(o)) if *o == function);
            if holds(slot) || (entry.flags.accessor() && holds(slot + 1)) {
                found.push(entry.key);
            }
        }
        Ok(())
    }

    /// An upper bound of the number of keys that
    /// [`Heap::own_property_keys`] lists, without listing them.
    pub fn own_key_bound(&self, object: Gc<Object>) -> Result<usize> {
        let record = self.object(object)?;
        let shape = self.arenas.shapes.get(record.shape)?;
        let mut count = record
            .elements
            .stored()
            .saturating_add(shape.entries(&self.arenas.key_lists)?.len())
            .saturating_add(1);
        if let ObjectKind::StringWrapper(string) = record.kind {
            count = count.saturating_add(self.string(string)?.len());
        }
        Ok(count)
    }

    /// `[[OwnPropertyKeys]]` (§10.1.11.1): array indices in ascending order,
    /// then strings and then symbols, each in creation order. A String
    /// object lists the indices of its string first (§10.4.3.3).
    pub fn own_property_keys(&self, object: Gc<Object>) -> Result<Vec<PropertyKey>> {
        let record = self.object(object)?;
        let shape = self.arenas.shapes.get(record.shape)?;
        let entries = shape.entries(&self.arenas.key_lists)?;
        let mut keys = Vec::new();
        // The vector is outside the heap accounting: check that it fits
        // under the limit before it is allocated.
        let count = self.own_key_bound(object)?;
        self.check_room(count.saturating_mul(size_of::<PropertyKey>()))?;
        keys.try_reserve(count)?;
        if let ObjectKind::StringWrapper(string) = record.kind {
            // §10.4.3.3 step 5: the indices of the string come first.
            let length = self.string(string)?.len();
            keys.extend((0..length).map(|index| PropertyKey::Index(index as u32)));
        }
        record.elements.keys(&mut keys);
        if let ObjectKind::Array { .. } = record.kind {
            // `length` is created with the array, before any other string
            // key.
            keys.push(PropertyKey::String(self.names.length));
        }
        let live = entries.iter().filter(|entry| !entry.flags.deleted());
        keys.extend(live.clone().filter(|e| !e.key.is_symbol()).map(|e| e.key));
        keys.extend(live.filter(|e| e.key.is_symbol()).map(|e| e.key));
        Ok(keys)
    }
}
