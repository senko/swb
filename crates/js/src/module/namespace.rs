//! Module namespace exotic objects (§10.4.6,
//! <https://tc39.es/ecma262/2025/#sec-module-namespace-exotic-objects>;
//! ADR 0034 section 2) and `GetModuleNamespace` (§16.2.1.12).
//!
//! A namespace object is `ObjectKind::ModuleNamespace`: the export names
//! that resolve unambiguously, sorted by code units, each with the cell of
//! its binding (resolved once, when the object is made: `ResolveExport` gives
//! the same answer each time). `[[Prototype]]` is null, the object is not
//! extensible, and its only stored property is `@@toStringTag` `"Module"`.
//! The internal methods for string keys are the hooks [`NAMESPACE_HOOKS`],
//! which `Runtime::host_hooks` gives for this kind, so the runtime's
//! generic paths treat a namespace as a host object with hooks (the inline
//! caches never store it). `[[HasProperty]]` reads no binding
//! ([`Runtime::namespace_has`]). A read of a binding in its dead zone
//! throws the `ReferenceError`; the refused `[[Set]]` and `[[Delete]]` have
//! Chromium's messages (measured: `Cannot assign to property 'b' of
//! [object Module]`, `Cannot delete property 'b' of [object Module]`).
//!
//! The cell of a namespace (for `import * as ns` and `export * as ns`)
//! exists before the object: making the namespace of a module whose
//! exports reach the namespace of another module needs only that module's
//! cell, and a work list makes the missing objects, so cycles of `export *
//! as` need no recursion.

use std::collections::HashMap;

use crate::heap::{Gc, Generic, Tracer};
use crate::host::{HostHooks, Refusal, SetAnswer};
use crate::object::{Object, ObjectKind, Property, PropertyDescriptor};
use crate::runtime::Runtime;
use crate::string::{JsString, PropertyKey};
use crate::value::{Value, ValueCell};
use crate::vm::{VmError, VmResult, WellKnown};

use super::{Resolved, Target};

/// The data of a namespace object (`[[Module]]` and `[[Exports]]`).
pub struct NamespaceData {
    /// The module.
    pub(crate) module: Gc<Generic>,
    /// The export names, sorted by code units.
    pub(crate) names: Box<[Gc<JsString>]>,
    /// The cell of each name's binding.
    pub(crate) cells: Box<[Gc<ValueCell>]>,
    /// The position of each name.
    pub(crate) index: HashMap<Gc<JsString>, u32>,
}

impl NamespaceData {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.generic(self.module);
        for &name in &self.names {
            tracer.string(name);
        }
        for &cell in &self.cells {
            tracer.cell(cell);
        }
    }

    pub(crate) fn heap_size(&self) -> usize {
        size_of::<NamespaceData>()
            + self.names.len() * size_of::<Gc<JsString>>()
            + self.cells.len() * size_of::<Gc<ValueCell>>()
            + self.index.capacity() * (size_of::<(Gc<JsString>, u32)>() + 8)
    }
}

/// The internal methods of namespace objects (§10.4.6.2 to §10.4.6.11).
pub(crate) const NAMESPACE_HOOKS: HostHooks = HostHooks {
    get_own_property: Some(get_own_property),
    define_own_property: Some(define_own_property),
    get: Some(get),
    set: Some(set),
    delete: Some(delete),
    own_keys: Some(own_keys),
    prevent_extensions: Some(prevent_extensions),
    refusal: Some(refusal),
};

/// The position of the export name of a string key, `None` for a symbol
/// or a name that the module does not export.
fn export_position(
    rt: &mut Runtime,
    object: Gc<Object>,
    key: PropertyKey,
) -> VmResult<Option<u32>> {
    let name = match key {
        PropertyKey::Symbol(_) => return Ok(None),
        PropertyKey::String(name) => name,
        PropertyKey::Index(index) => rt.heap.intern(crate::string::decimal(index).as_str16())?,
    };
    match &rt.heap.object(object)?.kind {
        ObjectKind::ModuleNamespace(data) => Ok(data.index.get(&name).copied()),
        _ => Err(VmError::invariant("a namespace hook on another object")),
    }
}

/// Which internal method reads a binding, for the text of the error
/// that a binding in its dead zone throws (Chromium 148's, measured).
#[derive(Clone, Copy)]
enum Zone {
    /// `[[Get]]`.
    Get,
    /// `[[GetOwnProperty]]` and `[[DefineOwnProperty]]`.
    Own,
    /// The key enumeration of the namespace object itself (`Object.keys`,
    /// `for-in`, `JSON.stringify`).
    Listing,
}

