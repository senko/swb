//! Classes (§15.7) and `super` (§13.3.7).
//!
//! `ClassDefinitionEvaluation` (§15.7.14) compiles to a sequence in the
//! enclosing function: the scopes of the class (the inner name binding in
//! its dead zone), the heritage (`ClassHeritage`), the constructor and the
//! prototype (`NewClass`), the elements in source order (methods and
//! accessors are defined at once; computed field keys are evaluated and
//! kept in an internal array), the inner binding, the instance field
//! initializer function (`[[Fields]]` of the constructor) and the call of
//! the static initializer function.
//!
//! The fields of a class are defined by two functions that the parser
//! makes ([`FunctionKind::InstanceInitializer`],
//! [`FunctionKind::StaticInitializer`]): their code defines each field in
//! order with the field's key (a site, or the computed key at its index in
//! the array of the class) and runs the static blocks. A base constructor
//! calls the instance initializer at its start; a derived constructor right
//! after `super(...)` returns (`InitFields`). A class without a
//! `constructor` method gets a default constructor whose code the compiler
//! makes here ([`default_constructor`]); the derived one passes its
//! arguments to the parent without the array iterator (§15.7.14 step
//! 14.a).
//!
//! Private names (§6.2.12): a class with private names creates its private
//! environment when its body scope is entered, before the heritage
//! (`NewPrivateEnv`, with the names of `private_names_const`); private
//! methods and accessors go into it as their elements are evaluated, and
//! the constructor owns it (`PrivateClass`). The initializer functions get
//! it in their class slots for the private fields they define; every
//! other use (`o.#x`, `#x in o`) reaches it through the class body's
//! `%private` binding, which closures capture like other bindings.
//!
//! `super` properties: `this` (checked in a derived constructor), the key
//! expression, then the home object's prototype (`SuperBase`) go into a
//! pair of registers (base, `this`); `ToPropertyKey` runs in the access
//! itself (§13.3.7.1: after `GetSuperBase`, and for an assignment after the
//! value).

use swb_js_syntax::{
    BindingKind, ClassElement, ClassElementKind, ClassId, ClassKey, ExprId, ExprKind, FunctionId,
    FunctionKind, PropertyKey as AstKey, RefId, Resolution, SuperCallId,
};
use swb_js_text::String16;

use super::expr::KeyReg;
use super::function::FunctionCompiler;
use super::{Builder, CResult, ConstSpec};
use crate::bytecode::{
    CodeKind, ITER_SPREAD, Insn, NAME_PREFIX_GET, NAME_PREFIX_NONE, NAME_PREFIX_SET, PrivateKind,
    Reg, ThisMode,
};

/// The name of the instance field initializer in stack traces (V8's).
const INSTANCE_INITIALIZER: &str = "<instance_members_initializer>";

/// The name of the static initializer in stack traces (V8's).
const STATIC_INITIALIZER: &str = "<static_initializer>";

