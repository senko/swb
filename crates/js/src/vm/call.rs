//! Calls (ADR 0026 section 5, memo 1.2): frame setup with the argument
//! rules, native calls, deferred calls, returns, re-entry and unwinding.
//!
//! Frame layout: a caller puts the callee, `this` and the arguments in
//! consecutive slots of the value stack. The callee's register window
//! starts at the first argument, so the arguments become the parameter
//! registers without a copy. Missing arguments are `undefined`; arguments
//! beyond the declared parameters are copied into the frame record only
//! when the code uses `arguments`, and dropped otherwise. All other
//! registers of the window start as `undefined`.
//!
//! A native function runs at once, with a handle scope open. Its deferred
//! call or generator resumption becomes a frame whose result goes where
//! the native call's result would have gone.
//!
//! A deferred call and the call of a bound function's target are tail
//! calls: `invoke_tail_calls` runs them in a loop, in one window at the
//! top of the value stack that each next tail call replaces. So a chain of
//! them (`f = Function.prototype.call.bind(f)` repeated) costs neither
//! Rust stack nor value stack in proportion to its length. Each level is
//! a step of the time countdown and charges the copy of its arguments.

use std::rc::Rc;

use crate::bytecode::{CodeKind, FunctionCode, ThisMode};
use crate::heap::{Gc, Generic};
use crate::object::{GetResult, Object, ObjectKind};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::function::{is_callable_kind, is_constructor_kind};
use crate::vm::generator::GeneratorKind;
use crate::vm::{
    Closure, Frame, Intrinsic, NativeCall, NativeReturn, REENTRY_WEIGHT, ReturnTo, STACK_OVERFLOW,
    VmError, VmResult,
};

/// The outcome of starting a call.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Step {
    /// The call completed (a native function); its result.
    Value(Value),
    /// A frame was pushed; the loop continues in it.
    Entered,
    /// The callee is not callable (or not a constructor); the caller
    /// reports the `TypeError` with its own description of the callee.
    NotCallable,
}

/// A tail call: the deferred call of a native function or the call of a
/// bound function's target.
struct TailCall {
    callee: Value,
    this: Value,
    args: Vec<Value>,
    /// `new.target` of a `[[Construct]]`; `None` for `[[Call]]`.
    new_target: Option<Gc<Object>>,
}

/// What a call of a proxy does next.
enum ProxyStep {
    /// A tail call of the target or of the trap.
    Tail(TailCall),
    /// The result of a `construct` trap.
    Value(Value),
    /// The trap is not callable.
    NotCallable,
}

/// How a call was started.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CallSite {
    /// The index of the first argument in the value stack (the callee is
    /// at `window - 2`, `this` at `window - 1`).
    pub(crate) window: usize,
    /// The number of arguments.
    pub(crate) argc: usize,
    /// Where the result of a pushed frame goes.
    pub(crate) ret: ReturnTo,
    /// The length of the value stack after a pushed frame returns.
    pub(crate) restore_top: usize,
    /// Whether this is `[[Construct]]` (`new`).
    pub(crate) construct: bool,
    /// `new.target` of a `[[Construct]]` whose `new.target` is not the
    /// callee (`Reflect.construct`, bound functions). The call keeps it
    /// alive in the slot of `this`.
    pub(crate) new_target: Option<Gc<Object>>,
}

/// What the frame of a closure needs from the function object, read in one
/// lookup.
struct Callee {
    function: Gc<Object>,
    code: Gc<Generic>,
    compiled: Rc<FunctionCode>,
    this_mode: ThisMode,
    realm: u32,
}

impl Callee {
    fn of(function: Gc<Object>, closure: &Closure) -> Callee {
        Callee {
            function,
            code: closure.code,
            compiled: Rc::clone(&closure.compiled),
            this_mode: closure.this_mode,
            realm: closure.realm,
        }
    }
}

/// Makes the value stack end at the top of a new register window of
/// `registers` registers at `window`: the first `kept` slots (the passed
/// arguments that are parameters) stay, the others become `undefined`.
#[inline]
fn clear_window(stack: &mut Vec<Value>, window: usize, kept: usize, registers: usize) {
    stack.truncate(window + kept);
    stack.resize(window + registers, Value::Undefined);
}

/// The result of a native function, checked: internal values (`Empty`,
/// `Cell`) must not reach a script.
fn native_value(value: Value) -> VmResult<Value> {
    if value.is_internal() {
        return Err(VmError::invariant("a native function returned Empty"));
    }
    Ok(value)
}

/// The `new.target` of a deferred `[[Construct]]`.
fn object_new_target(new_target: Value) -> VmResult<Gc<Object>> {
    match new_target {
        Value::Object(object) => Ok(object),
        _ => Err(VmError::invariant("new.target is not an object")),
    }
}

