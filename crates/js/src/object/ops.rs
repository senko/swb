//! The internal methods of ordinary objects that do not run script code
//! (ECMA-262 §10.1), with the dispatch to array exotic objects
//! (§10.4.2), String objects (§10.4.3), mapped arguments objects
//! (§10.4.4, `arguments.rs`) and typed arrays (§10.4.5, `typed_array.rs`:
//! a canonical numeric key is an element or no property and never walks
//! the prototype chain; conversions that run script code and the
//! elements of `BigInt` arrays, which are new `BigInt`s, are left to the
//! runtime through the `Generic` results).
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
use crate::object::elements::DenseWrite;
use crate::object::property::{Apply, validate_and_apply};
use crate::object::{
    Elements, Object, ObjectKind, Property, PropertyDescriptor, RawValue, ShapeKind, TypedArrayData,
};
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
    /// The walk met a proxy, whose internal methods run script code: the
    /// runtime takes the generic path from the start (nothing happened
    /// yet).
    Generic,
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
    /// The walk met a proxy, or the receiver is one: the runtime takes the
    /// generic path from the start (nothing was written).
    Generic,
}

/// The Number value of an array length.
pub(crate) fn length_value(length: u32) -> Value {
    Value::number(f64::from(length))
}

impl Heap {
    /// `[[GetPrototypeOf]]` (§10.1.1).
    #[inline]
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
        if let ObjectKind::Proxy(_) = record.kind {
            return Err(Error::invariant("a heap [[SetPrototypeOf]] of a proxy"));
        }
        let shape = self.arenas.shapes.get(record.shape)?;
        if shape.proto == proto {
            return Ok((true, 0));
        }
        if !record.extensible {
            return Ok((false, 0));
        }
        // Step 8: no cycles. The walk ends at a proxy (its
        // [[GetPrototypeOf]] is not the ordinary one): a proxy has no
        // prototype in its shape.
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
        self.layout_changed(object)?;
        if empty_shared {
            let root = self.root_shape(proto)?;
            self.object_mut(object)?.shape = root;
        } else {
            if let Some(proto) = proto {
                self.watch(proto)?;
            }
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

    /// `[[PreventExtensions]]` (§10.1.4; a typed array that can grow or
    /// shrink with its buffer refuses, §10.4.5).
    pub fn prevent_extensions(&mut self, object: Gc<Object>) -> Result<bool> {
        if let Some(data) = self.typed_array(object)?
            && !self.typed_fixed_length(&data)?
        {
            return Ok(false);
        }
        let record = self.object_mut(object)?;
        if let ObjectKind::Proxy(_) = record.kind {
            return Err(Error::invariant("a heap [[PreventExtensions]] of a proxy"));
        }
        record.extensible = false;
        Ok(true)
    }

    /// `[[GetOwnProperty]]` (§10.1.5; for arrays also the `length`
    /// property, §10.4.2; for typed arrays §10.4.5).
    #[inline]
    pub fn get_own_property(
        &self,
        object: Gc<Object>,
        key: PropertyKey,
    ) -> Result<Option<Property>> {
        let record = self.object(object)?;
        if let ObjectKind::TypedArray(_) = record.kind {
            return self.typed_own_property_of(record, key);
        }
        self.own_property_of(record, key)
    }

    /// [`Heap::get_own_property`] of an object record that is not a typed
    /// array, or of a typed array with a key that is not a canonical
    /// numeric string (the walks of `[[Get]]`, `[[Set]]` and
    /// `[[HasProperty]]` handle the numeric keys of typed arrays before
    /// they call this).
    #[inline]
    fn own_property_of(&self, record: &Object, key: PropertyKey) -> Result<Option<Property>> {
        match key {
            PropertyKey::Index(index) => match record.elements.get(index) {
                // §10.4.4.1: a mapped argument has its parameter's value
                // (only mapped arguments objects store cells, `arguments.rs`).
                Some(
                    property @ Property::Data {
                        value: Value::Cell(cell),
                        ..
                    },
                ) => Ok(Some(self.mapped_property(property, cell)?)),
                property => Ok(property),
            },
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

    /// [`Heap::own_property_of`] of a typed array: a canonical numeric key
    /// is an element or no property (never a stored one), another key a
    /// named property.
    #[cold]
    #[inline(never)]
    fn typed_own_property_of(&self, record: &Object, key: PropertyKey) -> Result<Option<Property>> {
        let ObjectKind::TypedArray(data) = &record.kind else {
            return Err(Error::invariant("a typed array lookup of another object"));
        };
        match self.numeric_index(key)? {
            Some(index) => self.typed_own_property(data, index),
            None => self.named_property(record, key),
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
        if let (PropertyKey::Index(index), Some(cell)) = (key, self.mapped_argument(object, key)?) {
            return self.arguments_define_own_property(object, (index, cell), desc, roots);
        }
        match &self.object(object)?.kind {
            ObjectKind::Array { .. } => self.array_define_own_property(object, key, desc, roots),
            ObjectKind::Proxy(_) => {
                Err(Error::invariant("a heap [[DefineOwnProperty]] of a proxy"))
            }
            ObjectKind::TypedArray(_) => self.typed_or_ordinary_define(object, key, desc, roots),
            _ => self.ordinary_define_own_property(object, key, desc, roots),
        }
    }

    /// `[[DefineOwnProperty]]` of a typed array (§10.4.5): an element for a
    /// numeric key, an ordinary property for another key.
    #[cold]
    #[inline(never)]
    fn typed_or_ordinary_define(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        desc: PropertyDescriptor,
        roots: &dyn RootSource,
    ) -> Result<bool> {
        let data = self.typed_array(object)?.ok_or(Error::invariant(
            "a typed array definition on another object",
        ))?;
        match self.numeric_index(key)? {
            Some(index) => self.typed_define_own_property(&data, index, &desc),
            None => self.ordinary_define_own_property(object, key, desc, roots),
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
        self.string_unit_of(self.object(object)?, key)
    }

    /// [`Heap::string_object_unit`] of an object record.
    fn string_unit_of(&self, record: &Object, key: PropertyKey) -> Result<Option<u16>> {
        let PropertyKey::Index(index) = key else {
            return Ok(None);
        };
        match record.kind {
            ObjectKind::StringWrapper(string) => Ok(self.text(string)?.get(index as usize)),
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
            Some(Value::String(string)) => {
                let text = self.string(string)?;
                Ok(text.len() == 1 && text.as_flat().and_then(|t| t.get(0)) == Some(unit))
            }
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
            if matches!(elements, Elements::Dense { .. }) {
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

    /// `[[HasProperty]]` (§10.1.7). A proxy on the chain is an internal
    /// error: the runtime's `has_property` answers for proxies.
    pub fn has_property(&self, object: Gc<Object>, key: PropertyKey) -> Result<bool> {
        self.has_property_fast(object, key)?.ok_or(Error::invariant(
            "a heap [[HasProperty]] that meets a proxy",
        ))
    }

    /// `[[HasProperty]]` (§10.1.7, §10.4.5 for typed arrays), or `None`
    /// when the walk meets a proxy (the runtime then takes the generic
    /// path).
    pub(crate) fn has_property_fast(
        &self,
        object: Gc<Object>,
        key: PropertyKey,
    ) -> Result<Option<bool>> {
        let mut current = object;
        loop {
            let record = self.object(current)?;
            if record.kind.walks_exotic() {
                match &record.kind {
                    ObjectKind::TypedArray(data) => {
                        if let Some(index) = self.numeric_index(key)? {
                            return Ok(Some(self.valid_index(data, index)?.is_some()));
                        }
                    }
                    _ => return Ok(None),
                }
            }
            if self.own_property_of(record, key)?.is_some()
                || self.string_unit_of(record, key)?.is_some()
            {
                return Ok(Some(true));
            }
            match self.arenas.shapes.get(record.shape)?.proto {
                Some(proto) => current = proto,
                None => return Ok(Some(false)),
            }
        }
    }

    /// [[Get]] (§10.1.8; §10.4.5 for a numeric key of a typed array,
    /// which ends the walk). Returns a request to call the getter where it
    /// meets an accessor.
    pub fn get(&self, object: Gc<Object>, key: PropertyKey, receiver: Value) -> Result<GetResult> {
        let mut current = object;
        loop {
            let record = self.object(current)?;
            if record.kind.walks_exotic()
                && let Some(found) = self.exotic_get(record, key)?
            {
                return Ok(found);
            }
            match self.own_property_of(record, key)? {
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
                None => match self.arenas.shapes.get(record.shape)?.proto {
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
        self.set_walk(object, key, value, receiver, roots)
    }

    /// The steps of [`Heap::set`] after its fast path: the walk of the
    /// prototype chain and the definition on the receiver. Out of line, so
    /// that the fast path of an own writable property stays short.
    #[inline(never)]
    fn set_walk(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        value: Value,
        receiver: Value,
        roots: &dyn RootSource,
    ) -> Result<SetResult> {
        // OrdinarySetWithOwnDescriptor (§10.1.9.2), steps 1 to 2 as a loop
        // over the prototype chain.
        let mut current = object;
        loop {
            let record = self.object(current)?;
            if record.kind.sets_exotic() {
                match self.exotic_set(record, current, key, receiver)? {
                    ExoticSet::Result(result) => return Ok(result),
                    // A valid index of a typed array: a writable data
                    // property (OrdinarySet).
                    ExoticSet::Element => break,
                    ExoticSet::Ordinary => {}
                }
            }
            match self.own_property_of(record, key)? {
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
                None => match self.arenas.shapes.get(record.shape)?.proto {
                    Some(proto) => current = proto,
                    None => break,
                },
            }
        }
        // Step 3: a writable data property (or none) was found.
        let Value::Object(target) = receiver else {
            return Ok(SetResult::Done(false));
        };
        let record = self.object(target)?;
        if record.kind.sets_exotic()
            && let Some(result) = self.exotic_receiver(record, key)?
        {
            return Ok(result);
        }
        let done = match self.own_property_of(record, key)? {
            Some(
                Property::Accessor { .. }
                | Property::Data {
                    writable: false, ..
                },
            ) => false,
            Some(Property::Data { .. }) => self
                .define_own_property(target, key, PropertyDescriptor::value(value), roots)
                .map_err(super::array::length_write_error)?,
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
                let record = self.object_mut(object)?;
                match record.elements.write_dense(index, value) {
                    DenseWrite::Done => Ok(true),
                    // §10.4.4.4: a mapped argument writes its parameter.
                    DenseWrite::Cell(cell) => {
                        self.cell_mut(cell)?.value = value;
                        Ok(true)
                    }
                    DenseWrite::NotApplicable => {
                        if let ObjectKind::TypedArray(_) = record.kind {
                            return self.typed_set_fast(object, index, value);
                        }
                        Ok(false)
                    }
                }
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

    /// `CreateDataProperty` (§7.3.5) for a builder of fresh objects (the
    /// JSON parser): `object` is an ordinary, extensible object that the
    /// caller made, so no property can reject the definition. A missing
    /// property is added and an existing one gets the new value, as the
    /// general definition does, without building and validating a
    /// descriptor. Can collect.
    pub(crate) fn create_fresh_data_property(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
        value: Value,
        roots: &dyn RootSource,
    ) -> Result<()> {
        match self.get_own_property(object, key)? {
            None => self.add_own_property(object, key, Property::data(value), roots),
            Some(old) => self.replace_own_property(object, key, old, Property::data(value), roots),
        }
    }

    /// Appends `value` to a fresh array whose elements are dense without
    /// holes (the JSON parser's arrays): `CreateDataProperty(array,
    /// length, value)`. Can collect.
    pub(crate) fn push_fresh_element(
        &mut self,
        array: Gc<Object>,
        value: Value,
        roots: &dyn RootSource,
    ) -> Result<()> {
        let (length, _) = self.array_length(array)?;
        if length == u32::MAX - 1 {
            return Err(Error::range_error(super::INVALID_ARRAY_LENGTH));
        }
        self.add_own_property(
            array,
            PropertyKey::Index(length),
            Property::data(value),
            roots,
        )?;
        self.set_array_length_field(array, length + 1, true)
    }

    /// Makes room in a fresh object for `slots` more named properties
    /// and `elements` more elements, so that a builder that knows about
    /// the final size grows the buffers once. No collection.
    pub(crate) fn presize(
        &mut self,
        object: Gc<Object>,
        slots: usize,
        elements: usize,
    ) -> Result<()> {
        let before = self.object_bytes(object)?;
        let record = self.object_mut(object)?;
        record.slots.try_reserve_exact(slots)?;
        if let Elements::Dense { values, .. } = &mut record.elements {
            values.try_reserve_exact(elements)?;
        }
        self.charge_growth(object, before)
    }

    /// [[Delete]] (§10.1.10; §10.4.5 for typed arrays).
    pub fn delete(&mut self, object: Gc<Object>, key: PropertyKey) -> Result<bool> {
        let record = self.object(object)?;
        match &record.kind {
            ObjectKind::Proxy(_) => return Err(Error::invariant("a heap [[Delete]] of a proxy")),
            ObjectKind::TypedArray(data) => {
                if let Some(index) = self.numeric_index(key)? {
                    return Ok(self.valid_index(data, index)?.is_none());
                }
            }
            _ => {}
        }
        if self.string_unit_of(record, key)?.is_some() {
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
        match &record.kind {
            ObjectKind::StringWrapper(string) => {
                count = count.saturating_add(self.string(*string)?.len());
            }
            ObjectKind::TypedArray(data) => {
                count = count.saturating_add(self.typed_length(data)?.unwrap_or(0));
            }
            _ => {}
        }
        Ok(count)
    }

    /// Calls `f` with each own string key of `object` (the keys of
    /// [`Heap::own_property_keys`] without the symbols, in its order) and
    /// whether the property is enumerable. Allocates nothing (for
    /// `for`-`in`, `vm/for_in.rs`).
    pub(crate) fn each_string_key(
        &self,
        object: Gc<Object>,
        mut f: impl FnMut(PropertyKey, bool),
    ) -> Result<()> {
        let record = self.object(object)?;
        let shape = self.arenas.shapes.get(record.shape)?;
        let entries = shape.entries(&self.arenas.key_lists)?;
        match &record.kind {
            ObjectKind::StringWrapper(string) => {
                for index in 0..self.string(*string)?.len() {
                    f(PropertyKey::Index(index as u32), true);
                }
            }
            ObjectKind::TypedArray(data) => {
                for index in typed_keys(self.typed_length(data)?)? {
                    f(PropertyKey::Index(index), true);
                }
            }
            _ => {}
        }
        record.elements.each_key(|index, enumerable| {
            f(PropertyKey::Index(index), enumerable);
        });
        if let ObjectKind::Array { .. } = record.kind {
            f(PropertyKey::String(self.names.length), false);
        }
        for entry in entries {
            if !entry.flags.deleted() && !entry.key.is_symbol() {
                f(entry.key, entry.flags.enumerable());
            }
        }
        Ok(())
    }

    /// Whether `object` has an own enumerable property with a string key
    /// (stops at the first one it finds).
    pub(crate) fn has_enumerable_string_key(&self, object: Gc<Object>) -> Result<bool> {
        let record = self.object(object)?;
        match &record.kind {
            ObjectKind::StringWrapper(string) if !self.string(*string)?.is_empty() => {
                return Ok(true);
            }
            ObjectKind::TypedArray(data) if self.typed_length(data)?.unwrap_or(0) > 0 => {
                return Ok(true);
            }
            _ => {}
        }
        let found = match &record.elements {
            Elements::Dense { count, .. } => *count > 0,
            Elements::Sparse(map) => map.values().any(Property::enumerable),
        };
        if found {
            return Ok(true);
        }
        let shape = self.arenas.shapes.get(record.shape)?;
        Ok(shape.entries(&self.arenas.key_lists)?.iter().any(|entry| {
            !entry.flags.deleted() && !entry.key.is_symbol() && entry.flags.enumerable()
        }))
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
        match &record.kind {
            ObjectKind::StringWrapper(string) => {
                // §10.4.3.3 step 5: the indices of the string come first.
                let length = self.string(*string)?.len();
                keys.extend((0..length).map(|index| PropertyKey::Index(index as u32)));
            }
            ObjectKind::TypedArray(data) => {
                // §10.4.5: the indices of the elements come first.
                keys.extend(typed_keys(self.typed_length(data)?)?.map(PropertyKey::Index));
            }
            _ => {}
        }
        record.elements.keys(&mut keys);
        if let ObjectKind::Array { .. } = record.kind {
            // `length` is created with the array, before any other string
            // key.
            keys.push(PropertyKey::String(self.names.length));
        }
        self.push_named_keys(entries, &mut keys);
        Ok(keys)
    }

    /// The own keys of an object that its shape holds (the named
    /// properties: strings, then symbols, each in creation order; no
    /// elements, no `length` of an array). For `console.log` of a typed
    /// array, whose elements it lists itself.
    pub(crate) fn own_named_keys(&self, object: Gc<Object>) -> Result<Vec<PropertyKey>> {
        let record = self.object(object)?;
        let entries = self
            .arenas
            .shapes
            .get(record.shape)?
            .entries(&self.arenas.key_lists)?;
        self.check_room(entries.len().saturating_mul(size_of::<PropertyKey>()))?;
        let mut keys = Vec::new();
        keys.try_reserve(entries.len())?;
        self.push_named_keys(entries, &mut keys);
        Ok(keys)
    }

    /// Appends the live string keys and then the symbol keys of shape
    /// entries (Private Names are not properties, `private.rs`).
    fn push_named_keys(
        &self,
        entries: &[crate::object::shape::ShapeEntry],
        keys: &mut Vec<PropertyKey>,
    ) {
        let live = entries.iter().filter(|entry| !entry.flags.deleted());
        keys.extend(live.clone().filter(|e| !e.key.is_symbol()).map(|e| e.key));
        keys.extend(
            live.filter(|e| e.key.is_symbol() && !self.is_private_key(e.key))
                .map(|e| e.key),
        );
    }
}

/// The indices of the elements of a typed array of `length` elements
/// (none when it is out of bounds), as array indices. A typed array with
/// 2^32 − 1 elements or more would need string keys; the list of its keys
/// (16 bytes each) is far above any heap limit, so this is an internal
/// error that the heap check before the list makes unreachable.
fn typed_keys(length: Option<usize>) -> Result<std::ops::Range<u32>> {
    let length = length.unwrap_or(0);
    let end = u32::try_from(length)
        .ok()
        .filter(|&end| end < u32::MAX)
        .ok_or(Error::invariant(
            "a key list of a typed array beyond the array indices",
        ))?;
    Ok(0..end)
}

/// What a step of the heap's `[[Set]]` walk found at a proxy or a typed
/// array.
enum ExoticSet {
    /// The result of `[[Set]]`.
    Result(SetResult),
    /// A valid element of a typed array that is not the receiver.
    Element,
    /// A key that the typed array treats as an ordinary property.
    Ordinary,
}

impl Heap {
    /// The `[[Get]]` walk at a proxy or typed array (`record`): `Generic`
    /// for a proxy, the result for a numeric key of a typed array, `None`
    /// for another key of a typed array (an ordinary property).
    #[cold]
    #[inline(never)]
    pub(super) fn exotic_get(
        &self,
        record: &Object,
        key: PropertyKey,
    ) -> Result<Option<GetResult>> {
        match &record.kind {
            ObjectKind::TypedArray(data) => match self.numeric_index(key)? {
                Some(index) => Ok(Some(self.typed_get_result(data, index)?)),
                None => Ok(None),
            },
            _ => Ok(Some(GetResult::Generic)),
        }
    }

    /// The `[[Set]]` walk at a proxy or typed array `current` (§10.4.5:
    /// for a numeric key, the receiver's own element converts the value,
    /// which can run script code, so the runtime takes over; another
    /// receiver sees a valid index as a writable data property and an
    /// index that is not valid as a write that did nothing).
    #[cold]
    #[inline(never)]
    fn exotic_set(
        &self,
        record: &Object,
        current: Gc<Object>,
        key: PropertyKey,
        receiver: Value,
    ) -> Result<ExoticSet> {
        let data = match &record.kind {
            ObjectKind::TypedArray(data) => data,
            // A character of a String object: not writable.
            ObjectKind::StringWrapper(_) if self.string_unit_of(record, key)?.is_some() => {
                return Ok(ExoticSet::Result(SetResult::Done(false)));
            }
            ObjectKind::StringWrapper(_) => return Ok(ExoticSet::Ordinary),
            _ => return Ok(ExoticSet::Result(SetResult::Generic)),
        };
        let Some(index) = self.numeric_index(key)? else {
            return Ok(ExoticSet::Ordinary);
        };
        if receiver == Value::Object(current) {
            // The fast path wrote the plain values.
            return Ok(ExoticSet::Result(SetResult::Generic));
        }
        if self.valid_index(data, index)?.is_none() {
            return Ok(ExoticSet::Result(SetResult::Done(true)));
        }
        Ok(ExoticSet::Element)
    }

    /// Step 3 of the heap's `[[Set]]` for a receiver that is a proxy (its
    /// `[[GetOwnProperty]]` and `[[DefineOwnProperty]]` run traps), a
    /// typed array with a numeric key (the definition converts the value)
    /// or a String object with a character at the key (not writable): the
    /// result; `None` for an ordinary property.
    #[cold]
    #[inline(never)]
    fn exotic_receiver(&self, record: &Object, key: PropertyKey) -> Result<Option<SetResult>> {
        Ok(match record.kind {
            ObjectKind::Proxy(_) => Some(SetResult::Generic),
            ObjectKind::TypedArray(_) if self.numeric_index(key)?.is_some() => {
                Some(SetResult::Generic)
            }
            ObjectKind::StringWrapper(_) if self.string_unit_of(record, key)?.is_some() => {
                Some(SetResult::Done(false))
            }
            _ => None,
        })
    }

    /// The result of `[[Get]]` for a numeric index of a typed array: the
    /// element, `undefined` for an index that is not valid, or `Generic`
    /// for an element of a `BigInt` array (a new `BigInt`, which the
    /// runtime allocates).
    fn typed_get_result(&self, data: &TypedArrayData, index: f64) -> Result<GetResult> {
        let Some(index) = self.valid_index(data, index)? else {
            return Ok(GetResult::Value(Value::Undefined));
        };
        Ok(match self.typed_get(data, index)? {
            Some(RawValue::Int(int)) => GetResult::Value(Value::Int(int)),
            Some(RawValue::Number(number)) => GetResult::Value(Value::number(number)),
            Some(RawValue::BigInt64(_) | RawValue::BigUint64(_)) => GetResult::Generic,
            None => GetResult::Value(Value::Undefined),
        })
    }

    /// The fast path of `[[Set]]` for an array index of a typed array that
    /// is its own receiver, with a value that needs no conversion by the
    /// runtime: the element is written if the index is valid, and the
    /// result is true either way (`TypedArraySetElement`). False when the
    /// fast path does not apply.
    #[inline(never)]
    fn typed_set_fast(&mut self, object: Gc<Object>, index: u32, value: Value) -> Result<bool> {
        let ObjectKind::TypedArray(data) = &self.object(object)?.kind else {
            return Ok(false);
        };
        let data = **data;
        let Some(element) = self.plain_element(data.ty, value)? else {
            return Ok(false);
        };
        self.typed_set(&data, index as usize, element)?;
        Ok(true)
    }
}
