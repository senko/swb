//! The behaviour of `DOMException` (Web IDL §4.3,
//! <https://webidl.spec.whatwg.org/#idl-DOMException>): the constructor,
//! `name`, `message` and `code` with the error names table, and the
//! creation of the exceptions that bindings throw.
//!
//! The glue is generated from `crates/web/idl/webidl.idl` (the interface
//! object, the constants, the prototype whose `[[Prototype]]` is
//! `%Error.prototype%`); the functions here are its hand-written part. As
//! in Chromium (measured in `tools/probes/web-bindings.json`), a
//! `DOMException` is `instanceof Error`, `String(e)` is `"Name: message"`
//! through `Error.prototype.toString`, and a constructed one has no own
//! properties. Deviation: a `DOMException` that a binding throws has no
//! own `stack` (Chromium gives it one).

use swb_js::{ClassId, Gc, GenericData, HostData, Object, Runtime, Tracer, VmError, VmResult};

use super::{Cx, Host, Platform, Result};

/// The error names table (Web IDL §2.8.1) with the legacy codes; names
/// without a code have 0. `ValidationError` is not in the table any more
/// (its code is 0 in Chromium too).
pub const NAMES: [(&str, u16); 32] = [
    ("IndexSizeError", 1),
    ("HierarchyRequestError", 3),
    ("WrongDocumentError", 4),
    ("InvalidCharacterError", 5),
    ("NoModificationAllowedError", 7),
    ("NotFoundError", 8),
    ("NotSupportedError", 9),
    ("InUseAttributeError", 10),
    ("InvalidStateError", 11),
    ("SyntaxError", 12),
    ("InvalidModificationError", 13),
    ("NamespaceError", 14),
    ("InvalidAccessError", 15),
    ("TypeMismatchError", 17),
    ("SecurityError", 18),
    ("NetworkError", 19),
    ("AbortError", 20),
    ("URLMismatchError", 21),
    ("QuotaExceededError", 22),
    ("TimeoutError", 23),
    ("InvalidNodeTypeError", 24),
    ("DataCloneError", 25),
    ("EncodingError", 0),
    ("NotReadableError", 0),
    ("UnknownError", 0),
    ("ConstraintError", 0),
    ("DataError", 0),
    ("TransactionInactiveError", 0),
    ("ReadOnlyError", 0),
    ("VersionError", 0),
    ("OperationError", 0),
    ("NotAllowedError", 0),
];

/// The legacy code of an error name (0 for other names).
pub fn code_of(name: &str) -> u16 {
    NAMES
        .iter()
        .find(|(n, _)| *n == name)
        .map_or(0, |&(_, code)| code)
}

/// The host data of a `DOMException`: its name and message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomExceptionData {
    /// The name.
    pub name: String,
    /// The message.
    pub message: String,
}

impl GenericData for DomExceptionData {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        self.name.capacity() + self.message.capacity()
    }
}

/// A new `DOMException` of `class` in `realm`, as a binding throws it.
pub fn create(
    rt: &mut Runtime,
    realm: u32,
    class: ClassId,
    name: &str,
    message: &str,
) -> VmResult<Gc<Object>> {
    let data = rt.alloc_host_data(DomExceptionData {
        name: name.to_owned(),
        message: message.to_owned(),
    })?;
    rt.new_host_object(realm, class, data)
}

/// `new DOMException(message, name)`: the constructor steps.
pub fn construct<H: Host>(cx: &mut Cx<'_>, message: String, name: String) -> Result<Gc<Object>> {
    let data = cx.rt.alloc_host_data(DomExceptionData { name, message })?;
    Ok(cx.new_instance::<H>(data)?)
}

/// The data of `this`.
fn data(rt: &Runtime, this: Platform) -> Result<&DomExceptionData> {
    match this.data {
        HostData::Generic(generic) => rt
            .heap()
            .generic::<DomExceptionData>(generic)
            .map_err(|_| VmError::type_error("Illegal invocation").into()),
        HostData::Word(_) => Err(VmError::type_error("Illegal invocation").into()),
    }
}

/// The `name` getter.
pub fn name(cx: &mut Cx<'_>, this: Platform) -> Result<String> {
    Ok(data(cx.rt, this)?.name.clone())
}

/// The `message` getter.
pub fn message(cx: &mut Cx<'_>, this: Platform) -> Result<String> {
    Ok(data(cx.rt, this)?.message.clone())
}

/// The `code` getter: the legacy code of the name.
pub fn code(cx: &mut Cx<'_>, this: Platform) -> Result<u16> {
    Ok(code_of(&data(cx.rt, this)?.name))
}

/// The behaviour of the generated `DOMException` glue of the browser's
/// set, for every binding host.
impl<H: Host> crate::generated::dom_exception::DOMExceptionImpl for H {
    fn constructor(cx: &mut Cx<'_>, message: String, name: String) -> Result<Gc<Object>> {
        construct::<H>(cx, message, name)
    }

    fn name(cx: &mut Cx<'_>, this: Platform) -> Result<String> {
        name(cx, this)
    }

    fn message(cx: &mut Cx<'_>, this: Platform) -> Result<String> {
        message(cx, this)
    }

    fn code(cx: &mut Cx<'_>, this: Platform) -> Result<u16> {
        code(cx, this)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_of_names() {
        assert_eq!(code_of("SyntaxError"), 12);
        assert_eq!(code_of("QuotaExceededError"), 22);
        assert_eq!(code_of("EncodingError"), 0);
        assert_eq!(code_of("Error"), 0);
        assert_eq!(code_of("syntaxerror"), 0);
    }
}
