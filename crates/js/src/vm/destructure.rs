//! The run-time parts of destructuring, rest and spread: V8's messages
//! for an object pattern whose value is `undefined` or `null` (§7.2.1
//! `RequireObjectCoercible`), the array of a rest parameter (§10.2.11),
//! the object of an object rest element (§8.6.2, §13.15.5.4) and the
//! window of a call with spread arguments (§13.3.8.1).
//!
//! The compiled code builds the argument list of a spread call in an
//! array of its own; the call copies it onto the value stack. The list
//! is at most the value stack's limit (4M values by default). An
//! embedder can configure a stack of up to 2^30 values; the list still
//! stays below 2^27 values (V8's limit for argument lists, as for
//! `Function.prototype.apply`).

use std::collections::HashSet;

use swb_js_text::hash::BuildKeyedHasher;

use crate::bytecode::{COERCIBLE_PARAMETER, COERCIBLE_RENDERED, COERCIBLE_VALUE};
use crate::heap::Gc;
use crate::object::{INVALID_ARRAY_LENGTH, Property};
use crate::runtime::Runtime;
use crate::string::{JsString, PropertyKey};
use crate::value::Value;
use crate::vm::{Intrinsic, STACK_OVERFLOW, VmError, VmResult};

/// V8's limit of the length of an argument list. Only a value stack
/// larger than the default reaches it; the stack limit applies first.
const ARGUMENT_LIST_LIMIT: usize = 1 << 27;

impl Runtime {
    /// The `TypeError` of an object pattern whose value is `undefined` or
    /// `null`, in V8's words: `form` and `text` come from the compiler
    /// ([`crate::bytecode::Insn::RequireObjectCoercible`]).
    pub(crate) fn destructure_error(&self, value: Value, form: u16, text: Gc<JsString>) -> VmError {
        let text = self.name_text(text);
        let kind = if value == Value::Null {
            "null"
        } else {
            "undefined"
        };
        VmError::type_error(match form {
            COERCIBLE_RENDERED => format!("Cannot destructure {text} as it is {kind}."),
            COERCIBLE_VALUE => format!("Cannot destructure {text}'{kind}' as it is {kind}."),
            COERCIBLE_PARAMETER => format!(
                "Cannot destructure {text}'{}' as it is {kind}.",
                self.typed_text(value)
            ),
            _ => format!("Cannot read properties of {kind}{text}"),
        })
    }

    /// The array of a rest parameter: the arguments of the top frame from
    /// position `start` on. The arguments stay in their registers (one of
    /// them may be the destination) until the array is complete. A step
    /// of the time countdown per argument. Runs inside a handle scope.
    pub(crate) fn rest_arguments(&mut self, start: usize) -> VmResult<Value> {
        let frame = self
            .vm
            .frames
            .last()
            .ok_or(VmError::invariant("a rest parameter without a frame"))?;
        let (base, argc, params, realm) = (
            frame.base as usize,
            frame.argc as usize,
            frame.compiled.param_count as usize,
            frame.realm,
        );
        let count = argc.saturating_sub(start);
        self.charge(count)?;
        let proto = self.intrinsic(realm, Intrinsic::ArrayPrototype)?;
        let array = self.heap.new_array(Some(proto), 0)?;
        self.heap.record(array);
        for (position, index) in (start..argc).enumerate() {
            let value = if index < params {
                self.stack_value(base + index)?
            } else {
                self.vm
                    .frames
                    .last()
                    .and_then(|f| f.extra_args.get(index - params))
                    .copied()
                    .unwrap_or_default()
            };
            let key = PropertyKey::Index(position as u32);
            self.heap
                .create_data_property(array, key, value, &self.vm)?;
        }
        Ok(array.into())
    }

    /// The object of an object rest element: a new ordinary object with
    /// the own enumerable properties of `source` whose keys are not in
    /// the array `excluded` (the keys of the pattern's other properties,
    /// as values of `ToKey`). Runs inside a handle scope.
    pub(crate) fn copy_rest(&mut self, source: Value, excluded: Value) -> VmResult<Value> {
        let Value::Object(list) = excluded else {
            return Err(VmError::invariant(
                "the excluded keys of a rest are not an array",
            ));
        };
        let (count, _) = self.heap.array_length(list)?;
        self.charge(count as usize)?;
        let mut keys: HashSet<PropertyKey, BuildKeyedHasher> = HashSet::default();
        for index in 0..count {
            let key = data_value(
                self.heap
                    .get_own_property(list, PropertyKey::Index(index))?,
            );
            let key = self.key_from_converted(key)?;
            // The key can be a new atom: the copy below can collect.
            self.heap.record_keys(&[key]);
            keys.insert(key);
        }
        let realm = self.current_realm();
        let proto = self.intrinsic(realm, Intrinsic::ObjectPrototype)?;
        let target = self.heap.new_object(Some(proto))?;
        self.heap.record(target);
        self.copy_data_properties(target, source, &keys)?;
        Ok(target.into())
    }

    /// Pushes the window of a call with spread arguments on top of the
    /// value stack: `callee`, `this` and the elements of the argument
    /// array `list`. Returns the index of the first argument and the
    /// number of arguments.
    pub(crate) fn push_spread_window(
        &mut self,
        callee: Value,
        this: Value,
        list: Value,
    ) -> VmResult<(usize, usize)> {
        let Value::Object(list) = list else {
            return Err(VmError::invariant(
                "the arguments of a spread call are not an array",
            ));
        };
        let (length, _) = self.heap.array_length(list)?;
        let count = length as usize;
        if count >= ARGUMENT_LIST_LIMIT {
            return Err(VmError::range_error(INVALID_ARRAY_LENGTH));
        }
        if self.vm.stack.len() + 2 + count > self.vm.stack_limit {
            return Err(VmError::range_error(STACK_OVERFLOW));
        }
        self.charge(count)?;
        let window = self.push_window(callee, this, &[])?;
        let Self { heap, vm, .. } = self;
        match heap.dense_elements(list)? {
            Some(values) if values.len() >= count => {
                let values = values.get(..count).unwrap_or_default();
                vm.stack.extend(
                    values
                        .iter()
                        .map(|&v| if v.is_empty() { Value::Undefined } else { v }),
                );
            }
            _ => {
                for index in 0..length {
                    let property = heap.get_own_property(list, PropertyKey::Index(index))?;
                    vm.stack.push(data_value(property));
                }
            }
        }
        Ok((window, count))
    }
}

/// The value of an element of an array that compiled code made (a data
/// property, or `undefined` if there is none).
fn data_value(property: Option<Property>) -> Value {
    match property {
        Some(Property::Data { value, .. }) => value,
        _ => Value::Undefined,
    }
}
