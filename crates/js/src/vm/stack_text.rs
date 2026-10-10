//! The text of `stack` in the format of Chromium (measured in Node.js
//! 22): the first line (`Name: message`, as `Error.prototype.toString`
//! makes it) and one line `    at ...` per captured frame.
//!
//! A frame line has the name of the function and the position of the
//! call or throw: `f (file:line:column)` for a function called without
//! receiver, `Type.f (file:line:column)` for a method call (with
//! `[as key]` if the function was found under another name),
//! `new F (...)` for a construct call, and `file:line:column` for script
//! code and for functions without a name. No script code runs while the
//! frames are formatted: names and the receiver's type come from data
//! properties. The scan for the method name looks at a bounded number of
//! properties.
//!
//! Deviations from Chromium: no lines for built-in functions
//! (`at Array.forEach (<anonymous>)`), eval code and `async` frames; the
//! name of a function that a V8 heuristic infers from an assignment to a
//! member (`o.p = function () {}`) is only the compiler's name.

use swb_js_text::{String16, unicode::is_identifier_char};

use crate::bytecode::{CodeKind, FunctionCode};
use crate::heap::Gc;
use crate::object::{Object, ObjectKind};
use crate::runtime::Runtime;
use crate::string::JsString;
use crate::value::Value;
use crate::vm::VmResult;
use crate::vm::stack::StackFrame;

/// The most properties that the search for a method name examines per
/// frame.
const METHOD_SCAN_BUDGET: usize = 256;

/// The most prototypes that the type name and the method name look at.
const PROTOTYPE_LIMIT: usize = 16;

impl Runtime {
    /// Builds the text of the stack of the error object `holder` from its
    /// captured frames. Runs script code only for the first line.
    pub(crate) fn build_stack_text(&mut self, holder: Gc<Object>) -> VmResult<Gc<JsString>> {
        let header = crate::builtins::error_header(self, holder.into())?;
        let mut text = self.heap.string(header)?.as_str16().to_string16();
        if let ObjectKind::Error(Some(data)) = &self.heap.object(holder)?.kind {
            self.append_frames(&mut text, &data.frames);
        }
        let units = text.len();
        let string = self.heap.alloc_string(text)?;
        self.charge_units(units)?;
        Ok(string)
    }

    /// Appends a line `\n    at ...` for each frame.
    fn append_frames(&self, text: &mut String16, frames: &[StackFrame]) {
        for frame in frames {
            text.push_str16(String16::from("\n    at ").as_str16());
            text.push_str16(String16::from(self.frame_line(frame).as_str()).as_str16());
        }
    }

    /// The `stack` value for `Error.captureStackTrace` on an object that
    /// is not an error object: the text at once, or `undefined` when
    /// `Error.stackTraceLimit` is not a number.
    pub(crate) fn stack_text_of(
        &mut self,
        object: Gc<Object>,
        data: Option<Box<crate::vm::ErrorData>>,
    ) -> VmResult<Value> {
        let Some(data) = data else {
            return Ok(Value::Undefined);
        };
        let header = crate::builtins::error_header(self, object.into())?;
        let mut text = self.heap.string(header)?.as_str16().to_string16();
        self.append_frames(&mut text, &data.frames);
        let units = text.len();
        let string = self.heap.alloc_string(text)?;
        self.charge_units(units)?;
        Ok(string.into())
    }

    /// One frame line, without the leading `    at `.
    fn frame_line(&self, frame: &StackFrame) -> String {
        let location = frame_location(&frame.compiled, frame.offset);
        let Some(function) = frame.function else {
            return location;
        };
        let name = self.debug_name(function, &frame.compiled);
        if frame.construct {
            let name = if name.is_empty() {
                "<anonymous>"
            } else {
                &name
            };
            return format!("new {name} ({location})");
        }
        let described = if self.is_toplevel_receiver(frame.this) {
            name
        } else {
            self.method_call_name(frame.this, function, &name)
        };
        if described.is_empty() {
            location
        } else {
            format!("{described} ({location})")
        }
    }

    /// The name of a function in stack traces: its own `name` property if
    /// that is a non-empty string, else the name in its code.
    fn debug_name(&self, function: Gc<Object>, code: &FunctionCode) -> String {
        match self.own_data_string(function, self.vm.atoms.name) {
            Some(name) if !name.is_empty() => name,
            _ => self.code_name(code),
        }
    }

    /// The name in the code of a function (the `name` property does not
    /// count), else its debug name.
    fn code_name(&self, code: &FunctionCode) -> String {
        let name = self.clipped_text(code.name);
        if name.is_empty() {
            self.clipped_text(code.debug_name)
        } else {
            name
        }
    }

    /// The name of the `constructor` function found on the prototype chain
    /// from `object`, as Chromium shows it in a frame: the name that the
    /// function was created with, even when a script redefined its `name`
    /// property (an accessor too). No getter runs.
    fn chain_constructor_internal_name(&self, object: Gc<Object>) -> Option<String> {
        let Value::Object(constructor) =
            self.chain_data_value(object, self.vm.atoms.constructor)?
        else {
            return None;
        };
        match &self.heap.object(constructor).ok()?.kind {
            ObjectKind::Function(closure) => Some(self.code_name(&closure.compiled)),
            ObjectKind::Native(native) => native.name.map(|name| self.clipped_text(name)),
            _ => self.own_data_string(constructor, self.vm.atoms.name),
        }
    }