impl Runtime {
    /// Starts the call described by `site` (§10.2.1 `[[Call]]`, §10.2.2
    /// `[[Construct]]` for ordinary functions, §10.3.1 for natives).
    pub(crate) fn invoke(&mut self, site: CallSite) -> VmResult<Step> {
        let callee = self.stack_value(site.window.wrapping_sub(2))?;
        let Value::Object(function) = callee else {
            return Ok(Step::NotCallable);
        };
        let record = self.heap.object(function)?;
        // A closure first: one lookup of the object gives all that the
        // frame needs (the common case of script code).
        if let ObjectKind::Function(closure) = &record.kind {
            if site.construct && !is_constructor_kind(&record.kind) {
                return Ok(Step::NotCallable);
            }
            let callee = Callee::of(function, closure);
            return self.enter_function(callee, site);
        }
        if site.construct && !is_constructor_kind(&record.kind) {
            return Ok(Step::NotCallable);
        }
        match &record.kind {
            // Closures returned above; the arm keeps the match exhaustive
            // without a wildcard.
            ObjectKind::Function(_) => Err(VmError::invariant("a closure after its arm")),
            ObjectKind::Native(native) => {
                let (func, realm) = (native.func, native.realm);
                self.call_native(function, func, realm, site)
            }
            ObjectKind::Bound(_) => {
                let call = self.invoke_bound(function, site)?;
                self.invoke_tail_calls(site, call)
            }
            ObjectKind::Proxy(_) => self.invoke_proxy_step(function, site),
            _ => Ok(Step::NotCallable),
        }
    }

    /// [`Runtime::invoke`] of a proxy (out of line: the arm of the common
    /// kinds stays small).
    #[cold]
    #[inline(never)]
    fn invoke_proxy_step(&mut self, proxy: Gc<Object>, site: CallSite) -> VmResult<Step> {
        if !site.construct && !is_callable_kind(&self.heap.object(proxy)?.kind) {
            return Ok(Step::NotCallable);
        }
        match self.invoke_proxy(proxy, site)? {
            ProxyStep::Tail(call) => self.invoke_tail_calls(site, call),
            ProxyStep::Value(value) => Ok(Step::Value(value)),
            ProxyStep::NotCallable => Ok(Step::NotCallable),
        }
    }

    /// [`Runtime::invoke_proxy`] in the loop of tail calls: the step, or the
    /// next tail call; a trap that is not callable is the `TypeError` of
    /// `callee`.
    fn tail_proxy(
        &mut self,
        proxy: Gc<Object>,
        site: CallSite,
        callee: Value,
    ) -> VmResult<Result<Step, TailCall>> {
        match self.invoke_proxy(proxy, site)? {
            ProxyStep::Tail(next) => Ok(Err(next)),
            ProxyStep::Value(value) => Ok(Ok(Step::Value(value))),
            ProxyStep::NotCallable => {
                let what = if site.construct {
                    "a constructor"
                } else {
                    "a function"
                };
                Err(VmError::type_error(format!(
                    "{} is not {what}",
                    self.describe_value(callee)
                )))
            }
        }
    }

    /// `[[Call]]` and `[[Construct]]` of a proxy (§10.5.12, §10.5.13): the
    /// call of the target or of the trap as a tail call, or the checked
    /// result of a `construct` trap (which ran through re-entry). The
    /// arguments are in the window of `site`.
    fn invoke_proxy(&mut self, proxy: Gc<Object>, site: CallSite) -> VmResult<ProxyStep> {
        let args = self
            .vm
            .stack
            .get(site.window..site.window + site.argc)
            .ok_or(VmError::invariant("arguments outside the stack"))?
            .to_vec();
        if site.construct {
            let new_target = site.new_target.unwrap_or(proxy);
            return Ok(match self.proxy_construct(proxy, &args, new_target)? {
                None => ProxyStep::NotCallable,
                Some(Ok(object)) => ProxyStep::Value(object),
                Some(Err((callee, args, new_target))) => ProxyStep::Tail(TailCall {
                    callee,
                    this: Value::Undefined,
                    args,
                    new_target: Some(new_target),
                }),
            });
        }
        let this = self.stack_value(site.window - 1)?;
        Ok(match self.proxy_call_parts(proxy, this, &args)? {
            None => ProxyStep::NotCallable,
            Some((callee, this, args)) => ProxyStep::Tail(TailCall {
                callee,
                this,
                args,
                new_target: None,
            }),
        })
    }