/// The value of the binding of an export (§10.4.6.8 `[[Get]]` steps 3 to
/// 12): a binding in its dead zone throws.
fn binding_value(rt: &Runtime, object: Gc<Object>, position: u32, zone: Zone) -> VmResult<Value> {
    let ObjectKind::ModuleNamespace(data) = &rt.heap.object(object)?.kind else {
        return Err(VmError::invariant("a namespace hook on another object"));
    };
    let (Some(&cell), Some(&name)) = (
        data.cells.get(position as usize),
        data.names.get(position as usize),
    ) else {
        return Err(VmError::invariant("a namespace export outside the list"));
    };
    let value = rt.heap.cell(cell)?.value;
    if value == Value::Empty {
        let name = rt.heap.string_lossy(name, usize::MAX);
        return Err(VmError::reference_error(match zone {
            Zone::Get => format!(
                "Failed to read the '{name}' property from 'Module': Cannot access '{name}' before initialization"
            ),
            Zone::Own => format!("{name} is not defined"),
            Zone::Listing => format!("Cannot access '{name}' before initialization"),
        }));
    }
    Ok(value)
}

/// `[[GetOwnProperty]](P)` (§10.4.6.5).
fn get_own_property(
    rt: &mut Runtime,
    object: Gc<Object>,
    key: PropertyKey,
) -> VmResult<Option<Property>> {
    let Some(position) = export_position(rt, object, key)? else {
        return Ok(None);
    };
    let value = binding_value(rt, object, position, Zone::Own)?;
    Ok(Some(Property::Data {
        value,
        writable: true,
        enumerable: true,
        configurable: false,
    }))
}

/// `[[DefineOwnProperty]](P, Desc)` (§10.4.6.6).
fn define_own_property(
    rt: &mut Runtime,
    object: Gc<Object>,
    key: PropertyKey,
    desc: &PropertyDescriptor,
) -> VmResult<Option<bool>> {
    if matches!(key, PropertyKey::Symbol(_)) {
        return Ok(None);
    }
    let Some(position) = export_position(rt, object, key)? else {
        return Ok(Some(false));
    };
    let current = binding_value(rt, object, position, Zone::Own)?;
    if desc.configurable == Some(true)
        || desc.enumerable == Some(false)
        || desc.get.is_some()
        || desc.set.is_some()
        || desc.writable == Some(false)
    {
        return Ok(Some(false));
    }
    match desc.value {
        Some(value) => Ok(Some(rt.heap.same_value(value, current)?)),
        None => Ok(Some(true)),
    }
}

/// `[[Get]](P, Receiver)` (§10.4.6.8).
fn get(
    rt: &mut Runtime,
    object: Gc<Object>,
    key: PropertyKey,
    _: Value,
) -> VmResult<Option<Value>> {
    if matches!(key, PropertyKey::Symbol(_)) {
        return Ok(None);
    }
    match export_position(rt, object, key)? {
        Some(position) => binding_value(rt, object, position, Zone::Get).map(Some),
        None => Ok(Some(Value::Undefined)),
    }
}

/// `[[Set]]` (§10.4.6.9): always false.
#[allow(clippy::unnecessary_wraps, reason = "the signature of the hook")]
fn set(_: &mut Runtime, _: Gc<Object>, _: PropertyKey, _: Value, _: Value) -> VmResult<SetAnswer> {
    Ok(SetAnswer::Done(false))
}

/// `[[Delete]](P)` (§10.4.6.10).
fn delete(rt: &mut Runtime, object: Gc<Object>, key: PropertyKey) -> VmResult<Option<bool>> {
    if matches!(key, PropertyKey::Symbol(_)) {
        return Ok(None);
    }
    Ok(Some(export_position(rt, object, key)?.is_none()))
}

/// `[[OwnPropertyKeys]]` (§10.4.6.11): the export names in their order;
/// the runtime appends the symbol keys.
fn own_keys(rt: &mut Runtime, object: Gc<Object>, keys: &mut Vec<PropertyKey>) -> VmResult<()> {
    let names = match &rt.heap.object(object)?.kind {
        ObjectKind::ModuleNamespace(data) => data.names.clone(),
        _ => return Err(VmError::invariant("a namespace hook on another object")),
    };
    keys.try_reserve(names.len())
        .map_err(|_| VmError::range_error("Invalid array length"))?;
    for name in names {
        keys.push(rt.heap.key_from_string(name)?);
    }
    Ok(())
}

