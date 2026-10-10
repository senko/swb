//! Realms (ECMA-262 §9.3, ADR 0026 sections 3 and 12): the global object,
//! the global lexical bindings and the intrinsic objects.
//!
//! The global environment record (§9.1.1.4) has two parts: a declarative
//! record for the script-level `let` and `const` bindings ([`Realm::lexical`],
//! with `Empty` while a binding is in its temporal dead zone) and the
//! global object for `var` and function declarations. Both are accessed by
//! name.
//!
//! The intrinsics live in an array indexed by [`Intrinsic`] (§6.1.7.4):
//! the prototypes that the interpreter needs (objects, functions, arrays,
//! the primitive wrappers, generators, the error types) and
//! `%ThrowTypeError%`. The realm is built in a no-GC region; then the
//! built-in functions are installed ([`crate::builtins`]).

use crate::heap::{Gc, HandleMap, Tracer};
use crate::object::{Object, ObjectKind};
use crate::runtime::Runtime;
use crate::string::{JsString, PropertyKey};
use crate::value::Value;
use crate::vm::{VmError, VmResult};

/// The intrinsic objects of a realm that the VM uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(clippy::enum_variant_names, reason = "the names of the specification")]
pub(crate) enum Intrinsic {
    ObjectPrototype,
    FunctionPrototype,
    ArrayPrototype,
    StringPrototype,
    NumberPrototype,
    BooleanPrototype,
    SymbolPrototype,
    GeneratorPrototype,
    GeneratorFunctionPrototype,
    ErrorPrototype,
    TypeErrorPrototype,
    RangeErrorPrototype,
    ReferenceErrorPrototype,
    SyntaxErrorPrototype,
    EvalErrorPrototype,
    UriErrorPrototype,
    AggregateErrorPrototype,
    /// `%ThrowTypeError%` (§10.2.4.1).
    ThrowTypeError,
    /// `%IteratorPrototype%` (§27.1.2).
    IteratorPrototype,
    /// `%ArrayIteratorPrototype%` (§23.1.5.2).
    ArrayIteratorPrototype,
    /// `Function.prototype[@@hasInstance]` (§20.2.3.6), set when the
    /// built-in functions are installed.
    FunctionHasInstance,
    /// `%Array.prototype.values%` (§23.1.3.40), set when the built-in
    /// functions are installed; the `@@iterator` of arguments objects.
    ArrayPrototypeValues,
    /// `Error`, set when the built-in functions are installed (for
    /// `Error.stackTraceLimit`).
    ErrorConstructor,
    /// `Array`, set when the built-in functions are installed.
    ArrayConstructor,
    /// The getter and setter of the `stack` accessor of error objects.
    StackGetter,
    StackSetter,
}

/// The number of [`Intrinsic`] values.
const INTRINSIC_COUNT: usize = 26;

/// A binding of the global declarative record.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LexicalBinding {
    /// The value; `Empty` until the declaration runs.
    pub(crate) value: Value,
    /// `let` (true) or `const` (false).
    pub(crate) mutable: bool,
}

/// A realm.
pub(crate) struct Realm {
    /// The global object (in a browser, behind `WindowProxy`).
    pub(crate) global: Gc<Object>,
    /// `None` for the two intrinsics that exist only after the built-in
    /// functions are installed, until `set_intrinsic` runs.
    intrinsics: [Option<Gc<Object>>; INTRINSIC_COUNT],
    /// The global `let` and `const` bindings, by atom (a handle-keyed
    /// map: the keys are atoms, whose handles a script cannot choose).
    pub(crate) lexical: HandleMap<Gc<JsString>, LexicalBinding>,
}

impl Realm {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.object(self.global);
        for object in self.intrinsics.iter().flatten() {
            tracer.object(*object);
        }
        for (name, binding) in &self.lexical {
            tracer.string(*name);
            tracer.value(binding.value);
        }
    }

    /// Replaces an intrinsic (for objects that exist only after the
    /// built-in functions are installed).
    pub(crate) fn set_intrinsic(&mut self, which: Intrinsic, object: Gc<Object>) {
        if let Some(slot) = self.intrinsics.get_mut(which as usize) {
            *slot = Some(object);
        }
    }

    /// An intrinsic object; `None` while it is not set yet.
    pub(crate) fn intrinsic(&self, which: Intrinsic) -> Option<Gc<Object>> {
        // The array has one entry per variant, in declaration order.
        self.intrinsics.get(which as usize).copied().flatten()
    }
}

