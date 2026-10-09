//! Function objects (ECMA-262 §10.2, §10.3): closures of compiled code
//! and native functions, and the calling convention of native functions.
//!
//! A closure holds its code, the cells that it captured (flat closures,
//! ADR 0026 section 3) and how it binds `this`; the collector traces them
//! through the object kind. Ordinary functions get their `prototype`
//! object when they are created.
//!
//! Native functions: a Rust function pointer ([`NativeFn`]) with a
//! `length` and a `name`. The interpreter calls it with the arguments in a
//! window of the value stack ([`NativeCall`]) and a handle scope open; the
//! scope closes when the native function returns, also on an error. A
//! native function returns a value, or a request that the interpreter
//! runs as a script-to-script call: a deferred call ([`NativeReturn::Call`],
//! memo 1.2) or the resumption of a generator.

use std::rc::Rc;

use crate::bytecode::{CodeKind, FunctionCode, ThisMode};
use crate::heap::{Gc, Generic, Tracer};
use crate::object::{ObjectKind, PropertyDescriptor};
use crate::runtime::Runtime;
use crate::string::{JsString, PropertyKey};
use crate::value::{Value, ValueCell};
use crate::vm::{Intrinsic, VmError, VmResult};

/// The data of a function object of script code.
pub struct Closure {
    /// The code object.
    pub(crate) code: Gc<Generic>,
    /// The code (shared with the code object).
    pub(crate) compiled: Rc<FunctionCode>,
    /// The captured cells, in the order of the code's capture list.
    pub(crate) captures: Box<[Gc<ValueCell>]>,
    /// How the function binds `this`.
    pub(crate) this_mode: ThisMode,
    /// The function's realm.
    pub(crate) realm: u32,
}

impl Closure {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.generic(self.code);
        for cell in &self.captures {
            tracer.cell(*cell);
        }
    }

    pub(crate) fn heap_size(&self) -> usize {
        size_of::<Closure>() + self.captures.len() * size_of::<Gc<ValueCell>>()
    }
}

/// A native function: called with the runtime and the call's arguments.
pub type NativeFn = fn(&mut Runtime, &NativeCall) -> VmResult<NativeReturn>;

/// The data of a native function object.
pub struct NativeFunction {
    /// The Rust function.
    pub(crate) func: NativeFn,
    /// The function's realm.
    pub(crate) realm: u32,
    /// Whether `new` may call it (it then sees `new.target`).
    pub(crate) constructor: bool,
}

/// The arguments of a native call. The callee, `this` and the arguments
/// are in the value stack (`callee` at `start - 2`, `this` at
/// `start - 1`), so they stay rooted during the call.
#[derive(Clone, Copy, Debug)]
pub struct NativeCall {
    pub(crate) callee: Gc<crate::object::Object>,
    pub(crate) this: Value,
    pub(crate) new_target: Value,
    pub(crate) start: usize,
    pub(crate) argc: usize,
    pub(crate) realm: u32,
}

impl NativeCall {
    /// The `this` value.
    pub fn this(&self) -> Value {
        self.this
    }

    /// The number of arguments.
    pub fn argc(&self) -> usize {
        self.argc
    }

    /// `new.target`: the constructor for a `new` call, otherwise
    /// `undefined`.
    pub fn new_target(&self) -> Value {
        self.new_target
    }

    /// The function object that was called.
    pub fn callee(&self) -> Gc<crate::object::Object> {
        self.callee
    }

    /// The realm of the native function.
    pub fn realm(&self) -> u32 {
        self.realm
    }
}

/// What a native function returns.
#[derive(Clone, Debug, PartialEq)]
pub enum NativeReturn {
    /// The result.
    Value(Value),
    /// A deferred call (memo 1.2): the interpreter calls `callee` with
    /// `this` and `args` as a script-to-script call, and its result is the
    /// result of the native call. The values need no rooting: the
    /// interpreter moves them into the value stack before anything can
    /// collect.
    Call {
        /// The function to call.
        callee: Value,
        /// The `this` value.
        this: Value,
        /// The arguments.
        args: Vec<Value>,
    },
    /// Resume a generator (the `next` method of generator objects).
    Resume(Resume),
}

/// A request to resume a suspended generator with a value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Resume {
    pub(crate) generator: Gc<crate::object::Object>,
    pub(crate) value: Value,
}

