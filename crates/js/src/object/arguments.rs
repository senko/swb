//! The internal methods of mapped arguments objects (ECMA-262 §10.4.4,
//! <https://tc39.es/ecma262/#sec-arguments-exotic-objects>).
//!
//! A mapped arguments object of a sloppy function with a simple parameter
//! list maps each index below the number of arguments whose parameter is
//! mapped (the last parameter of each name) to the parameter's binding.
//! All parameters of such a function are cells (see `js-syntax`
//! `FunctionScope::mapped_arguments`), and the element of a mapped index
//! holds the cell itself (`Value::Cell`): the element is the
//! `[[ParameterMap]]` entry. Reads of the element take the cell's value
//! (§10.4.4.1), writes go to the cell, and an index stops being mapped when
//! its element is deleted (§10.4.4.5), redefined as an accessor or made
//! non-writable (§10.4.4.2), which replaces the cell by a plain value. So
//! the map costs no memory, and no element value is ever stale.
//!
//! Creation (M7 13b): the first arguments object of a realm is made with
//! the general definitions; its shape and slot values become the realm's
//! [`ArgumentsTemplate`] (one for sloppy functions, with `callee` as a data
//! property, one for strict functions, with the `%ThrowTypeError%`
//! accessor). Later objects are made in that shape at once, with all
//! elements in one dense vector ([`Heap::new_arguments_object`]).
//!
//! Only `create_arguments` (`vm/property.rs`) stores cells in elements;
//! the elements are read only through `[[GetOwnProperty]]` in `ops.rs`,
//! which takes the cell's value, so no cell reaches a script. `[[Get]]`
//! and `[[Set]]` of §10.4.4.3 and §10.4.4.4 follow from it, from
//! `[[DefineOwnProperty]]` here and from the fast path of `[[Set]]`, which
//! writes the cell.

use crate::error::{Error, Result};
use crate::heap::{Gc, Heap, RootSource, Tracer};
use crate::object::shape::ShapeKind;
use crate::object::{Elements, Object, ObjectKind, Property, PropertyDescriptor, Shape};
use crate::string::PropertyKey;
use crate::value::{Value, ValueCell};

/// The shape and the named property values of the arguments objects of a
/// realm, taken from the first one that the general definitions made:
/// `length`, `@@iterator` and `callee`, all in their final shape.
pub(crate) struct ArgumentsTemplate {
    /// The shape (shared; its prototype is the realm's
    /// `%Object.prototype%`).
    pub(crate) shape: Gc<Shape>,
    /// The slot values; the slots of `length` and of a data `callee` are
    /// replaced for each object (the template holds `undefined` there).
    pub(crate) slots: Box<[Value]>,
    /// The slot of `length`.
    pub(crate) length_slot: usize,
    /// The slot of `callee` when it is a data property (sloppy
    /// functions).
    pub(crate) callee_slot: Option<usize>,
}

impl ArgumentsTemplate {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.shape(self.shape);
        for value in &self.slots {
            tracer.value(*value);
        }
    }
}

impl Heap {
    /// The template of the arguments object `object`, which the general
    /// definitions just made (`length`, `@@iterator`, `callee` and only
    /// elements otherwise). `None` if its shape is not shared.
    pub(crate) fn arguments_template(
        &self,
        object: Gc<Object>,
        callee: PropertyKey,
    ) -> Result<Option<ArgumentsTemplate>> {
        let record = self.object(object)?;
        let shape = self.arenas.shapes.get(record.shape)?;
        if !matches!(shape.kind, ShapeKind::Shared(_)) {
            return Ok(None);
        }
        let lists = &self.arenas.key_lists;
        let length = PropertyKey::String(self.length_atom());
        let Some(length) = shape.find(length, lists)? else {
            return Ok(None);
        };
        let Some(callee) = shape.find(callee, lists)? else {
            return Ok(None);
        };
        let callee_slot = (!callee.flags.accessor()).then_some(callee.slot as usize);
        let mut slots: Box<[Value]> = record.slots.clone().into_boxed_slice();
        for slot in [Some(length.slot as usize), callee_slot]
            .into_iter()
            .flatten()
        {
            *slots
                .get_mut(slot)
                .ok_or(Error::invariant("a shape slot outside the object"))? = Value::Undefined;
        }
        Ok(Some(ArgumentsTemplate {
            shape: record.shape,
            slots,
            length_slot: length.slot as usize,
            callee_slot,
        }))
    }

