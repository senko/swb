//! The interpreter (ADR 0026 section 5, memo 1.1 to 1.3).
//!
//! A runtime has one value stack and one vector of frame records
//! ([`Vm`]). A frame is a window of registers in the value stack; its
//! record holds handles and indices only (the function, the code, the
//! program counter, the base of the window, `this`, `new.target`). A
//! script-to-script call pushes a frame record and continues the same
//! loop ([`interp`]); a return pops it. There is no Rust recursion for
//! such calls, so the frame limit (not the Rust stack) bounds the depth.
//!
//! Native functions run as Rust calls with their arguments in a window of
//! the value stack and a handle scope open. A native function can return a
//! request to call a function (a deferred call) or to resume a generator;
//! the loop runs it as a script-to-script call. A native function that
//! needs a result of script code in the middle of its work re-enters the
//! interpreter through Rust recursion and charges the shared recursion
//! budget ([`call`]).
//!
//! The VM is the root source of the heap: the value stack, the frame
//! records, the realms with their intrinsics and global lexical bindings,
//! and the value that the host reads last. Suspended generator frames are
//! traced through their generator objects.
//!
//! Exceptions (ADR 0026 section 5): an instruction that fails leaves the
//! loop with the error; [`Runtime::run_frames`] searches the handler
//! tables of the frames from the top down and continues at the handler,
//! or pops the frames and returns the error to its Rust caller (a native
//! function on the way gets it as the result of its call). A termination
//! skips every handler.
//!
//! Time (ADR 0026 section 9): a countdown at backward jumps, function
//! entries, generator resumptions and in long built-in loops; at zero the
//! time check compares the clock with the host's deadline and reads the
//! termination request of the host.

mod call;
pub(crate) mod convert;
mod function;
mod generator;
mod interp;
pub(crate) mod number;
mod property;
mod realm;
pub(crate) mod time;

use std::borrow::Cow;
use std::rc::Rc;

pub use function::{
    Closure, NativeCall, NativeFn, NativeFunction, NativeReturn, Resume, ResumeMode,
};
pub use generator::GeneratorState;
pub(crate) use property::SetOutcome;
pub(crate) use realm::{Intrinsic, Realm, error_prototype};
pub(crate) use time::TIME_CHECK_INTERVAL;
pub use time::TerminationHandle;

use crate::bytecode::{FunctionCode, Reg};
use crate::error::{Error, InternalError, Termination, ThrowKind};
use crate::heap::{Gc, Generic, RootSource, Tracer};
use crate::object::Object;
use crate::string::JsString;
use crate::value::Value;

/// The default limit of the frame vector (ADR 0026 section 9).
pub const DEFAULT_FRAME_LIMIT: usize = 10_000;

/// The default limit of the value stack, in values (64 MiB).
pub const DEFAULT_STACK_LIMIT: usize = 1 << 22;

/// The recursion budget weight of one native re-entry into the
/// interpreter: the Rust frames of the native call, the re-entry and the
/// interpreter loop. Measured: 6,688 bytes per level in a debug build
/// (`opt-level = 1`), 3,664 in a release build; a margin of 1.5. With the
/// default budget (4 MiB), about 400 re-entries nest.
pub(crate) const REENTRY_WEIGHT: u32 = 10 * 1024;

/// The message of the `RangeError` for too deep recursion.
pub(crate) const STACK_OVERFLOW: &str = "Maximum call stack size exceeded";

/// Where the result of a frame goes when the frame returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReturnTo {
    /// A register of the calling frame.
    Register(Reg),
    /// Nowhere (a setter call).
    Discard,
    /// Back to the Rust caller of the interpreter loop.
    Host,
}

/// A frame record (ADR 0026 section 5).
#[derive(Debug)]
pub(crate) struct Frame {
    /// The function; `None` for the code of a script.
    pub(crate) function: Option<Gc<Object>>,
    /// The code object, which keeps the code's constants alive.
    pub(crate) code: Gc<Generic>,
    /// The code itself (shared with the code object).
    pub(crate) compiled: Rc<FunctionCode>,
    /// The next instruction (written back before calls and errors).
    pub(crate) pc: u32,
    /// The first register of the window in the value stack.
    pub(crate) base: u32,
    /// The length of the value stack after the frame returns.
    pub(crate) restore_top: u32,
    /// The `this` value after `OrdinaryCallBindThis`.
    pub(crate) this: Value,
    /// `new.target`.
    pub(crate) new_target: Value,
    /// The number of arguments that the caller passed.
    pub(crate) argc: u32,
    /// The arguments beyond the declared parameters, kept only when the
    /// code uses `arguments`.
    pub(crate) extra_args: Vec<Value>,
    /// Where the result goes.
    pub(crate) ret: ReturnTo,
    /// Whether the frame runs a `[[Construct]]`: a result that is not an
    /// object is replaced by `this`.
    pub(crate) construct: bool,
    /// The generator object whose frame this is.
    pub(crate) generator: Option<Gc<Object>>,
    /// The realm of the function.
    pub(crate) realm: u32,
}

impl Frame {
    /// Reports the frame's handles (not its registers, which are in the
    /// value stack or in a generator's saved window).
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        if let Some(function) = self.function {
            tracer.object(function);
        }
        tracer.generic(self.code);
        tracer.value(self.this);
        tracer.value(self.new_target);
        for value in &self.extra_args {
            tracer.value(*value);
        }
        if let Some(generator) = self.generator {
            tracer.object(generator);
        }
    }
}