impl Runtime {
    /// An intrinsic object of a realm.
    pub(crate) fn intrinsic(&self, realm: u32, which: Intrinsic) -> VmResult<Gc<Object>> {
        self.vm
            .realms
            .get(realm as usize)
            .ok_or(VmError::invariant("a realm index without a realm"))?
            .intrinsic(which)
            .ok_or(VmError::invariant("an intrinsic that is not set yet"))
    }

    /// Sets an intrinsic of a realm that exists only after the built-in
    /// functions are installed.
    pub(crate) fn set_intrinsic(&mut self, realm: u32, which: Intrinsic, object: Gc<Object>) {
        if let Some(realm) = self.vm.realms.get_mut(realm as usize) {
            realm.set_intrinsic(which, object);
        }
    }

    /// The global object of a realm.
    pub(crate) fn global_object(&self, realm: u32) -> VmResult<Gc<Object>> {
        self.vm
            .realms
            .get(realm as usize)
            .map(|r| r.global)
            .ok_or(VmError::invariant("a realm index without a realm"))
    }

    /// Creates a realm with its intrinsics and global object (§9.3.1
    /// `InitializeHostDefinedRealm`) and returns its index. Runs in a no-GC
    /// region: nothing of the realm is rooted until it is complete.
    pub(crate) fn create_realm(&mut self) -> VmResult<u32> {
        let depth = self.heap.no_gc_depth();
        self.heap.enter_no_gc();
        let result = self.build_realm();
        self.heap.restore_no_gc_depth(depth);
        let realm = result?;
        let index = u32::try_from(self.vm.realms.len())
            .map_err(|_| VmError::invariant("too many realms"))?;
        self.vm.realms.push(realm);
        self.install_realm_functions(index)?;
        Ok(index)
    }

    /// Creates the realm's objects: the prototypes, `%ThrowTypeError%` and
    /// the global object, without their properties. `index` is the index
    /// that the realm will have.
    fn create_realm_objects(&mut self, index: u32) -> VmResult<Realm> {
        let heap = &mut self.heap;
        let object_proto = heap.new_object(None)?;
        let function_proto = heap.new_object_with_kind(
            Some(object_proto),
            ObjectKind::Native(Box::new(crate::vm::NativeFunction {
                func: function_prototype,
                realm: index,
                constructor: false,
                name: None,
            })),
        )?;
        let array_proto = heap.new_array(Some(object_proto), 0)?;
        let empty = self.vm.atoms.empty;
        let string_proto =
            heap.new_object_with_kind(Some(object_proto), ObjectKind::StringWrapper(empty))?;
        let number_proto =
            heap.new_object_with_kind(Some(object_proto), ObjectKind::NumberWrapper(0.0))?;
        let boolean_proto =
            heap.new_object_with_kind(Some(object_proto), ObjectKind::BooleanWrapper(false))?;
        let symbol_proto = heap.new_object(Some(object_proto))?;
        let iterator_proto = heap.new_object(Some(object_proto))?;
        let array_iterator_proto = heap.new_object(Some(iterator_proto))?;
        let generator_proto = heap.new_object(Some(iterator_proto))?;
        let generator_function_proto = heap.new_object(Some(function_proto))?;
        let error_proto = heap.new_object(Some(object_proto))?;
        let type_error_proto = heap.new_object(Some(error_proto))?;
        let range_error_proto = heap.new_object(Some(error_proto))?;
        let reference_error_proto = heap.new_object(Some(error_proto))?;
        let syntax_error_proto = heap.new_object(Some(error_proto))?;
        let eval_error_proto = heap.new_object(Some(error_proto))?;
        let uri_error_proto = heap.new_object(Some(error_proto))?;
        let aggregate_error_proto = heap.new_object(Some(error_proto))?;
        let throw_type_error = heap.new_object_with_kind(
            Some(function_proto),
            ObjectKind::Native(Box::new(crate::vm::NativeFunction {
                func: throw_type_error,
                realm: index,
                constructor: false,
                name: None,
            })),
        )?;
        let global = heap.new_object(Some(object_proto))?;
        let intrinsics = [
            Some(object_proto),
            Some(function_proto),
            Some(array_proto),
            Some(string_proto),
            Some(number_proto),
            Some(boolean_proto),
            Some(symbol_proto),
            Some(generator_proto),
            Some(generator_function_proto),
            Some(error_proto),
            Some(type_error_proto),
            Some(range_error_proto),
            Some(reference_error_proto),
            Some(syntax_error_proto),
            Some(eval_error_proto),
            Some(uri_error_proto),
            Some(aggregate_error_proto),
            Some(throw_type_error),
            Some(iterator_proto),
            Some(array_iterator_proto),
            // Set by `set_intrinsic` when the function exists.
            None,
            None,
            None,
            None,
            None,
            None,
        ];
        let realm = Realm {
            global,
            intrinsics,
            lexical: HandleMap::default(),
        };
        Ok(realm)
    }

