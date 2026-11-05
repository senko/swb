//! The slots of a `WindowProxy` exotic object (HTML §7.2.3,
//! <https://html.spec.whatwg.org/multipage/nav-history-apis.html#the-windowproxy-exotic-object>;
//! ADR 0031 section 5): the host's frame id and an optional target, the
//! global object of a realm of this runtime.
//!
//! A `WindowProxy` stores no properties and no elements. Its shape is the
//! root shape of `null`, as for a proxy, and the heap's walks stop at it and
//! ask the runtime to take the generic path (`vm/window_proxy.rs`). The
//! inline caches have an entry of their own for it
//! ([`crate::object::SiteCache::WindowProperty`]): the entry names the
//! target, so that a change of the target misses.

use crate::error::{Error, Result};
use crate::heap::{Gc, Heap, Tracer};
use crate::object::{Object, ObjectKind};

/// The bit of [`WindowProxySlot::frame`] that says that the slot has a
/// target. (The slot must stay at 12 bytes: `ObjectKind` is 16 bytes.)
const HAS_TARGET: u32 = 1 << 31;

/// The internal slots of a `WindowProxy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowProxySlot {
    /// The frame id, with [`HAS_TARGET`].
    frame: u32,
    /// The index and the generation of the target.
    target: [u32; 2],
}

impl WindowProxySlot {
    /// The slots of a proxy of the frame `frame` (below 2^31).
    pub(crate) fn new(frame: u32, target: Option<Gc<Object>>) -> Result<WindowProxySlot> {
        if frame & HAS_TARGET != 0 {
            return Err(Error::invariant("a frame id of 2^31 or more"));
        }
        Ok(match target {
            Some(target) => WindowProxySlot {
                frame: frame | HAS_TARGET,
                target: [target.index(), target.generation()],
            },
            None => WindowProxySlot {
                frame,
                target: [0, 0],
            },
        })
    }

    /// The host's frame id.
    pub(crate) fn frame(self) -> u32 {
        self.frame & !HAS_TARGET
    }

    /// The target: `None` for a proxy whose internal methods call the
    /// host's hooks.
    #[inline]
    pub(crate) fn target(self) -> Option<Gc<Object>> {
        if self.frame & HAS_TARGET == 0 {
            return None;
        }
        let [index, generation] = self.target;
        Some(Gc::new(index, generation))
    }

    /// Reports the target to the tracer.
    pub(crate) fn trace(self, tracer: &mut Tracer<'_>) {
        if let Some(target) = self.target() {
            tracer.object(target);
        }
    }
}

impl Heap {
    /// The slots of `object` if it is a `WindowProxy`.
    #[inline]
    pub(crate) fn window_proxy(&self, object: Gc<Object>) -> Result<Option<WindowProxySlot>> {
        Ok(match self.object(object)?.kind {
            ObjectKind::WindowProxy(slot) => Some(slot),
            _ => None,
        })
    }

    /// Changes the target of a `WindowProxy`. The inline caches name the
    /// target, so their entries for the old one miss from now on.
    pub(crate) fn set_window_proxy_target(
        &mut self,
        proxy: Gc<Object>,
        target: Option<Gc<Object>>,
    ) -> Result<()> {
        match &mut self.object_mut(proxy)?.kind {
            ObjectKind::WindowProxy(slot) => {
                *slot = WindowProxySlot::new(slot.frame(), target)?;
                Ok(())
            }
            _ => Err(Error::invariant("a target of an object that is no proxy")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slot_keeps_its_frame_and_target() {
        let target = Gc::new(12, 34);
        let local = WindowProxySlot::new(7, Some(target)).unwrap();
        assert_eq!(local.frame(), 7);
        assert_eq!(local.target(), Some(target));
        let remote = WindowProxySlot::new(7, None).unwrap();
        assert_eq!(remote.frame(), 7);
        assert_eq!(remote.target(), None);
        assert!(WindowProxySlot::new(HAS_TARGET, None).is_err());
        assert_eq!(size_of::<WindowProxySlot>(), 12);
        assert_eq!(size_of::<ObjectKind>(), 16);
    }
}