    /// Runs a tail call (the deferred call of a native, the target of a
    /// bound function) and the tail calls that follow it, in a loop. The
    /// first one pushes a window at the top of the value stack; each later
    /// one replaces it, because the values of the finished call are no
    /// longer needed (a tail call's values are in Rust locals, and nothing
    /// can collect until they are in the new window). A pushed frame
    /// restores the stack to `site.restore_top` when it returns; a native
    /// result drops the window here. A callee that is not callable is a
    /// `TypeError` with its own description.
    #[cold]
    #[inline(never)]
    fn invoke_tail_calls(&mut self, site: CallSite, mut call: TailCall) -> VmResult<Step> {
        let mut tail_window: Option<usize> = None;
        loop {
            // Each level is a step of the countdown, and the copy of its
            // arguments is charged: a native's deferred call can pass on
            // all but one of its arguments, so a chain of `call`s over a
            // long argument list would be quadratic work.
            self.tick()?;
            self.charge(call.args.len())?;
            if let Some(window) = tail_window {
                self.vm.stack.truncate(window - 2);
            }
            let window = self.push_window(call.callee, call.this, &call.args)?;
            tail_window = Some(window);
            let site = CallSite {
                window,
                argc: call.args.len(),
                ret: site.ret,
                restore_top: site.restore_top,
                construct: call.new_target.is_some(),
                new_target: call.new_target,
            };
            let not_callable = || {
                let what = if site.construct {
                    "a constructor"
                } else {
                    "a function"
                };
                (call.callee, what)
            };
            let Value::Object(function) = call.callee else {
                let (callee, what) = not_callable();
                return Err(VmError::type_error(format!(
                    "{} is not {what}",
                    self.describe_value(callee)
                )));
            };
            let record = self.heap.object(function)?;
            let callable = match &record.kind {
                ObjectKind::Function(_)
                | ObjectKind::Native(_)
                | ObjectKind::Bound(_)
                | ObjectKind::Proxy(_) => {
                    is_callable_kind(&record.kind)
                        && (!site.construct || is_constructor_kind(&record.kind))
                }
                _ => false,
            };
            if !callable {
                let (callee, what) = not_callable();
                return Err(VmError::type_error(format!(
                    "{} is not {what}",
                    self.describe_value(callee)
                )));
            }
            let step = match &record.kind {
                ObjectKind::Native(native) => {
                    let (func, realm) = (native.func, native.realm);
                    match self.run_native(function, func, realm, site)? {
                        NativeReturn::Value(value) => Step::Value(native_value(value)?),
                        NativeReturn::Call { callee, this, args } => {
                            call = TailCall {
                                callee,
                                this,
                                args,
                                new_target: None,
                            };
                            continue;
                        }
                        NativeReturn::Construct {
                            callee,
                            new_target,
                            args,
                        } => {
                            call = TailCall {
                                callee,
                                this: Value::Undefined,
                                args,
                                new_target: Some(object_new_target(new_target)?),
                            };
                            continue;
                        }
                        other @ (NativeReturn::Resume(_) | NativeReturn::Loop(_)) => {
                            self.native_request(other, site)?
                        }
                    }
                }
                ObjectKind::Bound(_) => {
                    call = self.invoke_bound(function, site)?;
                    continue;
                }
                ObjectKind::Proxy(_) => match self.tail_proxy(function, site, call.callee)? {
                    Ok(step) => step,
                    Err(next) => {
                        call = next;
                        continue;
                    }
                },
                _ => self.enter_closure(function, site)?,
            };
            if let Step::Value(_) = step {
                self.vm.stack.truncate(window - 2);
            }
            return Ok(step);
        }
    }

    /// `[[Call]]` and `[[Construct]]` of a bound function (§10.4.1.1,
    /// §10.4.1.2). A loop walks the chain of bound functions (a step of
    /// the countdown per level) and collects the bound arguments, the
    /// innermost first; then one call of the final target replaces the
    /// recursion of the specification. That call is a tail call of
    /// [`Runtime::invoke`].
    fn invoke_bound(&mut self, function: Gc<Object>, site: CallSite) -> VmResult<TailCall> {
        let mut current = function;
        let mut new_target = site.new_target.unwrap_or(function);
        let mut bound_this = Value::Undefined;
        let mut bound_count = 0usize;
        while let Some((target, this, count)) = self.bound_parts(current)? {
            self.tick()?;
            bound_count = bound_count.saturating_add(count);
            bound_this = this;
            // Step 4 of [[Construct]]: a `new.target` that is this bound
            // function becomes its target.
            if new_target == current {
                new_target = target;
            }
            current = target;
        }
        if bound_count.saturating_add(site.argc) > self.vm.stack_limit {
            return Err(VmError::range_error(STACK_OVERFLOW));
        }
        // The copy below is O(bound args + call args): charge it first.
        self.charge(bound_count.saturating_add(site.argc))?;
        let mut args = Vec::with_capacity(bound_count + site.argc);
        self.collect_bound_args(function, &mut args)?;
        args.extend_from_slice(
            self.vm
                .stack
                .get(site.window..site.window + site.argc)
                .ok_or(VmError::invariant("arguments outside the stack"))?,
        );
        Ok(TailCall {
            callee: Value::Object(current),
            this: bound_this,
            args,
            new_target: site.construct.then_some(new_target),
        })
    }

