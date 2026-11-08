//! `import()` and `import.meta` (ECMA-262 2025 §13.3.10,
//! <https://tc39.es/ecma262/2025/#sec-import-calls>, and §13.3.12,
//! <https://tc39.es/ecma262/2025/#sec-meta-properties>; ADR 0034 section 4).
//!
//! The referrer of running code is `GetActiveScriptOrModule()` (§9.4.1):
//! the [`ScriptOrModule`] of the topmost frame whose code has one (each code
//! object holds the one of its compile, `bytecode/mod.rs`); none gives the
//! current realm.
//!
//! `EvaluateImportCall` makes the promise, checks the options (each failure
//! rejects it) and asks the host's hook. `ContinueDynamicImport` runs when
//! the answer is there (at once, or from
//! [`Runtime::finish_loading_imported_module`]): it loads the rest of the
//! graph (`LoadRequestedModules`), and in the job of that promise's
//! reaction links and evaluates; the reaction on the evaluation's promise
//! resolves the `import()` promise with the namespace object. The state of
//! an import in flight is heap data that the collector traces: the pending
//! request in the module table (the promise and the referrer), then a
//! [`DynamicImport`] object that the reactions hold (the promise and the
//! module).
//!
//! Termination: a termination (or an engine error) at any step ends that
//! step's task and leaves the `import()` promise pending for ever; no later
//! step settles it, because each step runs once (a token is answered once,
//! a reaction runs once). In the hook's answer and the graph load it leaves
//! the graph load pending (as a static load); in the link the modules of
//! the link are `unlinked` again (a later `import()` links them again); in
//! the evaluation they are evaluated with the termination (ADR 0034 section
//! 6). The promise is settled only while it is pending
//! ([`Runtime::settle_import`]).

use crate::bytecode::{CodeObject, Constant, ScriptOrModule};
use crate::error::ThrowKind;
use crate::heap::{Gc, Generic, Tracer};
use crate::object::{Object, ObjectKind};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::{PromiseStatus, ReactionKind, VmError, VmResult};

use super::load::PendingLoad;
use super::{LoadAnswer, Module, Referrer};

/// Chromium 148's message when the options of `import()` are not an object
/// (probe case `msg-import-call-options`).
const OPTIONS_NOT_OBJECT: &str = "The second argument to import() must be an object";
/// ... when `options.with` is not an object.
const WITH_NOT_OBJECT: &str = "The 'with' option must be an object";
/// ... when an attribute's value is not a string.
const VALUE_NOT_STRING: &str = "Import attribute value must be a string";

/// The state of an `import()` after the host's answer: the data of the
/// reactions of `ContinueDynamicImport` (internal, an object that no
/// script sees).
pub struct DynamicImport {
    /// The promise of the `import()` (`promiseCapability`).
    promise: Gc<Object>,
    /// The module that the host gave.
    module: Gc<Generic>,
}

impl DynamicImport {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.object(self.promise);
        tracer.generic(self.module);
    }
}

/// A module request of `import()`: the specifier and the attributes,
/// sorted by key.
type Request = (super::load::SpecifierText, Vec<(String, String)>);

impl Runtime {
    /// `GetActiveScriptOrModule()` (§9.4.1): the script or module of the
    /// topmost frame that has one. Native functions have no frame, so a
    /// built-in function's context (whose `ScriptOrModule` is null) is
    /// skipped as the specification skips it.
    pub(crate) fn active_script_or_module(&self) -> ScriptOrModule {
        for frame in self.vm.frames.iter().rev() {
            let referrer = self
                .heap
                .generic::<CodeObject>(frame.code)
                .map_or(ScriptOrModule::None, |code| code.referrer.get());
            if referrer != ScriptOrModule::None {
                return referrer;
            }
        }
        ScriptOrModule::None
    }

    /// Gives the code object `code` and all code objects nested in it the
    /// script or module `referrer`. Allocates nothing, so unrooted code
    /// stays valid.
    pub(crate) fn set_script_or_module(
        &mut self,
        code: Gc<Generic>,
        referrer: ScriptOrModule,
    ) -> VmResult<()> {
        if referrer == ScriptOrModule::None {
            return Ok(());
        }
        let mut work = vec![code];
        while let Some(code) = work.pop() {
            let object = self.heap.generic::<CodeObject>(code)?;
            object.referrer.set(referrer);
            for constant in &object.code.constants {
                if let Constant::Function(child, _) = constant {
                    work.push(*child);
                }
            }
        }
        Ok(())
    }

