//! The embedding API (ADR 0026 section 12): a runtime with its realms,
//! script evaluation, native functions, and the reports of errors.
//!
//! A [`Runtime`] owns the heap, the VM state (value stack, frames,
//! realms) and the recursion budget. It is not `Send`. Every thread that
//! runs it needs a stack of at least 8 MiB (ADR 0026 section 9).
//!
//! Values that the API returns are handles: a returned result stays alive
//! until the next call that runs script code; a native function's values
//! stay alive until it returns (they are recorded in its handle scope).

use std::any::Any;
use std::borrow::Cow;
use std::fmt;
use std::rc::Rc;

use swb_js_syntax::{ErrorKind, ParseError, RecursionBudget};
use swb_js_text::{Str16, String16};

use crate::bytecode::FunctionCode;
use crate::compiler::{CompileError, CompileStats, compile_script};
use crate::error::{Error, InternalError, Termination, ThrowKind};
use crate::heap::{Gc, GcStats, Generic, Heap, HeapConfig};
use crate::object::{GetResult, PropertyDescriptor};
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::{
    Atoms, DEFAULT_FRAME_LIMIT, DEFAULT_STACK_LIMIT, Frame, NativeFn, ReturnTo, STACK_OVERFLOW, Vm,
    VmError, VmResult,
};

/// The largest value stack: frame records keep stack positions in 32 bits.
pub const MAX_STACK_LIMIT: usize = 1 << 30;

/// The limits and options of a runtime.
#[derive(Clone, Copy, Debug)]
pub struct RuntimeConfig {
    /// The heap limits and the collection policy (and the stress mode).
    pub heap: HeapConfig,
    /// The most frame records (script call depth).
    pub frame_limit: usize,
    /// The most values in the value stack (clamped to `MAX_STACK_LIMIT`,
    /// so that register positions fit the 32-bit fields of a frame).
    pub stack_limit: usize,
    /// The shared recursion budget (ADR 0026 section 9).
    pub recursion_budget: RecursionBudget,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        RuntimeConfig {
            heap: HeapConfig::default(),
            frame_limit: DEFAULT_FRAME_LIMIT,
            stack_limit: DEFAULT_STACK_LIMIT,
            recursion_budget: RecursionBudget::DEFAULT,
        }
    }
}

/// Why a script did not complete normally.
#[derive(Clone, Debug, PartialEq)]
pub enum ScriptError {
    /// The source did not compile (an early error, a resource limit of
    /// the front end or the compiler, or a construct outside the subset).
    Compile {
        /// `SyntaxError` or `RangeError`.
        kind: ThrowKind,
        /// The message.
        message: String,
        /// The code-unit offset of the error.
        offset: u32,
    },
    /// An exception that no handler caught. The thrown value stays alive
    /// until the next run ([`Runtime::last_value`]).
    Uncaught {
        /// The `name` of an error object (empty for other values).
        name: String,
        /// The `message` of an error object, or the text of another value.
        message: String,
        /// The source offset (code units) of the instruction that threw,
        /// in the innermost script frame.
        offset: Option<u32>,
    },
    /// The script was ended (heap limit; later the time limit).
    Terminated(Termination),
    /// An engine bug ended the script (logged).
    Internal(InternalError),
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScriptError::Compile { kind, message, .. } => write!(f, "{}: {message}", kind.name()),
            ScriptError::Uncaught { name, message, .. } if name.is_empty() => {
                write!(f, "Uncaught {message}")
            }
            ScriptError::Uncaught { name, message, .. } => {
                write!(f, "Uncaught {name}: {message}")
            }
            ScriptError::Terminated(termination) => write!(f, "terminated: {termination}"),
            ScriptError::Internal(error) => write!(f, "internal error: {error}"),
        }
    }
}

impl std::error::Error for ScriptError {}

impl From<ParseError> for ScriptError {
    fn from(error: ParseError) -> Self {
        let mut message = error.message.into_owned();
        if let Some(construct) = error.unsupported {
            message.push_str(" (");
            message.push_str(construct);
            message.push(')');
        }
        ScriptError::Compile {
            kind: match error.kind {
                ErrorKind::Syntax => ThrowKind::SyntaxError,
                ErrorKind::Range => ThrowKind::RangeError,
            },
            message,
            offset: error.offset,
        }
    }
}

