//! Promise objects and their abstract operations (ECMA-262 §27.2.1,
//! §27.2.2, §27.2.4.7.1, §27.2.5.4.1,
//! <https://tc39.es/ecma262/#sec-promise-abstract-operations>).
//!
//! A promise is an object of kind [`ObjectKind::Promise`]: its state, its
//! result, its reactions and `[[PromiseIsHandled]]`. `PerformPromiseThen`
//! adds the fulfill and the reject reaction together, so the state keeps
//! one list of [`Reaction`] pairs that share the capability.
//!
//! A reaction's handler is `empty`, a JavaScript function, or an internal
//! continuation ([`Handler`]): a Rust function with a value, which a job
//! calls with the settlement. `await` (M7 7c) and async generators (7d)
//! attach continuations, so they add no function objects and the promise
//! code does not change for them.
//!
//! Capabilities: `NewPromiseCapability(%Promise%)` whose resolving
//! functions no script can reach (the result of `then`, the promise of
//! `Promise.resolve`) is [`Capability::Intrinsic`], the promise alone: the
//! engine resolves it directly, without function objects. Its
//! `[[AlreadyResolved]]` is not needed, because the engine resolves such a
//! promise at most once (one of the two reactions of one `then` runs once).
//! Every other capability (a subclass, the result of the combinators,
//! whose functions are passed to `then`) is a [`Capability::Record`] with
//! real functions.
//!
//! Errors: a completion that the specification captures (the executor,
//! a handler, the `Get` of `then`) captures only thrown values; a
//! termination or an engine error passes through and never becomes a
//! rejection (ADR 0026 section 5).

use crate::heap::{Gc, Tracer};
use crate::object::{Object, ObjectKind, Property};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::{Value, ValueCell};
use crate::vm::jobs::Job;
use crate::vm::{Intrinsic, NativeCall, NativeReturn, VmError, VmResult, WellKnown};

/// `[[PromiseState]]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromiseStatus {
    /// Not settled yet.
    Pending,
    /// Fulfilled with its result.
    Fulfilled,
    /// Rejected with its result.
    Rejected,
}

/// The payload of a promise object (§27.2.6, the internal slots).
pub struct PromiseState {
    /// `[[PromiseState]]`.
    pub(crate) status: PromiseStatus,
    /// `[[PromiseResult]]` (`undefined` while pending).
    pub(crate) result: Value,
    /// `[[PromiseFulfillReactions]]` and `[[PromiseRejectReactions]]`,
    /// as pairs; empty once settled.
    pub(crate) reactions: Reactions,
    /// `[[PromiseIsHandled]]`.
    pub(crate) is_handled: bool,
}

impl PromiseState {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.value(self.result);
        match &self.reactions {
            Reactions::None => {}
            Reactions::One(reaction) => reaction.trace(tracer),
            Reactions::Many(reactions) => {
                for reaction in reactions {
                    reaction.trace(tracer);
                }
            }
        }
    }

    pub(crate) fn heap_size(&self) -> usize {
        let many = match &self.reactions {
            Reactions::Many(reactions) => reactions.capacity() * size_of::<Reaction>(),
            Reactions::None | Reactions::One(_) => 0,
        };
        size_of::<PromiseState>() + many
    }
}

/// The reaction pairs of a pending promise. Most promises get one (the
/// `then` of a chain, an `await`), which needs no vector.
#[derive(Default)]
pub(crate) enum Reactions {
    #[default]
    None,
    One(Reaction),
    Many(Vec<Reaction>),
}

/// `[[Type]]` of a `PromiseReaction` Record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReactionKind {
    /// A fulfill reaction.
    Fulfill,
    /// A reject reaction.
    Reject,
}

/// The function of an internal continuation: called with its data, the
/// kind of the settlement and the value. Its result resolves the
/// reaction's capability (if any); a thrown value rejects it.
pub type ContinuationFn = fn(&mut Runtime, Value, ReactionKind, Value) -> VmResult<Value>;

/// An internal continuation: a handler that is not a function object.
#[derive(Clone, Copy)]
pub struct Continuation {
    pub(crate) func: ContinuationFn,
    /// The value it works on (for `await`: the suspended function's
    /// object); traced with the reaction.
    pub(crate) data: Value,
}

