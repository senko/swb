//! The JavaScript engine of swb (ADR 0025, ADR 0026): bytecode compiler,
//! interpreter, heap and garbage collector, values, strings, objects,
//! built-in objects, number conversion, `Date`, realms, the job queue and
//! the embedding API.
//!
//! Modules: `heap` (arenas with generational handles, the collector,
//! roots, safepoints), `value`, `string` (strings and atoms), `object`
//! (shapes, elements, the ordinary internal methods, arrays), `compiler`
//! and `bytecode`, `vm` (the interpreter, frames, closures, generators,
//! realms, conversions, the time limit), `builtins`, and `runtime` (the
//! embedding API, [`Runtime`]). The front end is in `swb-js-syntax`, the
//! code units and Unicode data in `swb-js-text`, and regular expressions
//! in `swb-js-regexp`.

mod builtins;
mod bytecode;
mod compiler;
mod error;
mod heap;
mod object;
mod runtime;
mod string;
mod value;
mod vm;

pub use error::{Error, InternalError, Result, Termination, ThrowKind};
pub use heap::{
    Gc, GcStats, Generic, GenericData, HandleScope, Heap, HeapConfig, HeapCounts, NoRoots,
    Persistent, Root, RootSource, Tracer,
};
pub use object::{
    ArrayIterator, GetResult, IterationKind, Object, ObjectKind, Property, PropertyDescriptor,
    SetResult, Shape,
};
pub use string::{JsString, PropertyKey};
pub use value::{BigInt, Equality, Symbol, Value, ValueCell};

pub use compiler::CompileStats;
pub use runtime::{ConsoleSink, MAX_STACK_LIMIT, Runtime, RuntimeConfig, ScriptError};
pub use swb_js_text::RecursionBudget;
pub use vm::{
    BoundFunction, Closure, DEFAULT_FRAME_LIMIT, DEFAULT_STACK_LIMIT, GeneratorState, NativeCall,
    NativeFn, NativeFunction, NativeReturn, Resume, ResumeMode, TerminationHandle, VmError,
    VmResult,
};
