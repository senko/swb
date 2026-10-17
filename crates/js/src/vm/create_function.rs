//! The `Function`, `GeneratorFunction`, `AsyncFunction` and
//! `AsyncGeneratorFunction` constructors
//! (ECMA-262 §20.2.1.1, §27.3.1.1, §27.7.1.1, §27.4.1.1,
//! <https://tc39.es/ecma262/#sec-createdynamicfunction>): the source text
//! of a normal, generator or async function from the arguments, its parse
//! and compile as global code of the constructor's realm, and the function
//! object.
//!
//! The source is `(function anonymous(P\n) {\nbody\n})` (with
//! `function*` for a generator, `async function` for an async function,
//! `async function*` for an async generator), as V8 builds it:
//! `Function.prototype.toString` shows the text between the parentheses
//! (§20.2.1.1.1 step 16 `sourceString`), and positions on the first line
//! count the `(` (measured in Node.js 22). The parse checks that the
//! parameters and the body do not reach into each other (see
//! `swb_js_syntax::parse_dynamic_function`). The parse and the compile are
//! charged like eval code: the heap must have room for the source and its
//! tree before the parse, each code unit costs time, and the recursion
//! budget bounds the nesting. The script around the function is garbage
//! once the function exists; the function's code owns the source.

use std::rc::Rc;

use swb_js_text::String16;

use crate::bytecode::FunctionCode;
use crate::compiler::{CompileError, compile_dynamic_function};
use crate::heap::{Gc, Generic};
use crate::object::Object;
use crate::runtime::Runtime;
use crate::string::JsString;
use crate::vm::function::ClosureParts;
use crate::vm::{NativeCall, VmError, VmResult};

/// The text before the parameters.
const PREFIX: &str = "(function anonymous(";
/// The text before the parameters of a generator function.
const GENERATOR_PREFIX: &str = "(function* anonymous(";
/// The text before the parameters of an async function.
const ASYNC_PREFIX: &str = "(async function anonymous(";
/// The text before the parameters of an async generator function.
const ASYNC_GENERATOR_PREFIX: &str = "(async function* anonymous(";

/// The kind of function that `CreateDynamicFunction` makes (§20.2.1.1.1
/// `kind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DynamicKind {
    Normal,
    Generator,
    Async,
    AsyncGenerator,
}
/// The text between the parameters and the body.
const MIDDLE: &str = "\n) {\n";
/// The text after the body.
const SUFFIX: &str = "\n})";

/// The source text being built, and the length that the heap has room
/// for.
struct Source {
    text: String16,
    room: usize,
}

impl Runtime {
    /// The code of the function that `Function(...args)`,
    /// `GeneratorFunction(...args)` or `AsyncFunction(...args)` creates
    /// (§20.2.1.1.1 steps 1 to 29):
    /// `ToString` of the parameter arguments, then of the body, the parse
    /// with its early errors (a `SyntaxError` at the call), the compile.
    /// The code object is recorded in the current handle scope.
    pub(crate) fn dynamic_function_code(
        &mut self,
        call: &NativeCall,
        kind: DynamicKind,
    ) -> VmResult<(Gc<Generic>, Rc<FunctionCode>)> {
        let argc = call.argc();
        let prefix = match kind {
            DynamicKind::Normal => PREFIX,
            DynamicKind::Generator => GENERATOR_PREFIX,
            DynamicKind::Async => ASYNC_PREFIX,
            DynamicKind::AsyncGenerator => ASYNC_GENERATOR_PREFIX,
        };
        let mut text = Source {
            text: String16::from(prefix),
            room: 0,
        };
        // Steps 9 to 11: the parameters, joined with commas.
        for index in 0..argc.saturating_sub(1) {
            // A time step per argument: `Function.apply` can pass millions
            // of empty strings.
            self.tick()?;
            if index > 0 {
                text.text.push(u16::from(b','));
            }
            let part = self.to_string(self.arg(call, index))?;
            self.append_part(&mut text, part)?;
        }
        let params_end = text.text.len() + 1;
        text.text.push_str16(String16::from(MIDDLE).as_str16());
        if argc > 0 {
            let body = self.to_string(self.arg(call, argc - 1))?;
            self.append_part(&mut text, body)?;
        }
        let mut text = text.text;
        text.push_str16(String16::from(SUFFIX).as_str16());
        // The length was checked against the source limit (below 2^31).
        let params_end = u32::try_from(params_end)
            .map_err(|_| VmError::invariant("a Function source past the length limit"))?;
        let script = self.parse_code(text.as_str16(), |source, budget| {
            swb_js_syntax::parse_dynamic_function(source, params_end, budget)
        })?;
        let origin = self.eval_origin();
        let (handle, compiled) = compile_dynamic_function(
            &mut self.heap,
            &script,
            (text.as_str16(), origin),
            &mut self.budget,
        )
        .map_err(|error| match error {
            CompileError::Script { kind, message, .. } => VmError::Raise { kind, message },
            CompileError::Heap(error) => error.into(),
        })?;
        self.heap.record(handle);
        if !compiled.captures.is_empty() {
            return Err(VmError::invariant(
                "the function of a Function constructor captures cells",
            ));
        }
        Ok((handle, compiled))
    }