/// `[[Handler]]` of a `PromiseReaction` Record.
#[derive(Clone, Copy)]
pub(crate) enum Handler {
    /// `empty`: the value passes to the capability (fulfill) or rejects it.
    Empty,
    /// A callable value (the `JobCallback` of `HostMakeJobCallback`, which is
    /// the function itself).
    Function(Value),
    /// An internal continuation.
    Internal(Continuation),
}

impl Handler {
    fn trace(self, tracer: &mut Tracer<'_>) {
        match self {
            Handler::Empty => {}
            Handler::Function(function) => tracer.value(function),
            Handler::Internal(continuation) => tracer.value(continuation.data),
        }
    }

    /// The values that keep the handler alive.
    pub(crate) fn value(self) -> Value {
        match self {
            Handler::Empty => Value::Undefined,
            Handler::Function(function) => function,
            Handler::Internal(continuation) => continuation.data,
        }
    }
}

/// A `PromiseCapability` Record (§27.2.1.1).
#[derive(Clone)]
pub(crate) enum Capability {
    /// `NewPromiseCapability(%Promise%)` whose resolving functions no
    /// script can reach: the promise (see the module documentation).
    Intrinsic(Gc<Object>),
    /// `{ [[Promise]], [[Resolve]], [[Reject]] }`.
    Record(Box<CapabilityRecord>),
}

/// The fields of a general capability.
#[derive(Clone, Copy)]
pub(crate) struct CapabilityRecord {
    pub(crate) promise: Value,
    pub(crate) resolve: Value,
    pub(crate) reject: Value,
}

impl Capability {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        match self {
            Capability::Intrinsic(promise) => tracer.object(*promise),
            Capability::Record(record) => {
                tracer.value(record.promise);
                tracer.value(record.resolve);
                tracer.value(record.reject);
            }
        }
    }

    /// `[[Promise]]`.
    pub(crate) fn promise(&self) -> Value {
        match self {
            Capability::Intrinsic(promise) => Value::Object(*promise),
            Capability::Record(record) => record.promise,
        }
    }

    /// The values that keep the capability alive.
    pub(crate) fn values(&self) -> [Value; 3] {
        match self {
            Capability::Intrinsic(promise) => {
                [Value::Object(*promise), Value::Undefined, Value::Undefined]
            }
            Capability::Record(record) => [record.promise, record.resolve, record.reject],
        }
    }
}

/// A pair of `PromiseReaction` Records that `PerformPromiseThen` adds: the
/// fulfill and the reject reaction with their shared capability.
pub(crate) struct Reaction {
    pub(crate) capability: Option<Capability>,
    pub(crate) on_fulfilled: Handler,
    pub(crate) on_rejected: Handler,
}

impl Reaction {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        if let Some(capability) = &self.capability {
            capability.trace(tracer);
        }
        self.on_fulfilled.trace(tracer);
        self.on_rejected.trace(tracer);
    }
}

/// V8's message for a capability whose functions are not callable
/// (`NewPromiseCapability` steps 6 and 7).
const NOT_CALLABLE: &str = "Promise resolve or reject function is not callable";

/// The slots of the resolving functions: the promise and the shared
/// `[[AlreadyResolved]]` record (a cell holding a Boolean).
const SLOT_PROMISE: usize = 0;
const SLOT_ALREADY_RESOLVED: usize = 1;

impl Runtime {
    // --- Creation and inspection ---

    /// The promise object of `value` (`IsPromise`, §27.2.1.6).
    pub(crate) fn as_promise(&self, value: Value) -> VmResult<Option<Gc<Object>>> {
        match value {
            Value::Object(object)
                if matches!(self.heap.object(object)?.kind, ObjectKind::Promise(_)) =>
            {
                Ok(Some(object))
            }
            _ => Ok(None),
        }
    }

    /// A new pending promise with the prototype `proto`, recorded in the
    /// current handle scope.
    pub(crate) fn new_promise_with(&mut self, proto: Gc<Object>) -> VmResult<Gc<Object>> {
        let state = PromiseState {
            status: PromiseStatus::Pending,
            result: Value::Undefined,
            reactions: Reactions::None,
            is_handled: false,
        };
        let promise = self
            .heap
            .new_object_with_kind(Some(proto), ObjectKind::Promise(Box::new(state)))?;
        Ok(self.heap.record(promise))
    }

