//! Private names at run time: Private Names (ECMA-262 §6.2.12), the
//! private environment of a class evaluation (§9.2), `PrivateGet`,
//! `PrivateSet`, `PrivateFieldAdd` and `PrivateMethodOrAccessorAdd`
//! (§7.3.26 to §7.3.32), and `#x in o` (§13.10.1), with V8's messages.
//!
//! Each evaluation of a class body creates a private environment: an
//! internal object with a new Private Name (a private symbol, see
//! `object/private.rs`) for each private name of the class. The class's
//! code reaches it through one binding of the class body (the scope
//! analysis gives each private name an index into it).
//!
//! Private fields are private elements of each object. Private methods and
//! accessors are the same for all objects of a class evaluation, so the
//! environment holds them, and an object that has them carries one private
//! element instead: the brand of the class (as in V8; not observable,
//! because `InitializeInstanceElements` adds all instance methods of a
//! class at once, §7.3.34). Static private methods and accessors belong to
//! the class constructor alone, so the check is the identity of the
//! constructor.

use crate::bytecode::{PrivateKind, PrivateNames};
use crate::heap::{Arena, Gc, Tracer};
use crate::object::{Object, ObjectKind};
use crate::runtime::Runtime;
use crate::string::{JsString, PropertyKey};
use crate::value::{Symbol, Value};
use crate::vm::{Lookup, SetOutcome, VmError, VmResult};

/// The private environment of one evaluation of a class body.
pub struct PrivateEnvironment {
    /// The name of the class in V8's messages
    /// ([`PrivateNames::class_name`]).
    pub(crate) class_name: Gc<JsString>,
    /// The brand that instances of the class carry when the class has
    /// instance private methods or accessors.
    pub(crate) brand: Option<Gc<Symbol>>,
    /// The class constructor, once the class has one: the only object
    /// with the static private methods and accessors.
    pub(crate) class: Option<Gc<Object>>,
    /// The private names, by index.
    pub(crate) names: Box<[PrivateElement]>,
}

/// A private name of a class evaluation (§6.2.10 `PrivateElement`
/// without the per-object part).
#[derive(Clone, Copy, Debug)]
pub(crate) struct PrivateElement {
    /// The Private Name.
    pub(crate) name: Gc<Symbol>,
    pub(crate) kind: PrivateKind,
    pub(crate) is_static: bool,
    /// The method, or the getter (`undefined` if there is none).
    pub(crate) get: Value,
    /// The setter (`undefined` if there is none).
    pub(crate) set: Value,
}

impl PrivateEnvironment {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.string(self.class_name);
        if let Some(brand) = self.brand {
            tracer.symbol(brand);
        }
        if let Some(class) = self.class {
            tracer.object(class);
        }
        for element in &self.names {
            tracer.symbol(element.name);
            tracer.value(element.get);
            tracer.value(element.set);
        }
    }

    pub(crate) fn heap_size(&self) -> usize {
        size_of::<PrivateEnvironment>() + self.names.len() * size_of::<PrivateElement>()
    }
}

/// What a private element access needs to know about its name.
#[derive(Clone, Copy)]
struct Resolved {
    element: PrivateElement,
    brand: Option<Gc<Symbol>>,
    class: Option<Gc<Object>>,
    class_name: Gc<JsString>,
}

/// The functions of a private method or accessor that
/// [`Runtime::set_private_function`] sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PrivateFunction {
    Method,
    Getter,
    Setter,
}