/// A JavaScript runtime: one heap, one value stack, realms.
pub struct Runtime {
    pub(crate) heap: Heap,
    pub(crate) vm: Vm,
    pub(crate) budget: RecursionBudget,
    host: Option<Box<dyn Any>>,
}

impl Runtime {
    /// A runtime with one realm (index 0).
    pub fn new(config: RuntimeConfig) -> Result<Runtime, ScriptError> {
        let mut heap = Heap::new(config.heap);
        let atoms = make_atoms(&mut heap).map_err(|e| finish_error(VmError::from(e)))?;
        let vm = Vm {
            stack: Vec::new(),
            frames: Vec::new(),
            realms: Vec::new(),
            atoms,
            frame_limit: config.frame_limit,
            stack_limit: config.stack_limit.min(MAX_STACK_LIMIT),
            last_value: Value::Undefined,
            error_offset: None,
        };
        let mut runtime = Runtime {
            heap,
            vm,
            budget: config.recursion_budget,
            host: None,
        };
        runtime.create_realm().map_err(finish_error)?;
        Ok(runtime)
    }

    /// The heap (statistics, the stress mode, direct access in tests).
    pub fn heap(&self) -> &Heap {
        &self.heap
    }

    /// The heap, for writing.
    pub fn heap_mut(&mut self) -> &mut Heap {
        &mut self.heap
    }

    /// Runs a full collection with the VM's roots.
    pub fn collect(&mut self) -> GcStats {
        self.heap.collect(&self.vm)
    }

    /// The last result or uncaught exception value of a run.
    pub fn last_value(&self) -> Value {
        self.vm.last_value
    }

    /// The remaining recursion budget (in approximate stack bytes).
    pub fn recursion_budget(&self) -> RecursionBudget {
        self.budget
    }

    /// Stores host data that native functions can reach with
    /// [`Runtime::host_data_mut`].
    pub fn set_host_data<T: Any>(&mut self, data: T) {
        self.host = Some(Box::new(data));
    }

    /// The host data, if it has the type `T`.
    pub fn host_data_mut<T: Any>(&mut self) -> Option<&mut T> {
        self.host.as_mut()?.downcast_mut()
    }

    /// Compiles and runs a classic script in realm 0. Returns its
    /// completion value (the value of the last expression statement that
    /// ran; the full completion rules of §16.1.6 come later).
    pub fn eval(&mut self, source: &str) -> Result<Value, ScriptError> {
        let text = String16::from(source);
        self.eval_source(text.as_str16())
    }

    /// [`Runtime::eval`] with source text in code units.
    pub fn eval_source(&mut self, source: Str16<'_>) -> Result<Value, ScriptError> {
        self.vm.error_offset = None;
        let (code, compiled) = self.compile(source)?;
        let result = self.run_script(code, compiled, 0);
        self.finish(result)
    }

    /// Parses and compiles a script; the code object is not rooted, so the
    /// caller runs it before anything can collect.
    fn compile(
        &mut self,
        source: Str16<'_>,
    ) -> Result<(Gc<Generic>, Rc<FunctionCode>), ScriptError> {
        let script = swb_js_syntax::parse_script(source, &mut self.budget)?;
        // The compile data counts against the heap limit while it exists
        // (ADR 0026 section 3).
        let size = script.ast.heap_size() + script.scopes.heap_size();
        self.heap
            .reserve(size, &self.vm)
            .map_err(|e| finish_error(e.into()))?;
        compile_script(&mut self.heap, &script, source, &mut self.budget).map_err(compile_error)
    }

    /// Compiles a script and returns the statistics of its code (for
    /// measurements); nothing runs.
    pub fn compile_stats(&mut self, source: &str) -> Result<CompileStats, ScriptError> {
        let text = String16::from(source);
        let script = swb_js_syntax::parse_script(text.as_str16(), &mut self.budget)?;
        let (_, compiled) =
            compile_script(&mut self.heap, &script, text.as_str16(), &mut self.budget)
                .map_err(compile_error)?;
        Ok(CompileStats::of(&compiled))
    }

