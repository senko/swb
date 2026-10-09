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
    /// `%ThrowTypeError%` (§10.2.4.1).
    ThrowTypeError,
}

/// The number of [`Intrinsic`] values.
const INTRINSIC_COUNT: usize = 17;

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
    intrinsics: [Gc<Object>; INTRINSIC_COUNT],
    /// The global `let` and `const` bindings, by atom (a handle-keyed
    /// map: the keys are atoms, whose handles a script cannot choose).
    pub(crate) lexical: HandleMap<Gc<JsString>, LexicalBinding>,
}

impl Realm {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.object(self.global);
        for object in &self.intrinsics {
            tracer.object(*object);
        }
        for (name, binding) in &self.lexical {
            tracer.string(*name);
            tracer.value(binding.value);
        }
    }

    /// An intrinsic object.
    pub(crate) fn intrinsic(&self, which: Intrinsic) -> Gc<Object> {
        // The array has one entry per variant, in declaration order.
        self.intrinsics
            .get(which as usize)
            .copied()
            .unwrap_or(self.global)
    }
}

impl Runtime {
    /// An intrinsic object of a realm.
    pub(crate) fn intrinsic(&self, realm: u32, which: Intrinsic) -> VmResult<Gc<Object>> {
        self.vm
            .realms
            .get(realm as usize)
            .map(|r| r.intrinsic(which))
            .ok_or(VmError::invariant("a realm index without a realm"))
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

    fn build_realm(&mut self) -> VmResult<Realm> {
        let heap = &mut self.heap;
        let object_proto = heap.new_object(None)?;
        let index = u32::try_from(self.vm.realms.len())
            .map_err(|_| VmError::invariant("too many realms"))?;
        let function_proto = heap.new_object_with_kind(
            Some(object_proto),
            ObjectKind::Native(Box::new(crate::vm::NativeFunction {
                func: function_prototype,
                realm: index,
                constructor: false,
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
        let generator_proto = heap.new_object(Some(iterator_proto))?;
        let generator_function_proto = heap.new_object(Some(function_proto))?;
        let error_proto = heap.new_object(Some(object_proto))?;
        let type_error_proto = heap.new_object(Some(error_proto))?;
        let range_error_proto = heap.new_object(Some(error_proto))?;
        let reference_error_proto = heap.new_object(Some(error_proto))?;
        let syntax_error_proto = heap.new_object(Some(error_proto))?;
        let eval_error_proto = heap.new_object(Some(error_proto))?;
        let uri_error_proto = heap.new_object(Some(error_proto))?;
        let throw_type_error = heap.new_object_with_kind(
            Some(function_proto),
            ObjectKind::Native(Box::new(crate::vm::NativeFunction {
                func: throw_type_error,
                realm: index,
                constructor: false,
            })),
        )?;
        let global = heap.new_object(Some(object_proto))?;
        let intrinsics = [
            object_proto,
            function_proto,
            array_proto,
            string_proto,
            number_proto,
            boolean_proto,
            symbol_proto,
            generator_proto,
            generator_function_proto,
            error_proto,
            type_error_proto,
            range_error_proto,
            reference_error_proto,
            syntax_error_proto,
            eval_error_proto,
            uri_error_proto,
            throw_type_error,
        ];
        let realm = Realm {
            global,
            intrinsics,
            lexical: HandleMap::default(),
        };
        // Properties. The realm is not a root yet, but the region does
        // not collect.
        let length = PropertyKey::String(self.heap.length_atom());
        self.define(string_proto, length, Value::Int(0), false, false, false)?;
        let name_key = PropertyKey::String(self.vm.atoms.name);
        // Function.prototype is a function with length 0 and name "".
        let length = PropertyKey::String(self.heap.length_atom());
        self.define(function_proto, length, Value::Int(0), false, false, true)?;
        let empty = self.vm.atoms.empty;
        self.define(function_proto, name_key, empty.into(), false, false, true)?;
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
        // The value properties of the global object (§19.1).
        for (name, value) in [
            ("undefined", Value::Undefined),
            ("NaN", Value::Double(f64::NAN)),
            ("Infinity", Value::Double(f64::INFINITY)),
        ] {
            let key = self.heap.key_from_str(name)?;
            self.define(global, key, value, false, false, false)?;
        }
        let key = self.heap.key_from_str("globalThis")?;
        self.define(global, key, global.into(), true, false, true)?;
        Ok(realm)
    }

    /// Creates the error object of a [`VmError::Raise`] in the current
    /// realm: an error object with the error prototype and an own
    /// `message`, as the constructors create it (§20.5.6.1.1; `stack` is
    /// M7).
    pub(crate) fn error_object(
        &mut self,
        kind: crate::error::ThrowKind,
        message: &str,
    ) -> VmResult<Gc<Object>> {
        let proto = self.intrinsic(self.current_realm(), error_prototype(kind))?;
        let object = self
            .heap
            .new_object_with_kind(Some(proto), ObjectKind::Error)?;
        let text = self.heap.alloc_str(message)?;
        // The object and the text are the arguments of the definition, so
        // they are rooted during its reservation.
        let key = PropertyKey::String(self.vm.atoms.message);
        self.define(object, key, text.into(), true, false, true)?;
        Ok(object)
    }

    /// The native functions of the realm's intrinsics and the built-in
    /// objects (after the realm is a root).
    fn install_realm_functions(&mut self, realm: u32) -> VmResult<()> {
        crate::builtins::install(self, realm)?;
        let scope = self.heap.open_scope();
        let generator_proto = self.intrinsic(realm, Intrinsic::GeneratorPrototype)?;
        let methods: [(&str, crate::vm::NativeFn); 3] = [
            ("next", crate::vm::generator::next),
            ("return", crate::vm::generator::return_),
            ("throw", crate::vm::generator::throw),
        ];
        for (name, func) in methods {
            let function = self.new_native_function(realm, name, 1, func)?;
            let key = self.heap.key_from_str(name)?;
            self.define(generator_proto, key, function.into(), true, false, true)?;
        }
        self.heap.close_scope(scope)?;
        Ok(())
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
/// arguments objects of strict functions. V8's message.
fn throw_type_error(
    _: &mut Runtime,
    call: &crate::vm::NativeCall,
) -> VmResult<crate::vm::NativeReturn> {
    // The one function is both getter and setter; a setter gets the
    // value. V8 words an assignment as a write to a read-only property
    // (measured in Node.js 22).
    if call.argc() > 0 {
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
