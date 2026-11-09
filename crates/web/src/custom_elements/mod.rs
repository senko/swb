//! Custom elements (HTML §4.13,
//! <https://html.spec.whatwg.org/multipage/custom-elements.html>; ADR 0035).
//!
//! - The definitions belong to the agent ([`Agent`], in `WebState`): an
//!   element keeps its definition when it moves to another document of the
//!   runtime. The number in the element's data (`swb_dom::ElementData`, 0 for
//!   none) is the index in [`Agent::definitions`] plus one. Definitions are
//!   never removed.
//! - A window has one registry ([`Registry`], in `DocState`): its names and
//!   constructors, the "element definition is running" flag and the
//!   when-defined promises. `registry.rs` has the `CustomElementRegistry`
//!   interface (`define`, `get`, `getName`, `whenDefined`, `upgrade`).
//! - `constructor.rs`: the HTML element constructors (`[HTMLConstructor]`)
//!   with the construction stack of each definition; `create.rs`: "create an
//!   element" with the synchronous custom elements flag (`createElement`,
//!   `createElementNS`); `upgrade.rs`: the upgrade, "try to upgrade" and the
//!   upgrade candidates of a new definition; `reactions.rs`: the custom
//!   element reactions (the element queues, the reactions stack, the backup
//!   element queue, the invocation with its nesting limit).
//!
//! Only the window's document has a registry: a document of
//! `createHTMLDocument`, `DOMParser` or template contents has none (measured,
//! cases `create-element-sync` and `upgrade-at-define`). In an arena, the
//! window's document is [`NodeId::DOCUMENT`].
//!
//! Out of scope (ADR 0035 section 9): customized built-in elements (a valid
//! `extends` throws), scoped registries, `ElementInternals` and
//! form-associated custom elements, `connectedMoveCallback` (read, never
//! called).

pub(crate) mod constructor;
pub(crate) mod create;
pub(crate) mod reactions;
mod registry;
pub(crate) mod upgrade;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use swb_dom::{LocalName, NodeId};
use swb_js::{Gc, Object, Runtime, Tracer, VmError, VmResult};

use crate::state::{DocKey, state, state_mut};

pub(crate) use reactions::Reactions;
pub(crate) use registry::registry_object;

/// The most definitions of an agent (ADR 0035 section 10; Chromium has no
/// limit).
pub(crate) const MAX_DEFINITIONS: usize = 65_536;

/// The most observed attribute names of all definitions of an agent, and
/// the most bytes of their text.
pub(crate) const MAX_OBSERVED_NAMES: usize = 1 << 20;
pub(crate) const MAX_OBSERVED_BYTES: usize = 16 << 20;

/// The most pending when-defined promises of a registry.
pub(crate) const MAX_WHEN_DEFINED: usize = 65_536;

/// The most bytes of the names of the definitions of an agent, and of the
/// names that wait in `whenDefined` in a registry (the engine's heap limit
/// does not count these Rust copies; ADR 0035 section 10).
pub(crate) const MAX_NAME_BYTES: usize = 16 << 20;

/// The lifecycle callbacks of a definition, in the order in which `define`
/// reads them (measured, case `define-read-order`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Callback {
    Connected,
    Disconnected,
    ConnectedMove,
    Adopted,
    AttributeChanged,
}

impl Callback {
    /// All callbacks, in the order of `define`'s reads.
    pub(crate) const ALL: [Callback; 5] = [
        Callback::Connected,
        Callback::Disconnected,
        Callback::ConnectedMove,
        Callback::Adopted,
        Callback::AttributeChanged,
    ];

    /// The property name of the callback on the prototype.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Callback::Connected => "connectedCallback",
            Callback::Disconnected => "disconnectedCallback",
            Callback::ConnectedMove => "connectedMoveCallback",
            Callback::Adopted => "adoptedCallback",
            Callback::AttributeChanged => "attributeChangedCallback",
        }
    }
}

/// An entry of a construction stack: an element that waits for its
/// constructor, or the "already constructed" marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StackEntry {
    Element(DocKey, NodeId),
    AlreadyConstructed,
}

