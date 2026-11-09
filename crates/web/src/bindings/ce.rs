//! `[CEReactions]` (HTML §4.13.7.4,
//! <https://html.spec.whatwg.org/multipage/custom-elements.html#cereactions>;
//! ADR 0035 section 6): the generated glue of a member with the attribute
//! calls the hand-written member inside [`scope`].

use swb_js::Runtime;

use super::Result;

/// The bit of [`Runtime::host_flags`] that is set once the agent has a
/// custom element definition.
pub const DEFINITIONS_BIT: u8 = 1;

/// Runs `body` with an element queue pushed on the custom element reactions
/// stack, then pops the queue and invokes its reactions; the body's result
/// comes back after the reactions ran (an exception of the body too, as HTML
/// says).
///
/// While the agent has no definition, no element is custom and no upgrade
/// can be enqueued, so the scope is the call of `body` and one test of a
/// flag (ADR 0035 section 6, "Why the fast path is safe").
#[allow(
    clippy::inline_always,
    reason = "one test in the glue of every `[CEReactions]` member, ADR 0035 section 6"
)]
#[inline(always)]
pub fn scope<T>(rt: &mut Runtime, body: impl FnOnce(&mut Runtime) -> Result<T>) -> Result<T> {
    if rt.host_flags() & (1 << DEFINITIONS_BIT) == 0 {
        let result = body(rt);
        if rt.host_flags() & (1 << DEFINITIONS_BIT) != 0 {
            // The body ran script that made the agent's first definition
            // (an inline `<script>` inserted by the call).
            first_definition_in_body(rt)?;
        }
        return result;
    }
    scope_with_reactions(rt, body)
}

/// The end of a fast-path [`scope`] whose body made the agent's first
/// definition: the backup element queue was empty at the entry (nothing
/// could be enqueued), so its reactions are the ones that the rest of the
/// body enqueued, which HTML's element queue of this scope would hold. They
/// run now and not at the next microtask checkpoint.
#[cold]
#[inline(never)]
fn first_definition_in_body(rt: &mut Runtime) -> Result<()> {
    Ok(crate::custom_elements::reactions::flush_backup(rt)?)
}

/// [`scope`] while the agent has definitions.
#[cold]
#[inline(never)]
fn scope_with_reactions<T>(
    rt: &mut Runtime,
    body: impl FnOnce(&mut Runtime) -> Result<T>,
) -> Result<T> {
    crate::custom_elements::reactions::push_queue(rt);
    let result = body(rt);
    crate::custom_elements::reactions::pop_and_invoke(rt)?;
    result
}

#[cfg(test)]
mod tests {
    use swb_dom::NodeId;

    use super::*;
    use crate::clock::{ClockMode, PageClock};
    use crate::custom_elements::Callback;
    use crate::custom_elements::reactions::{Args, Reaction, enqueue};
    use crate::runtime::ScriptRuntime;
    use crate::state::state;

    fn backup_len(rt: &Runtime) -> usize {
        state(rt).unwrap().custom.reactions.borrow().backup_len()
    }

    /// The body of a fast-path scope makes the agent's first definition (an
    /// inline script that a DOM call inserted) and then enqueues a
    /// reaction with the reactions stack empty: the scope invokes the backup
    /// queue before it returns, not the next microtask checkpoint.
    #[test]
    fn a_fast_path_scope_invokes_the_backup_queue_after_the_first_definition() {
        let mut runtime = ScriptRuntime::new(PageClock::new(ClockMode::Virtual, None)).unwrap();
        let rt = &mut runtime.rt;
        let handles = rt.heap_mut().open_scope();
        assert_eq!(rt.host_flags() & (1 << DEFINITIONS_BIT), 0);
        let place = (0, NodeId::from_index(1).unwrap());
        let mut during = 0;
        let _ = scope(rt, |rt| {
            rt.set_host_flag_bit(DEFINITIONS_BIT, true);
            enqueue(
                rt,
                place,
                Reaction::Callback(Callback::Connected, Args::None),
            )?;
            during = backup_len(rt);
            Ok(())
        });
        assert_eq!(during, 1);
        assert_eq!(backup_len(rt), 0);
        rt.heap_mut().close_scope(handles).unwrap();
    }
}
