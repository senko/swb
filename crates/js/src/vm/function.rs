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
    /// The internal slots of methods and classes; `None` for functions
    /// that need none (no `super` property, not a class constructor or
    /// field initializer).
    pub(crate) class: Option<Box<ClassSlots>>,
}

/// The internal slots of a method, a class constructor or a field
/// initializer function (§10.2, §15.7.14).
#[derive(Debug, Default)]
pub(crate) struct ClassSlots {
    /// `[[HomeObject]]`: the object whose prototype `super` properties
    /// read.
    pub(crate) home: Option<Gc<crate::object::Object>>,
    /// `[[Fields]]` of a class constructor: the function that defines the
    /// instance fields.
    pub(crate) fields: Option<Gc<crate::object::Object>>,
    /// The computed field keys of a field initializer function: an
    /// internal array, evaluated when the class was defined.
    pub(crate) field_keys: Option<Gc<crate::object::Object>>,
    /// The private environment of the class (`vm/private.rs`): of a field
    /// initializer function, for its private fields; of a class
    /// constructor whose class has instance private methods, for their
    /// brand.
    pub(crate) private: Option<Gc<crate::object::Object>>,
}

impl Closure {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.generic(self.code);
        for cell in &self.captures {
            tracer.cell(*cell);
        }
        if let Some(slots) = &self.class {
            for object in [slots.home, slots.fields, slots.field_keys, slots.private]
                .into_iter()
                .flatten()
            {
                tracer.object(object);
            }
        }
    }

    pub(crate) fn heap_size(&self) -> usize {
        size_of::<Closure>()
            + self.captures.len() * size_of::<Gc<ValueCell>>()
            + self.class.as_ref().map_or(0, |_| size_of::<ClassSlots>())
    }
}

/// The data of a bound function exotic object (§10.4.1): its target (a
/// function, possibly bound itself), the bound `this` and the bound
/// arguments. A call walks the chain of bound functions in a loop, so a
/// chain of any depth needs no Rust recursion.
pub struct BoundFunction {
    /// `[[BoundTargetFunction]]`.
    pub(crate) target: Gc<crate::object::Object>,
    /// `[[BoundThis]]`.
    pub(crate) this: Value,
    /// `[[BoundArguments]]`.
    pub(crate) args: Box<[Value]>,
    /// Whether the target is a constructor.
    pub(crate) constructor: bool,
}

impl BoundFunction {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.object(self.target);
        tracer.value(self.this);
        for value in &self.args {
            tracer.value(*value);
        }
    }

    pub(crate) fn heap_size(&self) -> usize {
        size_of::<BoundFunction>() + self.args.len() * size_of::<Value>()
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
    /// The name that the function was created with (an atom; none for
    /// the nameless intrinsics). `Function.prototype.toString` shows it
    /// even after a script redefines the `name` property.
    pub(crate) name: Option<Gc<JsString>>,
    /// The internal slots of a built-in function that the specification
    /// creates with state (§10.3.4 `CreateBuiltinFunction` with
    /// `additionalInternalSlotsList`: the resolving functions of a promise,
    /// the element functions of `Promise.all`, the closures of
    /// `Promise.prototype.finally`). Empty for the others; the collector
    /// traces them. A slot may hold a cell (`Value::Cell`) for a record that
    /// two functions share; the native code that owns the slots reads it.
    pub(crate) slots: Box<[Value]>,
}

impl NativeFunction {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        if let Some(name) = self.name {
            tracer.string(name);
        }
        for value in &self.slots {
            tracer.value(*value);
        }
    }

    pub(crate) fn heap_size(&self) -> usize {
        size_of::<NativeFunction>() + self.slots.len() * size_of::<Value>()
    }
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
#[derive(Debug)]
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
    /// A deferred `[[Construct]]` (`Reflect.construct`): like `Call`, with
    /// `new.target`.
    Construct {
        /// The constructor.
        callee: Value,
        /// `new.target`.
        new_target: Value,
        /// The arguments.
        args: Vec<Value>,
    },
    /// Resume a generator (the `next`, `return` and `throw` methods of
    /// generator objects).
    Resume(Resume),
    /// A native loop (`vm/native_loop.rs`): the interpreter runs its steps
    /// and their calls; the loop's result is the result of the native
    /// call.
    Loop(LoopState),
}