/// HTML "custom element definition" (autonomous custom elements only).
pub(crate) struct Definition {
    /// The name, which is also the local name.
    pub(crate) name: LocalName,
    pub(crate) constructor: Gc<Object>,
    /// The lifecycle callbacks in the order of [`Callback::ALL`].
    pub(crate) callbacks: [Option<Gc<Object>>; 5],
    pub(crate) observed: HashSet<LocalName>,
    /// Read and kept; swb has no `ElementInternals` (ADR 0035 section 9).
    #[allow(dead_code, reason = "kept for ElementInternals (backlog)")]
    pub(crate) form_associated: bool,
    #[allow(dead_code, reason = "kept for ElementInternals (backlog)")]
    pub(crate) disable_internals: bool,
    #[allow(
        dead_code,
        reason = "checked at an upgrade with attachShadow (feature 5)"
    )]
    pub(crate) disable_shadow: bool,
    pub(crate) construction_stack: Vec<StackEntry>,
}

impl Definition {
    /// The function of a lifecycle callback, if the prototype had one.
    pub(crate) fn callback(&self, callback: Callback) -> Option<Gc<Object>> {
        let index = Callback::ALL.iter().position(|&c| c == callback)?;
        self.callbacks.get(index).copied().flatten()
    }
}

/// The custom element state of the agent.
#[derive(Default)]
pub(crate) struct Agent {
    /// The definitions; the number of a definition is its index plus one.
    pub(crate) definitions: Vec<Definition>,
    /// The reactions, Rust data only (shared with the documents' mutation
    /// sinks with ADR 0035 sub-feature 4b).
    pub(crate) reactions: Rc<RefCell<Reactions>>,
    /// The observed attribute names of all definitions and their bytes.
    pub(crate) observed_names: usize,
    pub(crate) observed_bytes: usize,
    /// The bytes of the names of the definitions.
    pub(crate) name_bytes: usize,
    /// True after the warning about the definition budget.
    pub(crate) budget_warned: bool,
    /// The function of the backup element queue's microtask, made at its
    /// first use.
    pub(crate) backup_fn: Option<Gc<Object>>,
}

impl Agent {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for definition in &self.definitions {
            tracer.object(definition.constructor);
            for callback in definition.callbacks.iter().flatten() {
                tracer.object(*callback);
            }
        }
        if let Some(function) = self.backup_fn {
            tracer.object(function);
        }
    }
}

/// The `CustomElementRegistry` of a window.
#[derive(Default)]
pub(crate) struct Registry {
    /// The object, made at the first read of `window.customElements`.
    pub(crate) object: Option<Gc<Object>>,
    /// The definitions by name and by constructor.
    pub(crate) by_name: HashMap<LocalName, u32>,
    pub(crate) by_constructor: HashMap<Gc<Object>, u32>,
    /// HTML "element definition is running".
    pub(crate) running: bool,
    /// HTML "when-defined promise map".
    pub(crate) when_defined: HashMap<String, Gc<Object>>,
    /// The bytes of the keys of `when_defined`.
    pub(crate) when_defined_bytes: usize,
    /// True after the warning about the when-defined budget.
    pub(crate) when_defined_warned: bool,
}

impl Registry {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        if let Some(object) = self.object {
            tracer.object(object);
        }
        for promise in self.when_defined.values() {
            tracer.object(*promise);
        }
    }
}

/// The definition with this number.
pub(crate) fn definition(rt: &Runtime, number: u32) -> VmResult<&Definition> {
    let index = (number as usize).wrapping_sub(1);
    state(rt)?
        .custom
        .definitions
        .get(index)
        .ok_or(VmError::invariant(
            "a custom element definition number without a definition",
        ))
}

/// The definition with this number, for changes.
pub(crate) fn definition_mut(rt: &mut Runtime, number: u32) -> VmResult<&mut Definition> {
    let index = (number as usize).wrapping_sub(1);
    state_mut(rt)?
        .custom
        .definitions
        .get_mut(index)
        .ok_or(VmError::invariant(
            "a custom element definition number without a definition",
        ))
}

/// HTML "look up a custom element definition" for an HTML element named
/// `local` whose node document is `document` (a node of the current
/// document's arena): the number of the definition in the window's registry,
/// if `document` is the window's document.
/// <https://html.spec.whatwg.org/multipage/custom-elements.html#look-up-a-custom-element-definition>
pub(crate) fn look_up(rt: &Runtime, document: NodeId, local: &LocalName) -> Option<u32> {
    if document != NodeId::DOCUMENT {
        return None;
    }
    let state = state(rt).ok()?;
    if state.custom.definitions.is_empty() {
        return None;
    }
    state.doc.custom.by_name.get(local).copied()
}

/// True if the agent has a custom element definition.
#[inline]
pub(crate) fn any_definitions(rt: &Runtime) -> bool {
    rt.host_flags() & (1 << crate::bindings::ce::DEFINITIONS_BIT) != 0
}