    /// The target, the bound `this` and the number of bound arguments of
    /// a bound function; `None` for any other object.
    fn bound_parts(&self, function: Gc<Object>) -> VmResult<Option<(Gc<Object>, Value, usize)>> {
        Ok(match &self.heap.object(function)?.kind {
            ObjectKind::Bound(bound) => Some((bound.target, bound.this, bound.args.len())),
            _ => None,
        })
    }

    /// Appends the bound arguments of a chain of bound functions, the
    /// innermost function's first.
    fn collect_bound_args(&self, function: Gc<Object>, out: &mut Vec<Value>) -> VmResult<()> {
        let mut chain = Vec::new();
        let mut current = function;
        while let ObjectKind::Bound(bound) = &self.heap.object(current)?.kind {
            chain.push(bound);
            current = bound.target;
        }
        for bound in chain.iter().rev() {
            out.extend_from_slice(&bound.args);
        }
        Ok(())
    }

    /// [`Runtime::enter_function`] of a closure that the caller knows only
    /// by its handle (a tail call).
    fn enter_closure(&mut self, function: Gc<Object>, site: CallSite) -> VmResult<Step> {
        let ObjectKind::Function(closure) = &self.heap.object(function)?.kind else {
            return Err(VmError::invariant("not a closure"));
        };
        let callee = Callee::of(function, closure);
        self.enter_function(callee, site)
    }

    /// Pushes the frame of a closure. A function entry is a step of the
    /// time countdown.
    ///
    /// The frame is built in place: the parameter registers are the
    /// caller's argument slots, the stack grows (or shrinks) to the top of
    /// the new window, and only the registers above the passed arguments
    /// are cleared to `undefined`.
    #[inline]
    fn enter_function(&mut self, callee: Callee, site: CallSite) -> VmResult<Step> {
        self.tick()?;
        let registers = callee.compiled.register_count as usize;
        let window = site.window;
        if self.vm.frames.len() >= self.vm.frame_limit || window + registers > self.vm.stack_limit {
            return Err(VmError::range_error(STACK_OVERFLOW));
        }
        if site.construct || callee.compiled.kind.is_class_constructor() {
            return self.enter_constructor_frame(callee, site);
        }
        let Callee {
            function,
            code,
            compiled,
            this_mode,
            realm,
        } = callee;
        let params = compiled.param_count as usize;
        let this = match this_mode {
            ThisMode::Lexical => Value::Undefined,
            ThisMode::Strict => self.stack_value(window - 1)?,
            // §10.2.1.2: an object stays, `undefined` and `null` become the
            // global object of the function's realm.
            ThisMode::Global => match self.stack_value(window - 1)? {
                this @ Value::Object(_) => this,
                Value::Undefined | Value::Null => match self.vm.realms.get(realm as usize) {
                    Some(record) => Value::Object(record.global),
                    None => return Err(VmError::invariant("a realm index without a realm")),
                },
                this => self.coerce_this(this, realm)?,
            },
        };
        let extra_args = if compiled.keeps_extra_args && site.argc > params {
            self.extra_args(window, params, site.argc)?
        } else {
            Vec::new()
        };
        clear_window(&mut self.vm.stack, window, site.argc.min(params), registers);
        self.vm.frames.push(Frame {
            function: Some(function),
            code,
            compiled,
            pc: 0,
            base: window as u32,
            restore_top: site.restore_top as u32,
            this,
            new_target: Value::Undefined,
            argc: site.argc as u32,
            extra_args,
            ret: site.ret,
            construct: false,
            generator: None,
            realm,
        });
        Ok(Step::Entered)
    }

    /// The arguments beyond the declared parameters, for the frame record.
    #[inline(never)]
    fn extra_args(&self, window: usize, params: usize, argc: usize) -> VmResult<Vec<Value>> {
        self.vm
            .stack
            .get(window + params..window + argc)
            .map(<[Value]>::to_vec)
            .ok_or(VmError::invariant("arguments outside the stack"))
    }

