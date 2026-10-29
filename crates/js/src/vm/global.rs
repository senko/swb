//! The global bindings (ECMA-262 §9.1.1.4 the global environment record,
//! §16.1.7 `GlobalDeclarationInstantiation` with the changes of Annex
//! B.3.2.2 for function declarations in blocks).

use crate::bytecode::{FunctionCode, GlobalKind};
use crate::heap::Gc;
use crate::object::{GetResult, Object, ObjectKind, Property, PropertyDescriptor, SiteCache};
use crate::runtime::Runtime;
use crate::string::{JsString, PropertyKey};
use crate::value::Value;
use crate::vm::property::{Lookup, SetOutcome};
use crate::vm::realm::LexicalBinding;
use crate::vm::{VmError, VmResult};

impl Runtime {
    /// Looks up a global name: the declarative record, then the global
    /// object. `None` if neither has it.
    pub(crate) fn get_global(&mut self, name: Gc<JsString>) -> VmResult<Option<Lookup>> {
        let realm = self.current_realm();
        let record = self
            .vm
            .realms
            .get(realm as usize)
            .ok_or(VmError::invariant("no realm"))?;
        if !record.lexical.is_empty()
            && let Some(binding) = record.lexical.get(name)
        {
            if binding.value.is_empty() {
                return Err(self.tdz_error(name));
            }
            return Ok(Some(Lookup::Value(binding.value)));
        }
        let global = record.global;
        let key = PropertyKey::String(name);
        // HasBinding and GetBindingValue of the object record (§9.1.1.2):
        // one walk that finds the property.
        let mut current = Some(global);
        while let Some(object) = current {
            if let Some(property) = self.heap.get_own_property(object, key)? {
                return Ok(Some(match property {
                    // An interface object that is created at its first
                    // read (`host/lazy.rs`).
                    Property::Data {
                        value: Value::Lazy(id),
                        ..
                    } => Lookup::Value(self.resolve_lazy(object, key, id)?),
                    Property::Data { value, .. } => Lookup::Value(value),
                    Property::Accessor { get, .. } if get.is_undefined() => {
                        Lookup::Value(Value::Undefined)
                    }
                    Property::Accessor { get, .. } => Lookup::Getter {
                        getter: get,
                        receiver: global.into(),
                    },
                }));
            }
            current = self.heap.get_prototype_of(object)?;
            // The global object is not a proxy or a host object with hooks;
            // a prototype can be one.
            if let Some(proto) = current
                && self.heap.needs_runtime(proto)?
            {
                return self.global_through_generic(global, proto, key);
            }
        }
        Ok(None)
    }

    /// The rest of a global read whose walk met a proxy or a host object
    /// with hooks on the prototype chain of the global object: `HasBinding`
    /// (its `[[HasProperty]]`), then `GetBindingValue` of the object record
    /// (`HasProperty` and `Get` of the global object, §9.1.1.2.6).
    #[cold]
    #[inline(never)]
    fn global_through_generic(
        &mut self,
        global: Gc<Object>,
        proxy: Gc<Object>,
        key: PropertyKey,
    ) -> VmResult<Option<Lookup>> {
        self.with_scope(|rt| {
            if !rt.has_generic(proxy, key)? {
                return Ok(None);
            }
            if !rt.has_property(global, key)? {
                return Ok(Some(Lookup::Value(Value::Undefined)));
            }
            Ok(Some(rt.get_with_receiver(global, key, global.into())?))
        })
    }

    /// A global read through the cache entry of its site (ADR 0026
    /// section 15, M7 13a): the result on a hit, `None` on a miss. A
    /// binding in its dead zone is a miss (the general path throws).
    #[inline]
    pub(crate) fn cached_global_get(&self, entry: SiteCache) -> VmResult<Option<GetResult>> {
        match entry {
            SiteCache::GlobalLexical { realm, index } if realm == self.current_realm() => Ok(self
                .vm
                .realms
                .get(realm as usize)
                .and_then(|record| record.lexical.binding(index))
                .filter(|binding| !binding.value.is_empty())
                .map(|binding| GetResult::Value(binding.value))),
            SiteCache::GlobalProperty {
                realm,
                lexicals,
                holder,
                slot,
                accessor,
                epoch,
            } if realm == self.current_realm() && epoch == self.heap.watch_epoch() => {
                let Some(record) = self.vm.realms.get(realm as usize) else {
                    return Ok(None);
                };
                if record.lexical.len() != lexicals {
                    return Ok(None);
                }
                let value = self.heap.object(holder)?.slots.get(slot as usize).copied();
                Ok(match value {
                    Some(getter) if accessor => Some(if getter.is_undefined() {
                        GetResult::Value(Value::Undefined)
                    } else {
                        GetResult::CallGetter {
                            getter,
                            receiver: record.global.into(),
                        }
                    }),
                    // No entry is made for a lazy value and no slot gets one
                    // back; a miss is the safe answer anyway.
                    Some(value) if !value.is_lazy() => Some(GetResult::Value(value)),
                    Some(_) | None => None,
                })
            }
            _ => Ok(None),
        }
    }

