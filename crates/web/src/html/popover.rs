//! Popovers for scripts (HTML §6.12, <https://html.spec.whatwg.org/multipage/popover.html>;
//! ADR 0037 sections 2, 3 and 9): `showPopover`, `hidePopover`,
//! `togglePopover`, `popoverTargetElement`, the activation behavior of the
//! invokers, the attribute change steps of `popover`, and the `toggle` task.
//!
//! The algorithms are `swb_dom`'s (`top_layer.rs`); [`WebHost`] is their
//! host here, also for the dialog algorithms (`dialog.rs`): it fires
//! `beforetoggle` (a trusted `ToggleEvent`) and `cancel` through the bindings
//! at once, queues the `toggle` and `close` tasks in the document's event
//! loop, sets a dialog's `open` attribute and moves the focus. A termination
//! of a listener stops the algorithm and goes up.

use swb_dom::{
    Document, NodeId, PopoverError, PopoverErrorKind, TargetAction, ToggleState, TopLayerHost,
};
use swb_js::{Gc, Object, Runtime, VmError, VmResult};

use crate::bindings::{self as b, Cx};
use crate::event_loop::Task;
use crate::events::{DispatchTarget, EventInterface, TrustedEvent, fire_trusted};
use crate::state::{dom, dom_mut, state, state_mut};

/// The host of the popover algorithms for scripts.
pub(crate) struct WebHost<'a> {
    rt: &'a mut Runtime,
    /// The first error of the runtime (a termination): the algorithm stops.
    error: Option<VmError>,
    /// A document to lend when the runtime has none (the algorithm then
    /// stops at its next event).
    spare: Option<Document>,
    /// The `source` option of the call when it is an element of another
    /// document, with the popover that the call shows: the algorithm gets no
    /// source (its arena cannot name the node), the show's `beforetoggle`
    /// gets the wrapper.
    foreign: Option<(NodeId, Gc<Object>)>,
}

impl<'a> WebHost<'a> {
    pub(crate) fn new(rt: &'a mut Runtime) -> Self {
        WebHost {
            rt,
            error: None,
            spare: None,
            foreign: None,
        }
    }

    /// A host for a call with the option `source`: a node of another
    /// document goes to the event only. Returns the host and the source
    /// that the algorithm gets.
    fn with_source(
        rt: &'a mut Runtime,
        element: NodeId,
        source: Option<b::NodeRef>,
    ) -> (Self, Option<NodeId>) {
        let mut host = WebHost::new(rt);
        let here = source.and_then(|s| s.here(host.rt));
        if here.is_none() {
            host.foreign = source.map(|s| (element, s.object));
        }
        (host, here)
    }

    /// The result of an algorithm: the stored error of a stop, else the
    /// algorithm's error as Chromium's `DOMException`.
    pub(crate) fn finish<T>(self, result: Result<T, PopoverError>) -> b::Result<T> {
        if let Some(error) = self.error {
            return Err(error.into());
        }
        result.map_err(|error| match error.kind {
            PopoverErrorKind::Stopped => VmError::Internal(swb_js::InternalError::Invariant(
                "a popover algorithm stops only after an error of the host",
            ))
            .into(),
            _ => b::Exception::dom(error.name(), error.message()),
        })
    }
}

