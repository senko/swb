//! Native loops (ADR 0026 sections 5 and 15, memo 1.2 "resumable native
//! functions"): natives that call script functions in a loop without a
//! nested interpreter loop for each call.
//!
//! A native function that calls script code for its elements (the
//! callback of `forEach`, the comparator of `sort`, the replacer of
//! `replace`) returns its state as a [`NativeLoop`] instead of calling
//! [`Runtime::call`]. The interpreter drives the state: a step pushes a
//! call window ([`Runtime::push_call`]) or gives the native's result. A
//! call of a script function pushes its frame in the running interpreter
//! loop with [`ReturnTo::Native`], and the return of that frame runs the
//! next step with the result. So a callback costs a script-to-script call
//! and a step, not an entry of the interpreter.
//!
//! The records of the running loops are a stack of the VM ([`LoopRecord`]):
//! a record sits between the frame that called the native (below) and the
//! frames of its calls (above); its `depth` is the number of frames below
//! it. A frame that returns with [`ReturnTo::Native`] always resumes the
//! top record, because a frame finishes before the frames below it.
//!
//! - Registers: a loop keeps the heap values that it needs from one step
//!   to the next in registers, a window of the value stack above the
//!   native's arguments (like a frame's registers), so a collection sees
//!   them and a step needs no rooting of its state. A state may keep
//!   copies of the handles in its registers in Rust fields. The calls'
//!   windows go above the registers.
//! - Exceptions: [`Runtime::run_frames`] gives an exception that leaves
//!   the frames above a record to the record's `on_error` (the default
//!   passes it on; a loop that must close an iterator or settle a promise
//!   would do it there). Records above a handler frame go with the frames
//!   ([`Runtime::unwind`]). A record whose step runs, or whose step's
//!   native callee runs in Rust, belongs to an outer run of the
//!   interpreter: a re-entry from there runs above it and must not give
//!   its exceptions to it.
//! - Limits: each step ticks the countdown; the call of a script function
//!   checks the frame limit. A native callee that starts its own loop
//!   recurses in Rust (`drive` → `invoke` → `start_loop`), so the call of
//!   a callee charges the recursion budget ([`LOOP_WEIGHT`]) while it
//!   starts. The return of a loop's result to the loop below it is a loop,
//!   not a recursion.
//! - Stack traces: a native loop has no frame, as a native call has none,
//!   so the text of `stack` is the same as with a nested entry.

use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::call::{CallSite, Step};
use crate::vm::{
    LoopState, NativeReturn, REENTRY_WEIGHT, ReturnTo, STACK_OVERFLOW, VmError, VmResult,
};

/// The recursion budget weight of starting a native callee from a step:
/// the Rust frames of `drive`, `invoke`, the native and its first step.
/// The same as a re-entry (a native loop that a step starts can start
/// another one).
pub(crate) const LOOP_WEIGHT: u32 = REENTRY_WEIGHT;

/// What a step of a native loop asks for.
pub(crate) enum LoopStep {
    /// The loop is done: the result of the native call.
    Done(Value),
    /// The step pushed a call window with [`Runtime::push_call`]; the
    /// call's result is the input of the next step.
    Call,
}

/// The state of a native loop.
pub(crate) trait NativeLoop {
    /// The next step after the call that the last step requested returned
    /// `input` (`undefined` for the first step). `regs` is the index of the
    /// loop's first register in the value stack. `input` is recorded in
    /// the step's handle scope.
    fn step(&mut self, rt: &mut Runtime, regs: usize, input: Value) -> VmResult<LoopStep>;

    /// The next step after the call that the last step requested threw
    /// (`error` is a `Throw` or a `Raise`). The default passes it on.
    fn on_error(&mut self, _rt: &mut Runtime, _regs: usize, error: VmError) -> VmResult<LoopStep> {
        Err(error)
    }
}

/// A native loop on the stack of loops: its state (taken out while a step
/// runs), its registers, where its result goes, the length of the value
/// stack after it, and the number of frames below it.
pub(crate) struct LoopRecord {
    state: Option<Box<dyn NativeLoop>>,
    /// Whether the call of a step starts (a native callee runs in Rust,
    /// and its re-entries are runs of the interpreter above this loop).
    calling: bool,
    regs: usize,
    ret: ReturnTo,
    restore_top: usize,
    pub(crate) depth: usize,
}