    /// A new pending promise with `%Promise.prototype%` of `realm`.
    pub(crate) fn new_promise(&mut self, realm: u32) -> VmResult<Gc<Object>> {
        let proto = self.intrinsic(realm, Intrinsic::PromisePrototype)?;
        self.new_promise_with(proto)
    }

    pub(crate) fn promise_state(&self, promise: Gc<Object>) -> VmResult<&PromiseState> {
        match &self.heap.object(promise)?.kind {
            ObjectKind::Promise(state) => Ok(state),
            _ => Err(VmError::invariant("a promise operation on another object")),
        }
    }

    fn promise_state_mut(&mut self, promise: Gc<Object>) -> VmResult<&mut PromiseState> {
        match &mut self.heap.object_mut(promise)?.kind {
            ObjectKind::Promise(state) => Ok(state),
            _ => Err(VmError::invariant("a promise operation on another object")),
        }
    }

    // --- Completions ---

    /// The value of a thrown error: a `Raise` becomes an error object of
    /// the current realm. Terminations and engine errors stay errors.
    pub(crate) fn thrown_value(&mut self, error: VmError) -> VmResult<Value> {
        match error {
            VmError::Throw(value) => Ok(value),
            VmError::Raise { kind, message } => Ok(self.error_object(kind, &message)?.into()),
            other => Err(other),
        }
    }

    /// `Completion(...)` of an operation: `Ok(Ok(value))` for a normal
    /// completion, `Ok(Err(value))` for a throw completion (recorded in the
    /// current handle scope), `Err` for a termination or an engine error.
    pub(crate) fn completion(&mut self, result: VmResult<Value>) -> VmResult<Result<Value, Value>> {
        match result {
            Ok(value) => Ok(Ok(value)),
            Err(error) => {
                let value = self.thrown_value(error)?;
                Ok(Err(self.heap.record(value)))
            }
        }
    }

    // --- Resolving functions (§27.2.1.3) ---

    /// `CreateResolvingFunctions(promise)` (§27.2.1.3): two built-in
    /// functions of `realm` that share an `[[AlreadyResolved]]` cell. Both
    /// are recorded in the current handle scope.
    pub(crate) fn create_resolving_functions(
        &mut self,
        promise: Gc<Object>,
        realm: u32,
    ) -> VmResult<(Gc<Object>, Gc<Object>)> {
        let cell = self.heap.alloc_cell(Value::Bool(false))?;
        self.heap.record(cell);
        let slots = || Box::new([Value::Object(promise), Value::Cell(cell)]) as Box<[Value]>;
        let resolve = self.new_builtin_with_slots(realm, 1, promise_resolve_function, slots())?;
        let reject = self.new_builtin_with_slots(realm, 1, promise_reject_function, slots())?;
        Ok((resolve, reject))
    }

    /// The promise of a resolving function, after the check and update of
    /// its `[[AlreadyResolved]]`; `None` when it was resolved already.
    fn claim_resolving_function(&mut self, function: Gc<Object>) -> VmResult<Option<Gc<Object>>> {
        let promise = self.native_slot(function, SLOT_PROMISE)?;
        let Value::Cell(cell) = self.native_slot(function, SLOT_ALREADY_RESOLVED)? else {
            return Err(VmError::invariant(
                "a resolving function without its record",
            ));
        };
        let record: &mut ValueCell = self.heap.cell_mut(cell)?;
        if record.value == Value::Bool(true) {
            return Ok(None);
        }
        record.value = Value::Bool(true);
        match promise {
            Value::Object(promise) => Ok(Some(promise)),
            _ => Err(VmError::invariant(
                "a resolving function without its promise",
            )),
        }
    }

    /// Calls the reject function `reject` of a pair of resolving functions
    /// (§27.2.1.3.1) without a call.
    pub(crate) fn reject_with_function(
        &mut self,
        reject: Gc<Object>,
        reason: Value,
    ) -> VmResult<()> {
        if let Some(promise) = self.claim_resolving_function(reject)? {
            self.reject_promise(promise, reason)?;
        }
        Ok(())
    }

