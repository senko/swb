//! Loading of module graphs (§16.2.1.6.1.1 `LoadRequestedModules`,
//! <https://tc39.es/ecma262/2025/#sec-LoadRequestedModules>, with
//! `InnerModuleLoading`, `ContinueModuleLoading`, `HostLoadImportedModule`
//! and `FinishLoadingImportedModule`; ADR 0034 section 1).
//!
//! The engine keeps the state of each graph load (`GraphLoadingState`) and
//! the pending requests in a table of the runtime ([`ModuleTable`], a root),
//! so the host holds only numbers: the hook gets a token, and an answer
//! that comes later names it ([`Runtime::finish_loading_imported_module`]).
//! The walk of `InnerModuleLoading` is a loop over an explicit stack of
//! (module, next request), so a chain of modules costs no Rust stack; the
//! hook is called in the order of the specification's recursion.

use std::collections::{HashMap, HashSet};

use crate::heap::{Gc, Generic, Root, Tracer};
use crate::object::Object;
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::{VmError, VmResult};

use super::{Module, Status};

/// The referrer of a load request (§16.2.1.10): a module, a classic
/// script (by the host's number) or a realm (code without a script).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Referrer {
    /// A module record.
    Module(Module),
    /// A classic script, by its host-defined number.
    Script(u32),
    /// A realm.
    Realm(u32),
}

/// A request to the host to load a module (`HostLoadImportedModule`,
/// §16.2.1.10).
#[derive(Clone, Debug)]
pub struct LoadRequest {
    /// Who asks.
    pub referrer: Referrer,
    /// The specifier (lossy for lone surrogates).
    pub specifier: String,
    /// The import attributes (key, value), sorted by key.
    pub attributes: Vec<(String, String)>,
    /// Whether the request comes from `import()`.
    pub dynamic: bool,
    /// The number that a later answer names
    /// ([`Runtime::finish_loading_imported_module`]).
    pub token: u32,
}

/// The answer of the host to a load request.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LoadAnswer {
    /// The module, or the error of the load (a `TypeError`, or the
    /// `SyntaxError` of a module that did not parse): the module map has
    /// the result now. The value must be rooted by the host until the hook
    /// returns (a value just made is: the engine records it at once).
    Now(Result<Module, Value>),
    /// A fetch started; the host calls
    /// [`Runtime::finish_loading_imported_module`] with the token later.
    Later,
}

/// `HostLoadImportedModule`: the host's hook. It runs no script code.
pub type LoadImportedModuleHook = fn(&mut Runtime, LoadRequest) -> LoadAnswer;

/// The module hooks of a host (`Runtime::set_module_hooks`).
#[derive(Clone, Copy, Debug)]
pub struct ModuleHooks {
    /// `HostLoadImportedModule` (§16.2.1.10).
    pub load_imported_module: LoadImportedModuleHook,
}

/// `GraphLoadingState` Record (§16.2.1.6.1.1, Table 46).
struct GraphLoad {
    /// The promise of `[[PromiseCapability]]`.
    promise: Gc<Object>,
    /// `[[IsLoading]]`.
    loading: bool,
    /// `[[PendingModulesCount]]`.
    pending: u64,
    /// `[[Visited]]`.
    visited: Vec<Gc<Generic>>,
    visited_set: HashSet<Gc<Generic>>,
    /// The tokens of the requests of this load that wait for the host; the
    /// slot is free when the load is over and no token names it.
    tokens: u32,
}

/// A request that waits for the host's answer.
struct PendingLoad {
    /// The index of the graph load in [`ModuleTable::loads`].
    load: u32,
    referrer: Gc<Generic>,
    request: u32,
}

/// The module state of a runtime: the host's hooks, the graph loads and
/// the pending requests (a root of the collector).
#[derive(Default)]
pub(crate) struct ModuleTable {
    hooks: Option<ModuleHooks>,
    loads: Vec<Option<GraphLoad>>,
    free: Vec<u32>,
    pending: HashMap<u32, PendingLoad>,
    next_token: u32,
    /// `[[ModuleAsyncEvaluationCount]]` of the agent (§9.6.3,
    /// `IncrementModuleAsyncEvaluationCount`).
    async_evaluation_count: u64,
}

