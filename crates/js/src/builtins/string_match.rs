//! `String.prototype.match`, `matchAll` and `search` (ECMA-262
//! §22.1.3.13, §22.1.3.14, §22.1.3.22): they dispatch to the `@@match`,
//! `@@matchAll` and `@@search` method of an object argument (`GetMethod`,
//! as a deferred call; a primitive argument is not looked up, as for
//! `replace` and `split`), else they make a `RegExp` from the argument
//! (`RegExpCreate`) and invoke its method.

use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::{NativeCall, NativeReturn, VmError, VmResult, WellKnown};

/// The `TypeError` of a receiver that is `undefined` or `null`.
fn require_coercible(call: &NativeCall, method: &str) -> VmResult<Value> {
    let this = call.this();
    if this.is_nullish() {
        return Err(VmError::type_error(format!(
            "String.prototype.{method} called on null or undefined"
        )));
    }
    Ok(this)
}

impl Runtime {
    /// `GetMethod(value, @@which)` with V8's message for a value that is
    /// not callable.
    fn symbol_method_of(&mut self, value: Value, which: WellKnown) -> VmResult<Option<Value>> {
        let key = self.symbol_key(which);
        let method = self.get_value(value, key)?;
        if method.is_nullish() {
            return Ok(None);
        }
        if !self.is_callable(method)? {
            let name = crate::vm::WELL_KNOWN
                .iter()
                .find(|(known, _)| *known == which)
                .map_or("", |(_, name)| *name);
            return Err(VmError::type_error(format!(
                "'{}' returned for property 'Symbol(Symbol.{name})' of object '{}' is not a function",
                self.primitive_text(method),
                match value {
                    Value::Object(object) => self.object_text(object),
                    _ => self.primitive_text(value),
                }
            )));
        }
        Ok(Some(self.heap.record(method)))
    }

    /// The common steps of `match`, `matchAll` and `search`: the dispatch
    /// on an object argument, else `Invoke(RegExpCreate(regexp, flags),
    /// @@which, « S »)`.
    fn string_regexp_method(
        &mut self,
        call: &NativeCall,
        method: &str,
        which: WellKnown,
        flags: Option<&str>,
    ) -> VmResult<NativeReturn> {
        let this = require_coercible(call, method)?;
        let regexp = self.arg(call, 0);
        if matches!(regexp, Value::Object(_))
            && let Some(matcher) = self.symbol_method_of(regexp, which)?
        {
            return Ok(NativeReturn::Call {
                callee: matcher,
                this: regexp,
                args: vec![this],
            });
        }
        let string = self.to_string(this)?;
        self.heap.record(string);
        let flags = match flags {
            Some(text) => Value::String(self.heap.intern_str(text)?),
            None => Value::Undefined,
        };
        let rx = self.regexp_create(regexp, flags)?;
        let key = self.symbol_key(which);
        let callee = self.get_value(rx.into(), key)?;
        Ok(NativeReturn::Call {
            callee,
            this: rx.into(),
            args: vec![string.into()],
        })
    }
}

/// `String.prototype.match(regexp)` (§22.1.3.13).
pub(super) fn match_(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    rt.string_regexp_method(call, "match", WellKnown::Match, None)
}

/// `String.prototype.search(regexp)` (§22.1.3.22).
pub(super) fn search(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    rt.string_regexp_method(call, "search", WellKnown::Search, None)
}

/// `String.prototype.matchAll(regexp)` (§22.1.3.14): a regular expression
/// argument must have `g` in its flags.
pub(super) fn match_all(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    require_coercible(call, "matchAll")?;
    let regexp = rt.arg(call, 0);
    if matches!(regexp, Value::Object(_)) && rt.is_regexp(regexp)? {
        let key = crate::string::PropertyKey::String(rt.vm.atoms.flags);
        let flags = rt.get_value(regexp, key)?;
        if flags.is_nullish() {
            return Err(VmError::type_error(
                "The .flags property of the argument to String.prototype.matchAll cannot be null or undefined",
            ));
        }
        let flags = rt.to_string(flags)?;
        let global = rt
            .flat_text(flags)?
            .units()
            .any(|unit| unit == u16::from(b'g'));
        if !global {
            return Err(VmError::type_error(
                "String.prototype.matchAll called with a non-global RegExp argument",
            ));
        }
    }
    rt.string_regexp_method(call, "matchAll", WellKnown::MatchAll, Some("g"))
}
