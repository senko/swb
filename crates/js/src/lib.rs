//! The JavaScript engine of swb (ADR 0025, ADR 0026): bytecode compiler,
//! interpreter, heap and garbage collector, values, strings, objects,
//! built-in objects, number conversion, `Date`, realms, the job queue and
//! the embedding API.
//!
//! Modules: `heap` (arenas with generational handles, the collector,
//! roots, safepoints), `value`, `string` (strings and atoms), `object`
//! (shapes, elements, the ordinary internal methods, arrays), `compiler`
//! and `bytecode`, `vm` (the interpreter, frames, closures, generators,
//! realms, conversions, the time limit), `builtins`, `runtime` (the
//! embedding API, [`Runtime`]) and `host` (host classes, hooks, host data,
//! event handlers: the parts of the embedding API for the Web API
//! bindings). The front end is in `swb-js-syntax`, the
//! code units and Unicode data in `swb-js-text`, and regular expressions
//! in `swb-js-regexp`.

mod builtins;
mod bytecode;
mod compiler;
mod date;
mod error;
mod heap;
mod host;
mod object;
mod runtime;
mod source_text;
mod string;
mod value;
mod vm;

pub use error::{Error, InternalError, Result, Termination, ThrowKind};
pub use heap::{
    Gc, Generic, GenericData, HandleScope, Heap, HeapConfig, NoRoots, Persistent, Root, RootSource,
    Tracer,
};
pub use host::{
    Attributes, ClassId, ClassSpec, DefineOwnPropertyHook, DeleteHook, EventHandler, GetHook,
    GetOwnPropertyHook, GlobalHook, HostHooks, HostState, Interface, InterfaceHook, LazyHooks,
    OwnKeysHook, PreventExtensionsHook, Refusal, RefusalHook, ScriptIterator, SetAnswer, SetHook,
    Slots,
};
pub use object::{GetResult, HostData, Object, Property, PropertyDescriptor, SetResult};
pub use string::{JsString, PropertyKey};
pub use value::{BigInt, Symbol, Value, ValueCell};

pub use date::{Clock, TimeZone, TimeZoneError};
pub use runtime::{
    ConsoleLevel, ConsoleSink, MAX_STACK_LIMIT, Runtime, RuntimeConfig, ScriptError, SourceLocation,
};
pub use swb_js_text::RecursionBudget;
pub use vm::{
    DEFAULT_FRAME_LIMIT, DEFAULT_STACK_LIMIT, NativeCall, NativeFn, NativeReturn, PromiseStatus,
    ReactionKind, Resume, TerminationHandle, VmError, VmResult,
};
