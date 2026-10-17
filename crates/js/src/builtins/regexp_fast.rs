//! The checks of the fast paths of the `RegExp` methods (ADR 0026 section
//! 15, M7 8b).
//!
//! A fast path skips steps of the specification that no script can
//! observe. The steps read properties of the `RegExp` object, of
//! `%RegExp.prototype%` and of `%RegExp%`; they cannot be observed when
//! all three are as the realm made them:
//!
//! - the object has the shape of a new `RegExp` object: its prototype is
//!   `%RegExp.prototype%` of the realm and its only own property is
//!   `lastIndex`, a writable data property (slot 0);
//! - `%RegExp.prototype%` has its initial shape (no property added,
//!   deleted or redefined) and the built-in values in the slots of `exec`,
//!   `flags`, the eight flag getters, `constructor` and the five symbol
//!   methods;
//! - for the steps of `SpeciesConstructor`, `%RegExp%` has its initial
//!   shape and the built-in `@@species` getter.
//!
//! Then `Get(R, "flags")` gives the flags of the program, `Get(R,
//! "exec")` the built-in `exec`, and `SpeciesConstructor(R, %RegExp%)` is
//! `%RegExp%`. The shapes are recorded when the built-ins are installed.

use swb_js_regexp::Flags;

use crate::heap::Gc;
use crate::object::{Object, ObjectKind, RegExpShapes};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::{Intrinsic, VmError, VmResult, WellKnown};

/// The keys of `%RegExp.prototype%` whose values the fast paths need.
const PROTOTYPE_KEYS: [&str; 11] = [
    "exec",
    "flags",
    "hasIndices",
    "global",
    "ignoreCase",
    "multiline",
    "dotAll",
    "unicode",
    "unicodeSets",
    "sticky",
    "constructor",
];

/// The symbol methods of `%RegExp.prototype%`.
const PROTOTYPE_SYMBOLS: [WellKnown; 5] = [
    WellKnown::Match,
    WellKnown::MatchAll,
    WellKnown::Replace,
    WellKnown::Search,
    WellKnown::Split,
];

/// Records the shapes of a realm after its `RegExp` built-ins exist.
pub(super) fn record_shapes(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let proto = rt.intrinsic(realm, Intrinsic::RegExpPrototype)?;
    let ctor = rt.intrinsic(realm, Intrinsic::RegExpConstructor)?;
    let empty = rt.vm.atoms.empty;
    let flags = rt.vm.atoms.empty;
    let regex = rt.regexp_compile(empty, Flags::empty(), flags)?;
    let sample = rt.new_regexp_object(proto, empty, regex)?;
    let instance = rt.heap.object(sample)?.shape;
    let mut prototype_slots = Vec::new();
    for name in PROTOTYPE_KEYS {
        let key = rt.heap.key_from_str(name)?;
        prototype_slots.push(own_slot(rt, proto, key)?);
    }
    for which in PROTOTYPE_SYMBOLS {
        let key = rt.symbol_key(which);
        prototype_slots.push(own_slot(rt, proto, key)?);
    }
    let species_key = rt.symbol_key(WellKnown::Species);
    let species_slot = own_slot(rt, ctor, species_key)?;
    let shapes = RegExpShapes {
        instance,
        prototype: rt.heap.object(proto)?.shape,
        prototype_slots: prototype_slots.into(),
        constructor: rt.heap.object(ctor)?.shape,
        species_slot,
    };
    if let Some(record) = rt.vm.realms.get_mut(realm as usize) {
        record.regexp.shapes = Some(shapes);
    }
    Ok(())
}

/// The slot of an own named property and its function (the value, or the
/// getter of an accessor).
fn own_slot(rt: &Runtime, object: Gc<Object>, key: PropertyKey) -> VmResult<(u32, Gc<Object>)> {
    let record = rt.heap.object(object)?;
    let shape = rt.heap.shape(record.shape)?;
    let entry = shape
        .find(key, &rt.heap.arenas.key_lists)?
        .ok_or(VmError::invariant("a built-in RegExp property is missing"))?;
    let value = record
        .slots
        .get(entry.slot as usize)
        .copied()
        .ok_or(VmError::invariant("a shape entry outside the slots"))?;
    let Value::Object(function) = value else {
        return Err(VmError::invariant(
            "a built-in RegExp property is not a function",
        ));
    };
    Ok((entry.slot, function))
}

/// Whether slot `slot` of `slots` holds `function`.
fn holds(slots: &[Value], (slot, function): (u32, Gc<Object>)) -> bool {
    matches!(slots.get(slot as usize), Some(&Value::Object(o)) if o == function)
}

