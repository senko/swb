//! Evaluation (§16.2.1.6.1.3 `Evaluate`,
//! <https://tc39.es/ecma262/2025/#sec-moduleevaluation>, with
//! `InnerModuleEvaluation`, `ExecuteAsyncModule`,
//! `GatherAvailableAncestors`, `AsyncModuleExecutionFulfilled` and
//! `AsyncModuleExecutionRejected`, §16.2.1.6.1.3.1 to §16.2.1.6.1.3.5;
//! ADR 0034 sections 1, 3 and 6).
//!
//! `InnerModuleEvaluation` is Tarjan's DFS as a loop with an explicit
//! stack; each module body is resumed from that loop (its suspended frame
//! moves back on the stacks and runs in a fresh interpreter loop), so a
//! chain of modules costs no Rust stack per module. An error of a body
//! becomes `[[EvaluationError]]` of every module on the DFS stack, and a
//! later Evaluate of any of them gives the same error.
//!
//! Top-level `await` (ADR 0034 section 3): a module with `[[HasTLA]]` runs
//! its body as an async function. `ExecuteAsyncModule` gives its suspended
//! generator object the kind [`GeneratorKind::AsyncModule`], makes a new
//! promise (the capability, kept in the record), attaches an internal
//! reaction to that promise
//! and resumes the body; `await` suspends it as in an async function, the
//! end of the body resolves the promise and the handler over the body
//! rejects it. The reaction runs `AsyncModuleExecutionFulfilled` or
//! `AsyncModuleExecutionRejected` in a job. The walks of the async parents
//! (`GatherAvailableAncestors`, the rejection) are loops with explicit
//! stacks, as the DFS, and every loop charges the time countdown.
//!
//! Termination (ADR 0034 section 6): the modules whose evaluation a
//! termination ended are `evaluated` with [`EvaluationError::Terminated`]:
//! they never run again and report nothing more, and the promises that
//! wait for them stay pending. These are the modules on the DFS stack
//! (in Evaluate, also in the first run of an async body), a module whose
//! body a job resumed after an `await`, and the module that
//! `AsyncModuleExecutionFulfilled` was running with the gathered modules
//! that had not run yet.

use std::collections::HashSet;

use crate::error::Termination;
use crate::heap::{Gc, Generic, Root};
use crate::object::Object;
use crate::runtime::{Runtime, ScriptError};
use crate::value::Value;
use crate::vm::generator::GeneratorKind;
use crate::vm::{ReactionKind, VmError, VmResult};

use super::{AsyncOrder, EvaluationError, Module, ModuleRecord, Status};

/// A module whose requests the evaluation loop visits.
struct Visit {
    module: Gc<Generic>,
    next: u32,
}

/// How `InnerModuleEvaluation` ended abruptly: an error now (of a body, or
/// of the engine), or the cached error of a module evaluated before.
enum Abrupt {
    Fresh(VmError),
    Cached(EvaluationError),
}

impl From<VmError> for Abrupt {
    fn from(error: VmError) -> Self {
        Abrupt::Fresh(error)
    }
}

/// What the start of `InnerModuleEvaluation` found.
enum Entry {
    /// The module starts (it is on the stacks now).
    Entered,
    /// Evaluated (or being evaluated) without an error.
    Done,
    /// Evaluated with an error.
    Failed(EvaluationError),
}

/// A module of the rejection walk whose async parents are being visited.
struct Rejecting {
    module: Gc<Generic>,
    next: usize,
}

/// The termination reason of an error that ended a module's evaluation
/// without a thrown value (an engine error counts as a host request).
fn termination_of(error: &VmError) -> Termination {
    match error {
        VmError::Terminated(reason) => *reason,
        _ => Termination::HostRequest,
    }
}

