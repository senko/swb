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
//! - The event loop (feature 5b): `event_loop` (the task queue, the
//!   microtask checkpoint, the notification of rejected promises),
//!   `timers` (`setTimeout`, `setInterval`, `queueMicrotask`,
//!   `reportError`), `errors` (`error` and `unhandledrejection` events at
//!   the window), the page clock with real and virtual time
//!   ([`PageClock`]), and [`ScriptRuntime::run_due_tasks`], which the
//!   engine calls from its loop.
//! - Scripts in the document (feature 5c): `scripts` (script elements:
//!   prepare, execute, `HTMLScriptElement`) and `document_load` (the
//!   parser with its pauses, "the end" of the parse, `DOMContentLoaded`,
//!   `load`, `pageshow`, the unload). The engine lends the parser and
//!   fetches the scripts ([`EngineHost::parse_step`],
//!   [`EngineHost::fetch_script`], [`ScriptRuntime::script_fetched`]).

pub mod bindings;
mod clock;
mod document_load;
mod dom;
mod errors;
mod event_loop;
mod events;
#[rustfmt::skip]
pub mod generated;
mod host;
mod html;
mod runtime;
mod scripts;
mod state;
mod svg;
mod timers;
mod window;

pub use clock::{ClockMode, PageClock};
pub use events::{DispatchTarget, EventInterface, TrustedEvent, UserEvent, UserKind, modifiers};
pub use host::{ControlWrite, EngineHost};
pub use runtime::{
    ConsoleMessage, EVALUATION_TIMEOUT, Evaluation, EvaluationId, EvaluationStart, HEAP_LIMIT,
    JsonForm, SCRIPT_BUDGET, ScriptRuntime, TASK_TIME_LIMIT,
};
pub use scripts::{FetchedScript, is_javascript_mime_type};
pub use swb_js::{ConsoleLevel, ScriptError};