/// `[[PreventExtensions]]` (§10.4.6.4): true.
#[allow(clippy::unnecessary_wraps, reason = "the signature of the hook")]
fn prevent_extensions(_: &mut Runtime, _: Gc<Object>) -> VmResult<Option<bool>> {
    Ok(Some(true))
}

/// Chromium's messages for a refused `[[Set]]` and `[[Delete]]`.
#[allow(clippy::unnecessary_wraps, reason = "the signature of the hook")]
fn refusal(
    rt: &mut Runtime,
    _: Gc<Object>,
    key: PropertyKey,
    why: Refusal,
) -> VmResult<Option<String>> {
    let key_is_symbol = matches!(key, PropertyKey::Symbol(_));
    let key = rt.key_text(key);
    Ok(match why {
        Refusal::Set => Some(format!(
            "Cannot assign to property '{key}' of [object Module]"
        )),
        Refusal::Delete => Some(format!("Cannot delete property '{key}' of [object Module]")),
        // A symbol key keeps the generic text.
        Refusal::Define if key_is_symbol => None,
        Refusal::Define => Some(format!("Cannot redefine property: {key}")),
    })
}

impl Runtime {
    /// `[[GetOwnProperty]](P)` of any object that needs the runtime, for
    /// the key enumeration of `Object.keys`, `for-in` and `JSON.stringify`.
    /// Only a binding of a namespace object in its dead zone differs: its
    /// error has the text of the enumeration, not of `[[GetOwnProperty]]`.
    pub(crate) fn exotic_own_property_listed(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
    ) -> VmResult<Option<Property>> {
        if matches!(
            self.heap.object(object)?.kind,
            ObjectKind::ModuleNamespace(_)
        ) && let Some(position) = export_position(self, object, key)?
        {
            binding_value(self, object, position, Zone::Listing)?;
        }
        self.exotic_own_property(object, key)
    }

    /// `[[HasProperty]](P)` of a namespace object (§10.4.6.7): whether the
    /// module exports a string key, without reading the binding; `None`
    /// for a symbol (the ordinary method) or another object.
    pub(crate) fn namespace_has(
        &mut self,
        object: Gc<Object>,
        key: PropertyKey,
    ) -> VmResult<Option<bool>> {
        if !matches!(
            self.heap.object(object)?.kind,
            ObjectKind::ModuleNamespace(_)
        ) || matches!(key, PropertyKey::Symbol(_))
        {
            return Ok(None);
        }
        Ok(Some(export_position(self, object, key)?.is_some()))
    }

    /// `GetModuleNamespace(module)` (§16.2.1.12): the namespace object,
    /// made on first use with the namespaces that its exports need.
    pub(crate) fn namespace_object(&mut self, module: Gc<Generic>) -> VmResult<Gc<Object>> {
        if let Some(namespace) = self.record(module)?.namespace {
            return Ok(namespace);
        }
        let mut work = vec![module];
        self.make_namespaces(&mut work)?;
        self.record(module)?
            .namespace
            .ok_or(VmError::invariant("a namespace that was not made"))
    }

    /// The cell that holds the namespace object of `module` (made on first
    /// use, with the object).
    pub(crate) fn namespace_cell(&mut self, module: Gc<Generic>) -> VmResult<Gc<ValueCell>> {
        let mut work = Vec::new();
        let cell = self.namespace_cell_pending(module, &mut work)?;
        self.make_namespaces(&mut work)?;
        Ok(cell)
    }

    /// The namespace cell of `module`; a new one holds the object if it
    /// exists, else `Empty`, and the module goes on the work list.
    fn namespace_cell_pending(
        &mut self,
        module: Gc<Generic>,
        work: &mut Vec<Gc<Generic>>,
    ) -> VmResult<Gc<ValueCell>> {
        let record = self.record(module)?;
        if let Some(cell) = record.namespace_cell {
            return Ok(cell);
        }
        let namespace = record.namespace;
        let cell = self
            .heap
            .alloc_cell(namespace.map_or(Value::Empty, Value::Object))?;
        self.record_mut(module)?.namespace_cell = Some(cell);
        if namespace.is_none() {
            work.push(module);
        }
        Ok(cell)
    }