impl TopLayerHost for WebHost<'_> {
    fn doc(&mut self) -> &mut Document {
        match dom_mut(self.rt) {
            Ok(doc) => doc,
            Err(error) => {
                self.error.get_or_insert(error);
                self.spare.get_or_insert_with(Document::new)
            }
        }
    }

    fn fire_beforetoggle(
        &mut self,
        element: NodeId,
        old: ToggleState,
        new: ToggleState,
        cancelable: bool,
        source: Option<NodeId>,
    ) -> Option<bool> {
        if self.error.is_some() {
            return None;
        }
        let mut event = TrustedEvent::new("beforetoggle");
        event.cancelable = cancelable;
        let foreign = self
            .foreign
            .filter(|&(popover, _)| popover == element && new == ToggleState::Open)
            .map(|(_, object)| object);
        event.interface = EventInterface::Toggle {
            old,
            new,
            source,
            foreign,
        };
        match fire_trusted(self.rt, DispatchTarget::Node(element), &event) {
            Ok(proceed) => Some(proceed),
            Err(error) => {
                self.error = Some(error);
                None
            }
        }
    }

    /// Queues the task at the end and removes the element's task that waits
    /// (HTML: "remove ... task from its task queue"), so that an element has
    /// at most one task in the queue.
    fn queue_toggle_task(&mut self, element: NodeId, serial: u64) {
        let now = crate::event_loop::page_now(self.rt);
        let state = match state_mut(self.rt) {
            Ok(state) => state,
            Err(error) => {
                self.error.get_or_insert(error);
                return;
            }
        };
        if let Some(old) = state.doc.toggle_tasks.remove(&element) {
            state.event_loop.tasks.remove(old);
        }
        let task = Task::Toggle {
            node: element,
            serial,
        };
        let key = state.event_loop.tasks.push(now, task);
        state.doc.toggle_tasks.insert(element, key);
    }

    fn focused_element(&mut self) -> Option<NodeId> {
        if self.error.is_some() {
            return None;
        }
        super::focus::focused(self.rt).ok().flatten()
    }

    /// The focusing steps with their events, if `element` is a focusable
    /// area that is rendered.
    fn focus(&mut self, element: NodeId) -> bool {
        if self.error.is_some() {
            return false;
        }
        match super::focus::focus_popover_target(self.rt, element) {
            Ok(focused) => focused,
            Err(error) => {
                self.error = Some(error);
                false
            }
        }
    }

    fn focus_dialog(&mut self, dialog: NodeId) -> bool {
        if self.error.is_some() {
            return false;
        }
        match super::focus::focus_dialog(self.rt, dialog) {
            Ok(focused) => focused,
            Err(error) => {
                self.error = Some(error);
                false
            }
        }
    }

    fn set_dialog_open(&mut self, dialog: NodeId, open: bool) -> bool {
        if self.error.is_some() {
            return false;
        }
        match super::dialog::set_open(self.rt, dialog, open) {
            Ok(()) => true,
            Err(error) => {
                self.error = Some(error);
                false
            }
        }
    }

    fn fire_cancel(&mut self, dialog: NodeId, cancelable: bool) -> Option<bool> {
        if self.error.is_some() {
            return None;
        }
        let mut event = TrustedEvent::new("cancel");
        event.cancelable = cancelable;
        match fire_trusted(self.rt, DispatchTarget::Node(dialog), &event) {
            Ok(proceed) => Some(proceed),
            Err(error) => {
                self.error = Some(error);
                None
            }
        }
    }

    fn queue_dialog_toggle_task(&mut self, dialog: NodeId, serial: u64) {
        if let Err(error) = super::dialog::queue_toggle_task(self.rt, dialog, serial) {
            self.error.get_or_insert(error);
        }
    }

    fn queue_close_task(&mut self, dialog: NodeId) {
        if let Err(error) = super::dialog::queue_close_task(self.rt, dialog) {
            self.error.get_or_insert(error);
        }
    }
}

// ----- Light dismiss and close requests -----

/// The popovers that the invokers on the flat tree path of `node` name, for
/// the "nearest inclusive target popover" of light dismiss.
fn targets_on_path(rt: &Runtime, node: NodeId) -> VmResult<Vec<(NodeId, NodeId)>> {
    let doc = dom(rt)?;
    let mut targets = Vec::new();
    let mut current = Some(node);
    while let Some(n) = current {
        if swb_dom::can_invoke(doc, n)
            && let Some(popover) = invoker_target(rt, n)?
        {
            targets.push((n, popover));
        }
        current = doc.flat_parent(n);
    }
    Ok(targets)
}

/// Looks up the popover that the invoker `node` names in `targets`.
fn lookup(targets: &[(NodeId, NodeId)], node: NodeId) -> Option<NodeId> {
    targets
        .iter()
        .find(|&&(invoker, _)| invoker == node)
        .map(|&(_, popover)| popover)
}

/// A pointer button went down on `node`: the "popover pointerdown target"
/// and the "dialog pointerdown target" (HTML "light dismiss open popovers"
/// and "light dismiss open dialogs"; no script runs). `outside_modal`: the
/// press is on an open modal dialog outside its border box.
pub(crate) fn light_dismiss_down(
    rt: &mut Runtime,
    node: NodeId,
    outside_modal: bool,
) -> VmResult<()> {
    let targets = if dom(rt)?.light_dismiss_active() {
        targets_on_path(rt, node)?
    } else {
        Vec::new()
    };
    let doc = dom_mut(rt)?;
    swb_dom::light_dismiss_down(doc, node, |n| lookup(&targets, n));
    swb_dom::light_dismiss_dialogs_down(doc, node, outside_modal);
    Ok(())
}