    /// The steps of a promise resolve function after `[[AlreadyResolved]]`
    /// (§27.2.1.3.2 steps 7 to 16): self-resolution, a non-object, the
    /// `Get` of `then`, a thenable (a job), else fulfillment. `promise`
    /// and `resolution` must be rooted by the caller.
    pub(crate) fn resolve_promise(
        &mut self,
        promise: Gc<Object>,
        resolution: Value,
    ) -> VmResult<()> {
        if resolution == Value::Object(promise) {
            let message = format!(
                "Chaining cycle detected for promise {}",
                self.object_text(promise)
            );
            let error = self.error_object(crate::error::ThrowKind::TypeError, &message)?;
            self.heap.record(error);
            return self.reject_promise(promise, error.into());
        }
        let Value::Object(thenable) = resolution else {
            return self.fulfill_promise(promise, resolution);
        };
        let key = PropertyKey::String(self.vm.atoms.then);
        let then = self.get_value(thenable.into(), key);
        let then = match self.completion(then)? {
            Ok(then) => then,
            Err(reason) => return self.reject_promise(promise, reason),
        };
        if !self.is_callable(then)? {
            return self.fulfill_promise(promise, resolution);
        }
        let realm = self.job_realm(then);
        self.enqueue_job(
            Job::ResolveThenable {
                promise,
                thenable: resolution,
                then,
            },
            realm,
        );
        Ok(())
    }

    /// `FulfillPromise(promise, value)` (§27.2.1.4).
    pub(crate) fn fulfill_promise(&mut self, promise: Gc<Object>, value: Value) -> VmResult<()> {
        self.settle(promise, PromiseStatus::Fulfilled, value)
    }

    /// `RejectPromise(promise, reason)` (§27.2.1.7), with
    /// `HostPromiseRejectionTracker(promise, "reject")` when no handler is
    /// attached.
    pub(crate) fn reject_promise(&mut self, promise: Gc<Object>, reason: Value) -> VmResult<()> {
        if !self.promise_state(promise)?.is_handled {
            self.track_rejection(promise);
        }
        self.settle(promise, PromiseStatus::Rejected, reason)
    }

    /// Settles a pending promise and triggers its reactions
    /// (§27.2.1.8 `TriggerPromiseReactions`): one job per reaction, in the
    /// order in which they were added. A promise that is not pending is
    /// left alone (an engine invariant: only the paths that checked
    /// `[[AlreadyResolved]]` get here).
    fn settle(&mut self, promise: Gc<Object>, status: PromiseStatus, value: Value) -> VmResult<()> {
        let state = self.promise_state_mut(promise)?;
        if state.status != PromiseStatus::Pending {
            return Err(VmError::invariant("a settled promise was settled again"));
        }
        state.status = status;
        state.result = value;
        let reactions = std::mem::take(&mut state.reactions);
        let kind = match status {
            PromiseStatus::Rejected => ReactionKind::Reject,
            _ => ReactionKind::Fulfill,
        };
        match reactions {
            Reactions::None => {}
            Reactions::One(reaction) => self.enqueue_reaction(reaction, kind, value),
            Reactions::Many(reactions) => {
                for reaction in reactions {
                    self.enqueue_reaction(reaction, kind, value);
                }
            }
        }
        Ok(())
    }

    /// `NewPromiseReactionJob(reaction, argument)` and
    /// `HostEnqueuePromiseJob` (§27.2.2.1) for the reaction of `kind` of a
    /// pair.
    fn enqueue_reaction(&mut self, reaction: Reaction, kind: ReactionKind, argument: Value) {
        let handler = match kind {
            ReactionKind::Fulfill => reaction.on_fulfilled,
            ReactionKind::Reject => reaction.on_rejected,
        };
        // The realm of the handler's function (steps 2 and 3); `None` for
        // an empty handler or a continuation.
        let realm = match handler {
            Handler::Function(function) => self.job_realm(function),
            Handler::Empty | Handler::Internal(_) => None,
        };
        self.enqueue_job(
            Job::Reaction {
                capability: reaction.capability,
                handler,
                kind,
                argument,
            },
            realm,
        );
    }