    /// [`Runtime::enter_function`] of a `[[Construct]]`, or of a class
    /// constructor without `new` (an error).
    #[inline(never)]
    fn enter_constructor_frame(&mut self, callee: Callee, site: CallSite) -> VmResult<Step> {
        let Callee {
            function,
            code,
            compiled,
            this_mode: _,
            realm,
        } = callee;
        let params = compiled.param_count as usize;
        let registers = compiled.register_count as usize;
        let window = site.window;
        // (The checks of the stack and the frame limit ran in
        // `enter_function`.)
        let (this, new_target) = if site.construct {
            let new_target = site.new_target.unwrap_or(function);
            // The slot of `this` roots `new.target` while its `prototype`
            // is read (a getter can run script code), then the new object.
            self.set_stack_value(window - 1, new_target.into())?;
            if compiled.kind == CodeKind::DerivedConstructor {
                // §10.2.2 step 6: `this` stays uninitialized until
                // `super(...)` returns.
                (Value::Empty, Value::Object(new_target))
            } else {
                let this = self.construct_this(new_target, realm)?;
                self.set_stack_value(window - 1, this.into())?;
                (Value::Object(this), Value::Object(new_target))
            }
        } else if compiled.kind.is_class_constructor() {
            // §10.2.1 step 2: the error belongs to the caller's frame.
            let name = self.name_text(compiled.name);
            return Err(VmError::type_error(if name.is_empty() {
                "Class constructors cannot be invoked without 'new'".to_owned()
            } else {
                format!("Class constructor {name} cannot be invoked without 'new'")
            }));
        } else {
            return Err(VmError::invariant("a plain call of a constructor frame"));
        };
        let extra_args = if compiled.keeps_extra_args && site.argc > params {
            self.extra_args(window, params, site.argc)?
        } else {
            Vec::new()
        };
        clear_window(&mut self.vm.stack, window, site.argc.min(params), registers);
        self.vm.frames.push(Frame {
            function: Some(function),
            code,
            compiled,
            pc: 0,
            base: window as u32,
            restore_top: site.restore_top as u32,
            this,
            new_target,
            argc: site.argc as u32,
            extra_args,
            ret: site.ret,
            construct: site.construct,
            generator: None,
            realm,
        });
        Ok(Step::Entered)
    }

    /// OrdinaryCreateFromConstructor(newTarget, "%Object.prototype%")
    /// (§10.1.13) for `[[Construct]]` of an ordinary function. The
    /// `prototype` of an ordinary function is a data property, but another
    /// `new.target` (a bound function with a changed prototype chain) can
    /// have a getter; it runs through re-entry.
    fn construct_this(&mut self, new_target: Gc<Object>, realm: u32) -> VmResult<Gc<Object>> {
        let key = PropertyKey::String(self.vm.atoms.prototype);
        let proto = match self.heap.get(new_target, key, new_target.into())? {
            GetResult::Value(Value::Object(proto)) => Some(proto),
            GetResult::Value(_) => None,
            GetResult::CallGetter { getter, receiver } => {
                match self.with_scope(|rt| rt.call(getter, receiver, &[]))? {
                    Value::Object(proto) => Some(proto),
                    _ => None,
                }
            }
            // A proxy on the chain of `new.target` (or `new.target` itself).
            GetResult::Generic => {
                match self.with_scope(|rt| rt.get_value(new_target.into(), key))? {
                    Value::Object(proto) => Some(proto),
                    _ => None,
                }
            }
        };
        let proto = if let Some(proto) = proto {
            proto
        } else {
            let realm = self.function_realm_checked(new_target)?.unwrap_or(realm);
            self.intrinsic(realm, Intrinsic::ObjectPrototype)?
        };
        Ok(self.heap.new_object(Some(proto))?)
    }

    /// `GetFunctionRealm` (§7.3.24) for the function kinds that exist.
    pub(crate) fn function_realm(&self, function: Gc<Object>) -> Option<u32> {
        let mut current = function;
        loop {
            match &self.heap.object(current).ok()?.kind {
                ObjectKind::Function(closure) => return Some(closure.realm),
                ObjectKind::Native(native) => return Some(native.realm),
                ObjectKind::Bound(bound) => current = bound.target,
                // §7.3.24 step 4: the realm of the target (a revoked proxy
                // throws there; callers fall back to the current realm).
                ObjectKind::Proxy(data) => current = data.target?,
                _ => return None,
            }
        }
    }

    /// `GetFunctionRealm` (§7.3.24) with its error: a revoked proxy on the
    /// way is a `TypeError`.
    pub(crate) fn function_realm_checked(&self, function: Gc<Object>) -> VmResult<Option<u32>> {
        let mut current = function;
        loop {
            match &self.heap.object(current)?.kind {
                ObjectKind::Function(closure) => return Ok(Some(closure.realm)),
                ObjectKind::Native(native) => return Ok(Some(native.realm)),
                ObjectKind::Bound(bound) => current = bound.target,
                ObjectKind::Proxy(data) => match (data.target, data.handler) {
                    (Some(target), Some(_)) => current = target,
                    // V8 names no operation here (measured).
                    _ => return Err(crate::vm::proxy::revoked_error("undefined")),
                },
                _ => return Ok(None),
            }
        }
    }

