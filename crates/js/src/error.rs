//! Errors of the engine core (ADR 0026 sections 5, 8 and 9).
//!
//! Three kinds of failure leave the heap and the object model:
//!
//! - [`Error::Internal`]: an engine bug, for example a stale handle. It
//!   ends the script; it is never a panic.
//! - [`Error::Terminated`]: the script must end (heap limit, out of
//!   memory, time limit, a request of the host). No JavaScript handler
//!   and no `finally` block sees it.
//! - [`Error::Throw`]: the caller must throw a new error object of the
//!   given kind. The object model cannot create error objects itself,
//!   because they need a realm.

/// The result type of the engine core.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// A failure in the heap or the object model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// An engine bug. The script ends and the host logs it.
    #[error("internal error: {0}")]
    Internal(#[from] InternalError),
    /// The script ends without running any handler (ADR 0026 section 5).
    #[error("script terminated: {0}")]
    Terminated(#[from] Termination),
    /// The caller throws a new error object of this kind.
    #[error("{kind:?}: {message}")]
    Throw {
        /// The constructor of the error object.
        kind: ThrowKind,
        /// The message of the error object.
        message: &'static str,
    },
}

/// An engine bug.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InternalError {
    /// A handle whose slot was freed (and maybe reused): a rooting bug.
    #[error("stale {0} handle")]
    StaleHandle(&'static str),
    /// An internal invariant does not hold.
    #[error("broken invariant: {0}")]
    Invariant(&'static str),
}

/// Why a script ends without a JavaScript exception.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Termination {
    /// The heap stays above the limit that the host set, also after a
    /// collection.
    #[error("the heap limit was exceeded")]
    HeapLimit,
    /// The system allocator refused a request, or an arena has no free
    /// slot index left.
    #[error("out of memory")]
    OutOfMemory,
    /// The deadline that the host set has passed (ADR 0026 section 9).
    #[error("the time limit was exceeded")]
    TimeLimit,
    /// The host asked to end the script (a termination request, for
    /// example from another thread).
    #[error("the host ended the script")]
    HostRequest,
}

/// The kind of error object that the caller throws for [`Error::Throw`]
/// (and that the VM creates for its own errors).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThrowKind {
    /// `Error`.
    Error,
    /// `TypeError`.
    TypeError,
    /// `RangeError`.
    RangeError,
    /// `ReferenceError`.
    ReferenceError,
    /// `SyntaxError`.
    SyntaxError,
    /// `EvalError`.
    EvalError,
    /// `URIError`.
    UriError,
}

impl ThrowKind {
    /// The name of the constructor.
    pub fn name(self) -> &'static str {
        match self {
            ThrowKind::Error => "Error",
            ThrowKind::TypeError => "TypeError",
            ThrowKind::RangeError => "RangeError",
            ThrowKind::ReferenceError => "ReferenceError",
            ThrowKind::SyntaxError => "SyntaxError",
            ThrowKind::EvalError => "EvalError",
            ThrowKind::UriError => "URIError",
        }
    }
}

impl Error {
    /// A broken internal invariant.
    pub(crate) const fn invariant(message: &'static str) -> Self {
        Error::Internal(InternalError::Invariant(message))
    }

    /// A `RangeError` for the caller to throw.
    pub(crate) const fn range_error(message: &'static str) -> Self {
        Error::Throw {
            kind: ThrowKind::RangeError,
            message,
        }
    }

    /// A `TypeError` for the caller to throw.
    pub(crate) const fn type_error(message: &'static str) -> Self {
        Error::Throw {
            kind: ThrowKind::TypeError,
            message,
        }
    }
}

impl From<std::collections::TryReserveError> for Error {
    fn from(_: std::collections::TryReserveError) -> Self {
        Error::Terminated(Termination::OutOfMemory)
    }
}