impl Runtime {
    /// A new private environment with a new Private Name for each name of
    /// `names` (§15.7.14 steps 6 and 7). Reserves its memory first (a
    /// safepoint) and charges a time step per 16 names.
    pub(crate) fn new_private_environment(&mut self, names: &PrivateNames) -> VmResult<Value> {
        let count = names.names.len();
        let bytes = (count + 1)
            .saturating_mul(Arena::<Symbol>::slot_size() + size_of::<PrivateElement>())
            .saturating_add(Arena::<Object>::slot_size() + size_of::<PrivateEnvironment>());
        self.heap.reserve(bytes, &self.vm)?;
        self.charge(count / 16)?;
        // Nothing below collects: the new symbols need no roots until the
        // object holds them.
        let mut elements = Vec::new();
        elements
            .try_reserve_exact(count)
            .map_err(crate::error::Error::from)?;
        for spec in &names.names {
            elements.push(PrivateElement {
                name: self.heap.alloc_private_name(spec.description)?,
                kind: spec.kind,
                is_static: spec.is_static,
                get: Value::Undefined,
                set: Value::Undefined,
            });
        }
        let has_brand = names
            .names
            .iter()
            .any(|spec| !spec.is_static && spec.kind != PrivateKind::Field);
        let brand = if has_brand {
            Some(self.heap.alloc_private_name(names.class_name)?)
        } else {
            None
        };
        let env = PrivateEnvironment {
            class_name: names.class_name,
            brand,
            class: None,
            names: elements.into_boxed_slice(),
        };
        let object = self
            .heap
            .new_object_with_kind(None, ObjectKind::PrivateEnvironment(Box::new(env)))?;
        Ok(Value::Object(object))
    }

    /// The private environment in a value.
    fn private_env(&self, env: Value) -> VmResult<&PrivateEnvironment> {
        let Value::Object(env) = env else {
            return Err(VmError::invariant("a private environment that is not one"));
        };
        match &self.heap.object(env)?.kind {
            ObjectKind::PrivateEnvironment(env) => Ok(env),
            _ => Err(VmError::invariant("a private environment that is not one")),
        }
    }

    /// The private environment in a value, for writing.
    fn private_env_mut(&mut self, env: Value) -> VmResult<&mut PrivateEnvironment> {
        let Value::Object(env) = env else {
            return Err(VmError::invariant("a private environment that is not one"));
        };
        match &mut self.heap.object_mut(env)?.kind {
            ObjectKind::PrivateEnvironment(env) => Ok(env),
            _ => Err(VmError::invariant("a private environment that is not one")),
        }
    }

    /// Name `index` of a private environment with what the checks need.
    fn resolve_private(&self, env: Value, index: u32) -> VmResult<Resolved> {
        let env = self.private_env(env)?;
        let element = *env
            .names
            .get(index as usize)
            .ok_or(VmError::invariant("a private name outside its environment"))?;
        Ok(Resolved {
            element,
            brand: env.brand,
            class: env.class,
            class_name: env.class_name,
        })
    }

    /// Sets the method, getter or setter of name `index` of a private
    /// environment (`ClassElementEvaluation` of a private method,
    /// §15.4.4 and §15.7.14 step 23).
    pub(crate) fn set_private_function(
        &mut self,
        env: Value,
        index: u32,
        function: Value,
        which: PrivateFunction,
    ) -> VmResult<()> {
        let element = self
            .private_env_mut(env)?
            .names
            .get_mut(index as usize)
            .ok_or(VmError::invariant("a private name outside its environment"))?;
        match which {
            PrivateFunction::Method | PrivateFunction::Getter => element.get = function,
            PrivateFunction::Setter => element.set = function,
        }
        Ok(())
    }

    /// The class constructor `function` owns the private environment
    /// `env`: the static private methods belong to it
    /// (`PrivateMethodOrAccessorAdd(F, method)`, §15.7.14 step 31), and
    /// `InitializeInstanceElements` gives its instances the brand.
    pub(crate) fn set_private_class(&mut self, env: Value, function: Value) -> VmResult<()> {
        let Value::Object(class) = function else {
            return Err(VmError::invariant("a private class that is not an object"));
        };
        let has_brand = {
            let env = self.private_env_mut(env)?;
            env.class = Some(class);
            env.brand.is_some()
        };
        if has_brand {
            self.set_private_env(function, env)?;
        }
        Ok(())
    }