impl LoopRecord {
    /// Whether the loop waits for a frame (neither its step nor the start
    /// of its call runs in Rust).
    fn waits(&self) -> bool {
        self.state.is_some() && !self.calling
    }
}

/// The input of a step: a call's result or its exception.
type Input = Result<Value, VmError>;

impl Runtime {
    /// Starts the native loop `state` with its registers that a native call
    /// (`site`) returned: runs its steps until one calls a script function
    /// ([`Step::Entered`]; the record waits on the stack of loops) or the
    /// loop is done ([`Step::Value`]; the value stack is cut to
    /// `site.restore_top`).
    pub(crate) fn start_loop(&mut self, start: LoopState, site: CallSite) -> VmResult<Step> {
        let LoopState { state, regs } = start;
        self.vm.loops.push(LoopRecord {
            state: Some(state),
            calling: false,
            regs,
            ret: site.ret,
            restore_top: site.restore_top,
            depth: self.vm.frames.len(),
        });
        self.drive(Ok(Value::Undefined))
    }

    /// Runs the steps of the top loop record from `input`. A step's call of
    /// a native function that returns at once continues the loop here; a
    /// pushed frame leaves the record waiting ([`Step::Entered`]). When the
    /// loop is done or fails, its record goes and the value stack is cut
    /// to its `restore_top`.
    fn drive(&mut self, mut input: Input) -> VmResult<Step> {
        loop {
            let call_top = match self.loop_step(input) {
                Ok((LoopStep::Call, call_top)) => call_top,
                Ok((LoopStep::Done(value), _)) => {
                    self.end_loop();
                    return Ok(Step::Value(value));
                }
                Err(error) => {
                    self.end_loop();
                    return Err(error);
                }
            };
            let window = call_top + 2;
            let site = CallSite {
                window,
                argc: self.vm.stack.len().saturating_sub(window),
                ret: ReturnTo::Native,
                restore_top: call_top,
                construct: false,
                new_target: None,
            };
            if self.budget.enter(LOOP_WEIGHT).is_err() {
                self.end_loop();
                return Err(VmError::range_error(STACK_OVERFLOW));
            }
            let index = self.vm.loops.len() - 1;
            self.set_calling(index, true);
            let step = self.invoke(site);
            self.set_calling(index, false);
            self.budget.leave(LOOP_WEIGHT);
            input = match step {
                Ok(Step::Entered) => return Ok(Step::Entered),
                Ok(Step::Value(value)) => Ok(value),
                Ok(Step::NotCallable) => {
                    let callee = self.stack_value(call_top)?;
                    Err(self.not_callable(callee))
                }
                Err(error @ (VmError::Throw(_) | VmError::Raise { .. })) => Err(error),
                Err(error) => {
                    self.end_loop();
                    return Err(error);
                }
            };
            self.vm.stack.truncate(call_top);
        }
    }

    /// Marks the loop record `index` while its call starts.
    #[inline]
    fn set_calling(&mut self, index: usize, calling: bool) {
        if let Some(record) = self.vm.loops.get_mut(index) {
            record.calling = calling;
        }
    }

    /// Removes the top loop record and its registers and windows.
    fn end_loop(&mut self) {
        if let Some(record) = self.vm.loops.pop() {
            self.vm.stack.truncate(record.restore_top);
        }
    }

    /// One step of the top loop record, in a handle scope of its own that
    /// roots the input while the step runs. Returns the step and the length
    /// of the value stack before it (where a call's window starts).
    #[inline]
    fn loop_step(&mut self, input: Input) -> VmResult<(LoopStep, usize)> {
        self.tick()?;
        let record = self
            .vm
            .loops
            .last_mut()
            .ok_or(VmError::invariant("a step without a native loop"))?;
        let regs = record.regs;
        let mut state = record
            .state
            .take()
            .ok_or(VmError::invariant("a native loop that runs twice"))?;
        let call_top = self.vm.stack.len();
        let scope = self.heap.open_scope();
        let step = match input {
            Ok(value) => {
                self.heap.record(value);
                state.step(self, regs, value)
            }
            Err(error) => {
                self.hold_error(&error);
                state.on_error(self, regs, error)
            }
        };
        // Loops that the step started and finished are gone, so the top
        // record is this loop's again.
        if let Some(record) = self.vm.loops.last_mut() {
            record.state = Some(state);
        }
        let step = step?;
        self.heap.close_scope(scope)?;
        Ok((step, call_top))
    }