    /// The realm of a job's function (`GetFunctionRealm`).
    fn job_realm(&self, function: Value) -> Option<u32> {
        match function {
            Value::Object(function) => self.function_realm(function),
            _ => None,
        }
    }

    // --- PerformPromiseThen (§27.2.5.4.1) ---

    /// The handler of a `then` argument: a callable value, else `empty`
    /// (steps 3 to 6).
    pub(crate) fn handler_of(&self, value: Value) -> VmResult<Handler> {
        Ok(if self.is_callable(value)? {
            Handler::Function(value)
        } else {
            Handler::Empty
        })
    }

    /// `PerformPromiseThen(promise, onFulfilled, onRejected,
    /// resultCapability)` (§27.2.5.4.1). Never runs script code.
    pub(crate) fn perform_promise_then(
        &mut self,
        promise: Gc<Object>,
        on_fulfilled: Handler,
        on_rejected: Handler,
        capability: Option<Capability>,
    ) -> VmResult<()> {
        let reaction = Reaction {
            capability,
            on_fulfilled,
            on_rejected,
        };
        let state = self.promise_state_mut(promise)?;
        let (status, result) = (state.status, state.result);
        state.is_handled = true;
        match status {
            PromiseStatus::Pending => {
                let grown = match &mut state.reactions {
                    reactions @ Reactions::None => {
                        *reactions = Reactions::One(reaction);
                        0
                    }
                    reactions @ Reactions::One(_) => {
                        let mut many = Vec::new();
                        many.try_reserve_exact(4)
                            .map_err(crate::error::Error::from)?;
                        if let Reactions::One(first) = std::mem::take(reactions) {
                            many.push(first);
                        }
                        many.push(reaction);
                        *reactions = Reactions::Many(many);
                        4
                    }
                    Reactions::Many(many) => {
                        let before = many.capacity();
                        many.try_reserve(1).map_err(crate::error::Error::from)?;
                        many.push(reaction);
                        many.capacity() - before
                    }
                };
                self.heap.charge(grown * size_of::<Reaction>());
            }
            PromiseStatus::Fulfilled => {
                self.enqueue_reaction(reaction, ReactionKind::Fulfill, result);
            }
            PromiseStatus::Rejected => {
                // `HostPromiseRejectionTracker(promise, "handle")`: the
                // tracker drops a promise that is handled by now when the
                // host takes the list (`jobs.rs`).
                self.enqueue_reaction(reaction, ReactionKind::Reject, result);
            }
        }
        Ok(())
    }

    /// Attaches an internal continuation to a promise (for `await`, M7 7c,
    /// and the host's reactions): `PerformPromiseThen(promise, onFulfilled,
    /// onRejected)` without a result capability, both handlers calling
    /// `func` with `data`.
    pub(crate) fn then_internal(
        &mut self,
        promise: Gc<Object>,
        func: ContinuationFn,
        data: Value,
    ) -> VmResult<()> {
        let handler = Handler::Internal(Continuation { func, data });
        self.perform_promise_then(promise, handler, handler, None)
    }

    // --- Capabilities ---

    /// `NewPromiseCapability(C)` (§27.2.1.5). With `exposed` false the
    /// caller promises that the resolving functions never reach a script,
    /// so `%Promise%` of `realm` gives a [`Capability::Intrinsic`] (no
    /// executor, no functions: `Construct(%Promise%, ...)` is not
    /// observable). All values are recorded in the current handle scope.
    pub(crate) fn new_promise_capability(
        &mut self,
        constructor: Value,
        realm: u32,
        exposed: bool,
    ) -> VmResult<Capability> {
        let intrinsic = self.intrinsic(realm, Intrinsic::PromiseConstructor)?;
        if constructor == Value::Object(intrinsic) {
            let promise = self.new_promise(realm)?;
            if !exposed {
                return Ok(Capability::Intrinsic(promise));
            }
            let (resolve, reject) = self.create_resolving_functions(promise, realm)?;
            return Ok(Capability::Record(Box::new(CapabilityRecord {
                promise: promise.into(),
                resolve: resolve.into(),
                reject: reject.into(),
            })));
        }
        if !self.is_constructor(constructor)? {
            return Err(VmError::type_error(format!(
                "{} is not a constructor",
                self.primitive_text(constructor)
            )));
        }
        let slots = Box::new([Value::Undefined, Value::Undefined]) as Box<[Value]>;
        let executor = self.new_builtin_with_slots(realm, 2, capability_executor, slots)?;
        let promise = self.construct(constructor, &[executor.into()])?;
        let resolve = self.native_slot(executor, 0)?;
        let reject = self.native_slot(executor, 1)?;
        if !self.is_callable(resolve)? || !self.is_callable(reject)? {
            return Err(VmError::type_error(NOT_CALLABLE));
        }
        Ok(Capability::Record(Box::new(CapabilityRecord {
            promise,
            resolve,
            reject,
        })))
    }