    /// `PrivateMethodOrAccessorAdd` (§7.3.28) of all instance private
    /// methods and accessors of the class `constructor` to `receiver`: its
    /// brand (`InitializeInstanceElements`, §7.3.34 step 3).
    pub(crate) fn add_private_brand(
        &mut self,
        receiver: Value,
        constructor: Value,
    ) -> VmResult<()> {
        let Some(env) = self.private_env_of(constructor)? else {
            return Ok(());
        };
        let env = self.private_env(Value::Object(env))?;
        let (Some(brand), class_name) = (env.brand, env.class_name) else {
            return Ok(());
        };
        let Value::Object(object) = receiver else {
            return Err(VmError::invariant(
                "a brand for a value that is not an object",
            ));
        };
        if self.heap.private_find(object, brand)?.is_some() {
            return Err(VmError::type_error(format!(
                "Cannot initialize private methods of class {} twice on the same object",
                self.clipped_text(class_name)
            )));
        }
        self.heap
            .private_add(object, brand, Value::Undefined, &self.vm)?;
        Ok(())
    }

    /// `PrivateFieldAdd(obj, name, value)` (§7.3.27) of the field `index`.
    pub(crate) fn define_private_field(
        &mut self,
        target: Value,
        env: Value,
        index: u32,
        value: Value,
    ) -> VmResult<()> {
        let resolved = self.resolve_private(env, index)?;
        let Value::Object(object) = target else {
            return Err(VmError::invariant(
                "a private field of a value that is not an object",
            ));
        };
        let name = resolved.element.name;
        if self.heap.private_find(object, name)?.is_some() {
            return Err(VmError::type_error(format!(
                "Cannot initialize {} twice on the same object",
                self.private_text(name)
            )));
        }
        self.heap.private_add(object, name, value, &self.vm)?;
        Ok(())
    }

    /// `PrivateGet(obj, name)` (§7.3.31) for `obj.#name` with the base
    /// value `target` (`ToObject` first: `undefined` and `null` throw; a
    /// primitive has no private elements).
    pub(crate) fn private_get(&self, target: Value, env: Value, index: u32) -> VmResult<Lookup> {
        let resolved = self.resolve_private(env, index)?;
        let element = resolved.element;
        if target.is_nullish() {
            self.nullish_private_base(target, &resolved, false)?;
        }
        let object = match target {
            Value::Object(object) => Some(object),
            _ => None,
        };
        match element.kind {
            PrivateKind::Field => {
                let found = match object {
                    Some(object) => self.heap.private_find(object, element.name)?,
                    None => None,
                };
                found.map(Lookup::Value).ok_or_else(|| {
                    VmError::type_error(format!(
                        "Cannot read private member {} from an object whose class did not declare it",
                        self.private_text(element.name)
                    ))
                })
            }
            PrivateKind::Method => {
                self.check_brand(object, &resolved)?;
                Ok(Lookup::Value(element.get))
            }
            PrivateKind::Accessor => {
                self.check_brand(object, &resolved)?;
                if element.get == Value::Undefined {
                    return Err(VmError::type_error(format!(
                        "'{}' was defined without a getter",
                        self.private_text(element.name)
                    )));
                }
                Ok(Lookup::Getter {
                    getter: element.get,
                    receiver: target,
                })
            }
        }
    }

