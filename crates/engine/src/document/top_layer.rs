//! The engine's host of the top-layer algorithms of `swb_dom` (ADR 0037
//! section 2), for the steps that no script runs: in a document without
//! scripts no listener can see an event, so the events are not fired (none
//! is canceled) and a `toggle` task only clears its state. Invokers with
//! JavaScript off (sub-feature 6c) use it; the drain runs the attribute
//! change steps of `popover` that the bindings did not run with it.

use swb_dom::{Document, NodeId, PopoverError, ToggleState, TopLayerHost};

use super::DocumentState;

/// The host of a document without scripts.
pub(crate) struct NoScriptHost<'a>(pub(crate) &'a mut Document);

impl TopLayerHost for NoScriptHost<'_> {
    fn doc(&mut self) -> &mut Document {
        self.0
    }

    fn fire_beforetoggle(
        &mut self,
        _element: NodeId,
        _old: ToggleState,
        _new: ToggleState,
        _cancelable: bool,
        _source: Option<NodeId>,
    ) -> Option<bool> {
        Some(true)
    }

    fn queue_toggle_task(&mut self, element: NodeId, serial: u64) {
        // No listener can see the `toggle` event: the task is done at once.
        let _ = self.0.take_toggle(element, serial);
    }
}

impl DocumentState {
    /// Runs the attribute change steps of the showing popovers whose
    /// `popover` attribute changed state without the bindings (a document
    /// without scripts): they hide.
    pub(super) fn run_popover_attribute_steps(&mut self) {
        if let Err(error) = swb_dom::run_popover_attribute_steps(&mut NoScriptHost(&mut self.dom)) {
            log_error(error);
        }
    }
}

/// Logs an error of a top-layer algorithm of the engine at `debug!` (ADR
/// 0037 section 2).
pub(crate) fn log_error(error: PopoverError) {
    log::debug!("top layer: {error}");
}

#[cfg(test)]
mod tests {
    use swb_dom::{HideOptions, hide_popover, parse_html, show_popover, toggle_popover};

    use super::*;

    #[test]
    fn shows_and_hides_without_events() {
        let mut doc = parse_html("<div id=a popover>a</div><div id=b popover>b</div>");
        let a = doc.element_by_id("a").expect("a");
        let b = doc.element_by_id("b").expect("b");
        let mut host = NoScriptHost(&mut doc);
        show_popover(&mut host, a, true, None).expect("shows");
        assert!(host.0.is_popover_showing(a));
        // An auto popover closes the other.
        show_popover(&mut host, b, true, None).expect("shows");
        assert!(!host.0.is_popover_showing(a));
        assert_eq!(toggle_popover(&mut host, b, None, None), Ok(false));
        assert!(hide_popover(&mut host, b, HideOptions::default()).is_ok());
        // The toggle tasks were done at once: nothing is kept.
        assert!(!host.0.has_pending_toggle(a) && !host.0.has_pending_toggle(b));
        assert_eq!(host.0.top_layer().expect("state").layer().len(), 0);
        assert_eq!(host.0.showing_popover_lists(), (&[][..], &[][..]));
    }

    #[test]
    fn attribute_steps_that_no_script_ran() {
        let mut doc = parse_html("<div id=a popover>a</div>");
        let a = doc.element_by_id("a").expect("a");
        show_popover(&mut NoScriptHost(&mut doc), a, true, None).expect("shows");
        doc.set_attr(a, "popover", "manual").expect("set");
        assert!(doc.is_popover_showing(a));
        swb_dom::run_popover_attribute_steps(&mut NoScriptHost(&mut doc)).expect("runs");
        assert!(!doc.is_popover_showing(a));
    }
}