impl Runtime {
    /// `Evaluate()` (§16.2.1.6.1.3) of a linked module: runs the bodies of
    /// the graph that have not run. Returns the promise of the evaluation
    /// (`[[TopLevelCapability]]` of the module's cycle root). Without
    /// top-level `await` in the graph it is settled when the call returns:
    /// fulfilled with `undefined`, or rejected with the error of a body
    /// (also an error of an earlier evaluation of a module of the graph).
    /// With top-level `await` it can be pending: it settles in a later job
    /// of [`Runtime::run_jobs`], when the last async body of the graph
    /// ends; the host attaches its reaction with
    /// [`Runtime::react_to_promise`]. The promise counts as handled (the
    /// host reports the error). A termination is
    /// [`ScriptError::Terminated`]; the modules that were running are
    /// marked, and a later Evaluate of them gives a promise that never
    /// settles. The same module gives the same promise each time.
    pub fn evaluate(&mut self, module: Module) -> Result<Value, ScriptError> {
        self.vm.error_offset = None;
        let start = self.run_start();
        let scope = self.heap.open_scope();
        self.heap.record(Root::Generic(module.0));
        let result = self.evaluate_promise(module.0).map(Value::Object);
        if result.is_err() {
            self.restore_run_start(&start);
        } else if let Err(error) = self.heap.close_scope(scope) {
            return Err(crate::runtime::finish_error(error.into()));
        }
        self.finish(result)
    }

    /// Evaluate with its promise.
    fn evaluate_promise(&mut self, module: Gc<Generic>) -> VmResult<Gc<Object>> {
        let record = self.record(module)?;
        let module = match record.status {
            Status::Linked => module,
            // A module evaluated with an error before it left the DFS
            // stack has no cycle root: it stands for itself.
            Status::Evaluated | Status::EvaluatingAsync => record.cycle_root.unwrap_or(module),
            _ => {
                return Err(VmError::invariant(
                    "an evaluation of a module that is not linked",
                ));
            }
        };
        let record = self.record(module)?;
        if let Some(promise) = record.top_level_promise {
            return Ok(promise);
        }
        let realm = record.realm;
        let promise = self.new_promise(realm)?;
        self.heap.record(promise);
        self.set_promise_handled(promise)?;
        self.record_mut(module)?.top_level_promise = Some(promise);
        match self.evaluate_graph(module)? {
            // Evaluated now; an `evaluating-async` module settles the
            // promise when its async evaluation ends.
            None if self.record(module)?.status == Status::Evaluated => {
                self.resolve_promise(promise, Value::Undefined)?;
            }
            None | Some(EvaluationError::Terminated(_)) => {}
            Some(EvaluationError::Thrown(error)) => self.reject_promise(promise, error)?,
        }
        Ok(promise)
    }

    /// Evaluate for a host run that reports errors as a script does
    /// ([`Runtime::eval_module_at`], a module without requests and without
    /// top-level `await`): a thrown value is the error.
    pub(crate) fn evaluate_sync(&mut self, module: Gc<Generic>) -> VmResult<()> {
        match self.evaluate_graph(module)? {
            None => Ok(()),
            Some(EvaluationError::Thrown(error)) => Err(VmError::Throw(error)),
            Some(EvaluationError::Terminated(reason)) => Err(VmError::Terminated(reason)),
        }
    }

    /// Steps 4 to 10 of Evaluate: `InnerModuleEvaluation` from `module`;
    /// on an error, every module on the stack is evaluated with that error.
    /// Returns the error (`None` without one). A termination or an engine
    /// error now goes up as the error of the run.
    fn evaluate_graph(&mut self, module: Gc<Generic>) -> VmResult<Option<EvaluationError>> {
        let mut stack = Vec::new();
        let abrupt = match self.inner_evaluation(module, &mut stack) {
            Ok(()) => return Ok(None),
            Err(abrupt) => abrupt,
        };
        let (error, fresh) = match abrupt {
            Abrupt::Cached(error) => (error, None),
            Abrupt::Fresh(VmError::Throw(value)) => (EvaluationError::Thrown(value), None),
            Abrupt::Fresh(VmError::Raise { kind, message }) => {
                let realm = self.record(module)?.realm;
                let error = self.in_realm(realm, |rt| rt.error_object(kind, &message))?;
                (EvaluationError::Thrown(error.into()), None)
            }
            Abrupt::Fresh(error @ (VmError::Terminated(_) | VmError::Internal(_))) => (
                EvaluationError::Terminated(termination_of(&error)),
                Some(error),
            ),
        };
        for member in stack {
            let record = self.record_mut(member)?;
            record.status = Status::Evaluated;
            record.evaluation_error = Some(error);
            record.async_order = AsyncOrder::Done;
            record.body = None;
        }
        match fresh {
            Some(error) => Err(error),
            None => Ok(Some(error)),
        }
    }