/// An abrupt completion inside the VM.
#[derive(Clone, Debug, PartialEq)]
pub enum VmError {
    /// A thrown JavaScript value. It is not rooted while it propagates;
    /// nothing on the way to its handler (or to the host) collects.
    Throw(Value),
    /// An error object of this kind to create in the current realm when a
    /// handler (or the host) needs it.
    Raise {
        /// The constructor.
        kind: ThrowKind,
        /// The message.
        message: Cow<'static, str>,
    },
    /// The script ends without running any handler (ADR 0026 section 5).
    Terminated(Termination),
    /// An engine bug; the script ends and the host logs it.
    Internal(InternalError),
}

/// The result type of the VM.
pub type VmResult<T> = Result<T, VmError>;

impl VmError {
    /// A `TypeError` with a message.
    pub fn type_error(message: impl Into<Cow<'static, str>>) -> Self {
        VmError::Raise {
            kind: ThrowKind::TypeError,
            message: message.into(),
        }
    }

    /// A `ReferenceError` with a message.
    pub fn reference_error(message: impl Into<Cow<'static, str>>) -> Self {
        VmError::Raise {
            kind: ThrowKind::ReferenceError,
            message: message.into(),
        }
    }

    /// A `RangeError` with a message.
    pub fn range_error(message: impl Into<Cow<'static, str>>) -> Self {
        VmError::Raise {
            kind: ThrowKind::RangeError,
            message: message.into(),
        }
    }

    /// A broken internal invariant.
    pub(crate) fn invariant(message: &'static str) -> Self {
        VmError::Internal(InternalError::Invariant(message))
    }
}

impl From<Error> for VmError {
    fn from(error: Error) -> Self {
        match error {
            Error::Internal(internal) => VmError::Internal(internal),
            Error::Terminated(termination) => VmError::Terminated(termination),
            Error::Throw { kind, message } => VmError::Raise {
                kind,
                message: Cow::Borrowed(message),
            },
        }
    }
}

impl From<Termination> for VmError {
    fn from(termination: Termination) -> Self {
        VmError::Terminated(termination)
    }
}

/// Atoms that the VM uses; created with the runtime and always alive.
pub(crate) struct Atoms {
    pub(crate) prototype: Gc<JsString>,
    pub(crate) constructor: Gc<JsString>,
    pub(crate) name: Gc<JsString>,
    pub(crate) message: Gc<JsString>,
    pub(crate) value: Gc<JsString>,
    pub(crate) done: Gc<JsString>,
    pub(crate) callee: Gc<JsString>,
    pub(crate) value_of: Gc<JsString>,
    pub(crate) to_string: Gc<JsString>,
    pub(crate) empty: Gc<JsString>,
    pub(crate) undefined: Gc<JsString>,
    pub(crate) null: Gc<JsString>,
    pub(crate) object: Gc<JsString>,
    pub(crate) boolean: Gc<JsString>,
    pub(crate) number: Gc<JsString>,
    pub(crate) string: Gc<JsString>,
    pub(crate) symbol: Gc<JsString>,
    pub(crate) bigint: Gc<JsString>,
    pub(crate) function: Gc<JsString>,
    pub(crate) true_: Gc<JsString>,
    pub(crate) false_: Gc<JsString>,
}

impl Atoms {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        for atom in [
            self.prototype,
            self.constructor,
            self.name,
            self.message,
            self.value,
            self.done,
            self.callee,
            self.value_of,
            self.to_string,
            self.empty,
            self.undefined,
            self.null,
            self.object,
            self.boolean,
            self.number,
            self.string,
            self.symbol,
            self.bigint,
            self.function,
            self.true_,
            self.false_,
        ] {
            tracer.string(atom);
        }
    }
}

/// The VM state outside the heap: the roots of the collector.
pub(crate) struct Vm {
    /// The value stack: the register windows of all frames.
    pub(crate) stack: Vec<Value>,
    /// The frame records, innermost last.
    pub(crate) frames: Vec<Frame>,
    /// The realms (ADR 0026 section 12).
    pub(crate) realms: Vec<Realm>,
    pub(crate) atoms: Atoms,
    /// The most frame records.
    pub(crate) frame_limit: usize,
    /// The most values in the value stack.
    pub(crate) stack_limit: usize,
    /// The last result or uncaught exception that the host received; it
    /// stays alive until the next one.
    pub(crate) last_value: Value,
    /// The source offset where the current uncaught error was thrown.
    pub(crate) error_offset: Option<u32>,
    /// The countdown to the next time check.
    pub(crate) countdown: u32,
    /// The deadline of the running task (set by the host).
    pub(crate) deadline: Option<std::time::Instant>,
    /// The termination request of the host (another thread can set it).
    pub(crate) termination: TerminationHandle,
    /// The arrays that `Array.prototype.join` is joining (V8's cycle
    /// detection: a nested join of one of them gives the empty string).
    pub(crate) join_stack: Vec<Gc<Object>>,
}

impl RootSource for Vm {
    fn trace_roots(&self, tracer: &mut Tracer<'_>) {
        for value in &self.stack {
            tracer.value(*value);
        }
        for frame in &self.frames {
            frame.trace(tracer);
        }
        for realm in &self.realms {
            realm.trace(tracer);
        }
        self.atoms.trace(tracer);
        tracer.value(self.last_value);
        for object in &self.join_stack {
            tracer.object(*object);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The record sizes that the spike's measurements assume; a change
    /// shows up here first.
    #[test]
    fn record_sizes() {
        assert_eq!(size_of::<Frame>(), 128);
        assert_eq!(size_of::<FunctionCode>(), 168);
        assert_eq!(size_of::<crate::bytecode::Constant>(), 24);
        assert_eq!(size_of::<GeneratorState>(), 160);
        assert_eq!(size_of::<Closure>(), 40);
    }
}
