//! Private elements on objects (ECMA-262 §7.3.26 `PrivateElementFind`,
//! §7.3.27 `PrivateFieldAdd`, §7.3.28 `PrivateMethodOrAccessorAdd`; the
//! semantics are in `vm/private.rs`).
//!
//! A Private Name (§6.2.12) is a symbol with the private flag. An object
//! keeps its private fields, and the brand of the private methods of a
//! class, as named properties under such a symbol in its shape and slots
//! (ADR 0026 section 15). So instances of a class share the shapes of
//! their private fields, the collector traces them like other properties,
//! and their memory counts like other properties.
//!
//! No script can hold a Private Name: the compiler keeps them in private
//! environments, and [`Heap::own_property_keys`] leaves them out, so
//! `Object.keys`, `for`-`in`, `Reflect.ownKeys`, spread, `freeze` and
//! every other enumeration do not see them. The functions here work on the
//! shape directly, without the internal methods of the object's kind, so
//! they hold for every kind of object (arrays, functions, String objects)
//! and ignore extensibility and integrity levels: a frozen object can get
//! and change private fields.

use crate::error::Result;
use crate::heap::{Arena, Gc, Heap, RootSource};
use crate::object::{Object, Property};
use crate::string::{JsString, PropertyKey};
use crate::value::{Symbol, Value};

impl Heap {
    /// A new Private Name with its description (the name with its `#`).
    pub(crate) fn alloc_private_name(&mut self, description: Gc<JsString>) -> Result<Gc<Symbol>> {
        self.charge(Arena::<Symbol>::slot_size());
        self.arenas.symbols.insert(Symbol {
            description: Some(description),
            private: true,
        })
    }

    /// Whether a property key is a Private Name.
    pub(crate) fn is_private_key(&self, key: PropertyKey) -> bool {
        match key {
            PropertyKey::Symbol(symbol) => self
                .arenas
                .symbols
                .get(symbol)
                .is_ok_and(|symbol| symbol.private),
            _ => false,
        }
    }

    /// The value of the private element `name` of `object`
    /// (`PrivateElementFind`, §7.3.26), or `None` if it has none.
    pub(crate) fn private_find(
        &self,
        object: Gc<Object>,
        name: Gc<Symbol>,
    ) -> Result<Option<Value>> {
        let record = self.object(object)?;
        Ok(
            match self.named_property(record, PropertyKey::Symbol(name))? {
                Some(Property::Data { value, .. }) => Some(value),
                _ => None,
            },
        )
    }

    /// Adds the private element `name` with `value` to `object`, which
    /// does not have it (the caller checks). Can collect: the growth of
    /// the slots is reserved first.
    pub(crate) fn private_add(
        &mut self,
        object: Gc<Object>,
        name: Gc<Symbol>,
        value: Value,
        roots: &dyn RootSource,
    ) -> Result<()> {
        let property = Property::Data {
            value,
            writable: true,
            enumerable: false,
            configurable: false,
        };
        self.add_named(object, PropertyKey::Symbol(name), property, roots)
    }

    /// Replaces the value of the private element `name` of `object`;
    /// returns false if the object does not have it.
    pub(crate) fn private_write(
        &mut self,
        object: Gc<Object>,
        name: Gc<Symbol>,
        value: Value,
    ) -> Result<bool> {
        let key = PropertyKey::Symbol(name);
        let record = self.object(object)?;
        let Some(
            old @ Property::Data {
                writable,
                enumerable,
                configurable,
                ..
            },
        ) = self.named_property(record, key)?
        else {
            return Ok(false);
        };
        let new = Property::Data {
            value,
            writable,
            enumerable,
            configurable,
        };
        self.replace_named(object, key, old, new)?;
        Ok(true)
    }
}
