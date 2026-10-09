//! Realms (ECMA-262 §9.3, ADR 0026 sections 3 and 12): the global object,
//! the global lexical bindings and the intrinsic objects.
//!
//! The global environment record (§9.1.1.4) has two parts: a declarative
//! record for the script-level `let` and `const` bindings ([`Realm::lexical`],
//! with `Empty` while a binding is in its temporal dead zone) and the
//! global object for `var` and function declarations. Both are accessed by
//! name.
//!
//! The intrinsics live in an array indexed by [`Intrinsic`] (§6.1.7.4).
//! This session creates only what the interpreter needs: the prototypes of
//! objects, functions, arrays, the primitive wrappers, generators and the
//! error types (with `name` and `message`), and `next` on the generator
//! prototype. The full built-in objects come in session 5.

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
}

/// The number of [`Intrinsic`] values.
const INTRINSIC_COUNT: usize = 14;

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
        let message_key = PropertyKey::String(self.vm.atoms.message);
        for (proto, name) in [
            (error_proto, "Error"),
            (type_error_proto, "TypeError"),
            (range_error_proto, "RangeError"),
            (reference_error_proto, "ReferenceError"),
            (syntax_error_proto, "SyntaxError"),
        ] {
            let name = self.heap.intern_str(name)?;
            self.define(proto, name_key, name.into(), true, false, true)?;
            let empty = self.vm.atoms.empty;
            self.define(proto, message_key, empty.into(), true, false, true)?;
        }
        // Function.prototype is a function with length 0 and name "".
        let length = PropertyKey::String(self.heap.length_atom());
        self.define(function_proto, length, Value::Int(0), false, false, true)?;
        let empty = self.vm.atoms.empty;
        self.define(function_proto, name_key, empty.into(), false, false, true)?;
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
    /// realm: an ordinary object with the error prototype and an own
    /// `message` (§20.5.6.1.1; `stack` and the constructors come with the
    /// built-ins).
    pub(crate) fn error_object(
        &mut self,
        kind: crate::error::ThrowKind,
        message: &str,
    ) -> VmResult<Gc<Object>> {
        use crate::error::ThrowKind;
        let proto = match kind {
            ThrowKind::Error => Intrinsic::ErrorPrototype,
            ThrowKind::TypeError => Intrinsic::TypeErrorPrototype,
            ThrowKind::RangeError => Intrinsic::RangeErrorPrototype,
            ThrowKind::ReferenceError => Intrinsic::ReferenceErrorPrototype,
            ThrowKind::SyntaxError => Intrinsic::SyntaxErrorPrototype,
        };
        let proto = self.intrinsic(self.current_realm(), proto)?;
        let object = self.heap.new_object(Some(proto))?;
        let text = self.heap.alloc_str(message)?;
        // The object and the text are the arguments of the definition, so
        // they are rooted during its reservation.
        let key = PropertyKey::String(self.vm.atoms.message);
        self.define(object, key, text.into(), true, false, true)?;
        Ok(object)
    }

    /// The native functions of the realm's intrinsics (after the realm is
    /// a root).
    fn install_realm_functions(&mut self, realm: u32) -> VmResult<()> {
        let scope = self.heap.open_scope();
        let generator_proto = self.intrinsic(realm, Intrinsic::GeneratorPrototype)?;
        let next = self.new_native_function(realm, "next", 1, crate::vm::generator::next)?;
        let key = self.heap.key_from_str("next")?;
        self.define(generator_proto, key, next.into(), true, false, true)?;
        self.heap.close_scope(scope)?;
        Ok(())
    }
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
