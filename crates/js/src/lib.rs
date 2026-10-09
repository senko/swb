//! The JavaScript engine of swb (ADR 0025, ADR 0026): bytecode compiler,
//! interpreter, heap and garbage collector, values, strings, objects,
//! built-in objects, number conversion, `Date`, realms, the job queue and
//! the embedding API.
//!
//! So far this crate has the heap ([`Heap`]: arenas with generational
//! handles, the mark-and-sweep collector, roots, handle scopes,
//! safepoints and accounting), the values ([`Value`]), strings and atoms
//! ([`JsString`], [`PropertyKey`]), the object model ([`Object`]:
//! shapes, dictionary mode, elements, the ordinary internal methods and
//! the array exotic object), the bytecode compiler and its verifier, the
//! interpreter with frames, closures, native functions and generators,
//! the realms with their minimal intrinsics, and the embedding API
//! ([`Runtime`]). The front end is in `swb-js-syntax`, the code units and
//! Unicode data in `swb-js-text`, and regular expressions in
//! `swb-js-regexp`.

mod bytecode;
mod compiler;
pub mod error;
pub mod heap;
pub mod object;
mod runtime;
pub mod string;
pub mod value;
mod vm;

pub use error::{Error, InternalError, Result, Termination, ThrowKind};
pub use heap::{
    Gc, GcStats, Generic, GenericData, HandleScope, Heap, HeapConfig, HeapCounts, NoRoots,
    Persistent, Root, RootSource, Tracer,
};
pub use object::{GetResult, Object, ObjectKind, Property, PropertyDescriptor, SetResult, Shape};
pub use string::{JsString, PropertyKey};
pub use value::{BigInt, Equality, Symbol, Value, ValueCell};

pub use compiler::CompileStats;
pub use runtime::{MAX_STACK_LIMIT, Runtime, RuntimeConfig, ScriptError};
pub use swb_js_syntax::RecursionBudget;
pub use vm::{
    Closure, DEFAULT_FRAME_LIMIT, DEFAULT_STACK_LIMIT, GeneratorState, NativeCall, NativeFn,
    NativeFunction, NativeReturn, Resume, VmError, VmResult,
};