    fn build_realm(&mut self) -> VmResult<Realm> {
        let index = u32::try_from(self.vm.realms.len())
            .map_err(|_| VmError::invariant("too many realms"))?;
        let realm = self.create_realm_objects(index)?;
        // The realm is not a root yet, but the region does not collect.
        self.define_prototype_properties(&realm)?;
        self.define_global_values(realm.global)?;
        Ok(realm)
    }

    /// The `length` and `name` of `%Function.prototype%` and
    /// `%ThrowTypeError%`, and the `length` of `%String.prototype%`.
    fn define_prototype_properties(&mut self, realm: &Realm) -> VmResult<()> {
        let string_proto = realm
            .intrinsic(Intrinsic::StringPrototype)
            .ok_or(VmError::invariant("an intrinsic that is not set yet"))?;
        let function_proto = realm
            .intrinsic(Intrinsic::FunctionPrototype)
            .ok_or(VmError::invariant("an intrinsic that is not set yet"))?;
        let throw_type_error = realm
            .intrinsic(Intrinsic::ThrowTypeError)
            .ok_or(VmError::invariant("an intrinsic that is not set yet"))?;
        let length = PropertyKey::String(self.heap.length_atom());
        self.define(string_proto, length, Value::Int(0), false, false, false)?;
        let name_key = PropertyKey::String(self.vm.atoms.name);
        // Function.prototype is a function with length 0 and name "".
        let empty = self.vm.atoms.empty;
        self.define(function_proto, length, Value::Int(0), false, false, true)?;
        self.define(function_proto, name_key, empty.into(), false, false, true)?;
        // AddRestrictedFunctionProperties (§10.2.4): `arguments` and
        // `caller` throw. Chromium's order of the keys.
        for name in ["arguments", "caller"] {
            let key = self.heap.key_from_str(name)?;
            let thrower: Value = throw_type_error.into();
            let desc = crate::object::PropertyDescriptor::accessor(thrower, thrower, false, true);
            self.heap
                .define_own_property(function_proto, key, desc, &self.vm)?;
        }
        // %ThrowTypeError%: length 0 and name "", both fixed; not
        // extensible (§10.2.4.1).
        self.define(throw_type_error, length, Value::Int(0), false, false, false)?;
        self.define(
            throw_type_error,
            name_key,
            empty.into(),
            false,
            false,
            false,
        )?;
        self.heap.prevent_extensions(throw_type_error)?;
        Ok(())
    }

    /// The value properties of the global object (§19.1).
    fn define_global_values(&mut self, global: Gc<Object>) -> VmResult<()> {
        for (name, value) in [
            ("undefined", Value::Undefined),
            ("NaN", Value::Double(f64::NAN)),
            ("Infinity", Value::Double(f64::INFINITY)),
        ] {
            let key = self.heap.key_from_str(name)?;
            self.define(global, key, value, false, false, false)?;
        }
        let key = self.heap.key_from_str("globalThis")?;
        self.define(global, key, global.into(), true, false, true)
    }

