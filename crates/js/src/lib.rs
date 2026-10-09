//! The JavaScript engine of swb (ADR 0025, ADR 0026): bytecode compiler,
//! interpreter, heap and garbage collector, values, strings, objects,
//! built-in objects, number conversion, `Date`, realms, the job queue and
//! the embedding API.
//!
//! So far this crate has the heap ([`Heap`]: arenas with generational
//! handles, the mark-and-sweep collector, roots, handle scopes,
//! safepoints and accounting), the values ([`Value`]), strings and atoms
//! ([`JsString`], [`PropertyKey`]) and the object model ([`Object`]:
//! shapes, dictionary mode, elements, the ordinary internal methods and
//! the array exotic object). The front end is in `swb-js-syntax`, the
//! code units and Unicode data in `swb-js-text`, and regular expressions
//! in `swb-js-regexp`.

pub mod error;
pub mod heap;
pub mod object;
pub mod string;
pub mod value;

pub use error::{Error, InternalError, Result, Termination, ThrowKind};
pub use heap::{
    Gc, GcStats, Generic, GenericData, HandleScope, Heap, HeapConfig, HeapCounts, NoRoots,
    Persistent, Root, RootSource, Tracer,
};
pub use object::{GetResult, Object, ObjectKind, Property, PropertyDescriptor, SetResult, Shape};
pub use string::{JsString, PropertyKey};
pub use value::{BigInt, Equality, Symbol, Value, ValueCell};