    /// Appends the text of `part` to the source being built. The source
    /// is a buffer outside the heap that a script sizes, so the heap must
    /// have room for it first (checked when the room doubles, so that a
    /// list of many short parts does not collect for each); the copy costs
    /// time.
    fn append_part(&mut self, source: &mut Source, part: Gc<JsString>) -> VmResult<()> {
        let units = self.heap.string(part)?.len();
        // Two units more for the separators that follow.
        let total = source.text.len().saturating_add(units).saturating_add(2);
        swb_js_syntax::check_source_len(total).map_err(super::eval::parse_failure)?;
        if total > source.room {
            source.room = total.max(source.room.saturating_mul(2));
            self.heap.reserve(source.room.saturating_mul(2), &self.vm)?;
        }
        self.heap.push_string(&mut source.text, part)?;
        self.charge_units(units)
    }

    /// The function object of `Function(...)`, `GeneratorFunction(...)` or
    /// `AsyncFunction(...)` (§20.2.1.1.1 steps 30 to 36):
    /// `OrdinaryFunctionCreate` with the prototype `proto` (from
    /// `new.target`), in the realm of the constructor, with the name
    /// `anonymous` and, except for an async function, a `prototype`
    /// property (`MakeConstructor` for a normal function; for a generator
    /// an object that inherits from `%GeneratorPrototype%`, step 33).
    pub(crate) fn new_dynamic_function(
        &mut self,
        (code, compiled): (Gc<Generic>, &Rc<FunctionCode>),
        proto: Gc<Object>,
        realm: u32,
    ) -> VmResult<Gc<Object>> {
        let parts = ClosureParts {
            function_proto: Some(proto),
            home: None,
        };
        self.new_closure_with(code, compiled, Box::new([]), realm, parts)
    }
}

#[cfg(test)]
mod tests {
    use super::{ASYNC_GENERATOR_PREFIX, ASYNC_PREFIX, GENERATOR_PREFIX, MIDDLE, PREFIX, SUFFIX};

    #[test]
    fn the_source_has_the_form_of_v8() {
        let source = format!("{PREFIX}a,b{MIDDLE}return a{SUFFIX}");
        assert_eq!(source, "(function anonymous(a,b\n) {\nreturn a\n})");
        let source = format!("{GENERATOR_PREFIX}a,b{MIDDLE}yield a{SUFFIX}");
        assert_eq!(source, "(function* anonymous(a,b\n) {\nyield a\n})");
        let source = format!("{ASYNC_PREFIX}a{MIDDLE}await a{SUFFIX}");
        assert_eq!(source, "(async function anonymous(a\n) {\nawait a\n})");
        let source = format!("{ASYNC_GENERATOR_PREFIX}a{MIDDLE}yield a{SUFFIX}");
        assert_eq!(source, "(async function* anonymous(a\n) {\nyield a\n})");
    }
}