    /// `OrdinaryCallBindThis` for sloppy functions (§10.2.1.2): `undefined`
    /// and `null` become the global object, primitives become objects.
    fn coerce_this(&mut self, this: Value, realm: u32) -> VmResult<Value> {
        match this {
            Value::Undefined | Value::Null => Ok(self.global_object(realm)?.into()),
            Value::Object(_) => Ok(this),
            // `Frame::this` roots the wrapper; no allocation happens before the
            // frame record exists, so it needs no handle.
            _ => Ok(self.to_object(this)?.into()),
        }
    }

    /// Calls a native function with a handle scope open; runs its deferred
    /// call or generator resumption.
    #[inline]
    fn call_native(
        &mut self,
        function: Gc<Object>,
        func: crate::vm::NativeFn,
        realm: u32,
        site: CallSite,
    ) -> VmResult<Step> {
        match self.run_native(function, func, realm, site)? {
            NativeReturn::Value(value) => Ok(Step::Value(native_value(value)?)),
            other => self.native_request(other, site),
        }
    }

    /// The deferred call or generator resumption that a native function
    /// returned (out of line: most natives return a value).
    #[inline(never)]
    fn native_request(&mut self, request: NativeReturn, site: CallSite) -> VmResult<Step> {
        match request {
            NativeReturn::Value(value) => Ok(Step::Value(native_value(value)?)),
            NativeReturn::Call { callee, this, args } => self.invoke_tail_calls(
                site,
                TailCall {
                    callee,
                    this,
                    args,
                    new_target: None,
                },
            ),
            NativeReturn::Construct {
                callee,
                new_target,
                args,
            } => self.invoke_tail_calls(
                site,
                TailCall {
                    callee,
                    this: Value::Undefined,
                    args,
                    new_target: Some(object_new_target(new_target)?),
                },
            ),
            NativeReturn::Resume(resume) => {
                self.resume_generator(resume, site.ret, site.restore_top)
            }
            NativeReturn::Loop(state) => self.start_loop(state, site),
        }
    }

    /// Runs a native function with a handle scope open and returns what it
    /// returned.
    #[inline]
    fn run_native(
        &mut self,
        function: Gc<Object>,
        func: crate::vm::NativeFn,
        realm: u32,
        site: CallSite,
    ) -> VmResult<NativeReturn> {
        let this = if site.construct {
            Value::Undefined
        } else {
            self.stack_value(site.window - 1)?
        };
        let new_target = if site.construct {
            let new_target = site.new_target.unwrap_or(function);
            // The slot of `this` keeps `new.target` alive during the call.
            self.set_stack_value(site.window - 1, new_target.into())?;
            new_target.into()
        } else {
            Value::Undefined
        };
        let call = NativeCall {
            callee: function,
            this,
            new_target,
            start: site.window,
            argc: site.argc,
            realm,
        };
        let depth = self.heap.scope_depth();
        let scope = self.heap.open_scope();
        let result = func(self, &call);
        match result {
            Ok(_) => self.heap.close_scope(scope)?,
            Err(_) => self.heap.close_scopes_to(depth),
        }
        result
    }

    /// Pushes `callee`, `this` and the arguments on top of the value
    /// stack; returns the index of the first argument.
    pub(crate) fn push_window(
        &mut self,
        callee: Value,
        this: Value,
        args: &[Value],
    ) -> VmResult<usize> {
        // The host and the natives build these values; internal variants
        // (`Empty`, `Cell`) must not reach a script.
        if callee.is_internal() || this.is_internal() || args.iter().any(|v| v.is_internal()) {
            return Err(VmError::invariant(
                "an internal value (Empty or Cell) was passed to a call",
            ));
        }
        let stack = &mut self.vm.stack;
        if stack.len() + 2 + args.len() > self.vm.stack_limit {
            return Err(VmError::range_error(STACK_OVERFLOW));
        }
        stack.push(callee);
        stack.push(this);
        let window = stack.len();
        stack.extend_from_slice(args);
        Ok(window)
    }

    /// Pops the top frame after a `return` (§10.2.1.4, §10.2.2 step 10 for
    /// `[[Construct]]`) and delivers its result. Returns the result if it
    /// goes to the host.
    pub(crate) fn return_from_frame(&mut self, value: Value) -> VmResult<Option<Value>> {
        let frame = self
            .vm
            .frames
            .last()
            .ok_or(VmError::invariant("return without a frame"))?;
        let mut value = value;
        if frame.construct && !matches!(value, Value::Object(_)) {
            // A derived constructor's code checks its result itself
            // (`DerivedReturn`), so its uninitialized `this` never gets here.
            value = frame.this;
            if value.is_internal() {
                return Err(VmError::invariant(
                    "a derived constructor returned without an object",
                ));
            }
        }
        if let Some(generator) = frame.generator {
            return self.return_from_generator(generator, value);
        }
        // The record is dropped in place (a `pop` would move it out).
        let (ret, restore_top) = (frame.ret, frame.restore_top as usize);
        let depth = self.vm.frames.len() - 1;
        self.vm.frames.truncate(depth);
        self.vm.stack.resize(restore_top, Value::Undefined);
        self.deliver(ret, value)
    }