    /// Makes the namespace objects of the modules on the work list (and of
    /// the modules that their exports add).
    fn make_namespaces(&mut self, work: &mut Vec<Gc<Generic>>) -> VmResult<()> {
        while let Some(module) = work.pop() {
            if self.record(module)?.namespace.is_none() {
                self.make_namespace(module, work)?;
            }
        }
        Ok(())
    }

    /// `ModuleNamespaceCreate(module, exports)` (§10.4.6.12) with the
    /// unambiguous export names (§16.2.1.12 steps 3.a to 3.d).
    fn make_namespace(&mut self, module: Gc<Generic>, work: &mut Vec<Gc<Generic>>) -> VmResult<()> {
        let names = self.exported_names(module)?;
        let mut entries: Vec<(Gc<JsString>, Gc<ValueCell>)> = Vec::with_capacity(names.len());
        for name in names {
            let Resolved::Found(owner, target) = self.resolve_export(module, name)? else {
                continue;
            };
            let cell = match target {
                Target::Cell(_) => self.binding_cell(owner, target)?,
                Target::Namespace => self.namespace_cell_pending(owner, work)?,
            };
            entries.push((name, cell));
        }
        self.charge(entries.len())?;
        let heap = &self.heap;
        let mut failed = None;
        entries.sort_by(|&(a, _), &(b, _)| {
            let flat = |name| heap.string(name).ok().and_then(JsString::as_flat);
            if let (Some(a), Some(b)) = (flat(a), flat(b)) {
                a.units().cmp(b.units())
            } else {
                failed = Some(VmError::invariant("an export name that is not flat"));
                std::cmp::Ordering::Equal
            }
        });
        if let Some(error) = failed {
            return Err(error);
        }
        let index = entries
            .iter()
            .enumerate()
            .map(|(position, &(name, _))| (name, position as u32))
            .collect();
        let data = NamespaceData {
            module,
            names: entries.iter().map(|&(name, _)| name).collect(),
            cells: entries.iter().map(|&(_, cell)| cell).collect(),
            index,
        };
        let namespace = self.scoped(|rt| {
            let object = rt
                .heap
                .new_object_with_kind(None, ObjectKind::ModuleNamespace(Box::new(data)))?;
            rt.heap.record(object);
            let tag = rt.heap.intern_str("Module")?;
            let key = rt.symbol_key(WellKnown::ToStringTag);
            let property = Property::Data {
                value: tag.into(),
                writable: false,
                enumerable: false,
                configurable: false,
            };
            rt.heap.add_named(object, key, property, &rt.vm)?;
            rt.heap.object_mut(object)?.extensible = false;
            Ok(object)
        })?;
        let record = self.record_mut(module)?;
        record.namespace = Some(namespace);
        if let Some(cell) = record.namespace_cell {
            self.heap.cell_mut(cell)?.value = Value::Object(namespace);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{linked, runtime};
    use super::*;

    #[test]
    fn names_are_sorted_by_code_units() {
        let mut rt = runtime(
            &[(
                "m.js",
                "const v = 1; export { v as 'é', v as b, v as B, v as '10', v as '9', v as 'a' };",
            )],
            true,
        );
        let m = linked(&mut rt, "m.js");
        let Value::Object(namespace) = rt.module_namespace(m).expect("a namespace") else {
            panic!("not an object");
        };
        let ObjectKind::ModuleNamespace(data) = &rt.heap.object(namespace).expect("live").kind
        else {
            panic!("not a namespace");
        };
        let names: Vec<String> = data
            .names
            .iter()
            .map(|&name| rt.heap.string_lossy(name, usize::MAX))
            .collect();
        assert_eq!(names, ["10", "9", "B", "a", "b", "é"]);
        // An index key finds a numeric name.
        assert_eq!(
            export_position(&mut rt, namespace, PropertyKey::Index(9)).expect("a lookup"),
            Some(1)
        );
        assert!(rt.heap.object(namespace).expect("live").kind.walks_exotic());
    }

    #[test]
    fn the_cell_of_a_namespace_holds_its_object() {
        let mut rt = runtime(
            &[
                ("a.js", "export * as b from './b.js';"),
                ("b.js", "export * as a from './a.js';"),
            ],
            true,
        );
        let a = linked(&mut rt, "a.js");
        let Value::Object(namespace) = rt.module_namespace(a).expect("a namespace") else {
            panic!("not an object");
        };
        let cell = rt
            .record(a.0)
            .expect("a record")
            .namespace_cell
            .expect("made");
        assert_eq!(
            rt.heap.cell(cell).expect("a cell").value,
            Value::Object(namespace)
        );
    }
}
