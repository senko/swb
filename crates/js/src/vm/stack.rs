//! The stack of error objects (ADR 0026 sections 12 and 15): the capture
//! of the frames and the `stack` accessor, in the form of Chromium.
//!
//! An error object has an own accessor property `stack` (not enumerable,
//! configurable) whose getter and setter are shared by all errors of a
//! realm. Creating an error captures the top frames of the VM as plain
//! data ([`ErrorData`] in the object kind): per frame the function, the
//! code, the source offset of the call or throw, the receiver and
//! whether it was a `[[Construct]]`. At most `Error.stackTraceLimit`
//! frames are captured, and never more than [`MAX_FRAMES`], so an error
//! costs a bounded amount whatever the depth of the recursion. The text
//! is built on the first read of `stack` ([`stack_text`](super::stack_text)),
//! as Chromium does: the first line comes from the `name` and `message`
//! at that time.
//!
//! `Error.captureStackTrace(object, constructorOpt)` captures the same
//! for any object: an error object gets new frames; another object gets
//! a data property with the text built at once.

use std::rc::Rc;

use crate::bytecode::FunctionCode;
use crate::heap::{Gc, Generic, Tracer};
use crate::object::{Object, ObjectKind, Property, PropertyDescriptor};
use crate::runtime::Runtime;
use crate::string::PropertyKey;
use crate::value::Value;
use crate::vm::{Intrinsic, NativeCall, NativeReturn, VmError, VmResult};

/// The most frames that one error captures, whatever the value of
/// `Error.stackTraceLimit` (Chromium has no such limit; its default
/// is 10).
pub(crate) const MAX_FRAMES: usize = 200;

/// The most prototypes that the search for the holder of the captured
/// stack walks.
const HOLDER_CHAIN_LIMIT: usize = 1_000;

/// One captured frame.
pub struct StackFrame {
    /// The function; `None` for the code of a script.
    pub(crate) function: Option<Gc<Object>>,
    /// The code object, which keeps the code's constants alive.
    pub(crate) code: Gc<Generic>,
    /// The code (shared with the code object).
    pub(crate) compiled: Rc<FunctionCode>,
    /// The source offset of the call or throw in this frame.
    pub(crate) offset: Option<u32>,
    /// The receiver of the call.
    pub(crate) this: Value,
    /// Whether the frame runs a `[[Construct]]`.
    pub(crate) construct: bool,
}

/// The stack of an error object: the captured frames until the text is
/// built, then the value of `stack`.
pub struct ErrorData {
    /// The captured frames, innermost first. Empty once the text exists.
    pub(crate) frames: Box<[StackFrame]>,
    /// The value of `stack`: `Empty` until it is read or assigned.
    pub(crate) stack: Value,
    /// Whether the text is being built (a getter that reads `stack`
    /// again gets `undefined`).
    pub(crate) building: bool,
}

impl ErrorData {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for frame in &self.frames {
            if let Some(function) = frame.function {
                tracer.object(function);
            }
            tracer.generic(frame.code);
            tracer.value(frame.this);
        }
        tracer.value(self.stack);
    }

    pub(crate) fn heap_size(&self) -> usize {
        size_of::<ErrorData>() + self.frames.len() * size_of::<StackFrame>()
    }
}

/// The receiver of a frame as the caller passed it. An arrow function
/// has no `this` of its own (the frame record has `undefined`), but
/// Chromium shows the receiver of the call (`Object.f` for an arrow
/// stored in the property `f`); the caller's slot still holds it.
fn receiver_of(frame: &crate::vm::Frame, stack: &[Value]) -> Value {
    if frame.compiled.this_mode == crate::bytecode::ThisMode::Lexical
        && !frame.construct
        && let Some(slot) = (frame.base as usize).checked_sub(1)
        && let Some(&this) = stack.get(slot)
    {
        return this;
    }
    frame.this
}

impl Runtime {
    /// The number of frames to capture: `Error.stackTraceLimit` of the
    /// current realm when it is a data property that holds a number
    /// (clamped to `0..=MAX_FRAMES`); `None` otherwise, as in Chromium.
    fn stack_trace_limit(&self) -> VmResult<Option<usize>> {
        let constructor = self.intrinsic(self.current_realm(), Intrinsic::ErrorConstructor)?;
        let key = PropertyKey::String(self.vm.atoms.stack_trace_limit);
        let Some(Property::Data { value, .. }) = self.heap.get_own_property(constructor, key)?
        else {
            return Ok(None);
        };
        Ok(value.as_number().map(|n| {
            if n.is_nan() || n <= 0.0 {
                0
            } else {
                n.min(MAX_FRAMES as f64) as usize
            }
        }))
    }

