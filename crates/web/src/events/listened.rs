//! Which event types can have a listener: the answer that lets an event
//! that the engine fires skip its path (ADR 0036 "5d implementation notes",
//! review fixes).
//!
//! A path through thousands of shadow trees costs a struct per node, and an
//! engine event (a `slotchange`, a `mousemove`) that no listener of its type
//! can see needs none of them. [`Listened`] keeps the event types for which
//! a listener or an event handler was ever added, on any target of the
//! runtime (`addEventListener`, the IDL handler setters, the compile of a
//! content attribute); the content attributes that no script has read yet
//! are in the arena (`swb_dom::Document::ever_had_on_attribute`). The answer
//! is an over-approximation: a type stays after its last listener goes, and
//! a runtime that adds more than [`MAX_KINDS`] types says yes to all.

use std::collections::HashSet;

use swb_js::{Runtime, VmResult};

use super::handler_names::NAMES;
use crate::state::{dom, state};

/// The most types that the set holds; a page that adds listeners of more
/// kinds (and removes them again) gets the slow path for every event.
const MAX_KINDS: usize = 4096;

/// The event types with a listener or a handler, ever.
#[derive(Debug, Default)]
pub(crate) struct Listened {
    kinds: HashSet<String>,
    /// True once the set was full.
    overflow: bool,
}

impl Listened {
    /// Notes that a listener or a handler of `kind` was added.
    pub(crate) fn note(&mut self, kind: &str) {
        if self.overflow || self.kinds.contains(kind) {
            return;
        }
        if self.kinds.len() >= MAX_KINDS {
            self.overflow = true;
            self.kinds = HashSet::new();
            return;
        }
        self.kinds.insert(kind.to_owned());
    }

    /// False only if no listener of `kind` was ever added.
    fn may_have(&self, kind: &str) -> bool {
        self.overflow || self.kinds.contains(kind)
    }
}

/// True if an event of type `kind` at a node of the current document, or at
/// its window, can reach a listener or an event handler: one was added by a
/// script, or an element of the document ever had the content attribute of
/// a handler of this type. `false` is exact; `true` may be a stale answer.
pub(crate) fn may_have_listener(rt: &Runtime, kind: &str) -> VmResult<bool> {
    if state(rt)?.listened.may_have(kind) {
        return Ok(true);
    }
    let dom = dom(rt)?;
    Ok(NAMES
        .iter()
        .any(|name| name.event == kind && dom.ever_had_on_attribute(name.attr)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_set_answers_yes_for_everything() {
        let mut listened = Listened::default();
        assert!(!listened.may_have("click"));
        listened.note("click");
        assert!(listened.may_have("click"));
        assert!(!listened.may_have("focus"));
        for i in 0..=MAX_KINDS {
            listened.note(&format!("type{i}"));
        }
        assert!(listened.may_have("focus"));
    }
}
