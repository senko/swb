//! The custom element reactions of the agent (HTML §4.13.6,
//! <https://html.spec.whatwg.org/multipage/custom-elements.html#custom-element-reactions>;
//! ADR 0035 section 5).
//!
//! [`Reactions`] is Rust data only: the element reaction queues by element
//! ([`Place`]: document key and node), the reactions stack and the backup
//! element queue. The stack is one flat `Vec` of queued elements with a
//! `Vec` of marks: pushing an element queue records the length, popping
//! takes the suffix. A nested scope always pops before its parent goes on,
//! so the suffix after the last mark is HTML's current element queue.
//!
//! An element can be in an element queue several times; its reactions run
//! when the queue first reaches it (measured, case `reactions-queue-order`).
//!
//! The nesting limit is Chromium's (measured in three shapes, ADR 0035
//! section 5): at [`MAX_DEPTH`] nested invocations (callbacks, upgrades,
//! synchronous constructions), a further reaction is dropped without a
//! report. It also bounds the Rust stack: each level is one re-entry of the
//! interpreter.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::collections::hash_map::{Entry, HashMap};
use std::rc::Rc;

use swb_dom::{LocalName, NodeId};
use swb_js::{NativeCall, NativeReturn, Runtime, ScriptError, Value, VmError, VmResult};

use super::Callback;
use crate::state::{DocKey, state, state_mut};

/// An element of a document of the runtime.
pub(crate) type Place = (DocKey, NodeId);

/// The most nested reaction invocations and synchronous constructions
/// (Chromium 148, measured: cases `reactions-depth-limit`,
/// `reactions-recursion-depth`, `reactions-reinsert-loop`).
pub(crate) const MAX_DEPTH: u32 = 43;

/// The most queued entries (reactions and element queue entries) of the
/// agent, and the most bytes of their argument text (ADR 0035 section 10).
pub(crate) const MAX_QUEUED: usize = 1 << 22;
pub(crate) const MAX_QUEUED_BYTES: usize = 128 << 20;

/// The arguments of a callback reaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Args {
    None,
    /// `attributeChangedCallback(localName, oldValue, newValue, namespace)`.
    Attribute {
        local: LocalName,
        old: Option<String>,
        new: Option<String>,
        namespace: Option<String>,
    },
}

impl Args {
    /// The bytes of argument text, for the budget.
    fn bytes(&self) -> usize {
        match self {
            Args::None => 0,
            Args::Attribute {
                local,
                old,
                new,
                namespace,
            } => {
                local.len()
                    + old.as_ref().map_or(0, String::len)
                    + new.as_ref().map_or(0, String::len)
                    + namespace.as_ref().map_or(0, String::len)
            }
        }
    }
}

/// A custom element reaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Reaction {
    /// An upgrade reaction with the number of its definition.
    Upgrade(u32),
    /// A callback reaction. The function is looked up in the element's
    /// definition when it runs (definitions do not change).
    Callback(Callback, Args),
}

/// The reaction queue of one element. Most elements have one reaction at a
/// time (an upgrade, a connected callback), so the first is kept inline: a
/// `VecDeque` costs an allocation of four reactions (ADR 0035 "4a
/// implementation notes": 100,000 upgrades).
#[derive(Debug)]
enum Pending {
    One(Reaction),
    Many(VecDeque<Reaction>),
}

/// What an enqueue did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Enqueued {
    /// Into the current element queue, or into the backup queue whose
    /// microtask is queued already.
    Done,
    /// Into the backup element queue, which needs its microtask now.
    NeedsMicrotask,
    /// Dropped: the budget is spent.
    Dropped,
}

/// The reaction state of the agent.
#[derive(Debug, Default)]
pub(crate) struct Reactions {
    queues: HashMap<Place, Pending>,
    /// The elements of all element queues of the stack, oldest first.
    stack: Vec<Place>,
    /// The start of each element queue in `stack`.
    marks: Vec<usize>,
    backup: Vec<Place>,
    /// HTML "processing the backup element queue" flag.
    backup_processing: bool,
    /// The nested invocations.
    depth: u32,
    /// The queued reactions and element queue entries, and the bytes of
    /// their argument text.
    queued: usize,
    bytes: usize,
    warned: bool,
}