/// The state of a native loop that a native function returns
/// (`Runtime::native_loop`): the state and the index of its registers in
/// the value stack.
pub struct LoopState {
    pub(crate) state: Box<dyn crate::vm::native_loop::NativeLoop>,
    pub(crate) regs: usize,
}

impl std::fmt::Debug for LoopState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LoopState")
    }
}

/// How a generator resumes (§27.5.3.3 `GeneratorResume`, §27.5.3.4
/// `GeneratorResumeAbrupt`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResumeMode {
    /// `next(v)`: `v` is the value of the `yield` expression.
    Next,
    /// `throw(e)`: the `yield` throws `e`.
    Throw,
    /// `return(v)`: the generator returns `v` (after its `finally`
    /// blocks).
    Return,
}

/// A request to resume a suspended generator with a value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Resume {
    pub(crate) generator: Gc<crate::object::Object>,
    pub(crate) value: Value,
    pub(crate) mode: ResumeMode,
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
        self.new_closure_with(code, compiled, captures, realm, ClosureParts::default())
    }

    /// [`Runtime::new_closure`] with a home object and, for a class
    /// constructor, the prototype of the function object (the class's
    /// heritage). A class constructor gets no `prototype` here; the class
    /// definition makes it.
    pub(crate) fn new_closure_with(
        &mut self,
        code: Gc<Generic>,
        compiled: &Rc<FunctionCode>,
        captures: Box<[Gc<ValueCell>]>,
        realm: u32,
        parts: ClosureParts,
    ) -> VmResult<Gc<crate::object::Object>> {
        let proto = if let Some(proto) = parts.function_proto {
            proto
        } else {
            let proto_kind = match compiled.kind {
                CodeKind::Generator => Intrinsic::GeneratorFunctionPrototype,
                CodeKind::Async => Intrinsic::AsyncFunctionPrototype,
                CodeKind::AsyncGenerator => Intrinsic::AsyncGeneratorFunctionPrototype,
                _ => Intrinsic::FunctionPrototype,
            };
            self.intrinsic(realm, proto_kind)?
        };
        let needs_slots = parts.home.is_some()
            || compiled.kind.is_class_constructor()
            || compiled.kind == CodeKind::Initializer;
        let class = needs_slots.then(|| {
            Box::new(ClassSlots {
                home: parts.home,
                ..ClassSlots::default()
            })
        });
        let kind = ObjectKind::Function(Box::new(Closure {
            code,
            compiled: Rc::clone(compiled),
            captures,
            this_mode: compiled.this_mode,
            realm,
            class,
        }));
        let function = self.heap.new_object_with_kind(Some(proto), kind)?;
        self.heap.record(function);
        let length = Value::Int(compiled.length as i32);
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
            CodeKind::Generator | CodeKind::AsyncGenerator => {
                let which = if compiled.kind == CodeKind::Generator {
                    Intrinsic::GeneratorPrototype
                } else {
                    Intrinsic::AsyncGeneratorPrototype
                };
                let generator_proto = self.intrinsic(realm, which)?;
                let prototype = self.heap.new_object(Some(generator_proto))?;
                self.heap.record(prototype);
                let key = PropertyKey::String(self.vm.atoms.prototype);
                self.define(function, key, prototype.into(), true, false, false)?;
            }
            CodeKind::Script
            | CodeKind::Eval
            | CodeKind::Arrow
            | CodeKind::Method
            | CodeKind::Async
            | CodeKind::ClassConstructor
            | CodeKind::DerivedConstructor
            | CodeKind::Initializer => {}
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
        self.new_builtin(realm, name, length, func, false)
    }

    /// [`Runtime::new_native_function`] that can be a constructor (it
    /// then sees `new.target`).
    pub(crate) fn new_builtin(
        &mut self,
        realm: u32,
        name: &str,
        length: u32,
        func: NativeFn,
        constructor: bool,
    ) -> VmResult<Gc<crate::object::Object>> {
        let proto = self.intrinsic(realm, Intrinsic::FunctionPrototype)?;
        self.new_builtin_with_proto(realm, proto, name, length, func, constructor)
    }

    /// [`Runtime::new_builtin`] with another prototype than
    /// `%Function.prototype%` (the typed array constructors inherit from
    /// `%TypedArray%`, §23.2.6).
    pub(crate) fn new_builtin_with_proto(
        &mut self,
        realm: u32,
        proto: Gc<crate::object::Object>,
        name: &str,
        length: u32,
        func: NativeFn,
        constructor: bool,
    ) -> VmResult<Gc<crate::object::Object>> {
        let name = self.heap.intern_str(name)?;
        self.heap.record(name);
        let kind = ObjectKind::Native(Box::new(NativeFunction {
            func,
            realm,
            constructor,
            name: Some(name),
            slots: Box::default(),
        }));
        let function = self.heap.new_object_with_kind(Some(proto), kind)?;
        self.heap.record(function);
        self.define_function_properties(function, Value::Int(length as i32), name.into())?;
        Ok(function)
    }

    /// `CreateBuiltinFunction(steps, length, "", slots)` (§10.3.4) in
    /// `realm`: an anonymous native function that is not a constructor,
    /// with internal slots. The values of `slots` must be rooted by the
    /// caller; the function is recorded in the current handle scope.
    pub(crate) fn new_builtin_with_slots(
        &mut self,
        realm: u32,
        length: u32,
        func: NativeFn,
        slots: Box<[Value]>,
    ) -> VmResult<Gc<crate::object::Object>> {
        let proto = self.intrinsic(realm, Intrinsic::FunctionPrototype)?;
        let name = self.vm.atoms.empty;
        let kind = ObjectKind::Native(Box::new(NativeFunction {
            func,
            realm,
            constructor: false,
            name: Some(name),
            slots,
        }));
        let function = self.heap.new_object_with_kind(Some(proto), kind)?;
        self.heap.record(function);
        // The object is new and has no property: add `length` and `name`
        // without the checks of `[[DefineOwnProperty]]` (these functions are
        // made per promise and per element of the combinators).
        self.heap.presize(function, 2, 0)?;
        let fixed = |value| crate::object::Property::Data {
            value,
            writable: false,
            enumerable: false,
            configurable: true,
        };
        let length_key = PropertyKey::String(self.heap.length_atom());
        self.heap.add_named(
            function,
            length_key,
            fixed(Value::Int(length as i32)),
            &self.vm,
        )?;
        let name_key = PropertyKey::String(self.vm.atoms.name);
        self.heap
            .add_named(function, name_key, fixed(name.into()), &self.vm)?;
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
        self.heap.string_lossy(name, usize::MAX)
    }
}