impl Runtime {
    /// Creates a closure of `code` with the captured cells: a function
    /// object with `length`, `name` and (for ordinary and generator
    /// functions) `prototype` (§10.2.3 `OrdinaryFunctionCreate`, §10.2.5
    /// `MakeConstructor`). The result is recorded in the current handle
    /// scope.
    pub(crate) fn new_closure(
        &mut self,
        code: Gc<Generic>,
        compiled: &Rc<FunctionCode>,
        captures: Box<[Gc<ValueCell>]>,
        realm: u32,
    ) -> VmResult<Gc<crate::object::Object>> {
        let proto_kind = if compiled.kind == CodeKind::Generator {
            Intrinsic::GeneratorFunctionPrototype
        } else {
            Intrinsic::FunctionPrototype
        };
        let proto = self.intrinsic(realm, proto_kind)?;
        let kind = ObjectKind::Function(Box::new(Closure {
            code,
            compiled: Rc::clone(compiled),
            captures,
            this_mode: compiled.this_mode,
            realm,
        }));
        let function = self.heap.new_object_with_kind(Some(proto), kind)?;
        self.heap.record(function);
        let length = Value::Int(compiled.param_count as i32);
        self.define_function_properties(function, length, compiled.name.into())?;
        match compiled.kind {
            CodeKind::Normal => {
                let object_proto = self.intrinsic(realm, Intrinsic::ObjectPrototype)?;
                let prototype = self.heap.new_object(Some(object_proto))?;
                self.heap.record(prototype);
                let constructor = PropertyKey::String(self.vm.atoms.constructor);
                self.define(prototype, constructor, function.into(), true, false, true)?;
                let key = PropertyKey::String(self.vm.atoms.prototype);
                self.define(function, key, prototype.into(), true, false, false)?;
            }
            CodeKind::Generator => {
                let generator_proto = self.intrinsic(realm, Intrinsic::GeneratorPrototype)?;
                let prototype = self.heap.new_object(Some(generator_proto))?;
                self.heap.record(prototype);
                let key = PropertyKey::String(self.vm.atoms.prototype);
                self.define(function, key, prototype.into(), true, false, false)?;
            }
            CodeKind::Script | CodeKind::Arrow | CodeKind::Method => {}
        }
        Ok(function)
    }

    /// Creates a native function object in `realm` (§10.3.4
    /// `CreateBuiltinFunction`). The result is recorded in the current
    /// handle scope.
    pub(crate) fn new_native_function(
        &mut self,
        realm: u32,
        name: &str,
        length: u32,
        func: NativeFn,
    ) -> VmResult<Gc<crate::object::Object>> {
        let proto = self.intrinsic(realm, Intrinsic::FunctionPrototype)?;
        let kind = ObjectKind::Native(Box::new(NativeFunction {
            func,
            realm,
            constructor: false,
        }));
        let function = self.heap.new_object_with_kind(Some(proto), kind)?;
        self.heap.record(function);
        let name = self.heap.intern_str(name)?;
        self.heap.record(name);
        self.define_function_properties(function, Value::Int(length as i32), name.into())?;
        Ok(function)
    }

    /// Defines `length` and `name` (non-writable, non-enumerable,
    /// configurable; §10.2.9 `SetFunctionName`, §10.2.10 `SetFunctionLength`).
    fn define_function_properties(
        &mut self,
        function: Gc<crate::object::Object>,
        length: Value,
        name: Value,
    ) -> VmResult<()> {
        let length_key = PropertyKey::String(self.heap.length_atom());
        self.define(function, length_key, length, false, false, true)?;
        let name_key = PropertyKey::String(self.vm.atoms.name);
        self.define(function, name_key, name, false, false, true)
    }

    /// Defines an own data property that the engine creates; a rejection
    /// is an internal error.
    pub(crate) fn define(
        &mut self,
        object: Gc<crate::object::Object>,
        key: PropertyKey,
        value: Value,
        writable: bool,
        enumerable: bool,
        configurable: bool,
    ) -> VmResult<()> {
        let desc = PropertyDescriptor::data(value, writable, enumerable, configurable);
        if self.heap.define_own_property(object, key, desc, &self.vm)? {
            Ok(())
        } else {
            Err(VmError::invariant("the engine could not define a property"))
        }
    }

    /// The name text of a function name value, for messages.
    pub(crate) fn name_text(&self, name: Gc<JsString>) -> String {
        self.heap
            .string(name)
            .map(|s| s.as_str16().to_string_lossy())
            .unwrap_or_default()
    }
}

/// Whether an object kind is callable (has `[[Call]]`).
pub(crate) fn is_callable_kind(kind: &ObjectKind) -> bool {
    matches!(kind, ObjectKind::Function(_) | ObjectKind::Native(_))
}

/// Whether an object kind is a constructor (has `[[Construct]]`).
pub(crate) fn is_constructor_kind(kind: &ObjectKind) -> bool {
    match kind {
        ObjectKind::Function(closure) => closure.compiled.kind == CodeKind::Normal,
        ObjectKind::Native(native) => native.constructor,
        _ => false,
    }
}