impl Runtime {
    /// The recorded shapes of the current realm.
    fn regexp_shapes(&self) -> Option<&RegExpShapes> {
        self.vm
            .realms
            .get(self.current_realm() as usize)?
            .regexp
            .shapes
            .as_ref()
    }

    /// Whether `object` has the shape of a new `RegExp` object of the
    /// current realm (so `lastIndex` is its slot 0, writable).
    fn has_instance_shape(&self, object: Gc<Object>) -> VmResult<bool> {
        let Some(shapes) = self.regexp_shapes() else {
            return Ok(false);
        };
        Ok(self.heap.object(object)?.shape == shapes.instance)
    }

    /// The value of the own `lastIndex` of a `RegExp` object with the
    /// instance shape, without a lookup.
    pub(super) fn own_last_index(&self, object: Gc<Object>) -> VmResult<Option<Value>> {
        if !self.has_instance_shape(object)? {
            return Ok(None);
        }
        Ok(self.heap.object(object)?.slots.first().copied())
    }

    /// Writes the own `lastIndex` of a `RegExp` object with the instance
    /// shape; false for other objects (the caller does the `Set`).
    pub(super) fn write_own_last_index(
        &mut self,
        object: Gc<Object>,
        value: Value,
    ) -> VmResult<bool> {
        if !self.has_instance_shape(object)? {
            return Ok(false);
        }
        match self.heap.object_mut(object)?.slots.first_mut() {
            Some(slot) => {
                *slot = value;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Whether the steps of the symbol methods on `object` cannot be
    /// observed (see the module documentation); `species` also checks
    /// `%RegExp%` for `SpeciesConstructor`.
    pub(super) fn regexp_is_pristine(&self, object: Gc<Object>, species: bool) -> VmResult<bool> {
        let Some(shapes) = self.regexp_shapes() else {
            return Ok(false);
        };
        let record = self.heap.object(object)?;
        if record.shape != shapes.instance || !matches!(record.kind, ObjectKind::RegExp(_)) {
            return Ok(false);
        }
        let realm = self.current_realm();
        let proto = self
            .heap
            .object(self.intrinsic(realm, Intrinsic::RegExpPrototype)?)?;
        if proto.shape != shapes.prototype {
            return Ok(false);
        }
        if !shapes
            .prototype_slots
            .iter()
            .all(|&entry| holds(&proto.slots, entry))
        {
            return Ok(false);
        }
        if species {
            let ctor = self
                .heap
                .object(self.intrinsic(realm, Intrinsic::RegExpConstructor)?)?;
            if ctor.shape != shapes.constructor || !holds(&ctor.slots, shapes.species_slot) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Whether `Get(R, "exec")` gives the built-in `exec` of the current
    /// realm without a lookup: `R` is a `RegExp` object with the instance
    /// shape, and `%RegExp.prototype%` has its initial shape and the
    /// built-in `exec` (the first of the recorded slots).
    pub(super) fn has_builtin_exec(&self, object: Gc<Object>) -> VmResult<bool> {
        let Some(shapes) = self.regexp_shapes() else {
            return Ok(false);
        };
        let record = self.heap.object(object)?;
        if record.shape != shapes.instance || !matches!(record.kind, ObjectKind::RegExp(_)) {
            return Ok(false);
        }
        let proto = self.intrinsic(self.current_realm(), Intrinsic::RegExpPrototype)?;
        let proto = self.heap.object(proto)?;
        let Some(&exec) = shapes.prototype_slots.first() else {
            return Ok(false);
        };
        Ok(proto.shape == shapes.prototype && holds(&proto.slots, exec))
    }

    /// The instance shape of the current realm, for a new `RegExp` object
    /// whose prototype is `proto` (`None` for another prototype).
    pub(super) fn instance_shape_for(
        &self,
        proto: Gc<Object>,
    ) -> VmResult<Option<Gc<crate::object::Shape>>> {
        let Some(shapes) = self.regexp_shapes() else {
            return Ok(None);
        };
        let regexp_proto = self.intrinsic(self.current_realm(), Intrinsic::RegExpPrototype)?;
        Ok((proto == regexp_proto).then_some(shapes.instance))
    }

    /// The flags of a pristine `RegExp` object (what `Get(R, "flags")`
    /// would give), or `None`.
    pub(super) fn pristine_flags(&self, object: Gc<Object>) -> VmResult<Option<Flags>> {
        if !self.regexp_is_pristine(object, false)? {
            return Ok(None);
        }
        match &self.heap.object(object)?.kind {
            ObjectKind::RegExp(data) => Ok(Some(data.regex.flags())),
            _ => Ok(None),
        }
    }
}
