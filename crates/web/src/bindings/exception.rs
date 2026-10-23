//! Exceptions of the bindings (Web IDL §3.14,
//! <https://webidl.spec.whatwg.org/#es-exceptions>) with Chromium's
//! messages (measured in `tools/probes/web-bindings.json`).
//!
//! The hand-written behaviour and the conversions return an [`Exception`]
//! with a bare message; the glue adds the context of the member
//! ([`Context`]): `Failed to execute 'appendChild' on 'Node': `,
//! `Failed to set the 'title' property on 'HTMLElement': `,
//! `Failed to construct 'Event': `, and in dictionaries
//! `Failed to read the 'x' property from 'Y': `.
//! A value that script code threw passes unchanged ([`Exception::Js`]).

use std::borrow::Cow;

use swb_js::{ClassId, Runtime, VmError};

/// An exception of a binding.
#[derive(Debug)]
pub enum Exception {
    /// A `TypeError`; the glue puts the context before the message.
    Type(Cow<'static, str>),
    /// A `RangeError`; the glue puts the context before the message.
    Range(Cow<'static, str>),
    /// A `DOMException` with one of the names of the error names table
    /// ([`super::dom_exception::NAMES`]); the glue puts the context before
    /// the message.
    Dom {
        /// The name (`"HierarchyRequestError"`).
        name: &'static str,
        /// The message.
        message: Cow<'static, str>,
    },
    /// A conversion found a value that is not of the type named here; the
    /// glue words it by the place of the value: "parameter 1 is not of
    /// type 'Node'." for an argument, "Failed to convert value to 'Node'."
    /// inside another value or for an attribute.
    NotOfType(Cow<'static, str>),
    /// An abrupt completion of script code or of the engine (a thrown
    /// value, a termination); it passes unchanged.
    Js(VmError),
}

impl From<VmError> for Exception {
    fn from(error: VmError) -> Exception {
        Exception::Js(error)
    }
}

impl Exception {
    /// A `TypeError` with `message`.
    pub fn type_error(message: impl Into<Cow<'static, str>>) -> Exception {
        Exception::Type(message.into())
    }

    /// A `RangeError` with `message`.
    pub fn range_error(message: impl Into<Cow<'static, str>>) -> Exception {
        Exception::Range(message.into())
    }

    /// A `DOMException` named `name` with `message`.
    pub fn dom(name: &'static str, message: impl Into<Cow<'static, str>>) -> Exception {
        Exception::Dom {
            name,
            message: message.into(),
        }
    }

    /// The exception for the argument at `index` (0-based): a value of the
    /// wrong type is "parameter N is not of type 'T'.".
    #[must_use]
    pub fn argument(self, index: usize) -> Exception {
        match self {
            Exception::NotOfType(ty) => {
                Exception::Type(format!("parameter {} is not of type '{ty}'.", index + 1).into())
            }
            other => other,
        }
    }

    /// The exception inside another value or for an attribute value: a
    /// value of the wrong type is "Failed to convert value to 'T'.".
    #[must_use]
    pub fn nested(self) -> Exception {
        match self {
            Exception::NotOfType(ty) => {
                Exception::Type(format!("Failed to convert value to '{ty}'.").into())
            }
            other => other,
        }
    }

    /// The exception of the dictionary member `member` of `dictionary`:
    /// "Failed to read the 'member' property from 'Dictionary': ...".
    #[must_use]
    pub fn member(self, member: &str, dictionary: &str) -> Exception {
        match self.nested() {
            Exception::Type(message) => Exception::Type(
                format!("Failed to read the '{member}' property from '{dictionary}': {message}")
                    .into(),
            ),
            Exception::Range(message) => Exception::Range(
                format!("Failed to read the '{member}' property from '{dictionary}': {message}")
                    .into(),
            ),
            other => other,
        }
    }
}

/// The member that an exception comes from: the prefix of its message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Context {
    /// An operation: interface and operation name.
    Operation(&'static str, &'static str),
    /// An attribute getter: interface and attribute name.
    Getter(&'static str, &'static str),
    /// An attribute setter: interface and attribute name.
    Setter(&'static str, &'static str),
    /// A constructor: the interface.
    Constructor(&'static str),
}

impl Context {
    /// Chromium's prefix of the messages of this member.
    pub fn prefix(self) -> String {
        match self {
            Context::Operation(interface, name) => {
                format!("Failed to execute '{name}' on '{interface}': ")
            }
            Context::Getter(interface, name) => {
                format!("Failed to read the '{name}' property from '{interface}': ")
            }
            Context::Setter(interface, name) => {
                format!("Failed to set the '{name}' property on '{interface}': ")
            }
            Context::Constructor(interface) => format!("Failed to construct '{interface}': "),
        }
    }
}

/// "N argument(s) required, but only M present." (Chromium's message for
/// too few arguments).
pub fn not_enough_arguments(required: usize, present: usize) -> Exception {
    let noun = if required == 1 {
        "argument"
    } else {
        "arguments"
    };
    Exception::Type(format!("{required} {noun} required, but only {present} present.").into())
}

/// Turns an exception into the error that the native function returns:
/// a `TypeError` or `RangeError` with the context's prefix, or a thrown
/// `DOMException` of `dom_exception` (the class of the interface in the
/// runtime) in `realm`.
pub fn throw(
    rt: &mut Runtime,
    realm: u32,
    context: Context,
    dom_exception: ClassId,
    exception: Exception,
) -> VmError {
    match exception {
        Exception::Type(message) => VmError::type_error(format!("{}{message}", context.prefix())),
        Exception::Range(message) => VmError::range_error(format!("{}{message}", context.prefix())),
        Exception::NotOfType(ty) => VmError::type_error(format!(
            "{}The provided value is not of type '{ty}'.",
            context.prefix()
        )),
        Exception::Dom { name, message } => {
            let message = format!("{}{message}", context.prefix());
            match super::dom_exception::create(rt, realm, dom_exception, name, &message) {
                Ok(object) => VmError::Throw(object.into()),
                Err(error) => error,
            }
        }
        Exception::Js(error) => error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_by_place() {
        let text = |e: Exception| match e {
            Exception::Type(m) | Exception::Range(m) => m.into_owned(),
            other => format!("{other:?}"),
        };
        assert_eq!(
            text(Exception::NotOfType("Node".into()).argument(0)),
            "parameter 1 is not of type 'Node'."
        );
        assert_eq!(
            text(Exception::NotOfType("object".into()).nested()),
            "Failed to convert value to 'object'."
        );
        assert_eq!(
            text(Exception::NotOfType("Window".into()).member("view", "UIEventInit")),
            "Failed to read the 'view' property from 'UIEventInit': Failed to convert value to 'Window'."
        );
        assert_eq!(
            text(not_enough_arguments(1, 0)),
            "1 argument required, but only 0 present."
        );
        assert_eq!(
            text(not_enough_arguments(2, 1)),
            "2 arguments required, but only 1 present."
        );
        assert_eq!(
            Context::Setter("HTMLElement", "title").prefix(),
            "Failed to set the 'title' property on 'HTMLElement': "
        );
    }
}