    /// Whether the receiver makes the call a plain function call: no
    /// receiver, or the global object.
    fn is_toplevel_receiver(&self, this: Value) -> bool {
        match this {
            Value::Undefined | Value::Null | Value::Empty => true,
            Value::Object(object) => self.vm.realms.iter().any(|realm| realm.global == object),
            _ => false,
        }
    }

    /// `Type.name [as key]` for a call with a receiver.
    fn method_call_name(&self, this: Value, function: Gc<Object>, name: &str) -> String {
        let type_name = self.receiver_type_name(this);
        let method = self.method_name(this, function, name);
        if name.is_empty() {
            let method = method.as_deref().unwrap_or("<anonymous>");
            return format!("{type_name}.{method}");
        }
        let mut out = String::new();
        if is_identifier(name) && !type_name.is_empty() && !name.starts_with(&type_name) {
            out.push_str(&type_name);
            out.push('.');
        }
        out.push_str(name);
        if let Some(method) = method
            && !method.is_empty()
            && !names_method(name, &method)
        {
            out.push_str(" [as ");
            out.push_str(&method);
            out.push(']');
        }
        out
    }

    /// The name that Chromium shows for the type of a receiver: the name
    /// of the class of a class constructor, `Function` for other
    /// functions, else the `constructor` found on the prototype chain
    /// (the receiver's own properties do not count), else `Object`.
    fn receiver_type_name(&self, this: Value) -> String {
        let object = match this {
            Value::Bool(_) => return "Boolean".to_owned(),
            Value::Int(_) | Value::Double(_) => return "Number".to_owned(),
            Value::String(_) => return "String".to_owned(),
            Value::Symbol(_) => return "Symbol".to_owned(),
            Value::BigInt(_) => return "BigInt".to_owned(),
            Value::Object(object) => object,
            _ => return "Object".to_owned(),
        };
        let Ok(record) = self.heap.object(object) else {
            return "Object".to_owned();
        };
        match &record.kind {
            ObjectKind::Function(closure) if is_class(&closure.compiled) => self
                .own_data_string(object, self.vm.atoms.name)
                .unwrap_or_else(|| "Function".to_owned()),
            ObjectKind::Function(_) | ObjectKind::Native(_) | ObjectKind::Bound(_) => {
                "Function".to_owned()
            }
            _ => self
                .heap
                .get_prototype_of(object)
                .ok()
                .flatten()
                .and_then(|proto| self.chain_constructor_internal_name(proto))
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| "Object".to_owned()),
        }
    }

    /// The key under which the receiver (or an object on its prototype
    /// chain) holds the function: `name` itself if it is one of the keys,
    /// else the one key that holds it, if there is exactly one.
    fn method_name(&self, this: Value, function: Gc<Object>, name: &str) -> Option<String> {
        let Value::Object(receiver) = this else {
            return None;
        };
        let mut budget = METHOD_SCAN_BUDGET;
        let mut found = Vec::new();
        let mut current = Some(receiver);
        for _ in 0..PROTOTYPE_LIMIT {
            let Some(object) = current else {
                break;
            };
            self.heap
                .keys_holding(object, function, &mut budget, &mut found)
                .ok()?;
            if budget == 0 {
                break;
            }
            current = self.heap.get_prototype_of(object).ok().flatten();
        }
        let texts: Vec<String> = found.iter().map(|&key| self.key_text(key)).collect();
        if !name.is_empty() && texts.iter().any(|text| text == name) {
            return Some(name.to_owned());
        }
        let first = texts.first()?;
        texts
            .iter()
            .all(|text| text == first)
            .then(|| first.clone())
    }
}

/// Whether the name of a function already says the key `method` that it
/// was found under: the name is the key or ends with the key after a
/// `.` (`A.c`) or a space (`get c`).
fn names_method(name: &str, method: &str) -> bool {
    name == method
        || name
            .strip_suffix(method)
            .is_some_and(|rest| rest.ends_with('.') || rest.ends_with(' '))
}

/// Whether a function is a class constructor (its text starts with
/// `class`).
fn is_class(code: &FunctionCode) -> bool {
    if code.kind != CodeKind::Normal {
        return false;
    }
    let (start, _) = code.span;
    code.source
        .text
        .as_str16()
        .slice(start as usize, start as usize + 5)
        .is_some_and(|text| text.eq_str("class"))
}

/// `file:line:column` of an offset in a code's script.
fn frame_location(code: &FunctionCode, offset: Option<u32>) -> String {
    let name: &str = &code.source.name;
    let name = if name.is_empty() { "<anonymous>" } else { name };
    let name: String = name.chars().take(1_024).collect();
    match offset {
        Some(offset) => {
            let location = code.source.location(offset);
            format!("{name}:{}:{}", location.line, location.column)
        }
        None => name,
    }
}

/// Whether the whole text is an identifier (ASCII or Unicode, §12.7).
fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    is_identifier_char(u32::from(first), true)
        && chars.all(|c| is_identifier_char(u32::from(c), false))
}