    /// `InnerModuleEvaluation` (§16.2.1.6.1.3.1) as a loop.
    fn inner_evaluation(
        &mut self,
        root: Gc<Generic>,
        stack: &mut Vec<Gc<Generic>>,
    ) -> Result<(), Abrupt> {
        let mut index = 0u32;
        let mut visits = Vec::new();
        match self.enter_evaluation(root, &mut index, stack, &mut visits)? {
            Entry::Entered => {}
            Entry::Done => return Ok(()),
            Entry::Failed(error) => return Err(Abrupt::Cached(error)),
        }
        while let Some(top) = visits.last_mut() {
            self.tick()?;
            let (module, request) = (top.module, top.next);
            if (request as usize) < self.record(module)?.loaded.len() {
                top.next += 1;
                let required = self.imported_module(module, request)?;
                match self.enter_evaluation(required, &mut index, stack, &mut visits)? {
                    Entry::Entered => {}
                    Entry::Done => self.after_required(module, required)?,
                    Entry::Failed(error) => return Err(Abrupt::Cached(error)),
                }
                continue;
            }
            visits.pop();
            self.run_or_wait(module)?;
            let record = self.record(module)?;
            if record.dfs_ancestor_index == record.dfs_index {
                while let Some(member) = stack.pop() {
                    let record = self.record_mut(member)?;
                    record.status = match record.async_order {
                        AsyncOrder::Unset => Status::Evaluated,
                        _ => Status::EvaluatingAsync,
                    };
                    record.cycle_root = Some(module);
                    if member == module {
                        break;
                    }
                }
            }
            if let Some(parent) = visits.last() {
                let parent = parent.module;
                self.after_required(parent, module)?;
            }
        }
        Ok(())
    }

    /// Steps 2 to 11 of `InnerModuleEvaluation`.
    fn enter_evaluation(
        &mut self,
        module: Gc<Generic>,
        index: &mut u32,
        stack: &mut Vec<Gc<Generic>>,
        visits: &mut Vec<Visit>,
    ) -> VmResult<Entry> {
        let record = self.record_mut(module)?;
        match record.status {
            Status::Evaluated | Status::EvaluatingAsync => {
                return Ok(match record.evaluation_error {
                    Some(error) => Entry::Failed(error),
                    None => Entry::Done,
                });
            }
            Status::Evaluating => return Ok(Entry::Done),
            Status::Linked => {}
            _ => {
                return Err(VmError::invariant(
                    "an evaluation of a module that is not linked",
                ));
            }
        }
        record.status = Status::Evaluating;
        record.dfs_index = *index;
        record.dfs_ancestor_index = *index;
        record.pending_async = 0;
        *index = index.saturating_add(1);
        stack.push(module);
        visits.push(Visit { module, next: 0 });
        Ok(Entry::Entered)
    }

    /// Step 12.c of `InnerModuleEvaluation` for a required module whose
    /// evaluation is over or in progress: one still `evaluating` lowers the
    /// ancestor index; for any other, its cycle root stands for it and
    /// passes on its error. A module that waits for async evaluation (the
    /// required one or its cycle root) gets `module` as an async parent.
    fn after_required(&mut self, module: Gc<Generic>, required: Gc<Generic>) -> Result<(), Abrupt> {
        let child = self.record(required)?;
        let required = if child.status == Status::Evaluating {
            let ancestor = child.dfs_ancestor_index;
            let record = self.record_mut(module)?;
            record.dfs_ancestor_index = record.dfs_ancestor_index.min(ancestor);
            required
        } else {
            let root = child.cycle_root.unwrap_or(required);
            if let Some(error) = self.record(root)?.evaluation_error {
                return Err(Abrupt::Cached(error));
            }
            root
        };
        if matches!(self.record(required)?.async_order, AsyncOrder::Order(_)) {
            let record = self.record_mut(module)?;
            record.pending_async = record.pending_async.saturating_add(1);
            self.heap
                .generic_resized::<ModuleRecord, _>(required, |r| r.async_parents.push(module))
                .map_err(VmError::from)?;
        }
        Ok(())
    }