    /// Creates the error object of a [`VmError::Raise`] in the current
    /// realm: an error object with the error prototype, a `stack` and an
    /// own `message`, as the constructors create it (§20.5.6.1.1). The
    /// stack is that of the frames at the time of the call, so the
    /// caller creates the object before it removes any frame.
    pub(crate) fn error_object(
        &mut self,
        kind: crate::error::ThrowKind,
        message: &str,
    ) -> VmResult<Gc<Object>> {
        // The object is rooted in a scope of its own while it is built;
        // the caller stores it before anything else can collect.
        self.scoped(|rt| {
            let proto = rt.intrinsic(rt.current_realm(), error_prototype(kind))?;
            let object = rt
                .heap
                .new_object_with_kind(Some(proto), ObjectKind::Error(None))?;
            rt.heap.record(object);
            rt.install_stack(object, None)?;
            let text = rt.heap.alloc_str(message)?;
            rt.heap.record(text);
            let key = PropertyKey::String(rt.vm.atoms.message);
            rt.define(object, key, text.into(), true, false, true)?;
            Ok(object)
        })
    }

    /// The native functions of the realm's intrinsics and the built-in
    /// objects (after the realm is a root).
    fn install_realm_functions(&mut self, realm: u32) -> VmResult<()> {
        crate::builtins::install(self, realm)?;
        self.scoped(|rt| {
            let generator_proto = rt.intrinsic(realm, Intrinsic::GeneratorPrototype)?;
            let methods: [(&str, crate::vm::NativeFn); 3] = [
                ("next", crate::vm::generator::next),
                ("return", crate::vm::generator::return_),
                ("throw", crate::vm::generator::throw),
            ];
            for (name, func) in methods {
                let function = rt.new_native_function(realm, name, 1, func)?;
                let key = rt.heap.key_from_str(name)?;
                rt.define(generator_proto, key, function.into(), true, false, true)?;
            }
            crate::builtins::to_string_tag(rt, generator_proto, "Generator")?;
            let generator_function_proto =
                rt.intrinsic(realm, Intrinsic::GeneratorFunctionPrototype)?;
            crate::builtins::to_string_tag(rt, generator_function_proto, "GeneratorFunction")
        })
    }
}

/// The intrinsic prototype of the error objects of a kind.
pub(crate) fn error_prototype(kind: crate::error::ThrowKind) -> Intrinsic {
    use crate::error::ThrowKind;
    match kind {
        ThrowKind::Error => Intrinsic::ErrorPrototype,
        ThrowKind::TypeError => Intrinsic::TypeErrorPrototype,
        ThrowKind::RangeError => Intrinsic::RangeErrorPrototype,
        ThrowKind::ReferenceError => Intrinsic::ReferenceErrorPrototype,
        ThrowKind::SyntaxError => Intrinsic::SyntaxErrorPrototype,
        ThrowKind::EvalError => Intrinsic::EvalErrorPrototype,
        ThrowKind::UriError => Intrinsic::UriErrorPrototype,
    }
}

/// `%ThrowTypeError%` (§10.2.4.1): the accessor of `callee` on the
/// arguments objects of strict functions, and the getter and setter of
/// `caller` and `arguments` on `Function.prototype`. V8's messages.
fn throw_type_error(
    rt: &mut Runtime,
    call: &crate::vm::NativeCall,
) -> VmResult<crate::vm::NativeReturn> {
    // The one function is both getter and setter; a setter gets the
    // value. V8 words an assignment to `callee` as a write to a
    // read-only property (measured in Node.js 22), but an assignment to
    // `caller` or `arguments` of a function as the access error.
    let on_arguments = matches!(
        call.this(),
        Value::Object(object) if matches!(rt.heap.object(object)?.kind, ObjectKind::Arguments)
    );
    if call.argc() > 0 && on_arguments {
        return Err(VmError::type_error(
            "Cannot assign to read only property 'callee' of object '#<Object>'",
        ));
    }
    Err(VmError::type_error(
        "'caller', 'callee', and 'arguments' properties may not be accessed on strict mode functions or the arguments objects for calls to them",
    ))
}

/// `%Function.prototype%` called as a function: returns `undefined`
/// (§20.2.3).
#[allow(
    clippy::unnecessary_wraps,
    reason = "the signature of native functions"
)]
fn function_prototype(
    _: &mut Runtime,
    _: &crate::vm::NativeCall,
) -> VmResult<crate::vm::NativeReturn> {
    Ok(crate::vm::NativeReturn::Value(Value::Undefined))
}
