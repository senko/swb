//! The Web API bindings of swb (ADR 0028 sections 1, 2, 4 and 5).
//!
//! - `bindings`: the binding context ([`bindings::Cx`]), the host trait
//!   that the glue needs ([`bindings::Host`]: node wrappers, content
//!   attributes for `[Reflect]`, the glue state), the conversions of Web
//!   IDL §3.2 in both directions, exceptions with Chromium's messages and
//!   `DOMException`, overload resolution, `[Reflect]`, legacy platform
//!   objects and iterables (features 4b and 4c1).
//! - `generated`: the glue that `swb-webidl` writes from the IDL files in
//!   `crates/web/idl/` (`just bindings`); checked in. Each interface has a
//!   trait with one function per member, which the hand-written behaviour
//!   implements; the glue does the brand checks, the argument conversions,
//!   the calls and the result conversions.
//! - The runtime of a document (feature 4c2): [`ScriptRuntime`] creates a
//!   `js::Runtime` with the bindings and the `Window` global, runs classic
//!   scripts as tasks, evaluates expressions and keeps the console. The
//!   engine lends its document state through [`EngineHost`] for each task.
//!   `state` holds the binding state (the wrapper vector, the side table,
//!   the listeners of nodes and the window); `dom`, `window` and `events`
//!   the behaviour of the interfaces.
//! - Events (feature 5a): `EventTarget`, `Event` and its subclasses, the
//!   dispatch algorithm, and [`ScriptRuntime::dispatch_event`], the entry
//!   for events that the engine fires ([`DispatchTarget`],
//!   [`TrustedEvent`]).

pub mod bindings;
mod dom;
mod events;
#[rustfmt::skip]
pub mod generated;
mod host;
mod runtime;
mod scripts;
mod state;
mod window;

pub use events::{DispatchTarget, EventInterface, TrustedEvent};
pub use host::EngineHost;
pub use runtime::{
    ConsoleMessage, Evaluation, HEAP_LIMIT, JsonForm, ScriptRuntime, TASK_TIME_LIMIT,
};
pub use swb_js::{ConsoleLevel, ScriptError};
