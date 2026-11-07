//! Evaluation (§16.2.1.6.1.3 `Evaluate`,
//! <https://tc39.es/ecma262/2025/#sec-moduleevaluation>, with
//! `InnerModuleEvaluation` §16.2.1.6.1.3.1; ADR 0034 sections 1 and 6)
//! for graphs without top-level `await` (sub-feature 3a-1; the compiler
//! refuses `await` at the top level of a module until 3a-2).
//!
//! `InnerModuleEvaluation` is Tarjan's DFS as a loop with an explicit
//! stack; each module body is resumed from that loop (its suspended frame
//! moves back on the stacks and runs in a fresh interpreter loop), so a
//! chain of modules costs no Rust stack per module. An error of a body
//! becomes `[[EvaluationError]]` of every module on the DFS stack, and a
//! later Evaluate of any of them gives the same error. A termination marks
//! them too: they never run again, and a later Evaluate gives a promise
//! that never settles (it reports nothing more).

use crate::heap::{Gc, Generic, Root};
use crate::object::Object;
use crate::runtime::{Runtime, ScriptError};
use crate::value::Value;
use crate::vm::{VmError, VmResult};

use super::{EvaluationError, Module, Status};

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

impl Runtime {
    /// `Evaluate()` (§16.2.1.6.1.3) of a linked module: runs the bodies of
    /// the graph that have not run. Returns the promise of the evaluation,
    /// which is settled when the call returns (no top-level `await` yet):
    /// fulfilled with `undefined`, or rejected with the error of a body
    /// (also an error of an earlier evaluation of a module of the graph).
    /// The promise counts as handled (the host reports the error). A
    /// termination is [`ScriptError::Terminated`]; the modules that were
    /// running are marked, and a later Evaluate of them gives a promise that
    /// never settles. The same module gives the same promise each time.
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
            Status::Evaluated => record.cycle_root.unwrap_or(module),
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
            None => self.resolve_promise(promise, Value::Undefined)?,
            Some(EvaluationError::Thrown(error)) => self.reject_promise(promise, error)?,
            Some(EvaluationError::Terminated(_)) => {}
        }
        Ok(promise)
    }

    /// Evaluate for a host run that reports errors as a script does
    /// ([`Runtime::eval_module_at`]): a thrown value is the error.
    pub(crate) fn evaluate_sync(&mut self, module: Gc<Generic>) -> VmResult<()> {
        match self.evaluate_graph(module)? {
            None => Ok(()),
            Some(EvaluationError::Thrown(error)) => Err(VmError::Throw(error)),
            Some(EvaluationError::Terminated(reason)) => Err(VmError::Terminated(reason)),
        }
    }

    /// Steps 4 to 7 of Evaluate: `InnerModuleEvaluation` from `module`; on
    /// an error, every module on the stack is evaluated with that error.
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
            Abrupt::Fresh(error @ VmError::Terminated(reason)) => {
                (EvaluationError::Terminated(reason), Some(error))
            }
            Abrupt::Fresh(error @ VmError::Internal(_)) => (
                EvaluationError::Terminated(crate::error::Termination::HostRequest),
                Some(error),
            ),
        };
        for member in stack {
            let record = self.record_mut(member)?;
            record.status = Status::Evaluated;
            record.evaluation_error = Some(error);
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
            self.execute_module(module)?;
            let record = self.record(module)?;
            if record.dfs_ancestor_index == record.dfs_index {
                while let Some(member) = stack.pop() {
                    let record = self.record_mut(member)?;
                    record.status = Status::Evaluated;
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

    /// Steps 2 to 9 of `InnerModuleEvaluation`.
    fn enter_evaluation(
        &mut self,
        module: Gc<Generic>,
        index: &mut u32,
        stack: &mut Vec<Gc<Generic>>,
        visits: &mut Vec<Visit>,
    ) -> VmResult<Entry> {
        let record = self.record_mut(module)?;
        match record.status {
            Status::Evaluated => {
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
        *index = index.saturating_add(1);
        stack.push(module);
        visits.push(Visit { module, next: 0 });
        Ok(Entry::Entered)
    }

    /// Step 10.d of `InnerModuleEvaluation` for a required module whose
    /// evaluation is over or in progress: one still `evaluating` lowers the
    /// ancestor index; an evaluated one passes on the error of its cycle
    /// root.
    fn after_required(&mut self, module: Gc<Generic>, required: Gc<Generic>) -> Result<(), Abrupt> {
        let child = self.record(required)?;
        if child.status == Status::Evaluating {
            let ancestor = child.dfs_ancestor_index;
            let record = self.record_mut(module)?;
            record.dfs_ancestor_index = record.dfs_ancestor_index.min(ancestor);
            return Ok(());
        }
        let root = child.cycle_root.unwrap_or(required);
        match self.record(root)?.evaluation_error {
            Some(error) => Err(Abrupt::Cached(error)),
            None => Ok(()),
        }
    }

    /// `ExecuteModule()` (§16.2.1.7.3.2) of a module without top-level
    /// `await`: resumes the module function after its prologue and runs the
    /// body to its end.
    fn execute_module(&mut self, module: Gc<Generic>) -> VmResult<()> {
        let record = self.record(module)?;
        // The compiler refuses top-level `await` (sub-feature 3a-2).
        if record.has_tla {
            return Err(VmError::invariant("a module with top-level await"));
        }
        let body = record
            .body
            .ok_or(VmError::invariant("a module body that was not prepared"))?;
        let result = self.run_suspended(body);
        self.record_mut(module)?.body = None;
        result.map(drop)
    }
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
}