impl Reactions {
    /// Pushes a new element queue on the reactions stack.
    pub(crate) fn push(&mut self) {
        self.marks.push(self.stack.len());
    }

    /// Pops the current element queue.
    pub(crate) fn pop(&mut self) -> Vec<Place> {
        let mark = self.marks.pop().unwrap_or(self.stack.len());
        let queue = self.stack.split_off(mark.min(self.stack.len()));
        self.queued = self.queued.saturating_sub(queue.len());
        queue
    }

    /// True if the reactions stack is empty.
    #[cfg(test)]
    fn stack_is_empty(&self) -> bool {
        self.marks.is_empty()
    }

    /// Adds `reaction` to the reaction queue of `place` and puts the element
    /// on the appropriate element queue (HTML "enqueue an element on the
    /// appropriate element queue").
    pub(crate) fn enqueue(&mut self, place: Place, reaction: Reaction) -> Enqueued {
        let bytes = match &reaction {
            Reaction::Callback(_, args) => args.bytes(),
            Reaction::Upgrade(_) => 0,
        };
        if self.queued + 2 > MAX_QUEUED || self.bytes + bytes > MAX_QUEUED_BYTES {
            if !self.warned {
                self.warned = true;
                log::warn!("too many custom element reactions are queued: later ones are dropped");
            }
            return Enqueued::Dropped;
        }
        self.queued += 2;
        self.bytes += bytes;
        match self.queues.entry(place) {
            Entry::Vacant(slot) => {
                slot.insert(Pending::One(reaction));
            }
            Entry::Occupied(mut slot) => {
                let pending = slot.get_mut();
                if let Pending::Many(queue) = pending {
                    queue.push_back(reaction);
                } else {
                    let old = std::mem::replace(pending, Pending::Many(VecDeque::new()));
                    if let (Pending::One(first), Pending::Many(queue)) = (old, pending) {
                        queue.push_back(first);
                        queue.push_back(reaction);
                    }
                }
            }
        }
        if !self.marks.is_empty() {
            self.stack.push(place);
            return Enqueued::Done;
        }
        self.backup.push(place);
        if self.backup_processing {
            return Enqueued::Done;
        }
        self.backup_processing = true;
        Enqueued::NeedsMicrotask
    }

    /// Takes the next reaction of `place`.
    pub(crate) fn take(&mut self, place: Place) -> Option<Reaction> {
        let reaction = match self.queues.get_mut(&place)? {
            Pending::One(_) => match self.queues.remove(&place) {
                Some(Pending::One(reaction)) => Some(reaction),
                _ => None,
            },
            Pending::Many(queue) => {
                let reaction = queue.pop_front();
                if queue.is_empty() {
                    self.queues.remove(&place);
                }
                reaction
            }
        };
        if let Some(reaction) = &reaction {
            self.queued = self.queued.saturating_sub(1);
            if let Reaction::Callback(_, args) = reaction {
                self.bytes = self.bytes.saturating_sub(args.bytes());
            }
        }
        reaction
    }

    /// Empties the reaction queue of `place` (a failed upgrade).
    pub(crate) fn clear(&mut self, place: Place) {
        while self.take(place).is_some() {}
    }

    /// Takes the backup element queue for its microtask.
    pub(crate) fn take_backup(&mut self) -> Vec<Place> {
        let queue = std::mem::take(&mut self.backup);
        self.queued = self.queued.saturating_sub(queue.len());
        queue
    }

    /// The end of the backup queue's microtask (or the loss of the microtask
    /// to a termination): the next enqueue queues a new one.
    pub(crate) fn backup_done(&mut self) {
        self.backup_processing = false;
    }

    /// Enters a nested invocation; false at the nesting limit (the
    /// reaction is dropped, without a report).
    pub(crate) fn enter(&mut self) -> bool {
        if self.depth >= MAX_DEPTH {
            return false;
        }
        self.depth += 1;
        true
    }