impl ModuleTable {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for load in self.loads.iter().flatten() {
            tracer.object(load.promise);
            for &module in &load.visited {
                tracer.generic(module);
            }
        }
        for pending in self.pending.values() {
            tracer.generic(pending.referrer);
        }
    }

    /// The bytes that the table holds for scripts.
    pub(crate) fn bytes(&self) -> usize {
        let visited: usize = self
            .loads
            .iter()
            .flatten()
            .map(|load| load.visited.capacity() * 8 + load.visited_set.capacity() * 16)
            .sum();
        self.loads.capacity() * size_of::<Option<GraphLoad>>()
            + visited
            + self.pending.capacity() * (size_of::<PendingLoad>() + 8)
    }

    fn load_mut(&mut self, index: u32) -> VmResult<&mut GraphLoad> {
        self.loads
            .get_mut(index as usize)
            .and_then(Option::as_mut)
            .ok_or(VmError::invariant("a graph load without its state"))
    }

    /// Frees the slot of a load that is over when no request waits for
    /// it.
    fn release_if_done(&mut self, index: u32) {
        let done = self
            .loads
            .get(index as usize)
            .and_then(Option::as_ref)
            .is_some_and(|load| !load.loading && load.tokens == 0);
        if done && let Some(slot) = self.loads.get_mut(index as usize) {
            *slot = None;
            self.free.push(index);
        }
    }

    /// `IncrementModuleAsyncEvaluationCount()` (§9.6.3): the next number of
    /// the order of async evaluations.
    pub(crate) fn next_async_order(&mut self) -> u64 {
        let order = self.async_evaluation_count;
        self.async_evaluation_count = order.wrapping_add(1);
        order
    }

    /// A token that no pending request uses.
    fn new_token(&mut self) -> u32 {
        loop {
            let token = self.next_token;
            self.next_token = self.next_token.wrapping_add(1);
            if !self.pending.contains_key(&token) {
                return token;
            }
        }
    }
}

/// A step of the walk: a module whose requests are being visited.
struct Visit {
    module: Gc<Generic>,
    next: u32,
}

impl Runtime {
    /// Sets the module hooks of the host.
    pub fn set_module_hooks(&mut self, hooks: ModuleHooks) {
        self.vm.modules.hooks = Some(hooks);
    }

    /// `LoadRequestedModules()` (§16.2.1.6.1.1) of a module: loads the
    /// graph through the host's hook. Returns the promise, which is
    /// fulfilled when the graph is loaded or rejected with the error of a
    /// load; the host attaches its reaction with
    /// [`Runtime::react_to_promise`] or reads it with
    /// [`Runtime::promise_settlement`]. The promise counts as handled (the
    /// host reports the error). Without hooks, every request fails with a
    /// `TypeError`.
    pub fn load_requested_modules(&mut self, module: Module) -> VmResult<Value> {
        let realm = self.record(module.0)?.realm;
        self.scoped(|rt| {
            let promise = rt.new_promise(realm)?;
            // The host reports a failed load (in its reaction), not the
            // rejection tracker.
            rt.set_promise_handled(promise)?;
            let index = rt.vm.modules.free.pop().unwrap_or_else(|| {
                rt.vm.modules.loads.push(None);
                (rt.vm.modules.loads.len() - 1) as u32
            });
            let slot = rt
                .vm
                .modules
                .loads
                .get_mut(index as usize)
                .ok_or(VmError::invariant("a graph load without its slot"))?;
            *slot = Some(GraphLoad {
                promise,
                loading: true,
                pending: 1,
                visited: Vec::new(),
                visited_set: HashSet::new(),
                tokens: 0,
            });
            rt.walk_loading(index, module.0)?;
            rt.vm.modules.release_if_done(index);
            Ok(promise.into())
        })
    }

    /// `FinishLoadingImportedModule` (§16.2.1.11) for a request that the
    /// hook answered with [`LoadAnswer::Later`]: records the module in the
    /// referrer's `[[LoadedModules]]` and continues the graph load. An
    /// unknown token (answered before, or never given) does nothing.
    pub fn finish_loading_imported_module(
        &mut self,
        token: u32,
        result: Result<Module, Value>,
    ) -> VmResult<()> {
        let Some(pending) = self.vm.modules.pending.remove(&token) else {
            return Ok(());
        };
        self.scoped(|rt| {
            rt.heap.record(match result {
                Ok(module) => Root::Generic(module.0),
                Err(error) => Root::Value(error),
            });
            rt.heap.record(Root::Generic(pending.referrer));
            let load = rt.vm.modules.load_mut(pending.load)?;
            load.tokens = load.tokens.saturating_sub(1);
            if let Ok(module) = result {
                rt.set_loaded(pending.referrer, pending.request, module.0)?;
            }
            rt.continue_loading(pending.load, result.map(|m| m.0))?;
            rt.vm.modules.release_if_done(pending.load);
            Ok(())
        })
    }