    /// Pushes a call window (`callee`, `this`, the arguments) for a step of
    /// a native loop, which then returns [`LoopStep::Call`].
    ///
    /// The values come from the heap and from calls, which never give the
    /// internal values (`Empty`, `Cell`); a debug build checks it.
    #[inline]
    pub(crate) fn push_call(&mut self, callee: Value, this: Value, args: &[Value]) -> VmResult<()> {
        debug_assert!(
            !(callee.is_internal() || this.is_internal() || args.iter().any(|v| v.is_internal())),
            "an internal value (Empty or Cell) was passed to a call"
        );
        let stack = &mut self.vm.stack;
        if stack.len() + 2 + args.len() > self.vm.stack_limit {
            return Err(VmError::range_error(STACK_OVERFLOW));
        }
        stack.extend_from_slice(&[callee, this]);
        stack.extend_from_slice(args);
        Ok(())
    }

    /// The result of a native function that runs `state` as a native
    /// loop with `registers` (pushed on the value stack here, as the last
    /// action of the native).
    pub(crate) fn native_loop(
        &mut self,
        state: impl NativeLoop + 'static,
        registers: &[Value],
    ) -> VmResult<NativeReturn> {
        if registers.iter().any(|value| value.is_internal()) {
            return Err(VmError::invariant("an internal value in a loop register"));
        }
        if self.vm.stack.len() + registers.len() > self.vm.stack_limit {
            return Err(VmError::range_error(STACK_OVERFLOW));
        }
        let regs = self.vm.stack.len();
        self.vm.stack.extend_from_slice(registers);
        Ok(NativeReturn::Loop(LoopState {
            state: Box::new(state),
            regs,
        }))
    }

    /// Sets register `index` of a native loop.
    #[inline]
    pub(crate) fn set_loop_reg(&mut self, regs: usize, index: usize, value: Value) -> VmResult<()> {
        let slot = self
            .vm
            .stack
            .get_mut(regs + index)
            .ok_or(VmError::invariant("a loop register outside the stack"))?;
        *slot = value;
        Ok(())
    }

    /// Continues the top loop record with the result of the frame that
    /// returned to it (or with its exception). A loop that is done gives
    /// its result to its own `ret`, and the next loop below continues if
    /// that is [`ReturnTo::Native`]. Returns the result if it goes to the
    /// host.
    pub(crate) fn resume_loop(&mut self, mut input: Input) -> VmResult<Option<Value>> {
        loop {
            let ret = match self.vm.loops.last() {
                Some(record) => record.ret,
                None => return Err(VmError::invariant("a return to a native loop without one")),
            };
            let value = match self.drive(input)? {
                Step::Entered => return Ok(None),
                Step::Value(value) => value,
                Step::NotCallable => return Err(VmError::invariant("a loop that is not callable")),
            };
            match ret {
                ReturnTo::Native => input = Ok(value),
                ret => return self.deliver(ret, value),
            }
        }
    }

    /// The depth of the top loop record if it waits for a frame above
    /// `lowest` (the lowest frame of a run of the interpreter) and above
    /// the handler frame `handler`: an exception in those frames reaches
    /// the loop first.
    ///
    /// A record whose step runs (its state is out) or whose call starts
    /// belongs to an outer run: a run of the interpreter that its step or
    /// its native callee started (a re-entry) is above it.
    pub(crate) fn catching_loop(&self, lowest: usize, handler: Option<usize>) -> Option<usize> {
        let record = self.vm.loops.last()?;
        let above_handler = handler.is_none_or(|index| record.depth > index);
        (record.waits() && record.depth >= lowest && above_handler).then_some(record.depth)
    }

    /// Gives `error` to the top loop record after the frames above it are
    /// gone. Returns the result if the loop is done and its result goes to
    /// the host, `None` if the interpreter continues (a frame was pushed or
    /// a result was delivered), or the error that the loop passes on.
    pub(crate) fn loop_error(&mut self, error: VmError) -> VmResult<Option<Value>> {
        self.resume_loop(Err(error))
    }

    /// Removes the loop records whose calls are in frames at `depth` or
    /// above (after an uncaught error, for [`Runtime::unwind`]). A record
    /// whose step or call start runs stays: the frames of a re-entry from
    /// it are above it, and its own `drive` ends it.
    pub(crate) fn drop_loops(&mut self, depth: usize) {
        while self
            .vm
            .loops
            .last()
            .is_some_and(|record| record.depth >= depth && record.waits())
        {
            self.vm.loops.pop();
        }
    }
}