/// A pointer button went up on `node`: the popovers that the press and the
/// release left outside hide, with their events; then a topmost dialog with
/// `closedby=any` that they left outside gets a close request.
pub(crate) fn light_dismiss_up(
    rt: &mut Runtime,
    node: NodeId,
    outside_modal: bool,
) -> VmResult<()> {
    let targets = if dom(rt)?.light_dismiss_active() {
        targets_on_path(rt, node)?
    } else {
        Vec::new()
    };
    let mut host = WebHost::new(rt);
    let result = swb_dom::light_dismiss_up(&mut host, node, |n| lookup(&targets, n))
        .and_then(|()| swb_dom::light_dismiss_dialogs_up(&mut host, node, outside_modal));
    // Other errors end the dismissal silently.
    match host.finish(result) {
        Err(b::Exception::Js(error)) => Err(error),
        _ => Ok(()),
    }
}

/// HTML "process close watchers" (the Escape key): returns whether a
/// watcher was processed.
pub(crate) fn process_close_watchers(rt: &mut Runtime) -> VmResult<bool> {
    let mut host = WebHost::new(rt);
    let result = swb_dom::process_close_watchers(&mut host);
    match host.finish(result) {
        Ok(processed) => Ok(processed),
        Err(b::Exception::Js(error)) => Err(error),
        Err(_) => Ok(true),
    }
}

/// `showPopover(options)`.
pub(crate) fn show(cx: &mut Cx<'_>, this: NodeId, source: Option<b::NodeRef>) -> b::Result<()> {
    let (mut host, source) = WebHost::with_source(cx.rt, this, source);
    let result = swb_dom::show_popover(&mut host, this, true, source);
    host.finish(result)
}

/// `hidePopover()`.
pub(crate) fn hide(cx: &mut Cx<'_>, this: NodeId) -> b::Result<()> {
    let options = swb_dom::HideOptions {
        focus_previous: true,
        fire_events: true,
        throw: true,
        ignore_dom: false,
        source: None,
    };
    let mut host = WebHost::new(cx.rt);
    let result = swb_dom::hide_popover(&mut host, this, options);
    host.finish(result)
}

/// `togglePopover(options)`: `force` and `source` from the options.
pub(crate) fn toggle(
    cx: &mut Cx<'_>,
    this: NodeId,
    force: Option<bool>,
    source: Option<b::NodeRef>,
) -> b::Result<bool> {
    let (mut host, source) = WebHost::with_source(cx.rt, this, source);
    let result = swb_dom::toggle_popover(&mut host, this, force, source);
    host.finish(result)
}

/// Runs the attribute change steps of `popover` after a change of the
/// attribute (`dom/attrs.rs`): a showing popover whose state changed hides
/// with events.
pub(crate) fn attribute_changed(rt: &mut Runtime) -> b::Result<()> {
    if !dom(rt)?.has_popover_attribute_hides() {
        return Ok(());
    }
    let mut host = WebHost::new(rt);
    let result = swb_dom::run_popover_attribute_steps(&mut host);
    host.finish(result)
}

/// The `toggle` task of `node` (HTML "queue a popover toggle event task"):
/// fires `toggle` with the coalesced states, unless a later change replaced
/// the task.
pub(crate) fn toggle_task(rt: &mut Runtime, node: NodeId, serial: u64) -> VmResult<()> {
    state_mut(rt)?.doc.toggle_tasks.remove(&node);
    let Some(toggle) = dom_mut(rt)?.take_toggle(node, serial) else {
        return Ok(());
    };
    // A node that went to another arena is an empty comment here.
    if dom(rt)?.element(node).is_none() {
        return Ok(());
    }
    let mut event = TrustedEvent::new("toggle");
    event.interface = EventInterface::Toggle {
        old: toggle.old,
        new: toggle.new,
        source: toggle
            .source
            .filter(|&s| dom(rt).is_ok_and(|d| d.element(s).is_some())),
        // A source of another document is not kept for the task.
        foreign: None,
    };
    fire_trusted(rt, DispatchTarget::Node(node), &event).map(|_| ())
}

// ----- Invokers -----

/// HTML "get the `popovertarget`-associated element" of `invoker`: the
/// element that a script set (`popoverTargetElement`) if it is a descendant
/// of one of the invoker's shadow-including ancestors, else the element of
/// the id in the invoker's tree.
/// <https://html.spec.whatwg.org/multipage/common-dom-interfaces.html#attr-associated-element>
pub(crate) fn associated_target(rt: &Runtime, invoker: NodeId) -> VmResult<Option<NodeId>> {
    let doc = dom(rt)?;
    if !doc
        .element(invoker)
        .is_some_and(|e| e.has_attr("popovertarget"))
    {
        return Ok(None);
    }
    if let Some(&target) = state(rt)?.doc.popover_targets.get(&invoker) {
        // The target is a descendant of one of the invoker's
        // shadow-including ancestors if the root of its tree is one of them.
        let root = doc.root(target);
        let reachable = doc.get(target).is_some_and(swb_dom::Node::is_element)
            && target != root
            && root != invoker
            && doc.is_shadow_including_inclusive_ancestor(root, invoker);
        return Ok(reachable.then_some(target));
    }
    Ok(swb_dom::popover_target_by_id(doc, invoker))
}