    /// The `import()` instruction: `EvaluateImportCall` (§13.3.10.1) after
    /// the evaluation of the operands. Returns the promise; every failure
    /// after the operands rejects it. A termination in a conversion, a
    /// getter or the host's answer is an error of the running code.
    pub(crate) fn import_call(&mut self, specifier: Value, options: Value) -> VmResult<Value> {
        let referrer = self.active_script_or_module();
        let realm = self.current_realm();
        let promise = self.new_promise(realm)?;
        match self.import_request(specifier, options)? {
            Ok(request) => self.start_dynamic_import(promise, referrer, request)?,
            Err(reason) => self.reject_promise(promise, reason)?,
        }
        Ok(promise.into())
    }

    /// Steps 8 to 12 of `EvaluateImportCall`: `ToString` of the specifier,
    /// then the options (`with` and its entries). `Err` is the reason of
    /// the rejection.
    fn import_request(
        &mut self,
        specifier: Value,
        options: Value,
    ) -> VmResult<Result<Request, Value>> {
        let text = self.to_string(specifier);
        let specifier = match self.caught(text)? {
            Ok(text) => self.specifier_text(text),
            Err(reason) => return Ok(Err(reason)),
        };
        if options == Value::Undefined {
            return Ok(Ok((specifier, Vec::new())));
        }
        let Value::Object(options) = options else {
            return self.type_error(OPTIONS_NOT_OBJECT).map(Err);
        };
        let with = PropertyKey::String(self.heap.intern_str("with")?);
        let with = self.get_value(options.into(), with);
        let attributes = match self.caught(with)? {
            Ok(Value::Undefined) => return Ok(Ok((specifier, Vec::new()))),
            Ok(Value::Object(attributes)) => attributes,
            Ok(_) => return self.type_error(WITH_NOT_OBJECT).map(Err),
            Err(reason) => return Ok(Err(reason)),
        };
        self.heap.record(attributes);
        let mut entries = match self.attribute_entries(attributes)? {
            Ok(entries) => entries,
            Err(reason) => return Ok(Err(reason)),
        };
        // `AllImportAttributesSupported` (`HostGetSupportedImportAttributes`
        // gives `type`), after all values are checked; Chromium 148's text.
        if let Some((key, _)) = entries.iter().find(|(key, _)| key != "type") {
            let message = format!("Invalid attribute key \"{key}\".");
            return self.type_error(&message).map(Err);
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(Ok((specifier, entries)))
    }

    /// `EnumerableOwnProperties(attributes, key+value)` and the check that
    /// each value of a string key is a string (step 11.d of
    /// `EvaluateImportCall`).
    fn attribute_entries(
        &mut self,
        attributes: Gc<Object>,
    ) -> VmResult<Result<Vec<(String, String)>, Value>> {
        let keys = self.own_keys(attributes);
        let keys = match self.caught(keys)? {
            Ok(keys) => keys,
            Err(reason) => return Ok(Err(reason)),
        };
        self.heap.record_keys(&keys);
        // All entries first (their getters run), then the check of the
        // values, as the specification orders them.
        let mut entries = Vec::new();
        for key in keys {
            self.tick()?;
            let Some(name) = self.key_to_string(key) else {
                continue;
            };
            let enumerable = self.own_enumerable(attributes, key);
            match self.caught(enumerable)? {
                Ok(true) => {}
                Ok(false) => continue,
                Err(reason) => return Ok(Err(reason)),
            }
            let value = self.get_value(attributes.into(), key);
            match self.caught(value)? {
                Ok(value) => entries.push((name, self.heap.record(value))),
                Err(reason) => return Ok(Err(reason)),
            }
        }
        let mut texts = Vec::with_capacity(entries.len());
        for (name, value) in entries {
            let Value::String(text) = value else {
                return self.type_error(VALUE_NOT_STRING).map(Err);
            };
            texts.push((name, self.heap.string_lossy(text, usize::MAX)));
        }
        Ok(Ok(texts))
    }

    /// `Completion(...)` of an operation with any result: `Ok(Err(value))`
    /// for a throw (recorded in the current handle scope), `Err` for a
    /// termination or an engine error.
    fn caught<T>(&mut self, result: VmResult<T>) -> VmResult<Result<T, Value>> {
        match result {
            Ok(value) => Ok(Ok(value)),
            Err(error) => {
                let reason = self.thrown_value(error)?;
                Ok(Err(self.heap.record(reason)))
            }
        }
    }

    /// A new `TypeError` of the current realm (the reason of a rejection).
    fn type_error(&mut self, message: &str) -> VmResult<Value> {
        let error = self.error_object(ThrowKind::TypeError, message)?;
        Ok(self.heap.record(error).into())
    }

    /// Step 13 of `EvaluateImportCall`: `HostLoadImportedModule(referrer,
    /// moduleRequest, empty, promiseCapability)`.
    fn start_dynamic_import(
        &mut self,
        promise: Gc<Object>,
        referrer: ScriptOrModule,
        (specifier, attributes): Request,
    ) -> VmResult<()> {
        let (host_referrer, module_referrer) = match referrer {
            ScriptOrModule::Module(module) => (Referrer::Module(Module(module)), Some(module)),
            ScriptOrModule::Script(number) => (Referrer::Script(number), None),
            ScriptOrModule::None => (Referrer::Realm(self.current_realm()), None),
        };
        let Some((token, answer)) =
            self.call_load_hook(host_referrer, (specifier.clone(), attributes), true)
        else {
            let error = self.no_loader_error(&specifier.0)?;
            return self.settle_import(promise, Err(error));
        };
        match answer {
            LoadAnswer::Now(result) => self.continue_dynamic_import(promise, result.map(|m| m.0)),
            LoadAnswer::Later => {
                self.vm.modules.add_pending(
                    token,
                    PendingLoad::Dynamic {
                        promise,
                        referrer: module_referrer,
                    },
                );
                Ok(())
            }
        }
    }

    /// `ContinueDynamicImport(promiseCapability, moduleCompletion)`
    /// (§13.3.10.1.1): a failed load rejects the promise; else the graph of
    /// the module loads, and the reactions of its promise go on. `promise`
    /// and the result must be rooted by the caller.
    pub(super) fn continue_dynamic_import(
        &mut self,
        promise: Gc<Object>,
        result: Result<Gc<Generic>, Value>,
    ) -> VmResult<()> {
        let module = match result {
            Ok(module) => module,
            Err(reason) => return self.settle_import(promise, Err(reason)),
        };
        let Value::Object(loaded) = self.load_requested_modules(Module(module))? else {
            return Err(VmError::invariant("a graph load without its promise"));
        };
        // The table of graph loads keeps the promise only while it loads.
        self.heap.record(loaded);
        let state = self.heap.new_object_with_kind(
            None,
            ObjectKind::DynamicImport(Box::new(DynamicImport { promise, module })),
        )?;
        self.heap.record(state);
        self.then_internal(loaded, dynamic_import_loaded, state.into())
    }

    /// Settles the promise of an `import()` if it is still pending (each
    /// step settles it at most once; the check keeps an engine invariant
    /// out of the host's way).
    fn settle_import(&mut self, promise: Gc<Object>, result: Result<Value, Value>) -> VmResult<()> {
        if self.promise_state(promise)?.status != PromiseStatus::Pending {
            log::debug!("an import() promise that was settled before");
            return Ok(());
        }
        match result {
            Ok(namespace) => self.resolve_promise(promise, namespace),
            Err(reason) => self.reject_promise(promise, reason),
        }
    }

    /// The promise and the module of the data of a reaction.
    fn dynamic_import_state(&self, data: Value) -> VmResult<(Gc<Object>, Gc<Generic>)> {
        let Value::Object(object) = data else {
            return Err(VmError::invariant("an import() reaction without its state"));
        };
        match &self.heap.object(object)?.kind {
            ObjectKind::DynamicImport(state) => Ok((state.promise, state.module)),
            _ => Err(VmError::invariant("an import() reaction without its state")),
        }
    }

    /// `import.meta` (§13.3.12.1): the meta object of the running code's
    /// module; the first use makes an ordinary object with a null
    /// prototype and lets the host define its properties.
    pub(crate) fn import_meta(&mut self) -> VmResult<Value> {
        let ScriptOrModule::Module(module) = self.active_script_or_module() else {
            return Err(VmError::invariant("import.meta outside module code"));
        };
        if let Some(meta) = self.record(module)?.meta {
            return Ok(meta.into());
        }
        let meta = self.heap.new_object(None)?;
        self.heap.record(meta);
        if let Some(hook) = self
            .vm
            .modules
            .hooks()
            .and_then(|hooks| hooks.import_meta_properties)
        {
            hook(self, Module(module), meta)?;
        }
        let record = self.record_mut(module)?;
        // The hook runs no script code, so nothing made one meanwhile.
        if let Some(existing) = record.meta {
            return Ok(existing.into());
        }
        record.meta = Some(meta);
        Ok(meta.into())
    }
}

/// The reaction on the promise of `LoadRequestedModules` (the closures
/// `linkAndEvaluate` and `onRejected` of `ContinueDynamicImport`): links
/// and evaluates the module, then waits for the evaluation's promise.
fn dynamic_import_loaded(
    rt: &mut Runtime,
    data: Value,
    kind: ReactionKind,
    value: Value,
) -> VmResult<Value> {
    let (promise, module) = rt.dynamic_import_state(data)?;
    if kind == ReactionKind::Reject {
        rt.settle_import(promise, Err(value))?;
        return Ok(Value::Undefined);
    }
    match rt.link_module(module) {
        Ok(()) => {}
        Err(error @ (VmError::Throw(_) | VmError::Raise { .. })) => {
            let reason = rt.thrown_value(error)?;
            rt.heap.record(reason);
            rt.settle_import(promise, Err(reason))?;
            return Ok(Value::Undefined);
        }
        Err(error) => return Err(error),
    }
    let evaluated = rt.evaluate_promise(module)?;
    rt.heap.record(evaluated);
    rt.then_internal(evaluated, dynamic_import_evaluated, data)?;
    Ok(Value::Undefined)
}

/// The reaction on the evaluation's promise (the closures `onFulfilled`
/// and `onRejected`): the namespace object resolves the promise of the
/// `import()`.
fn dynamic_import_evaluated(
    rt: &mut Runtime,
    data: Value,
    kind: ReactionKind,
    value: Value,
) -> VmResult<Value> {
    let (promise, module) = rt.dynamic_import_state(data)?;
    let result = match kind {
        ReactionKind::Reject => Err(value),
        ReactionKind::Fulfill => Ok(rt.namespace_object(module)?.into()),
    };
    rt.settle_import(promise, result)?;
    Ok(Value::Undefined)
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{Sources, module, runtime};
    use super::*;
    use crate::runtime::{ScriptError, finish_error};

    /// A graph with a static import, a function that the link makes, a
    /// module with top-level `await` and a namespace.
    const SOURCES: &[(&str, &str)] = &[
        (
            "c.js",
            "import './tla.js'; import { f } from './b.js'; export const x = f();",
        ),
        ("b.js", "export function f() { return 3; }"),
        ("tla.js", "await 0; export const w = 2;"),
    ];

    /// Answers the waiting loads and runs the jobs until nothing is left;
    /// the errors on the way.
    fn drive(rt: &mut Runtime) -> Vec<ScriptError> {
        let mut errors = Vec::new();
        for _ in 0..10_000 {
            let sources = rt.host_data_mut::<Sources>().expect("the test host");
            let next = sources.waiting.as_mut().and_then(Vec::pop);
            if let Some((token, name)) = next {
                let result = module(rt, &name);
                if let Err(error) = rt.finish_loading_imported_module(token, result) {
                    errors.push(finish_error(error));
                }
                continue;
            }
            if rt.pending_jobs() == 0 {
                break;
            }
            if let Err(error) = rt.run_jobs() {
                errors.push(error);
            }
        }
        errors
    }

    /// The state of a global promise: `none`, `pending`, `fulfilled` or
    /// `rejected`.
    fn promise_state(rt: &mut Runtime, name: &str) -> &'static str {
        let value = rt.eval(name).unwrap_or(Value::Undefined);
        match rt.promise_settlement(value) {
            None => "none",
            Some((PromiseStatus::Pending, _)) => "pending",
            Some((PromiseStatus::Fulfilled, _)) => "fulfilled",
            Some((PromiseStatus::Rejected, _)) => "rejected",
        }
    }