impl FunctionCompiler<'_> {
    /// `ClassDefinitionEvaluation` (§15.7.14) of the class `id`; the
    /// constructor is the result. `runtime_name` is the register of a
    /// property key that names an anonymous class (`NamedEvaluation` with
    /// a computed key).
    pub(super) fn class_definition(
        &mut self,
        id: ClassId,
        dst: Option<Reg>,
        runtime_name: Option<Reg>,
    ) -> CResult<Reg> {
        // All parts of a class are strict mode code (§11.2.2); in sloppy
        // code, the instructions of the definition form a strict range.
        if self.strict {
            return self.class_definition_strict(id, dst, runtime_name);
        }
        let start = self.here() as u32;
        self.strict = true;
        let result = self.class_definition_strict(id, dst, runtime_name);
        self.strict = false;
        self.strict_ranges.push((start, self.here() as u32));
        result
    }

    /// [`Self::class_definition`] in strict mode code.
    fn class_definition_strict(
        &mut self,
        id: ClassId,
        dst: Option<Reg>,
        runtime_name: Option<Reg>,
    ) -> CResult<Reg> {
        let class = *self.ast.class(id);
        let t = self.target(dst)?;
        let mark = self.mark();
        let start = class.span.start;
        self.position = start;
        self.enter_scope(class.scope)?;
        self.enter_scope(class.body_scope)?;
        let env = self.private_environment(id)?;
        // `parents` and `parents + 1`: the heritage's parents, then the
        // constructor and the prototype.
        let parents = self.temps(2)?;
        match class.heritage {
            Some(heritage) => {
                self.expr(heritage, Some(parents))?;
                self.position = self.ast.expr(heritage).span.start;
                self.emit(Insn::ClassHeritage {
                    dst: parents,
                    src: parents,
                });
            }
            None => {
                self.emit(Insn::Empty { dst: parents });
            }
        }
        let index = self.constructor_const(id);
        self.position = start;
        self.emit(Insn::NewClass {
            dst: parents,
            index,
        });
        let (function, prototype) = (parents, parents + 1);
        if let Some(env) = env {
            self.emit(Insn::PrivateClass {
                env,
                func: function,
            });
        }
        if let Some(key) = runtime_name {
            self.emit(Insn::SetFunctionName {
                func: function,
                key,
                prefix: NAME_PREFIX_NONE,
            });
        }
        let elements = self.ast.class_elements(class.elements).to_vec();
        let keys = self.temp()?;
        if elements.iter().any(is_computed_field) {
            self.emit(Insn::NewArray { dst: keys });
        }
        for element in &elements {
            let target = if element.is_static {
                function
            } else {
                prototype
            };
            self.class_element(element, target, keys, env)?;
        }
        self.position = start;
        if let Some(inner) = class.inner {
            let binding = self
                .reference(inner)
                .binding
                .ok_or(Self::internal("a class name without a binding"))?;
            self.init_binding(binding, function)?;
        }
        let keys = elements.iter().any(is_computed_field).then_some(keys);
        self.class_initializers(&class, (function, prototype), keys, env)?;
        // The keys stay alive only in the initializer functions.
        if let Some(keys) = keys {
            self.emit(Insn::Undefined { dst: keys });
        }
        self.emit(Insn::Move {
            dst: t,
            src: function,
        });
        self.release(mark);
        Ok(t)
    }

    /// The constant of a class's constructor code: its `constructor`
    /// method (which gets the class's name), or a default constructor.
    fn constructor_const(&mut self, id: ClassId) -> u32 {
        let name = self.class_name(id);
        if let Some(constructor) = self.ast.class(id).constructor {
            self.session.inferred_names.insert(constructor, name);
            return self.function_const(constructor);
        }
        let builder = default_constructor(self, id, name);
        self.session.default_constructors.insert(id, builder);
        self.push_constant(ConstSpec::DefaultConstructor(id))
    }

    /// The private environment of a class with private names (§15.7.14
    /// steps 6 and 7) in its binding; returns the register that holds it
    /// during the definition.
    fn private_environment(&mut self, id: ClassId) -> CResult<Option<Reg>> {
        let body_scope = self.ast.class(id).body_scope;
        let Some(binding) = self.scopes.private_environment(body_scope) else {
            return Ok(None);
        };
        let index = self.private_names_const(id)?;
        let t = match self.scopes.binding(binding).storage {
            swb_js_syntax::Storage::Register(r) => r,
            _ => self.temp()?,
        };
        self.emit(Insn::NewPrivateEnv { dst: t, index });
        self.init_binding(binding, t)?;
        Ok(Some(t))
    }

    /// The constant of a class's private names: each name with the kind
    /// and placement of its element, in the order of their indices, and
    /// the class's name for V8's messages.
    fn private_names_const(&mut self, id: ClassId) -> CResult<u32> {
        let class = *self.ast.class(id);
        let count = self
            .scopes
            .bindings_of(class.body_scope)
            .iter()
            .filter(|&&b| self.scopes.binding(b).kind == BindingKind::PrivateName)
            .count();
        let mut names: Vec<Option<(String16, PrivateKind, bool)>> = vec![None; count];
        let mut has_static_method = false;
        for element in self.ast.class_elements(class.elements) {
            let ClassKey::Private(ident) = element.key else {
                continue;
            };
            let kind = match element.kind {
                ClassElementKind::Field => PrivateKind::Field,
                ClassElementKind::Method => PrivateKind::Method,
                _ => PrivateKind::Accessor,
            };
            let index = self.private_index(ident)?;
            has_static_method |= element.is_static
                && kind != PrivateKind::Field
                && self
                    .scopes
                    .private_name(ident.reference)
                    .is_some_and(|binding| self.session.used_private_names.contains(&binding));
            let slot = names
                .get_mut(index as usize)
                .ok_or(Self::internal("a private name outside its class"))?;
            *slot = Some((self.name_text(ident.name), kind, element.is_static));
        }
        let names = names
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .ok_or(Self::internal("a private name without its element"))?;
        // V8's name of the class in its messages: the name that
        // `NamedEvaluation` gives does not count; an anonymous class whose
        // static private methods or accessors are used is `.` (measured).
        let class_name = match class.name {
            Some(name) => self.name_text(name),
            None if has_static_method => String16::from("."),
            None => String16::from("anonymous"),
        };
        Ok(self.push_constant(ConstSpec::PrivateNames { class_name, names }))
    }

    /// The index of a private name occurrence in the private environment of
    /// its class.
    pub(super) fn private_index(&self, ident: swb_js_syntax::Ident) -> CResult<u32> {
        // Eval code inside a class sees the names of the caller's classes.
        let Some(binding) = self.scopes.private_name(ident.reference) else {
            return self
                .caller_private_index(ident)
                .ok_or(Self::internal("a private name without its class"));
        };
        match self.scopes.binding(binding).storage {
            swb_js_syntax::Storage::Private(index) => Ok(index),
            _ => Err(Self::internal("a private name without its index")),
        }
    }

    /// The initializer functions of a class (§15.7.14 steps 27 and 29):
    /// the instance one becomes `[[Fields]]` of the constructor, the
    /// static one runs now with the constructor as `this`. Both get the
    /// array of computed field keys in `keys`, if there is one, and the
    /// private environment.
    fn class_initializers(
        &mut self,
        class: &swb_js_syntax::Class,
        (function, prototype): (Reg, Reg),
        keys: Option<Reg>,
        env: Option<Reg>,
    ) -> CResult<()> {
        if let Some(init) = class.instance_init {
            let c = self.temp()?;
            let index = self.function_const(init);
            self.emit(Insn::MethodClosure {
                dst: c,
                home: prototype,
                index,
            });
            if let Some(keys) = keys {
                self.emit(Insn::SetFieldKeys { func: c, keys });
            }
            if let Some(env) = env {
                self.emit(Insn::SetPrivateEnv { func: c, env });
            }
            self.emit(Insn::SetClassFields {
                func: function,
                src: c,
            });
        }
        if let Some(init) = class.static_init {
            let window = self.temps(2)?;
            let index = self.function_const(init);
            self.emit(Insn::MethodClosure {
                dst: window,
                home: function,
                index,
            });
            if let Some(keys) = keys {
                self.emit(Insn::SetFieldKeys { func: window, keys });
            }
            if let Some(env) = env {
                self.emit(Insn::SetPrivateEnv { func: window, env });
            }
            self.emit(Insn::Move {
                dst: window + 1,
                src: function,
            });
            self.emit(Insn::Call {
                callee: window,
                argc: 0,
            });
        }
        Ok(())
    }

    /// The name of a class: its binding identifier, else the name that
    /// `NamedEvaluation` gives it, else the empty string.
    fn class_name(&self, id: ClassId) -> String16 {
        match self.ast.class(id).name {
            Some(name) => self.name_text(name),
            None => self
                .session
                .class_names
                .get(&id)
                .cloned()
                .unwrap_or_default(),
        }
    }

    /// `ClassElementEvaluation` (§15.7.14 step 23) of one element on
    /// `target` (the prototype or the constructor): methods and accessors
    /// are defined (private ones go into the private environment `env`); a
    /// computed field key is evaluated and appended to the array in
    /// `keys`.
    fn class_element(
        &mut self,
        element: &ClassElement,
        target: Reg,
        keys: Reg,
        env: Option<Reg>,
    ) -> CResult<()> {
        self.position = element.span.start;
        let mark = self.mark();
        let member = match element.kind {
            ClassElementKind::Method => Some((NAME_PREFIX_NONE, "")),
            ClassElementKind::Getter => Some((NAME_PREFIX_GET, "get ")),
            ClassElementKind::Setter => Some((NAME_PREFIX_SET, "set ")),
            ClassElementKind::Field | ClassElementKind::StaticBlock { .. } => None,
        };
        let key = match element.key {
            ClassKey::Property(key) => Some(key),
            ClassKey::Private(ident) => {
                if let Some((_, prefix_text)) = member {
                    self.private_method(element, ident, (target, env), prefix_text)?;
                }
                self.release(mark);
                return Ok(());
            }
            ClassKey::StaticBlock => None,
        };
        match (member, key) {
            (Some((prefix, prefix_text)), Some(key)) => {
                let value = element
                    .value
                    .ok_or(Self::internal("a method without a function"))?;
                let ExprKind::Function(function) = self.ast.expr(value).kind else {
                    return Err(Self::internal("a method that is not a function"));
                };
                let k = self.temp()?;
                let computed = self.member_key(key, k, function, prefix_text)?;
                self.position = self.ast.expr(value).span.start;
                let v = self.method_closure(function, target, None)?;
                if computed {
                    self.emit(Insn::SetFunctionName {
                        func: v,
                        key: k,
                        prefix,
                    });
                }
                self.position = element.span.start;
                let (obj, src) = (target, v);
                self.emit(match element.kind {
                    ClassElementKind::Getter => Insn::DefineClassGetter { obj, key: k, src },
                    ClassElementKind::Setter => Insn::DefineClassSetter { obj, key: k, src },
                    _ => Insn::DefineMethod { obj, key: k, src },
                });
            }
            (None, Some(AstKey::Computed(key))) => {
                let k = self.temp()?;
                self.expr(key, Some(k))?;
                self.emit(Insn::ToKey { dst: k, src: k });
                self.emit(Insn::Push {
                    array: keys,
                    src: k,
                });
            }
            _ => {}
        }
        self.release(mark);
        Ok(())
    }

    /// A private method, getter or setter (§15.4.4 `DefineMethod` and the
    /// `PrivateElement` of §15.7.14 step 23): the closure, with the home
    /// object in `home`, goes into the private environment `env`.
    fn private_method(
        &mut self,
        element: &ClassElement,
        ident: swb_js_syntax::Ident,
        (home, env): (Reg, Option<Reg>),
        prefix: &str,
    ) -> CResult<()> {
        let env = env.ok_or(Self::internal("a private method without its environment"))?;
        let index = self.private_index(ident)?;
        let value = element
            .value
            .ok_or(Self::internal("a method without a function"))?;
        let ExprKind::Function(function) = self.ast.expr(value).kind else {
            return Err(Self::internal("a method that is not a function"));
        };
        let mut name = String16::from(prefix);
        name.push_str16(self.name_text(ident.name).as_str16());
        self.session.inferred_names.insert(function, name);
        self.position = self.ast.expr(value).span.start;
        let src = self.method_closure(function, home, None)?;
        self.position = element.span.start;
        self.emit(match element.kind {
            ClassElementKind::Getter => Insn::PrivateGetter { env, src, index },
            ClassElementKind::Setter => Insn::PrivateSetter { env, src, index },
            _ => Insn::PrivateMethod { env, src, index },
        });
        Ok(())
    }

    /// The key of a method into `k`: a computed key is evaluated with
    /// `ToPropertyKey` (returns true); a literal key is a string constant,
    /// and the method gets its name now.
    fn member_key(
        &mut self,
        key: AstKey,
        k: Reg,
        function: FunctionId,
        prefix: &str,
    ) -> CResult<bool> {
        if let AstKey::Computed(key) = key {
            self.expr(key, Some(k))?;
            self.emit(Insn::ToKey { dst: k, src: k });
            return Ok(true);
        }
        let text = self
            .key_text(key)
            .ok_or(Self::internal("a method key without a text"))?;
        let mut name = String16::from(prefix);
        name.push_str16(text.as_str16());
        self.session.inferred_names.insert(function, name);
        let index = self.string_const(text);
        self.emit(Insn::Const { dst: k, index });
        Ok(false)
    }

    /// A closure of a method (§15.4.4 `DefineMethod`): with the home
    /// object in `home` if the method uses `super` properties.
    pub(super) fn method_closure(
        &mut self,
        function: FunctionId,
        home: Reg,
        dst: Option<Reg>,
    ) -> CResult<Reg> {
        let t = self.target(dst)?;
        let index = self.function_const(function);
        if self.scopes.function(function).home_object_binding.is_some() {
            self.emit(Insn::MethodClosure {
                dst: t,
                home,
                index,
            });
        } else {
            self.emit(Insn::Closure { dst: t, index });
        }
        Ok(t)
    }

    /// `NamedEvaluation` with a key known at run time (in `key`, a value of
    /// `ToKey`): an anonymous class gets the name before its elements run,
    /// an anonymous function after its creation.
    pub(super) fn expr_named_by_key(
        &mut self,
        e: ExprId,
        dst: Option<Reg>,
        key: Reg,
    ) -> CResult<Reg> {
        let inner = self.unparen(e);
        if let ExprKind::Class(id) = self.ast.expr(inner).kind
            && self.ast.class(id).name.is_none()
        {
            self.enter(super::function::EXPR_WEIGHT)?;
            let outer = std::mem::replace(&mut self.position, self.ast.expr(inner).span.start);
            let result = self.class_definition(id, dst, Some(key));
            self.position = outer;
            self.leave(super::function::EXPR_WEIGHT);
            return result;
        }
        let v = self.expr(e, dst)?;
        if self.is_anonymous_function(e) {
            self.emit(Insn::SetFunctionName {
                func: v,
                key,
                prefix: NAME_PREFIX_NONE,
            });
        }
        Ok(v)
    }

    /// The class whose constructor or initializer function this is.
    fn own_class(&self) -> CResult<ClassId> {
        self.session
            .function_classes
            .get(&self.id)
            .copied()
            .ok_or(Self::internal("a class function without its class"))
    }

    /// The start of a base class constructor (§10.2.2 step 6.b):
    /// `InitializeInstanceElements(this, F)` before the parameters.
    pub(super) fn base_constructor_fields(&mut self) -> CResult<()> {
        let class = *self.ast.class(self.own_class()?);
        if class.instance_init.is_none() && !self.has_instance_private_methods(&class) {
            return Ok(());
        }
        let mark = self.mark();
        let this = self.temp()?;
        let function = self.temp()?;
        self.position = self.parameters_position(self.function.span.start);
        self.emit(Insn::This { dst: this });
        self.emit(Insn::Callee { dst: function });
        self.emit(Insn::InitFields {
            this,
            func: function,
        });
        self.release(mark);
        Ok(())
    }

    /// Whether a class has instance private methods or accessors (its
    /// instances get its brand).
    fn has_instance_private_methods(&self, class: &swb_js_syntax::Class) -> bool {
        self.ast
            .class_elements(class.elements)
            .iter()
            .any(|element| {
                !element.is_static
                    && matches!(element.key, ClassKey::Private(_))
                    && element.kind != ClassElementKind::Field
            })
    }

    /// The code of an initializer function: each field of its kind
    /// (instance or static) in order, with the static blocks
    /// (§15.7.14 step 29, §7.3.34 `InitializeInstanceElements`).
    pub(super) fn initializer_body(&mut self) -> CResult<()> {
        let class = *self.ast.class(self.own_class()?);
        let is_static = self.function.kind == FunctionKind::StaticInitializer;
        let this = self.temp()?;
        self.emit(Insn::This { dst: this });
        let has_private_fields = self
            .ast
            .class_elements(class.elements)
            .iter()
            .any(|element| {
                element.is_static == is_static
                    && element.kind == ClassElementKind::Field
                    && matches!(element.key, ClassKey::Private(_))
            });
        let env = if has_private_fields {
            let env = self.temp()?;
            self.emit(Insn::LoadPrivateEnv { dst: env });
            Some(env)
        } else {
            None
        };
        let mut next_key = 0u32;
        for element in self.ast.class_elements(class.elements).to_vec() {
            let index = is_computed_field(&element).then(|| {
                next_key += 1;
                next_key - 1
            });
            if element.is_static != is_static {
                continue;
            }
            self.position = element.span.start;
            let mark = self.mark();
            match element.kind {
                ClassElementKind::Field => {
                    self.field_definition(&element, (this, env), index)?;
                }
                ClassElementKind::StaticBlock { body, scope } => {
                    self.enter_scope(scope)?;
                    for &statement in self.ast.stmts(body) {
                        self.stmt(statement)?;
                    }
                }
                _ => {}
            }
            self.release(mark);
        }
        Ok(())
    }

    /// `DefineField` (§7.3.33) of one field on `this`; `index` is the
    /// position of a computed key in the class's key array; `env` holds
    /// the private environment for a private field.
    fn field_definition(
        &mut self,
        element: &ClassElement,
        (this, env): (Reg, Option<Reg>),
        index: Option<u32>,
    ) -> CResult<()> {
        let v = self.temp()?;
        match (element.key, index) {
            (_, Some(index)) => {
                let k = self.temp()?;
                self.emit(Insn::FieldKey { dst: k, index });
                match element.value {
                    Some(value) => {
                        self.expr_named_by_key(value, Some(v), k)?;
                    }
                    None => {
                        self.emit(Insn::Undefined { dst: v });
                    }
                }
                self.position = element.span.start;
                self.emit(Insn::DefineField {
                    obj: this,
                    key: k,
                    src: v,
                });
            }
            (ClassKey::Property(key), None) => {
                let text = self
                    .key_text(key)
                    .ok_or(Self::internal("a field key without a text"))?;
                match element.value {
                    Some(value) => {
                        self.expr_named(value, Some(v), text.clone())?;
                    }
                    None => {
                        self.emit(Insn::Undefined { dst: v });
                    }
                }
                let site = self.site(text);
                self.position = element.span.start;
                self.emit(Insn::DefineFieldNamed {
                    obj: this,
                    src: v,
                    site,
                });
            }
            (ClassKey::Private(ident), None) => {
                let env = env.ok_or(Self::internal("a private field without its environment"))?;
                let name = self.name_text(ident.name);
                match element.value {
                    Some(value) => {
                        self.expr_named(value, Some(v), name)?;
                    }
                    None => {
                        self.emit(Insn::Undefined { dst: v });
                    }
                }
                let index = self.private_index(ident)?;
                // Chromium reports a field that is added twice at its
                // initializer, or at its name without one.
                self.position = element
                    .value
                    .map_or(element.span.start, |value| self.ast.expr(value).span.start);
                self.emit(Insn::DefinePrivateField {
                    obj: this,
                    src: v,
                    env,
                    index,
                });
            }
            (ClassKey::StaticBlock, None) => {
                return Err(Self::internal("a field without a key"));
            }
        }
        Ok(())
    }

    // --- super(...) ---

    /// `super(arguments)` (§13.3.7.1): the parent constructor (the active
    /// function's prototype), `new.target`, the arguments, the
    /// construction, the `this` binding (once), the instance fields.
    pub(super) fn super_call(&mut self, id: SuperCallId, dst: Option<Reg>) -> CResult<Reg> {
        let call = *self.ast.super_call(id);
        let arguments = self.ast.exprs(call.arguments).to_vec();
        let Ok(argc) = u16::try_from(arguments.len()) else {
            return Err(super::CompileError::Script {
                kind: crate::error::ThrowKind::SyntaxError,
                message: swb_js_syntax::messages::TOO_MANY_ARGUMENTS.into(),
                offset: self.position,
            });
        };
        let spread = arguments
            .iter()
            .any(|&e| matches!(self.ast.expr(e).kind, ExprKind::Spread(_)));
        let position = self.position;
        let t = self.target(dst)?;
        let mark = self.mark();
        let active = self.temp()?;
        self.load_ref(call.function, Some(active))?;
        let base = self.temps(2 + if spread { 1 } else { arguments.len() })?;
        if base != active + 1 {
            return Err(Self::internal(
                "a super call window apart from its function",
            ));
        }
        self.emit(Insn::SuperConstructor {
            dst: base,
            func: active,
        });
        self.load_ref(call.new_target, Some(base + 1))?;
        let mut next = base + 2;
        if spread {
            self.emit(Insn::NewArray { dst: next });
            for &argument in &arguments {
                self.array_element(next, argument, ITER_SPREAD, Some(position))?;
            }
        } else {
            for &argument in &arguments {
                self.expr(argument, Some(next))?;
                next += 1;
            }
        }
        self.position = position;
        if spread {
            self.emit(Insn::SuperCallSpread { callee: base });
        } else {
            self.emit(Insn::SuperCall { callee: base, argc });
        }
        let check = self.temp()?;
        self.load_this_raw(call.this, check)?;
        self.emit(Insn::CheckSuperOnce { src: check });
        self.store_this_raw(call.this, base)?;
        self.emit(Insn::InitFields {
            this: base,
            func: active,
        });
        self.emit(Insn::Move { dst: t, src: base });
        self.release(mark);
        Ok(t)
    }

    /// Loads the `this` binding without the check of its initialization.
    fn load_this_raw(&mut self, id: RefId, dst: Reg) -> CResult<()> {
        match self.this_capture(id) {
            Resolution::Register(r) => self.emit(Insn::Move { dst, src: r }),
            Resolution::Cell(cell) => self.emit(Insn::LoadCell { dst, cell }),
            Resolution::Capture(index) => self.emit(Insn::LoadCapture { dst, index }),
            _ => return Err(Self::internal("a this binding without storage")),
        };
        Ok(())
    }

    /// Initializes the `this` binding (`BindThisValue`, §9.1.1.3.1).
    fn store_this_raw(&mut self, id: RefId, src: Reg) -> CResult<()> {
        match self.this_capture(id) {
            Resolution::Register(r) => self.emit(Insn::Move { dst: r, src }),
            Resolution::Cell(cell) => self.emit(Insn::StoreCell { cell, src }),
            Resolution::Capture(index) => self.emit(Insn::StoreCapture { src, index }),
            _ => return Err(Self::internal("a this binding without storage")),
        };
        Ok(())
    }

    /// The storage of a `this` reference; in eval code, the capture of the
    /// caller's binding.
    fn this_capture(&self, id: RefId) -> Resolution {
        match self.reference(id).resolution {
            Resolution::Caller => self
                .caller_access(id)
                .and_then(|access| access.capture)
                .map_or(Resolution::Unresolved, Resolution::Capture),
            resolution => resolution,
        }
    }

    /// A return of a derived constructor: the value goes into the result
    /// register, and a jump to the epilogue (§10.2.2 steps 10 to 12 run
    /// after the function body, so no handler of the body catches their
    /// errors).
    pub(super) fn derived_return(&mut self, value: Reg, (result, _): (Reg, Reg)) {
        if value != result {
            self.emit(Insn::Move {
                dst: result,
                src: value,
            });
        }
        let at = self.emit(Insn::Jump { offset: 0 });
        self.derived_exits.push(at);
    }

    /// The epilogue of a derived constructor, after all of its code: the
    /// result is checked against the `this` binding.
    ///
    /// The end of the body falls into a check of its own, at V8's position
    /// (the start of the last statement, or the parameter list of an empty
    /// body); the `return` statements jump to a check at the closing
    /// brace.
    pub(super) fn derived_epilogue(&mut self, registers: (Reg, Reg)) {
        let mut exits = std::mem::take(&mut self.derived_exits);
        if let Some(at) = exits.pop() {
            self.patch_here(at);
        }
        self.position = match self.ast.stmts(self.function.body).last() {
            Some(&last) => self.ast.stmt(last).span.start,
            None => self.parameters_position(self.function.span.start),
        };
        self.derived_check(registers);
        if exits.is_empty() {
            return;
        }
        for at in exits {
            self.patch_here(at);
        }
        self.position = self.function.span.end.saturating_sub(1);
        self.derived_check(registers);
    }

    /// The check of a derived constructor's result in `result` and its
    /// return.
    fn derived_check(&mut self, (result, this): (Reg, Reg)) {
        let this_register = match self
            .fscope
            .this_binding
            .map(|b| self.scopes.binding(b).storage)
        {
            Some(swb_js_syntax::Storage::Register(r)) => r,
            Some(swb_js_syntax::Storage::Cell(cell)) => {
                self.emit(Insn::LoadCell { dst: this, cell });
                this
            }
            _ => {
                self.emit(Insn::Empty { dst: this });
                this
            }
        };
        self.emit(Insn::DerivedReturn {
            dst: result,
            this: this_register,
        });
        self.emit(Insn::Return { src: result });
    }

    // --- super properties ---

    /// Evaluates a `super` property reference (§13.3.7.1): the super base
    /// in `base` and `this` in `base + 1` (returned), and the key (a site,
    /// or a register with the value of the key expression, not yet
    /// converted).
    pub(super) fn super_reference(&mut self, e: ExprId) -> CResult<(Reg, KeyReg)> {
        let (this, home, key) = match self.ast.expr(e).kind {
            ExprKind::SuperMember {
                property,
                this,
                home,
            } => (this, home, Err(self.name_text(property))),
            ExprKind::SuperIndex { index, this, home } => match self.constant_key(index) {
                Some(text) => (this, home, Err(text)),
                None => (this, home, Ok(index)),
            },
            _ => return Err(Self::internal("a super reference that is not super")),
        };
        let base = self.temps(2)?;
        self.load_ref(this, Some(base + 1))?;
        let key = match key {
            Ok(index) => {
                let k = self.temp()?;
                self.expr(index, Some(k))?;
                KeyReg::Register(k)
            }
            Err(text) => KeyReg::Site(self.site(text)),
        };
        self.load_ref(home, Some(base))?;
        self.emit(Insn::SuperBase {
            dst: base,
            home: base,
        });
        Ok((base, key))
    }

    /// `GetValue` of a `super` reference into `dst`.
    pub(super) fn emit_get_super(&mut self, dst: Reg, base: Reg, key: KeyReg) -> CResult<()> {
        match key {
            KeyReg::Site(site) => self.emit(Insn::GetSuperNamed { dst, base, site }),
            KeyReg::Register(key) => self.emit(Insn::GetSuper { dst, base, key }),
            KeyReg::Private { .. } => return Err(Self::internal("a private super reference")),
        };
        Ok(())
    }

    /// `PutValue` of `src` on a `super` reference.
    pub(super) fn emit_put_super(&mut self, base: Reg, key: KeyReg, src: Reg) -> CResult<()> {
        match key {
            KeyReg::Site(site) => self.emit(Insn::SetSuperNamed { base, src, site }),
            KeyReg::Register(key) => self.emit(Insn::SetSuper { base, key, src }),
            KeyReg::Private { .. } => return Err(Self::internal("a private super reference")),
        };
        Ok(())
    }

    /// Converts a computed key in place, for a reference that is read and
    /// then written (`ToPropertyKey` once).
    pub(super) fn key_once(&mut self, key: KeyReg) {
        if let KeyReg::Register(k) = key {
            self.emit(Insn::ToKey { dst: k, src: k });
        }
    }

    /// A `super` property read into `dst`.
    pub(super) fn super_load(&mut self, e: ExprId, dst: Option<Reg>) -> CResult<Reg> {
        let t = self.target(dst)?;
        let mark = self.mark();
        let (base, key) = self.super_reference(e)?;
        self.position = self.ast.expr(e).span.start;
        self.emit_get_super(t, base, key)?;
        self.release(mark);
        Ok(t)
    }

    /// The callee of a call `super.m(...)`: the function into `base`,
    /// `this` into `base + 1`.
    pub(super) fn super_callee(&mut self, e: ExprId, base: Reg) -> CResult<()> {
        let mark = self.mark();
        let (pair, key) = self.super_reference(e)?;
        self.emit(Insn::Move {
            dst: base + 1,
            src: pair + 1,
        });
        self.emit_get_super(base, pair, key)?;
        self.release(mark);
        Ok(())
    }

    /// `delete super.x` (§13.5.1.2 step 5.b): the reference is evaluated
    /// (`this`, the key expression), then a `ReferenceError`.
    pub(super) fn super_delete(&mut self, e: ExprId) -> CResult<()> {
        let mark = self.mark();
        let (this, index) = match self.ast.expr(e).kind {
            ExprKind::SuperMember { this, .. } => (this, None),
            ExprKind::SuperIndex { index, this, .. } => (this, Some(index)),
            _ => return Err(Self::internal("a super reference that is not super")),
        };
        self.load_ref(this, None)?;
        if let Some(index) = index {
            self.effect(index)?;
        }
        self.emit(Insn::ThrowSuperDelete);
        self.release(mark);
        Ok(())
    }

    /// Whether a `this` reference needs the check of a derived
    /// constructor's `this` binding (before `super(...)`).
    pub(super) fn is_derived_this(&self, reference: &swb_js_syntax::Reference) -> bool {
        let Some(binding) = reference.binding else {
            // Eval code: `load_caller` checks the caller's binding.
            return false;
        };
        let binding = self.scopes.binding(binding);
        binding.kind == BindingKind::This
            && self
                .ast
                .function(self.scopes.scope(binding.scope).function)
                .kind
                == FunctionKind::DerivedConstructor
    }
}