    /// Leaves a nested invocation.
    pub(crate) fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    /// The elements in the backup element queue, for the tests.
    #[cfg(test)]
    pub(crate) fn backup_len(&self) -> usize {
        self.backup.len()
    }

    /// The queued entries, for the tests of the budget.
    #[cfg(test)]
    fn queued(&self) -> (usize, usize) {
        (self.queued, self.bytes)
    }
}

/// The agent's reaction state.
pub(crate) fn shared(rt: &Runtime) -> VmResult<Rc<RefCell<Reactions>>> {
    Ok(Rc::clone(&state(rt)?.custom.reactions))
}

/// Pushes a new element queue (the start of a `[CEReactions]` scope): one
/// lookup of the state, no `Rc` clone.
pub(crate) fn push_queue(rt: &mut Runtime) {
    if let Ok(state) = state(rt) {
        state.custom.reactions.borrow_mut().push();
    }
}

/// Pops the current element queue and invokes its reactions (the end of a
/// `[CEReactions]` scope). Only a termination comes back as an error.
pub(crate) fn pop_and_invoke(rt: &mut Runtime) -> VmResult<()> {
    let queue = state(rt)?.custom.reactions.borrow_mut().pop();
    if queue.is_empty() {
        return Ok(());
    }
    invoke(rt, &queue)
}

/// Invokes the reactions of the backup element queue now (its microtask,
/// if one was queued, then finds it empty).
pub(crate) fn flush_backup(rt: &mut Runtime) -> VmResult<()> {
    let queue = state(rt)?.custom.reactions.borrow_mut().take_backup();
    if queue.is_empty() {
        return Ok(());
    }
    invoke(rt, &queue)
}

/// Enqueues `reaction` for `place`, and the backup queue's microtask if it
/// needs one.
pub(crate) fn enqueue(rt: &mut Runtime, place: Place, reaction: Reaction) -> VmResult<()> {
    let enqueued = shared(rt)?.borrow_mut().enqueue(place, reaction);
    if enqueued == Enqueued::NeedsMicrotask {
        queue_backup_microtask(rt)?;
    }
    Ok(())
}

/// HTML "invoke custom element reactions" of an element queue: for each
/// element, its reactions one at a time until its queue is empty (reactions
/// that an upgrade adds run in the same loop: constructor, attributes,
/// connected). An error of a reaction is reported; a termination stops the
/// invocation and drops the reactions that did not run.
pub(crate) fn invoke(rt: &mut Runtime, queue: &[Place]) -> VmResult<()> {
    for (position, &place) in queue.iter().enumerate() {
        if let Err(error) = invoke_element(rt, place) {
            let reactions = shared(rt)?;
            let mut reactions = reactions.borrow_mut();
            for &rest in queue.get(position..).unwrap_or_default() {
                reactions.clear(rest);
            }
            return Err(error);
        }
    }
    Ok(())
}

fn invoke_element(rt: &mut Runtime, place: Place) -> VmResult<()> {
    let reactions = shared(rt)?;
    loop {
        let reaction = reactions.borrow_mut().take(place);
        let Some(reaction) = reaction else {
            return Ok(());
        };
        if !reactions.borrow_mut().enter() {
            // Chromium drops the reaction at the nesting limit, without a
            // report, and never runs it later (measured).
            continue;
        }
        let result = match reaction {
            Reaction::Upgrade(number) => super::upgrade::upgrade_reaction(rt, place, number),
            Reaction::Callback(callback, args) => call_callback(rt, place, callback, args),
        };
        reactions.borrow_mut().leave();
        result?;
    }
}

/// Runs a callback reaction: the callback of the element's definition with
/// the element as `this`; an exception is reported for the callback's
/// realm, and the microtasks run if no script is on the stack (HTML "clean
/// up after running a callback").
fn call_callback(rt: &mut Runtime, place: Place, callback: Callback, args: Args) -> VmResult<()> {
    let entered = crate::state::switch_if_other(rt, place.0)?;
    let result = call_callback_here(rt, place.1, callback, args);
    crate::state::leave(rt, entered);
    result
}