    #[test]
    fn a_termination_at_any_step_leaves_a_usable_state() {
        // The termination comes at the n-th step of the countdown: in the
        // script, in `import()`, in the graph load (now, or in the answer
        // that comes later), in the link and the evaluation (jobs), in the
        // async body, or in the reactions. Afterwards the `import()`
        // promise is pending or fulfilled, every error was a termination,
        // and a new `import()` of the module completes or waits for ever
        // (a module that a termination ended never runs again).
        let mut outcomes = std::collections::BTreeSet::new();
        for later in [false, true] {
            for steps in 1..=300 {
                let stress = steps % 37 == 0;
                let mut rt = runtime(SOURCES, stress);
                if later {
                    rt.host_data_mut::<Sources>().expect("the host").waiting = Some(Vec::new());
                }
                rt.vm.countdown.set(steps);
                rt.termination_handle().terminate();
                let mut errors = Vec::new();
                if let Err(error) = rt.eval(
                    "globalThis.r = 'pending'; globalThis.p = import('./c.js'); \
                     p.then(ns => { r = ns.x; }, () => { r = 'rejected'; })",
                ) {
                    errors.push(error);
                }
                errors.extend(drive(&mut rt));
                // A request that did not fire comes out here.
                let unfired = rt.check_time().is_err();
                assert!(
                    errors
                        .iter()
                        .all(|error| matches!(error, ScriptError::Terminated(_))),
                    "{later} {steps}: {errors:?}"
                );
                assert_eq!(errors.len(), usize::from(!unfired), "{later} {steps}");
                assert_eq!(rt.stack_depths(), (0, 0));
                let first = promise_state(&mut rt, "p");
                assert!(
                    matches!(first, "none" | "pending" | "fulfilled"),
                    "{later} {steps}: {first}"
                );
                rt.eval(
                    "globalThis.r2 = 'pending'; import('./c.js')\
                     .then(ns => { r2 = ns.x; }, () => { r2 = 'rejected'; })",
                )
                .expect("a new script runs");
                let errors = drive(&mut rt);
                assert!(errors.is_empty(), "{later} {steps}: {errors:?}");
                let second = rt.eval("String(r2)").expect("r2");
                let second = rt.display(second);
                assert!(
                    matches!(second.as_str(), "3" | "pending"),
                    "{later} {steps}: {second}"
                );
                assert_eq!(rt.heap().stats().total_stale_roots, 0);
                let second = if second == "3" { "3" } else { "pending" };
                outcomes.insert((first, second, unfired));
            }
        }
        // The sweep reached the end of the work (nothing fired) and ended
        // the import in several ways.
        assert!(outcomes.contains(&("fulfilled", "3", true)), "{outcomes:?}");
        assert!(
            outcomes.contains(&("pending", "pending", false)),
            "{outcomes:?}"
        );
        assert!(outcomes.contains(&("pending", "3", false)), "{outcomes:?}");
        assert!(outcomes.contains(&("none", "3", false)), "{outcomes:?}");
    }

    #[test]
    fn code_objects_share_their_referrer() {
        let mut rt = runtime(&[("m.js", "function f() { return () => 1; }")], true);
        let m = module(&mut rt, "m.js").expect("parses");
        let code = rt.record(m.0).expect("a record").code;
        let mut work = vec![code];
        let mut count = 0;
        while let Some(code) = work.pop() {
            let object = rt.heap.generic::<CodeObject>(code).expect("code");
            assert_eq!(object.referrer.get(), ScriptOrModule::Module(m.0));
            count += 1;
            for constant in &object.code.constants {
                if let Constant::Function(child, _) = constant {
                    work.push(*child);
                }
            }
        }
        assert_eq!(count, 3);
    }
}