/// Whether a class element is a field with a computed key.
fn is_computed_field(element: &ClassElement) -> bool {
    element.kind == ClassElementKind::Field
        && matches!(element.key, ClassKey::Property(AstKey::Computed(_)))
}

/// The code of the default constructor of the class `id` (§15.7.14 step
/// 14): a base class creates the object (the `[[Construct]]` of the
/// constructor does) and defines its fields; a derived class constructs
/// its parent with the arguments and `new.target`, then defines its
/// fields. Its source text is the class's.
pub(super) fn default_constructor(
    compiler: &FunctionCompiler<'_>,
    id: ClassId,
    name: String16,
) -> Builder {
    let class = compiler.ast.class(id);
    let derived = class.heritage.is_some();
    let insns = if derived {
        vec![
            Insn::Callee { dst: 0 },
            Insn::SuperConstructor { dst: 1, func: 0 },
            Insn::NewTarget { dst: 2 },
            Insn::RestArguments { dst: 3, start: 0 },
            Insn::SuperCallSpread { callee: 1 },
            Insn::InitFields { this: 1, func: 0 },
            Insn::Return { src: 1 },
        ]
    } else {
        vec![
            Insn::Callee { dst: 0 },
            Insn::This { dst: 1 },
            Insn::InitFields { this: 1, func: 0 },
            Insn::Return { src: 1 },
        ]
    };
    Builder {
        insns,
        constants: Vec::new(),
        site_keys: Vec::new(),
        sites: Vec::new(),
        captures: Vec::new(),
        globals: Vec::new(),
        register_count: if derived { 4 } else { 2 },
        param_count: 0,
        length: 0,
        kind: if derived {
            CodeKind::DerivedConstructor
        } else {
            CodeKind::ClassConstructor
        },
        this_mode: ThisMode::Strict,
        strict: true,
        strict_ranges: Vec::new(),
        keeps_extra_args: derived,
        name: Some(name),
        lines: vec![(0, class.span.start)],
        call_names: Vec::new(),
        handlers: Vec::new(),
        span: (class.span.start, class.span.end),
        eval: None,
    }
}

/// The name of an initializer function in stack traces.
pub(super) fn initializer_name(kind: FunctionKind) -> Option<String16> {
    match kind {
        FunctionKind::InstanceInitializer => Some(String16::from(INSTANCE_INITIALIZER)),
        FunctionKind::StaticInitializer => Some(String16::from(STATIC_INITIALIZER)),
        _ => None,
    }
}