    /// Steps 13 and 14 of `InnerModuleEvaluation`: a module that has
    /// top-level `await` or waits for an async dependency gets its place in
    /// the order of async evaluations and starts when it waits for nothing;
    /// any other module runs now.
    fn run_or_wait(&mut self, module: Gc<Generic>) -> VmResult<()> {
        let record = self.record(module)?;
        if record.pending_async == 0 && !record.has_tla {
            return self.execute_module(module);
        }
        let order = self.vm.modules.next_async_order();
        let record = self.record_mut(module)?;
        record.async_order = AsyncOrder::Order(order);
        if record.pending_async == 0 {
            self.execute_async_module(module)?;
        }
        Ok(())
    }

    /// `ExecuteModule()` (§16.2.1.7.3.2) of a module without top-level
    /// `await`: resumes the module function after its prologue and runs the
    /// body to its end.
    fn execute_module(&mut self, module: Gc<Generic>) -> VmResult<()> {
        let record = self.record(module)?;
        if record.has_tla {
            return Err(VmError::invariant(
                "a synchronous run of a module with top-level await",
            ));
        }
        let body = record
            .body
            .ok_or(VmError::invariant("a module body that was not prepared"))?;
        let result = self.run_suspended(body);
        self.record_mut(module)?.body = None;
        result.map(drop)
    }

    /// `ExecuteAsyncModule(module)` (§16.2.1.6.1.3.2) with `ExecuteModule(
    /// capability)` (§16.2.1.7.3.2 step 10, `AsyncBlockStart`): the
    /// capability's promise, its reactions (an internal continuation), and
    /// the run of the body up to its first `await` or its end. A throw of
    /// the body rejects the promise; only a termination or an engine error
    /// comes back (the caller marks the module: it is on the DFS stack, or
    /// in the list of `AsyncModuleExecutionFulfilled`).
    fn execute_async_module(&mut self, module: Gc<Generic>) -> VmResult<()> {
        let record = self.record(module)?;
        let realm = record.realm;
        let body = record
            .body
            .ok_or(VmError::invariant("a module body that was not prepared"))?;
        // The promise is recorded in the current handle scope; the record
        // keeps it, and its reaction roots the body object, which roots the
        // module.
        let promise = self.new_promise(realm)?;
        self.generator_state(body)?.kind = GeneratorKind::AsyncModule(module);
        self.then_internal(promise, async_module_settled, Value::Object(body))?;
        let record = self.record_mut(module)?;
        record.async_capability = Some(promise);
        record.body = None;
        self.run_suspended(body).map(drop)
    }

    /// A termination (or an engine error) ended the run of the async body
    /// in `generator`: its module is evaluated as terminated (it never runs
    /// again; its capability never settles). Nothing for other generators.
    pub(crate) fn module_body_ended(&mut self, generator: Gc<Object>, error: &VmError) {
        let Ok(state) = self.generator_state(generator) else {
            return;
        };
        if let GeneratorKind::AsyncModule(module) = state.kind {
            self.mark_terminated(module, termination_of(error));
        }
    }

    /// Marks a module whose evaluation a termination ended (it keeps an
    /// earlier error).
    fn mark_terminated(&mut self, module: Gc<Generic>, reason: Termination) {
        if let Ok(record) = self.record_mut(module)
            && record.evaluation_error.is_none()
        {
            record.status = Status::Evaluated;
            record.evaluation_error = Some(EvaluationError::Terminated(reason));
            record.async_order = AsyncOrder::Done;
            record.body = None;
        }
    }