    /// [`Runtime::return_from_frame`] of a generator's frame (the result
    /// is `{ value, done: true }`), an async function's frame (the value
    /// resolves the result promise, `async_function.rs`) or an async
    /// generator's frame (the value completes the head request,
    /// `async_generator.rs`).
    #[inline(never)]
    fn return_from_generator(
        &mut self,
        generator: Gc<Object>,
        value: Value,
    ) -> VmResult<Option<Value>> {
        match self.generator_state(generator)?.kind {
            GeneratorKind::Sync => {}
            GeneratorKind::AsyncFunction(_) => return self.async_return(generator, value, false),
            GeneratorKind::AsyncGenerator(_) => {
                return self.async_generator_complete(generator, Ok(value));
            }
        }
        if let Some(ReturnTo::IterValue { dst, .. }) = self.vm.frames.last().map(|f| f.ret) {
            // A step of `for`-`of`: done (the state register says so
            // already), no result object.
            self.complete_generator(generator)?;
            let frame = self
                .vm
                .frames
                .pop()
                .ok_or(VmError::invariant("return without a frame"))?;
            self.vm
                .stack
                .resize(frame.restore_top as usize, Value::Undefined);
            self.set_register(dst, Value::Undefined);
            return Ok(None);
        }
        // The registers of the frame still root `value`.
        let scope = self.heap.open_scope();
        let result = self.iter_result(value, true)?;
        self.heap.record(result);
        self.complete_generator(generator)?;
        let frame = self
            .vm
            .frames
            .pop()
            .ok_or(VmError::invariant("return without a frame"))?;
        self.vm
            .stack
            .resize(frame.restore_top as usize, Value::Undefined);
        self.heap.close_scope(scope)?;
        self.deliver(frame.ret, result.into())
    }

    /// Delivers the result of a frame: into a register of the frame below,
    /// nowhere, to the host (returned), or to the top native loop, which
    /// continues (it can push a frame or deliver its own result).
    // Inlined into every return (measured: 6 instructions per call
    // more as a call, M7 13c).
    #[allow(clippy::inline_always)]
    #[inline(always)]
    pub(crate) fn deliver(&mut self, ret: ReturnTo, value: Value) -> VmResult<Option<Value>> {
        match ret {
            ReturnTo::Register(register) => {
                self.set_register(register, value);
                Ok(None)
            }
            ReturnTo::Discard => Ok(None),
            ReturnTo::Host => Ok(Some(value)),
            ReturnTo::Native | ReturnTo::IterValue { .. } => self.deliver_special(ret, value),
        }
    }

    /// [`Runtime::deliver`] to a native loop or to a step of `for`-`of`
    /// (out of line: the common returns stay small).
    #[inline(never)]
    fn deliver_special(&mut self, ret: ReturnTo, value: Value) -> VmResult<Option<Value>> {
        match ret {
            ReturnTo::IterValue { dst, state } => {
                self.set_register(dst, value);
                self.set_register(state, Value::Int(crate::bytecode::ITERATOR_PENDING));
                Ok(None)
            }
            _ => self.resume_loop(Ok(value)),
        }
    }

    /// Writes a register of the top frame (a result delivered to it).
    #[inline]
    pub(crate) fn set_register(&mut self, register: crate::bytecode::Reg, value: Value) {
        if let Some(frame) = self.vm.frames.last()
            && let Some(slot) = self
                .vm
                .stack
                .get_mut(frame.base as usize + usize::from(register))
        {
            *slot = value;
        }
    }

    /// Pops frames down to `depth` after an uncaught error: generators of
    /// popped frames complete, the value stack returns to the length
    /// before the outermost popped frame, and the native loops that wait
    /// for the popped frames go too.
    pub(crate) fn unwind(&mut self, depth: usize) {
        self.unwind_frames(depth);
        self.drop_loops(depth);
    }

    /// [`Runtime::unwind`] without the native loops.
    pub(crate) fn unwind_frames(&mut self, depth: usize) {
        while self.vm.frames.len() > depth {
            let Some(frame) = self.vm.frames.pop() else {
                break;
            };
            if let Some(generator) = frame.generator {
                // A failure here is an engine bug; the error that is
                // unwinding already ends the script.
                let _ = self.complete_generator(generator);
                let _ = self.abandon_async_generator(generator);
            }
            self.vm
                .stack
                .resize(frame.restore_top as usize, Value::Undefined);
        }
    }

