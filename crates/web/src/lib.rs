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
//! - The rendering update (feature 7b2): `animation_frames`
//!   (`requestAnimationFrame`) and `media_queries` (`matchMedia` and the
//!   change events); the engine runs the steps of a frame through
//!   [`ScriptRuntime::evaluate_media_queries`] and
//!   [`ScriptRuntime::run_animation_frame_callbacks`].
//! - Browser services (feature 8a2): `navigator`, `performance` (the entry
//!   buffer, marks, measures, the navigation entry) with
//!   `performance_observer`, `crypto`, `base64` (`atob`, `btoa`),
//!   `text_codec` (`TextEncoder`, `TextDecoder`) and `idle_callbacks`
//!   (`requestIdleCallback`; the engine runs the idle periods through
//!   [`ScriptRuntime::run_idle_callbacks`]).
//! - Cookies and Web Storage (feature 8b): `cookie` (`document.cookie`
//!   over the page's jar, through [`EngineHost::cookie_string`] and
//!   [`EngineHost::set_cookie`]) and `storage` (`localStorage` and
//!   `sessionStorage` over a [`StorageStore`] that the page owns and lends
//!   through [`EngineHost::storage`]).

mod animation_frames;
mod base64;
pub mod bindings;
mod clock;
mod cookie;
mod crypto;
mod cssom;
mod cssom_view;
mod document_load;
mod dom;
mod errors;
mod event_loop;
mod events;
#[rustfmt::skip]
pub mod generated;
mod host;
mod html;
mod idle_callbacks;
mod intersection_observer;
mod location;
mod media_queries;
mod mutation_observer;
mod mutation_registry;
mod navigator;
mod performance;
mod performance_observer;
mod resize_observer;
mod runtime;
mod scripts;
mod state;
mod storage;
mod structured;
mod svg;
mod text_codec;
mod timers;
mod url_api;
mod window;

pub use clock::{ClockMode, PageClock};
pub use events::{DispatchTarget, EventInterface, TrustedEvent, UserEvent, UserKind, modifiers};
pub use host::{
    ControlValidity, ControlWrite, DomRect, EngineHost, EventOrigins, NavigationInfo,
    NavigationKind, ObservedBox, ObservedPosition, OffsetMetrics, RangeTextMode, ScrollAlign,
    ScrollMetrics, ScrollRequest, SelectionKey, SelectionRange, Viewport,
};
pub use idle_callbacks::IDLE_PERIOD_MS;
pub use runtime::{
    ConsoleMessage, EVALUATION_TIMEOUT, Evaluation, EvaluationId, EvaluationStart, HEAP_LIMIT,
    JsonForm, SCRIPT_BUDGET, ScriptRuntime, TASK_TIME_LIMIT,
};
pub use scripts::{FetchedScript, is_javascript_mime_type};
pub use storage::{
    AREA_COST_BYTES, AREA_QUOTA_UNITS, KEY_COST_BYTES, MAX_KEYS_PER_AREA, QuotaExceeded,
    SharedStorage, StorageKind, StorageStore, TOTAL_LIMIT_BYTES,
};
pub use structured::Serialized;
pub use swb_js::{ConsoleLevel, ScriptError};