    /// `Call(capability.[[Resolve]], undefined, « value »)`.
    pub(crate) fn capability_resolve(
        &mut self,
        capability: &Capability,
        value: Value,
    ) -> VmResult<()> {
        match capability {
            Capability::Intrinsic(promise) => {
                if self.promise_state(*promise)?.status != PromiseStatus::Pending {
                    return Err(VmError::invariant("an intrinsic capability resolved twice"));
                }
                self.resolve_promise(*promise, value)
            }
            Capability::Record(record) => {
                self.call(record.resolve, Value::Undefined, &[value])?;
                Ok(())
            }
        }
    }

    /// `Call(capability.[[Reject]], undefined, « reason »)`.
    pub(crate) fn capability_reject(
        &mut self,
        capability: &Capability,
        reason: Value,
    ) -> VmResult<()> {
        match capability {
            Capability::Intrinsic(promise) => {
                if self.promise_state(*promise)?.status != PromiseStatus::Pending {
                    return Err(VmError::invariant("an intrinsic capability rejected twice"));
                }
                self.reject_promise(*promise, reason)
            }
            Capability::Record(record) => {
                self.call(record.reject, Value::Undefined, &[reason])?;
                Ok(())
            }
        }
    }

    /// `IfAbruptRejectPromise(result, capability)` (§27.2.1.1.1): a throw
    /// completion rejects the capability and gives its promise.
    pub(crate) fn reject_if_abrupt(
        &mut self,
        result: VmResult<Value>,
        capability: &Capability,
    ) -> VmResult<Result<Value, Value>> {
        match self.completion(result)? {
            Ok(value) => Ok(Ok(value)),
            Err(reason) => {
                self.capability_reject(capability, reason)?;
                Ok(Err(capability.promise()))
            }
        }
    }

    /// `PromiseResolve(C, x)` (§27.2.4.7.1). The result is recorded in the
    /// current handle scope.
    pub(crate) fn promise_resolve(
        &mut self,
        constructor: Gc<Object>,
        value: Value,
        realm: u32,
    ) -> VmResult<Value> {
        if self.as_promise(value)?.is_some() {
            let key = PropertyKey::String(self.vm.atoms.constructor);
            let value_constructor = self.get_value(value, key)?;
            if value_constructor == Value::Object(constructor) {
                return Ok(value);
            }
        }
        let capability = self.new_promise_capability(constructor.into(), realm, false)?;
        self.capability_resolve(&capability, value)?;
        Ok(capability.promise())
    }

    /// `SpeciesConstructor(promise, %Promise%)` (§7.3.22) with V8's
    /// messages. The default `constructor` and `@@species` of `realm` give
    /// `%Promise%` without a call. The result is recorded.
    pub(crate) fn promise_species_constructor(
        &mut self,
        object: Gc<Object>,
        realm: u32,
    ) -> VmResult<Value> {
        let default = self.intrinsic(realm, Intrinsic::PromiseConstructor)?;
        let key = PropertyKey::String(self.vm.atoms.constructor);
        // The common case reads data properties only: `constructor` from
        // the prototype and the built-in `@@species` getter, which would
        // return its receiver.
        if let crate::object::GetResult::Value(value) = self.heap.get(object, key, object.into())?
            && value == Value::Object(default)
            && self.has_builtin_species(default, realm)?
        {
            return Ok(value);
        }
        let constructor = self.get_value(object.into(), key)?;
        let Value::Object(constructor) = constructor else {
            if constructor.is_undefined() {
                return Ok(default.into());
            }
            return Err(VmError::type_error(
                "The .constructor property is not an object",
            ));
        };
        if constructor == default && self.has_builtin_species(default, realm)? {
            return Ok(default.into());
        }
        let species_key = self.symbol_key(WellKnown::Species);
        let species = self.get_value(constructor.into(), species_key)?;
        if species.is_nullish() {
            return Ok(default.into());
        }
        if self.is_constructor(species)? {
            return Ok(species);
        }
        Err(VmError::type_error(
            "object.constructor[Symbol.species] is not a constructor",
        ))
    }