fn call_callback_here(
    rt: &mut Runtime,
    element: NodeId,
    callback: Callback,
    args: Args,
) -> VmResult<()> {
    let (_, number) = crate::state::dom(rt)?.custom_element_state(element);
    if number == 0 {
        return Ok(());
    }
    let Some(function) = super::definition(rt, number)?.callback(callback) else {
        return Ok(());
    };
    let this = crate::state::wrap_node_with_proto(rt, element, None)?;
    let values = match args {
        Args::None => Vec::new(),
        Args::Attribute {
            local,
            old,
            new,
            namespace,
        } => {
            let mut values = Vec::with_capacity(4);
            for text in [Some(local.to_string()), old, new, namespace] {
                let value = match text {
                    Some(text) => rt.string_value(&text)?,
                    None => Value::Null,
                };
                // Kept at once: the next string may collect.
                crate::bindings::keep(rt, value);
                values.push(value);
            }
            values
        }
    };
    match rt.call_function(Value::Object(function), Value::Object(this), &values) {
        Ok(_) => {}
        Err(ScriptError::Terminated(termination)) => return Err(VmError::Terminated(termination)),
        Err(ScriptError::Internal(error)) => return Err(VmError::Internal(error)),
        Err(error) => {
            let site = crate::errors::Site::named("a custom element callback");
            crate::errors::report_callback(rt, function, &site, &error)?;
        }
    }
    crate::event_loop::after_listener(rt)
}

/// Queues the microtask of the backup element queue.
fn queue_backup_microtask(rt: &mut Runtime) -> VmResult<()> {
    let function = if let Some(function) = state(rt)?.custom.backup_fn {
        function
    } else {
        let realm = crate::state::doc_realm(rt)?;
        let function = rt.new_host_method(realm, "", 0, run_backup)?;
        state_mut(rt)?.custom.backup_fn = Some(function);
        function
    };
    rt.queue_microtask(Value::Object(function));
    Ok(())
}

/// The microtask of the backup element queue: invokes its reactions.
fn run_backup(rt: &mut Runtime, _call: &NativeCall) -> VmResult<NativeReturn> {
    let reactions = shared(rt)?;
    let queue = reactions.borrow_mut().take_backup();
    let result = invoke(rt, &queue);
    reactions.borrow_mut().backup_done();
    result?;
    Ok(NativeReturn::Value(Value::Undefined))
}

