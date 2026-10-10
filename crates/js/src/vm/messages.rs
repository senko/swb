//! The texts of error messages and value descriptions. No script code
//! runs here: getters and `toString` methods are never called. The
//! wording follows V8's (measured in Node.js 22).

use crate::heap::Gc;
use crate::object::{Object, ObjectKind, Property};
use crate::runtime::Runtime;
use crate::string::{JsString, PropertyKey};
use crate::value::Value;
use crate::vm::VmError;
use crate::vm::function::is_callable_kind;

/// The longest prototype chain that the text helpers follow.
const CHAIN_LIMIT: usize = 1000;

impl Runtime {
    /// The `TypeError` of a read from `undefined` or `null`.
    pub(crate) fn cannot_read(&self, base: Value, key: PropertyKey) -> VmError {
        let what = if base == Value::Null {
            "null"
        } else {
            "undefined"
        };
        VmError::type_error(format!(
            "Cannot read properties of {what} (reading '{}')",
            self.key_text(key)
        ))
    }

    /// The `TypeError` of a write to `undefined` or `null`.
    pub(crate) fn cannot_set(&self, base: Value, key: PropertyKey) -> VmError {
        let what = if base == Value::Null {
            "null"
        } else {
            "undefined"
        };
        VmError::type_error(format!(
            "Cannot set properties of {what} (setting '{}')",
            self.key_text(key)
        ))
    }

    /// The `TypeError` of a strict write to a primitive.
    pub(crate) fn cannot_create(&self, base: Value, key: PropertyKey) -> VmError {
        if let Value::String(string) = base {
            // An own property of the string (an index below its length, or
            // `length`) is read-only instead of missing.
            let own = self.heap.string(string).is_ok_and(|text| match key {
                PropertyKey::Index(index) => (index as usize) < text.len(),
                PropertyKey::String(name) => name == self.heap.length_atom(),
                PropertyKey::Symbol(_) => false,
            });
            if own {
                return VmError::type_error(format!(
                    "Cannot assign to read only property '{}' of string '{}'",
                    self.key_text(key),
                    self.name_text(string)
                ));
            }
        }
        let (kind, text) = match base {
            Value::String(string) => ("string", self.name_text(string)),
            Value::Int(_) | Value::Double(_) => ("number", self.primitive_text(base)),
            Value::Bool(_) => ("boolean", self.primitive_text(base)),
            _ => ("symbol", "Symbol()".to_owned()),
        };
        VmError::type_error(format!(
            "Cannot create property '{}' on {kind} '{text}'",
            self.key_text(key)
        ))
    }

    /// The `TypeError` of a failed strict write to an object: a read-only
    /// property on the chain, or a non-extensible object.
    pub(crate) fn failed_write(&self, target: Gc<Object>, key: PropertyKey) -> VmError {
        // A write to `length` of an array with a writable `length` fails
        // when ArraySetLength meets an element that cannot be deleted
        // (§10.4.2.4 step 17); V8 names that element, which is now the last.
        if key == PropertyKey::String(self.heap.length_atom())
            && let Ok((length, true)) = self.heap.array_length(target)
            && length > 0
        {
            return VmError::type_error(format!(
                "Cannot delete property '{}' of [object Array]",
                length - 1
            ));
        }
        let mut current = Some(target);
        while let Some(object) = current {
            // The characters of a String object are read-only.
            let character = matches!(self.heap.string_object_unit(object, key), Ok(Some(_)));
            let own = if character {
                Ok(Some(Property::Data {
                    value: Value::Undefined,
                    writable: false,
                    enumerable: true,
                    configurable: false,
                }))
            } else {
                self.heap.get_own_property(object, key)
            };
            match own {
                Ok(Some(Property::Accessor { .. })) => {
                    return VmError::type_error(format!(
                        "Cannot set property {} of {} which has only a getter",
                        self.key_text(key),
                        self.object_text(target)
                    ));
                }
                Ok(Some(_)) => {
                    let kind = if self.is_callable_object(target) {
                        "function"
                    } else {
                        "object"
                    };
                    let mut text = self.object_text(target);
                    if character && text == "#<Object>" {
                        // V8 words an inherited character of a String
                        // object this way.
                        "[object Object]".clone_into(&mut text);
                    }
                    return VmError::type_error(format!(
                        "Cannot assign to read only property '{}' of {kind} '{text}'",
                        self.key_text(key),
                    ));
                }
                _ => {}
            }
            current = self.heap.get_prototype_of(object).ok().flatten();
        }
        VmError::type_error(format!(
            "Cannot add property {}, object is not extensible",
            self.key_text(key)
        ))
    }

    /// The `ReferenceError` of a binding in its temporal dead zone.
    pub(crate) fn tdz_error(&self, name: Gc<JsString>) -> VmError {
        VmError::reference_error(format!(
            "Cannot access '{}' before initialization",
            self.name_text(name)
        ))
    }

    /// Whether the object is a function (for the "of function '...'" form
    /// of messages).
    fn is_callable_object(&self, object: Gc<Object>) -> bool {
        self.heap
            .object(object)
            .is_ok_and(|o| is_callable_kind(&o.kind))
    }