    /// Compiles a script and returns a listing of the bytecode of all its
    /// functions (for tests and debugging); nothing runs.
    pub fn disassemble(&mut self, source: &str) -> Result<String, ScriptError> {
        let text = String16::from(source);
        let script = swb_js_syntax::parse_script(text.as_str16(), &mut self.budget)?;
        let (_, compiled) =
            compile_script(&mut self.heap, &script, text.as_str16(), &mut self.budget)
                .map_err(compile_error)?;
        Ok(crate::bytecode::disassemble(&compiled, &self.heap))
    }

    /// Runs the code of a script in a realm: a frame whose result goes to
    /// the host.
    fn run_script(
        &mut self,
        code: Gc<Generic>,
        compiled: Rc<FunctionCode>,
        realm: u32,
    ) -> VmResult<Value> {
        let global = self.global_object(realm)?;
        let restore_top = self.vm.stack.len();
        let registers = compiled.register_count as usize;
        if self.vm.frames.len() >= self.vm.frame_limit
            || restore_top + 2 + registers > self.vm.stack_limit
        {
            return Err(VmError::range_error(STACK_OVERFLOW));
        }
        let window = self.push_window(Value::Undefined, global.into(), &[])?;
        self.vm.stack.resize(window + registers, Value::Undefined);
        self.vm.frames.push(Frame {
            function: None,
            code,
            compiled,
            pc: 0,
            base: window as u32,
            restore_top: restore_top as u32,
            this: global.into(),
            new_target: Value::Undefined,
            argc: 0,
            extra_args: Vec::new(),
            ret: ReturnTo::Host,
            construct: false,
            generator: None,
            realm,
        });
        self.run_frames()
    }

    /// Calls a function from the host (a re-entry, see [`Runtime::call`]).
    pub fn call_function(
        &mut self,
        callee: Value,
        this: Value,
        args: &[Value],
    ) -> Result<Value, ScriptError> {
        self.vm.error_offset = None;
        let scope = self.heap.open_scope();
        let result = self.call(callee, this, args);
        if let Err(error) = self.heap.close_scope(scope) {
            return Err(finish_error(error.into()));
        }
        self.finish(result)
    }

    /// Turns the result of a run into the API's result and keeps the value
    /// alive until the next run.
    fn finish(&mut self, result: VmResult<Value>) -> Result<Value, ScriptError> {
        match result {
            Ok(value) => {
                self.vm.last_value = value;
                Ok(value)
            }
            Err(VmError::Throw(value)) => {
                self.vm.last_value = value;
                let (name, message) = self.describe_exception(value);
                Err(ScriptError::Uncaught {
                    name,
                    message,
                    offset: self.vm.error_offset,
                })
            }
            Err(VmError::Raise { kind, message }) => {
                // The host can inspect the error object as the last value.
                self.vm.last_value = match self.error_object(kind, &message) {
                    Ok(object) => object.into(),
                    Err(error) => return Err(finish_error(error)),
                };
                Err(ScriptError::Uncaught {
                    name: kind.name().to_owned(),
                    message: message.into_owned(),
                    offset: self.vm.error_offset,
                })
            }
            Err(error) => {
                self.vm.last_value = Value::Undefined;
                Err(finish_error(error))
            }
        }
    }

    /// The `name` and `message` of a thrown value, read without running
    /// script code (getters are not called).
    fn describe_exception(&self, value: Value) -> (String, String) {
        let Value::Object(object) = value else {
            return (String::new(), self.primitive_text(value));
        };
        let read = |key| match self.heap.get(object, PropertyKey::String(key), value) {
            Ok(GetResult::Value(Value::String(text))) => Some(self.name_text(text)),
            _ => None,
        };
        match read(self.vm.atoms.message) {
            Some(message) => {
                let name = read(self.vm.atoms.name).unwrap_or_else(|| "Error".to_owned());
                (name, message)
            }
            None => (String::new(), self.describe_value(value)),
        }
    }