/// The job queue of the agent was cleared (a termination): the backup
/// queue's microtask is gone.
pub(crate) fn jobs_cleared(rt: &mut Runtime) {
    if let Ok(reactions) = shared(rt) {
        let mut reactions = reactions.borrow_mut();
        let queue = reactions.take_backup();
        for place in queue {
            reactions.clear(place);
        }
        reactions.backup_done();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(i: usize) -> Place {
        (0, NodeId::from_index(i).unwrap())
    }

    fn connected() -> Reaction {
        Reaction::Callback(Callback::Connected, Args::None)
    }

    /// The suffix after the last mark is the current element queue; a nested
    /// queue pops before its parent.
    #[test]
    fn the_stack_keeps_its_queues_apart_by_marks() {
        let mut r = Reactions::default();
        assert!(r.stack_is_empty());
        r.push();
        assert_eq!(r.enqueue(place(1), connected()), Enqueued::Done);
        r.push();
        assert_eq!(r.enqueue(place(2), connected()), Enqueued::Done);
        assert_eq!(r.enqueue(place(1), connected()), Enqueued::Done);
        assert_eq!(r.pop(), vec![place(2), place(1)]);
        assert_eq!(r.enqueue(place(3), connected()), Enqueued::Done);
        // An empty nested queue pops nothing.
        r.push();
        assert_eq!(r.pop(), Vec::<Place>::new());
        assert_eq!(r.pop(), vec![place(1), place(3)]);
        assert!(r.stack_is_empty());
        // The element's own queue keeps its reactions in order until taken.
        assert_eq!(r.take(place(1)), Some(connected()));
        assert_eq!(r.take(place(1)), Some(connected()));
        assert_eq!(r.take(place(1)), None);
    }

    /// With an empty stack, the backup queue takes the element, and only
    /// the first enqueue asks for the microtask.
    #[test]
    fn the_backup_queue_asks_for_one_microtask() {
        let mut r = Reactions::default();
        assert_eq!(r.enqueue(place(1), connected()), Enqueued::NeedsMicrotask);
        assert_eq!(r.enqueue(place(2), connected()), Enqueued::Done);
        assert_eq!(r.take_backup(), vec![place(1), place(2)]);
        assert_eq!(r.enqueue(place(3), connected()), Enqueued::Done);
        r.backup_done();
        assert_eq!(r.enqueue(place(4), connected()), Enqueued::NeedsMicrotask);
    }

    /// An element queued twice runs all its reactions when first reached:
    /// `take` drains the element's queue in order.
    #[test]
    fn an_element_queue_groups_the_reactions_of_an_element() {
        let mut r = Reactions::default();
        r.push();
        let attribute = Reaction::Callback(
            Callback::AttributeChanged,
            Args::Attribute {
                local: LocalName::from("a"),
                old: None,
                new: Some("1".to_owned()),
                namespace: None,
            },
        );
        r.enqueue(place(1), Reaction::Upgrade(1));
        r.enqueue(place(2), connected());
        r.enqueue(place(1), attribute.clone());
        r.enqueue(place(1), connected());
        let queue = r.pop();
        assert_eq!(queue, vec![place(1), place(2), place(1), place(1)]);
        assert_eq!(r.take(place(1)), Some(Reaction::Upgrade(1)));
        assert_eq!(r.take(place(1)), Some(attribute));
        assert_eq!(r.take(place(1)), Some(connected()));
        assert_eq!(r.take(place(1)), None);
        r.clear(place(2));
        assert_eq!(r.take(place(2)), None);
    }

    /// The depth counter stops at the limit and comes back.
    #[test]
    fn the_nesting_limit_is_43() {
        let mut r = Reactions::default();
        for _ in 0..MAX_DEPTH {
            assert!(r.enter());
        }
        assert!(!r.enter());
        r.leave();
        assert!(r.enter());
        assert!(!r.enter());
    }

    /// The budget counts the queued entries and the argument bytes, drops
    /// past it, and gives the room back when the reactions are taken.
    #[test]
    fn the_budget_counts_entries_and_bytes() {
        let mut r = Reactions::default();
        r.push();
        let big = "x".repeat(MAX_QUEUED_BYTES / 2);
        let attribute = |value: &str| {
            Reaction::Callback(
                Callback::AttributeChanged,
                Args::Attribute {
                    local: LocalName::from("a"),
                    old: None,
                    new: Some(value.to_owned()),
                    namespace: None,
                },
            )
        };
        assert_eq!(r.enqueue(place(1), attribute(&big)), Enqueued::Done);
        assert_eq!(r.queued(), (2, big.len() + 1));
        assert_eq!(r.enqueue(place(1), attribute(&big)), Enqueued::Dropped);
        assert_eq!(r.enqueue(place(1), connected()), Enqueued::Done);
        assert_eq!(r.queued(), (4, big.len() + 1));
        assert_eq!(r.pop().len(), 2);
        assert_eq!(r.queued(), (2, big.len() + 1));
        r.clear(place(1));
        assert_eq!(r.queued(), (0, 0));
        // The entry count.
        r.push();
        let mut dropped = 0;
        for _ in 0..=MAX_QUEUED / 2 {
            if r.enqueue(place(2), Reaction::Upgrade(1)) == Enqueued::Dropped {
                dropped += 1;
            }
        }
        assert_eq!(dropped, 1);
        assert_eq!(r.queued().0, MAX_QUEUED);
    }
}