    /// A new arguments object in the shape of `template`, with `length`
    /// = the number of `elements`, `callee` (for a template with a data
    /// `callee`) and the elements (values, or the cells of mapped
    /// parameters; no holes). Never collects.
    pub(crate) fn new_arguments_object(
        &mut self,
        template: &ArgumentsTemplate,
        callee: Value,
        elements: Vec<Value>,
    ) -> Result<Gc<Object>> {
        let mut slots = template.slots.to_vec();
        let length = Value::number(elements.len() as f64);
        for (slot, value) in [
            (Some(template.length_slot), length),
            (template.callee_slot, callee),
        ] {
            if let Some(slot) = slot {
                *slots
                    .get_mut(slot)
                    .ok_or(Error::invariant("a template slot outside the slots"))? = value;
            }
        }
        self.new_object_with_shape(template.shape, slots, elements, ObjectKind::Arguments)
    }

    /// The cell of the mapped element `key` of `object`, if it is one.
    pub(crate) fn mapped_argument(
        &self,
        object: Gc<Object>,
        key: PropertyKey,
    ) -> Result<Option<Gc<ValueCell>>> {
        let PropertyKey::Index(index) = key else {
            return Ok(None);
        };
        let record = self.object(object)?;
        if !matches!(record.kind, ObjectKind::Arguments) {
            return Ok(None);
        }
        Ok(match record.elements.get(index) {
            Some(Property::Data {
                value: Value::Cell(cell),
                ..
            }) => Some(cell),
            _ => None,
        })
    }

    /// Maps the element `index` of a new arguments object to the cell of
    /// its parameter: the element (a default data property that holds the
    /// parameter's value) holds the cell from now on.
    pub(crate) fn map_argument(
        &mut self,
        object: Gc<Object>,
        index: u32,
        cell: Gc<ValueCell>,
    ) -> Result<()> {
        self.cell(cell)?;
        match &mut self.object_mut(object)?.elements {
            Elements::Dense { values, .. } => match values.get_mut(index as usize) {
                Some(slot) if !slot.is_empty() => {
                    *slot = Value::Cell(cell);
                    Ok(())
                }
                _ => Err(Error::invariant("a mapped argument without its element")),
            },
            Elements::Sparse(_) => Err(Error::invariant(
                "the elements of a new arguments object are sparse",
            )),
        }
    }

    /// The property of an own element whose value is a cell: the mapped
    /// argument with its parameter's value (§10.4.4.1 step 3). Out of
    /// line, so that `[[GetOwnProperty]]` stays small.
    #[inline(never)]
    pub(crate) fn mapped_property(
        &self,
        property: Property,
        cell: Gc<ValueCell>,
    ) -> Result<Property> {
        Ok(match property {
            Property::Data {
                writable,
                enumerable,
                configurable,
                ..
            } => Property::Data {
                value: self.cell(cell)?.value,
                writable,
                enumerable,
                configurable,
            },
            accessor @ Property::Accessor { .. } => accessor,
        })
    }

    /// `[[DefineOwnProperty]]` of a mapped arguments object for a mapped
    /// index (§10.4.4.2). Can collect.
    #[inline(never)]
    pub(crate) fn arguments_define_own_property(
        &mut self,
        object: Gc<Object>,
        (index, cell): (u32, Gc<ValueCell>),
        desc: PropertyDescriptor,
        roots: &dyn RootSource,
    ) -> Result<bool> {
        let key = PropertyKey::Index(index);
        let mut new_desc = desc;
        // Step 3: a non-writable redefinition without a value keeps the
        // parameter's current value.
        if !desc.is_accessor() && desc.value.is_none() && desc.writable == Some(false) {
            new_desc.value = Some(self.cell(cell)?.value);
        }
        // Step 4 replaces the element (the cell) by a plain property,
        // unless nothing changes.
        if !self.ordinary_define_own_property(object, key, new_desc, roots)? {
            return Ok(false);
        }
        if desc.is_accessor() {
            // Step 6.a: the mapping ends with the accessor.
            return Ok(true);
        }
        // Step 6.b.i: the parameter gets the value.
        if let Some(value) = desc.value {
            self.cell_mut(cell)?.value = value;
        }
        if desc.writable == Some(false) {
            // Step 6.b.ii: the mapping ends; the element holds the value.
            return Ok(true);
        }
        // The index stays mapped: the element holds the cell again, with
        // the attributes that the definition gave it.
        let record = self.object_mut(object)?;
        if let Some(Property::Data {
            writable,
            enumerable,
            configurable,
            ..
        }) = record.elements.get(index)
        {
            record.elements.replace(
                index,
                Property::Data {
                    value: Value::Cell(cell),
                    writable,
                    enumerable,
                    configurable,
                },
            )?;
        }
        Ok(true)
    }
}