    /// `AsyncModuleExecutionFulfilled(module)` (§16.2.1.6.1.3.4), in the job
    /// of the reaction on the capability of `module`.
    fn async_module_fulfilled(&mut self, module: Gc<Generic>) -> VmResult<()> {
        let record = self.record_mut(module)?;
        match record.status {
            // Rejected meanwhile (its error is set).
            Status::Evaluated => return Ok(()),
            Status::EvaluatingAsync => {}
            _ => {
                return Err(VmError::invariant(
                    "an async module that is not evaluating-async",
                ));
            }
        }
        record.async_order = AsyncOrder::Done;
        record.status = Status::Evaluated;
        if let Some(promise) = record.top_level_promise {
            self.resolve_promise(promise, Value::Undefined)?;
        }
        let mut ready = Vec::new();
        let gathered = self.gather_available_ancestors(module, &mut ready);
        let result = gathered.and_then(|()| self.execute_ancestors(&mut ready));
        if let Err(error) = &result {
            // The gathered modules that did not run never will.
            for &waiting in &ready {
                self.mark_terminated(waiting, termination_of(error));
            }
        }
        result
    }

    /// `GatherAvailableAncestors(module, execList)` (§16.2.1.6.1.3.3) as a
    /// loop: the async parents of `module` (and of the synchronous ones
    /// among them, transitively) whose last async dependency this was, into
    /// `ready` with their order. The set does not depend on the order of
    /// the walk.
    fn gather_available_ancestors(
        &mut self,
        module: Gc<Generic>,
        ready: &mut Vec<Gc<Generic>>,
    ) -> VmResult<()> {
        let mut listed = HashSet::new();
        let mut work = vec![module];
        while let Some(child) = work.pop() {
            let mut next = 0;
            while let Some(parent) = self.record(child)?.async_parents.get(next).copied() {
                next += 1;
                self.tick()?;
                if listed.contains(&parent) {
                    continue;
                }
                let record = self.record(parent)?;
                // The specification asserts `evaluating-async` and a count
                // above 0 here; anything else (after a termination) waits.
                if record.status != Status::EvaluatingAsync || record.pending_async == 0 {
                    continue;
                }
                let root = record.cycle_root.unwrap_or(parent);
                if self.record(root)?.evaluation_error.is_some() {
                    continue;
                }
                let record = self.record_mut(parent)?;
                record.pending_async -= 1;
                if record.pending_async == 0 {
                    let has_tla = record.has_tla;
                    listed.insert(parent);
                    ready.push(parent);
                    if !has_tla {
                        work.push(parent);
                    }
                }
            }
        }
        Ok(())
    }

    /// Steps 9 to 12 of `AsyncModuleExecutionFulfilled`: runs the gathered
    /// modules in the order of their `[[AsyncEvaluationOrder]]`. Each one
    /// that runs leaves `ready`; after an error the rest is still there.
    fn execute_ancestors(&mut self, ready: &mut Vec<Gc<Generic>>) -> VmResult<()> {
        let mut ordered = Vec::with_capacity(ready.len());
        for &module in ready.iter() {
            let order = match self.record(module)?.async_order {
                AsyncOrder::Order(order) => order,
                _ => u64::MAX,
            };
            ordered.push((order, module));
        }
        ordered.sort_by_key(|&(order, _)| order);
        // Reversed, so that the next module is at the end.
        ready.clear();
        ready.extend(ordered.into_iter().rev().map(|(_, module)| module));
        while let Some(&module) = ready.last() {
            self.tick()?;
            self.execute_ancestor(module)?;
            ready.pop();
        }
        Ok(())
    }

    /// Step 12 of `AsyncModuleExecutionFulfilled` for one module.
    fn execute_ancestor(&mut self, module: Gc<Generic>) -> VmResult<()> {
        let record = self.record(module)?;
        if record.status == Status::Evaluated {
            // Rejected by a module before it in the list.
            return Ok(());
        }
        if record.has_tla {
            return self.execute_async_module(module);
        }
        let realm = record.realm;
        match self.execute_module(module) {
            Ok(()) => {
                let record = self.record_mut(module)?;
                record.async_order = AsyncOrder::Done;
                record.status = Status::Evaluated;
                if let Some(promise) = record.top_level_promise {
                    self.resolve_promise(promise, Value::Undefined)?;
                }
                Ok(())
            }
            Err(error @ (VmError::Throw(_) | VmError::Raise { .. })) => {
                let value = self.in_realm(realm, |rt| rt.thrown_value(error))?;
                self.async_module_rejected(module, value)
            }
            Err(error) => Err(error),
        }
    }

