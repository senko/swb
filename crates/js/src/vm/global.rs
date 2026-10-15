//! The global bindings (ECMA-262 §9.1.1.4 the global environment record,
//! §16.1.7 `GlobalDeclarationInstantiation` with the changes of Annex
//! B.3.2.2 for function declarations in blocks).

use std::borrow::Cow;

use crate::bytecode::{FunctionCode, GlobalKind};
use crate::heap::Gc;
use crate::object::{Object, Property, PropertyDescriptor};
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
            && let Some(binding) = record.lexical.get(&name)
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
        }
        Ok(None)
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
        if let Some(binding) = record.lexical.get_mut(&name) {
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
        if strict && !self.heap.has_property(global, key)? {
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
            .and_then(|r| r.lexical.get_mut(&name))
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
        if record.lexical.contains_key(&name) {
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
                .is_some_and(|r| r.lexical.contains_key(&name));
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
                return Err(VmError::Raise {
                    kind: crate::error::ThrowKind::SyntaxError,
                    message: Cow::Owned(format!(
                        "Identifier '{}' has already been declared",
                        self.name_text(name)
                    )),
                });
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
                        return Err(VmError::Raise {
                            kind: crate::error::ThrowKind::SyntaxError,
                            message: Cow::Owned(format!(
                                "Identifier '{}' has already been declared",
                                self.name_text(name)
                            )),
                        });
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
            .is_some_and(|r| r.lexical.contains_key(&name));
        if lexical {
            return Ok(false);
        }
        let global = self.global_object(realm)?;
        self.can_declare_global_var(global, name)
    }
}