    /// Captures the top frames. With `skip_through`, the frames above
    /// and including the innermost call of that function are left out
    /// (`Error.captureStackTrace`); if it is not on the stack, nothing
    /// is captured. `None` if `Error.stackTraceLimit` is not a number.
    pub(crate) fn capture_stack(
        &mut self,
        skip_through: Option<Gc<Object>>,
    ) -> VmResult<Option<Box<ErrorData>>> {
        let Some(limit) = self.stack_trace_limit()? else {
            return Ok(None);
        };
        let mut end = self.vm.frames.len();
        if let Some(function) = skip_through {
            self.charge(end)?;
            end = self
                .vm
                .frames
                .iter()
                .rposition(|frame| frame.function == Some(function))
                .unwrap_or(0);
        }
        let take = limit.min(end);
        let frames: Box<[StackFrame]> = self
            .vm
            .frames
            .get(end - take..end)
            .unwrap_or_default()
            .iter()
            .rev()
            .map(|frame| StackFrame {
                function: frame.function,
                code: frame.code,
                compiled: Rc::clone(&frame.compiled),
                offset: (frame.pc as usize)
                    .checked_sub(1)
                    .and_then(|pc| frame.compiled.source_offset(pc))
                    .or_else(|| frame.compiled.source_offset(0)),
                this: receiver_of(frame, &self.vm.stack),
                construct: frame.construct,
            })
            .collect();
        self.charge(frames.len())?;
        let data = Box::new(ErrorData {
            frames,
            stack: Value::Empty,
            building: false,
        });
        // The object kind is set after the object exists, so the heap has
        // not counted these bytes yet.
        self.heap.charge(data.heap_size());
        Ok(Some(data))
    }

    /// Gives a new error object (one without properties) its `stack`: the
    /// captured frames in the object kind and the accessor property
    /// (defined first, before `message`, as in Chromium). Without the
    /// accessor functions of the realm (during the setup), the object has
    /// no `stack`.
    pub(crate) fn install_stack(
        &mut self,
        object: Gc<Object>,
        skip_through: Option<Gc<Object>>,
    ) -> VmResult<()> {
        let realm = self.current_realm();
        let (Ok(getter), Ok(setter)) = (
            self.intrinsic(realm, Intrinsic::StackGetter),
            self.intrinsic(realm, Intrinsic::StackSetter),
        ) else {
            return Ok(());
        };
        let data = self.capture_stack(skip_through)?;
        if let ObjectKind::Error(slot) = &mut self.heap.object_mut(object)?.kind {
            *slot = data;
        }
        // Room for `stack` (two slots) and `message` and `cause`, so that
        // the constructor grows the slots once.
        self.heap
            .object_mut(object)?
            .slots
            .try_reserve_exact(4)
            .map_err(crate::error::Error::from)?;
        self.heap.charge(4 * size_of::<Value>());
        // The object is new and has no property yet: add the accessor
        // without the checks of `[[DefineOwnProperty]]`.
        let key = PropertyKey::String(self.vm.atoms.stack);
        let accessor = Property::Accessor {
            get: getter.into(),
            set: setter.into(),
            enumerable: false,
            configurable: true,
        };
        self.heap.add_named(object, key, accessor, &self.vm)?;
        Ok(())
    }

    /// The first object on the prototype chain of `start` (itself
    /// included) that is an error object, to read or write its stack.
    fn error_holder(&self, start: Value) -> VmResult<Option<Gc<Object>>> {
        let Value::Object(mut current) = start else {
            return Ok(None);
        };
        for _ in 0..HOLDER_CHAIN_LIMIT {
            if matches!(self.heap.object(current)?.kind, ObjectKind::Error(_)) {
                return Ok(Some(current));
            }
            match self.heap.get_prototype_of(current)? {
                Some(next) => current = next,
                None => break,
            }
        }
        Ok(None)
    }

