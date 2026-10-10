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

use crate::bytecode::ThisMode;
use crate::heap::Gc;
use crate::object::{GetResult, Object, ObjectKind};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::function::is_constructor_kind;
use crate::vm::{
    Frame, Intrinsic, NativeCall, NativeReturn, REENTRY_WEIGHT, ReturnTo, STACK_OVERFLOW, VmError,
    VmResult,
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

impl Runtime {
    /// Starts the call described by `site` (§10.2.1 `[[Call]]`, §10.2.2
    /// `[[Construct]]` for ordinary functions, §10.3.1 for natives).
    pub(crate) fn invoke(&mut self, site: CallSite) -> VmResult<Step> {
        let callee = self.stack_value(site.window.wrapping_sub(2))?;
        let Value::Object(function) = callee else {
            return Ok(Step::NotCallable);
        };
        let record = self.heap.object(function)?;
        if site.construct && !is_constructor_kind(&record.kind) {
            return Ok(Step::NotCallable);
        }
        match &record.kind {
            ObjectKind::Function(_) => self.enter_function(function, site),
            ObjectKind::Native(native) => {
                let (func, realm) = (native.func, native.realm);
                self.call_native(function, func, realm, site)
            }
            ObjectKind::Bound(_) => self.invoke_bound(function, site),
            _ => Ok(Step::NotCallable),
        }
    }

    /// `[[Call]]` and `[[Construct]]` of a bound function (§10.4.1.1,
    /// §10.4.1.2). A loop walks the chain of bound functions (a step of
    /// the countdown per level) and collects the bound arguments, the
    /// innermost first; then one call of the final target replaces the
    /// recursion of the specification. The call goes through a new window
    /// at the top of the value stack, as a deferred call does.
    fn invoke_bound(&mut self, function: Gc<Object>, site: CallSite) -> VmResult<Step> {
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
        let nt = site.construct.then_some(Value::Object(new_target));
        self.invoke_deferred(Value::Object(current), bound_this, &args, nt, site)
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

    /// Calls (or constructs, with `new_target`) `callee` through a new
    /// window at the top of the value stack. The result of a frame that
    /// this call pushes goes where `site` says.
    fn invoke_deferred(
        &mut self,
        callee: Value,
        this: Value,
        args: &[Value],
        new_target: Option<Value>,
        site: CallSite,
    ) -> VmResult<Step> {
        let new_target = match new_target {
            None => None,
            Some(Value::Object(object)) => Some(object),
            Some(_) => return Err(VmError::invariant("new.target is not an object")),
        };
        let window = self.push_window(callee, this, args)?;
        let deferred = CallSite {
            window,
            argc: args.len(),
            ret: site.ret,
            restore_top: site.restore_top,
            construct: new_target.is_some(),
            new_target,
        };
        match self.invoke(deferred)? {
            Step::Value(value) => {
                self.vm.stack.truncate(window - 2);
                Ok(Step::Value(value))
            }
            Step::NotCallable => {
                let what = if new_target.is_some() {
                    "a constructor"
                } else {
                    "a function"
                };
                Err(VmError::type_error(format!(
                    "{} is not {what}",
                    self.describe_value(callee)
                )))
            }
            Step::Entered => Ok(Step::Entered),
        }
    }

    /// Pushes the frame of a closure. A function entry is a step of the
    /// time countdown.
    fn enter_function(&mut self, function: Gc<Object>, site: CallSite) -> VmResult<Step> {
        self.tick()?;
        if self.vm.frames.len() >= self.vm.frame_limit {
            return Err(VmError::range_error(STACK_OVERFLOW));
        }
        let ObjectKind::Function(closure) = &self.heap.object(function)?.kind else {
            return Err(VmError::invariant("not a closure"));
        };
        let code = closure.code;
        let compiled = std::rc::Rc::clone(&closure.compiled);
        let this_mode = closure.this_mode;
        let realm = closure.realm;
        let params = compiled.param_count as usize;
        let registers = compiled.register_count as usize;
        let window = site.window;
        if window + registers > self.vm.stack_limit {
            return Err(VmError::range_error(STACK_OVERFLOW));
        }
        let (this, new_target) = if site.construct {
            let new_target = site.new_target.unwrap_or(function);
            // The slot of `this` roots `new.target` while its `prototype`
            // is read (a getter can run script code), then the new object.
            self.set_stack_value(window - 1, new_target.into())?;
            let this = self.construct_this(new_target, realm)?;
            self.set_stack_value(window - 1, this.into())?;
            (Value::Object(this), Value::Object(new_target))
        } else {
            let this = self.stack_value(window - 1)?;
            let this = match this_mode {
                ThisMode::Lexical => Value::Undefined,
                ThisMode::Strict => this,
                ThisMode::Global => self.coerce_this(this, realm)?,
            };
            (this, Value::Undefined)
        };
        let extra_args = if compiled.uses_arguments && site.argc > params {
            self.vm
                .stack
                .get(window + params..window + site.argc)
                .map(<[Value]>::to_vec)
                .ok_or(VmError::invariant("arguments outside the stack"))?
        } else {
            Vec::new()
        };
        let stack = &mut self.vm.stack;
        stack.truncate(window + site.argc.min(params));
        stack.resize(window + registers, Value::Undefined);
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
        };
        let proto = if let Some(proto) = proto {
            proto
        } else {
            let realm = self.function_realm(new_target).unwrap_or(realm);
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
                _ => return None,
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
    fn call_native(
        &mut self,
        function: Gc<Object>,
        func: crate::vm::NativeFn,
        realm: u32,
        site: CallSite,
    ) -> VmResult<Step> {
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
        match result? {
            NativeReturn::Value(value) => {
                if value.is_internal() {
                    return Err(VmError::invariant("a native function returned Empty"));
                }
                Ok(Step::Value(value))
            }
            NativeReturn::Call { callee, this, args } => {
                self.invoke_deferred(callee, this, &args, None, site)
            }
            NativeReturn::Construct {
                callee,
                new_target,
                args,
            } => self.invoke_deferred(callee, Value::Undefined, &args, Some(new_target), site),
            NativeReturn::Resume(resume) => {
                self.resume_generator(resume, site.ret, site.restore_top)
            }
        }
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
            value = frame.this;
        }
        let scope = if let Some(generator) = frame.generator {
            // The registers of the frame still root `value`.
            let scope = self.heap.open_scope();
            let result = self.iter_result(value, true)?;
            value = self.heap.record(result).into();
            self.complete_generator(generator)?;
            Some(scope)
        } else {
            None
        };
        let frame = self
            .vm
            .frames
            .pop()
            .ok_or(VmError::invariant("return without a frame"))?;
        self.vm
            .stack
            .resize(frame.restore_top as usize, Value::Undefined);
        if let Some(scope) = scope {
            self.heap.close_scope(scope)?;
        }
        Ok(self.deliver(frame.ret, value))
    }

    /// Delivers the result of a frame: into a register of the frame below,
    /// nowhere, or to the host (returned).
    pub(crate) fn deliver(&mut self, ret: ReturnTo, value: Value) -> Option<Value> {
        match ret {
            ReturnTo::Register(register) => {
                if let Some(frame) = self.vm.frames.last()
                    && let Some(slot) = self
                        .vm
                        .stack
                        .get_mut(frame.base as usize + usize::from(register))
                {
                    *slot = value;
                }
                None
            }
            ReturnTo::Discard => None,
            ReturnTo::Host => Some(value),
        }
    }

    /// Pops frames down to `depth` after an uncaught error: generators of
    /// popped frames complete, the value stack returns to the length
    /// before the outermost popped frame.
    pub(crate) fn unwind(&mut self, depth: usize) {
        while self.vm.frames.len() > depth {
            let Some(frame) = self.vm.frames.pop() else {
                break;
            };
            if let Some(generator) = frame.generator {
                // A failure here is an engine bug; the error that is
                // unwinding already ends the script.
                let _ = self.complete_generator(generator);
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
            Ok(Step::Entered) => self.run_frames(),
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
            Ok(Step::Entered) => self.run_frames(),
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
