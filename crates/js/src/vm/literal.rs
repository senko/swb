//! The run-time parts of literals: accessors of object literals
//! (§15.4.5), the names of functions with computed keys (§10.2.9
//! `SetFunctionName`), object spread and object rest (§7.3.25
//! `CopyDataProperties`) and the template objects of tagged templates
//! (§13.2.8.4 `GetTemplateObject`).

use std::collections::HashSet;
use std::rc::Rc;

use swb_js_text::String16;
use swb_js_text::hash::BuildKeyedHasher;

use crate::bytecode::{
    CodeObject, Constant, FunctionCode, NAME_PREFIX_GET, NAME_PREFIX_SET, TEMPLATE_ENTRY_BYTES,
};
use crate::heap::{Gc, Generic};
use crate::object::{Object, PropertyDescriptor};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::{Intrinsic, VmError, VmResult};

impl Runtime {
    /// Defines the getter (or setter) `function` of an object literal:
    /// `{ [[Get]]: function, [[Enumerable]]: true, [[Configurable]]: true }`
    /// with `DefinePropertyOrThrow`, so a getter and a setter of one key
    /// make one property (§15.4.5 `MethodDefinitionEvaluation`).
    pub(crate) fn define_literal_accessor(
        &mut self,
        object: Gc<Object>,
        key: Value,
        function: Value,
        getter: bool,
    ) -> VmResult<()> {
        let key = self.key_from_converted(key)?;
        let mut desc = PropertyDescriptor {
            enumerable: Some(true),
            configurable: Some(true),
            ..PropertyDescriptor::default()
        };
        if getter {
            desc.get = Some(function);
        } else {
            desc.set = Some(function);
        }
        if self.heap.define_own_property(object, key, desc, &self.vm)? {
            Ok(())
        } else {
            Err(VmError::type_error(format!(
                "Cannot redefine property: {}",
                self.key_text(key)
            )))
        }
    }

    /// `SetFunctionName(function, key, prefix)` (§10.2.9) for a key known
    /// at run time: a symbol gives `[description]` (or the empty string),
    /// a prefix gives `get name` or `set name`. Redefines the `name`
    /// property that the closure got at its creation.
    pub(crate) fn set_function_name(
        &mut self,
        function: Value,
        key: Value,
        prefix: u32,
    ) -> VmResult<()> {
        let Value::Object(function) = function else {
            return Err(VmError::invariant(
                "SetFunctionName of a value that is not a function",
            ));
        };
        let mut name = match prefix {
            NAME_PREFIX_GET => String16::from("get "),
            NAME_PREFIX_SET => String16::from("set "),
            _ => String16::default(),
        };
        if let Value::Symbol(symbol) = key {
            if let Some(description) = self.heap.symbol(symbol)?.description {
                name.push(u16::from(b'['));
                name.push_str16(self.heap.string(description)?.as_str16());
                name.push(u16::from(b']'));
            }
        } else {
            let key = self.key_from_converted(key)?;
            let Value::String(text) = self.heap.key_to_value(key)? else {
                return Err(VmError::invariant("a string key without a string"));
            };
            name.push_str16(self.heap.string(text)?.as_str16());
        }
        // The copy of a long key is work in proportion to its length.
        self.charge_units(name.len())?;
        let name = self.heap.alloc_string(name)?;
        let key = PropertyKey::String(self.vm.atoms.name);
        self.define(function, key, name.into(), false, false, true)
    }

    /// `CopyDataProperties(target, source, excluded)` (§7.3.25) for
    /// object spread and object rest: each own enumerable key of
    /// `ToObject(source)` that is not in `excluded`, in the order of
    /// `[[OwnPropertyKeys]]`, read with `[[Get]]` (a getter runs) and
    /// defined on `target`. `undefined` and `null` copy nothing. Runs
    /// inside a handle scope.
    pub(crate) fn copy_data_properties(
        &mut self,
        target: Gc<Object>,
        source: Value,
        excluded: &HashSet<PropertyKey, BuildKeyedHasher>,
    ) -> VmResult<()> {
        if source.is_nullish() {
            return Ok(());
        }
        let from = self.to_object(source)?;
        self.heap.record(from);
        let keys = self.own_keys(from)?;
        self.heap.record_keys(&keys);
        for key in keys {
            if excluded.contains(&key) {
                continue;
            }
            self.with_scope(|rt| {
                let Some(property) = rt.own_property(from, key)? else {
                    return Ok(());
                };
                if !property.enumerable() {
                    return Ok(());
                }
                let value = rt.get_value(from.into(), key)?;
                if rt.heap.create_data_property(target, key, value, &rt.vm)? {
                    Ok(())
                } else {
                    Err(VmError::type_error(format!(
                        "Cannot redefine property: {}",
                        rt.key_text(key)
                    )))
                }
            })?;
        }
        Ok(())
    }

    /// `GetTemplateObject` (§13.2.8.4): the template object of the
    /// template constant `index` of `compiled` (whose code object is
    /// `code`) in `realm`. The first evaluation creates it (a frozen
    /// array of the cooked strings, `undefined` for an invalid escape,
    /// with a frozen array of the raw strings as `raw`); the code object
    /// keeps it for later evaluations. Runs inside a handle scope.
    pub(crate) fn template_object(
        &mut self,
        code: Gc<Generic>,
        compiled: &Rc<FunctionCode>,
        index: u32,
        realm: u32,
    ) -> VmResult<Value> {
        let cached = self
            .heap
            .generic::<CodeObject>(code)?
            .templates
            .borrow()
            .get(&(index, realm))
            .copied();
        if let Some(template) = cached {
            return Ok(template.into());
        }
        let Some(Constant::Template(parts)) = compiled.constants.get(index as usize) else {
            return Err(VmError::invariant(
                "a template constant that is not a template",
            ));
        };
        let proto = self.intrinsic(realm, Intrinsic::ArrayPrototype)?;
        let template = self.heap.new_array(Some(proto), 0)?;
        self.heap.record(template);
        let raw = self.heap.new_array(Some(proto), 0)?;
        self.heap.record(raw);
        for (i, part) in parts.iter().enumerate() {
            let key = PropertyKey::Index(i as u32);
            let cooked = part.cooked.map_or(Value::Undefined, Value::String);
            self.define(template, key, cooked, false, true, false)?;
            self.define(raw, key, Value::String(part.raw), false, true, false)?;
        }
        let raw_key = PropertyKey::String(self.vm.atoms.raw);
        self.define(template, raw_key, raw.into(), false, false, false)?;
        // `SetIntegrityLevel(frozen)` of both arrays: the elements and
        // `raw` are already read-only and not configurable.
        let length = PropertyKey::String(self.heap.length_atom());
        let count = Value::number(parts.len() as f64);
        for array in [template, raw] {
            self.define(array, length, count, false, false, false)?;
            self.heap.prevent_extensions(array)?;
        }
        let grown = {
            let code_object = self.heap.generic::<CodeObject>(code)?;
            let mut templates = code_object.templates.borrow_mut();
            let before = templates.capacity();
            templates.insert((index, realm), template);
            templates.capacity().saturating_sub(before)
        };
        self.heap.charge(grown * TEMPLATE_ENTRY_BYTES);
        Ok(template.into())
    }
}