/// Whether an object kind is callable (has `[[Call]]`).
pub(crate) fn is_callable_kind(kind: &ObjectKind) -> bool {
    match kind {
        ObjectKind::Function(_) | ObjectKind::Native(_) | ObjectKind::Bound(_) => true,
        ObjectKind::Proxy(data) => data.callable,
        _ => false,
    }
}

/// The optional parts of a new closure ([`Runtime::new_closure_with`]).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ClosureParts {
    /// The prototype of the function object (default: the realm's
    /// `%Function.prototype%`, `%GeneratorFunction.prototype%` or
    /// `%AsyncFunction.prototype%`).
    pub(crate) function_proto: Option<Gc<crate::object::Object>>,
    /// `[[HomeObject]]`.
    pub(crate) home: Option<Gc<crate::object::Object>>,
}

/// Whether an object kind is a constructor (has `[[Construct]]`).
pub(crate) fn is_constructor_kind(kind: &ObjectKind) -> bool {
    match kind {
        ObjectKind::Function(closure) => matches!(
            closure.compiled.kind,
            CodeKind::Normal | CodeKind::ClassConstructor | CodeKind::DerivedConstructor
        ),
        ObjectKind::Native(native) => native.constructor,
        ObjectKind::Bound(bound) => bound.constructor,
        ObjectKind::Proxy(data) => data.constructor,
        _ => false,
    }
}