    /// Records a loaded module (§16.2.1.11 step 1): the first answer for a
    /// request stays.
    fn set_loaded(
        &mut self,
        referrer: Gc<Generic>,
        request: u32,
        module: Gc<Generic>,
    ) -> VmResult<()> {
        let record = self.record_mut(referrer)?;
        if let Some(slot) = record.loaded.get_mut(request as usize)
            && slot.is_none()
        {
            *slot = Some(module);
        }
        Ok(())
    }

    /// `ContinueModuleLoading(state, moduleCompletion)` (§16.2.1.6.1.1.2).
    fn continue_loading(&mut self, index: u32, result: Result<Gc<Generic>, Value>) -> VmResult<()> {
        if !self.vm.modules.load_mut(index)?.loading {
            return Ok(());
        }
        match result {
            Ok(module) => self.walk_loading(index, module),
            Err(error) => self.fail_loading(index, error),
        }
    }

    /// The load failed: `[[IsLoading]]` false, the promise rejected.
    fn fail_loading(&mut self, index: u32, error: Value) -> VmResult<()> {
        let load = self.vm.modules.load_mut(index)?;
        load.loading = false;
        let promise = load.promise;
        self.reject_promise(promise, error)
    }

    /// `InnerModuleLoading(state, module)` (§16.2.1.6.1.1.1) as a loop: the
    /// module and every module that it reaches through loaded requests or
    /// answers of the hook that come now.
    fn walk_loading(&mut self, index: u32, module: Gc<Generic>) -> VmResult<()> {
        let mut stack: Vec<Visit> = Vec::new();
        self.enter_loading(index, module, &mut stack)?;
        while let Some(top) = stack.last_mut() {
            self.tick()?;
            let (module, request) = (top.module, top.next);
            let count = self.record(module)?.loaded.len() as u32;
            if request >= count {
                stack.pop();
                self.finish_one(index)?;
                continue;
            }
            top.next += 1;
            if let Some(loaded) = self
                .record(module)?
                .loaded
                .get(request as usize)
                .copied()
                .flatten()
            {
                self.enter_loading(index, loaded, &mut stack)?;
            } else {
                match self.host_load(index, module, request)? {
                    Some(Ok(loaded)) => {
                        self.set_loaded(module, request, loaded)?;
                        self.enter_loading(index, loaded, &mut stack)?;
                    }
                    Some(Err(error)) => {
                        self.fail_loading(index, error)?;
                    }
                    None => {}
                }
            }
            if !self.vm.modules.load_mut(index)?.loading {
                return Ok(());
            }
        }
        Ok(())
    }

    /// The start of `InnerModuleLoading` for one module: a new module adds
    /// its requests to the pending count and is visited; any other module
    /// counts as done at once.
    fn enter_loading(
        &mut self,
        index: u32,
        module: Gc<Generic>,
        stack: &mut Vec<Visit>,
    ) -> VmResult<()> {
        let record = self.record(module)?;
        let (status, count) = (record.status, record.loaded.len() as u64);
        let load = self.vm.modules.load_mut(index)?;
        if status == Status::New && !load.visited_set.contains(&module) {
            load.visited_set.insert(module);
            load.visited.push(module);
            load.pending += count;
            stack.push(Visit { module, next: 0 });
            return Ok(());
        }
        self.finish_one(index)
    }

    /// Steps 3 to 5 of `InnerModuleLoading`: one module less to wait for;
    /// at zero the graph is loaded.
    fn finish_one(&mut self, index: u32) -> VmResult<()> {
        let load = self.vm.modules.load_mut(index)?;
        load.pending = load.pending.saturating_sub(1);
        if load.pending > 0 {
            return Ok(());
        }
        load.loading = false;
        let promise = load.promise;
        let visited = std::mem::take(&mut load.visited);
        load.visited_set = HashSet::new();
        for module in visited {
            let record = self.record_mut(module)?;
            if record.status == Status::New {
                record.status = Status::Unlinked;
            }
        }
        self.resolve_promise(promise, Value::Undefined)
    }