    /// The text of the stack of an error object, built on the first read.
    fn error_stack_value(&mut self, holder: Gc<Object>) -> VmResult<Value> {
        let state = match &self.heap.object(holder)?.kind {
            ObjectKind::Error(Some(data)) => Some((data.stack, data.building)),
            _ => None,
        };
        let Some((stack, building)) = state else {
            return Ok(Value::Undefined);
        };
        if !matches!(stack, Value::Empty) {
            return Ok(stack);
        }
        if building {
            return Ok(Value::Undefined);
        }
        self.set_building(holder, true)?;
        let text = self.build_stack_text(holder);
        self.set_building(holder, false)?;
        let text = Value::String(text?);
        // A script that assigned `stack` while the text was built (in a
        // getter of `name` or `message`) keeps its value.
        if let ObjectKind::Error(Some(data)) = &self.heap.object(holder)?.kind
            && !matches!(data.stack, Value::Empty)
        {
            return Ok(data.stack);
        }
        self.store_stack(holder, text)?;
        Ok(text)
    }

    fn set_building(&mut self, holder: Gc<Object>, building: bool) -> VmResult<()> {
        if let ObjectKind::Error(Some(data)) = &mut self.heap.object_mut(holder)?.kind {
            data.building = building;
        }
        Ok(())
    }

    /// Stores the value of `stack` in the error object and drops the
    /// captured frames.
    fn store_stack(&mut self, holder: Gc<Object>, value: Value) -> VmResult<()> {
        let object = self.heap.object_mut(holder)?;
        let ObjectKind::Error(slot) = &mut object.kind else {
            return Ok(());
        };
        match slot {
            Some(data) => {
                data.stack = value;
                data.frames = Box::default();
            }
            None => {
                *slot = Some(Box::new(ErrorData {
                    frames: Box::default(),
                    stack: value,
                    building: false,
                }));
            }
        }
        Ok(())
    }
}

/// The getter of `stack`: the text of the first error object on the
/// prototype chain of `this`, `undefined` for any other receiver.
pub(crate) fn stack_getter(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let value = match rt.error_holder(call.this())? {
        Some(holder) => rt.error_stack_value(holder)?,
        None => Value::Undefined,
    };
    Ok(NativeReturn::Value(value))
}

/// The setter of `stack`: replaces the text; a receiver that is not an
/// error object is left alone.
pub(crate) fn stack_setter(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    if let Some(holder) = rt.error_holder(call.this())? {
        let value = rt.arg(call, 0);
        rt.store_stack(holder, value)?;
    }
    Ok(NativeReturn::Value(Value::Undefined))
}

/// `Error.captureStackTrace(object, constructorOpt)` (a V8 extension).
pub(crate) fn capture_stack_trace(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let Value::Object(object) = rt.arg(call, 0) else {
        // Chromium's message is the name of its message template.
        return Err(VmError::type_error("invalid_argument"));
    };
    let skip = match rt.arg(call, 1) {
        Value::Object(function) if rt.is_callable(function.into())? => Some(function),
        _ => None,
    };
    let data = rt.capture_stack(skip)?;
    if matches!(rt.heap.object(object)?.kind, ObjectKind::Error(_)) {
        rt.define_stack_accessor(object)?;
        if let ObjectKind::Error(slot) = &mut rt.heap.object_mut(object)?.kind {
            *slot = data;
        }
    } else {
        let text = rt.stack_text_of(object, data)?;
        let key = PropertyKey::String(rt.vm.atoms.stack);
        let desc = PropertyDescriptor::data(text, true, false, true);
        if !rt.define_own_property(object, key, desc)? {
            return Err(VmError::type_error(
                "Cannot define property stack, object is not extensible",
            ));
        }
    }
    Ok(NativeReturn::Value(Value::Undefined))
}

impl Runtime {
    /// Defines the `stack` accessor on an error object that lacks it
    /// (`Error.captureStackTrace` on an error whose `stack` was deleted).
    fn define_stack_accessor(&mut self, object: Gc<Object>) -> VmResult<()> {
        let realm = self.current_realm();
        let getter = self.intrinsic(realm, Intrinsic::StackGetter)?;
        let setter = self.intrinsic(realm, Intrinsic::StackSetter)?;
        let key = PropertyKey::String(self.vm.atoms.stack);
        let desc = PropertyDescriptor::accessor(getter.into(), setter.into(), false, true);
        if !self.heap.define_own_property(object, key, desc, &self.vm)? {
            return Err(VmError::type_error(
                "Cannot define property stack, object is not extensible",
            ));
        }
        Ok(())
    }
}
