//! The data of a Proxy exotic object (ECMA-262 §10.5,
//! <https://tc39.es/ecma262/#sec-proxy-object-internal-methods-and-internal-slots>):
//! `[[ProxyTarget]]` and `[[ProxyHandler]]`, and whether the proxy has
//! `[[Call]]` and `[[Construct]]` (`ProxyCreate`, §10.5.15).
//!
//! The internal methods run script code (the traps), so the heap does not
//! answer them: its walks along a prototype chain stop at a proxy and ask
//! the runtime to take the generic path (`vm/proxy.rs`). A proxy stores
//! no properties and no elements of its own; its shape is the root shape
//! of `null`, so a heap method that reads it without the runtime sees an
//! empty object without a prototype, and the methods that would change it
//! refuse with an internal error.

use crate::error::{Error, Result};
use crate::heap::{Gc, Heap, Tracer};
use crate::object::{Object, ObjectKind};

/// The internal slots of a proxy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProxyData {
    /// `[[ProxyTarget]]`; `None` after revocation.
    pub(crate) target: Option<Gc<Object>>,
    /// `[[ProxyHandler]]`; `None` after revocation.
    pub(crate) handler: Option<Gc<Object>>,
    /// Whether the proxy has `[[Call]]` (its target was callable when the
    /// proxy was made).
    pub(crate) callable: bool,
    /// Whether the proxy has `[[Construct]]`.
    pub(crate) constructor: bool,
}

impl ProxyData {
    /// Reports the target and the handler to the tracer.
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        if let Some(target) = self.target {
            tracer.object(target);
        }
        if let Some(handler) = self.handler {
            tracer.object(handler);
        }
    }
}

impl Heap {
    /// The slots of `object` if it is a proxy.
    pub(crate) fn proxy_data(&self, object: Gc<Object>) -> Result<Option<ProxyData>> {
        Ok(match &self.object(object)?.kind {
            ObjectKind::Proxy(data) => Some(**data),
            _ => None,
        })
    }

    /// Whether `object` is a proxy.
    #[inline]
    pub(crate) fn is_proxy(&self, object: Gc<Object>) -> Result<bool> {
        Ok(matches!(self.object(object)?.kind, ObjectKind::Proxy(_)))
    }

    /// Revokes a proxy (§28.2.2.1.1 steps 5 and 6): both slots become
    /// null. A revoked proxy keeps `[[Call]]` and `[[Construct]]`.
    pub(crate) fn revoke_proxy(&mut self, proxy: Gc<Object>) -> Result<()> {
        match &mut self.object_mut(proxy)?.kind {
            ObjectKind::Proxy(data) => {
                data.target = None;
                data.handler = None;
                Ok(())
            }
            _ => Err(Error::invariant(
                "a revoke of an object that is not a proxy",
            )),
        }
    }
}