    /// Installs a native function as a property of the global object of
    /// realm 0.
    pub fn define_global_function(
        &mut self,
        name: &str,
        length: u32,
        func: NativeFn,
    ) -> Result<(), ScriptError> {
        let scope = self.heap.open_scope();
        let result = (|| {
            let function = self.new_native_function(0, name, length, func)?;
            let global = self.global_object(0)?;
            let key = self.heap.key_from_str(name)?;
            self.define(global, key, function.into(), true, false, true)
        })();
        self.heap
            .close_scope(scope)
            .map_err(|e| finish_error(e.into()))?;
        result.map_err(finish_error)
    }

    /// `ToString` of a value as Rust text (lossy for unpaired surrogates).
    /// Can run script code (`toString`, `valueOf`).
    pub fn to_rust_string(&mut self, value: Value) -> VmResult<String> {
        let scope = self.heap.open_scope();
        let result = self.to_string(value);
        let text = match result {
            Ok(string) => Ok(self.name_text(string)),
            Err(error) => Err(error),
        };
        self.heap.close_scope(scope)?;
        text
    }

    /// The text of a value without running script code (for tests and
    /// diagnostics): primitives as `ToString` gives them, objects as
    /// `#<Object>`.
    pub fn display(&self, value: Value) -> String {
        self.primitive_text(value)
    }

    /// Defines an accessor property on an object (a helper for natives
    /// and tests until `Object.defineProperty` exists).
    pub fn define_accessor(
        &mut self,
        object: Value,
        name: &str,
        getter: Value,
        setter: Value,
    ) -> VmResult<bool> {
        let Value::Object(object) = object else {
            return Err(VmError::type_error("not an object"));
        };
        let check = |rt: &Runtime, f: Value| -> VmResult<()> {
            if f.is_undefined() || rt.is_callable(f)? {
                Ok(())
            } else {
                Err(VmError::type_error("Getter must be a function"))
            }
        };
        check(self, getter)?;
        check(self, setter)?;
        let key = self.heap.key_from_str(name)?;
        let desc = PropertyDescriptor::accessor(getter, setter, true, true);
        Ok(self.heap.define_own_property(object, key, desc, &self.vm)?)
    }
}

/// Creates the atoms of the VM.
fn make_atoms(heap: &mut Heap) -> Result<Atoms, Error> {
    let mut atom = |text: &str| heap.intern_str(text);
    Ok(Atoms {
        prototype: atom("prototype")?,
        constructor: atom("constructor")?,
        name: atom("name")?,
        message: atom("message")?,
        value: atom("value")?,
        done: atom("done")?,
        callee: atom("callee")?,
        value_of: atom("valueOf")?,
        to_string: atom("toString")?,
        empty: atom("")?,
        undefined: atom("undefined")?,
        null: atom("null")?,
        object: atom("object")?,
        boolean: atom("boolean")?,
        number: atom("number")?,
        string: atom("string")?,
        symbol: atom("symbol")?,
        bigint: atom("bigint")?,
        function: atom("function")?,
        true_: atom("true")?,
        false_: atom("false")?,
    })
}

/// The API error of an abrupt completion other than a thrown value.
fn finish_error(error: VmError) -> ScriptError {
    match error {
        VmError::Throw(_) => ScriptError::Uncaught {
            name: String::new(),
            message: "(exception)".to_owned(),
            offset: None,
        },
        VmError::Raise { kind, message } => ScriptError::Uncaught {
            name: kind.name().to_owned(),
            message: message.into_owned(),
            offset: None,
        },
        VmError::Terminated(termination) => ScriptError::Terminated(termination),
        VmError::Internal(internal) => {
            log::warn!("JavaScript engine internal error: {internal}");
            ScriptError::Internal(internal)
        }
    }
}

/// The API error of a failed compile.
fn compile_error(error: CompileError) -> ScriptError {
    match error {
        CompileError::Script {
            kind,
            message,
            offset,
        } => ScriptError::Compile {
            kind,
            message: match message {
                Cow::Borrowed(text) => text.to_owned(),
                Cow::Owned(text) => text,
            },
            offset,
        },
        CompileError::Heap(error) => finish_error(error.into()),
    }
}