    /// Calls `callee` with `this` and `args` from Rust (a native function
    /// or the host) and returns its result, recorded in the current handle
    /// scope. Script code runs in a nested interpreter loop: a re-entry
    /// that charges the shared recursion budget (ADR 0026 section 9).
    pub fn call(&mut self, callee: Value, this: Value, args: &[Value]) -> VmResult<Value> {
        if self.budget.enter(REENTRY_WEIGHT).is_err() {
            return Err(VmError::range_error(STACK_OVERFLOW));
        }
        let result = self.call_inner(callee, this, args);
        self.budget.leave(REENTRY_WEIGHT);
        let value = result?;
        if self.heap.scope_depth() > 0 {
            self.heap.record(value);
        }
        Ok(value)
    }

    fn call_inner(&mut self, callee: Value, this: Value, args: &[Value]) -> VmResult<Value> {
        let restore_top = self.vm.stack.len();
        let window = self.push_window(callee, this, args)?;
        let site = CallSite {
            window,
            argc: args.len(),
            ret: ReturnTo::Host,
            restore_top,
            construct: false,
            new_target: None,
        };
        match self.invoke(site) {
            Ok(Step::Value(value)) => {
                self.vm.stack.truncate(restore_top);
                Ok(value)
            }
            Ok(Step::Entered) => self.run_entered(restore_top),
            Ok(Step::NotCallable) => {
                self.vm.stack.truncate(restore_top);
                Err(VmError::type_error(format!(
                    "{} is not a function",
                    self.describe_value(callee)
                )))
            }
            Err(error) => {
                self.vm.stack.truncate(restore_top);
                Err(error)
            }
        }
    }

    /// Runs the frame that a call from Rust pushed and returns its result;
    /// the value stack ends at `restore_top` afterwards (a native loop's
    /// frames restore the stack to the loop's window, not to the call's).
    #[inline]
    fn run_entered(&mut self, restore_top: usize) -> VmResult<Value> {
        let result = self.run_frames();
        self.vm.stack.truncate(restore_top);
        result
    }

    /// `Construct(callee, args)` from Rust (a native function or the host):
    /// the new object, recorded in the current handle scope. Like
    /// [`Runtime::call`], script code runs in a nested interpreter loop
    /// that charges the shared recursion budget.
    pub(crate) fn construct(&mut self, callee: Value, args: &[Value]) -> VmResult<Value> {
        if self.budget.enter(REENTRY_WEIGHT).is_err() {
            return Err(VmError::range_error(STACK_OVERFLOW));
        }
        let result = self.construct_inner(callee, args);
        self.budget.leave(REENTRY_WEIGHT);
        let value = result?;
        if self.heap.scope_depth() > 0 {
            self.heap.record(value);
        }
        Ok(value)
    }

    fn construct_inner(&mut self, callee: Value, args: &[Value]) -> VmResult<Value> {
        let restore_top = self.vm.stack.len();
        let window = self.push_window(callee, Value::Undefined, args)?;
        let site = CallSite {
            window,
            argc: args.len(),
            ret: ReturnTo::Host,
            restore_top,
            construct: true,
            new_target: None,
        };
        match self.invoke(site) {
            Ok(Step::Value(value)) => {
                self.vm.stack.truncate(restore_top);
                Ok(value)
            }
            Ok(Step::Entered) => self.run_entered(restore_top),
            Ok(Step::NotCallable) => {
                self.vm.stack.truncate(restore_top);
                Err(VmError::type_error(format!(
                    "{} is not a constructor",
                    self.describe_value(callee)
                )))
            }
            Err(error) => {
                self.vm.stack.truncate(restore_top);
                Err(error)
            }
        }
    }

    /// The value at an absolute index of the value stack.
    pub(crate) fn stack_value(&self, index: usize) -> VmResult<Value> {
        self.vm
            .stack
            .get(index)
            .copied()
            .ok_or(VmError::invariant("a stack index outside the stack"))
    }

    fn set_stack_value(&mut self, index: usize, value: Value) -> VmResult<()> {
        let slot = self
            .vm
            .stack
            .get_mut(index)
            .ok_or(VmError::invariant("a stack index outside the stack"))?;
        *slot = value;
        Ok(())
    }

    /// Replaces argument `index` of a native call (which must have been
    /// passed), for a native that converts its arguments in place before
    /// a second pass over them (`Math.hypot`); the value stack keeps the
    /// new value rooted.
    pub(crate) fn set_arg(
        &mut self,
        call: &NativeCall,
        index: usize,
        value: Value,
    ) -> VmResult<()> {
        if index >= call.argc {
            return Err(VmError::invariant("an argument index outside the call"));
        }
        self.set_stack_value(call.start + index, value)
    }

    /// Argument `index` of a native call (`undefined` if it was not
    /// passed).
    pub fn arg(&self, call: &NativeCall, index: usize) -> Value {
        if index >= call.argc {
            return Value::Undefined;
        }
        self.vm
            .stack
            .get(call.start + index)
            .copied()
            .unwrap_or_default()
    }
}