    /// A global write through the cache entry of its site: true if it
    /// wrote the value. A constant, a binding in its dead zone and every
    /// other case are a miss (the general path).
    #[inline]
    pub(crate) fn cached_global_set(&mut self, entry: SiteCache, value: Value) -> VmResult<bool> {
        match entry {
            SiteCache::GlobalLexical { realm, index } if realm == self.current_realm() => {
                let binding = self
                    .vm
                    .realms
                    .get_mut(realm as usize)
                    .and_then(|record| record.lexical.binding_mut(index));
                match binding {
                    Some(binding) if binding.mutable && !binding.value.is_empty() => {
                        binding.value = value;
                        Ok(true)
                    }
                    _ => Ok(false),
                }
            }
            SiteCache::GlobalProperty {
                realm,
                lexicals,
                holder,
                slot,
                accessor: false,
                epoch,
            } if realm == self.current_realm() && epoch == self.heap.watch_epoch() => {
                let fresh = self.vm.realms.get(realm as usize).is_some_and(|record| {
                    record.lexical.len() == lexicals && record.global == holder
                });
                if !fresh {
                    return Ok(false);
                }
                match self.heap.object_mut(holder)?.slots.get_mut(slot as usize) {
                    Some(stored) => {
                        *stored = value;
                        Ok(true)
                    }
                    None => Ok(false),
                }
            }
            _ => Ok(false),
        }
    }

    /// The global read of the name of `key` after a miss of the cache
    /// entry of `site`: [`Runtime::get_global`] (a `ReferenceError` if the
    /// name has no binding), then the entry for it.
    #[inline(never)]
    pub(crate) fn get_global_miss(
        &mut self,
        code: &FunctionCode,
        site: u32,
        key: PropertyKey,
    ) -> VmResult<Lookup> {
        let PropertyKey::String(name) = key else {
            return Err(VmError::invariant("a global site without a name"));
        };
        let Some(found) = self.get_global(name)? else {
            let text = self.name_text(name);
            return Err(VmError::reference_error(format!("{text} is not defined")));
        };
        let entry = self.global_entry(name, false)?;
        self.fill_cache(code, site, entry);
        Ok(found)
    }

    /// The global write of the name of `key` after a miss of the cache
    /// entry of `site`: [`Runtime::set_global`], with the entry for the
    /// next write.
    #[inline(never)]
    pub(crate) fn set_global_miss(
        &mut self,
        code: &FunctionCode,
        site: u32,
        key: PropertyKey,
        value: Value,
        strict: bool,
    ) -> VmResult<SetOutcome> {
        let PropertyKey::String(name) = key else {
            return Err(VmError::invariant("a global site without a name"));
        };
        let outcome = self.set_global(name, value, strict)?;
        let entry = self.global_entry(name, true)?;
        self.fill_cache(code, site, entry);
        Ok(outcome)
    }

    /// The cache entry of the global name `name` in the current realm: its
    /// lexical binding, or the property of the global object or of its
    /// prototype chain that holds it (for a write, only a writable data
    /// property of the global object). `Empty` if there is none or the
    /// walk meets an object whose kind a cache cannot follow.
    fn global_entry(&self, name: Gc<JsString>, write: bool) -> VmResult<SiteCache> {
        let realm = self.current_realm();
        let record = self
            .vm
            .realms
            .get(realm as usize)
            .ok_or(VmError::invariant("no realm"))?;
        if let Some(index) = record.lexical.position(name) {
            return Ok(SiteCache::GlobalLexical { realm, index });
        }
        let key = PropertyKey::String(name);
        let mut current = Some(record.global);
        while let Some(object) = current {
            let object_record = self.heap.object(object)?;
            let plain = !matches!(
                object_record.kind,
                ObjectKind::Proxy(_) | ObjectKind::HostExotic(_) | ObjectKind::TypedArray(_)
            );
            if !plain || !object_record.watched {
                return Ok(SiteCache::Empty);
            }
            let shape = self.heap.shape(object_record.shape)?;
            if let Some(found) = shape.find(key, &self.heap.arenas.key_lists)? {
                let accessor = found.flags.accessor();
                if write && (object != record.global || accessor || !found.flags.writable()) {
                    return Ok(SiteCache::Empty);
                }
                // A lazy value that is not created yet (`host/lazy.rs`).
                if !accessor
                    && object_record
                        .slots
                        .get(found.slot as usize)
                        .is_some_and(|value| value.is_lazy())
                {
                    return Ok(SiteCache::Empty);
                }
                return Ok(SiteCache::GlobalProperty {
                    realm,
                    lexicals: record.lexical.len(),
                    holder: object,
                    slot: found.slot,
                    accessor,
                    epoch: self.heap.watch_epoch(),
                });
            }
            current = shape.proto();
        }
        Ok(SiteCache::Empty)
    }