    /// `AsyncModuleExecutionRejected(module, error)` (§16.2.1.6.1.3.5) as a
    /// loop: `module` and its async parents (transitively) are evaluated
    /// with the error; the top-level promises are rejected after those of
    /// the parents, in the order of the specification's recursion.
    fn async_module_rejected(&mut self, module: Gc<Generic>, error: Value) -> VmResult<()> {
        let mut stack = Vec::new();
        self.start_rejection(module, error, &mut stack)?;
        while let Some(top) = stack.last_mut() {
            self.tick()?;
            let (current, next) = (top.module, top.next);
            if let Some(parent) = self.record(current)?.async_parents.get(next).copied() {
                top.next += 1;
                self.start_rejection(parent, error, &mut stack)?;
                continue;
            }
            stack.pop();
            if let Some(promise) = self.record(current)?.top_level_promise {
                self.reject_promise(promise, error)?;
            }
        }
        Ok(())
    }

    /// Steps 1 to 6 of `AsyncModuleExecutionRejected`: a module that is not
    /// evaluated yet gets the error and is visited.
    fn start_rejection(
        &mut self,
        module: Gc<Generic>,
        error: Value,
        stack: &mut Vec<Rejecting>,
    ) -> VmResult<()> {
        let record = self.record_mut(module)?;
        if record.status == Status::Evaluated {
            return Ok(());
        }
        record.evaluation_error = Some(EvaluationError::Thrown(error));
        record.status = Status::Evaluated;
        record.async_order = AsyncOrder::Done;
        // A parent that waited never runs.
        record.body = None;
        stack.push(Rejecting { module, next: 0 });
        Ok(())
    }
}