    /// The value of the first data property `name` on the prototype chain
    /// from `object` (the object itself included). `None` if the first
    /// property of that name is an accessor or no object has one; no
    /// getter runs.
    pub(crate) fn chain_data_value(&self, object: Gc<Object>, name: Gc<JsString>) -> Option<Value> {
        let key = PropertyKey::String(name);
        let mut current = Some(object);
        for _ in 0..CHAIN_LIMIT {
            let object = current?;
            match self.heap.get_own_property(object, key) {
                Ok(Some(Property::Data { value, .. })) => return Some(value),
                Ok(Some(Property::Accessor { .. })) => return None,
                _ => current = self.heap.get_prototype_of(object).ok().flatten(),
            }
        }
        None
    }

    /// An own data property that holds a string.
    pub(crate) fn own_data_string(&self, object: Gc<Object>, name: Gc<JsString>) -> Option<String> {
        match self
            .heap
            .get_own_property(object, PropertyKey::String(name))
        {
            Ok(Some(Property::Data {
                value: Value::String(text),
                ..
            })) => Some(self.name_text(text)),
            _ => None,
        }
    }

    /// A data property on the prototype chain that holds a string.
    pub(crate) fn chain_data_string(
        &self,
        object: Gc<Object>,
        name: Gc<JsString>,
    ) -> Option<String> {
        match self.chain_data_value(object, name)? {
            Value::String(text) => Some(self.name_text(text)),
            _ => None,
        }
    }

    /// The `name` of the function in the `constructor` data property found
    /// on the prototype chain from `object` (no getter runs).
    pub(crate) fn chain_constructor_name(&self, object: Gc<Object>) -> Option<String> {
        match self.chain_data_value(object, self.vm.atoms.constructor)? {
            Value::Object(constructor) => self.own_data_string(constructor, self.vm.atoms.name),
            _ => None,
        }
    }

    /// The text of a property key.
    pub(crate) fn key_text(&self, key: PropertyKey) -> String {
        match key {
            PropertyKey::Index(index) => index.to_string(),
            PropertyKey::String(name) => self.name_text(name),
            PropertyKey::Symbol(symbol) => {
                let description = self
                    .heap
                    .symbol(symbol)
                    .ok()
                    .and_then(|s| s.description)
                    .map(|d| self.name_text(d))
                    .unwrap_or_default();
                format!("Symbol({description})")
            }
        }
    }

    /// The text of an object in messages (V8 shows the constructor name;
    /// the global object as Node.js shows it).
    pub(crate) fn object_text(&self, object: Gc<Object>) -> String {
        if self.vm.realms.iter().any(|realm| realm.global == object) {
            return "[object Object]".to_owned();
        }
        match self.heap.object(object).map(|o| &o.kind) {
            Ok(ObjectKind::Array { .. }) => "[object Array]".to_owned(),
            Ok(ObjectKind::StringWrapper(_)) => "[object String]".to_owned(),
            Ok(ObjectKind::Function(closure)) => closure
                .compiled
                .source_text()
                .map_or_else(|| "#<Object>".to_owned(), |text| shorten_source(&text)),
            Ok(ObjectKind::Native(_)) => {
                let name = self
                    .own_data_string(object, self.vm.atoms.name)
                    .unwrap_or_default();
                format!("function {name}() {{ [native code] }}")
            }
            _ => "#<Object>".to_owned(),
        }
    }

    /// The text of a primitive value without running script code.
    pub(crate) fn primitive_text(&self, value: Value) -> String {
        match value {
            Value::Undefined => "undefined".to_owned(),
            Value::Null => "null".to_owned(),
            Value::Bool(b) => b.to_string(),
            Value::Int(i) => i.to_string(),
            Value::Double(d) => super::number::number_to_string(d),
            Value::String(s) => self.name_text(s),
            Value::Symbol(s) => self.key_text(PropertyKey::Symbol(s)),
            Value::BigInt(_) => "BigInt".to_owned(),
            Value::Object(o) => self.object_text(o),
            Value::Empty | Value::Cell(_) => "(internal)".to_owned(),
        }
    }

    /// A short description of a value for messages.
    pub(crate) fn describe_value(&self, value: Value) -> String {
        match value {
            Value::Object(object) => match self.heap.object(object).map(|o| &o.kind) {
                Ok(kind) if is_callable_kind(kind) => "function".to_owned(),
                _ => self.object_text(object),
            },
            _ => self.primitive_text(value),
        }
    }
}

/// The source of a function as V8 shows it in messages: at most 128 code
/// units; a longer text keeps its first 111 units, "...<omitted>..." and
/// its last 2 (measured in Node.js 22).
fn shorten_source(text: &swb_js_text::String16) -> String {
    const LIMIT: usize = 128;
    const TAIL: usize = 2;
    const OMITTED: &str = "...<omitted>...";
    let text = text.as_str16();
    if text.len() <= LIMIT {
        return text.to_string_lossy();
    }
    let head = LIMIT - OMITTED.len() - TAIL;
    let part = |start, end| {
        text.slice(start, end)
            .map(swb_js_text::Str16::to_string_lossy)
            .unwrap_or_default()
    };
    format!(
        "{}{OMITTED}{}",
        part(0, head),
        part(text.len() - TAIL, text.len())
    )
}