    /// `PrivateSet(obj, name, value)` (§7.3.32) for `obj.#name = value`.
    pub(crate) fn private_set(
        &mut self,
        target: Value,
        env: Value,
        index: u32,
        value: Value,
    ) -> VmResult<SetOutcome> {
        let resolved = self.resolve_private(env, index)?;
        let element = resolved.element;
        if target.is_nullish() {
            self.nullish_private_base(target, &resolved, true)?;
        }
        let object = match target {
            Value::Object(object) => Some(object),
            _ => None,
        };
        match element.kind {
            PrivateKind::Field => {
                let written = match object {
                    Some(object) => self.heap.private_write(object, element.name, value)?,
                    None => false,
                };
                if written {
                    return Ok(SetOutcome::Done);
                }
                Err(VmError::type_error(format!(
                    "Cannot write private member {} to an object whose class did not declare it",
                    self.private_text(element.name)
                )))
            }
            PrivateKind::Method => {
                self.check_brand(object, &resolved)?;
                Err(VmError::type_error(format!(
                    "Private method '{}' is not writable",
                    self.private_text(element.name)
                )))
            }
            PrivateKind::Accessor => {
                self.check_brand(object, &resolved)?;
                if element.set == Value::Undefined {
                    return Err(VmError::type_error(format!(
                        "'{}' was defined without a setter",
                        self.private_text(element.name)
                    )));
                }
                Ok(SetOutcome::Setter {
                    setter: element.set,
                    receiver: target,
                })
            }
        }
    }

    /// `#name in target` (§13.10.1 step 4 to 6): a `TypeError` for a
    /// value that is not an object, else whether it has the element.
    pub(crate) fn private_in(&self, target: Value, env: Value, index: u32) -> VmResult<bool> {
        let resolved = self.resolve_private(env, index)?;
        let element = resolved.element;
        let Value::Object(object) = target else {
            // V8 names the brand: the class for instance methods and
            // accessors, the private name otherwise.
            let name = if element.kind != PrivateKind::Field && !element.is_static {
                self.clipped_text(resolved.class_name)
            } else {
                self.private_text(element.name)
            };
            return Err(VmError::type_error(format!(
                "Cannot use 'in' operator to search for '{name}' in {}",
                self.primitive_text(target)
            )));
        };
        Ok(match (element.kind, element.is_static) {
            (PrivateKind::Field, _) => self.heap.private_find(object, element.name)?.is_some(),
            (_, true) => resolved.class == Some(object),
            (_, false) => match resolved.brand {
                Some(brand) => self.heap.private_find(object, brand)?.is_some(),
                None => false,
            },
        })
    }

    /// The `TypeError` of `ToObject` on an `undefined` or `null` base of
    /// a private member, in V8's words: a field names itself; an instance
    /// method or accessor names its class, as a read also for a write; a
    /// static method or accessor fails its brand check (below).
    fn nullish_private_base(
        &self,
        target: Value,
        resolved: &Resolved,
        write: bool,
    ) -> VmResult<()> {
        let element = resolved.element;
        match (element.kind, element.is_static) {
            (PrivateKind::Field, _) if write => {
                Err(self.cannot_set(target, PropertyKey::Symbol(element.name)))
            }
            (PrivateKind::Field, _) => {
                Err(self.cannot_read(target, PropertyKey::Symbol(element.name)))
            }
            (_, false) => Err(self.cannot_read(target, PropertyKey::String(resolved.class_name))),
            (_, true) => Ok(()),
        }
    }

    /// The brand check of a private method or accessor (the
    /// `PrivateElementFind` of §7.3.31 and §7.3.32 for them).
    fn check_brand(&self, object: Option<Gc<Object>>, resolved: &Resolved) -> VmResult<()> {
        let found = match (object, resolved.element.is_static, resolved.brand) {
            (Some(object), true, _) => resolved.class == Some(object),
            (Some(object), false, Some(brand)) => self.heap.private_find(object, brand)?.is_some(),
            _ => false,
        };
        if found {
            return Ok(());
        }
        let class = self.clipped_text(resolved.class_name);
        Err(VmError::type_error(if resolved.element.is_static {
            format!("Receiver must be class {class}")
        } else {
            format!("Receiver must be an instance of class {class}")
        }))
    }

    /// The text of a Private Name (its description, with the `#`).
    fn private_text(&self, name: Gc<Symbol>) -> String {
        self.heap
            .symbol(name)
            .ok()
            .and_then(|symbol| symbol.description)
            .map(|description| self.clipped_text(description))
            .unwrap_or_default()
    }
}