    /// `PutValue` on a global name (§9.1.1.4.5 `SetMutableBinding`; an
    /// unresolvable name in sloppy mode creates a global property).
    pub(crate) fn set_global(
        &mut self,
        name: Gc<JsString>,
        value: Value,
        strict: bool,
    ) -> VmResult<SetOutcome> {
        let realm = self.current_realm();
        let record = self
            .vm
            .realms
            .get_mut(realm as usize)
            .ok_or(VmError::invariant("no realm"))?;
        if let Some(binding) = record.lexical.get_mut(name) {
            if binding.value.is_empty() {
                return Err(self.tdz_error(name));
            }
            if !binding.mutable {
                return Err(VmError::type_error("Assignment to constant variable."));
            }
            binding.value = value;
            return Ok(SetOutcome::Done);
        }
        let global = record.global;
        let key = PropertyKey::String(name);
        if strict && !self.has_property(global, key)? {
            let text = self.name_text(name);
            return Err(VmError::reference_error(format!("{text} is not defined")));
        }
        self.put_value(global.into(), key, value, strict)
    }

    /// Initializes a global `let` or `const` binding at its declaration.
    pub(crate) fn init_global_lexical(&mut self, name: Gc<JsString>, value: Value) -> VmResult<()> {
        let realm = self.current_realm();
        let binding = self
            .vm
            .realms
            .get_mut(realm as usize)
            .and_then(|r| r.lexical.get_mut(name))
            .ok_or(VmError::invariant(
                "a global lexical binding was not declared",
            ))?;
        binding.value = value;
        Ok(())
    }

    /// `CreateGlobalFunctionBinding` (§9.1.1.4.18); `deletable` (`D`) for
    /// the functions of eval code.
    pub(crate) fn init_global_function(
        &mut self,
        name: Gc<JsString>,
        value: Value,
        deletable: bool,
    ) -> VmResult<()> {
        let global = self.global_object(self.current_realm())?;
        let key = PropertyKey::String(name);
        let existing = self.heap.get_own_property(global, key)?;
        let desc = match existing {
            None => PropertyDescriptor::data(value, true, true, deletable),
            Some(property) if property.configurable() => {
                PropertyDescriptor::data(value, true, true, deletable)
            }
            Some(_) => PropertyDescriptor::value(value),
        };
        if !self.heap.define_own_property(global, key, desc, &self.vm)? {
            return Err(VmError::type_error(format!(
                "Cannot redefine property: {}",
                self.name_text(name)
            )));
        }
        // Step 6: Set(globalObject, N, V, false).
        let _ = self.heap.set(global, key, value, global.into(), &self.vm)?;
        Ok(())
    }

    /// `delete name` for a global name (§9.1.1.4.7 `DeleteBinding`).
    pub(crate) fn delete_global(&mut self, name: Gc<JsString>) -> VmResult<bool> {
        let realm = self.current_realm();
        let record = self
            .vm
            .realms
            .get(realm as usize)
            .ok_or(VmError::invariant("no realm"))?;
        if record.lexical.contains_key(name) {
            return Ok(false);
        }
        let global = record.global;
        Ok(self.heap.delete(global, PropertyKey::String(name))?)
    }