/// The reaction on the capability of `ExecuteAsyncModule` (its
/// `onFulfilled` and `onRejected` closures): `data` is the module's body
/// object, whose kind names the module.
fn async_module_settled(
    rt: &mut Runtime,
    data: Value,
    kind: ReactionKind,
    value: Value,
) -> VmResult<Value> {
    let Value::Object(body) = data else {
        return Err(VmError::invariant("a module reaction without its body"));
    };
    let GeneratorKind::AsyncModule(module) = rt.generator_state(body)?.kind else {
        return Err(VmError::invariant("a module reaction without its module"));
    };
    // The body has ended: nothing needs the capability any more.
    rt.record_mut(module)?.async_capability = None;
    match kind {
        ReactionKind::Fulfill => rt.async_module_fulfilled(module)?,
        ReactionKind::Reject => rt.async_module_rejected(module, value)?,
    }
    Ok(Value::Undefined)
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{linked, module, runtime};
    use super::*;
    use crate::vm::PromiseStatus;

    #[test]
    fn a_cycle_has_one_root() {
        let mut rt = runtime(
            &[
                ("a.js", "import './b.js'; import './c.js';"),
                ("b.js", "import './a.js';"),
                ("c.js", ""),
            ],
            true,
        );
        let a = linked(&mut rt, "a.js");
        let promise = rt.evaluate(a).expect("runs");
        assert!(matches!(
            rt.promise_settlement(promise),
            Some((PromiseStatus::Fulfilled, _))
        ));
        let b = module(&mut rt, "b.js").expect("loaded");
        let c = module(&mut rt, "c.js").expect("loaded");
        for (m, root) in [(a, a), (b, a), (c, c)] {
            let record = rt.record(m.0).expect("a record");
            assert_eq!(record.status, Status::Evaluated);
            assert_eq!(record.cycle_root, Some(root.0));
            assert!(record.body.is_none());
        }
        // Evaluate of a member gives the promise of its cycle root.
        assert_eq!(rt.evaluate(b).expect("again"), promise);
    }

    #[test]
    fn an_error_marks_the_stack() {
        let mut rt = runtime(
            &[
                ("a.js", "import './b.js';"),
                ("b.js", "import './a.js'; throw 7;"),
            ],
            true,
        );
        let a = linked(&mut rt, "a.js");
        let promise = rt.evaluate(a).expect("runs");
        assert_eq!(
            rt.promise_settlement(promise),
            Some((PromiseStatus::Rejected, Value::Int(7)))
        );
        let b = module(&mut rt, "b.js").expect("loaded");
        for m in [a, b] {
            assert_eq!(
                rt.record(m.0).expect("a record").evaluation_error,
                Some(EvaluationError::Thrown(Value::Int(7)))
            );
        }
    }

    #[test]
    fn a_termination_ends_the_task_and_the_modules() {
        let mut rt = runtime(
            &[
                ("loop.js", "for (;;) {}"),
                ("user.js", "import './loop.js';"),
            ],
            false,
        );
        let m = linked(&mut rt, "loop.js");
        rt.set_deadline(Some(
            std::time::Instant::now() + std::time::Duration::from_millis(50),
        ));
        assert!(matches!(rt.evaluate(m), Err(ScriptError::Terminated(_))));
        rt.set_deadline(None);
        assert_eq!(rt.stack_depths(), (0, 0));
        // A module that imports it never settles and runs nothing.
        let user = linked(&mut rt, "user.js");
        let promise = rt.evaluate(user).expect("no error");
        assert!(matches!(
            rt.promise_settlement(promise),
            Some((PromiseStatus::Pending, _))
        ));
    }

    /// The status, the error kind and whether the body is kept.
    fn after_termination(rt: &Runtime, name: &str) -> (Status, bool, bool) {
        let sources = rt
            .host_data::<super::super::test_support::Sources>()
            .expect("the test host");
        let m = sources.modules[name];
        let record = rt.record(m.0).expect("a record");
        (
            record.status,
            matches!(
                record.evaluation_error,
                Some(EvaluationError::Terminated(_))
            ),
            record.body.is_some(),
        )
    }

    /// A deadline `millis` from now (also a long one, so that a mistake
    /// ends as a termination, not a hang).
    fn deadline(rt: &mut Runtime, millis: u64) {
        rt.set_deadline(Some(
            std::time::Instant::now() + std::time::Duration::from_millis(millis),
        ));
    }

    #[test]
    fn a_termination_in_a_resumed_body_marks_its_module() {
        let mut rt = runtime(
            &[
                ("main.js", "import './a.js';"),
                ("a.js", "await 0; for (;;) {}"),
            ],
            true,
        );
        let main = linked(&mut rt, "main.js");
        deadline(&mut rt, 30_000);
        rt.evaluate(main).expect("runs");
        deadline(&mut rt, 50);
        assert!(matches!(rt.run_jobs(), Err(ScriptError::Terminated(_))));
        rt.set_deadline(None);
        assert_eq!(
            after_termination(&rt, "a.js"),
            (Status::Evaluated, true, false)
        );
        // The parent still waits for it.
        assert_eq!(
            after_termination(&rt, "main.js"),
            (Status::EvaluatingAsync, false, true)
        );
    }

    #[test]
    fn a_termination_in_a_gathered_ancestor_marks_the_rest() {
        let mut rt = runtime(
            &[
                ("main.js", "import './s.js'; import './t.js';"),
                ("a.js", "await 0;"),
                ("s.js", "import './a.js'; for (;;) {}"),
                ("t.js", "import './a.js';"),
            ],
            true,
        );
        let main = linked(&mut rt, "main.js");
        deadline(&mut rt, 30_000);
        rt.evaluate(main).expect("runs");
        deadline(&mut rt, 50);
        assert!(matches!(rt.run_jobs(), Err(ScriptError::Terminated(_))));
        deadline(&mut rt, 30_000);
        rt.run_jobs().expect("nothing more");
        assert_eq!(
            after_termination(&rt, "a.js"),
            (Status::Evaluated, false, false)
        );
        // `main` was gathered with them (both of its dependencies are
        // synchronous and were to run in the same job).
        for name in ["s.js", "t.js", "main.js"] {
            assert_eq!(
                after_termination(&rt, name),
                (Status::Evaluated, true, false),
                "{name}"
            );
        }
    }

    #[test]
    fn a_termination_keeps_the_error_of_a_gathered_module() {
        // `a` ends: `s` (throws) and `t` (loops) are gathered, then `x`,
        // which imports both; the order is `s`, `t`, `x`. `s` rejects `x`,
        // `t` ends in a termination, and `x` (still in the list) keeps the
        // error of `s`: a later importer gets the rejection and does not
        // wait for ever.
        let mut rt = runtime(
            &[
                ("main.js", "import './x.js';"),
                ("x.js", "import './s.js'; import './t.js';"),
                ("a.js", "await 0;"),
                ("s.js", "import './a.js'; throw 7;"),
                ("t.js", "import './a.js'; for (;;) {}"),
                ("y.js", "import './x.js';"),
            ],
            true,
        );
        let main = linked(&mut rt, "main.js");
        deadline(&mut rt, 30_000);
        rt.evaluate(main).expect("runs");
        deadline(&mut rt, 50);
        assert!(matches!(rt.run_jobs(), Err(ScriptError::Terminated(_))));
        deadline(&mut rt, 30_000);
        rt.run_jobs().expect("nothing more");
        assert_eq!(
            after_termination(&rt, "t.js"),
            (Status::Evaluated, true, false)
        );
        // `x` keeps the error of `s`, not the termination.
        assert_eq!(
            after_termination(&rt, "x.js"),
            (Status::Evaluated, false, false)
        );
        let y = linked(&mut rt, "y.js");
        let promise = rt.evaluate(y).expect("no error");
        assert_eq!(
            rt.promise_settlement(promise),
            Some((PromiseStatus::Rejected, Value::Int(7)))
        );
    }

    #[test]
    fn a_termination_in_the_first_run_of_an_async_body_marks_the_stack() {
        let mut rt = runtime(
            &[
                ("main.js", "import './a.js';"),
                ("a.js", "for (;;) {} await 0;"),
            ],
            true,
        );
        let main = linked(&mut rt, "main.js");
        deadline(&mut rt, 50);
        assert!(matches!(rt.evaluate(main), Err(ScriptError::Terminated(_))));
        deadline(&mut rt, 30_000);
        for name in ["a.js", "main.js"] {
            assert_eq!(
                after_termination(&rt, name),
                (Status::Evaluated, true, false),
                "{name}"
            );
        }
        rt.run_jobs().expect("nothing more");
    }

    #[test]
    fn async_modules_get_their_order_and_parents() {
        // `a` awaits; `b` waits for it; `c` is synchronous.
        let mut rt = runtime(
            &[
                ("root.js", "import './b.js'; import './c.js';"),
                ("b.js", "import './a.js';"),
                ("a.js", "await 0;"),
                ("c.js", ""),
            ],
            true,
        );
        let root = linked(&mut rt, "root.js");
        let promise = rt.evaluate(root).expect("runs");
        assert!(matches!(
            rt.promise_settlement(promise),
            Some((PromiseStatus::Pending, _))
        ));
        let a = module(&mut rt, "a.js").expect("loaded");
        let b = module(&mut rt, "b.js").expect("loaded");
        let c = module(&mut rt, "c.js").expect("loaded");
        let state = |rt: &Runtime, m: Module| {
            let record = rt.record(m.0).expect("a record");
            (record.status, record.async_order, record.pending_async)
        };
        assert!(matches!(
            state(&rt, a),
            (Status::EvaluatingAsync, AsyncOrder::Order(_), 0)
        ));
        assert!(matches!(
            state(&rt, b),
            (Status::EvaluatingAsync, AsyncOrder::Order(_), 1)
        ));
        assert_eq!(state(&rt, c), (Status::Evaluated, AsyncOrder::Unset, 0));
        assert!(matches!(
            state(&rt, root),
            (Status::EvaluatingAsync, AsyncOrder::Order(_), 1)
        ));
        assert_eq!(rt.record(a.0).expect("a").async_parents, [b.0]);
        assert_eq!(rt.record(b.0).expect("b").async_parents, [root.0]);
        rt.collect();
        rt.run_jobs().expect("the jobs");
        for m in [a, b, root] {
            assert_eq!(state(&rt, m), (Status::Evaluated, AsyncOrder::Done, 0));
        }
        assert_eq!(state(&rt, c), (Status::Evaluated, AsyncOrder::Unset, 0));
        assert!(matches!(
            rt.promise_settlement(promise),
            Some((PromiseStatus::Fulfilled, _))
        ));
    }
}
