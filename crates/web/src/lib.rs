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
//!   `state` holds the binding state (what the agent shares, and for each
//!   document the wrapper vector, the side table, the listeners of nodes and
//!   the window; M9 feature 12a, ADR 0031 section 5); `dom`, `window` and
//!   `events` the behaviour of the interfaces; `window_proxy` the cross-origin
//!   `Window` and `Location` (M9 feature 13a).
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
//! - The DOM and HTML interfaces (features 6 and 11): `dom` (nodes, lists,
//!   `dataset`, attributes), `html` (the HTML element interfaces and their
//!   activation behaviour), `svg` (the SVG element interfaces),
//!   `mutation_observer` with `mutation_registry`, `intersection_observer`
//!   and `resize_observer` (feature 9a), and `image_events` (the `load` and
//!   `error` events of images and style sheets).
//! - CSS for scripts: `cssom` (`element.style`, `CSS`), `cssom_view`
//!   (geometry, scrolling, `screen`), and `dom_matrix` over `matrix`
//!   (`DOMMatrix`, `DOMPoint`).
//! - Navigation and data: `location` (`Location`, `History`), `structured`
//!   (`structuredClone`, the state of `pushState`), `url_api` (`URL`,
//!   `URLSearchParams`), `abort` (`AbortController`, `AbortSignal`).
//! - Network APIs (feature 10): `fetch` (`fetch()`, `Headers`, `Request`,
//!   `Response`), `xhr` (`XMLHttpRequest`) and `file_api` (`Blob`, `File`,
//!   `FormData`, `FileReader`, Blob URLs).
//! - Other services (feature 11): `clipboard` (`navigator.clipboard`) and
//!   `dialogs` (`alert`, `confirm`, `prompt`, `print`); `mime` (the essence
//!   of a MIME type string).
//! - `host`: the [`EngineHost`] and [`FrameHost`] traits and the data types
//!   that cross them.
//! - `frames`: frames for scripts (ADR 0031 section 5): the realm of a frame
//!   document at its first need, the members of `Window` and `iframe` that name
//!   frames, making and destroying frames inside a task.

mod abort;
mod animation_frames;
mod base64;
pub mod bindings;
mod clipboard;
mod clock;
mod cookie;
mod crypto;
mod cssom;
mod cssom_sheets;
mod cssom_view;
mod dialogs;
mod document_load;
mod document_write;
mod dom;
mod dom_matrix;
mod domain;
mod errors;
mod event_loop;
mod events;
mod fetch;
mod file_api;
#[rustfmt::skip]
pub mod generated;
mod frames;
mod host;
mod html;
mod idle_callbacks;
mod image_events;
mod intersection_observer;
mod location;
mod matrix;
mod media_queries;
mod message_port;
mod messaging;
mod mime;
mod mutation_observer;
mod mutation_registry;
mod navigator;
mod page_budget;
mod performance;
mod performance_observer;
mod resize_observer;
mod runtime;
mod scripts;
mod state;
mod storage;
mod storage_events;
mod structured;
mod svg;
mod text_codec;
mod timers;
mod url_api;
mod window;
mod window_proxy;
mod xhr;

pub use clock::{ClockMode, PageClock};
pub use events::{DispatchTarget, EventInterface, TrustedEvent, UserEvent, UserKind, modifiers};
pub use host::{
    ControlSnapshot, ControlValidity, ControlWrite, DomRect, EngineHost, EventOrigins,
    FormDataEntry, FormDataValue, FrameHost, ImageInfo, LentDocuments, NavigationInfo,
    NavigationKind, NewFrame, ObservedBox, ObservedPosition, OffsetMetrics, RangeTextMode,
    RetiredFrame, ScrollAlign, ScrollMetrics, ScrollRequest, SelectionKey, SelectionRange,
    Viewport,
};
pub use idle_callbacks::IDLE_PERIOD_MS;
pub use messaging::{Mail, MailTarget};
pub use runtime::{
    AgentShared, ConsoleMessage, DocumentKey, EVALUATION_TIMEOUT, Evaluation, EvaluationId,
    EvaluationStart, HEAP_LIMIT, JsonForm, SCRIPT_BUDGET, ScriptRuntime, TASK_TIME_LIMIT,
};
pub use scripts::{FetchedScript, ScriptFetchMode, is_javascript_mime_type};
pub use storage::{
    AREA_COST_BYTES, AREA_QUOTA_UNITS, KEY_COST_BYTES, MAX_KEYS_PER_AREA, QuotaExceeded,
    SharedStorage, StorageKey, StorageKind, StoragePartition, StorageStore, TOTAL_LIMIT_BYTES,
};
pub use structured::Serialized;
pub use swb_js::{ConsoleLevel, ScriptError};