    /// `GlobalDeclarationInstantiation` (§16.1.7) for the declarations of a
    /// script: the redeclaration checks, the lexical bindings in their
    /// dead zone, and the `var` properties. Function values are set by
    /// `InitGlobalFunction` after their closures are created.
    pub(crate) fn declare_globals(&mut self, code: &FunctionCode) -> VmResult<()> {
        let realm = self.current_realm();
        let global = self.global_object(realm)?;
        let names: Vec<(Gc<JsString>, GlobalKind)> = code
            .globals
            .iter()
            .map(|g| {
                code.string(g.name)
                    .map(|name| (name, g.kind))
                    .ok_or(VmError::invariant("a global name that is not a string"))
            })
            .collect::<VmResult<_>>()?;
        // Steps 3 to 6: conflicts with existing bindings (the vars of
        // Annex B.3.2.2 are not VarDeclaredNames and never conflict).
        for &(name, kind) in &names {
            let lexical_exists = self
                .vm
                .realms
                .get(realm as usize)
                .is_some_and(|r| r.lexical.contains_key(name));
            let redeclared = match kind {
                GlobalKind::Let | GlobalKind::Const => {
                    lexical_exists
                        || self
                            .heap
                            .get_own_property(global, PropertyKey::String(name))?
                            .is_some_and(|p| !p.configurable())
                }
                GlobalKind::Var | GlobalKind::Function => lexical_exists,
                GlobalKind::BlockFunction => false,
            };
            if redeclared {
                return Err(VmError::syntax_error(format!(
                    "Identifier '{}' has already been declared",
                    self.name_text(name)
                )));
            }
        }
        // Steps 11 and 12 (§9.1.1.4.15 `CanDeclareGlobalVar`, §9.1.1.4.16
        // `CanDeclareGlobalFunction`): every check runs before any binding
        // is created. V8 reports a function that cannot be declared as a
        // `SyntaxError` with the message of a redeclaration.
        for &(name, kind) in &names {
            let key = PropertyKey::String(name);
            let existing = self.heap.get_own_property(global, key)?;
            match kind {
                GlobalKind::Function => {
                    let allowed = match existing {
                        None => self.heap.is_extensible(global)?,
                        Some(property) if property.configurable() => true,
                        Some(Property::Data {
                            writable,
                            enumerable,
                            ..
                        }) => writable && enumerable,
                        Some(Property::Accessor { .. }) => false,
                    };
                    if !allowed {
                        return Err(VmError::syntax_error(format!(
                            "Identifier '{}' has already been declared",
                            self.name_text(name)
                        )));
                    }
                }
                GlobalKind::Var => {
                    if existing.is_none() && !self.heap.is_extensible(global)? {
                        return Err(VmError::type_error(format!(
                            "Cannot define global variable '{}', the global object is not extensible",
                            self.name_text(name)
                        )));
                    }
                }
                GlobalKind::Let | GlobalKind::Const | GlobalKind::BlockFunction => {}
            }
        }
        // Annex B.3.2.2 (in place of step 12): the var bindings of the
        // hoisted block functions, before all other bindings.
        // `CreateGlobalVarBinding(F, false)` keeps an existing property.
        for &(name, kind) in &names {
            if kind == GlobalKind::BlockFunction && self.block_function_hoisted(name)? {
                self.create_global_var(global, name)?;
            }
        }
        // Steps 15 and 17: lexical bindings, then `var` bindings
        // (§9.1.1.4.17 CreateGlobalVarBinding).
        for &(name, kind) in &names {
            match kind {
                GlobalKind::Let | GlobalKind::Const => {
                    if let Some(record) = self.vm.realms.get_mut(realm as usize) {
                        record.lexical.insert(
                            name,
                            LexicalBinding {
                                value: Value::Empty,
                                mutable: kind == GlobalKind::Let,
                            },
                        );
                    }
                }
                GlobalKind::Var => self.create_global_var(global, name)?,
                GlobalKind::Function | GlobalKind::BlockFunction => {}
            }
        }
        Ok(())
    }

    /// `CreateGlobalVarBinding(name, false)` (§9.1.1.4.17): a
    /// non-deletable property `undefined` unless the global object has the
    /// property already or is not extensible.
    fn create_global_var(&mut self, global: Gc<Object>, name: Gc<JsString>) -> VmResult<()> {
        let key = PropertyKey::String(name);
        if self.heap.get_own_property(global, key)?.is_none() && self.heap.is_extensible(global)? {
            let desc = PropertyDescriptor::data(Value::Undefined, true, true, false);
            self.heap.define_own_property(global, key, desc, &self.vm)?;
        }
        Ok(())
    }

    /// Whether Annex B.3.2.2 hoists a block function of a script into the
    /// global var `name`: no lexical declaration has the name
    /// (`HasLexicalDeclaration`) and `CanDeclareGlobalVar` is true.
    ///
    /// `DeclareGlobals` decides with it, and the evaluation of the
    /// declaration (`SetBlockFunctionGlobal`) decides again instead of
    /// keeping the first answer: no script can change it in between. A
    /// global lexical binding cannot be removed, and a later script cannot
    /// add one for a hoisted name (it is a var name then, and its property
    /// is not configurable); a global object that is not extensible stays
    /// so, and a property that was missing cannot be added to it; when the
    /// script deletes the property of a hoisted function and then makes the
    /// global object non-extensible, the answer changes, but the
    /// specification's `SetMutableBinding` writes nothing then either.
    /// Kept out of the interpreter loop, whose speed depends on its size.
    #[inline(never)]
    pub(crate) fn block_function_hoisted(&mut self, name: Gc<JsString>) -> VmResult<bool> {
        let realm = self.current_realm();
        let lexical = self
            .vm
            .realms
            .get(realm as usize)
            .is_some_and(|r| r.lexical.contains_key(name));
        if lexical {
            return Ok(false);
        }
        let global = self.global_object(realm)?;
        self.can_declare_global_var(global, name)
    }
}