    /// `HostLoadImportedModule(referrer, request, …, state)`: asks the host.
    /// Returns its answer if it came now; `None` if it comes later (the
    /// token is pending). An unsupported import attribute fails with a
    /// `SyntaxError` without a call (§16.2.1.6.1.1.1 step 2.d.i).
    fn host_load(
        &mut self,
        index: u32,
        referrer: Gc<Generic>,
        request: u32,
    ) -> VmResult<Option<Result<Gc<Generic>, Value>>> {
        let record = self.record(referrer)?;
        let entry = record
            .requests
            .get(request as usize)
            .ok_or(VmError::invariant("a module request outside the list"))?;
        let text = |string| self.heap.string_lossy(string, usize::MAX);
        let specifier = text(entry.specifier);
        let attributes: Vec<(String, String)> = entry
            .attributes
            .iter()
            .map(|&(key, value)| (text(key), text(value)))
            .collect();
        if let Some((key, _)) = attributes.iter().find(|(key, _)| key != "type") {
            // Chromium 148's text; the key error comes before the value
            // error of `type`.
            let message = format!("Invalid attribute key \"{key}\".");
            let error = self.new_error(crate::error::ThrowKind::SyntaxError, &message)?;
            return Ok(Some(Err(error.into())));
        }
        let Some(hooks) = self.vm.modules.hooks else {
            let message = format!("Cannot load module '{specifier}': no module loader");
            let error = self.new_error(crate::error::ThrowKind::TypeError, &message)?;
            return Ok(Some(Err(error.into())));
        };
        let token = self.vm.modules.new_token();
        let answer = (hooks.load_imported_module)(
            self,
            LoadRequest {
                referrer: Referrer::Module(Module(referrer)),
                specifier,
                attributes,
                dynamic: false,
                token,
            },
        );
        match answer {
            LoadAnswer::Now(result) => {
                self.heap.record(match result {
                    Ok(module) => Root::Generic(module.0),
                    Err(error) => Root::Value(error),
                });
                Ok(Some(result.map(|module| module.0)))
            }
            LoadAnswer::Later => {
                self.vm.modules.pending.insert(
                    token,
                    PendingLoad {
                        load: index,
                        referrer,
                        request,
                    },
                );
                self.vm.modules.load_mut(index)?.tokens += 1;
                Ok(None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{module, runtime};
    use super::*;
    use crate::vm::PromiseStatus;

    fn later(_: &mut Runtime, _: LoadRequest) -> LoadAnswer {
        LoadAnswer::Later
    }

    #[test]
    fn tokens_skip_the_pending_ones() {
        let mut table = ModuleTable {
            next_token: u32::MAX,
            ..ModuleTable::default()
        };
        assert_eq!(table.new_token(), u32::MAX);
        table.pending.insert(
            0,
            PendingLoad {
                load: 0,
                referrer: Gc::new(0, 0),
                request: 0,
            },
        );
        // The counter wraps and steps over the token in use.
        assert_eq!(table.new_token(), 1);
    }

    #[test]
    fn a_graph_load_marks_its_modules_unlinked() {
        let mut rt = runtime(
            &[
                ("a.js", "import './b.js'; import './c.js';"),
                ("b.js", "import './c.js'; import './a.js';"),
                ("c.js", ""),
            ],
            true,
        );
        let a = module(&mut rt, "a.js").expect("parses");
        let promise = rt.load_requested_modules(a).expect("loads");
        assert!(matches!(
            rt.promise_settlement(promise),
            Some((PromiseStatus::Fulfilled, _))
        ));
        for name in ["a.js", "b.js", "c.js"] {
            let m = module(&mut rt, name).expect("loaded");
            assert_eq!(rt.record(m.0).expect("a record").status, Status::Unlinked);
        }
        // The slot of the load is free again.
        assert!(rt.vm.modules.loads.iter().all(Option::is_none));
        assert!(rt.vm.modules.pending.is_empty());
    }

    #[test]
    fn a_pending_load_keeps_its_state_until_the_answer() {
        let mut rt = runtime(&[("a.js", "import './b.js';"), ("b.js", "")], true);
        rt.set_module_hooks(ModuleHooks {
            load_imported_module: later,
        });
        let a = module(&mut rt, "a.js").expect("parses");
        let promise = rt.load_requested_modules(a).expect("loads");
        let token = *rt.vm.modules.pending.keys().next().expect("a token");
        rt.collect();
        assert!(matches!(
            rt.promise_settlement(promise),
            Some((PromiseStatus::Pending, _))
        ));
        let b = module(&mut rt, "b.js").expect("parses");
        rt.finish_loading_imported_module(token, Ok(b))
            .expect("continues");
        assert!(matches!(
            rt.promise_settlement(promise),
            Some((PromiseStatus::Fulfilled, _))
        ));
        assert_eq!(rt.record(a.0).expect("a record").loaded, [Some(b.0)]);
        assert!(rt.vm.modules.loads.iter().all(Option::is_none));
        // The token is used up.
        rt.finish_loading_imported_module(token, Err(Value::Null))
            .expect("ignored");
    }
}
