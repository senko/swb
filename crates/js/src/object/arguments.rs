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
//! Only `create_arguments` (`vm/property.rs`) stores cells in elements;
//! the elements are read only through `[[GetOwnProperty]]` in `ops.rs`,
//! which takes the cell's value, so no cell reaches a script. `[[Get]]`
//! and `[[Set]]` of §10.4.4.3 and §10.4.4.4 follow from it, from
//! `[[DefineOwnProperty]]` here and from the fast path of `[[Set]]`, which
//! writes the cell.

use crate::error::{Error, Result};
use crate::heap::{Gc, Heap, RootSource};
use crate::object::{Elements, Object, ObjectKind, Property, PropertyDescriptor};
use crate::string::PropertyKey;
use crate::value::{Value, ValueCell};

impl Heap {
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