    /// Whether `%Promise%` of `realm` still has the built-in `@@species`
    /// getter (which returns its receiver).
    fn has_builtin_species(&self, promise: Gc<Object>, realm: u32) -> VmResult<bool> {
        let key = self.symbol_key(WellKnown::Species);
        Ok(match self.heap.get_own_property(promise, key)? {
            Some(Property::Accessor { get, .. }) => {
                get == Value::Object(self.intrinsic(realm, Intrinsic::PromiseSpeciesGetter)?)
            }
            _ => false,
        })
    }

    // --- Built-in functions with slots ---

    /// Slot `index` of a native function.
    pub(crate) fn native_slot(&self, function: Gc<Object>, index: usize) -> VmResult<Value> {
        match &self.heap.object(function)?.kind {
            ObjectKind::Native(native) => native
                .slots
                .get(index)
                .copied()
                .ok_or(VmError::invariant("a native slot outside the slots")),
            _ => Err(VmError::invariant("native slots of another object")),
        }
    }

    /// Writes slot `index` of a native function.
    pub(crate) fn set_native_slot(
        &mut self,
        function: Gc<Object>,
        index: usize,
        value: Value,
    ) -> VmResult<()> {
        match &mut self.heap.object_mut(function)?.kind {
            ObjectKind::Native(native) => {
                let slot = native
                    .slots
                    .get_mut(index)
                    .ok_or(VmError::invariant("a native slot outside the slots"))?;
                *slot = value;
                Ok(())
            }
            _ => Err(VmError::invariant("native slots of another object")),
        }
    }
}

/// A promise resolve function (§27.2.1.3.2).
pub(crate) fn promise_resolve_function(
    rt: &mut Runtime,
    call: &NativeCall,
) -> VmResult<NativeReturn> {
    if let Some(promise) = rt.claim_resolving_function(call.callee())? {
        let resolution = rt.arg(call, 0);
        rt.resolve_promise(promise, resolution)?;
    }
    Ok(NativeReturn::Value(Value::Undefined))
}

/// A promise reject function (§27.2.1.3.1).
pub(crate) fn promise_reject_function(
    rt: &mut Runtime,
    call: &NativeCall,
) -> VmResult<NativeReturn> {
    if let Some(promise) = rt.claim_resolving_function(call.callee())? {
        let reason = rt.arg(call, 0);
        rt.reject_promise(promise, reason)?;
    }
    Ok(NativeReturn::Value(Value::Undefined))
}

/// The executor of `NewPromiseCapability` (§27.2.1.5 step 3): stores the
/// resolving functions in its slots, once (V8's message).
fn capability_executor(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let executor = call.callee();
    if !rt.native_slot(executor, 0)?.is_undefined() || !rt.native_slot(executor, 1)?.is_undefined()
    {
        return Err(VmError::type_error(
            "Promise executor has already been invoked with non-undefined arguments",
        ));
    }
    let (resolve, reject) = (rt.arg(call, 0), rt.arg(call, 1));
    rt.set_native_slot(executor, 0, resolve)?;
    rt.set_native_slot(executor, 1, reject)?;
    Ok(NativeReturn::Value(Value::Undefined))
}

/// The cell in a slot that holds a record which two functions share.
pub(crate) fn cell_of(value: Value) -> VmResult<Gc<ValueCell>> {
    match value {
        Value::Cell(cell) => Ok(cell),
        _ => Err(VmError::invariant("a record slot without its cell")),
    }
}