/// HTML "popover target element" of `invoker` for its activation: the
/// associated element if it is a popover and the invoker acts (an invoker
/// kind, not disabled; Chromium: a submit or reset button with a form owner
/// submits or resets instead, measured).
pub(crate) fn invoker_target(rt: &Runtime, invoker: NodeId) -> VmResult<Option<NodeId>> {
    let doc = dom(rt)?;
    if !swb_dom::can_invoke(doc, invoker) || swb_style::is_actually_disabled(doc, invoker) {
        return Ok(None);
    }
    // A `button` of any type but `button`, and an `input` of type
    // `submit`, `image` or `reset`.
    if swb_dom::invoker_acts_on_form(doc, invoker) && super::form_owner(doc, invoker).is_some() {
        return Ok(None);
    }
    let target = associated_target(rt, invoker)?;
    Ok(target.filter(|&t| dom(rt).is_ok_and(|d| d.popover_state(t).is_some())))
}

/// The activation behavior of the invoker `invoker` for a click whose target
/// was `event_target` (HTML "popover target attribute activation
/// behavior").
pub(crate) fn activate(rt: &mut Runtime, invoker: NodeId, event_target: NodeId) -> VmResult<()> {
    let Some(popover) = invoker_target(rt, invoker)? else {
        return Ok(());
    };
    let action = TargetAction::of(
        dom(rt)?
            .element(invoker)
            .and_then(|e| e.attr("popovertargetaction")),
    );
    let mut host = WebHost::new(rt);
    let result =
        swb_dom::popover_target_activation(&mut host, invoker, popover, (action, event_target));
    // Other errors end the activation silently.
    if let Err(b::Exception::Js(error)) = host.finish(result) {
        return Err(error);
    }
    Ok(())
}

// ----- popoverTargetElement -----

/// The getter of `popoverTargetElement`.
pub(crate) fn target_element(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
    Ok(associated_target(cx.rt, this)?)
}

/// The setter of `popoverTargetElement`: `null` removes the attribute; an
/// element sets it to the empty string and is kept (HTML "attr-associated
/// element" reflection). An element of another document is accepted and the
/// getter then gives `null`, as in Chromium (measured, `popover-target-arenas`):
/// it sets the attribute and keeps nothing, since a node of another arena
/// is never reachable.
pub(crate) fn set_target_element(
    cx: &mut Cx<'_>,
    this: NodeId,
    value: Option<b::NodeRef>,
) -> b::Result<()> {
    match value {
        None => super::remove_attribute(cx, this, "popovertarget"),
        Some(target) => {
            let here = target.here(cx.rt);
            super::set_attribute(cx, this, "popovertarget", "")?;
            let targets = &mut state_mut(cx.rt)?.doc.popover_targets;
            match here {
                Some(target) => {
                    targets.insert(this, target);
                }
                None => {
                    targets.remove(&this);
                }
            }
            Ok(())
        }
    }
}

/// A move of nodes to another arena (`state::move_node_state`): the
/// elements that scripts set as `popoverTargetElement` follow when invoker
/// and target both move (Chromium keeps the association, measured). An
/// entry with one end left behind is dropped: the other end is in another
/// arena and never reachable. Ids are never reused, so an old id reads an
/// empty comment, not another node.
pub(crate) fn move_targets(
    from: &mut crate::state::DocState,
    to: &mut crate::state::DocState,
    pairs: &[(NodeId, NodeId)],
) {
    if from.popover_targets.is_empty() {
        return;
    }
    let new_ids: std::collections::HashMap<NodeId, NodeId> = pairs.iter().copied().collect();
    from.popover_targets.retain(|invoker, target| {
        match (new_ids.get(invoker), new_ids.get(target)) {
            (Some(&invoker), Some(&target)) => {
                to.popover_targets.insert(invoker, target);
                false
            }
            (None, None) => true,
            _ => false,
        }
    });
}

/// A change of the `popovertarget` content attribute of `element` forgets
/// the element that a script set.
pub(crate) fn target_attribute_changed(rt: &mut Runtime, element: NodeId) -> VmResult<()> {
    let targets = &mut state_mut(rt)?.doc.popover_targets;
    if !targets.is_empty() {
        targets.remove(&element);
    }
    Ok(())
}
